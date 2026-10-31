//! Batch 3 LLM gateway records: per-call usage ledger, interaction hashes,
//! and the optional per-task spend cap.
//!
//! Privacy rule: only metadata and SHA-256 hashes land here. Raw prompts,
//! completions, and secret values are never persisted.

use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;
use crate::repo::hash_payload;

fn bad(what: &str) -> TalosError {
    TalosError::Storage(what.to_owned())
}

fn valid_ident(what: &'static str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        return Err(TalosError::InvalidId {
            what,
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    Ok(())
}

/// Full `usage_records` row (one per provider call).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct UsageRecordRow {
    /// Record identifier.
    pub id: String,
    /// Owning task.
    pub task_id: String,
    /// Calling agent.
    pub agent_id: String,
    /// Provider id.
    pub provider_id: String,
    /// Model id.
    pub model_id: String,
    /// Input tokens.
    pub prompt_tokens: i64,
    /// Output tokens.
    pub completion_tokens: i64,
    /// Total tokens.
    pub total_tokens: i64,
    /// Estimated cost in USD.
    pub estimated_cost_usd: f64,
    /// End-to-end latency in milliseconds.
    pub latency_ms: i64,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Payload for [`insert_usage_record`].
#[derive(Debug, Clone, PartialEq)]
pub struct NewUsageRecord<'a> {
    /// Record identifier.
    pub id: &'a str,
    /// Owning task.
    pub task_id: &'a str,
    /// Calling agent.
    pub agent_id: &'a str,
    /// Provider id.
    pub provider_id: &'a str,
    /// Model id.
    pub model_id: &'a str,
    /// Input tokens.
    pub prompt_tokens: u32,
    /// Output tokens.
    pub completion_tokens: u32,
    /// Total tokens.
    pub total_tokens: u32,
    /// Estimated cost in USD.
    pub estimated_cost_usd: f64,
    /// End-to-end latency in milliseconds.
    pub latency_ms: u64,
}

/// Full `model_interactions` row (hashes only, never content).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ModelInteractionRow {
    /// Interaction identifier.
    pub id: String,
    /// Owning task.
    pub task_id: String,
    /// Calling agent.
    pub agent_id: String,
    /// Provider id.
    pub provider_id: String,
    /// Model id.
    pub model_id: String,
    /// `sha256:` hash of the canonical request metadata.
    pub request_hash: String,
    /// `sha256:` hash of the canonical response metadata.
    pub response_hash: String,
    /// Stop reason.
    pub finish_reason: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Payload for [`insert_model_interaction`].
#[derive(Debug, Clone, PartialEq)]
pub struct NewModelInteraction<'a> {
    /// Interaction identifier.
    pub id: &'a str,
    /// Owning task.
    pub task_id: &'a str,
    /// Calling agent.
    pub agent_id: &'a str,
    /// Provider id.
    pub provider_id: &'a str,
    /// Model id.
    pub model_id: &'a str,
    /// `sha256:` hash of the request (hashes only, never raw content).
    pub request_hash: &'a str,
    /// `sha256:` hash of the response metadata.
    pub response_hash: &'a str,
    /// Stop reason.
    pub finish_reason: &'a str,
}

/// Append one usage record to the spend ledger.
pub async fn insert_usage_record(
    db: &EvidenceDb,
    record: &NewUsageRecord<'_>,
) -> Result<UsageRecordRow> {
    valid_ident("usage_record_id", record.id)?;
    valid_ident("task_id", record.task_id)?;
    valid_ident("agent_id", record.agent_id)?;
    if record.estimated_cost_usd < 0.0 || !record.estimated_cost_usd.is_finite() {
        return Err(bad("estimated cost must be finite and >= 0"));
    }
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO usage_records
         (id, task_id, agent_id, provider_id, model_id, prompt_tokens,
          completion_tokens, total_tokens, estimated_cost_usd, latency_ms, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(record.id)
    .bind(record.task_id)
    .bind(record.agent_id)
    .bind(record.provider_id)
    .bind(record.model_id)
    .bind(record.prompt_tokens as i64)
    .bind(record.completion_tokens as i64)
    .bind(record.total_tokens as i64)
    .bind(record.estimated_cost_usd)
    .bind(record.latency_ms as i64)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("insert usage record: {e}")))?;
    Ok(UsageRecordRow {
        id: record.id.to_owned(),
        task_id: record.task_id.to_owned(),
        agent_id: record.agent_id.to_owned(),
        provider_id: record.provider_id.to_owned(),
        model_id: record.model_id.to_owned(),
        prompt_tokens: record.prompt_tokens as i64,
        completion_tokens: record.completion_tokens as i64,
        total_tokens: record.total_tokens as i64,
        estimated_cost_usd: record.estimated_cost_usd,
        latency_ms: record.latency_ms as i64,
        created_at,
    })
}

/// Append one interaction-hash record (hashes only, never content).
pub async fn insert_model_interaction(
    db: &EvidenceDb,
    interaction: &NewModelInteraction<'_>,
) -> Result<ModelInteractionRow> {
    valid_ident("interaction_id", interaction.id)?;
    valid_ident("task_id", interaction.task_id)?;
    valid_ident("agent_id", interaction.agent_id)?;
    for (what, hash) in [
        ("request_hash", interaction.request_hash),
        ("response_hash", interaction.response_hash),
    ] {
        if !hash.starts_with("sha256:") || hash.len() > 128 {
            return Err(TalosError::InvalidId {
                what,
                reason: "must be a sha256: hash".to_owned(),
            });
        }
    }
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO model_interactions
         (id, task_id, agent_id, provider_id, model_id, request_hash,
          response_hash, finish_reason, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(interaction.id)
    .bind(interaction.task_id)
    .bind(interaction.agent_id)
    .bind(interaction.provider_id)
    .bind(interaction.model_id)
    .bind(interaction.request_hash)
    .bind(interaction.response_hash)
    .bind(interaction.finish_reason)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("insert interaction: {e}")))?;
    Ok(ModelInteractionRow {
        id: interaction.id.to_owned(),
        task_id: interaction.task_id.to_owned(),
        agent_id: interaction.agent_id.to_owned(),
        provider_id: interaction.provider_id.to_owned(),
        model_id: interaction.model_id.to_owned(),
        request_hash: interaction.request_hash.to_owned(),
        response_hash: interaction.response_hash.to_owned(),
        finish_reason: interaction.finish_reason.to_owned(),
        created_at,
    })
}

/// Cumulative estimated spend for one task in USD.
pub async fn sum_task_spend(db: &EvidenceDb, task_id: &str) -> Result<f64> {
    valid_ident("task_id", task_id)?;
    let total: Option<f64> =
        sqlx::query_scalar("SELECT SUM(estimated_cost_usd) FROM usage_records WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(db.pool())
            .await
            .map_err(|e| bad(&format!("sum spend: {e}")))?;
    Ok(total.unwrap_or(0.0))
}

/// Session-wide totals: (total_tokens, estimated_cost_usd, call_count).
pub async fn summarize_usage(db: &EvidenceDb) -> Result<(i64, f64, i64)> {
    let row: (Option<i64>, Option<f64>, i64) = sqlx::query_as(
        "SELECT SUM(total_tokens), SUM(estimated_cost_usd), COUNT(*) FROM usage_records",
    )
    .fetch_one(db.pool())
    .await
    .map_err(|e| bad(&format!("summarize usage: {e}")))?;
    Ok((row.0.unwrap_or(0), row.1.unwrap_or(0.0), row.2))
}

/// Per-provider session totals: `(provider_id, total_tokens, cost_usd, calls)`.
pub async fn summarize_usage_by_provider(db: &EvidenceDb) -> Result<Vec<(String, i64, f64, i64)>> {
    let rows: Vec<(String, Option<i64>, Option<f64>, i64)> = sqlx::query_as(
        "SELECT provider_id, SUM(total_tokens), SUM(estimated_cost_usd), COUNT(*)
         FROM usage_records GROUP BY provider_id ORDER BY provider_id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("summarize by provider: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|(id, tokens, cost, calls)| (id, tokens.unwrap_or(0), cost.unwrap_or(0.0), calls))
        .collect())
}

/// Latest usage records, newest first (bounded).
pub async fn list_recent_usage(db: &EvidenceDb, limit: i64) -> Result<Vec<UsageRecordRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, UsageRecordRow>(
        "SELECT * FROM usage_records ORDER BY created_at DESC, id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list usage: {e}")))
}

/// Set (or clear with `None`) the per-task spend cap in USD.
pub async fn set_task_budget(
    db: &EvidenceDb,
    task_id: &str,
    max_spend_usd: Option<f64>,
) -> Result<()> {
    valid_ident("task_id", task_id)?;
    if let Some(cap) = max_spend_usd {
        if cap < 0.0 || !cap.is_finite() {
            return Err(bad("budget cap must be finite and >= 0"));
        }
    }
    sqlx::query("UPDATE runtime_tasks SET max_spend_usd = ? WHERE id = ?")
        .bind(max_spend_usd)
        .bind(task_id)
        .execute(db.pool())
        .await
        .map_err(|e| bad(&format!("set budget: {e}")))?;
    Ok(())
}

/// Read the per-task spend cap (`None` = uncapped or unknown task).
pub async fn get_task_budget(db: &EvidenceDb, task_id: &str) -> Result<Option<f64>> {
    valid_ident("task_id", task_id)?;
    let cap: Option<f64> =
        sqlx::query_scalar("SELECT max_spend_usd FROM runtime_tasks WHERE id = ?")
            .bind(task_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|e| bad(&format!("get budget: {e}")))?
            .flatten();
    Ok(cap)
}

/// Hash canonical request metadata (hashes only, never raw content).
pub fn hash_request_metadata(metadata: &serde_json::Value) -> String {
    hash_payload(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::EnqueueParams;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    async fn seeded_db() -> EvidenceDb {
        let db = db().await;
        crate::enqueue_task(
            &db,
            EnqueueParams {
                id: "task-b3",
                title: "budget task",
                description: None,
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 5000,
                backoff_ms: 500,
                scenario: "quick_success",
            },
        )
        .await
        .expect("enqueue");
        db
    }

    fn usage(id: &str) -> NewUsageRecord<'_> {
        NewUsageRecord {
            id,
            task_id: "task-b3",
            agent_id: "agent-1",
            provider_id: "openai",
            model_id: "gpt-4o-mini",
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            estimated_cost_usd: 0.000045,
            latency_ms: 120,
        }
    }

    #[tokio::test]
    async fn usage_round_trip_and_spend_sum() {
        let db = seeded_db().await;
        assert_eq!(sum_task_spend(&db, "task-b3").await.unwrap(), 0.0);
        insert_usage_record(&db, &usage("u-1")).await.unwrap();
        insert_usage_record(&db, &usage("u-2")).await.unwrap();
        let (tokens, cost, calls) = summarize_usage(&db).await.unwrap();
        assert_eq!(tokens, 300);
        assert!((cost - 0.00009).abs() < 1e-9);
        assert_eq!(calls, 2);
        assert!((sum_task_spend(&db, "task-b3").await.unwrap() - 0.00009).abs() < 1e-9);
        let recent = list_recent_usage(&db, 10).await.unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[tokio::test]
    async fn interaction_hashes_only() {
        let db = db().await;
        let row = insert_model_interaction(
            &db,
            &NewModelInteraction {
                id: "m-1",
                task_id: "task-b3",
                agent_id: "agent-1",
                provider_id: "openai",
                model_id: "gpt-4o-mini",
                request_hash: "sha256:abc",
                response_hash: "sha256:def",
                finish_reason: "stop",
            },
        )
        .await
        .unwrap();
        assert!(row.request_hash.starts_with("sha256:"));
        // Raw content is rejected: hashes only.
        let raw = NewModelInteraction {
            id: "m-2",
            task_id: "task-b3",
            agent_id: "agent-1",
            provider_id: "openai",
            model_id: "gpt-4o-mini",
            request_hash: "not-a-hash",
            response_hash: "sha256:def",
            finish_reason: "stop",
        };
        assert!(insert_model_interaction(&db, &raw).await.is_err());
    }

    #[tokio::test]
    async fn budget_cap_round_trip() {
        let db = seeded_db().await;
        assert_eq!(get_task_budget(&db, "task-b3").await.unwrap(), None);
        assert_eq!(get_task_budget(&db, "missing").await.unwrap(), None);
        set_task_budget(&db, "task-b3", Some(0.01)).await.unwrap();
        assert_eq!(get_task_budget(&db, "task-b3").await.unwrap(), Some(0.01));
        set_task_budget(&db, "task-b3", None).await.unwrap();
        assert_eq!(get_task_budget(&db, "task-b3").await.unwrap(), None);
        assert!(set_task_budget(&db, "task-b3", Some(-1.0)).await.is_err());
    }

    #[tokio::test]
    async fn rejects_bad_records() {
        let db = db().await;
        let mut bad = usage("");
        assert!(insert_usage_record(&db, &bad).await.is_err());
        bad = usage("u-x");
        let mut bad_cost = bad.clone();
        bad_cost.estimated_cost_usd = -1.0;
        assert!(insert_usage_record(&db, &bad_cost).await.is_err());
    }
}
