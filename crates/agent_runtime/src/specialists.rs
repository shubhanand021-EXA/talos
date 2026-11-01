//! Specialist agents: scoped, bounded, artifact-reporting workers.
//!
//! Each specialist has a fixed tool allowlist (a subset of the 23 built-ins),
//! a bounded context window (it never sees other agents' work), and reports
//! structured [`crate::delegation::ArtifactRef`]s instead of conversation
//! dumps. Real tool execution lands in later batches; Batch 3 stubs produce
//! deterministic mock artifacts so delegation, merging, and policy gating can
//! be verified end to end.

use crate::delegation::{ArtifactRef, DelegationResult, SpecialistRole};

/// Isolated context window per specialist (tokens).
pub const SPECIALIST_CONTEXT_BUDGET: u32 = 4_000;

/// Behaviour every specialist implements (Batch 3 mock execution).
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
    /// Deterministic mock execution: one artifact, no I/O.
    fn execute(&self) -> DelegationResult;
}

fn artifact_for(role: SpecialistRole, child_task_id: &str, scope: &str) -> ArtifactRef {
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
    }
}

fn ok_result(role: SpecialistRole, child_task_id: &str, scope: &str) -> DelegationResult {
    DelegationResult {
        child_task_id: child_task_id.to_owned(),
        status: "completed".to_owned(),
        artifacts: vec![artifact_for(role, child_task_id, scope)],
        evidence_ids: Vec::new(),
        limitations: vec![format!("{} mock: no live tools in Batch 3", role.as_str())],
        remaining_risks: vec!["mock output requires human review before use".to_owned()],
    }
}

/// File/shell specialist (`fs.*`, `process.exec` subset).
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
    fn execute(&self) -> DelegationResult {
        ok_result(SpecialistRole::Coder, &self.task_id, &self.scope)
    }
}

/// Browser specialist (`browser.*` subset).
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
    fn execute(&self) -> DelegationResult {
        ok_result(SpecialistRole::Browser, &self.task_id, &self.scope)
    }
}

/// Research specialist (read-only + extract subset, mock gathering).
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
    fn execute(&self) -> DelegationResult {
        ok_result(SpecialistRole::Researcher, &self.task_id, &self.scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn researcher_reports_structured_artifact() {
        let agent = ResearchAgent::new("a-3", "t-3", "gather", 4000);
        let result = agent.execute();
        assert_eq!(result.status, "completed");
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].kind, "research_note");
        assert!(!result.limitations.is_empty());
    }

    #[test]
    fn each_role_reports_distinct_kind() {
        let coder = CoderAgent::new("a-c", "t-c", "s", 4000).execute();
        let browser = BrowserAgent::new("a-b", "t-b", "s", 4000).execute();
        assert_eq!(coder.artifacts[0].kind, "code_patch");
        assert_eq!(browser.artifacts[0].kind, "browser_snapshot");
    }
}
