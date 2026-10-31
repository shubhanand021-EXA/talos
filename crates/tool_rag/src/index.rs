//! SQLite-backed Tool-RAG index: normalized tables, deterministic tokens.
//!
//! Schema choice: normalized `rag_tools` + `rag_terms` + `rag_receipts`
//! tables instead of FTS5. FTS5 was evaluated and rejected on purpose: its
//! native tokenizer varies across platforms and ranks by BM25 internals we
//! do not control, while exact-match token tables give byte-deterministic
//! ordering and auditable scores. No external calls, no network.
//!
//! All four index sources fold into one tool document: the manifest supplies
//! identity, capabilities, and risk; tool receipts supply success/failure
//! statistics; task titles supply usage context; compensation strategies
//! supply recovery context for tools that require it.

use std::collections::{BTreeSet, HashMap};

use evidence_store::{CompensationPlanRow, EvidenceDb, EvidenceRow, RuntimeTaskRow};
use talos_common::{now_iso, Result, TalosError};
use tool_runtime::ToolManifest;

use crate::model::{IndexedTool, ReceiptStats};
use crate::provider::FIELD_WEIGHTS;
use crate::tokenizer::tokenize;

/// Create the RAG tables when missing (idempotent, safe to call per query).
pub async fn ensure_rag_schema(db: &EvidenceDb) -> Result<()> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS rag_tools (
            tool_id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            capabilities_json TEXT NOT NULL,
            risk_level TEXT NOT NULL,
            reversible INTEGER NOT NULL,
            requires_evidence INTEGER NOT NULL,
            requires_compensation INTEGER NOT NULL,
            enabled INTEGER NOT NULL,
            text TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS rag_terms (
            token TEXT NOT NULL,
            tool_id TEXT NOT NULL,
            field TEXT NOT NULL,
            weight REAL NOT NULL,
            PRIMARY KEY (token, tool_id, field)
        )",
        "CREATE INDEX IF NOT EXISTS idx_rag_terms_token ON rag_terms(token)",
        "CREATE TABLE IF NOT EXISTS rag_receipts (
            tool_id TEXT PRIMARY KEY,
            success INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0,
            denied INTEGER NOT NULL DEFAULT 0,
            disabled INTEGER NOT NULL DEFAULT 0,
            last_status TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL
        )",
    ] {
        sqlx::query(statement)
            .execute(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    }
    Ok(())
}

/// Folded context text for one tool: manifest description plus sorted,
/// deduplicated task titles, evidence kinds, and compensation strategies.
#[must_use]
pub fn build_tool_text(
    description: &str,
    capabilities: &[String],
    task_titles: &[String],
    evidence_kinds: &[String],
    plan_strategies: &[String],
) -> String {
    let mut parts = vec![description.trim().to_owned()];
    if !capabilities.is_empty() {
        let mut caps: Vec<String> = capabilities.to_vec();
        caps.sort();
        caps.dedup();
        parts.push(format!("capabilities: {}", caps.join(" ")));
    }
    for (label, values) in [
        ("tasks", task_titles),
        ("evidence", evidence_kinds),
        ("compensation", plan_strategies),
    ] {
        let mut sorted: Vec<String> = values.to_vec();
        sorted.sort();
        sorted.dedup();
        let capped: Vec<String> = sorted.into_iter().take(5).collect();
        if !capped.is_empty() {
            parts.push(format!("{label}: {}", capped.join(" ")));
        }
    }
    parts.join("\n")
}

/// Build one index document per manifest, folding ledger context in.
///
/// - `receipt_tool_tasks`: `(tool_id, task_id)` pairs from `tool_receipt`
///   evidence rows (task titles resolved via `tasks`).
/// - `plan_strategies`: every recent compensation strategy (attached only to
///   tools with `requires_compensation`, sorted and capped).
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_indexed_tools(
    manifests: &[ToolManifest],
    receipt_tool_tasks: &[(String, String)],
    tasks: &[RuntimeTaskRow],
    evidence_kinds: &[String],
    plan_strategies: &[String],
    enabled: &HashMap<String, bool>,
) -> Vec<IndexedTool> {
    let titles: HashMap<&str, &str> = tasks
        .iter()
        .map(|task| (task.id.as_str(), task.title.as_str()))
        .collect();
    let mut by_tool: HashMap<&str, Vec<String>> = HashMap::new();
    for (tool_id, task_id) in receipt_tool_tasks {
        if let Some(title) = titles.get(task_id.as_str()) {
            by_tool
                .entry(tool_id.as_str())
                .or_default()
                .push((*title).to_owned());
        }
    }
    let mut docs = Vec::with_capacity(manifests.len());
    for manifest in manifests {
        let capabilities: Vec<String> = manifest
            .capabilities
            .iter()
            .map(|capability| format!("{capability:?}"))
            .collect();
        let task_titles = by_tool
            .get(manifest.id.as_str())
            .cloned()
            .unwrap_or_default();
        let strategies: Vec<String> = if manifest.requires_compensation {
            plan_strategies.to_vec()
        } else {
            Vec::new()
        };
        let text = build_tool_text(
            &manifest.description,
            &capabilities,
            &task_titles,
            evidence_kinds,
            &strategies,
        );
        docs.push(IndexedTool {
            tool_id: manifest.id.clone(),
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            capabilities,
            risk_level: format!("{:?}", manifest.risk_level),
            reversible: manifest.reversible,
            requires_evidence: manifest.requires_evidence,
            requires_compensation: manifest.requires_compensation,
            enabled: enabled.get(&manifest.id).copied().unwrap_or(false),
            text,
        });
    }
    docs.sort_by(|left, right| left.tool_id.cmp(&right.tool_id));
    docs
}

/// Replace the whole tool index (deterministic: sorted by tool id).
pub async fn index_tools(db: &EvidenceDb, docs: &[IndexedTool]) -> Result<usize> {
    ensure_rag_schema(db).await?;
    sqlx::query("DELETE FROM rag_terms")
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    sqlx::query("DELETE FROM rag_tools")
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    let updated_at = now_iso();
    for doc in docs {
        sqlx::query(
            "INSERT INTO rag_tools
             (tool_id, name, description, capabilities_json, risk_level, reversible,
              requires_evidence, requires_compensation, enabled, text, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&doc.tool_id)
        .bind(&doc.name)
        .bind(&doc.description)
        .bind(serde_json::to_string(&doc.capabilities).unwrap_or_else(|_| "[]".to_owned()))
        .bind(&doc.risk_level)
        .bind(i64::from(doc.reversible))
        .bind(i64::from(doc.requires_evidence))
        .bind(i64::from(doc.requires_compensation))
        .bind(i64::from(doc.enabled))
        .bind(&doc.text)
        .bind(&updated_at)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
        for (field, weight) in FIELD_WEIGHTS {
            let source = match field {
                "name" => doc.tool_id.clone() + " " + &doc.name,
                "capability" => doc.capabilities.join(" "),
                "description" => doc.description.clone(),
                _ => doc.text.clone(),
            };
            for token in tokenize(&source) {
                sqlx::query(
                    "INSERT OR IGNORE INTO rag_terms (token, tool_id, field, weight)
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&token)
                .bind(&doc.tool_id)
                .bind(field)
                .bind(weight)
                .execute(db.pool())
                .await
                .map_err(|err| TalosError::Storage(err.to_string()))?;
            }
        }
    }
    Ok(docs.len())
}

/// Record one tool receipt outcome (upserts per-tool counters).
pub async fn record_receipt(db: &EvidenceDb, tool_id: &str, status: &str) -> Result<()> {
    if tool_id.trim().is_empty() || tool_id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "tool_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    ensure_rag_schema(db).await?;
    let (success, failed, denied, disabled) = match status {
        "success" => (1, 0, 0, 0),
        "failed" => (0, 1, 0, 0),
        "denied" => (0, 0, 1, 0),
        "disabled" => (0, 0, 0, 1),
        _ => (0, 0, 0, 0),
    };
    sqlx::query(
        "INSERT INTO rag_receipts
         (tool_id, success, failed, denied, disabled, last_status, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (tool_id) DO UPDATE SET
            success = success + excluded.success,
            failed = failed + excluded.failed,
            denied = denied + excluded.denied,
            disabled = disabled + excluded.disabled,
            last_status = excluded.last_status,
            updated_at = excluded.updated_at",
    )
    .bind(tool_id)
    .bind(success)
    .bind(failed)
    .bind(denied)
    .bind(disabled)
    .bind(status)
    .bind(now_iso())
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(())
}

/// Load every indexed tool document, sorted by tool id.
pub async fn fetch_indexed_tools(db: &EvidenceDb) -> Result<Vec<IndexedTool>> {
    ensure_rag_schema(db).await?;
    let rows = sqlx::query(
        "SELECT tool_id, name, description, capabilities_json, risk_level, reversible,
                requires_evidence, requires_compensation, enabled, text
         FROM rag_tools ORDER BY tool_id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    let mut docs = Vec::with_capacity(rows.len());
    for row in rows {
        use sqlx::Row;
        let capabilities_json: String = row.try_get("capabilities_json").unwrap_or_default();
        let capabilities: Vec<String> =
            serde_json::from_str(&capabilities_json).unwrap_or_default();
        let enabled_int: i64 = row.try_get("enabled").unwrap_or(0);
        let reversible_int: i64 = row.try_get("reversible").unwrap_or(0);
        let requires_evidence_int: i64 = row.try_get("requires_evidence").unwrap_or(0);
        let requires_compensation_int: i64 = row.try_get("requires_compensation").unwrap_or(0);
        docs.push(IndexedTool {
            tool_id: row.try_get("tool_id").unwrap_or_default(),
            name: row.try_get("name").unwrap_or_default(),
            description: row.try_get("description").unwrap_or_default(),
            capabilities,
            risk_level: row.try_get("risk_level").unwrap_or_default(),
            reversible: reversible_int != 0,
            requires_evidence: requires_evidence_int != 0,
            requires_compensation: requires_compensation_int != 0,
            enabled: enabled_int != 0,
            text: row.try_get("text").unwrap_or_default(),
        });
    }
    Ok(docs)
}

/// Load per-tool receipt statistics.
pub async fn fetch_receipt_stats(db: &EvidenceDb) -> Result<HashMap<String, ReceiptStats>> {
    ensure_rag_schema(db).await?;
    let rows = sqlx::query(
        "SELECT tool_id, success, failed, denied, disabled FROM rag_receipts ORDER BY tool_id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    let mut stats = HashMap::new();
    for row in rows {
        use sqlx::Row;
        let tool_id: String = row.try_get("tool_id").unwrap_or_default();
        let get = |key: &str| -> u64 { row.try_get::<i64, _>(key).unwrap_or(0).max(0) as u64 };
        stats.insert(
            tool_id,
            ReceiptStats {
                success: get("success"),
                failed: get("failed"),
                denied: get("denied"),
                disabled: get("disabled"),
            },
        );
    }
    Ok(stats)
}

/// Aggregate per-tool receipt statistics directly from ledger evidence rows.
///
/// Only `tool_receipt` rows with a parseable `{ toolId, status }` payload
/// count; the row's own status (`verified`, ...) is the ledger lifecycle,
/// not the tool outcome. Malformed payloads are skipped so the ledger stays
/// authoritative. Read-only: unlike [`record_receipt`], this never writes,
/// which keeps query commands side-effect free.
#[must_use]
pub fn stats_from_evidence(rows: &[EvidenceRow]) -> HashMap<String, ReceiptStats> {
    let mut stats: HashMap<String, ReceiptStats> = HashMap::new();
    for row in rows {
        if row.kind != "tool_receipt" {
            continue;
        }
        let Some(payload) = row
            .payload_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        else {
            continue;
        };
        let tool_id = payload
            .get("toolId")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if tool_id.is_empty() || tool_id.len() > 128 {
            continue;
        }
        let entry = stats.entry(tool_id.to_owned()).or_default();
        match payload.get("status").and_then(|value| value.as_str()) {
            Some("success") => entry.success += 1,
            Some("failed") => entry.failed += 1,
            Some("denied") => entry.denied += 1,
            Some("disabled") => entry.disabled += 1,
            _ => {}
        }
    }
    stats
}
/// Extract `(tool_id, task_id)` pairs from `tool_receipt` evidence rows.
///
/// Malformed payloads are skipped (the ledger stays authoritative; the index
/// only reflects what it can parse deterministically).
#[must_use]
pub fn receipt_tool_tasks(rows: &[EvidenceRow]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for row in rows {
        if row.kind != "tool_receipt" {
            continue;
        }
        let task_id = row.task_id.clone().unwrap_or_default();
        if task_id.is_empty() {
            continue;
        }
        let tool_id = row
            .payload_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|payload| {
                payload
                    .get("toolId")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            });
        if let Some(tool_id) = tool_id {
            if !tool_id.is_empty() {
                pairs.push((tool_id, task_id));
            }
        }
    }
    pairs.sort();
    pairs.dedup();
    pairs
}

/// Sorted unique evidence kinds across rows (index context, deterministic).
#[must_use]
pub fn evidence_kind_list(rows: &[EvidenceRow]) -> Vec<String> {
    let mut kinds: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        if row.kind != "tool_receipt" && !row.kind.trim().is_empty() {
            kinds.insert(row.kind.clone());
        }
    }
    kinds.into_iter().collect()
}

/// Sorted unique compensation strategies across plans (deterministic).
#[must_use]
pub fn plan_strategy_list(plans: &[CompensationPlanRow]) -> Vec<String> {
    let mut strategies: BTreeSet<String> = BTreeSet::new();
    for plan in plans {
        if !plan.strategy.trim().is_empty() {
            strategies.insert(plan.strategy.clone());
        }
    }
    strategies.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str) -> ToolManifest {
        ToolManifest {
            id: id.to_owned(),
            name: id.to_owned(),
            version: "0.1.0".to_owned(),
            description: format!("Use {id} for work"),
            capabilities: vec![policy_broker::Capability::ReadFile],
            risk_level: policy_broker::RiskLevel::Low,
            reversible: true,
            requires_evidence: false,
            requires_compensation: false,
            enabled_by_default: true,
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn rebuild_is_deterministic_across_runs() {
        let db = EvidenceDb::connect_memory().await.expect("memory db");
        let manifests = vec![manifest("fs.read"), manifest("fs.list")];
        let enabled: HashMap<String, bool> =
            [("fs.read".to_owned(), true), ("fs.list".to_owned(), true)]
                .into_iter()
                .collect();
        let docs = build_indexed_tools(&manifests, &[], &[], &[], &[], &enabled);
        assert_eq!(index_tools(&db, &docs).await.expect("index"), 2);
        let first = fetch_indexed_tools(&db).await.expect("fetch");
        assert_eq!(index_tools(&db, &docs).await.expect("reindex"), 2);
        let second = fetch_indexed_tools(&db).await.expect("fetch");
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn receipt_stats_accumulate_per_tool() {
        let db = EvidenceDb::connect_memory().await.expect("memory db");
        record_receipt(&db, "fs.read", "success")
            .await
            .expect("record");
        record_receipt(&db, "fs.read", "success")
            .await
            .expect("record");
        record_receipt(&db, "fs.read", "failed")
            .await
            .expect("record");
        record_receipt(&db, "fs.read", "denied")
            .await
            .expect("record");
        let stats = fetch_receipt_stats(&db).await.expect("stats");
        let entry = stats.get("fs.read").expect("entry");
        assert_eq!(entry.success, 2);
        assert_eq!(entry.failed, 1);
        assert_eq!(entry.denied, 1);
        assert!(record_receipt(&db, "", "success").await.is_err());
    }

    #[test]
    fn tool_text_folds_context_deterministically() {
        let first = build_tool_text("x", &[], &[], &[], &[]);
        let second = build_tool_text("x", &[], &[], &[], &[]);
        assert_eq!(first, second);
    }

    #[test]
    fn stats_from_evidence_counts_tool_outcomes_only() {
        use evidence_store::EvidenceRow;
        let row = |id: &str, kind: &str, payload: serde_json::Value| EvidenceRow {
            id: id.to_owned(),
            task_id: Some("task-1".to_owned()),
            agent_id: None,
            session_id: None,
            kind: kind.to_owned(),
            status: "verified".to_owned(),
            label: None,
            summary: None,
            hash: "sha256:x".to_owned(),
            uri: None,
            mime_type: None,
            payload_json: Some(payload.to_string()),
            created_at: "2026-10-31T00:00:00Z".to_owned(),
            verified_at: None,
            seq: None,
            prev_hash: None,
        };
        let rows = vec![
            row(
                "e1",
                "tool_receipt",
                serde_json::json!({"toolId": "fs.read", "status": "success"}),
            ),
            row(
                "e2",
                "tool_receipt",
                serde_json::json!({"toolId": "fs.read", "status": "failed"}),
            ),
            row(
                "e3",
                "tool_receipt",
                serde_json::json!({"toolId": "fs.read", "status": "denied"}),
            ),
            row(
                "e4",
                "context_compaction",
                serde_json::json!({"toolId": "fs.read", "status": "success"}),
            ),
            row("e5", "tool_receipt", serde_json::json!({"nope": true})),
        ];
        let stats = stats_from_evidence(&rows);
        let entry = stats.get("fs.read").expect("entry");
        assert_eq!((entry.success, entry.failed, entry.denied), (1, 1, 1));
        assert_eq!(stats.len(), 1);
    }

    #[test]
    fn tool_text_folds_context_case_insensitively() {
        let first = build_tool_text(
            "Do work",
            &["ReadFile".to_owned()],
            &["b task".to_owned(), "a task".to_owned()],
            &["tool_receipt".to_owned()],
            &["backup_restore".to_owned()],
        );
        let second = build_tool_text(
            "Do work",
            &["ReadFile".to_owned()],
            &["a task".to_owned(), "b task".to_owned()],
            &["tool_receipt".to_owned()],
            &["backup_restore".to_owned()],
        );
        assert_eq!(first, second);
        assert!(first.contains("a task b task"));
    }
}
