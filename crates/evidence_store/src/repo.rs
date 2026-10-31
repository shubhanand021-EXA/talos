//! Append-only repository functions: INSERT and SELECT only.

use sha2::{Digest, Sha256};
use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;
use crate::models::{ClaimRow, EventRow, EvidenceRow, NewClaim, NewEvent, NewEvidence};

/// Hash helper: `sha256:<hex>` over the canonical JSON payload.
#[must_use]
pub fn hash_payload(payload: &serde_json::Value) -> String {
    let raw = serde_json::to_string(payload).unwrap_or_default();
    let digest = Sha256::digest(raw.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

fn validate_id_list(what: &'static str, values: &[String]) -> Result<()> {
    if values.is_empty() {
        return Err(TalosError::InvalidId {
            what,
            reason: "at least one entry is required".to_owned(),
        });
    }
    for value in values {
        if value.is_empty() || value.len() > 128 {
            return Err(TalosError::InvalidId {
                what,
                reason: "entries must be 1..=128 chars".to_owned(),
            });
        }
    }
    Ok(())
}

/// Append one event row.
pub async fn insert_event(db: &EvidenceDb, event: NewEvent) -> Result<EventRow> {
    if event.id.is_empty() || event.id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "event_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if event.kind.is_empty() {
        return Err(TalosError::InvalidId {
            what: "event_kind",
            reason: "must not be empty".to_owned(),
        });
    }
    let created_at = now_iso();
    let payload_json = serde_json::to_string(&event.payload_json)?;
    sqlx::query("INSERT INTO events (id, kind, payload_json, created_at) VALUES (?, ?, ?, ?)")
        .bind(&event.id)
        .bind(&event.kind)
        .bind(&payload_json)
        .bind(&created_at)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(EventRow {
        id: event.id,
        kind: event.kind,
        payload_json,
        created_at,
    })
}

/// Append one evidence row (hash is computed from the payload).
pub async fn insert_evidence(db: &EvidenceDb, evidence: NewEvidence) -> Result<EvidenceRow> {
    if evidence.id.is_empty() || evidence.id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "evidence_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if evidence.kind.is_empty() {
        return Err(TalosError::InvalidId {
            what: "evidence_kind",
            reason: "must not be empty".to_owned(),
        });
    }
    let created_at = now_iso();
    let hash = hash_payload(&evidence.payload);
    let payload_json = serde_json::to_string(&evidence.payload)?;
    sqlx::query(
        "INSERT INTO evidence_records
         (id, task_id, agent_id, session_id, kind, status, label, summary, hash, uri, mime_type, payload_json, created_at, verified_at)
         VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?, NULL)",
    )
    .bind(&evidence.id)
    .bind(&evidence.task_id)
    .bind(&evidence.agent_id)
    .bind(&evidence.session_id)
    .bind(&evidence.kind)
    .bind(&evidence.label)
    .bind(&evidence.summary)
    .bind(&hash)
    .bind(&evidence.uri)
    .bind(&evidence.mime_type)
    .bind(&payload_json)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(EvidenceRow {
        id: evidence.id,
        task_id: evidence.task_id,
        agent_id: evidence.agent_id,
        session_id: evidence.session_id,
        kind: evidence.kind,
        status: "pending".to_owned(),
        label: evidence.label,
        summary: evidence.summary,
        hash,
        uri: evidence.uri,
        mime_type: evidence.mime_type,
        payload_json: Some(payload_json),
        created_at,
        verified_at: None,
    })
}

/// Append one claim row.
pub async fn insert_claim(db: &EvidenceDb, claim: NewClaim) -> Result<ClaimRow> {
    if claim.id.is_empty() || claim.id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "claim_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if claim.statement.is_empty() {
        return Err(TalosError::InvalidId {
            what: "claim_statement",
            reason: "must not be empty".to_owned(),
        });
    }
    validate_id_list("evidence_ids", &claim.evidence_ids)?;
    let created_at = now_iso();
    let evidence_ids_json = serde_json::to_string(&claim.evidence_ids)?;
    sqlx::query(
        "INSERT INTO claims (id, statement, evidence_ids_json, created_by_agent_id, task_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&claim.id)
    .bind(&claim.statement)
    .bind(&evidence_ids_json)
    .bind(&claim.created_by_agent_id)
    .bind(&claim.task_id)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(ClaimRow {
        id: claim.id,
        statement: claim.statement,
        evidence_ids_json,
        created_by_agent_id: claim.created_by_agent_id,
        task_id: claim.task_id,
        created_at,
    })
}

/// Newest-first event listing (bounded by `limit`, capped at 1000).
pub async fn list_recent_events(db: &EvidenceDb, limit: i64) -> Result<Vec<EventRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, EventRow>(
        "SELECT id, kind, payload_json, created_at FROM events ORDER BY created_at DESC, rowid DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Newest-first evidence listing (bounded by `limit`, capped at 1000).
pub async fn list_recent_evidence(db: &EvidenceDb, limit: i64) -> Result<Vec<EvidenceRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, EvidenceRow>(
        "SELECT id, task_id, agent_id, session_id, kind, status, label, summary, hash, uri, mime_type, payload_json, created_at, verified_at
         FROM evidence_records ORDER BY created_at DESC, rowid DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_db() -> EvidenceDb {
        EvidenceDb::connect_memory()
            .await
            .expect("in-memory database connects")
    }

    #[tokio::test]
    async fn events_insert_and_list() {
        let db = memory_db().await;
        insert_event(
            &db,
            NewEvent {
                id: "evt-1".to_owned(),
                kind: "task_created".to_owned(),
                payload_json: serde_json::json!({ "task": "t-1" }),
            },
        )
        .await
        .expect("insert works");
        insert_event(
            &db,
            NewEvent {
                id: "evt-2".to_owned(),
                kind: "task_updated".to_owned(),
                payload_json: serde_json::json!({}),
            },
        )
        .await
        .expect("insert works");
        let rows = list_recent_events(&db, 10).await.expect("list works");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "evt-2");
    }

    #[tokio::test]
    async fn evidence_insert_hashes_payload() {
        let db = memory_db().await;
        let row = insert_evidence(
            &db,
            NewEvidence {
                id: "ev-1".to_owned(),
                task_id: Some("task-1".to_owned()),
                agent_id: Some("agent-1".to_owned()),
                session_id: None,
                kind: "command_result".to_owned(),
                label: Some("pnpm test".to_owned()),
                summary: None,
                uri: None,
                mime_type: None,
                payload: serde_json::json!({ "exitCode": 0 }),
            },
        )
        .await
        .expect("insert works");
        assert!(row.hash.starts_with("sha256:"));
        assert_eq!(row.status, "pending");
        let rows = list_recent_evidence(&db, 10).await.expect("list works");
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn claims_require_evidence() {
        let db = memory_db().await;
        let bad = insert_claim(
            &db,
            NewClaim {
                id: "claim-1".to_owned(),
                statement: "works".to_owned(),
                evidence_ids: vec![],
                created_by_agent_id: None,
                task_id: None,
            },
        )
        .await;
        assert!(bad.is_err());
        let row = insert_claim(
            &db,
            NewClaim {
                id: "claim-1".to_owned(),
                statement: "tests pass".to_owned(),
                evidence_ids: vec!["ev-1".to_owned()],
                created_by_agent_id: Some("agent-1".to_owned()),
                task_id: None,
            },
        )
        .await
        .expect("insert works");
        assert_eq!(row.id, "claim-1");
    }

    #[tokio::test]
    async fn rejects_malformed_ids() {
        let db = memory_db().await;
        let bad = insert_event(
            &db,
            NewEvent {
                id: String::new(),
                kind: "x".to_owned(),
                payload_json: serde_json::json!({}),
            },
        )
        .await;
        assert!(bad.is_err());
    }
}
