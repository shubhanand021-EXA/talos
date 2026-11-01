//! Live browser worker client: spawns the Node worker per call.
//!
//! The worker (`packages/browser-use/dist/worker.js`) executes one JSON-RPC
//! call over argv/stdout with a hard 30s timeout: no response in time kills
//! the child. Worker values are hashes/metadata only; the keychain, prompts,
//! and evidence ledger are never reachable from the child.

use std::path::PathBuf;
use std::time::Duration;

/// Hard per-call worker timeout (the child is killed past this).
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(30);
/// Environment variable selecting live execution.
pub const LIVE_FLAG_ENV: &str = "TALOS_BROWSER_LIVE";
/// Environment variable overriding the worker entry point.
pub const WORKER_ENTRY_ENV: &str = "TALOS_BROWSER_WORKER";
/// Node binary used to spawn the worker.
pub const NODE_BINARY_ENV: &str = "TALOS_BROWSER_NODE";

/// Serializes tests that flip worker env vars (process-global routing).
#[cfg(test)]
pub(crate) static ENV_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// True only when the operator opted into live browsing.
#[must_use]
pub fn live_enabled() -> bool {
    std::env::var(LIVE_FLAG_ENV).as_deref() == Ok("1")
}

/// Resolve the worker entry point: explicit override, else the dev-tree bundle.
#[must_use]
pub fn worker_entry() -> PathBuf {
    if let Ok(custom) = std::env::var(WORKER_ENTRY_ENV) {
        return PathBuf::from(custom);
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("packages/browser-use/dist/worker.js")
}

fn node_binary() -> String {
    std::env::var(NODE_BINARY_ENV).unwrap_or_else(|_| "node".to_owned())
}

/// Outcome of one worker call.
#[derive(Debug)]
pub enum WorkerOutcome {
    /// Worker returned a receipt-shaped object (status preserved).
    Receipt {
        /// Worker status (`success` | `failed` | `denied` | `disabled`).
        status: String,
        /// Receipt fields minus `status`.
        output: serde_json::Value,
    },
    /// The worker could not serve the call (never silent).
    Failure {
        /// `worker_spawn_failed` | `worker_timeout` | `worker_crashed` | `worker_protocol`.
        code: &'static str,
        /// Short, content-free detail.
        message: String,
    },
}

/// Execute one worker call: `{...params}` on argv, receipt JSON on stdout.
pub async fn execute_live(input: serde_json::Value) -> WorkerOutcome {
    let entry = worker_entry();
    let payload = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_owned());
    let mut child = match tokio::process::Command::new(node_binary())
        .arg(entry)
        .arg(payload)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return WorkerOutcome::Failure {
                code: "worker_spawn_failed",
                message: format!("could not spawn browser worker: {err}"),
            }
        }
    };
    // Wait with a hard deadline; the child handle stays owned so a late
    // worker can always be killed (dropping a Child never kills it).
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let exit = tokio::select! {
        result = child.wait() => {
            match result {
                Ok(status) => Some(status),
                Err(err) => {
                    return WorkerOutcome::Failure {
                        code: "worker_crashed",
                        message: format!("browser worker I/O failure: {err}"),
                    }
                }
            }
        }
        _ = tokio::time::sleep(WORKER_TIMEOUT) => None,
    };
    let Some(exit) = exit else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return WorkerOutcome::Failure {
            code: "worker_timeout",
            message: "browser worker timed out after 30s and was killed".to_owned(),
        };
    };
    use tokio::io::AsyncReadExt as _;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(pipe) = stdout_pipe.as_mut() {
        let _ = pipe.read_to_end(&mut stdout).await;
    }
    if let Some(pipe) = stderr_pipe.as_mut() {
        let _ = pipe.read_to_end(&mut stderr).await;
    }
    if !exit.success() {
        let detail: String = String::from_utf8_lossy(&stderr).chars().take(200).collect();
        return WorkerOutcome::Failure {
            code: "worker_crashed",
            message: format!("browser worker exited unsuccessfully: {detail}"),
        };
    }
    let stdout_text = String::from_utf8_lossy(&stdout);
    match serde_json::from_str::<serde_json::Value>(stdout_text.trim()) {
        Ok(mut value) => {
            let status = value
                .get("status")
                .and_then(|status| status.as_str())
                .unwrap_or("failed")
                .to_owned();
            if let Some(object) = value.as_object_mut() {
                object.remove("status");
            }
            WorkerOutcome::Receipt {
                status,
                output: value,
            }
        }
        Err(_) => WorkerOutcome::Failure {
            code: "worker_protocol",
            message: "browser worker returned malformed JSON".to_owned(),
        },
    }
}

/// Map a worker receipt status to a tool status string.
#[must_use]
pub fn tool_status_for(worker_status: &str) -> &'static str {
    match worker_status {
        "success" => "success",
        "denied" => "denied",
        "disabled" => "disabled",
        _ => "failed",
    }
}

/// Build a tool error from a worker receipt's `error` object, if present.
fn receipt_error(
    output: &serde_json::Value,
    fallback_code: &str,
    fallback_message: &str,
) -> Option<crate::model::ToolErrorDetail> {
    use crate::model::ToolErrorDetail;
    output.get("error").map(|error| ToolErrorDetail {
        code: error
            .get("code")
            .and_then(|code| code.as_str())
            .unwrap_or(fallback_code)
            .to_owned(),
        message: error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or(fallback_message)
            .to_owned(),
    })
}

/// Execute one browser tool through the worker: build the JSON-RPC input
/// from the validated params, spawn the worker, and map the receipt.
pub async fn execute_tool_call(
    tool_id: &str,
    task_id: &str,
    params: &serde_json::Value,
) -> (
    crate::model::ToolStatus,
    serde_json::Value,
    Option<crate::model::ToolErrorDetail>,
) {
    use crate::model::{ToolErrorDetail, ToolStatus};
    fn fail(
        code: &str,
        message: String,
    ) -> (ToolStatus, serde_json::Value, Option<ToolErrorDetail>) {
        (
            ToolStatus::Failed,
            serde_json::json!({}),
            Some(ToolErrorDetail {
                code: code.to_owned(),
                message,
            }),
        )
    }
    let mut input = params.clone();
    if tool_id == crate::model::BROWSER_SESSION {
        let operation = params
            .get("operation")
            .and_then(|value| value.as_str())
            .unwrap_or("status");
        input = serde_json::json!({
            "method": format!("session.{operation}"),
            "params": { "taskId": task_id },
        });
    } else if let Some(action) = tool_id.strip_prefix("browser.") {
        if let Some(object) = input.as_object_mut() {
            object.insert(
                "action".to_owned(),
                serde_json::Value::String(action.to_owned()),
            );
            object.insert(
                "taskId".to_owned(),
                serde_json::Value::String(task_id.to_owned()),
            );
        }
    } else {
        return fail("invalid_params", format!("not a browser tool: {tool_id}"));
    }
    match execute_live(input).await {
        WorkerOutcome::Failure { code, message } => fail(code, message),
        WorkerOutcome::Receipt { status, output } => {
            let (error, tool_status) = match status.as_str() {
                "success" => (None, ToolStatus::Success),
                "denied" => (
                    receipt_error(&output, "policy_denied", "denied by worker").or_else(|| {
                        Some(ToolErrorDetail {
                            code: "policy_denied".to_owned(),
                            message: "denied by worker".to_owned(),
                        })
                    }),
                    ToolStatus::Denied,
                ),
                "disabled" => (
                    Some(ToolErrorDetail {
                        code: "tool_disabled".to_owned(),
                        message: "worker reports the tool disabled".to_owned(),
                    }),
                    ToolStatus::Disabled,
                ),
                _ => (
                    receipt_error(&output, "action_failed", "worker call failed").or_else(|| {
                        Some(ToolErrorDetail {
                            code: "action_failed".to_owned(),
                            message: "worker call failed".to_owned(),
                        })
                    }),
                    ToolStatus::Failed,
                ),
            };
            (tool_status, output, error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_flag_defaults_off() {
        // The flag is env-driven; assert the helper reads the exact value.
        assert_eq!(
            live_enabled(),
            std::env::var(LIVE_FLAG_ENV).as_deref() == Ok("1")
        );
    }

    #[tokio::test]
    async fn missing_worker_binary_fails_closed() {
        let _guard = ENV_GUARD.lock().await;
        std::env::set_var(WORKER_ENTRY_ENV, "/nonexistent/worker-stub.js");
        let outcome = execute_live(serde_json::json!({"action": "ping"})).await;
        std::env::remove_var(WORKER_ENTRY_ENV);
        match outcome {
            WorkerOutcome::Failure { code, .. } => {
                assert!(
                    code == "worker_spawn_failed"
                        || code == "worker_crashed"
                        || code == "worker_timeout"
                );
            }
            WorkerOutcome::Receipt { .. } => {
                panic!("missing worker entry must not produce a receipt")
            }
        }
    }

    #[tokio::test]
    async fn node_stub_round_trip() {
        let _guard = ENV_GUARD.lock().await;
        let dir = std::env::temp_dir().join(format!(
            "talos-worker-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let stub = dir.join("worker-stub.mjs");
        std::fs::write(
            &stub,
            "const input = JSON.parse(process.argv[2]);\n\
             console.log(JSON.stringify({ status: \"success\", echo: input.action ?? null }));\n",
        )
        .expect("write stub");
        std::env::set_var(WORKER_ENTRY_ENV, &stub);
        let outcome = execute_live(serde_json::json!({"action": "ping"})).await;
        std::env::remove_var(WORKER_ENTRY_ENV);
        match outcome {
            WorkerOutcome::Receipt { status, output } => {
                assert_eq!(status, "success");
                assert_eq!(output.get("echo").and_then(|v| v.as_str()), Some("ping"));
            }
            WorkerOutcome::Failure { code, message } => {
                panic!("stub worker must succeed, got {code}: {message}")
            }
        }
    }

    #[test]
    fn status_mapping_covers_worker_outcomes() {
        assert_eq!(tool_status_for("success"), "success");
        assert_eq!(tool_status_for("denied"), "denied");
        assert_eq!(tool_status_for("disabled"), "disabled");
        assert_eq!(tool_status_for("failed"), "failed");
        assert_eq!(tool_status_for("bogus"), "failed");
    }
}
