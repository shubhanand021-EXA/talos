//! Deterministic skill extraction from verified task runs.
//!
//! When a task completes with `fidelity == "verified"`, the extractor reads
//! the task's `tool_receipt` evidence chain (tool ids, order, success, and
//! the recorded action-request parameters), identifies the reusable pattern,
//! and generates a SKILL.md document validated by `instruction_parser`.
//! No LLM is involved: same evidence chain → same skill document, always.
//!
//! The outcome is stored as a `quarantined` skill row and indexed for
//! semantic recall. Promotion stays explicit (user validates → publishes).

use evidence_store::{EvidenceDb, NewSkill, RuntimeTaskRow, SkillRow};
use talos_common::{now_iso, Result};

/// Outcome of one extraction attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtractionOutcome {
    /// A new quarantined skill was created and indexed.
    Created(Box<SkillRow>),
    /// Extraction was skipped; the reason is human-readable.
    Skipped {
        /// Machine-readable code (`not_completed`, `not_verified`,
        /// `no_tool_calls`, `already_distilled`, ...).
        code: String,
        /// Human-readable detail.
        detail: String,
    },
}

/// One successful tool call in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedCall {
    /// Tool identifier (`fs.read`, ...).
    pub tool_id: String,
    /// Recorded action-request parameters, if the audit trail has them.
    pub params: Option<serde_json::Value>,
    /// Policy verdict that authorized the call.
    pub policy_decision: String,
}

fn skipped(code: &str, detail: String) -> ExtractionOutcome {
    ExtractionOutcome::Skipped {
        code: code.to_owned(),
        detail,
    }
}

/// Lowercase slug: alphanumerics kept, runs of other chars become one `-`,
/// trimmed and capped at 48 chars (`"task"` when empty).
#[must_use]
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in text.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        return "task".to_owned();
    }
    trimmed.chars().take(48).collect()
}

const TRIGGER_STOPWORDS: &[&str] = &[
    "the", "with", "from", "that", "this", "then", "into", "over", "under", "after", "before",
    "when", "what", "which", "your", "their", "there", "here", "have", "will", "would", "should",
    "task",
];

/// Trigger phrases from a task title: significant lowercase words (len > 3,
/// not stopwords), deduplicated, capped at 8, sorted for determinism.
#[must_use]
pub fn triggers_for(title: &str) -> Vec<String> {
    let mut words: Vec<String> = title
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| word.len() > 3 && !TRIGGER_STOPWORDS.contains(word))
        .map(str::to_owned)
        .collect();
    words.sort();
    words.dedup();
    words.truncate(8);
    words
}

/// Highest risk level across a tool set, as a skill permission ceiling
/// (`low` | `medium` | `high` | `irreversible`).
#[must_use]
pub fn ceiling_for(tool_ids: &[String]) -> String {
    let mut rank = 0u8;
    for tool_id in tool_ids {
        let (risk, _) = tool_runtime::risk_for(tool_id);
        let level = match risk {
            policy_broker::RiskLevel::Low => 0,
            policy_broker::RiskLevel::Medium => 1,
            policy_broker::RiskLevel::High => 2,
            policy_broker::RiskLevel::Irreversible => 3,
        };
        rank = rank.max(level);
    }
    match rank {
        0 => "low",
        1 => "medium",
        2 => "high",
        _ => "irreversible",
    }
    .to_owned()
}

/// One-line parameter summary: `key=value` pairs joined with `, `, values
/// truncated to 40 chars, whole summary capped at 160 chars. Keys sorted.
fn summarize_params(params: &serde_json::Value) -> Option<String> {
    let object = params.as_object()?;
    if object.is_empty() {
        return None;
    }
    let mut keys: Vec<&String> = object.keys().collect();
    keys.sort();
    let mut parts = Vec::new();
    for key in keys {
        let value = &object[key.as_str()];
        let rendered = match value {
            serde_json::Value::String(text) => text.chars().take(40).collect::<String>(),
            serde_json::Value::Null => continue,
            other => {
                let raw = other.to_string();
                if raw.len() > 40 {
                    format!("{}…", &raw[..40])
                } else {
                    raw
                }
            }
        };
        parts.push(format!("{key}={rendered}"));
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join(", ");
    Some(joined.chars().take(160).collect())
}

/// Render one procedure step: `Call fs.read (path=notes.txt). Policy: allow.`
fn render_step(index: usize, call: &OrderedCall) -> String {
    let mut step = format!("{}. Call `{}`", index + 1, call.tool_id);
    if let Some(params) = call.params.as_ref().and_then(summarize_params) {
        step.push_str(&format!(" ({params})"));
    }
    step.push_str(&format!(". Policy: {}.", call.policy_decision));
    step
}

/// Pure document builder: task row + ordered successful calls → [`NewSkill`].
/// Returns `None` when there is nothing reusable (no calls). Deterministic:
/// sorted tools, sorted triggers, fixed formatting.
#[must_use]
pub fn build_skill_document(task: &RuntimeTaskRow, calls: &[OrderedCall]) -> Option<NewSkill> {
    if calls.is_empty() {
        return None;
    }
    let mut tool_ids: Vec<String> = calls.iter().map(|call| call.tool_id.clone()).collect();
    tool_ids.sort();
    tool_ids.dedup();
    let steps: Vec<String> = calls
        .iter()
        .enumerate()
        .map(|(index, call)| render_step(index, call))
        .collect();
    let title = task.title.trim();
    let name = format!("{}-skill", slug(title));
    let detail = task.description.as_deref().unwrap_or("").trim();
    let description = if detail.is_empty() {
        format!(
            "Reusable procedure distilled from verified task '{}'.",
            truncate(title, 120)
        )
    } else {
        format!(
            "Reusable procedure distilled from verified task '{}': {}.",
            truncate(title, 80),
            truncate(detail, 120)
        )
    };
    let description = truncate(&description, 300);
    Some(NewSkill {
        id: format!("skill-{}", slug(&task.id)),
        name: truncate(&name, 128),
        description,
        procedure_text: steps.join("\n"),
        source_task_id: Some(task.id.clone()),
        version: "1".to_owned(),
        test_fixture_ref: Some(task.id.clone()),
        required_tools: tool_ids.clone(),
        permission_ceiling: ceiling_for(&tool_ids),
    })
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect()
}

/// Render the SKILL.md document for a [`NewSkill`] (frontmatter + body) in
/// exactly the shape `instruction_parser::parse_skill` accepts.
#[must_use]
pub fn skill_markdown(skill: &NewSkill, triggers: &[String]) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", skill.name));
    out.push_str(&format!("description: {}\n", skill.description));
    out.push_str(&format!("triggers: [{}]\n", triggers.join(", ")));
    out.push_str(&format!(
        "required_tools: [{}]\n",
        skill.required_tools.join(", ")
    ));
    out.push_str("---\n");
    out.push_str(&skill.procedure_text);
    if !skill.procedure_text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Load a task's successful tool calls in execution order (oldest first).
/// Receipts that fail to parse are skipped (never fatal); dry-runs are kept
/// only when successful since they still describe the approved procedure.
/// Denied/disabled/failed calls never become procedure steps.
pub async fn ordered_calls_for_task(db: &EvidenceDb, task_id: &str) -> Result<Vec<OrderedCall>> {
    let rows = evidence_store::list_recent_evidence(db, 1000).await?;
    let mut calls: Vec<(String, String, OrderedCall)> = Vec::new();
    for row in rows {
        if row.task_id.as_deref() != Some(task_id) || row.kind != "tool_receipt" {
            continue;
        }
        let Some(payload) = row.payload_json.as_deref() else {
            continue;
        };
        let receipt: tool_runtime::ToolReceipt = match serde_json::from_str(payload) {
            Ok(receipt) => receipt,
            Err(_) => continue,
        };
        if receipt.status != tool_runtime::ToolStatus::Success {
            continue;
        }
        let params =
            evidence_store::get_action_payload(db, &format!("tool-{}", receipt.id)).await?;
        calls.push((
            receipt.executed_at.clone(),
            receipt.id.clone(),
            OrderedCall {
                tool_id: receipt.tool_id,
                params,
                policy_decision: format!("{:?}", receipt.policy_decision).to_ascii_lowercase(),
            },
        ));
    }
    calls.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    Ok(calls.into_iter().map(|(_, _, call)| call).collect())
}

/// Extract a skill from one task: verified completion → quarantined skill.
///
/// Skips (never errors on) tasks that are missing, not completed, not
/// verified, have no successful tool calls, or were already distilled.
/// Created skills are indexed for semantic recall before returning.
pub async fn extract_skill_for_task(db: &EvidenceDb, task_id: &str) -> Result<ExtractionOutcome> {
    let started = std::time::Instant::now();
    let outcome = extract_inner(db, task_id).await?;
    let duration_ms = started.elapsed().as_millis().clamp(0, i64::MAX as u128) as i64;
    let (status, message) = match &outcome {
        ExtractionOutcome::Created(row) => {
            (talos_telemetry::SpanStatus::Ok, format!("skill {}", row.id))
        }
        ExtractionOutcome::Skipped { code, .. } => {
            (talos_telemetry::SpanStatus::Ok, format!("skipped: {code}"))
        }
    };
    talos_telemetry::record_now(
        "skill_extractor",
        "extract",
        Some(task_id),
        None,
        status,
        &message,
        duration_ms,
        &[("skill.outcome", serde_json::json!(message))],
    );
    Ok(outcome)
}

async fn extract_inner(db: &EvidenceDb, task_id: &str) -> Result<ExtractionOutcome> {
    if task_id.trim().is_empty() || task_id.len() > 128 {
        return Ok(skipped(
            "invalid_task_id",
            "task id must be 1..=128 chars".to_owned(),
        ));
    }
    let Some(task) = evidence_store::get_task(db, task_id).await? else {
        return Ok(skipped(
            "unknown_task",
            format!("task {task_id} does not exist"),
        ));
    };
    if task.status != "completed" {
        return Ok(skipped(
            "not_completed",
            format!("task status is '{}', need 'completed'", task.status),
        ));
    }
    if task.fidelity != "verified" {
        return Ok(skipped(
            "not_verified",
            format!("task fidelity is '{}', need 'verified'", task.fidelity),
        ));
    }
    // Dedupe: one distilled skill per source task.
    let existing = evidence_store::list_skills(db, None, 500).await?;
    if existing
        .iter()
        .any(|row| row.source_task_id.as_deref() == Some(task_id))
    {
        return Ok(skipped(
            "already_distilled",
            "a skill already exists for this task".to_owned(),
        ));
    }
    let calls = ordered_calls_for_task(db, task_id).await?;
    let Some(draft) = build_skill_document(&task, &calls) else {
        return Ok(skipped(
            "no_tool_calls",
            "no successful tool calls to distill".to_owned(),
        ));
    };
    // The generated markdown must round-trip through the SKILL.md parser:
    // extraction output is always a valid skill document.
    let triggers = triggers_for(&task.title);
    let markdown = skill_markdown(&draft, &triggers);
    if let Err(err) = instruction_parser::parse_skill(&markdown, None) {
        return Ok(skipped(
            "invalid_document",
            format!("generated SKILL.md rejected: {err}"),
        ));
    }
    let row = evidence_store::create_skill(db, draft).await?;
    // Index for semantic recall (same text shape as the memory indexer).
    let text = memory_indexer::skill_text(&row.name, &row.description, &row.procedure_text);
    let _ = vector_store::upsert_entry(
        db,
        vector_store::NewVectorEntry {
            source_type: "skill".to_owned(),
            source_id: row.id.clone(),
            task_id: row.source_task_id.clone(),
            text,
            verified: false,
            metadata: serde_json::json!({ "status": row.status }),
        },
    )
    .await;
    let _ = now_iso();
    Ok(ExtractionOutcome::Created(Box::new(row)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, title: &str) -> RuntimeTaskRow {
        RuntimeTaskRow {
            id: id.to_owned(),
            title: title.to_owned(),
            description: Some("Read the manifest and hash it".to_owned()),
            priority: "normal".to_owned(),
            status: "completed".to_owned(),
            attempts: 1,
            max_attempts: 3,
            timeout_ms: 5000,
            backoff_ms: 500,
            scenario: "quick_success".to_owned(),
            last_error: None,
            next_retry_at: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:01Z".to_owned(),
            max_spend_usd: None,
            parent_task_id: None,
            fidelity: "verified".to_owned(),
        }
    }

    fn call(tool: &str) -> OrderedCall {
        OrderedCall {
            tool_id: tool.to_owned(),
            params: Some(serde_json::json!({"path": "notes.txt"})),
            policy_decision: "allow".to_owned(),
        }
    }

    #[test]
    fn document_build_is_deterministic() {
        let task = task("task-1", "Read Manifest Hash");
        let calls = vec![call("fs.list"), call("fs.read")];
        let first = build_skill_document(&task, &calls).expect("doc");
        let second = build_skill_document(&task, &calls).expect("doc");
        assert_eq!(first, second);
        assert_eq!(
            first.required_tools,
            vec!["fs.list".to_owned(), "fs.read".to_owned()]
        );
        assert_eq!(first.permission_ceiling, "low");
        assert_eq!(first.version, "1");
        assert!(first.procedure_text.contains("1. Call `fs.list`"));
        assert!(first.procedure_text.contains("2. Call `fs.read`"));
        // No calls → no document.
        assert!(build_skill_document(&task, &[]).is_none());
    }

    #[test]
    fn ceiling_reflects_highest_tool_risk() {
        assert_eq!(ceiling_for(&["fs.read".to_owned()]), "low");
        assert_eq!(
            ceiling_for(&["fs.read".to_owned(), "fs.write".to_owned()]),
            "medium"
        );
        assert_eq!(ceiling_for(&["process.exec".to_owned()]), "high");
        assert_eq!(ceiling_for(&["fs.delete".to_owned()]), "irreversible");
        assert_eq!(ceiling_for(&[]), "low");
    }

    #[test]
    fn markdown_round_trips_through_parser() {
        let task = task("task-1", "Read Manifest Hash");
        let draft = build_skill_document(&task, &[call("fs.read")]).expect("doc");
        let triggers = triggers_for(&task.title);
        assert_eq!(
            triggers,
            vec!["hash".to_owned(), "manifest".to_owned(), "read".to_owned()]
        );
        let markdown = skill_markdown(&draft, &triggers);
        let parsed = instruction_parser::parse_skill(&markdown, None).expect("parses");
        assert_eq!(parsed.name, draft.name);
        assert_eq!(parsed.required_tools, draft.required_tools);
        assert_eq!(parsed.steps.len(), 1);
    }

    #[test]
    fn slug_is_stable() {
        assert_eq!(slug("Read Manifest Hash!"), "read-manifest-hash");
        assert_eq!(slug("!!!"), "task");
        assert_eq!(slug("Read Manifest Hash!"), slug("Read Manifest Hash!"));
    }

    async fn mem_db() -> EvidenceDb {
        EvidenceDb::connect_memory().await.expect("memory db")
    }

    fn receipt_value(id: &str, task: &str, tool: &str, at: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "toolId": tool,
            "taskId": task,
            "dryRun": false,
            "status": "success",
            "output": {"path": "notes.txt"},
            "error": null,
            "policyDecision": "allow",
            "policyReason": "allowed",
            "evidenceId": null,
            "compensationPlanId": null,
            "executionFidelity": "live",
            "executedAt": at,
        })
    }

    #[tokio::test]
    async fn extraction_from_verified_task_produces_valid_skill() {
        let db = mem_db().await;
        evidence_store::enqueue_task(
            &db,
            evidence_store::EnqueueParams {
                id: "task-extract",
                title: "Read Manifest Hash",
                description: Some("Read the manifest and hash it"),
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 5000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .expect("enqueue");
        evidence_store::force_transition(&db, "task-extract", "queued", "completed", "test")
            .await
            .expect("complete");
        evidence_store::set_task_fidelity(&db, "task-extract", "verified")
            .await
            .expect("fidelity");
        for (index, tool) in ["fs.read", "fs.list"].iter().enumerate() {
            let receipt_id = format!("receipt-{index}");
            evidence_store::insert_evidence(
                &db,
                evidence_store::NewEvidence {
                    id: format!("ev-{index}"),
                    task_id: Some("task-extract".to_owned()),
                    agent_id: None,
                    session_id: Some("session-local".to_owned()),
                    kind: "tool_receipt".to_owned(),
                    label: None,
                    summary: None,
                    uri: None,
                    mime_type: None,
                    payload: receipt_value(
                        &receipt_id,
                        "task-extract",
                        tool,
                        &format!("2026-01-01T00:00:0{index}Z"),
                    ),
                },
            )
            .await
            .expect("insert");
            evidence_store::record_action_request(
                &db,
                &evidence_store::NewActionRecord {
                    id: format!("tool-{receipt_id}"),
                    agent_id: "agent-1".to_owned(),
                    task_id: "task-extract".to_owned(),
                    capability: "ReadFile".to_owned(),
                    intent: "extract".to_owned(),
                    target: "notes.txt".to_owned(),
                    payload: serde_json::json!({"path": "notes.txt"}),
                    risk_level: "Low".to_owned(),
                    reversible: true,
                    requested_at: "2026-01-01T00:00:00Z".to_owned(),
                },
                "talos-policy-broker/1",
                "Allow",
                "allowed",
                &[],
            )
            .await
            .expect("audit");
        }
        let outcome = extract_skill_for_task(&db, "task-extract")
            .await
            .expect("extract");
        let ExtractionOutcome::Created(row) = outcome else {
            panic!("expected Created, got {outcome:?}");
        };
        assert_eq!(row.status, "quarantined");
        assert_eq!(row.source_task_id.as_deref(), Some("task-extract"));
        assert_eq!(row.test_fixture_ref.as_deref(), Some("task-extract"));
        assert_eq!(row.version, "1");
        // Generated markdown is a valid SKILL.md document.
        let markdown = skill_markdown(
            &evidence_store::NewSkill {
                id: row.id.clone(),
                name: row.name.clone(),
                description: row.description.clone(),
                procedure_text: row.procedure_text.clone(),
                source_task_id: row.source_task_id.clone(),
                version: row.version.clone(),
                test_fixture_ref: row.test_fixture_ref.clone(),
                required_tools: serde_json::from_str(&row.required_tools).expect("tools"),
                permission_ceiling: row.permission_ceiling.clone(),
            },
            &triggers_for("Read Manifest Hash"),
        );
        let parsed = instruction_parser::parse_skill(&markdown, None).expect("parses");
        assert_eq!(parsed.steps.len(), 2);
        assert!(parsed.steps[0].contains("fs.read"));
        // Second run dedupes: already distilled.
        let again = extract_skill_for_task(&db, "task-extract")
            .await
            .expect("rerun");
        assert!(matches!(
            again,
            ExtractionOutcome::Skipped { code, .. } if code == "already_distilled"
        ));
    }

    #[tokio::test]
    async fn extraction_skips_unverified_tasks() {
        let db = mem_db().await;
        evidence_store::enqueue_task(
            &db,
            evidence_store::EnqueueParams {
                id: "task-mock",
                title: "Mock Task",
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
        .expect("enqueue");
        // Still queued: skipped.
        let outcome = extract_skill_for_task(&db, "task-mock")
            .await
            .expect("extract");
        assert!(matches!(
            outcome,
            ExtractionOutcome::Skipped { code, .. } if code == "not_completed"
        ));
        // Completed but mock fidelity: skipped.
        evidence_store::force_transition(&db, "task-mock", "queued", "completed", "test")
            .await
            .expect("done");
        let outcome = extract_skill_for_task(&db, "task-mock")
            .await
            .expect("extract");
        assert!(matches!(
            outcome,
            ExtractionOutcome::Skipped { code, .. } if code == "not_verified"
        ));
        // Unknown task: skipped.
        let outcome = extract_skill_for_task(&db, "missing")
            .await
            .expect("extract");
        assert!(matches!(
            outcome,
            ExtractionOutcome::Skipped { code, .. } if code == "unknown_task"
        ));
    }
}
