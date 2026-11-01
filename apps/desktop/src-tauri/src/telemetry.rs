//! Operations observability commands: aggregated metrics, cost
//! attribution, recent trace summaries, and explicit local telemetry export.
//!
//! All data is local (evidence store + in-memory span collector). Nothing
//! leaves the machine; `export_telemetry` writes a JSON file only when the
//! operator invokes it. Every command records an IPC span
//! (`talos.ipc.{command}` with command name, duration, success/error).

use talos_telemetry::{SpanStatus, SpanSummary};

use crate::commands::IpcError;
use crate::state::AppState;

/// Default session budget in USD when the operator never configures one.
/// `None` (uncapped) is the default: metering without enforcement until the
/// operator opts into a cap via `set_session_budget`.
const DEFAULT_SESSION_BUDGET_USD: Option<f64> = None;

/// RAII IPC span: records `talos.ipc.{command}` on drop.
///
/// Success paths call [`IpcSpan::ok`]; anything else (including `?` early
/// returns) records an error with an explicit message.
pub(crate) struct IpcSpan {
    command: &'static str,
    task_id: Option<String>,
    started: std::time::Instant,
    status: SpanStatus,
    message: String,
}

impl IpcSpan {
    /// Start timing `command` (optional task linkage for attribution).
    pub(crate) fn new(command: &'static str, task_id: Option<String>) -> Self {
        Self {
            command,
            task_id,
            started: std::time::Instant::now(),
            status: SpanStatus::Error,
            message: "early exit".to_owned(),
        }
    }

    /// Mark the command successful before returning.
    pub(crate) fn ok(&mut self, message: &str) {
        self.status = SpanStatus::Ok;
        self.message = message.chars().take(128).collect();
    }

    /// Mark the command failed with a bounded message.
    pub(crate) fn err(&mut self, message: &str) {
        self.status = SpanStatus::Error;
        self.message = message.chars().take(128).collect();
    }
}

impl Drop for IpcSpan {
    fn drop(&mut self) {
        let duration_ms = self
            .started
            .elapsed()
            .as_millis()
            .clamp(0, i64::MAX as u128) as i64;
        let command = serde_json::json!(self.command);
        talos_telemetry::record_now(
            "ipc",
            self.command,
            self.task_id.as_deref(),
            None,
            self.status,
            &self.message.clone(),
            duration_ms,
            &[("ipc.command", command)],
        );
    }
}

/// Read the configured session budget cap (`None` = uncapped).
async fn session_budget_usd(state: &AppState) -> Option<f64> {
    state
        .session_budget_usd
        .lock()
        .await
        .or(DEFAULT_SESSION_BUDGET_USD)
}

/// Deny when session spend already exceeds the configured cap.
///
/// Called before side-effecting work (tool invocations, model calls) with a
/// zero proposed cost: it denies only when the cap is already breached, so
/// uncapped sessions (the default) always pass.
pub async fn check_session_budget(state: &AppState) -> Result<(), IpcError> {
    let Some(cap) = session_budget_usd(state).await else {
        return Ok(());
    };
    if cap <= 0.0 {
        return Ok(());
    }
    let (_, llm_cost, _) = evidence_store::summarize_usage(&state.db).await?;
    let tracked = evidence_store::sum_all_cost_records(&state.db).await?;
    let spent = llm_cost + tracked;
    if spent > cap {
        return Err(IpcError {
            code: "budget_exceeded".to_owned(),
            message: format!("session budget exceeded: spent {spent:.6} > cap {cap:.6} USD"),
        });
    }
    Ok(())
}

/// `get_operational_metrics` — dashboard aggregates: task success/failure
/// rates, model latency p50/p95/p99, per-tool and per-model stats, policy
/// denial rate, chain verification timing, session cost.
#[tauri::command]
pub async fn get_operational_metrics(
    state: tauri::State<'_, AppState>,
) -> Result<cost_tracker::OperationalMetrics, IpcError> {
    let mut span = IpcSpan::new("get_operational_metrics", None);
    let metrics = cost_tracker::operational_metrics(&state.db)
        .await
        .map_err(|err| {
            span.err("metrics failed");
            IpcError {
                code: "storage_unavailable".to_owned(),
                message: err.to_string(),
            }
        })?;
    span.ok("metrics snapshot");
    Ok(metrics)
}

/// `get_cost_breakdown` — cost attribution by task / agent / model / tool,
/// plus session total and budget consumption fraction.
#[tauri::command]
pub async fn get_cost_breakdown(
    state: tauri::State<'_, AppState>,
) -> Result<cost_tracker::CostBreakdown, IpcError> {
    let mut span = IpcSpan::new("get_cost_breakdown", None);
    let budget = session_budget_usd(&state).await;
    let breakdown = cost_tracker::cost_breakdown(&state.db, budget)
        .await
        .map_err(|err| {
            span.err("breakdown failed");
            IpcError {
                code: "storage_unavailable".to_owned(),
                message: err.to_string(),
            }
        })?;
    span.ok("cost breakdown");
    Ok(breakdown)
}

/// `get_trace_summary` — recent span summaries (metadata only: name,
/// duration, status, task/agent linkage). Never span payloads.
#[tauri::command]
pub async fn get_trace_summary(
    state: tauri::State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<SpanSummary>, IpcError> {
    let mut span = IpcSpan::new("get_trace_summary", None);
    let limit = limit.unwrap_or(50).clamp(1, 500) as usize;
    let summaries = state.telemetry.recent_summaries(limit);
    span.ok("trace summary");
    Ok(summaries)
}

/// `export_telemetry` — write buffered spans to a local JSON file (explicit
/// user action only). Old exports beyond the 7-day retention are purged.
#[tauri::command]
pub async fn export_telemetry(
    state: tauri::State<'_, AppState>,
) -> Result<talos_telemetry::ExportStats, IpcError> {
    let mut span = IpcSpan::new("export_telemetry", None);
    let dir = state.telemetry_export_dir.clone();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        span.err("export dir failed");
        return Err(IpcError {
            code: "storage_unavailable".to_owned(),
            message: format!("telemetry export dir: {err}"),
        });
    }
    // Retention first: purge in-memory spans and old files past 7 days.
    state.telemetry.retain_old();
    talos_telemetry::cleanup_old_exports(&dir, std::time::Duration::from_secs(7 * 24 * 3600));
    let filename = format!(
        "talos-traces-{}.json",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let path = dir.join(filename);
    let stats = talos_telemetry::export_spans_to_file(&state.telemetry.all_spans(), &path)
        .map_err(|err| {
            span.err("export failed");
            IpcError {
                code: "storage_unavailable".to_owned(),
                message: err.to_string(),
            }
        })?;
    span.ok("telemetry exported");
    Ok(stats)
}

/// `set_session_budget` — configure the global session spend cap in USD
/// (`None` clears it). The Operations dashboard meters consumption; live
/// tool/model paths deny once spend exceeds the cap.
#[tauri::command]
pub async fn set_session_budget(
    state: tauri::State<'_, AppState>,
    max_spend_usd: Option<f64>,
) -> Result<Option<f64>, IpcError> {
    let mut span = IpcSpan::new("set_session_budget", None);
    if let Some(cap) = max_spend_usd {
        if cap < 0.0 || !cap.is_finite() {
            span.err("invalid cap");
            return Err(IpcError {
                code: "invalid_id".to_owned(),
                message: "session budget cap must be finite and >= 0".to_owned(),
            });
        }
    }
    *state.session_budget_usd.lock().await = max_spend_usd;
    span.ok("session budget updated");
    Ok(max_spend_usd)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_state() -> AppState {
        AppState::new_in_memory_with_vault(talos_secrets::SecretVault::mock())
            .await
            .expect("mock-vault state builds")
    }

    #[tokio::test]
    async fn metrics_and_breakdown_start_empty() {
        let state = test_state().await;
        let metrics = cost_tracker::operational_metrics(&state.db).await.unwrap();
        assert_eq!(metrics.total_tasks, 0);
        let breakdown = cost_tracker::cost_breakdown(&state.db, None).await.unwrap();
        assert_eq!(breakdown.session_total_usd, 0.0);
    }

    #[tokio::test]
    async fn session_budget_round_trip_and_enforcement() {
        let state = test_state().await;
        assert_eq!(session_budget_usd(&state).await, None);
        // Uncapped: always passes.
        check_session_budget(&state).await.unwrap();
        *state.session_budget_usd.lock().await = Some(0.0);
        // Zero cap is treated as unset (no enforcement) by design.
        check_session_budget(&state).await.unwrap();
        *state.session_budget_usd.lock().await = Some(1.0);
        check_session_budget(&state).await.unwrap();
    }

    #[tokio::test]
    async fn trace_summary_shape_is_metadata_only() {
        let state = test_state().await;
        state
            .telemetry
            .start_span("ipc", "probe", None, Some("t-1"), Some("a-1"))
            .finish_ok("ok");
        let summaries = state.telemetry.recent_summaries(10);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "talos.ipc.probe");
        // Serializes to the exact camelCase wire shape the UI validates.
        let json = serde_json::to_value(&summaries[0]).unwrap();
        for key in ["name", "durationMs", "status", "startTime"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
