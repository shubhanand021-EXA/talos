//! Operational metrics + cost attribution aggregates.
//!
//! All queries run against the local evidence store. Latency percentiles use
//! nearest-rank over bounded samples (exact, not estimated).

use evidence_store::EvidenceDb;
use talos_common::{Result, TalosError};

fn bad(what: &str) -> TalosError {
    TalosError::Storage(what.to_owned())
}

/// Nearest-rank percentile over a sorted sample (0..=100). Returns 0 for
/// empty input.
#[must_use]
pub fn percentile(sorted: &[i64], pct: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let pct = pct.clamp(0.0, 100.0);
    if pct <= 0.0 {
        return sorted[0];
    }
    let rank = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Per-tool success/failure counters.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStats {
    /// Tool id (or resource for cost-sourced tools).
    pub tool_id: String,
    /// Successful invocations.
    pub success: i64,
    /// Failed invocations.
    pub failure: i64,
    /// Success ratio 0..=1.
    pub success_rate: f64,
}

/// Per-model latency distribution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLatency {
    /// Provider id.
    pub provider_id: String,
    /// Model id.
    pub model_id: String,
    /// Call count.
    pub calls: i64,
    /// p50 latency in ms.
    pub p50_ms: i64,
    /// p95 latency in ms.
    pub p95_ms: i64,
    /// p99 latency in ms.
    pub p99_ms: i64,
    /// Total cost in USD.
    pub cost_usd: f64,
}

/// Latency percentile triple.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStats {
    /// p50 in ms.
    pub p50_ms: i64,
    /// p95 in ms.
    pub p95_ms: i64,
    /// p99 in ms.
    pub p99_ms: i64,
    /// Sample count.
    pub samples: i64,
}

/// One failure reason with its occurrence count.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureReason {
    /// `last_error` value (truncated at the source query).
    pub reason: String,
    /// Tasks carrying this error.
    pub count: i64,
}

/// Dashboard-grade operational metrics (all local, no external calls).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationalMetrics {
    /// Total tasks ever enqueued.
    pub total_tasks: i64,
    /// Tasks in a terminal success state.
    pub completed_tasks: i64,
    /// Tasks in `failed` / `timed_out`.
    pub failed_tasks: i64,
    /// Top `last_error` values for failed tasks (reason attribution).
    pub failure_reasons: Vec<FailureReason>,
    /// completed / total (0 when empty).
    pub success_rate: f64,
    /// failed / total (0 when empty).
    pub failure_rate: f64,
    /// Average task duration in ms (attempts table, finished only).
    pub avg_task_duration_ms: f64,
    /// Policy evaluations recorded.
    pub policy_evaluations: i64,
    /// Policy denials recorded.
    pub policy_denials: i64,
    /// Denials / evaluations (0 when empty).
    pub policy_denial_rate: f64,
    /// Model call latency percentiles (all providers).
    pub model_latency: LatencyStats,
    /// Per-model latency + cost.
    pub per_model: Vec<ModelLatency>,
    /// Per-tool success/failure.
    pub per_tool: Vec<ToolStats>,
    /// Evidence chain links verified + last verification time in ms.
    pub chain_checked: i64,
    /// Chain verification time in ms (last measured).
    pub chain_verify_ms: f64,
    /// Session totals: tokens, cost, calls.
    pub total_tokens: i64,
    /// Session cost (LLM + tracked).
    pub session_cost_usd: f64,
    /// Snapshot timestamp (RFC 3339).
    pub captured_at: String,
}

/// Cost attribution slice.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSlice {
    /// Slice key (task id, agent id, `provider/model`, or tool id).
    pub key: String,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Record count.
    pub records: i64,
}

/// Full cost breakdown for the Operations dashboard.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostBreakdown {
    /// Session total in USD (LLM + tracked).
    pub session_total_usd: f64,
    /// Session budget cap, if configured.
    pub session_budget_usd: Option<f64>,
    /// Fraction of session budget consumed (0 when uncapped).
    pub budget_consumed: f64,
    /// Cost by task id.
    pub by_task: Vec<CostSlice>,
    /// Cost by agent id.
    pub by_agent: Vec<CostSlice>,
    /// Cost by `provider/model`.
    pub by_model: Vec<CostSlice>,
    /// Cost by tool / cost type.
    pub by_tool: Vec<CostSlice>,
    /// Snapshot timestamp (RFC 3339).
    pub captured_at: String,
}

fn success_rate(completed: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        completed as f64 / total as f64
    }
}

/// Compute dashboard metrics from the evidence store.
///
/// `chain_verify_ms` is measured live by re-verifying the chain; on large
/// ledgers this is the honest cost of verification (also surfaced as the
/// "evidence chain verification time" metric).
pub async fn operational_metrics(db: &EvidenceDb) -> Result<OperationalMetrics> {
    let total_tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runtime_tasks")
        .fetch_one(db.pool())
        .await
        .map_err(|e| bad(&format!("count tasks: {e}")))?;
    let completed_tasks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runtime_tasks WHERE status IN ('completed','succeeded')",
    )
    .fetch_one(db.pool())
    .await
    .map_err(|e| bad(&format!("count completed: {e}")))?;
    let failed_tasks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runtime_tasks WHERE status IN ('failed','timed_out')",
    )
    .fetch_one(db.pool())
    .await
    .map_err(|e| bad(&format!("count failed: {e}")))?;

    let reason_rows: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT SUBSTR(last_error, 1, 160), COUNT(*) FROM runtime_tasks
         WHERE status IN ('failed','timed_out') AND last_error IS NOT NULL
         GROUP BY last_error ORDER BY COUNT(*) DESC LIMIT 5",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("failure reasons: {e}")))?;
    let failure_reasons: Vec<FailureReason> = reason_rows
        .into_iter()
        .map(|(reason, count)| FailureReason {
            reason: reason.unwrap_or_else(|| "unknown".to_owned()),
            count,
        })
        .collect();

    let avg_task_duration_ms: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(CAST((julianday(finished_at) - julianday(started_at)) * 86400000 AS REAL))
         FROM runtime_attempts WHERE finished_at IS NOT NULL",
    )
    .fetch_one(db.pool())
    .await
    .map_err(|e| bad(&format!("avg duration: {e}")))?;

    let policy_evaluations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM policy_evaluations")
        .fetch_one(db.pool())
        .await
        .map_err(|e| bad(&format!("count policy: {e}")))?;
    let policy_denials: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM policy_evaluations WHERE decision = 'deny'")
            .fetch_one(db.pool())
            .await
            .map_err(|e| bad(&format!("count denials: {e}")))?;

    let latencies: Vec<i64> =
        sqlx::query_scalar("SELECT latency_ms FROM usage_records ORDER BY latency_ms")
            .fetch_all(db.pool())
            .await
            .map_err(|e| bad(&format!("latencies: {e}")))?;
    let samples = latencies.len() as i64;
    let model_latency = LatencyStats {
        p50_ms: percentile(&latencies, 50.0),
        p95_ms: percentile(&latencies, 95.0),
        p99_ms: percentile(&latencies, 99.0),
        samples,
    };

    let model_rows: Vec<(String, String, i64, f64)> = sqlx::query_as(
        "SELECT provider_id, model_id, COUNT(*), SUM(estimated_cost_usd)
         FROM usage_records GROUP BY provider_id, model_id ORDER BY provider_id, model_id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("per-model: {e}")))?;
    let mut per_model = Vec::new();
    for (provider_id, model_id, calls, cost) in model_rows {
        let mut lat: Vec<i64> = sqlx::query_scalar(
            "SELECT latency_ms FROM usage_records WHERE provider_id = ? AND model_id = ? ORDER BY latency_ms",
        )
        .bind(&provider_id)
        .bind(&model_id)
        .fetch_all(db.pool())
        .await
        .map_err(|e| bad(&format!("model latencies: {e}")))?;
        lat.sort_unstable();
        per_model.push(ModelLatency {
            provider_id,
            model_id,
            calls,
            p50_ms: percentile(&lat, 50.0),
            p95_ms: percentile(&lat, 95.0),
            p99_ms: percentile(&lat, 99.0),
            cost_usd: cost,
        });
    }

    let tool_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT resource, SUM(CASE WHEN amount_usd >= 0 THEN 1 ELSE 0 END), COUNT(*)
         FROM cost_records GROUP BY resource ORDER BY resource",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| bad(&format!("per-tool: {e}")))?;
    let per_tool: Vec<ToolStats> = tool_rows
        .into_iter()
        .map(|(tool_id, success, total)| ToolStats {
            tool_id,
            success,
            failure: total - success,
            success_rate: success_rate(success, total),
        })
        .collect();

    let started = std::time::Instant::now();
    let report = evidence_store::get_integrity_report(db)
        .await
        .map_err(|e| bad(&format!("chain verify: {e}")))?;
    let chain_verify_ms = started.elapsed().as_secs_f64() * 1_000.0;

    let (total_tokens, llm_cost, _) = evidence_store::summarize_usage(db).await?;
    let tracked = evidence_store::sum_all_cost_records(db).await?;

    Ok(OperationalMetrics {
        total_tasks,
        completed_tasks,
        failed_tasks,
        failure_reasons,
        success_rate: success_rate(completed_tasks, total_tasks),
        failure_rate: success_rate(failed_tasks, total_tasks),
        avg_task_duration_ms: avg_task_duration_ms.unwrap_or(0.0),
        policy_evaluations,
        policy_denials,
        policy_denial_rate: success_rate(policy_denials, policy_evaluations),
        model_latency,
        per_model,
        per_tool,
        chain_checked: report.chain.checked,
        chain_verify_ms,
        total_tokens,
        session_cost_usd: llm_cost + tracked,
        captured_at: talos_common::now_iso(),
    })
}

/// Compute cost attribution across tasks / agents / models / tools.
pub async fn cost_breakdown(
    db: &EvidenceDb,
    session_budget_usd: Option<f64>,
) -> Result<CostBreakdown> {
    let by_task = combined_slices(
        db,
        "SELECT task_id, SUM(estimated_cost_usd), COUNT(*) FROM usage_records GROUP BY task_id",
        "SELECT task_id, SUM(amount_usd), COUNT(*) FROM cost_records GROUP BY task_id",
    )
    .await?;
    let by_agent = combined_slices(
        db,
        "SELECT agent_id, SUM(estimated_cost_usd), COUNT(*) FROM usage_records GROUP BY agent_id",
        "SELECT agent_id, SUM(amount_usd), COUNT(*) FROM cost_records GROUP BY agent_id",
    )
    .await?;
    let by_model: Vec<CostSlice> = {
        let rows: Vec<(String, String, Option<f64>, i64)> = sqlx::query_as(
            "SELECT provider_id, model_id, SUM(estimated_cost_usd), COUNT(*)
             FROM usage_records GROUP BY provider_id, model_id",
        )
        .fetch_all(db.pool())
        .await
        .map_err(|e| bad(&format!("by-model: {e}")))?;
        let mut out: Vec<CostSlice> = rows
            .into_iter()
            .map(|(p, m, c, n)| CostSlice {
                key: format!("{p}/{m}"),
                cost_usd: c.unwrap_or(0.0),
                records: n,
            })
            .collect();
        out.sort_by(|a, b| {
            b.cost_usd
                .partial_cmp(&a.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    };
    let by_tool = combined_slices(
        db,
        "SELECT model_id, SUM(estimated_cost_usd), COUNT(*) FROM usage_records GROUP BY model_id",
        "SELECT resource, SUM(amount_usd), COUNT(*) FROM cost_records GROUP BY resource",
    )
    .await?;

    let (_, llm_cost, _) = evidence_store::summarize_usage(db).await?;
    let tracked = evidence_store::sum_all_cost_records(db).await?;
    let session_total_usd = llm_cost + tracked;
    let budget_consumed = match session_budget_usd {
        Some(cap) if cap > 0.0 => (session_total_usd / cap).clamp(0.0, 1.0),
        _ => 0.0,
    };
    Ok(CostBreakdown {
        session_total_usd,
        session_budget_usd,
        budget_consumed,
        by_task,
        by_agent,
        by_model,
        by_tool,
        captured_at: talos_common::now_iso(),
    })
}

async fn combined_slices(db: &EvidenceDb, llm_sql: &str, cost_sql: &str) -> Result<Vec<CostSlice>> {
    use std::collections::HashMap;
    let llm_rows: Vec<(String, Option<f64>, i64)> = sqlx::query_as(llm_sql)
        .fetch_all(db.pool())
        .await
        .map_err(|e| bad(&format!("cost slice llm: {e}")))?;
    let cost_rows: Vec<(String, Option<f64>, i64)> = sqlx::query_as(cost_sql)
        .fetch_all(db.pool())
        .await
        .map_err(|e| bad(&format!("cost slice tracked: {e}")))?;
    let mut merged: HashMap<String, (f64, i64)> = HashMap::new();
    for (key, cost, count) in llm_rows.into_iter().chain(cost_rows) {
        let entry = merged.entry(key).or_insert((0.0, 0));
        entry.0 += cost.unwrap_or(0.0);
        entry.1 += count;
    }
    let mut out: Vec<CostSlice> = merged
        .into_iter()
        .map(|(key, (cost_usd, records))| CostSlice {
            key,
            cost_usd,
            records,
        })
        .collect();
    out.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evidence_store::{EnqueueParams, NewCostRecord, NewUsageRecord};

    #[test]
    fn percentiles_nearest_rank() {
        let sorted = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(percentile(&sorted, 50.0), 50);
        assert_eq!(percentile(&sorted, 95.0), 100);
        assert_eq!(percentile(&sorted, 99.0), 100);
        assert_eq!(percentile(&[], 50.0), 0);
    }

    #[tokio::test]
    async fn metrics_and_breakdown_on_empty_db() {
        let db = EvidenceDb::connect_memory().await.unwrap();
        let metrics = operational_metrics(&db).await.unwrap();
        assert_eq!(metrics.total_tasks, 0);
        assert_eq!(metrics.success_rate, 0.0);
        let breakdown = cost_breakdown(&db, Some(10.0)).await.unwrap();
        assert_eq!(breakdown.session_total_usd, 0.0);
        assert_eq!(breakdown.budget_consumed, 0.0);
    }

    #[tokio::test]
    async fn breakdown_combines_llm_and_tracked() {
        let db = EvidenceDb::connect_memory().await.unwrap();
        evidence_store::enqueue_task(
            &db,
            EnqueueParams {
                id: "t-ops",
                title: "ops",
                description: None,
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 5000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .unwrap();
        evidence_store::insert_usage_record(
            &db,
            &NewUsageRecord {
                id: "u-1",
                task_id: "t-ops",
                agent_id: "a-1",
                provider_id: "openai",
                model_id: "gpt-4o-mini",
                prompt_tokens: 1000,
                completion_tokens: 1000,
                total_tokens: 2000,
                estimated_cost_usd: 0.00075,
                latency_ms: 120,
            },
        )
        .await
        .unwrap();
        evidence_store::insert_cost_record(
            &db,
            &NewCostRecord {
                id: "c-1",
                task_id: "t-ops",
                agent_id: "a-1",
                cost_type: "tool",
                amount_usd: 0.001,
                resource: "fs.read",
            },
        )
        .await
        .unwrap();
        let breakdown = cost_breakdown(&db, None).await.unwrap();
        assert!((breakdown.session_total_usd - 0.00175).abs() < 1e-9);
        assert_eq!(breakdown.by_task.len(), 1);
        assert_eq!(breakdown.by_model.len(), 1);
    }
}
