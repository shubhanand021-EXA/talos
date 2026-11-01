//! Multi-agent delegation protocol: typed requests, results, and rules.
//!
//! The Supervisor ([`crate::supervisor::SupervisorAgent`]) decomposes a complex
//! task into a DAG of scoped sub-tasks, assigns each to a specialist, and
//! merges the structured results. This module holds the wire types plus the
//! deterministic Batch 3 rules:
//!
//! - Fan-out at most [`MAX_FANOUT`] (default 4) concurrent sub-agents.
//! - Depth at most [`MAX_DEPTH`] levels (root = 1, child = 2; a child at
//!   depth 2 cannot spawn grandchildren).
//! - A child may only NARROW its parent's tool authority (subset check).
//! - Children inherit the parent's policy constraints; the Supervisor can
//!   never override a policy decision for a child.
//! - Parent budgets split equally among children (configurable override).

use talos_common::{Result, TalosError};

/// Maximum concurrent sub-agents per parent (Perplexity §2.6 default 4).
pub const MAX_FANOUT: usize = 4;
/// Maximum delegation depth (root = 1, child = 2). A task at depth
/// [`MAX_DEPTH`] cannot spawn further children.
pub const MAX_DEPTH: usize = 2;
/// Hard ceiling for any configured fan-out (Perplexity §2.6: at most 16).
pub const MAX_FANOUT_LIMIT: usize = 16;
/// Hard ceiling for any configured depth (Perplexity §2.6: at most 4).
pub const MAX_DEPTH_LIMIT: usize = 4;
/// Default concurrent-agent budget when the caller omits one.
pub const DEFAULT_MAX_CONCURRENT_AGENTS: usize = 2;
/// Default per-child context window (tokens) for specialist isolation.
pub const DEFAULT_CHILD_CONTEXT_BUDGET: u32 = 4_000;

/// Specialist role a sub-task is assigned to (mirrors `AgentRole` contracts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpecialistRole {
    /// File/shell work via `tool_runtime`.
    Coder,
    /// Browser automation via `browser-use`.
    Browser,
    /// Information gathering (mock for Batch 3).
    Researcher,
}

impl SpecialistRole {
    /// Stable wire value (`coder` | `browser` | `researcher`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coder => "coder",
            Self::Browser => "browser",
            Self::Researcher => "researcher",
        }
    }

    /// Parse a wire value (case-insensitive).
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "coder" => Ok(Self::Coder),
            "browser" => Ok(Self::Browser),
            "researcher" | "research" => Ok(Self::Researcher),
            _ => Err(TalosError::Storage(format!(
                "unknown specialist role: {value}"
            ))),
        }
    }

    /// Scoped tool allowlist: the only tools this specialist may invoke.
    ///
    /// Each specialist gets a bounded subset of the built-ins; a child
    /// may only narrow this further, never widen it.
    #[must_use]
    pub fn tool_allowlist(self) -> Vec<String> {
        match self {
            Self::Coder => vec![
                "fs.read".to_owned(),
                "fs.list".to_owned(),
                "fs.search".to_owned(),
                "fs.write".to_owned(),
                "fs.edit".to_owned(),
                "fs.create".to_owned(),
                "fs.delete".to_owned(),
                "fs.move".to_owned(),
                "process.exec".to_owned(),
                "code.run_tests".to_owned(),
                "code.install_deps".to_owned(),
                "git.status".to_owned(),
                "git.diff".to_owned(),
                "git.branch".to_owned(),
                "git.commit".to_owned(),
            ],
            Self::Browser => vec![
                "browser.navigate".to_owned(),
                "browser.click".to_owned(),
                "browser.type".to_owned(),
                "browser.scroll".to_owned(),
                "browser.select".to_owned(),
                "browser.extract".to_owned(),
                "browser.screenshot".to_owned(),
                "browser.snapshot".to_owned(),
                "browser.session".to_owned(),
            ],
            Self::Researcher => vec![
                "fs.read".to_owned(),
                "fs.list".to_owned(),
                "fs.search".to_owned(),
                "browser.extract".to_owned(),
                "browser.snapshot".to_owned(),
            ],
        }
    }
}

/// One structured artifact a specialist reports (never a conversation dump).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    /// Stable artifact id (`{child}-artifact-{n}`).
    pub id: String,
    /// Artifact kind (`code_patch` | `browser_snapshot` | `research_note`).
    pub kind: String,
    /// One-line summary.
    pub summary: String,
    /// Content hash (`sha256:…`), when the body is stored as evidence.
    pub hash: Option<String>,
    /// Evidence row holding the body, if persisted.
    pub evidence_id: Option<String>,
}

/// Supervisor → specialist assignment contract.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationRequest {
    /// Parent (Supervisor-owned) task.
    pub parent_task_id: String,
    /// New child task (owned by the specialist).
    pub child_task_id: String,
    /// Specialist role for this sub-task.
    pub agent_role: SpecialistRole,
    /// Bounded scope: what the child may do (never the full parent goal).
    pub scope: String,
    /// Tools the child may invoke (must be a subset of the parent's).
    pub tool_allowlist: Vec<String>,
    /// Isolated context window for the child (tokens).
    pub context_budget: u32,
    /// Per-child deadline (milliseconds).
    pub deadline_ms: i64,
    /// Depth of the child (root = 1, child = 2).
    pub depth: usize,
    /// Reserved USD share carved from the parent budget (`None` = uncapped).
    pub budget_usd: Option<f64>,
}

/// Specialist → Supervisor completion contract.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationResult {
    /// Child task that produced this result.
    pub child_task_id: String,
    /// Terminal child status (`completed` | `failed` | `canceled` | `timed_out`).
    pub status: String,
    /// Structured outputs (artifacts, not conversation dumps).
    pub artifacts: Vec<ArtifactRef>,
    /// Evidence rows backing the result.
    pub evidence_ids: Vec<String>,
    /// What the child could not do (bounded, honest).
    pub limitations: Vec<String>,
    /// Residual risks the parent must account for.
    pub remaining_risks: Vec<String>,
}

/// One planned sub-task from deterministic decomposition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlannedSubtask {
    /// Specialist role for this slice.
    pub role: SpecialistRole,
    /// Bounded scope for the child.
    pub scope: String,
}

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

/// Deterministic Batch 3 decomposition (Grok §2.4, ChatGPT §2.5).
///
/// - goal contains `code` → [`SpecialistRole::Coder`]
/// - goal contains `browser` or `web` → [`SpecialistRole::Browser`]
/// - goal contains `research` → [`SpecialistRole::Researcher`]
/// - multiple domains → one sub-task per matching domain
/// - no domain keyword → single [`SpecialistRole::Researcher`] slice
///
/// Future (Batch 4): LLM-driven DAG decomposition.
#[must_use]
pub fn decompose_task(goal: &str, description: Option<&str>) -> Vec<PlannedSubtask> {
    let haystack = format!("{} {}", goal, description.unwrap_or_default()).to_ascii_lowercase();
    let mut out = Vec::new();
    if haystack.contains("code") {
        out.push(PlannedSubtask {
            role: SpecialistRole::Coder,
            scope: format!("code slice of: {}", goal.trim()),
        });
    }
    if haystack.contains("browser") || haystack.contains("web") {
        out.push(PlannedSubtask {
            role: SpecialistRole::Browser,
            scope: format!("browser slice of: {}", goal.trim()),
        });
    }
    if haystack.contains("research") {
        out.push(PlannedSubtask {
            role: SpecialistRole::Researcher,
            scope: format!("research slice of: {}", goal.trim()),
        });
    }
    if out.is_empty() {
        out.push(PlannedSubtask {
            role: SpecialistRole::Researcher,
            scope: format!("research slice of: {}", goal.trim()),
        });
    }
    out.truncate(MAX_FANOUT);
    out
}

/// True when `child` narrows (is a subset of) `parent` authority.
#[must_use]
pub fn is_narrower_authority(parent_allowlist: &[String], child_allowlist: &[String]) -> bool {
    child_allowlist
        .iter()
        .all(|tool| parent_allowlist.iter().any(|p| p == tool))
}

/// Validate a delegation: fan-out, depth, and narrowing.
///
/// - `existing_children` is the parent's current child count.
/// - `parent_depth` is 1 for roots, 2 for children.
/// - `parent_allowlist` is the authority the parent holds.
/// - `max_fanout` / `max_depth` come from the owning [`SupervisorConfig`]
///   (never the module constants: per-supervisor configuration is enforced).
pub fn validate_delegation(
    existing_children: usize,
    parent_depth: usize,
    parent_allowlist: &[String],
    request: &DelegationRequest,
    max_fanout: usize,
    max_depth: usize,
) -> Result<()> {
    if existing_children >= max_fanout {
        return Err(err(&format!(
            "fan-out limit exceeded ({max_fanout} children max)"
        )));
    }
    if parent_depth >= max_depth {
        return Err(err(&format!(
            "depth limit exceeded (max depth {max_depth}; child cannot spawn grandchildren)"
        )));
    }
    if request.depth != parent_depth + 1 {
        return Err(err("delegation depth must be parent depth + 1"));
    }
    if request.depth > max_depth {
        return Err(err(&format!("child depth exceeds max {max_depth}")));
    }
    if !is_narrower_authority(parent_allowlist, &request.tool_allowlist) {
        return Err(err(
            "child authority must narrow the parent (subset of parent tools)",
        ));
    }
    if request.child_task_id.trim().is_empty() || request.child_task_id.len() > 128 {
        return Err(err("child_task_id must be 1..=128 chars"));
    }
    if request.scope.trim().is_empty() || request.scope.len() > 2048 {
        return Err(err("scope must be 1..=2048 chars"));
    }
    if !(100..=3_600_000).contains(&request.deadline_ms) {
        return Err(err("deadline_ms out of range"));
    }
    Ok(())
}

/// Split a parent budget equally among `n` children (Batch 3 default).
///
/// Returns `n` identical shares (`None` stays `None` = uncapped).
#[must_use]
pub fn split_budget_equal(parent_budget_usd: Option<f64>, n: usize) -> Vec<Option<f64>> {
    if n == 0 {
        return Vec::new();
    }
    match parent_budget_usd {
        None => vec![None; n],
        Some(total) => {
            let share = (total / n as f64 * 1_000_000.0).round() / 1_000_000.0;
            vec![Some(share); n]
        }
    }
}

/// Merge child results into a parent outcome (all succeed → parent succeeds).
///
/// Returns `(parent_status, merged_artifacts)`: `completed` only when every
/// child is `completed`; otherwise `failed` with the failure detail preserved
/// by the caller in `last_error`.
#[must_use]
pub fn merge_results(results: &[DelegationResult]) -> (String, Vec<ArtifactRef>) {
    let mut artifacts = Vec::new();
    for result in results {
        artifacts.extend(result.artifacts.clone());
    }
    if results.is_empty() {
        return ("failed".to_owned(), artifacts);
    }
    if results.iter().all(|r| r.status == "completed") {
        ("completed".to_owned(), artifacts)
    } else {
        ("failed".to_owned(), artifacts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposes_code_goal_to_coder() {
        let planned = decompose_task("fix the code in fs.write", None);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].role, SpecialistRole::Coder);
    }

    #[test]
    fn decomposes_multi_domain_to_multiple() {
        let planned = decompose_task("code the page and browser test the web flow", None);
        let roles: Vec<SpecialistRole> = planned.iter().map(|p| p.role).collect();
        assert!(roles.contains(&SpecialistRole::Coder));
        assert!(roles.contains(&SpecialistRole::Browser));
    }

    #[test]
    fn decomposes_research_keyword() {
        let planned = decompose_task("research the registry options", None);
        assert_eq!(planned[0].role, SpecialistRole::Researcher);
    }

    #[test]
    fn respects_fan_out_limit() {
        let roles = vec![
            PlannedSubtask {
                role: SpecialistRole::Coder,
                scope: "s".to_owned(),
            };
            10
        ];
        assert!(roles.len() > MAX_FANOUT);
        let planned = decompose_task("code browser web research code browser web", None);
        assert!(planned.len() <= MAX_FANOUT);
    }

    #[test]
    fn narrower_authority_subset_check() {
        let parent = vec!["fs.read".to_owned(), "fs.write".to_owned()];
        assert!(is_narrower_authority(&parent, &["fs.read".to_owned()]));
        assert!(!is_narrower_authority(
            &parent,
            &["browser.click".to_owned()]
        ));
        assert!(is_narrower_authority(&parent, &[]));
    }

    #[test]
    fn rejects_wider_child_authority() {
        let parent = vec!["fs.read".to_owned()];
        let request = DelegationRequest {
            parent_task_id: "p".to_owned(),
            child_task_id: "c".to_owned(),
            agent_role: SpecialistRole::Browser,
            scope: "browse".to_owned(),
            tool_allowlist: vec!["browser.click".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
            depth: 2,
            budget_usd: None,
        };
        assert!(validate_delegation(0, 1, &parent, &request, MAX_FANOUT, MAX_DEPTH).is_err());
    }

    #[test]
    fn enforces_fan_out_and_depth() {
        let parent = SpecialistRole::Coder.tool_allowlist();
        let request = |child: &str, depth: usize| DelegationRequest {
            parent_task_id: "p".to_owned(),
            child_task_id: child.to_owned(),
            agent_role: SpecialistRole::Coder,
            scope: "code".to_owned(),
            tool_allowlist: vec!["fs.read".to_owned()],
            context_budget: 4000,
            deadline_ms: 30_000,
            depth,
            budget_usd: Some(0.5),
        };
        assert!(validate_delegation(
            MAX_FANOUT,
            1,
            &parent,
            &request("c", 2),
            MAX_FANOUT,
            MAX_DEPTH
        )
        .is_err());
        assert!(
            validate_delegation(0, 2, &parent, &request("c", 3), MAX_FANOUT, MAX_DEPTH).is_err()
        );
        assert!(
            validate_delegation(0, 1, &parent, &request("c", 2), MAX_FANOUT, MAX_DEPTH).is_ok()
        );
    }

    #[test]
    fn splits_budget_equally() {
        assert_eq!(split_budget_equal(None, 2), vec![None, None]);
        assert_eq!(
            split_budget_equal(Some(1.0), 4),
            vec![Some(0.25), Some(0.25), Some(0.25), Some(0.25)]
        );
        assert!(split_budget_equal(Some(1.0), 0).is_empty());
    }

    #[test]
    fn merging_all_success_completes_parent() {
        let ok = |id: &str| DelegationResult {
            child_task_id: id.to_owned(),
            status: "completed".to_owned(),
            artifacts: vec![ArtifactRef {
                id: format!("{id}-a1"),
                kind: "research_note".to_owned(),
                summary: "done".to_owned(),
                hash: None,
                evidence_id: None,
            }],
            evidence_ids: vec![],
            limitations: vec![],
            remaining_risks: vec![],
        };
        let (status, artifacts) = merge_results(&[ok("c1"), ok("c2")]);
        assert_eq!(status, "completed");
        assert_eq!(artifacts.len(), 2);
        let (failed, _) = merge_results(&[
            ok("c1"),
            DelegationResult {
                status: "failed".to_owned(),
                ..ok("c2")
            },
        ]);
        assert_eq!(failed, "failed");
    }
}
