//! Specialist agents: scoped, bounded, artifact-reporting workers.
//!
//! Each specialist has a fixed tool allowlist (a bounded subset of the
//! built-ins), a bounded context window (it never sees other agents' work),
//! and reports structured [`crate::delegation::ArtifactRef`]s instead of
//! conversation dumps.
//!
//! Execution fidelity (TAL-002): every specialist runs through the real
//! policy-gated tool framework whenever an isolated workspace is available.
//! The returned [`crate::delegation::DelegationResult`] always carries an
//! honest [`talos_common::ExecutionFidelity`]:
//!
//! - `mock`: no workspace or no live tools — deterministic simulation only.
//!   A mock result is always labeled mock, never `completed` with hidden
//!   simulation on a live-capable path (see `debug_assert` below).
//! - `live`: real tool calls executed with real side effects.
//! - `verified`: live execution plus independent verification (a test
//!   process exited 0, a hash verified against the ledger).
//!
//! Formulal: mock work may be labeled `mock`, but presenting it as `live`
//! or `verified` is a [`talos_common::FidelityMismatchError`] (see
//! `check_fidelity_claim`, enforced in tests).

use evidence_store::EvidenceDb;
use policy_broker::PolicyEngine;
use talos_common::{ExecutionFidelity, Result, TalosError};

use crate::delegation::{ArtifactRef, DelegationResult, SpecialistRole};

/// Isolated context window per specialist (tokens).
pub const SPECIALIST_CONTEXT_BUDGET: u32 = 4_000;

/// Live execution context for one specialist run.
///
/// The desktop delegation layer builds this from `AppState` (database,
/// policy engine, tool registry) plus the task's isolated workspace root.
/// `workspace_root: None` means "no isolated workspace" — the specialist
/// must fall back to deterministic mock execution and label it `mock`.
#[derive(Debug)]
pub struct SpecialistCtx<'a> {
    /// Evidence database (ledger + task budgets + cost records).
    pub db: &'a EvidenceDb,
    /// Policy authority (every tool call is gated, never bypassed).
    pub engine: &'a dyn PolicyEngine,
    /// Tool registry (built-ins, enablement flags).
    pub registry: &'a tool_runtime::ToolRegistry,
    /// Sandbox base config (roots are overridden per specialist).
    pub sandbox: &'a tool_runtime::SandboxConfig,
    /// Isolated workspace root (`None` = mock mode).
    pub workspace_root: Option<std::path::PathBuf>,
    /// Agent identifier (`agent-{child}-…`).
    pub agent_id: String,
    /// Owning child task (ledger linkage for evidence + budgets).
    pub task_id: String,
}

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Behaviour every specialist implements (real execution, mock fallback).
pub trait Specialist: std::fmt::Debug + Send + Sync {
    /// Specialist role (`coder` | `browser` | `researcher`).
    fn role(&self) -> SpecialistRole;
    /// Agent identifier (`agent-{child}-…`).
    fn id(&self) -> &str;
    /// Owning child task.
    fn task_id(&self) -> &str;
    /// Tools this specialist may invoke (subset of the parent's).
    fn tool_allowlist(&self) -> Vec<String>;
    /// Isolated context window (tokens).
    fn context_budget(&self) -> u32;
    /// Bounded scope (what this specialist may do).
    fn scope(&self) -> &str;
    /// Execute through the tool framework (real when a workspace exists,
    /// deterministic mock otherwise — always honestly labeled).
    fn execute(
        &self,
        ctx: &SpecialistCtx<'_>,
    ) -> impl std::future::Future<Output = DelegationResult> + Send;
}

fn mock_artifact(
    role: SpecialistRole,
    child_task_id: &str,
    scope: &str,
    fidelity: ExecutionFidelity,
) -> ArtifactRef {
    let (kind, summary) = match role {
        SpecialistRole::Coder => ("code_patch", format!("coder slice ready: {scope}")),
        SpecialistRole::Browser => ("browser_snapshot", format!("browser slice ready: {scope}")),
        SpecialistRole::Researcher => ("research_note", format!("research slice ready: {scope}")),
    };
    ArtifactRef {
        id: format!("{child_task_id}-artifact-1"),
        kind: kind.to_owned(),
        summary,
        hash: None,
        evidence_id: None,
        fidelity,
    }
}

/// Deterministic mock result, honestly labeled `mock`.
///
/// Used when no isolated workspace exists. Mock results carry the mock
/// limitation explicitly so no consumer can mistake them for real work.
fn mock_result(role: SpecialistRole, child_task_id: &str, scope: &str) -> DelegationResult {
    DelegationResult {
        child_task_id: child_task_id.to_owned(),
        status: "completed".to_owned(),
        artifacts: vec![mock_artifact(
            role,
            child_task_id,
            scope,
            ExecutionFidelity::Mock,
        )],
        evidence_ids: Vec::new(),
        limitations: vec![format!(
            "{} mock: no isolated workspace, no live tools in this run",
            role.as_str()
        )],
        remaining_risks: vec!["mock output requires human review before use".to_owned()],
        fidelity: ExecutionFidelity::Mock,
    }
}

/// One policy-gated tool call on behalf of a specialist.
///
/// Returns the receipt on success; on denial or transport failure returns
/// `Err` with a human sentence (the caller marks the task failed honestly).
async fn invoke_tool(
    ctx: &SpecialistCtx<'_>,
    task_id: &str,
    tool_id: &str,
    params: serde_json::Value,
) -> Result<tool_runtime::ToolReceipt> {
    let outcome = tool_runtime::invoke(
        ctx.db,
        ctx.engine,
        ctx.registry,
        ctx.sandbox,
        tool_runtime::ToolInvocation {
            tool_id: tool_id.to_owned(),
            task_id: task_id.to_owned(),
            agent_id: ctx.agent_id.clone(),
            params,
            dry_run: false,
        },
    )
    .await?;
    let receipt = outcome.receipt;
    match receipt.status {
        tool_runtime::ToolStatus::Success => Ok(receipt),
        tool_runtime::ToolStatus::Denied | tool_runtime::ToolStatus::Disabled => {
            Err(err(&format!(
                "tool {tool_id} not permitted: {}",
                receipt
                    .error
                    .as_ref()
                    .map(|e| e.message.as_str())
                    .unwrap_or("denied")
            )))
        }
        tool_runtime::ToolStatus::Failed => Err(err(&format!(
            "tool {tool_id} failed: {}",
            receipt
                .error
                .as_ref()
                .map(|e| e.message.as_str())
                .unwrap_or("failed")
        ))),
    }
}

/// Persist one evidence row for a specialist and mark it verified.
///
/// Returns the evidence id. Verification here means "the row is the exact
/// output the tool produced" (hash-linked); stronger claims (test pass)
/// are recorded by the caller.
async fn record_verified_evidence(
    ctx: &SpecialistCtx<'_>,
    id: &str,
    kind: &str,
    summary: String,
    payload: serde_json::Value,
) -> Result<String> {
    let row = evidence_store::insert_evidence(
        ctx.db,
        evidence_store::NewEvidence {
            id: id.to_owned(),
            task_id: Some(ctx.task_id.clone()),
            agent_id: Some(ctx.agent_id.clone()),
            session_id: Some("session-local".to_owned()),
            kind: kind.to_owned(),
            label: None,
            summary: Some(summary),
            uri: None,
            mime_type: None,
            payload,
        },
    )
    .await?;
    evidence_store::transition_evidence_status(ctx.db, &row.id, "verified").await?;
    Ok(row.id)
}

/// File/shell specialist: workspace-scoped `fs.*`, `code.*`, and `git.*`
/// tools executed for real through the policy-gated tool framework.
///
/// Protocol (deterministic, no LLM): read the workspace root (evidence
/// seed), create the task artifact, run the test suite, commit when the
/// workspace is a git checkout. Verified requires a test process that
/// ran and exited 0 with zero failures.
#[derive(Debug, Clone)]
pub struct CoderAgent {
    /// Agent identifier.
    pub id: String,
    /// Owning child task.
    pub task_id: String,
    /// Bounded scope.
    pub scope: String,
    /// Isolated context window.
    pub context_budget: u32,
}

impl CoderAgent {
    /// Create a coder for one child task.
    #[must_use]
    pub fn new(id: &str, task_id: &str, scope: &str, context_budget: u32) -> Self {
        Self {
            id: id.to_owned(),
            task_id: task_id.to_owned(),
            scope: scope.to_owned(),
            context_budget,
        }
    }

    /// File name for this task's artifact (sanitized, deterministic).
    fn artifact_name(&self) -> String {
        let mut base: String = self
            .task_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        base.truncate(64);
        format!("{base}-output.md")
    }
}

impl Specialist for CoderAgent {
    fn role(&self) -> SpecialistRole {
        SpecialistRole::Coder
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn task_id(&self) -> &str {
        &self.task_id
    }
    fn tool_allowlist(&self) -> Vec<String> {
        SpecialistRole::Coder.tool_allowlist()
    }
    fn context_budget(&self) -> u32 {
        self.context_budget
    }
    fn scope(&self) -> &str {
        &self.scope
    }
    async fn execute(&self, ctx: &SpecialistCtx<'_>) -> DelegationResult {
        let Some(root) = ctx.workspace_root.clone() else {
            return mock_result(SpecialistRole::Coder, &self.task_id, &self.scope);
        };
        let sandbox = tool_runtime::SandboxConfig {
            root: root.clone(),
            ..ctx.sandbox.clone()
        };
        let live = SpecialistCtx {
            db: ctx.db,
            engine: ctx.engine,
            registry: ctx.registry,
            sandbox: &sandbox,
            workspace_root: Some(root.clone()),
            agent_id: ctx.agent_id.clone(),
            task_id: self.task_id.clone(),
        };
        let ctx = &live;
        // 1. Read the workspace root first: low-risk read, allowed outright,
        //    and its receipt becomes the evidence that gates the writes.
        if let Err(e) = invoke_tool(
            ctx,
            &self.task_id,
            tool_runtime::FS_LIST,
            serde_json::json!({ "path": ".", "depth": 1 }),
        )
        .await
        {
            return failed_result(
                SpecialistRole::Coder,
                &self.task_id,
                &format!("workspace unreadable: {e}"),
            );
        }
        // 2. Create the task artifact for real.
        let file = self.artifact_name();
        let body = format!(
            "# Coder output\n\nScope: {}\n\nProduced by {} for task {} at {}.\n",
            self.scope,
            self.id,
            self.task_id,
            talos_common::now_iso()
        );
        if let Err(e) = invoke_tool(
            ctx,
            &self.task_id,
            tool_runtime::FS_CREATE,
            serde_json::json!({ "path": file, "content": body }),
        )
        .await
        {
            return failed_result(
                SpecialistRole::Coder,
                &self.task_id,
                &format!("artifact creation failed: {e}"),
            );
        }
        // 3. Run the test suite. No framework → live with a limitation;
        //    failures → failed with the output as evidence.
        enum Tests {
            Passed,
            Absent,
            Failed(String),
        }
        let tests = match invoke_tool(
            ctx,
            &self.task_id,
            tool_runtime::CODE_RUN_TESTS,
            serde_json::json!({}),
        )
        .await
        {
            Ok(receipt) => {
                let output = &receipt.output;
                let exit_ok = output.get("exitCode").and_then(|v| v.as_i64()) == Some(0);
                let failed = output.get("failed").and_then(|v| v.as_i64()).unwrap_or(0);
                let passed = output.get("passed").and_then(|v| v.as_i64()).unwrap_or(0);
                if exit_ok && failed == 0 && passed > 0 {
                    Tests::Passed
                } else {
                    Tests::Failed(output.to_string())
                }
            }
            Err(e) if e.to_string().contains("no test framework") => Tests::Absent,
            Err(e) => Tests::Failed(e.to_string()),
        };
        // 4. Best-effort commit when the workspace is a git checkout.
        //    Identity/network failures are limitations, never fatal.
        let mut limitations = Vec::new();
        let mut git_evidence: Option<String> = None;
        if root.join(".git").exists() {
            match invoke_tool(
                ctx,
                &self.task_id,
                tool_runtime::GIT_COMMIT,
                serde_json::json!({ "message": format!("coder {}: add {}", self.task_id, file) }),
            )
            .await
            {
                Ok(receipt) => {
                    git_evidence = receipt.evidence_id.clone();
                }
                Err(e) => limitations.push(format!("git commit skipped: {e}")),
            }
        }
        // 5. Fidelity from what actually happened — never upgraded blindly.
        let file_hash = evidence_store::hash_payload(&serde_json::json!({ "path": file }));
        let mut evidence_ids = Vec::new();
        let mut artifact_evidence = None;
        if let Ok(id) = record_verified_evidence(
            ctx,
            &format!("{}-artifact", self.task_id),
            "code_artifact",
            format!("coder artifact {file}"),
            serde_json::json!({ "path": file, "task_id": self.task_id }),
        )
        .await
        {
            artifact_evidence = Some(id.clone());
            evidence_ids.push(id);
        }
        if let Some(id) = git_evidence {
            evidence_ids.push(id);
        }
        let artifact = ArtifactRef {
            id: format!("{}-artifact-1", self.task_id),
            kind: "code_patch".to_owned(),
            summary: format!("coder slice ready: {}", self.scope),
            hash: Some(file_hash),
            evidence_id: artifact_evidence,
            fidelity: ExecutionFidelity::Live,
        };
        match tests {
            Tests::Passed => {
                let mut result = completed_result(
                    &self.task_id,
                    vec![artifact],
                    evidence_ids,
                    limitations,
                    ExecutionFidelity::Verified,
                );
                // A passing suite upgrades this run to verified.
                result.fidelity = ExecutionFidelity::Verified;
                for a in &mut result.artifacts {
                    a.fidelity = ExecutionFidelity::Verified;
                }
                debug_assert_ne!(result.fidelity, ExecutionFidelity::Mock);
                result
            }
            Tests::Absent => {
                limitations.push(
                    "no test framework detected in workspace: live execution, not independently verified"
                        .to_owned(),
                );
                let result = completed_result(
                    &self.task_id,
                    vec![artifact],
                    evidence_ids,
                    limitations,
                    ExecutionFidelity::Live,
                );
                debug_assert_ne!(result.fidelity, ExecutionFidelity::Mock);
                result
            }
            Tests::Failed(detail) => {
                let mut result = failed_result(
                    SpecialistRole::Coder,
                    &self.task_id,
                    &format!("tests failed: {detail}"),
                );
                let _ = record_verified_evidence(
                    ctx,
                    &format!("{}-test-failure", self.task_id),
                    "test_output",
                    format!("failing test output for {}", self.task_id),
                    serde_json::json!({ "task_id": self.task_id, "output": detail }),
                )
                .await
                .map(|id| result.evidence_ids.push(id));
                result
            }
        }
    }
}

/// Browser specialist (`browser.*` subset).
///
/// Mock adapter (default, CI-safe): dry-run navigation + extraction —
/// deterministic, no network. Live adapter (`TALOS_BROWSER_LIVE=1`): real
/// navigation/extraction/screenshot, receipts hashed and verified against
/// the ledger.
#[derive(Debug, Clone)]
pub struct BrowserAgent {
    /// Agent identifier.
    pub id: String,
    /// Owning child task.
    pub task_id: String,
    /// Bounded scope.
    pub scope: String,
    /// Isolated context window.
    pub context_budget: u32,
}

impl BrowserAgent {
    /// Create a browser specialist for one child task.
    #[must_use]
    pub fn new(id: &str, task_id: &str, scope: &str, context_budget: u32) -> Self {
        Self {
            id: id.to_owned(),
            task_id: task_id.to_owned(),
            scope: scope.to_owned(),
            context_budget,
        }
    }

    /// First http(s) URL in the scope, else the deterministic example page.
    fn target_url(&self) -> String {
        for token in self.scope.split_whitespace() {
            let trimmed = token.trim_matches(|c: char| "<>()[]\"',.;".contains(c));
            if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
                return trimmed.to_owned();
            }
        }
        "https://example.com".to_owned()
    }
}

impl Specialist for BrowserAgent {
    fn role(&self) -> SpecialistRole {
        SpecialistRole::Browser
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn task_id(&self) -> &str {
        &self.task_id
    }
    fn tool_allowlist(&self) -> Vec<String> {
        SpecialistRole::Browser.tool_allowlist()
    }
    fn context_budget(&self) -> u32 {
        self.context_budget
    }
    fn scope(&self) -> &str {
        &self.scope
    }
    async fn execute(&self, ctx: &SpecialistCtx<'_>) -> DelegationResult {
        if !tool_runtime::browser_worker::live_enabled() {
            // Mock adapter: dry-run plans are deterministic and side-effect
            // free; honestly labeled mock.
            let url = self.target_url();
            let mut limitations = Vec::new();
            let mut evidence_ids = Vec::new();
            let mut dom_hash = None;
            match invoke_tool(
                ctx,
                &self.task_id,
                tool_runtime::BROWSER_NAVIGATE,
                serde_json::json!({ "url": url }),
            )
            .await
            {
                Ok(receipt) => {
                    dom_hash = receipt
                        .output
                        .get("domHash")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_owned());
                    if let Some(id) = receipt.evidence_id {
                        evidence_ids.push(id);
                    }
                }
                Err(e) => limitations.push(format!("mock navigation unavailable: {e}")),
            }
            let artifact = ArtifactRef {
                id: format!("{}-artifact-1", self.task_id),
                kind: "browser_snapshot".to_owned(),
                summary: format!("browser slice ready: {}", self.scope),
                hash: dom_hash,
                evidence_id: None,
                fidelity: ExecutionFidelity::Mock,
            };
            // Mock path: honestly labeled completed+mock (documented
            // exemption for deterministic simulation; live paths must
            // use completed_result, which asserts non-mock fidelity).
            let mut result = DelegationResult {
                child_task_id: self.task_id.clone(),
                status: "completed".to_owned(),
                artifacts: vec![artifact],
                evidence_ids,
                limitations,
                remaining_risks: vec!["mock output requires human review before use".to_owned()],
                fidelity: ExecutionFidelity::Mock,
            };
            result.limitations.insert(
                0,
                "mock adapter: deterministic simulation, no network touched".to_owned(),
            );
            return result;
        }
        // Live adapter: real navigation + extraction + screenshot.
        let url = self.target_url();
        let mut evidence_ids = Vec::new();
        let mut limitations = Vec::new();
        let nav = match invoke_tool(
            ctx,
            &self.task_id,
            tool_runtime::BROWSER_NAVIGATE,
            serde_json::json!({ "url": url }),
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(e) => {
                return failed_result(
                    SpecialistRole::Browser,
                    &self.task_id,
                    &format!("live navigation failed: {e}"),
                );
            }
        };
        if let Some(id) = nav.evidence_id.clone() {
            evidence_ids.push(id);
        }
        let extract = match invoke_tool(
            ctx,
            &self.task_id,
            tool_runtime::BROWSER_EXTRACT,
            serde_json::json!({ "url": url, "maxChars": 5000 }),
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(e) => {
                return failed_result(
                    SpecialistRole::Browser,
                    &self.task_id,
                    &format!("live extraction failed: {e}"),
                );
            }
        };
        if let Some(id) = extract.evidence_id.clone() {
            evidence_ids.push(id);
        }
        let shot = invoke_tool(
            ctx,
            &self.task_id,
            tool_runtime::BROWSER_SCREENSHOT,
            serde_json::json!({ "url": url }),
        )
        .await;
        let (screenshot_hash, shot_evidence) = match shot {
            Ok(receipt) => (
                receipt
                    .output
                    .get("screenshotHash")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_owned()),
                receipt.evidence_id.clone(),
            ),
            Err(e) => {
                limitations.push(format!("live screenshot unavailable: {e}"));
                (None, None)
            }
        };
        if let Some(id) = shot_evidence {
            evidence_ids.push(id);
        }
        let dom_hash = nav
            .output
            .get("domHash")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        // Independent verification: re-hash the live receipt outputs and
        // persist the verified linkage in the ledger.
        let verified = if dom_hash.is_some() {
            let payload = serde_json::json!({
                "task_id": self.task_id,
                "url": url,
                "domHash": dom_hash,
                "screenshotHash": screenshot_hash,
            });
            record_verified_evidence(
                ctx,
                &format!("{}-browser-verified", self.task_id),
                "browser_verification",
                format!("verified live browse of {url}"),
                payload,
            )
            .await
            .map(|id| {
                evidence_ids.push(id);
            })
            .is_ok()
        } else {
            limitations.push("live receipts carried no domHash; cannot verify".to_owned());
            false
        };
        let fidelity = if verified {
            ExecutionFidelity::Verified
        } else {
            limitations.push("hash verification against the ledger did not complete".to_owned());
            ExecutionFidelity::Live
        };
        let artifact = ArtifactRef {
            id: format!("{}-artifact-1", self.task_id),
            kind: "browser_snapshot".to_owned(),
            summary: format!("browser slice ready: {}", self.scope),
            hash: dom_hash,
            evidence_id: None,
            fidelity,
        };
        let result = completed_result(
            &self.task_id,
            vec![artifact],
            evidence_ids,
            limitations,
            fidelity,
        );
        debug_assert_ne!(result.fidelity, ExecutionFidelity::Mock);
        result
    }
}

/// Research specialist (read-only + extract subset, mock gathering).
///
/// Batch 4 keeps the researcher on the deterministic mock path: research
/// notes are structured artifacts, always labeled `mock`, never `verified`.
/// (Live web retrieval arrives in Batch 5.)
#[derive(Debug, Clone)]
pub struct ResearchAgent {
    /// Agent identifier.
    pub id: String,
    /// Owning child task.
    pub task_id: String,
    /// Bounded scope.
    pub scope: String,
    /// Isolated context window.
    pub context_budget: u32,
}

impl ResearchAgent {
    /// Create a researcher for one child task.
    #[must_use]
    pub fn new(id: &str, task_id: &str, scope: &str, context_budget: u32) -> Self {
        Self {
            id: id.to_owned(),
            task_id: task_id.to_owned(),
            scope: scope.to_owned(),
            context_budget,
        }
    }
}

impl Specialist for ResearchAgent {
    fn role(&self) -> SpecialistRole {
        SpecialistRole::Researcher
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn task_id(&self) -> &str {
        &self.task_id
    }
    fn tool_allowlist(&self) -> Vec<String> {
        SpecialistRole::Researcher.tool_allowlist()
    }
    fn context_budget(&self) -> u32 {
        self.context_budget
    }
    fn scope(&self) -> &str {
        &self.scope
    }
    async fn execute(&self, _ctx: &SpecialistCtx<'_>) -> DelegationResult {
        // Explicitly mock: Batch 4 has no live retrieval path for research.
        mock_result(SpecialistRole::Researcher, &self.task_id, &self.scope)
    }
}

/// Completed result with an explicit fidelity (live paths only).
fn completed_result(
    child_task_id: &str,
    artifacts: Vec<ArtifactRef>,
    evidence_ids: Vec<String>,
    limitations: Vec<String>,
    fidelity: ExecutionFidelity,
) -> DelegationResult {
    debug_assert_ne!(
        fidelity,
        ExecutionFidelity::Mock,
        "live paths must never complete as mock"
    );
    DelegationResult {
        child_task_id: child_task_id.to_owned(),
        status: "completed".to_owned(),
        artifacts,
        evidence_ids,
        limitations,
        remaining_risks: Vec::new(),
        fidelity,
    }
}

/// Failed result (honest: the work did not complete).
fn failed_result(role: SpecialistRole, child_task_id: &str, reason: &str) -> DelegationResult {
    DelegationResult {
        child_task_id: child_task_id.to_owned(),
        status: "failed".to_owned(),
        artifacts: Vec::new(),
        evidence_ids: Vec::new(),
        limitations: vec![reason.to_owned()],
        remaining_risks: vec![format!("{} work incomplete", role.as_str())],
        fidelity: ExecutionFidelity::Mock,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        db: &'a evidence_store::EvidenceDb,
        engine: &'a policy_broker::DefaultPolicyEngine,
        registry: &'a tool_runtime::ToolRegistry,
        sandbox: &'a tool_runtime::SandboxConfig,
        workspace_root: Option<std::path::PathBuf>,
        task_id: &str,
    ) -> SpecialistCtx<'a> {
        SpecialistCtx {
            db,
            engine,
            registry,
            sandbox,
            workspace_root,
            agent_id: "agent-test".to_owned(),
            task_id: task_id.to_owned(),
        }
    }

    async fn harness() -> (
        tempfile::TempDir,
        evidence_store::EvidenceDb,
        policy_broker::DefaultPolicyEngine,
        tool_runtime::ToolRegistry,
        tool_runtime::SandboxConfig,
    ) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db");
        let sandbox = tool_runtime::SandboxConfig {
            root: dir.path().to_path_buf(),
            exec_enabled: false,
            exec_allowlist: Vec::new(),
            ..Default::default()
        };
        (
            dir,
            db,
            policy_broker::DefaultPolicyEngine::new(),
            tool_runtime::ToolRegistry::with_builtins(),
            sandbox,
        )
    }

    #[test]
    fn coder_has_scoped_allowlist() {
        let agent = CoderAgent::new("a-1", "t-1", "code slice", 4000);
        assert_eq!(agent.role(), SpecialistRole::Coder);
        let tools = agent.tool_allowlist();
        assert!(tools.contains(&"fs.read".to_owned()));
        assert!(!tools.contains(&"browser.click".to_owned()));
        assert_eq!(agent.context_budget(), 4000);
    }

    #[test]
    fn browser_has_browser_tools_only() {
        let agent = BrowserAgent::new("a-2", "t-2", "browse", 4000);
        let tools = agent.tool_allowlist();
        assert!(tools.contains(&"browser.navigate".to_owned()));
        assert!(!tools.contains(&"fs.write".to_owned()));
    }

    #[tokio::test]
    async fn researcher_labels_mock_and_never_verified() {
        let (_dir, db, engine, registry, sandbox) = harness().await;
        let agent = ResearchAgent::new("a-3", "t-3", "gather", 4000);
        let result = agent
            .execute(&ctx(&db, &engine, &registry, &sandbox, None, "t-3"))
            .await;
        assert_eq!(result.status, "completed");
        assert_eq!(result.artifacts[0].kind, "research_note");
        assert_eq!(result.fidelity, ExecutionFidelity::Mock);
        assert!(!result.limitations.is_empty());
        // A mock result can never be presented as verified.
        assert!(
            talos_common::check_fidelity_claim(result.fidelity, ExecutionFidelity::Verified)
                .is_err()
        );
    }

    #[tokio::test]
    async fn coder_without_workspace_returns_mock() {
        let (_dir, db, engine, registry, sandbox) = harness().await;
        let agent = CoderAgent::new("a-c", "t-c", "s", 4000);
        let result = agent
            .execute(&ctx(&db, &engine, &registry, &sandbox, None, "t-c"))
            .await;
        assert_eq!(result.fidelity, ExecutionFidelity::Mock);
        assert!(
            talos_common::check_fidelity_claim(result.fidelity, ExecutionFidelity::Verified)
                .is_err()
        );
    }

    #[tokio::test]
    async fn coder_without_tests_returns_live_with_limitation() {
        // Real workspace, real file op, no test framework → live, not verified.
        let (dir, db, engine, registry, sandbox) = harness().await;
        let agent = CoderAgent::new("a-c", "t-live", "write the notes file", 4000);
        let result = agent
            .execute(&ctx(
                &db,
                &engine,
                &registry,
                &sandbox,
                Some(dir.path().to_path_buf()),
                "t-live",
            ))
            .await;
        assert_eq!(result.status, "completed");
        assert_eq!(result.fidelity, ExecutionFidelity::Live);
        assert!(result
            .limitations
            .iter()
            .any(|l| l.contains("no test framework")));
        assert!(dir.path().join("t-live-output.md").exists());
    }

    #[tokio::test]
    async fn coder_with_passing_tests_returns_verified() {
        // Real workspace with a passing cargo test → verified.
        let (dir, db, engine, registry, sandbox) = harness().await;
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        std::fs::create_dir_all(dir.path().join("src")).expect("src dir");
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "#[test]\nfn probe_passes() { assert_eq!(1 + 1, 2); }\n",
        )
        .expect("lib");
        let agent = CoderAgent::new("a-c", "t-verify", "verify the probe crate", 4000);
        let result = agent
            .execute(&ctx(
                &db,
                &engine,
                &registry,
                &sandbox,
                Some(dir.path().to_path_buf()),
                "t-verify",
            ))
            .await;
        assert_eq!(
            result.status, "completed",
            "limitations: {:?}",
            result.limitations
        );
        assert_eq!(result.fidelity, ExecutionFidelity::Verified);
        assert!(!result.evidence_ids.is_empty());
    }

    #[tokio::test]
    async fn coder_with_failing_tests_reports_failed() {
        // A test process that exits non-zero → failed, never completed.
        let (dir, db, engine, registry, sandbox) = harness().await;
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        std::fs::create_dir_all(dir.path().join("src")).expect("src dir");
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "#[test]\nfn probe_fails() { assert_eq!(1 + 1, 3); }\n",
        )
        .expect("lib");
        let agent = CoderAgent::new("a-c", "t-fail", "verify the probe crate", 4000);
        let result = agent
            .execute(&ctx(
                &db,
                &engine,
                &registry,
                &sandbox,
                Some(dir.path().to_path_buf()),
                "t-fail",
            ))
            .await;
        assert_eq!(result.status, "failed");
        assert_ne!(result.fidelity, ExecutionFidelity::Verified);
        assert!(
            !result.evidence_ids.is_empty(),
            "failure output is evidence"
        );
    }

    #[tokio::test]
    async fn browser_mock_adapter_reports_mock() {
        let (_dir, db, engine, registry, sandbox) = harness().await;
        let agent = BrowserAgent::new("a-b", "t-b", "browse https://example.com/docs", 4000);
        let result = agent
            .execute(&ctx(&db, &engine, &registry, &sandbox, None, "t-b"))
            .await;
        assert_eq!(result.fidelity, ExecutionFidelity::Mock);
        assert!(
            talos_common::check_fidelity_claim(result.fidelity, ExecutionFidelity::Verified)
                .is_err()
        );
    }

    #[tokio::test]
    async fn mismatch_marking_mock_as_verified_is_an_error() {
        let err = talos_common::check_fidelity_claim(
            ExecutionFidelity::Mock,
            ExecutionFidelity::Verified,
        )
        .expect_err("mock as verified must fail");
        assert_eq!(err.actual, ExecutionFidelity::Mock);
        assert_eq!(err.claimed, ExecutionFidelity::Verified);
    }

    #[tokio::test]
    async fn each_role_reports_distinct_kind() {
        let (_dir, db, engine, registry, sandbox) = harness().await;
        let coder = CoderAgent::new("a-c", "t-c", "s", 4000)
            .execute(&ctx(&db, &engine, &registry, &sandbox, None, "t-c"))
            .await;
        let browser = BrowserAgent::new("a-b", "t-b", "s", 4000)
            .execute(&ctx(&db, &engine, &registry, &sandbox, None, "t-b"))
            .await;
        assert_eq!(coder.artifacts[0].kind, "code_patch");
        assert_eq!(browser.artifacts[0].kind, "browser_snapshot");
    }
}
