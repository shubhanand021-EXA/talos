//! SQLite-backed vector memory: embeddings + brute-force cosine search.
//!
//! Entries live in the same [`evidence_store::EvidenceDb`] as the ledger
//! (new tables, no second database). Vectors are 384-dim L2-normalized
//! `f32` blobs produced by [`embedding_engine`]; search is a single filtered
//! `SELECT` followed by in-Rust cosine scoring. At <10k entries this answers
//! top-10 queries in well under 100ms with zero native dependencies.
//!
//! Every entry carries a content hash: re-indexing an unchanged source is a
//! no-op, which makes incremental indexing cheap and idempotent.

use sha2::{Digest, Sha256};
use talos_common::{now_iso, Result, TalosError};

/// Sources that can be embedded for semantic recall.
pub const SOURCE_TYPES: &[&str] = &["claim", "evidence", "task", "skill"];

fn bad(what: &'static str, reason: &str) -> TalosError {
    TalosError::InvalidId {
        what,
        reason: reason.to_owned(),
    }
}

fn storage(msg: String) -> TalosError {
    TalosError::Storage(msg)
}

/// Create the vector tables (embedded migrations cover fresh databases;
/// this keeps older test databases working without a full migrate).
pub async fn ensure_vector_schema(db: &evidence_store::EvidenceDb) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS vector_entries (
            id            TEXT PRIMARY KEY,
            source_type   TEXT NOT NULL,
            source_id     TEXT NOT NULL,
            task_id       TEXT,
            text          TEXT NOT NULL,
            embedding     BLOB NOT NULL,
            dims          INTEGER NOT NULL,
            content_hash  TEXT NOT NULL,
            verified      INTEGER NOT NULL DEFAULT 0,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .map_err(|err| storage(format!("vector schema: {err}")))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vector_source ON vector_entries (source_type)")
        .execute(db.pool())
        .await
        .map_err(|err| storage(format!("vector schema: {err}")))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vector_task ON vector_entries (task_id)")
        .execute(db.pool())
        .await
        .map_err(|err| storage(format!("vector schema: {err}")))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS memory_index_state (
            source_id    TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            indexed_at   TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .map_err(|err| storage(format!("vector schema: {err}")))?;
    Ok(())
}

/// Encode `f32` dims as little-endian bytes.
#[must_use]
pub fn encode_embedding(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Decode little-endian bytes back to `f32` dims (`None` on length mismatch).
#[must_use]
pub fn decode_embedding(bytes: &[u8], dims: usize) -> Option<Vec<f32>> {
    if bytes.len() != dims * 4 {
        return None;
    }
    let mut out = Vec::with_capacity(dims);
    for chunk in bytes.chunks(4) {
        let word: [u8; 4] = chunk.try_into().ok()?;
        out.push(f32::from_le_bytes(word));
    }
    Some(out)
}

/// Content hash for change detection (`sha256:` hex of the indexed text).
#[must_use]
pub fn content_hash(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

/// One entry to embed and store.
#[derive(Debug, Clone)]
pub struct NewVectorEntry {
    /// `claim` | `evidence` | `task` | `skill`.
    pub source_type: String,
    /// Source row id.
    pub source_id: String,
    /// Owning task, if any (filterable).
    pub task_id: Option<String>,
    /// Text that was embedded.
    pub text: String,
    /// Whether the source is verified (rerank boost).
    pub verified: bool,
    /// Extra metadata (JSON object).
    pub metadata: serde_json::Value,
}

/// Outcome of [`upsert_entry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertOutcome {
    /// Entry id (`{source_type}:{source_id}`).
    pub id: String,
    /// True when a new row (or new content) was written.
    pub written: bool,
}

/// Insert or refresh one entry. Unchanged content is a no-op (returns
/// `written: false` without re-embedding).
pub async fn upsert_entry(
    db: &evidence_store::EvidenceDb,
    entry: NewVectorEntry,
) -> Result<UpsertOutcome> {
    if !SOURCE_TYPES.contains(&entry.source_type.as_str()) {
        return Err(bad("source_type", "must be claim|evidence|task|skill"));
    }
    if entry.source_id.is_empty() || entry.source_id.len() > 128 {
        return Err(bad("source_id", "must be 1..=128 chars"));
    }
    if entry.text.trim().is_empty() {
        return Err(bad("text", "must not be empty"));
    }
    ensure_vector_schema(db).await?;
    let id = format!("{}:{}", entry.source_type, entry.source_id);
    let hash = content_hash(&entry.text);
    let state: Option<String> =
        sqlx::query_scalar("SELECT content_hash FROM memory_index_state WHERE source_id = ?")
            .bind(&id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| storage(format!("index state: {err}")))?;
    if state.as_deref() == Some(hash.as_str()) {
        return Ok(UpsertOutcome { id, written: false });
    }
    let embedding = embedding_engine::embed(&entry.text);
    let blob = encode_embedding(&embedding);
    let now = now_iso();
    let metadata = entry.metadata.to_string();
    sqlx::query(
        "INSERT INTO vector_entries
         (id, source_type, source_id, task_id, text, embedding, dims,
          content_hash, verified, metadata_json, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, COALESCE((SELECT created_at FROM vector_entries WHERE id = ?), ?), ?)
         ON CONFLICT(id) DO UPDATE SET
           text = excluded.text, embedding = excluded.embedding, dims = excluded.dims,
           content_hash = excluded.content_hash, verified = excluded.verified,
           metadata_json = excluded.metadata_json, task_id = excluded.task_id,
           updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(&entry.source_type)
    .bind(&entry.source_id)
    .bind(&entry.task_id)
    .bind(&entry.text)
    .bind(&blob)
    .bind(embedding.len() as i64)
    .bind(&hash)
    .bind(i64::from(entry.verified))
    .bind(&metadata)
    .bind(&id)
    .bind(&now)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|err| storage(format!("vector upsert: {err}")))?;
    sqlx::query(
        "INSERT INTO memory_index_state (source_id, content_hash, indexed_at)
         VALUES (?, ?, ?)
         ON CONFLICT(source_id) DO UPDATE SET content_hash = excluded.content_hash, indexed_at = excluded.indexed_at",
    )
    .bind(&id)
    .bind(&hash)
    .bind(&now)
    .execute(db.pool())
    .await
    .map_err(|err| storage(format!("index state: {err}")))?;
    Ok(UpsertOutcome { id, written: true })
}

/// Filters for [`search`] (scope/ACL first, always).
#[derive(Debug, Clone, Default)]
pub struct VectorFilter {
    /// Restrict to these source types (empty = all).
    pub source_types: Vec<String>,
    /// Restrict to one task (None = cross-task recall).
    pub task_id: Option<String>,
    /// Only rows created at or after this RFC 3339 timestamp.
    pub since: Option<String>,
    /// Only rows created before this RFC 3339 timestamp.
    pub until: Option<String>,
    /// Only verified sources.
    pub verified_only: bool,
}

/// One ranked vector hit.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    /// Entry id (`{source_type}:{source_id}`).
    pub id: String,
    /// Source type.
    pub source_type: String,
    /// Source row id.
    pub source_id: String,
    /// Owning task, if any.
    pub task_id: Option<String>,
    /// Cosine similarity in [-1, 1].
    pub score: f32,
    /// Leading text snippet (240 chars).
    pub snippet: String,
    /// Verified flag from index time.
    pub verified: bool,
    /// Entry creation timestamp.
    pub created_at: String,
    /// Content hash (dedupe key).
    pub content_hash: String,
}

/// Brute-force cosine search over filtered entries (score desc, id asc).
pub async fn search(
    db: &evidence_store::EvidenceDb,
    query_embedding: &[f32],
    top_k: usize,
    filter: &VectorFilter,
) -> Result<Vec<VectorHit>> {
    ensure_vector_schema(db).await?;
    let top_k = top_k.clamp(1, 50);
    let mut sql = String::from(
        "SELECT id, source_type, source_id, task_id, text, embedding, dims,
                content_hash, verified, created_at
         FROM vector_entries WHERE 1 = 1",
    );
    if !filter.source_types.is_empty() {
        let placeholders = filter
            .source_types
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(&format!(" AND source_type IN ({placeholders})"));
    }
    if filter.task_id.is_some() {
        sql.push_str(" AND task_id = ?");
    }
    if filter.since.is_some() {
        sql.push_str(" AND created_at >= ?");
    }
    if filter.until.is_some() {
        sql.push_str(" AND created_at < ?");
    }
    if filter.verified_only {
        sql.push_str(" AND verified = 1");
    }
    sql.push_str(" ORDER BY id ASC LIMIT 10000");
    let mut query = sqlx::query(&sql);
    for source_type in &filter.source_types {
        query = query.bind(source_type);
    }
    if let Some(task_id) = filter.task_id.as_deref() {
        query = query.bind(task_id);
    }
    if let Some(since) = filter.since.as_deref() {
        query = query.bind(since);
    }
    if let Some(until) = filter.until.as_deref() {
        query = query.bind(until);
    }
    let rows: Vec<VectorRow> = query
        .fetch_all(db.pool())
        .await
        .map_err(|err| storage(format!("vector search: {err}")))?
        .into_iter()
        .map(VectorRow::from_row)
        .collect::<Result<Vec<_>>>()?;
    let mut hits = Vec::new();
    for row in rows {
        let Some(vector) = decode_embedding(&row.embedding, row.dims as usize) else {
            continue;
        };
        if vector.len() != query_embedding.len() {
            continue;
        }
        let score = embedding_engine::cosine_similarity(query_embedding, &vector);
        let snippet: String = row.text.chars().take(240).collect();
        hits.push(VectorHit {
            id: row.id,
            source_type: row.source_type,
            source_id: row.source_id,
            task_id: row.task_id,
            score,
            snippet,
            verified: row.verified != 0,
            created_at: row.created_at,
            content_hash: row.content_hash,
        });
    }
    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.id.cmp(&right.id))
    });
    hits.truncate(top_k);
    Ok(hits)
}

#[derive(Debug)]
struct VectorRow {
    id: String,
    source_type: String,
    source_id: String,
    task_id: Option<String>,
    text: String,
    embedding: Vec<u8>,
    dims: i64,
    content_hash: String,
    verified: i64,
    created_at: String,
}

impl VectorRow {
    fn from_row(row: sqlx::sqlite::SqliteRow) -> Result<Self> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id").map_err(|e| storage(e.to_string()))?,
            source_type: row
                .try_get("source_type")
                .map_err(|e| storage(e.to_string()))?,
            source_id: row
                .try_get("source_id")
                .map_err(|e| storage(e.to_string()))?,
            task_id: row.try_get("task_id").map_err(|e| storage(e.to_string()))?,
            text: row.try_get("text").map_err(|e| storage(e.to_string()))?,
            embedding: row
                .try_get("embedding")
                .map_err(|e| storage(e.to_string()))?,
            dims: row.try_get("dims").map_err(|e| storage(e.to_string()))?,
            content_hash: row
                .try_get("content_hash")
                .map_err(|e| storage(e.to_string()))?,
            verified: row
                .try_get("verified")
                .map_err(|e| storage(e.to_string()))?,
            created_at: row
                .try_get("created_at")
                .map_err(|e| storage(e.to_string()))?,
        })
    }
}

/// Remove one source and its index state.
pub async fn remove_by_source(
    db: &evidence_store::EvidenceDb,
    source_type: &str,
    source_id: &str,
) -> Result<bool> {
    ensure_vector_schema(db).await?;
    let id = format!("{source_type}:{source_id}");
    let deleted = sqlx::query("DELETE FROM vector_entries WHERE id = ?")
        .bind(&id)
        .execute(db.pool())
        .await
        .map_err(|err| storage(format!("vector remove: {err}")))?
        .rows_affected();
    sqlx::query("DELETE FROM memory_index_state WHERE source_id = ?")
        .bind(&id)
        .execute(db.pool())
        .await
        .map_err(|err| storage(format!("index state: {err}")))?;
    Ok(deleted > 0)
}

/// Entry counts grouped by source type (stable key order).
pub async fn counts_by_type(db: &evidence_store::EvidenceDb) -> Result<Vec<(String, i64)>> {
    ensure_vector_schema(db).await?;
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT source_type, COUNT(*) FROM vector_entries GROUP BY source_type")
            .fetch_all(db.pool())
            .await
            .map_err(|err| storage(format!("vector counts: {err}")))?;
    let mut out = rows;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Total indexed entries.
pub async fn count(db: &evidence_store::EvidenceDb) -> Result<i64> {
    ensure_vector_schema(db).await?;
    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM vector_entries")
        .fetch_one(db.pool())
        .await
        .map_err(|err| storage(format!("vector count: {err}")))?;
    Ok(total)
}

/// Most recent index timestamp, if anything was ever indexed.
pub async fn last_indexed_at(db: &evidence_store::EvidenceDb) -> Result<Option<String>> {
    ensure_vector_schema(db).await?;
    sqlx::query_scalar("SELECT MAX(indexed_at) FROM memory_index_state")
        .fetch_one(db.pool())
        .await
        .map_err(|err| storage(format!("index state: {err}")))
}

/// Drop all vectors and index state (a rebuild refills from the ledger).
pub async fn clear_all(db: &evidence_store::EvidenceDb) -> Result<()> {
    ensure_vector_schema(db).await?;
    sqlx::query("DELETE FROM vector_entries")
        .execute(db.pool())
        .await
        .map_err(|err| storage(format!("vector clear: {err}")))?;
    sqlx::query("DELETE FROM memory_index_state")
        .execute(db.pool())
        .await
        .map_err(|err| storage(format!("index state: {err}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> evidence_store::EvidenceDb {
        evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db")
    }

    fn entry(source_type: &str, source_id: &str, text: &str) -> NewVectorEntry {
        NewVectorEntry {
            source_type: source_type.to_owned(),
            source_id: source_id.to_owned(),
            task_id: Some("task-1".to_owned()),
            text: text.to_owned(),
            verified: false,
            metadata: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn add_search_remove_round_trip() {
        let db = db().await;
        let first = upsert_entry(&db, entry("claim", "c1", "reads succeed with hash"))
            .await
            .expect("upsert");
        assert!(first.written);
        upsert_entry(&db, entry("evidence", "e1", "purchase checkout button"))
            .await
            .expect("upsert");
        assert_eq!(count(&db).await.expect("count"), 2);
        let query = embedding_engine::embed("read file hash");
        let hits = search(&db, &query, 10, &VectorFilter::default())
            .await
            .expect("search");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].source_id, "c1");
        assert!(hits[0].score >= hits[1].score);
        assert!(remove_by_source(&db, "claim", "c1").await.expect("remove"));
        assert_eq!(count(&db).await.expect("count"), 1);
        assert!(!remove_by_source(&db, "claim", "c1").await.expect("remove"));
    }

    #[tokio::test]
    async fn reindexing_unchanged_content_is_a_noop() {
        let db = db().await;
        let first = upsert_entry(&db, entry("task", "t1", "gather memory facts"))
            .await
            .expect("upsert");
        assert!(first.written);
        let second = upsert_entry(&db, entry("task", "t1", "gather memory facts"))
            .await
            .expect("upsert");
        assert!(!second.written);
        let third = upsert_entry(&db, entry("task", "t1", "gather memory facts fast"))
            .await
            .expect("upsert");
        assert!(third.written);
    }

    #[tokio::test]
    async fn filters_scope_results() {
        let db = db().await;
        upsert_entry(
            &db,
            NewVectorEntry {
                task_id: Some("task-a".to_owned()),
                ..entry("claim", "c1", "reads succeed")
            },
        )
        .await
        .expect("upsert");
        upsert_entry(
            &db,
            NewVectorEntry {
                task_id: Some("task-b".to_owned()),
                ..entry("claim", "c2", "reads succeed")
            },
        )
        .await
        .expect("upsert");
        let query = embedding_engine::embed("reads succeed");
        let scoped = search(
            &db,
            &query,
            10,
            &VectorFilter {
                task_id: Some("task-a".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect("search");
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].task_id.as_deref(), Some("task-a"));
        let typed = search(
            &db,
            &query,
            10,
            &VectorFilter {
                source_types: vec!["evidence".to_owned()],
                ..Default::default()
            },
        )
        .await
        .expect("search");
        assert!(typed.is_empty());
        let verified = search(
            &db,
            &query,
            10,
            &VectorFilter {
                verified_only: true,
                ..Default::default()
            },
        )
        .await
        .expect("search");
        assert!(verified.is_empty());
    }

    #[tokio::test]
    async fn encoding_round_trips() {
        let vector = embedding_engine::embed("round trip bytes");
        let decoded =
            decode_embedding(&encode_embedding(&vector), embedding_engine::EMBEDDING_DIMS)
                .expect("decode");
        assert_eq!(vector, decoded);
        assert!(decode_embedding(&[0u8; 10], embedding_engine::EMBEDDING_DIMS).is_none());
    }

    #[tokio::test]
    async fn rejects_bad_entries() {
        let db = db().await;
        assert!(upsert_entry(
            &db,
            NewVectorEntry {
                source_type: "weird".to_owned(),
                ..entry("x", "x", "text")
            }
        )
        .await
        .is_err());
        assert!(upsert_entry(&db, entry("claim", "", "text")).await.is_err());
        assert!(upsert_entry(&db, entry("claim", "c", "   ")).await.is_err());
    }

    #[tokio::test]
    async fn search_is_fast_for_thousands() {
        let db = db().await;
        for i in 0..1000 {
            upsert_entry(
                &db,
                entry(
                    "evidence",
                    &format!("e{i}"),
                    &format!("log line {i} file hash"),
                ),
            )
            .await
            .expect("upsert");
        }
        let query = embedding_engine::embed("file hash log");
        let started = std::time::Instant::now();
        let hits = search(&db, &query, 10, &VectorFilter::default())
            .await
            .expect("search");
        assert_eq!(hits.len(), 10);
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
    }
}
