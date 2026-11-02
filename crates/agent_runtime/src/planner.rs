//! LLM-driven DAG planning with deterministic policy gates.
//!
//! The Supervisor sends the task goal to an LLM, receives a structured DAG
//! plan, and validates it deterministically before delegating to specialists.
//! LLM output is untrusted: schema, budgets, graph depth, fan-out, tool
//! scopes, and dependencies are all re-checked here. Any failure falls back
//! to the deterministic keyword decomposition in [`crate::delegation`]
//! (clearly labeled [`PlanningMethod::Keyword`]).
//!
//! [`PlanningPromptTemplate`] is deterministic and versioned
//! ([`PLANNING_PROMPT_VERSION`]). [`validate_plan`] enforces every gate.
//! [`topological_order`] / [`execution_levels`] drive DAG execution.

use crate::delegation::{decompose_task, PlannedSubtask, SpecialistRole};

/// Version of the planning prompt template (bump on any prompt change).
pub const PLANNING_PROMPT_VERSION: &str = "1.0";

/// Plan schema version the LLM must emit (must equal `"1.0"`).
pub const PLAN_SCHEMA_VERSION: &str = "1.0";

/// System framing for every planning call (deterministic, versioned).
pub const PLANNING_SYSTEM_PROMPT: &str = "You are the Talos Supervisor planner. Output exactly one JSON DAG object matching the requested schema and nothing else.";

/// Default planning model when the caller does not pin one.
pub const DEFAULT_PLANNING_MODEL: &str = "gpt-4o-mini";

/// Planning LLM timeout (30s per requirement).
pub const PLANNING_TIMEOUT_MS: u64 = 30_000;

/// How the plan was produced (always labeled, never hidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanningMethod {
    /// An LLM produced the DAG and it passed every deterministic gate.
    Llm,
    /// Deterministic keyword decomposition (no key, timeout, invalid JSON,
    /// or rejected plan).
    Keyword,
}

impl PlanningMethod {
    /// Stable wire value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Keyword => "keyword",
        }
    }
}

/// Step risk tier (mirrors the plan schema).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanRiskLevel {
    /// Read-only / reversible work.
    Low,
    /// Write-like work, still reversible.
    Medium,
    /// Irreversible or sensitive work: requires human approval.
    High,
}

impl PlanRiskLevel {
    /// Parse a wire value (case-insensitive).
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(format!("unknown risk_level: {other}")),
        }
    }
}

/// One LLM-proposed DAG step (strict schema: unknown fields rejected).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanStep {
    /// Stable step id (`1..=64` chars, unique within the plan).
    pub step_id: String,
    /// Specialist role for this step.
    pub role: SpecialistRole,
    /// Bounded goal for the child (`1..=2048` chars).
    pub goal: String,
    /// Step ids that must complete before this step runs.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Tool ids this step needs (must exist in the registry).
    #[serde(default)]
    pub tools_needed: Vec<String>,
    /// Risk tier (`high` requires human approval).
    pub risk_level: PlanRiskLevel,
    /// Estimated completion tokens (`1..=128_000`).
    pub estimated_tokens: u32,
}

/// The strict JSON schema the LLM must emit (unknown fields rejected).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmDagPlan {
    /// Must equal [`PLAN_SCHEMA_VERSION`].
    pub plan_version: String,
    /// Proposed steps (at least one).
    pub steps: Vec<PlanStep>,
    /// Sum of `estimated_tokens` (must match the steps).
    pub total_estimated_tokens: u64,
    /// Free-form rationale (never executed).
    #[serde(default)]
    pub reasoning: String,
}

/// Runtime planning settings (IPC-visible, camelCase).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningConfig {
    /// Model id for planning calls.
    pub model: String,
    /// Planning LLM timeout in milliseconds.
    pub timeout_ms: u64,
    /// When true, any planning failure falls back to keyword decomposition.
    pub fallback_enabled: bool,
}

impl Default for PlanningConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_PLANNING_MODEL.to_owned(),
            timeout_ms: PLANNING_TIMEOUT_MS,
            fallback_enabled: true,
        }
    }
}

impl PlanningConfig {
    /// Validate ranges (model `1..=128`, timeout `1_000..=120_000`).
    pub fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty() || self.model.len() > 128 {
            return Err("model must be 1..=128 chars".to_owned());
        }
        if !(1_000..=120_000).contains(&self.timeout_ms) {
            return Err("timeout_ms must be 1000..=120000".to_owned());
        }
        Ok(())
    }
}

/// Input to the planning prompt template.
#[derive(Debug, Clone)]
pub struct PlanningPromptInput<'a> {
    /// The task goal.
    pub goal: &'a str,
    /// Optional task detail.
    pub description: Option<&'a str>,
    /// Available tools (Tool-RAG top-K), sorted for determinism.
    pub available_tools: &'a [String],
    /// Task spend cap in USD (`None` = uncapped).
    pub task_budget_usd: Option<f64>,
    /// Max hierarchy depth (root = 1).
    pub max_depth: usize,
    /// Max concurrent steps.
    pub max_fanout: usize,
}

/// Deterministic, versioned planning prompt template.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanningPromptTemplate {
    _private: (),
}

impl PlanningPromptTemplate {
    /// Create the template (version is fixed).
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Template version (equals [`PLANNING_PROMPT_VERSION`]).
    #[must_use]
    pub fn version(&self) -> &'static str {
        PLANNING_PROMPT_VERSION
    }

    /// Render the planning prompt deterministically.
    ///
    /// Includes the task goal, specialist roles and capabilities, available
    /// tools, budget constraints, safety constraints (max depth, max
    /// fan-out), and the strict output specification. Tool ids are sorted
    /// so identical inputs always produce identical prompts.
    #[must_use]
    pub fn render(&self, input: &PlanningPromptInput<'_>) -> String {
        let mut tools: Vec<&str> = input.available_tools.iter().map(String::as_str).collect();
        tools.sort_unstable();
        let budget = match input.task_budget_usd {
            Some(cap) => format!("${cap:.6} USD"),
            None => "uncapped".to_owned(),
        };
        let description = input.description.unwrap_or("(none)");
        format!(
            "You are the Talos Supervisor planner (template version {version}).\n\
             Decompose the task goal into a dependency graph (DAG) of specialist steps.\n\
             \n\
             TASK GOAL: {goal}\n\
             TASK DESCRIPTION: {description}\n\
             \n\
             SPECIALIST ROLES AND CAPABILITIES:\n\
             - coder: file/shell work. tools: fs.read, fs.list, fs.search, fs.write, fs.edit, fs.create, fs.delete, fs.move, process.exec, code.run_tests, code.install_deps, git.status, git.diff, git.branch, git.commit\n\
             - browser: browser automation. tools: browser.navigate, browser.click, browser.type, browser.scroll, browser.select, browser.extract, browser.screenshot, browser.snapshot, browser.session\n\
             - researcher: information gathering (read-only). tools: fs.read, fs.list, fs.search, browser.extract, browser.snapshot\n\
             \n\
             AVAILABLE TOOLS (top-K from Tool-RAG; every tools_needed entry must be one of these):\n\
             {tools}\n\
             \n\
             BUDGET CONSTRAINTS: task budget {budget}; total_estimated_tokens must fit the budget.\n\
             SAFETY CONSTRAINTS: max DAG depth {max_depth} (longest dependency chain), max concurrent steps (fan-out) {max_fanout}.\n\
             Every step's tools_needed must be a subset of its role's capability list above.\n\
             A risk_level of high requires human approval before execution.\n\
             \n\
             OUTPUT SPECIFICATION: reply with exactly one JSON object and nothing else (no markdown fences, no commentary):\n\
             {{\n  \
               \"plan_version\": \"1.0\",\n  \
               \"steps\": [\n    \
                 {{\"step_id\": \"string (unique, 1..=64 chars)\", \"role\": \"coder\" | \"browser\" | \"research\", \"goal\": \"string (1..=2048 chars)\", \"depends_on\": [\"step_id\"], \"tools_needed\": [\"tool_id\"], \"risk_level\": \"low\" | \"medium\" | \"high\", \"estimated_tokens\": number}},\n    \
                 ...\n  \
               ],\n  \
               \"total_estimated_tokens\": number (must equal the sum of estimated_tokens),\n  \
               \"reasoning\": \"string (short rationale, never executed)\"\n\
             }}\n\
             Rules: every depends_on entry must reference an existing step_id; no circular dependencies; at least one step; step goals must be scoped slices of the task goal, never the full goal.",
            version = PLANNING_PROMPT_VERSION,
            goal = input.goal.trim(),
            description = description,
            tools = tools.join(", "),
            budget = budget,
            max_depth = input.max_depth,
            max_fanout = input.max_fanout,
        )
    }

    /// Correction prompt for the single retry after a parse/validation failure.
    #[must_use]
    pub fn correction_prompt(&self, first_output: &str, error: &str) -> String {
        format!(
            "Your previous planning output (template version {version}) was rejected: {error}\n\
             Previous output:\n{first_output}\n\
             Reply with exactly one corrected JSON object matching the output specification and nothing else.",
            version = PLANNING_PROMPT_VERSION,
            error = error,
        )
    }
}

/// Deterministic validation context (policy gates, never LLM output).
#[derive(Debug, Clone)]
pub struct ValidationContext {
    /// Max DAG depth (longest dependency chain, root = 1).
    pub max_depth: usize,
    /// Max concurrent steps (widest execution level).
    pub max_fanout: usize,
    /// Tool ids known to the registry (every `tools_needed` must be listed).
    pub allowed_tools: Vec<String>,
    /// Token budget cap (`None` = uncapped).
    pub token_budget: Option<u64>,
}

/// A plan that passed every deterministic gate.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedPlan {
    /// The validated plan (unmodified).
    pub plan: LlmDagPlan,
    /// Topological execution order (step ids).
    pub order: Vec<String>,
    /// Parallel execution levels (each level runs concurrently).
    pub levels: Vec<Vec<String>>,
    /// True when any step is high-risk (human approval required).
    pub requires_approval: bool,
}

/// Deterministic plan rejection (machine-readable reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRejection {
    /// Short machine-readable code (`schema`, `dependency`, `depth`,
    /// `fanout`, `tool`, `budget`).
    pub code: String,
    /// Human detail for evidence + UI.
    pub reason: String,
}

impl PlanRejection {
    fn new(code: &str, reason: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for PlanRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.reason)
    }
}

/// Extract raw JSON from an LLM response (strips markdown fences).
fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();
    for fence in ["```json", "```"] {
        if let Some(rest) = trimmed.strip_prefix(fence) {
            let body = rest.trim();
            let body = body.strip_suffix("```").unwrap_or(body).trim();
            return body.to_owned();
        }
    }
    trimmed.to_owned()
}

/// Parse one LLM response into a [`LlmDagPlan`] (strict schema).
pub fn parse_plan_json(raw: &str) -> Result<LlmDagPlan, PlanRejection> {
    let json = extract_json(raw);
    serde_json::from_str::<LlmDagPlan>(&json).map_err(|err| {
        PlanRejection::new(
            "schema",
            format!("plan JSON failed strict schema validation: {err}"),
        )
    })
}

/// Validate a parsed plan against every deterministic policy gate.
pub fn validate_plan(
    plan: &LlmDagPlan,
    ctx: &ValidationContext,
) -> Result<ValidatedPlan, PlanRejection> {
    if plan.plan_version != PLAN_SCHEMA_VERSION {
        return Err(PlanRejection::new(
            "schema",
            format!(
                "plan_version must be \"{PLAN_SCHEMA_VERSION}\" (got {:?})",
                plan.plan_version
            ),
        ));
    }
    if plan.steps.is_empty() {
        return Err(PlanRejection::new(
            "schema",
            "plan must contain at least one step",
        ));
    }
    // Step-level schema validation.
    let mut ids = std::collections::HashSet::new();
    for step in &plan.steps {
        if step.step_id.trim().is_empty() || step.step_id.len() > 64 {
            return Err(PlanRejection::new(
                "schema",
                format!("step_id must be 1..=64 chars (got {:?})", step.step_id),
            ));
        }
        if !ids.insert(step.step_id.clone()) {
            return Err(PlanRejection::new(
                "schema",
                format!("duplicate step_id: {:?}", step.step_id),
            ));
        }
        if step.goal.trim().is_empty() || step.goal.len() > 2048 {
            return Err(PlanRejection::new(
                "schema",
                format!("step {:?} goal must be 1..=2048 chars", step.step_id),
            ));
        }
        if step.depends_on.iter().any(|d| d == &step.step_id) {
            return Err(PlanRejection::new(
                "dependency",
                format!("step {:?} depends on itself", step.step_id),
            ));
        }
        if step.estimated_tokens == 0 || step.estimated_tokens > 128_000 {
            return Err(PlanRejection::new(
                "schema",
                format!(
                    "step {:?} estimated_tokens must be 1..=128000",
                    step.step_id
                ),
            ));
        }
        for tool in &step.tools_needed {
            if !ctx.allowed_tools.iter().any(|t| t == tool) {
                return Err(PlanRejection::new(
                    "tool",
                    format!("step {:?} needs unknown tool: {tool}", step.step_id),
                ));
            }
        }
        let role_tools = step.role.tool_allowlist();
        for tool in &step.tools_needed {
            if !role_tools.iter().any(|t| t == tool) {
                return Err(PlanRejection::new(
                    "tool",
                    format!(
                        "step {:?} tool {tool} is outside the {:?} role scope",
                        step.step_id, step.role,
                    ),
                ));
            }
        }
    }
    // Dependency validation: every edge must reference an existing step.
    for step in &plan.steps {
        for dep in &step.depends_on {
            if !ids.contains(dep) {
                return Err(PlanRejection::new(
                    "dependency",
                    format!("step {:?} depends on unknown step {:?}", step.step_id, dep),
                ));
            }
        }
    }
    // Total must match the sum of the parts (budget honesty).
    let sum: u64 = plan
        .steps
        .iter()
        .map(|s| u64::from(s.estimated_tokens))
        .sum();
    if plan.total_estimated_tokens != sum {
        return Err(PlanRejection::new(
            "budget",
            format!(
                "total_estimated_tokens {} does not equal the step sum {sum}",
                plan.total_estimated_tokens
            ),
        ));
    }
    // Topological order (also detects cycles) + levels.
    let (order, levels) = topo_sort(plan)?;
    // Depth validation: longest chain must not exceed max_depth.
    let depth = longest_chain(plan, &order);
    if depth > ctx.max_depth {
        return Err(PlanRejection::new(
            "depth",
            format!("plan depth {depth} exceeds max_depth {}", ctx.max_depth),
        ));
    }
    // Fan-out validation: widest concurrent level must fit max_fanout.
    let width = levels.iter().map(Vec::len).max().unwrap_or(0);
    if width > ctx.max_fanout {
        return Err(PlanRejection::new(
            "fanout",
            format!("plan fan-out {width} exceeds max_fanout {}", ctx.max_fanout),
        ));
    }
    // Budget validation.
    if let Some(cap) = ctx.token_budget {
        if plan.total_estimated_tokens > cap {
            return Err(PlanRejection::new(
                "budget",
                format!(
                    "total_estimated_tokens {} exceeds budget {cap}",
                    plan.total_estimated_tokens
                ),
            ));
        }
    }
    let requires_approval = plan
        .steps
        .iter()
        .any(|s| s.risk_level == PlanRiskLevel::High);
    Ok(ValidatedPlan {
        plan: plan.clone(),
        order,
        levels,
        requires_approval,
    })
}

/// Kahn topological sort; `Err` on circular dependencies.
fn topo_sort(plan: &LlmDagPlan) -> Result<(Vec<String>, Vec<Vec<String>>), PlanRejection> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut indegree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for step in &plan.steps {
        indegree.insert(step.step_id.as_str(), step.depends_on.len());
        dependents.entry(step.step_id.as_str()).or_default();
    }
    for step in &plan.steps {
        for dep in &step.depends_on {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(step.step_id.as_str());
        }
    }
    // Deterministic: BTreeSet pops the smallest ready id first.
    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut order: Vec<String> = Vec::with_capacity(plan.steps.len());
    let mut levels: Vec<Vec<String>> = Vec::new();
    while !ready.is_empty() {
        let level: Vec<String> = ready.iter().map(|s| (*s).to_owned()).collect();
        levels.push(level.clone());
        let mut next: BTreeSet<&str> = BTreeSet::new();
        for id in std::mem::take(&mut ready) {
            order.push(id.to_owned());
            if let Some(children) = dependents.get(id) {
                for child in children {
                    let entry = indegree.get_mut(child).expect("child tracked");
                    *entry -= 1;
                    if *entry == 0 {
                        next.insert(child);
                    }
                }
            }
        }
        ready = next;
    }
    if order.len() != plan.steps.len() {
        return Err(PlanRejection::new(
            "dependency",
            "circular dependency detected in plan graph",
        ));
    }
    Ok((order, levels))
}

/// Longest dependency chain (node count) over a topological order.
fn longest_chain(plan: &LlmDagPlan, order: &[String]) -> usize {
    use std::collections::HashMap;
    let by_id: HashMap<&str, &PlanStep> =
        plan.steps.iter().map(|s| (s.step_id.as_str(), s)).collect();
    let mut depth: HashMap<&str, usize> = HashMap::new();
    let mut max = 0;
    for id in order {
        let step = by_id[id.as_str()];
        let parent = step
            .depends_on
            .iter()
            .map(|d| depth.get(d.as_str()).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let mine = parent + 1;
        depth.insert(id.as_str(), mine);
        max = max.max(mine);
    }
    max
}

/// Topological execution order of step ids (`Err` on cycles).
pub fn topological_order(plan: &LlmDagPlan) -> Result<Vec<String>, PlanRejection> {
    Ok(topo_sort(plan)?.0)
}

/// Parallel execution levels (`Err` on cycles).
pub fn execution_levels(plan: &LlmDagPlan) -> Result<Vec<Vec<String>>, PlanRejection> {
    Ok(topo_sort(plan)?.1)
}

/// All transitive dependents of `failed_step_id` (cancel these on failure).
#[must_use]
pub fn failed_dependents(plan: &LlmDagPlan, failed_step_id: &str) -> Vec<String> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for step in &plan.steps {
        for dep in &step.depends_on {
            children
                .entry(dep.as_str())
                .or_default()
                .push(step.step_id.as_str());
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::from([failed_step_id]);
    while let Some(id) = queue.pop_front() {
        if let Some(next) = children.get(id) {
            for child in next {
                if seen.insert((*child).to_owned()) {
                    queue.push_back(child);
                }
            }
        }
    }
    let mut out: Vec<String> = seen.into_iter().collect();
    out.sort();
    out
}

/// One DAG step translated into the keyword-style slice the delegation flow
/// already executes (deterministic child ids come from the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagPlannedStep {
    /// Plan step id (for dependency tracking + child id derivation).
    pub step_id: String,
    /// Specialist role.
    pub role: SpecialistRole,
    /// Bounded scope (the step goal).
    pub scope: String,
    /// Step ids that must complete first.
    pub depends_on: Vec<String>,
    /// Validated tool needs (subset of the role allowlist).
    pub tools_needed: Vec<String>,
    /// Whether this step needs human approval.
    pub requires_approval: bool,
    /// Estimated completion tokens.
    pub estimated_tokens: u32,
}

/// Outcome of one planning pass (LLM or keyword fallback).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedDag {
    /// How the plan was produced (always labeled).
    pub method: PlanningMethod,
    /// Executable steps in topological order.
    pub steps: Vec<DagPlannedStep>,
    /// Parallel execution levels (step ids).
    pub levels: Vec<Vec<String>>,
    /// True when any step is high-risk.
    pub requires_approval: bool,
    /// Request hash (`sha256:…`) for evidence, when an LLM call ran.
    pub request_hash: Option<String>,
    /// Response hash (`sha256:…`) for evidence, when an LLM call ran.
    pub response_hash: Option<String>,
    /// Rejection reason when the LLM plan failed and keyword fallback ran.
    pub fallback_reason: Option<String>,
}

impl PlannedDag {
    /// Convert to the legacy [`PlannedSubtask`] slices (dependency order).
    #[must_use]
    pub fn as_slices(&self) -> Vec<PlannedSubtask> {
        self.steps
            .iter()
            .map(|s| PlannedSubtask {
                role: s.role,
                scope: s.scope.clone(),
            })
            .collect()
    }
}

/// Build a keyword-fallback [`PlannedDag`] (no dependencies, one level).
#[must_use]
pub fn keyword_fallback_dag(
    goal: &str,
    description: Option<&str>,
    fallback_reason: Option<String>,
) -> PlannedDag {
    let slices = decompose_task(goal, description);
    let steps = slices
        .into_iter()
        .enumerate()
        .map(|(i, slice)| DagPlannedStep {
            step_id: format!("kw-{}", i + 1),
            role: slice.role,
            scope: slice.scope,
            depends_on: Vec::new(),
            tools_needed: Vec::new(),
            requires_approval: false,
            estimated_tokens: 0,
        })
        .collect::<Vec<_>>();
    let level = steps.iter().map(|s| s.step_id.clone()).collect::<Vec<_>>();
    PlannedDag {
        method: PlanningMethod::Keyword,
        steps,
        levels: vec![level],
        requires_approval: false,
        request_hash: None,
        response_hash: None,
        fallback_reason,
    }
}

/// Convert a validated LLM plan into an executable [`PlannedDag`].
#[must_use]
pub fn validated_to_dag(
    validated: &ValidatedPlan,
    request_hash: Option<String>,
    response_hash: Option<String>,
) -> PlannedDag {
    let by_id: std::collections::HashMap<&str, &PlanStep> = validated
        .plan
        .steps
        .iter()
        .map(|s| (s.step_id.as_str(), s))
        .collect();
    let steps = validated
        .order
        .iter()
        .map(|id| {
            let step = by_id[id.as_str()];
            DagPlannedStep {
                step_id: step.step_id.clone(),
                role: step.role,
                scope: step.goal.clone(),
                depends_on: step.depends_on.clone(),
                tools_needed: step.tools_needed.clone(),
                requires_approval: step.risk_level == PlanRiskLevel::High,
                estimated_tokens: step.estimated_tokens,
            }
        })
        .collect();
    PlannedDag {
        method: PlanningMethod::Llm,
        steps,
        levels: validated.levels.clone(),
        requires_approval: validated.requires_approval,
        request_hash,
        response_hash,
        fallback_reason: None,
    }
}

/// Hash one planning payload for evidence (`sha256:…`, never the content).
#[must_use]
pub fn hash_planning_payload(payload: &serde_json::Value) -> String {
    evidence_store::hash_payload(payload)
}

/// Evidence payload for a planning pass (metadata only, never prompts).
#[must_use]
pub fn planning_evidence_payload(
    task_id: &str,
    method: PlanningMethod,
    request_hash: Option<&str>,
    response_hash: Option<&str>,
    fallback_reason: Option<&str>,
    requires_approval: bool,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "task_id": task_id,
        "planning_method": method.as_str(),
        "template_version": PLANNING_PROMPT_VERSION,
        "requires_approval": requires_approval,
    });
    if let Some(hash) = request_hash {
        payload["request_hash"] = serde_json::Value::String(hash.to_owned());
    }
    if let Some(hash) = response_hash {
        payload["response_hash"] = serde_json::Value::String(hash.to_owned());
    }
    if let Some(reason) = fallback_reason {
        payload["fallback_reason"] = serde_json::Value::String(reason.to_owned());
    }
    payload
}

/// Serialize a validated plan as the `plan_artifact` evidence body.
#[must_use]
pub fn plan_artifact(plan: &LlmDagPlan, method: PlanningMethod) -> serde_json::Value {
    serde_json::json!({
        "plan_version": plan.plan_version,
        "planning_method": method.as_str(),
        "template_version": PLANNING_PROMPT_VERSION,
        "total_estimated_tokens": plan.total_estimated_tokens,
        "steps": plan.steps.iter().map(|s| serde_json::json!({
            "step_id": s.step_id,
            "role": s.role.as_str(),
            "goal": s.goal,
            "depends_on": s.depends_on,
            "tools_needed": s.tools_needed,
            "risk_level": format!("{:?}", s.risk_level).to_ascii_lowercase(),
            "estimated_tokens": s.estimated_tokens,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed_tools() -> Vec<String> {
        let mut tools = SpecialistRole::Coder.tool_allowlist();
        tools.extend(SpecialistRole::Browser.tool_allowlist());
        tools.extend(SpecialistRole::Researcher.tool_allowlist());
        tools.sort();
        tools.dedup();
        tools
    }

    fn ctx() -> ValidationContext {
        ValidationContext {
            max_depth: 4,
            max_fanout: 4,
            allowed_tools: allowed_tools(),
            token_budget: None,
        }
    }

    fn step(id: &str, role: SpecialistRole, deps: &[&str]) -> PlanStep {
        PlanStep {
            step_id: id.to_owned(),
            role,
            goal: format!("do {id}"),
            depends_on: deps.iter().map(|s| (*s).to_owned()).collect(),
            tools_needed: vec!["fs.read".to_owned()],
            risk_level: PlanRiskLevel::Low,
            estimated_tokens: 100,
        }
    }

    fn plan(steps: Vec<PlanStep>) -> LlmDagPlan {
        let total = steps.iter().map(|s| u64::from(s.estimated_tokens)).sum();
        LlmDagPlan {
            plan_version: "1.0".to_owned(),
            steps,
            total_estimated_tokens: total,
            reasoning: "test".to_owned(),
        }
    }

    #[test]
    fn prompt_template_is_deterministic_and_versioned() {
        let template = PlanningPromptTemplate::new();
        assert_eq!(template.version(), "1.0");
        let tools = vec!["browser.extract".to_owned(), "fs.read".to_owned()];
        let input = PlanningPromptInput {
            goal: "summarize the repo",
            description: None,
            available_tools: &tools,
            task_budget_usd: Some(1.0),
            max_depth: 4,
            max_fanout: 4,
        };
        let first = template.render(&input);
        let reversed = vec!["fs.read".to_owned(), "browser.extract".to_owned()];
        let second = template.render(&PlanningPromptInput {
            available_tools: &reversed,
            ..input
        });
        assert_eq!(first, second);
        assert!(first.contains("1.0"));
        assert!(first.contains("summarize the repo"));
        assert!(first.contains("\"plan_version\": \"1.0\""));
    }

    #[test]
    fn llm_planning_produces_valid_dag() {
        let raw = serde_json::json!({
            "plan_version": "1.0",
            "steps": [
                {"step_id": "a", "role": "researcher", "goal": "gather context", "depends_on": [], "tools_needed": ["fs.read"], "risk_level": "low", "estimated_tokens": 100},
                {"step_id": "b", "role": "coder", "goal": "apply the fix", "depends_on": ["a"], "tools_needed": ["fs.read"], "risk_level": "medium", "estimated_tokens": 200}
            ],
            "total_estimated_tokens": 300,
            "reasoning": "research then code"
        })
        .to_string();
        let parsed = parse_plan_json(&raw).expect("parses");
        let validated = validate_plan(&parsed, &ctx()).expect("valid");
        assert_eq!(validated.order, vec!["a", "b"]);
        assert_eq!(validated.levels, vec![vec!["a"], vec!["b"]]);
        assert!(!validated.requires_approval);
        // Markdown fences are tolerated.
        let fenced = format!("```json\n{raw}\n```");
        assert!(parse_plan_json(&fenced).is_ok());
    }

    #[test]
    fn invalid_json_is_a_schema_rejection() {
        assert!(parse_plan_json("not json at all").is_err());
        let wrong_version = serde_json::json!({
            "plan_version": "2.0", "steps": [], "total_estimated_tokens": 0
        })
        .to_string();
        let parsed = parse_plan_json(&wrong_version).expect("shape parses");
        let err = validate_plan(&parsed, &ctx()).unwrap_err();
        assert_eq!(err.code, "schema");
    }

    #[test]
    fn circular_dependency_detected_and_rejected() {
        let bad = plan(vec![
            step("a", SpecialistRole::Coder, &["b"]),
            step("b", SpecialistRole::Coder, &["a"]),
        ]);
        let err = validate_plan(&bad, &ctx()).unwrap_err();
        assert_eq!(err.code, "dependency");
        assert!(err.reason.contains("circular"));
    }

    #[test]
    fn missing_dependency_rejected() {
        let bad = plan(vec![step("a", SpecialistRole::Coder, &["ghost"])]);
        let err = validate_plan(&bad, &ctx()).unwrap_err();
        assert_eq!(err.code, "dependency");
    }

    #[test]
    fn depth_limit_enforced_on_llm_plan() {
        let deep = plan(vec![
            step("a", SpecialistRole::Researcher, &[]),
            step("b", SpecialistRole::Coder, &["a"]),
            step("c", SpecialistRole::Coder, &["b"]),
        ]);
        let tight = ValidationContext {
            max_depth: 2,
            ..ctx()
        };
        let err = validate_plan(&deep, &tight).unwrap_err();
        assert_eq!(err.code, "depth");
    }

    #[test]
    fn fanout_limit_enforced_on_llm_plan() {
        let wide = plan(vec![
            step("a", SpecialistRole::Coder, &[]),
            step("b", SpecialistRole::Coder, &[]),
            step("c", SpecialistRole::Coder, &[]),
        ]);
        let tight = ValidationContext {
            max_fanout: 2,
            ..ctx()
        };
        let err = validate_plan(&wide, &tight).unwrap_err();
        assert_eq!(err.code, "fanout");
    }

    #[test]
    fn unknown_tool_in_plan_rejected() {
        let mut bad = step("a", SpecialistRole::Coder, &[]);
        bad.tools_needed = vec!["nope.tool".to_owned()];
        let err = validate_plan(&plan(vec![bad]), &ctx()).unwrap_err();
        assert_eq!(err.code, "tool");
    }

    #[test]
    fn out_of_role_tool_rejected() {
        // browser.navigate is a real tool but outside the coder role scope.
        let mut bad = step("a", SpecialistRole::Coder, &[]);
        bad.tools_needed = vec!["browser.navigate".to_owned()];
        let err = validate_plan(&plan(vec![bad]), &ctx()).unwrap_err();
        assert_eq!(err.code, "tool");
    }

    #[test]
    fn high_risk_plan_requires_approval() {
        let mut risky = step("a", SpecialistRole::Coder, &[]);
        risky.risk_level = PlanRiskLevel::High;
        let validated = validate_plan(&plan(vec![risky]), &ctx()).expect("valid");
        assert!(validated.requires_approval);
    }

    #[test]
    fn budget_exceeded_plan_rejected() {
        let capped = ValidationContext {
            token_budget: Some(150),
            ..ctx()
        };
        let over = plan(vec![
            step("a", SpecialistRole::Coder, &[]),
            step("b", SpecialistRole::Coder, &[]),
        ]);
        let err = validate_plan(&over, &capped).unwrap_err();
        assert_eq!(err.code, "budget");
    }

    #[test]
    fn mismatched_total_rejected() {
        let mut bad = plan(vec![step("a", SpecialistRole::Coder, &[])]);
        bad.total_estimated_tokens = 999;
        let err = validate_plan(&bad, &ctx()).unwrap_err();
        assert_eq!(err.code, "budget");
    }

    #[test]
    fn fallback_produces_valid_plan() {
        let dag = keyword_fallback_dag("fix the code path", None, Some("no key".to_owned()));
        assert_eq!(dag.method, PlanningMethod::Keyword);
        assert!(!dag.steps.is_empty());
        assert_eq!(dag.fallback_reason.as_deref(), Some("no key"));
    }

    #[test]
    fn dag_execution_respects_dependency_order() {
        let researcher = PlanStep {
            tools_needed: vec!["fs.read".to_owned()],
            ..step("a", SpecialistRole::Researcher, &[])
        };
        let coder_b = PlanStep {
            tools_needed: vec!["fs.read".to_owned()],
            ..step("b", SpecialistRole::Coder, &["a"])
        };
        let browser_c = PlanStep {
            tools_needed: vec!["browser.extract".to_owned()],
            ..step("c", SpecialistRole::Browser, &["a"])
        };
        let coder_d = PlanStep {
            tools_needed: vec!["fs.read".to_owned()],
            ..step("d", SpecialistRole::Coder, &["b", "c"])
        };
        let dag_plan = plan(vec![researcher, coder_b, browser_c, coder_d]);
        let validated = validate_plan(&dag_plan, &ctx()).expect("valid");
        assert_eq!(validated.levels[0], vec!["a"]);
        assert!(
            validated.order.iter().position(|s| s == "a").unwrap()
                < validated.order.iter().position(|s| s == "d").unwrap()
        );
        assert!(
            validated.order.iter().position(|s| s == "b").unwrap()
                < validated.order.iter().position(|s| s == "d").unwrap()
        );
    }

    #[test]
    fn failed_dependency_cancels_dependents() {
        let dag_plan = plan(vec![
            step("a", SpecialistRole::Researcher, &[]),
            step("b", SpecialistRole::Coder, &["a"]),
            step("c", SpecialistRole::Coder, &["b"]),
            step("solo", SpecialistRole::Browser, &[]),
        ]);
        let canceled = failed_dependents(&dag_plan, "a");
        assert!(canceled.contains(&"b".to_owned()));
        assert!(canceled.contains(&"c".to_owned()));
        assert!(!canceled.contains(&"solo".to_owned()));
        assert!(!canceled.contains(&"a".to_owned()));
    }
}
