//! Batch 3 (telemetry) cost ledger: per-task/agent/model/tool spend.
//!
//! Privacy rule: only metadata and USD amounts land here. Raw prompts,
//! file contents, screenshots, and secret values are never persisted.

use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;

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

/// Full `cost_records` row (one per tracked non-LLM charge).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct CostRecordRow {
    /// Record identifier.
    pub id: String,
    /// Owning task.
    pub task_id: String,
    /// Calling agent.
    pub agent_id: String,
    /// `llm` | `tool` | `browser` | `computer_use`.
    pub cost_type: String,
    /// Charge in USD (>= 0, finite).
    pub amount_usd: f64,
    /// Resource detail (tool id, `browser.session`, `computer.action`, ...).
    pub resource: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Payload for [`insert_cost_record`].
#[derive(Debug, Clone, PartialEq)]
pub struct NewCostRecord<'a> {
    /// Record identifier.
    pub id: &'a str,
    /// Owning task.
    pub task_id: &'a str,
    /// Calling agent.
    pub agent_id: &'a str,
    /// `llm` | `tool` | `browser` | `computer_use`.
    pub cost_type: &'a str,
    /// Charge in USD.
    pub amount_usd: f64,
    /// Resource detail.
    pub resource: &'a str,
}

/// Append one cost record to the spend ledger.
pub async fn insert_cost_record(
    db: &EvidenceDb,
    record: &NewCostRecord<'_>,
) -> Result<CostRecordRow> {
    valid_ident("cost_record_id", record.id)?;
    valid_ident("task_id", record.task_id)?;
    valid_ident("agent_id", record.agent_id)?;
    valid_ident("resource", record.resource)?;
    match record.cost_type {
        "llm" | "tool" | "browser" | "computer_use" => {}
        _ => return Err(bad("cost_type must be llm|tool|browser|computer_use")),
    }
    if record.amount_usd < 0.0 || !record.amount_usd.is_finite() {
        return Err(bad("amount must be finite and >= 0"));
    }
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO cost_records (id, task_id, agent_id, cost_type, amount_usd, resource, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(record.id)
    .bind(record.task_id)
    .bind(record.agent_id)
    .bind(record.cost_type)
    .bind(record.amount_usd)
    .bind(record.resource)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|e| bad(&format!("insert cost record: {e}")))?;
    Ok(CostRecordRow {
        id: record.id.to_owned(),
        task_id: record.task_id.to_owned(),
        agent_id: record.agent_id.to_owned(),
        cost_type: record.cost_type.to_owned(),
        amount_usd: record.amount_usd,
        resource: record.resource.to_owned(),
        created_at,
    })
}

/// Cumulative tracked spend for one task in USD.
pub async fn sum_task_cost_records(db: &EvidenceDb, task_id: &str) -> Result<f64> {
    valid_ident("task_id", task_id)?;
    let total: Option<f64> =
        sqlx::query_scalar("SELECT SUM(amount_usd) FROM cost_records WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(db.pool())
            .await
            .map_err(|e| bad(&format!("sum task cost: {e}")))?;
    Ok(total.unwrap_or(0.0))
}

/// Session-wide tracked spend in USD (all tasks, all types).
pub async fn sum_all_cost_records(db: &EvidenceDb) -> Result<f64> {
    let total: Option<f64> = sqlx::query_scalar("SELECT SUM(amount_usd) FROM cost_records")
        .fetch_one(db.pool())
        .await
        .map_err(|e| bad(&format!("sum all cost: {e}")))?;
    Ok(total.unwrap_or(0.0))
}

/// Latest cost records, newest first (bounded).
pub async fn list_recent_cost_records(db: &EvidenceDb, limit: i64) -> Result<Vec<CostRecordRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, CostRecordRow>(
        "SELECT * FROM cost_records ORDER BY created_at DESC, id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("list cost records: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    fn record(id: &str) -> NewCostRecord<'_> {
        NewCostRecord {
            id,
            task_id: "task-1",
            agent_id: "agent-1",
            cost_type: "tool",
            amount_usd: 0.001,
            resource: "fs.read",
        }
    }

    #[tokio::test]
    async fn round_trip_and_sums() {
        let db = db().await;
        assert_eq!(sum_task_cost_records(&db, "task-1").await.unwrap(), 0.0);
        assert_eq!(sum_all_cost_records(&db).await.unwrap(), 0.0);
        insert_cost_record(&db, &record("c-1")).await.unwrap();
        insert_cost_record(&db, &record("c-2")).await.unwrap();
        assert!((sum_task_cost_records(&db, "task-1").await.unwrap() - 0.002).abs() < 1e-9);
        assert!((sum_all_cost_records(&db).await.unwrap() - 0.002).abs() < 1e-9);
        assert_eq!(list_recent_cost_records(&db, 10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn rejects_bad_records() {
        let db = db().await;
        let mut bad_type = record("c-x");
        bad_type.cost_type = "bogus";
        assert!(insert_cost_record(&db, &bad_type).await.is_err());
        let mut bad_cost = record("c-y");
        bad_cost.amount_usd = -1.0;
        assert!(insert_cost_record(&db, &bad_cost).await.is_err());
        let mut bad_id = record("");
        bad_id.id = "";
        assert!(insert_cost_record(&db, &bad_id).await.is_err());
    }
}
