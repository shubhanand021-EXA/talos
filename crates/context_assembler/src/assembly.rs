//! Budget allocation, per-layer TypeCompact, and deterministic assembly.
//!
//! The assembler turns a [`GatheredContext`] into an [`AssembledContext`]
//! (messages + tool schemas + audit metadata). Safety content is verbatim and
//! is never compacted; every other overflowing layer is compacted in isolation
//! via `context_budget`, with truncation as the degraded fallback.

use std::collections::HashMap;

use context_budget::{
    BudgetConfig, CompactedSegment, ContextLayer, ContextSegment, ContextSnapshot, LayerConfig,
};

use crate::config::ContextAssemblyConfig;
use crate::estimator::{estimate_tokens, EstimatorKind};
use crate::gather::GatheredContext;
use crate::template;

/// Structured assembly failures.
#[derive(Debug, thiserror::Error)]
pub enum AssemblyError {
    /// Caller-supplied input failed validation.
    #[error("invalid assembly input: {0}")]
    InvalidInput(String),
    /// Prompt exceeds the model window after all compaction.
    #[error("context budget exceeded: {total} tokens over {window} window")]
    HardLimitExceeded {
        /// Final token count.
        total: u32,
        /// Model context window.
        window: u32,
    },
    /// Storage failure while gathering.
    #[error("storage failure: {0}")]
    Storage(String),
}

impl From<AssemblyError> for talos_common::TalosError {
    fn from(err: AssemblyError) -> Self {
        talos_common::TalosError::Internal(err.to_string())
    }
}

/// One applied compaction (or degraded truncation) for the audit trail.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummary {
    /// Layer name.
    pub layer: String,
    /// Tokens before.
    pub original_tokens: u64,
    /// Tokens after.
    pub compacted_tokens: u64,
    /// TypeCompact proof hash (`truncated` when degraded without compact).
    pub proof_hash: String,
    /// Invariants the compact step preserved (empty when degraded).
    pub preserved_invariants: Vec<String>,
    /// True when invariants failed and truncation applied instead.
    pub degraded: bool,
}

/// Final assembled prompt, ready for `LlmRequest`.
#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// Ordered messages (system, task, evidence, tools, history, instruction).
    pub messages: Vec<llm_gateway::LlmMessage>,
    /// Tool schemas for `LlmRequest.tools`.
    pub tools: Vec<llm_gateway::ToolSchema>,
    /// Total tokens across all messages.
    pub total_tokens: u32,
    /// Per-layer token counts (layer name → tokens).
    pub per_layer_tokens: HashMap<String, u32>,
    /// Per-layer budget targets (layer name → tokens).
    pub layer_targets: HashMap<String, u32>,
    /// Tool ids included in the catalog block.
    pub tools_included: Vec<String>,
    /// Evidence ids referenced (sorted, unique).
    pub evidence_refs: Vec<String>,
    /// Prompt template version.
    pub template_version: String,
    /// Estimator identity (`tiktoken-cl100k` or `char-heuristic-v1`).
    pub estimator_used: String,
    /// SHA-256 of the canonical messages JSON (audit).
    pub assembly_hash: String,
    /// Compactions applied, in layer order.
    pub compactions_applied: Vec<CompactionSummary>,
    /// True when any layer fell back to truncation.
    pub degraded: bool,
}

/// Input to one assembly run.
#[derive(Debug, Clone)]
pub struct AssemblyInput {
    /// Gathered task context.
    pub gathered: GatheredContext,
    /// Model id (drives estimator selection).
    pub model: String,
    /// Model context window in tokens.
    pub context_window: u32,
    /// Assembly knobs.
    pub config: ContextAssemblyConfig,
}

/// Assemble a prompt from gathered context with budget enforcement.
///
/// Telemetry wrapper: records a `talos.context_assembler.assemble` span
/// (duration, layer count, compaction count — metadata only, never prompt
/// text).
pub fn assemble(input: &AssemblyInput) -> Result<AssembledContext, AssemblyError> {
    let started = std::time::Instant::now();
    let task_id = input.gathered.task_id.clone();
    let outcome = assemble_inner(input);
    let duration_ms = started.elapsed().as_millis().clamp(0, i64::MAX as u128) as i64;
    match &outcome {
        Ok(assembled) => talos_telemetry::record_now(
            "context_assembler",
            "assemble",
            Some(&task_id),
            None,
            talos_telemetry::SpanStatus::Ok,
            "assembled",
            duration_ms,
            &[
                (
                    "count.layers",
                    serde_json::json!(assembled.per_layer_tokens.len()),
                ),
                (
                    "count.compactions",
                    serde_json::json!(assembled.compactions_applied.len()),
                ),
            ],
        ),
        Err(err) => talos_telemetry::record_now(
            "context_assembler",
            "assemble",
            Some(&task_id),
            None,
            talos_telemetry::SpanStatus::Error,
            "assembly failed",
            duration_ms,
            &[(
                "error.code",
                serde_json::json!(err.to_string().chars().take(128).collect::<String>()),
            )],
        ),
    }
    outcome
}

/// Inner assembly: validate → budget → layer build → compact → emit.
fn assemble_inner(input: &AssemblyInput) -> Result<AssembledContext, AssemblyError> {
    input
        .config
        .validate()
        .map_err(AssemblyError::InvalidInput)?;
    if input.model.is_empty() || input.model.len() > 128 {
        return Err(AssemblyError::InvalidInput(
            "model must be 1..=128 chars".to_owned(),
        ));
    }
    if input.context_window == 0 {
        return Err(AssemblyError::InvalidInput(
            "context_window must be positive".to_owned(),
        ));
    }
    let gathered = &input.gathered;
    if gathered.task_id.is_empty() {
        return Err(AssemblyError::InvalidInput(
            "task_id is required".to_owned(),
        ));
    }

    let budget = input.config.input_budget(input.context_window);
    let shares = layer_shares(&input.config, budget);
    let mut estimator_kinds = Vec::new();

    // Build one segment list per layer (deterministic order within layers).
    let mut layers: HashMap<&'static str, Vec<LayerPiece>> = HashMap::new();
    push_layer(&mut layers, safety_segments(gathered));
    push_layer(&mut layers, system_segments(gathered, &input.model));
    push_layer(&mut layers, task_segments(gathered));
    push_layer(&mut layers, session_segments(gathered));
    push_layer(&mut layers, evidence_segments(gathered));
    push_layer(&mut layers, tool_segments(gathered));
    push_layer(&mut layers, scratchpad_segments(gathered));

    // Count, then compact each overflowing non-safety layer in isolation.
    let mut compactions = Vec::new();
    let mut degraded = false;
    for &(layer, share) in &shares {
        let Some(pieces) = layers.get_mut(layer) else {
            continue;
        };
        let used: u32 = pieces
            .iter()
            .map(|piece| {
                let (tokens, kind) = estimate_tokens(&input.model, &piece.body);
                estimator_kinds.push(kind);
                piece.tokens.set(tokens);
                tokens
            })
            .sum();
        if used <= share || layer == "safety" {
            continue;
        }
        let snapshot = layer_snapshot(
            &input.gathered.task_id,
            &input.gathered.session_id,
            layer,
            pieces,
        );
        match compact_layer(&snapshot, budget, layer, share) {
            Ok((compacted, summary)) => {
                apply_compacted(pieces, &compacted, &input.model, &mut estimator_kinds);
                compactions.push(summary);
            }
            Err(_) => {
                truncate_layer(pieces, share);
                let (tokens, _) = recount(pieces, &input.model, &mut estimator_kinds);
                compactions.push(CompactionSummary {
                    layer: layer.to_string(),
                    original_tokens: used as u64,
                    compacted_tokens: tokens as u64,
                    proof_hash: "truncated".to_owned(),
                    preserved_invariants: Vec::new(),
                    degraded: true,
                });
                degraded = true;
            }
        }
    }

    // Safety is verbatim: assert its bodies survived untouched.
    debug_assert!(safety_intact(&layers, gathered));

    // Deterministic message order: system, task, evidence, tools, history, instruction.
    let (messages, per_layer_tokens, total_tokens) =
        render_messages(&layers, gathered, &input.model, &mut estimator_kinds);
    if total_tokens > input.context_window {
        return Err(AssemblyError::HardLimitExceeded {
            total: total_tokens,
            window: input.context_window,
        });
    }

    let tools: Vec<llm_gateway::ToolSchema> = gathered
        .tool_entries
        .iter()
        .map(|entry| llm_gateway::ToolSchema {
            name: entry.name.clone(),
            description: entry.description.clone(),
            input_schema: entry.input_schema.clone(),
        })
        .collect();
    let mut tools_included: Vec<String> = gathered
        .ranked_tools
        .iter()
        .map(|tool| tool.tool_id.clone())
        .chain(
            layers
                .values()
                .flat_map(|pieces| pieces.iter().flat_map(|piece| piece.tool_ids.clone())),
        )
        .collect();
    tools_included.sort();
    tools_included.dedup();
    let mut evidence_refs: Vec<String> = layers
        .values()
        .flat_map(|pieces| pieces.iter().flat_map(|piece| piece.evidence_refs.clone()))
        .collect();
    evidence_refs.sort();
    evidence_refs.dedup();
    let estimator_used = dominant_estimator(&estimator_kinds).as_str().to_owned();
    let assembly_hash = hash_messages(&messages);

    Ok(AssembledContext {
        messages,
        tools,
        total_tokens,
        per_layer_tokens,
        layer_targets: shares
            .into_iter()
            .map(|(layer, share)| (layer.to_owned(), share))
            .collect(),
        tools_included,
        evidence_refs,
        template_version: input.config.template_version.clone(),
        estimator_used,
        assembly_hash,
        compactions_applied: compactions,
        degraded,
    })
}

/// Metadata-only evidence payload for `context_assembly` rows (no prompt text).
pub fn assembly_evidence_payload(
    assembled: &AssembledContext,
    task_id: &str,
    agent_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "task_id": task_id,
        "agent_id": agent_id,
        "template_version": assembled.template_version,
        "estimator_used": assembled.estimator_used,
        "total_tokens": assembled.total_tokens,
        "per_layer_tokens": assembled.per_layer_tokens,
        "layer_targets": assembled.layer_targets,
        "tools_included": assembled.tools_included,
        "evidence_refs": assembled.evidence_refs,
        "assembly_hash": assembled.assembly_hash,
        "compactions_applied": assembled.compactions_applied,
        "degraded": assembled.degraded,
    })
}

// --- internals -----------------------------------------------------------

struct LayerPiece {
    id: String,
    body: String,
    protected: bool,
    safety_critical: bool,
    evidence_refs: Vec<String>,
    tool_ids: Vec<String>,
    tokens: std::cell::Cell<u32>,
}

fn push_layer(map: &mut HashMap<&'static str, Vec<LayerPiece>>, pieces: Vec<LayerPieceFull>) {
    for piece in pieces {
        map.entry(piece.layer).or_default().push(LayerPiece {
            id: piece.id,
            body: piece.body,
            protected: piece.protected,
            safety_critical: piece.safety_critical,
            evidence_refs: piece.evidence_refs,
            tool_ids: piece.tool_ids,
            tokens: std::cell::Cell::new(0),
        });
    }
}

struct LayerPieceFull {
    layer: &'static str,
    id: String,
    body: String,
    protected: bool,
    safety_critical: bool,
    evidence_refs: Vec<String>,
    tool_ids: Vec<String>,
}

fn piece(
    layer: &'static str,
    id: impl Into<String>,
    body: impl Into<String>,
    protected: bool,
    safety_critical: bool,
    evidence_refs: Vec<String>,
    tool_ids: Vec<String>,
) -> LayerPieceFull {
    LayerPieceFull {
        layer,
        id: id.into(),
        body: body.into(),
        protected,
        safety_critical,
        evidence_refs,
        tool_ids,
    }
}

fn safety_segments(gathered: &GatheredContext) -> Vec<LayerPieceFull> {
    gathered
        .safety_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            piece(
                "safety",
                format!("safety-{index}"),
                rule.clone(),
                true,
                true,
                Vec::new(),
                Vec::new(),
            )
        })
        .collect()
}

fn system_segments(gathered: &GatheredContext, model: &str) -> Vec<LayerPieceFull> {
    vec![piece(
        "system",
        "system-meta",
        format!(
            "model: {model}\ntask: {}\nagent: {}\ntemplate: {}",
            gathered.task_id,
            gathered.agent_id,
            crate::template::TEMPLATE_VERSION
        ),
        true,
        false,
        Vec::new(),
        Vec::new(),
    )]
}

fn task_segments(gathered: &GatheredContext) -> Vec<LayerPieceFull> {
    vec![piece(
        "task",
        "task-contract",
        template::render_task_context(
            &gathered.goal,
            &gathered.acceptance,
            &format!("{} ({})", gathered.task_status, gathered.task_priority),
            &gathered.constraints,
        ),
        true,
        false,
        Vec::new(),
        Vec::new(),
    )]
}

fn session_segments(gathered: &GatheredContext) -> Vec<LayerPieceFull> {
    let mut out: Vec<LayerPieceFull> = gathered
        .segments
        .iter()
        .filter(|segment| segment.layer == "session")
        .map(|segment| {
            piece(
                "session",
                segment.id.clone(),
                segment.body.clone(),
                false,
                false,
                segment.evidence_refs.clone(),
                segment.tool_ids.clone(),
            )
        })
        .collect();
    for (index, turn) in gathered.history.iter().enumerate() {
        out.push(piece(
            "session",
            format!("history-{index}"),
            format!("{}: {}", turn.role, turn.content),
            false,
            false,
            Vec::new(),
            Vec::new(),
        ));
    }
    out
}

fn evidence_segments(gathered: &GatheredContext) -> Vec<LayerPieceFull> {
    let mut out: Vec<LayerPieceFull> = gathered
        .segments
        .iter()
        .filter(|segment| segment.layer == "evidence")
        .map(|segment| {
            piece(
                "evidence",
                segment.id.clone(),
                segment.body.clone(),
                false,
                false,
                segment.evidence_refs.clone(),
                segment.tool_ids.clone(),
            )
        })
        .collect();
    if let Some(head) = &gathered.chain_head_hash {
        out.push(piece(
            "evidence",
            "chain-head",
            format!("evidence chain head: {head}"),
            false,
            false,
            Vec::new(),
            Vec::new(),
        ));
    }
    out
}

fn tool_segments(gathered: &GatheredContext) -> Vec<LayerPieceFull> {
    let mut out = vec![piece(
        "tool",
        "tool-catalog",
        template::render_tool_context(
            &gathered
                .ranked_tools
                .iter()
                .map(|tool| (tool.tool_id.clone(), tool.reason.clone()))
                .collect::<Vec<_>>(),
        ),
        false,
        false,
        Vec::new(),
        gathered
            .ranked_tools
            .iter()
            .map(|tool| tool.tool_id.clone())
            .collect(),
    )];
    for entry in &gathered.tool_entries {
        out.push(piece(
            "tool",
            format!("tool-schema-{}", entry.name),
            format!("{}: {}", entry.name, entry.description),
            false,
            false,
            Vec::new(),
            vec![entry.name.clone()],
        ));
    }
    out
}

fn scratchpad_segments(gathered: &GatheredContext) -> Vec<LayerPieceFull> {
    let mut out: Vec<LayerPieceFull> = gathered
        .segments
        .iter()
        .filter(|segment| segment.layer == "scratchpad")
        .map(|segment| {
            piece(
                "scratchpad",
                segment.id.clone(),
                segment.body.clone(),
                false,
                false,
                segment.evidence_refs.clone(),
                segment.tool_ids.clone(),
            )
        })
        .collect();
    out.push(piece(
        "scratchpad",
        "instruction",
        format!("## Instruction\n{}", gathered.instruction),
        false,
        false,
        Vec::new(),
        Vec::new(),
    ));
    out
}

/// Bucket share per assembly layer (contracts split across 3 layers).
fn layer_shares(config: &ContextAssemblyConfig, budget: u32) -> Vec<(&'static str, u32)> {
    let bucket = |pct: u8| config.bucket_target(budget, pct);
    let contracts = bucket(config.allocation.contracts_pct);
    let third = contracts / 3;
    vec![
        ("safety", third),
        ("system", third),
        ("task", contracts.saturating_sub(third.saturating_mul(2))),
        ("tool", bucket(config.allocation.tool_state_pct)),
        ("evidence", bucket(config.allocation.evidence_pct)),
        ("session", bucket(config.allocation.memory_pct)),
        ("scratchpad", bucket(config.allocation.plan_pct)),
    ]
}

fn parse_layer(name: &str) -> ContextLayer {
    match name {
        "safety" => ContextLayer::Safety,
        "system" => ContextLayer::System,
        "task" => ContextLayer::Task,
        "session" => ContextLayer::Session,
        "evidence" => ContextLayer::Evidence,
        "tool" => ContextLayer::Tool,
        _ => ContextLayer::Scratchpad,
    }
}

fn layer_snapshot(
    task_id: &str,
    session_id: &str,
    layer: &str,
    pieces: &[LayerPiece],
) -> ContextSnapshot {
    ContextSnapshot {
        task_id: task_id.to_owned(),
        session_id: session_id.to_owned(),
        segments: pieces
            .iter()
            .map(|piece| ContextSegment {
                id: piece.id.clone(),
                layer: parse_layer(layer),
                body: piece.body.clone(),
                protected: piece.protected,
                safety_critical: piece.safety_critical,
                schema_type: None,
                evidence_refs: piece.evidence_refs.clone(),
                compensation_refs: Vec::new(),
                policy_refs: Vec::new(),
                action_constraints: Vec::new(),
            })
            .collect(),
    }
}

/// Compact one layer with TypeCompact under its share budget.
fn compact_layer(
    snapshot: &ContextSnapshot,
    total_budget: u32,
    layer: &str,
    share: u32,
) -> Result<(Vec<CompactedSegment>, CompactionSummary), String> {
    let defaults = context_budget::layer_defaults();
    let template = defaults
        .iter()
        .find(|config| config.layer.as_str() == layer)
        .ok_or_else(|| format!("unknown layer {layer}"))?;
    let config = BudgetConfig {
        total_budget: total_budget as u64,
        layers: vec![LayerConfig {
            layer: template.layer,
            priority: template.priority,
            max_budget: share as u64,
            compression: template.compression,
            discard: template.discard,
            protected: template.protected,
        }],
    };
    let result =
        context_budget::compact_snapshot(snapshot, &config).map_err(|err| err.message.clone())?;
    Ok((
        result.segments.clone(),
        CompactionSummary {
            layer: layer.to_owned(),
            original_tokens: result.original_tokens,
            compacted_tokens: result.compacted_tokens,
            proof_hash: result.proof_hash.clone(),
            preserved_invariants: result.preserved_invariants.clone(),
            degraded: false,
        },
    ))
}

fn apply_compacted(
    pieces: &mut Vec<LayerPiece>,
    compacted: &[CompactedSegment],
    model: &str,
    kinds: &mut Vec<EstimatorKind>,
) {
    let bodies: HashMap<&str, &str> = compacted
        .iter()
        .map(|segment| (segment.id.as_str(), segment.body.as_str()))
        .collect();
    for piece in pieces.iter_mut() {
        if let Some(body) = bodies.get(piece.id.as_str()) {
            piece.body = (*body).to_owned();
        }
        let (tokens, kind) = estimate_tokens(model, &piece.body);
        piece.tokens.set(tokens);
        kinds.push(kind);
    }
    // Deterministic order: compacted id order, then leftovers by id.
    pieces.sort_by(|a, b| {
        let order = |id: &str| {
            compacted
                .iter()
                .position(|segment| segment.id == id)
                .unwrap_or(usize::MAX)
        };
        order(&a.id)
            .cmp(&order(&b.id))
            .then_with(|| a.id.cmp(&b.id))
    });
    pieces.retain(|piece| {
        compacted
            .iter()
            .find(|segment| segment.id == piece.id)
            .is_none_or(|segment| !segment.dropped)
    });
}

/// Deterministic truncation: protected and ref-bearing pieces survive.
fn truncate_layer(pieces: &mut Vec<LayerPiece>, share: u32) {
    pieces.sort_by(|a, b| {
        b.protected
            .cmp(&a.protected)
            .then_with(|| b.evidence_refs.len().cmp(&a.evidence_refs.len()))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut used = 0u32;
    pieces.retain(|piece| {
        let tokens = piece.tokens.get();
        if used.saturating_add(tokens) <= share {
            used = used.saturating_add(tokens);
            true
        } else {
            // Protected pieces are never dropped by truncation either:
            // if a protected piece cannot fit, the whole assembly fails.
            piece.protected
        }
    });
}

fn recount(pieces: &[LayerPiece], model: &str, kinds: &mut Vec<EstimatorKind>) -> (u32, ()) {
    let mut total = 0u32;
    for piece in pieces {
        let (tokens, kind) = estimate_tokens(model, &piece.body);
        piece.tokens.set(tokens);
        kinds.push(kind);
        total = total.saturating_add(tokens);
    }
    (total, ())
}

fn safety_intact(layers: &HashMap<&str, Vec<LayerPiece>>, gathered: &GatheredContext) -> bool {
    let Some(pieces) = layers.get("safety") else {
        return gathered.safety_rules.is_empty();
    };
    if pieces.len() != gathered.safety_rules.len() {
        return false;
    }
    pieces
        .iter()
        .zip(gathered.safety_rules.iter())
        .all(|(piece, rule)| &piece.body == rule)
}

#[allow(clippy::type_complexity)]
fn render_messages(
    layers: &HashMap<&str, Vec<LayerPiece>>,
    gathered: &GatheredContext,
    model: &str,
    kinds: &mut Vec<EstimatorKind>,
) -> (Vec<llm_gateway::LlmMessage>, HashMap<String, u32>, u32) {
    use llm_gateway::{LlmMessage, LlmRole};
    let mut messages = Vec::new();
    let mut per_layer = HashMap::new();
    let mut total = 0u32;
    // Safety rules ride verbatim inside the system prompt; account their
    // cost against the safety layer so every budget bucket stays visible.
    let mut safety_tokens = 0u32;
    for rule in &gathered.safety_rules {
        let (tokens, kind) = estimate_tokens(model, rule);
        kinds.push(kind);
        safety_tokens = safety_tokens.saturating_add(tokens);
    }
    per_layer.insert("safety".to_owned(), safety_tokens);
    let mut emit = |layer: &str, role: LlmRole, content: String| {
        let (tokens, kind) = estimate_tokens(model, &content);
        kinds.push(kind);
        *per_layer.entry(layer.to_owned()).or_insert(0) += tokens;
        total = total.saturating_add(tokens);
        messages.push(LlmMessage::new(role, content));
    };

    let bodies = |layer: &str| -> Vec<String> {
        let mut items: Vec<&LayerPiece> = layers
            .get(layer)
            .map_or(Vec::new(), |pieces| pieces.iter().collect());
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items.into_iter().map(|piece| piece.body.clone()).collect()
    };

    emit(
        "system",
        LlmRole::System,
        template::render_system_prompt(&gathered.safety_rules),
    );
    for body in bodies("system") {
        emit("system", LlmRole::System, body);
    }
    for body in bodies("task") {
        emit("task", LlmRole::User, body);
    }
    if !bodies("evidence").is_empty() {
        emit(
            "evidence",
            LlmRole::User,
            format!("## Evidence\n{}", bodies("evidence").join("\n")),
        );
    }
    if !bodies("tool").is_empty() {
        emit(
            "tool",
            LlmRole::User,
            format!("## Tools\n{}", bodies("tool").join("\n")),
        );
    }
    for turn in &gathered.history {
        let role = match turn.role.as_str() {
            "system" => LlmRole::System,
            "assistant" => LlmRole::Assistant,
            "tool" => LlmRole::Tool,
            _ => LlmRole::User,
        };
        emit("session", role, turn.content.clone());
    }
    for body in bodies("session") {
        emit("session", LlmRole::User, body);
    }
    for body in bodies("scratchpad") {
        emit("scratchpad", LlmRole::User, body);
    }
    (messages, per_layer, total)
}

fn dominant_estimator(kinds: &[EstimatorKind]) -> EstimatorKind {
    if kinds.contains(&EstimatorKind::TiktokenCl100k) {
        EstimatorKind::TiktokenCl100k
    } else {
        EstimatorKind::CharHeuristic
    }
}

fn hash_messages(messages: &[llm_gateway::LlmMessage]) -> String {
    let raw = serde_json::to_string(messages).unwrap_or_default();
    let digest = <sha2::Sha256 as sha2::Digest>::digest(raw.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ContextAssemblyConfig;
    use crate::gather::{GatheredContext, HistoryTurn, RankedTool, SegmentInput, ToolEntry};

    fn gathered() -> GatheredContext {
        GatheredContext {
            task_id: "task-a1".to_owned(),
            agent_id: "agent-a1".to_owned(),
            session_id: "session-a1".to_owned(),
            goal: "summarize the ledger".to_owned(),
            acceptance: vec!["mentions head hash".to_owned()],
            constraints: vec!["no network".to_owned()],
            task_title: "summarize".to_owned(),
            task_status: "executing".to_owned(),
            task_priority: "normal".to_owned(),
            safety_rules: vec!["deny writes without evidence".to_owned()],
            segments: vec![
                SegmentInput {
                    id: "ev-1".to_owned(),
                    layer: "evidence",
                    body: "claim one holds ".to_owned() + &"x ".repeat(4000),
                    protected: false,
                    safety_critical: false,
                    evidence_refs: vec!["ev-1".to_owned()],
                    tool_ids: Vec::new(),
                },
                SegmentInput {
                    id: "mem-1".to_owned(),
                    layer: "session",
                    body: "prior decision: cache receipts".to_owned(),
                    protected: false,
                    safety_critical: false,
                    evidence_refs: Vec::new(),
                    tool_ids: Vec::new(),
                },
            ],
            ranked_tools: vec![RankedTool {
                tool_id: "fs.read".to_owned(),
                score: 4.0,
                reason: "keyword".to_owned(),
            }],
            tool_entries: vec![ToolEntry {
                name: "fs.read".to_owned(),
                description: "read a file".to_owned(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            history: vec![HistoryTurn {
                role: "user".to_owned(),
                content: "start".to_owned(),
            }],
            instruction: "produce the summary now".to_owned(),
            chain_head_hash: Some("abc123".to_owned()),
        }
    }

    fn input(window: u32) -> AssemblyInput {
        AssemblyInput {
            gathered: gathered(),
            model: "gpt-4o-mini".to_owned(),
            context_window: window,
            config: ContextAssemblyConfig::default(),
        }
    }

    #[test]
    fn budget_allocation_sums_and_splits() {
        let config = ContextAssemblyConfig::default();
        let shares = layer_shares(&config, 85_000);
        let total: u32 = shares.iter().map(|(_, share)| share).sum();
        assert_eq!(total, 85_000);
        // Contracts bucket (20% = 17_000) splits across 3 layers.
        let contracts: u32 = shares
            .iter()
            .filter(|(layer, _)| ["safety", "system", "task"].contains(layer))
            .map(|(_, share)| share)
            .sum();
        assert_eq!(contracts, 17_000);
    }

    #[test]
    fn assembles_deterministically_with_stable_hash() {
        let first = assemble(&input(128_000)).unwrap();
        let second = assemble(&input(128_000)).unwrap();
        assert_eq!(first.assembly_hash, second.assembly_hash);
        assert!(first.assembly_hash.starts_with("sha256:"));
        assert_eq!(first.template_version, "ctx-assembler/1");
        assert!(!first.messages.is_empty());
        assert!(first.tools_included.contains(&"fs.read".to_owned()));
        assert!(first.evidence_refs.contains(&"ev-1".to_owned()));
        assert_eq!(first.tools.len(), 1);
    }

    #[test]
    fn overflow_triggers_compaction_not_failure() {
        // Tiny window forces the evidence layer over budget.
        let assembled = assemble(&input(2_000)).unwrap();
        assert!(assembled.total_tokens <= 2_000);
        assert!(assembled
            .compactions_applied
            .iter()
            .any(|summary| summary.layer == "evidence"));
        assert!(!assembled.degraded);
    }

    #[test]
    fn safety_layer_is_never_compacted() {
        let assembled = assemble(&input(2_000)).unwrap();
        assert!(assembled
            .compactions_applied
            .iter()
            .all(|summary| summary.layer != "safety"));
        let system_text: String = assembled
            .messages
            .iter()
            .filter(|message| message.role == llm_gateway::LlmRole::System)
            .map(|message| message.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(system_text.contains("- deny writes without evidence"));
    }

    #[test]
    fn hard_limit_fails_gracefully() {
        // A 32-token window cannot hold even the verbatim system prompt, so
        // no amount of compaction can fit: the assembly fails gracefully.
        // (`llama3` forces the deterministic char heuristic.)
        let mut tiny = gathered();
        tiny.instruction = "do it ".repeat(900);
        let input = AssemblyInput {
            gathered: tiny,
            model: "llama3".to_owned(),
            context_window: 32,
            config: ContextAssemblyConfig::default(),
        };
        let err = assemble(&input).unwrap_err();
        assert!(
            matches!(err, AssemblyError::HardLimitExceeded { .. }),
            "expected hard-limit failure, got assembly success"
        );
    }

    #[test]
    fn rejects_invalid_inputs() {
        let mut bad_model = input(128_000);
        bad_model.model.clear();
        assert!(assemble(&bad_model).is_err());
        assert!(assemble(&input(0)).is_err());
        let mut bad_task = input(128_000);
        bad_task.gathered.task_id.clear();
        assert!(assemble(&bad_task).is_err());
    }

    #[test]
    fn evidence_payload_carries_metadata_only() {
        let assembled = assemble(&input(128_000)).unwrap();
        let payload = assembly_evidence_payload(&assembled, "task-a1", "agent-a1");
        assert_eq!(payload["task_id"], serde_json::json!("task-a1"));
        assert_eq!(
            payload["assembly_hash"],
            serde_json::json!(assembled.assembly_hash)
        );
        // No prompt text, message bodies, or content keys.
        let raw = serde_json::to_string(&payload).unwrap();
        assert!(!raw.contains("produce the summary now"));
        assert!(!raw.contains("claim one holds"));
        assert!(!payload.as_object().unwrap().contains_key("messages"));
        assert!(!payload.as_object().unwrap().contains_key("content"));
    }
}
