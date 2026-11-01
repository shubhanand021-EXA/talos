//! Human-in-the-Loop approval lifecycle with Plan/Act separation.
//!
//! Flow: agent proposes action -> policy evaluates -> `require_human_review`
//! creates a pending [`ApprovalRequest`] (task moves to `waiting_policy`) ->
//! the user grants / denies / modifies via the ApprovalSheet UI -> the
//! runtime executes, fails, or re-evaluates. Approvals expire after
//! [`DEFAULT_APPROVAL_TIMEOUT_SECS`] (5 minutes) unless decided first.
//!
//! Plan mode: the agent proposes without executing; [`PlanDocument`] is the
//! ordered review list. Act mode executes already-approved actions. Auto
//! mode auto-approves low-risk reversible reads and gates everything else.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use policy_broker::{ActionRequest, Capability, PolicyDecision, RiskLevel};
use serde::{Deserialize, Serialize};
use talos_common::{now_iso, Result, TalosError};

/// Seconds before an undecided approval expires (5 minutes).
pub const DEFAULT_APPROVAL_TIMEOUT_SECS: i64 = 300;

/// Blast radius of a proposed action (UI icon + color source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectClass {
    /// Read-only: no state change.
    Read,
    /// Bounded local mutation (single file, workspace-scoped).
    LocalWrite,
    /// Leaves the device (email, network POST, publish).
    ExternalWrite,
    /// Wide or hard-to-reverse effects (bulk delete, permission change).
    Destructive,
    /// Moves money or paid quota.
    Financial,
    /// Escalates privilege (system control, keychain, installs).
    Privileged,
}

impl EffectClass {
    /// Human label for the ApprovalSheet badge.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "READ",
            Self::LocalWrite => "LOCAL_WRITE",
            Self::ExternalWrite => "EXTERNAL_WRITE",
            Self::Destructive => "DESTRUCTIVE",
            Self::Financial => "FINANCIAL",
            Self::Privileged => "PRIVILEGED",
        }
    }
}

/// Whether the action can be undone (drives the reversibility section).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    /// A checkpoint exists; one-click restore.
    Reversible,
    /// Needs the listed compensation steps to roll back.
    Irreversible,
    /// Cannot be determined statically.
    Unknown,
}

/// Execution mode per task (stored in task constraints).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskMode {
    /// Propose only; nothing executes until the user approves.
    #[default]
    Plan,
    /// Execute already-approved actions.
    Act,
    /// Auto-approve low-risk reversible reads; gate everything else.
    Auto,
}

/// Lifecycle of one approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    /// Waiting for a human decision.
    Pending,
    /// User approved; the runtime may execute.
    Granted,
    /// User denied; the action must not execute.
    Denied,
    /// Timed out without a decision.
    Expired,
    /// User edited parameters; re-evaluated by policy (then pending again).
    Modified,
}

/// One step of a compensation plan shown in the CompensationSheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationStepView {
    /// Step order (0-based).
    pub order: usize,
    /// Machine action (`rollback_command`, `restore_checkpoint`, ...).
    pub action_type: String,
    /// Human description.
    pub description: String,
    /// Step payload (target, command, ...).
    pub payload: serde_json::Value,
    /// Execution state: `pending` | `running` | `done` | `failed`.
    pub status: String,
}

/// Full HITL approval record: everything the ApprovalSheet renders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Stable id (`appr-{task}-{seq}`).
    pub approval_id: String,
    /// Owning task.
    pub task_id: String,
    /// Proposing agent.
    pub agent_id: String,
    /// Underlying action request id.
    pub action_id: String,
    /// Plain-language summary ("Send this email to 43 recipients...").
    pub summary: String,
    /// Exact resource (file path, URL, command, recipient).
    pub target: String,
    /// Capability string (`WriteFile`, `ExecuteCommand`, ...).
    pub capability: String,
    /// Blast radius.
    pub effect_class: EffectClass,
    /// Undo story.
    pub reversibility: Reversibility,
    /// Declared risk.
    pub risk_level: RiskLevel,
    /// Policy reason (`require_human_review` rationale).
    pub policy_reason: String,
    /// Follow-up checks the caller must satisfy.
    pub required_checks: Vec<String>,
    /// Exact payload that will be written/executed.
    pub payload: serde_json::Value,
    /// Payload shape: `file_edit` | `shell` | `browser` | `network` | `generic`.
    pub payload_kind: String,
    /// Original file content (for `file_edit` diffs).
    pub original_content: Option<String>,
    /// Proposed file content (for `file_edit` diffs).
    pub proposed_content: Option<String>,
    /// Multi-file diffs: (path, original, proposed).
    pub diff_files: Vec<DiffFile>,
    /// Compensation steps (when irreversible).
    pub compensation: Vec<CompensationStepView>,
    /// Estimated cost (`tokens`, `usd`) when known.
    pub cost_estimate: Option<CostEstimate>,
    /// What permissions this grants and for how long.
    pub scope: String,
    /// ISO-8601 expiry; undecided approvals die here.
    pub expires_at: String,
    /// ISO-8601 request time.
    pub requested_at: String,
    /// Current lifecycle state.
    pub status: ApprovalStatus,
    /// Who decided (`user`, `auto-policy`, `system-expiry`).
    pub decided_by: Option<String>,
    /// Denial reason, if denied.
    pub deny_reason: Option<String>,
}

/// One file in a multi-file diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    /// File path.
    pub path: String,
    /// Original content.
    pub original: String,
    /// Proposed content.
    pub proposed: String,
}

/// Token/compute cost preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Estimated tokens.
    pub tokens: u64,
    /// Estimated USD.
    pub estimated_usd: f64,
}

/// One ordered entry of a [`PlanDocument`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedAction {
    /// Approval backing this step.
    pub approval_id: String,
    /// Plain-language step description.
    pub description: String,
    /// Exact target.
    pub target: String,
    /// Blast radius.
    pub effect: EffectClass,
    /// Undo story.
    pub reversibility: Reversibility,
    /// Declared risk.
    pub risk: RiskLevel,
    /// Current approval status.
    pub status: ApprovalStatus,
}

/// Ordered review list for a task in Plan mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanDocument {
    /// Owning task.
    pub task_id: String,
    /// Mode the plan was built under.
    pub mode: TaskMode,
    /// Ordered steps.
    pub actions: Vec<PlannedAction>,
    /// ISO-8601 creation time.
    pub created_at: String,
    /// Approved / total counts.
    pub approved_count: usize,
    /// Pending / total counts.
    pub pending_count: usize,
}

/// Progress of one compensation execution (CompensationSheet live state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompensationExecution {
    /// Plan being executed.
    pub plan_id: String,
    /// Owning task.
    pub task_id: String,
    /// What went wrong (human sentence).
    pub failure_reason: String,
    /// Step states in order.
    pub steps: Vec<CompensationStepView>,
    /// Steps finished.
    pub steps_completed: usize,
    /// Overall: `pending` | `running` | `completed` | `failed_needs_manual_review`.
    pub status: String,
    /// ISO-8601 start.
    pub started_at: String,
    /// ISO-8601 finish, if done.
    pub finished_at: Option<String>,
}

fn is_expired_at(approval: &ApprovalRequest, now: &DateTime<Utc>) -> bool {
    approval
        .expires_at
        .parse::<DateTime<Utc>>()
        .map(|exp| *now >= exp)
        .unwrap_or(false)
}

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Map a capability + risk onto its blast-radius class.
///
/// Never panics; unknown combinations fall back to the safest bucket that
/// matches the capability family.
#[must_use]
pub fn effect_class_for(capability: Capability, risk: RiskLevel) -> EffectClass {
    match (capability, risk) {
        (Capability::ReadFile | Capability::OpenBrowser, RiskLevel::Low) => EffectClass::Read,
        (Capability::WriteFile, RiskLevel::Medium) => EffectClass::LocalWrite,
        (Capability::NetworkRequest, _) => EffectClass::ExternalWrite,
        (Capability::ExecuteCommand, RiskLevel::Irreversible) | (Capability::SystemControl, _) => {
            EffectClass::Privileged
        }
        (_, RiskLevel::Irreversible) | (_, RiskLevel::High) => EffectClass::Destructive,
        (Capability::UiAction, _) => EffectClass::LocalWrite,
        _ => EffectClass::LocalWrite,
    }
}

/// Reversibility for an action request: declared flag first, then risk.
#[must_use]
pub fn reversibility_for(reversible: bool, risk: RiskLevel) -> Reversibility {
    if reversible && risk != RiskLevel::Irreversible {
        Reversibility::Reversible
    } else if risk == RiskLevel::Irreversible {
        Reversibility::Irreversible
    } else if reversible {
        Reversibility::Reversible
    } else {
        Reversibility::Unknown
    }
}

/// Payload shape from capability (drives the diff/payload renderer).
#[must_use]
pub fn payload_kind_for(capability: Capability) -> &'static str {
    match capability {
        Capability::WriteFile => "file_edit",
        Capability::ExecuteCommand | Capability::SystemControl => "shell",
        Capability::OpenBrowser | Capability::UiAction => "browser",
        Capability::NetworkRequest => "network",
        Capability::ReadFile => "read",
    }
}

/// Plain-language summary naming the account/resource and side effects.
#[must_use]
pub fn summarize_action(request: &ActionRequest) -> String {
    let payload_accounts = request
        .payload_json
        .get("account")
        .or_else(|| request.payload_json.get("from"))
        .and_then(serde_json::Value::as_str)
        .map(|a| format!(" using the account {a}"))
        .unwrap_or_default();
    let recipients = request
        .payload_json
        .get("recipients")
        .and_then(serde_json::Value::as_array)
        .map(|r| format!(" to {} recipients", r.len()));
    let attachment = request
        .payload_json
        .get("attachment")
        .and_then(serde_json::Value::as_str)
        .map(|a| format!("; attachment {a} will leave the device"));
    match request.capability {
        Capability::NetworkRequest if recipients.is_some() => {
            format!(
                "{}{}; attachment detail{}",
                request.intent,
                recipients.unwrap_or_default(),
                attachment.unwrap_or_default()
            )
        }
        _ => format!(
            "{}{} on {}{}",
            request.intent,
            payload_accounts,
            request.target,
            attachment.unwrap_or_default()
        ),
    }
}

/// Scope + duration sentence for the approval (what this grant unlocks).
#[must_use]
pub fn scope_for(request: &ActionRequest, capability: Capability) -> String {
    format!(
        "Grants {request_id} one-shot permission to exercise {capability:?} on {target}; \
         valid for this task only and expires with the approval.",
        request_id = request.id,
        target = request.target,
    )
}

/// In-memory HITL store: pending approvals, per-task modes, opt-outs, plans.
#[derive(Debug, Default)]
pub struct ApprovalStore {
    approvals: HashMap<String, ApprovalRequest>,
    order: Vec<String>,
    task_modes: HashMap<String, TaskMode>,
    /// (task_id, capability) pairs the user muted with "don't ask again".
    scoped_opt_outs: HashSet<(String, String)>,
    executions: HashMap<String, CompensationExecution>,
    seq: u64,
}

impl ApprovalStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mode for a task (default [`TaskMode::Plan`], the safest).
    #[must_use]
    pub fn task_mode(&self, task_id: &str) -> TaskMode {
        self.task_modes.get(task_id).copied().unwrap_or_default()
    }

    /// Set the Plan/Act/Auto mode for a task.
    pub fn set_task_mode(&mut self, task_id: &str, mode: TaskMode) {
        self.task_modes.insert(task_id.to_owned(), mode);
    }

    /// True when the user muted this capability for this task.
    #[must_use]
    pub fn is_opted_out(&self, task_id: &str, capability: &str) -> bool {
        self.scoped_opt_outs
            .contains(&(task_id.to_owned(), capability.to_owned()))
    }

    /// Request human review for an action evaluated as `require_human_review`.
    ///
    /// In `Auto` mode low-risk reversible reads skip the sheet and return a
    /// pre-granted approval; muted capabilities do the same. Everything else
    /// becomes `pending` with a 5-minute expiry.
    pub fn request_approval(
        &mut self,
        request: &ActionRequest,
        decision: PolicyDecision,
        reason: &str,
        required_checks: &[String],
    ) -> ApprovalRequest {
        self.seq += 1;
        let approval_id = format!("appr-{}-{}", request.task_id, self.seq);
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(DEFAULT_APPROVAL_TIMEOUT_SECS);
        let effect_class = effect_class_for(request.capability, request.risk_level);
        let reversibility = reversibility_for(request.reversible, request.risk_level);
        let payload_kind = payload_kind_for(request.capability).to_owned();
        let (original_content, proposed_content, diff_files) =
            diff_parts_for(request, &payload_kind);
        let compensation = compensation_for(request, reversibility);
        let auto_grant = (self.task_mode(&request.task_id) == TaskMode::Auto
            && request.risk_level == RiskLevel::Low
            && request.reversible)
            || self.is_opted_out(&request.task_id, &format!("{:?}", request.capability));
        let status = if auto_grant {
            ApprovalStatus::Granted
        } else {
            ApprovalStatus::Pending
        };
        let approval = ApprovalRequest {
            approval_id: approval_id.clone(),
            task_id: request.task_id.clone(),
            agent_id: request.agent_id.clone(),
            action_id: request.id.clone(),
            summary: summarize_action(request),
            target: request.target.clone(),
            capability: format!("{:?}", request.capability),
            effect_class,
            reversibility,
            risk_level: request.risk_level,
            policy_reason: format!("{decision:?}: {reason}"),
            required_checks: required_checks.to_owned(),
            payload: request.payload_json.clone(),
            payload_kind,
            original_content,
            proposed_content,
            diff_files,
            compensation,
            cost_estimate: cost_estimate_for(request),
            scope: scope_for(request, request.capability),
            expires_at: expires_at.to_rfc3339(),
            requested_at: now.to_rfc3339(),
            status,
            decided_by: if auto_grant {
                Some("auto-policy".to_owned())
            } else {
                None
            },
            deny_reason: None,
        };
        self.order.push(approval_id.clone());
        self.approvals.insert(approval_id.clone(), approval.clone());
        approval
    }

    /// Fetch one approval by id.
    pub fn get(&self, approval_id: &str) -> Result<ApprovalRequest> {
        self.approvals
            .get(approval_id)
            .cloned()
            .ok_or_else(|| err("unknown approval"))
    }

    /// Grant a pending approval (optionally recording a scoped opt-out).
    pub fn grant(
        &mut self,
        approval_id: &str,
        granted_by: &str,
        dont_ask_again: bool,
    ) -> Result<ApprovalRequest> {
        let expired = self
            .approvals
            .get(approval_id)
            .map(|a| is_expired_at(a, &Utc::now()))
            .unwrap_or(false);
        let approval = self
            .approvals
            .get_mut(approval_id)
            .ok_or_else(|| err("unknown approval"))?;
        if approval.status == ApprovalStatus::Expired || expired {
            approval.status = ApprovalStatus::Expired;
            return Err(err("approval expired"));
        }
        if approval.status != ApprovalStatus::Pending && approval.status != ApprovalStatus::Modified
        {
            return Err(err("approval already decided"));
        }
        approval.status = ApprovalStatus::Granted;
        approval.decided_by = Some(granted_by.to_owned());
        if dont_ask_again {
            self.scoped_opt_outs
                .insert((approval.task_id.clone(), approval.capability.clone()));
        }
        Ok(approval.clone())
    }

    /// Deny a pending approval; the action must not execute.
    pub fn deny(
        &mut self,
        approval_id: &str,
        denied_by: &str,
        reason: &str,
    ) -> Result<ApprovalRequest> {
        let approval = self
            .approvals
            .get_mut(approval_id)
            .ok_or_else(|| err("unknown approval"))?;
        if approval.status != ApprovalStatus::Pending && approval.status != ApprovalStatus::Modified
        {
            return Err(err("approval already decided"));
        }
        approval.status = ApprovalStatus::Denied;
        approval.decided_by = Some(denied_by.to_owned());
        approval.deny_reason = Some(reason.to_owned());
        Ok(approval.clone())
    }

    /// Apply user-edited parameters and mark for policy re-evaluation.
    ///
    /// Returns the updated (still pending) approval; the caller re-runs the
    /// policy engine over the patched [`ActionRequest`] shape.
    pub fn modify(
        &mut self,
        approval_id: &str,
        modified_params: &serde_json::Value,
    ) -> Result<ApprovalRequest> {
        let approval = self
            .approvals
            .get_mut(approval_id)
            .ok_or_else(|| err("unknown approval"))?;
        if approval.status != ApprovalStatus::Pending {
            return Err(err("only pending approvals can be modified"));
        }
        approval.payload = modified_params.clone();
        if let Some(target) = modified_params.get("target").and_then(|v| v.as_str()) {
            approval.target = target.to_owned();
        }
        let (original, proposed, diffs) =
            diff_parts_for_value(modified_params, &approval.payload_kind, &approval.target);
        approval.original_content = original;
        approval.proposed_content = proposed;
        approval.diff_files = diffs;
        approval.summary = format!("{} (modified by reviewer)", approval.summary);
        approval.status = ApprovalStatus::Modified;
        // Modified approvals go back to pending for the policy re-check.
        approval.status = ApprovalStatus::Pending;
        approval.decided_by = None;
        Ok(approval.clone())
    }

    /// Sweep expired pendings; returns the newly-expired ids.
    pub fn expire_due(&mut self, now: &DateTime<Utc>) -> Vec<String> {
        let mut expired = Vec::new();
        for id in &self.order.clone() {
            let is_due = self
                .approvals
                .get(id)
                .map(|a| a.status == ApprovalStatus::Pending && is_expired_at(a, now))
                .unwrap_or(false);
            if is_due {
                if let Some(approval) = self.approvals.get_mut(id) {
                    approval.status = ApprovalStatus::Expired;
                    approval.decided_by = Some("system-expiry".to_owned());
                    expired.push(id.clone());
                }
            }
        }
        expired
    }

    /// Pending approvals in request order (the queue the sheet pages through).
    #[must_use]
    pub fn pending(&self) -> Vec<ApprovalRequest> {
        self.order
            .iter()
            .filter_map(|id| self.approvals.get(id))
            .filter(|a| a.status == ApprovalStatus::Pending)
            .cloned()
            .collect()
    }

    /// All approvals (newest last), for history surfaces.
    #[must_use]
    pub fn all(&self) -> Vec<ApprovalRequest> {
        self.order
            .iter()
            .filter_map(|id| self.approvals.get(id))
            .cloned()
            .collect()
    }

    /// Batch-approve low-risk reversible pendings for a task.
    ///
    /// High-risk / irreversible items always require individual review and
    /// are returned untouched in the second tuple element.
    pub fn batch_approve_low_risk(
        &mut self,
        task_id: &str,
        granted_by: &str,
    ) -> (Vec<ApprovalRequest>, Vec<ApprovalRequest>) {
        let mut approved = Vec::new();
        let mut needs_review = Vec::new();
        for id in &self.order.clone() {
            let Some(approval) = self.approvals.get_mut(id) else {
                continue;
            };
            if approval.task_id != task_id || approval.status != ApprovalStatus::Pending {
                continue;
            }
            let low = approval.risk_level == RiskLevel::Low
                && approval.reversibility == Reversibility::Reversible;
            if low {
                approval.status = ApprovalStatus::Granted;
                approval.decided_by = Some(granted_by.to_owned());
                approved.push(approval.clone());
            } else {
                needs_review.push(approval.clone());
            }
        }
        (approved, needs_review)
    }

    /// Build the ordered review list for a task (Plan mode).
    #[must_use]
    pub fn plan_document(&self, task_id: &str) -> PlanDocument {
        let mut actions: Vec<PlannedAction> = self
            .order
            .iter()
            .filter_map(|id| self.approvals.get(id))
            .filter(|a| a.task_id == task_id)
            .map(|a| PlannedAction {
                approval_id: a.approval_id.clone(),
                description: a.summary.clone(),
                target: a.target.clone(),
                effect: a.effect_class,
                reversibility: a.reversibility,
                risk: a.risk_level,
                status: a.status,
            })
            .collect();
        actions.sort_by(|a, b| a.approval_id.cmp(&b.approval_id));
        let approved_count = actions
            .iter()
            .filter(|a| a.status == ApprovalStatus::Granted)
            .count();
        let pending_count = actions
            .iter()
            .filter(|a| a.status == ApprovalStatus::Pending)
            .count();
        PlanDocument {
            task_id: task_id.to_owned(),
            mode: self.task_mode(task_id),
            actions,
            created_at: now_iso(),
            approved_count,
            pending_count,
        }
    }

    /// Start a compensation execution (CompensationSheet confirm path).
    pub fn start_compensation(
        &mut self,
        plan_id: &str,
        task_id: &str,
        failure_reason: &str,
        steps: Vec<CompensationStepView>,
    ) -> CompensationExecution {
        let execution = CompensationExecution {
            plan_id: plan_id.to_owned(),
            task_id: task_id.to_owned(),
            failure_reason: failure_reason.to_owned(),
            steps,
            steps_completed: 0,
            status: "running".to_owned(),
            started_at: now_iso(),
            finished_at: None,
        };
        self.executions
            .insert(plan_id.to_owned(), execution.clone());
        execution
    }

    /// Advance one compensation step; on failure the task needs manual review.
    pub fn advance_compensation(
        &mut self,
        plan_id: &str,
        ok: bool,
    ) -> Result<CompensationExecution> {
        let execution = self
            .executions
            .get_mut(plan_id)
            .ok_or_else(|| err("unknown compensation plan"))?;
        let next = execution
            .steps
            .iter_mut()
            .find(|s| s.status == "pending" || s.status == "running");
        match next {
            None => {
                execution.status = "completed".to_owned();
                execution.finished_at = Some(now_iso());
            }
            Some(step) => {
                if ok {
                    step.status = "done".to_owned();
                    execution.steps_completed += 1;
                    if execution.steps.iter().all(|s| s.status == "done") {
                        execution.status = "completed".to_owned();
                        execution.finished_at = Some(now_iso());
                    }
                } else {
                    step.status = "failed".to_owned();
                    execution.status = "failed_needs_manual_review".to_owned();
                    execution.finished_at = Some(now_iso());
                }
            }
        }
        Ok(execution.clone())
    }

    /// Fetch a compensation execution by plan id.
    pub fn compensation(&self, plan_id: &str) -> Result<CompensationExecution> {
        self.executions
            .get(plan_id)
            .cloned()
            .ok_or_else(|| err("unknown compensation plan"))
    }
}

fn diff_parts_for(
    request: &ActionRequest,
    payload_kind: &str,
) -> (Option<String>, Option<String>, Vec<DiffFile>) {
    diff_parts_for_value(&request.payload_json, payload_kind, &request.target)
}

fn diff_parts_for_value(
    payload: &serde_json::Value,
    payload_kind: &str,
    target: &str,
) -> (Option<String>, Option<String>, Vec<DiffFile>) {
    if payload_kind != "file_edit" {
        return (None, None, Vec::new());
    }
    let original = payload
        .get("original")
        .or_else(|| payload.get("before"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    let proposed = payload
        .get("proposed")
        .or_else(|| payload.get("after"))
        .or_else(|| payload.get("content"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    let files = payload
        .get("files")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|f| DiffFile {
                    path: f
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(target)
                        .to_owned(),
                    original: f
                        .get("original")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    proposed: f
                        .get("proposed")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let has_single = !original.is_empty() || !proposed.is_empty();
    (
        has_single.then(|| original.clone()),
        has_single.then(|| proposed.clone()),
        if files.is_empty() && has_single {
            vec![DiffFile {
                path: target.to_owned(),
                original,
                proposed,
            }]
        } else {
            files
        },
    )
}

fn compensation_for(
    request: &ActionRequest,
    reversibility: Reversibility,
) -> Vec<CompensationStepView> {
    if reversibility != Reversibility::Irreversible {
        return Vec::new();
    }
    let from_payload = request
        .payload_json
        .get("compensation")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(i, step)| CompensationStepView {
                    order: i,
                    action_type: step
                        .get("action_type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("manual_review")
                        .to_owned(),
                    description: step
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Review and restore manually")
                        .to_owned(),
                    payload: step
                        .get("payload")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    status: "pending".to_owned(),
                })
                .collect::<Vec<_>>()
        });
    from_payload.unwrap_or_else(|| {
        vec![
            CompensationStepView {
                order: 0,
                action_type: "restore_checkpoint".to_owned(),
                description: format!("Restore checkpoint for {}", request.target),
                payload: serde_json::json!({ "target": request.target }),
                status: "pending".to_owned(),
            },
            CompensationStepView {
                order: 1,
                action_type: "manual_review".to_owned(),
                description: "Human verifies the rollback".to_owned(),
                payload: serde_json::json!({}),
                status: "pending".to_owned(),
            },
        ]
    })
}

fn cost_estimate_for(request: &ActionRequest) -> Option<CostEstimate> {
    let tokens = request
        .payload_json
        .get("estimated_tokens")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            let len = request.payload_json.to_string().len() as u64;
            (len > 0).then(|| len / 4 + 120)
        })?;
    Some(CostEstimate {
        tokens,
        estimated_usd: (tokens as f64) * 0.000_002,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_request(id: &str, risk: RiskLevel, reversible: bool) -> ActionRequest {
        ActionRequest {
            id: id.to_owned(),
            agent_id: "agent-1".to_owned(),
            task_id: "task-1".to_owned(),
            capability: Capability::ExecuteCommand,
            intent: "Send this email to 43 recipients".to_owned(),
            target: "work@example.com".to_owned(),
            payload_json: serde_json::json!({
                "command": "send-email --to work@example.com",
                "account": "work@example.com",
            }),
            risk_level: risk,
            reversible,
            requested_at: Utc::now(),
        }
    }

    fn request(store: &mut ApprovalStore, req: &ActionRequest) -> ApprovalRequest {
        store.request_approval(
            req,
            PolicyDecision::RequireHumanReview,
            "high-risk non-reversible action requires human review",
            &["human_review".to_owned()],
        )
    }

    #[test]
    fn request_grant_lifecycle_executes() {
        let mut store = ApprovalStore::new();
        let req = sample_request("req-1", RiskLevel::High, false);
        let approval = request(&mut store, &req);
        assert_eq!(approval.status, ApprovalStatus::Pending);
        assert_eq!(approval.effect_class, EffectClass::Destructive);
        assert!(approval.summary.contains("work@example.com"));
        assert!(!approval.scope.is_empty());
        let granted = store
            .grant(&approval.approval_id, "user", false)
            .expect("grant");
        assert_eq!(granted.status, ApprovalStatus::Granted);
        assert_eq!(granted.decided_by.as_deref(), Some("user"));
    }

    #[test]
    fn deny_blocks_execution_and_marks_task_failed_path() {
        let mut store = ApprovalStore::new();
        let req = sample_request("req-2", RiskLevel::High, false);
        let approval = request(&mut store, &req);
        let denied = store
            .deny(&approval.approval_id, "user", "too risky right now")
            .expect("deny");
        assert_eq!(denied.status, ApprovalStatus::Denied);
        assert_eq!(denied.deny_reason.as_deref(), Some("too risky right now"));
        // Second decision is rejected: the action must not execute twice.
        assert!(store.grant(&approval.approval_id, "user", false).is_err());
    }

    #[test]
    fn approvals_expire_after_timeout() {
        let mut store = ApprovalStore::new();
        let req = sample_request("req-3", RiskLevel::High, false);
        let approval = request(&mut store, &req);
        // Backdate the expiry, then sweep.
        {
            let stored = store.approvals.get_mut(&approval.approval_id).unwrap();
            stored.expires_at = (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
        }
        let expired = store.expire_due(&Utc::now());
        assert_eq!(expired, vec![approval.approval_id.clone()]);
        assert_eq!(
            store.get(&approval.approval_id).unwrap().status,
            ApprovalStatus::Expired
        );
        assert!(store.grant(&approval.approval_id, "user", false).is_err());
    }

    #[test]
    fn plan_mode_holds_execution_until_approved() {
        let mut store = ApprovalStore::new();
        store.set_task_mode("task-1", TaskMode::Plan);
        let req = sample_request("req-4", RiskLevel::High, false);
        let approval = request(&mut store, &req);
        // Plan mode never auto-grants: still pending.
        assert_eq!(approval.status, ApprovalStatus::Pending);
        let doc = store.plan_document("task-1");
        assert_eq!(doc.mode, TaskMode::Plan);
        assert_eq!(doc.actions.len(), 1);
        assert_eq!(doc.pending_count, 1);
        assert_eq!(doc.approved_count, 0);
    }

    #[test]
    fn auto_mode_grants_low_risk_reads_only() {
        let mut store = ApprovalStore::new();
        store.set_task_mode("task-1", TaskMode::Auto);
        let low = ActionRequest {
            capability: Capability::ReadFile,
            risk_level: RiskLevel::Low,
            reversible: true,
            ..sample_request("req-low", RiskLevel::Low, true)
        };
        let granted = request(&mut store, &low);
        assert_eq!(granted.status, ApprovalStatus::Granted);
        let high = sample_request("req-high", RiskLevel::High, false);
        let held = request(&mut store, &high);
        assert_eq!(held.status, ApprovalStatus::Pending);
    }

    #[test]
    fn modify_updates_params_and_returns_to_pending() {
        let mut store = ApprovalStore::new();
        let req = sample_request("req-5", RiskLevel::High, false);
        let approval = request(&mut store, &req);
        let patched = serde_json::json!({
            "command": "send-email --to work@example.com --dry-run",
            "target": "work@example.com",
        });
        let updated = store
            .modify(&approval.approval_id, &patched)
            .expect("modify");
        assert_eq!(updated.status, ApprovalStatus::Pending);
        assert_eq!(updated.payload, patched);
        assert_eq!(updated.target, "work@example.com");
    }

    #[test]
    fn batch_approve_leaves_high_risk_for_individual_review() {
        let mut store = ApprovalStore::new();
        let low = ActionRequest {
            capability: Capability::ReadFile,
            risk_level: RiskLevel::Low,
            reversible: true,
            ..sample_request("req-b1", RiskLevel::Low, true)
        };
        let low_approval =
            store.request_approval(&low, PolicyDecision::RequireHumanReview, "review", &[]);
        let high = sample_request("req-b2", RiskLevel::High, false);
        let _high_approval = request(&mut store, &high);
        let (approved, needs_review) = store.batch_approve_low_risk("task-1", "user");
        // Low-risk read was manually requested (not auto) so it batch-approves.
        assert!(approved
            .iter()
            .any(|a| a.approval_id == low_approval.approval_id));
        assert_eq!(needs_review.len(), 1);
        assert!(needs_review.iter().all(|a| a.risk_level == RiskLevel::High));
    }

    #[test]
    fn compensation_failure_needs_manual_review() {
        let mut store = ApprovalStore::new();
        let steps = vec![CompensationStepView {
            order: 0,
            action_type: "restore_checkpoint".to_owned(),
            description: "Restore".to_owned(),
            payload: serde_json::json!({}),
            status: "pending".to_owned(),
        }];
        store.start_compensation("plan-1", "task-1", "delete failed", steps);
        let failed = store
            .advance_compensation("plan-1", false)
            .expect("advance");
        assert_eq!(failed.status, "failed_needs_manual_review");
    }
}
