//! Open-standards instruction parsers: AGENTS.md, CLAUDE.md, SKILL.md,
//! HEARTBEAT.md, plus the plugin-trust foundation.
//!
//! Every parser is deterministic (same bytes → same value) and bounded
//! (256 KiB inputs, bounded item counts). Parsed instruction content is
//! safety-critical: context assembly must carry it verbatim and TypeCompact
//! must never compress it (mark segments `protected + safety_critical`).

pub mod agents;
pub mod claude;
pub mod heartbeat;
pub mod plugin;
pub mod skill;

pub use agents::{parse_agents, AgentScope, AgentsFile};
pub use claude::{
    parse_claude_hierarchy, parse_claude_level, ClaudeFile, ClaudeLevel, HIERARCHY_DEPTH,
};
pub use heartbeat::{parse_heartbeat, ScheduleKind, ScheduledTask};
pub use plugin::{
    approval_evidence, review_manifest, verify_signature, PermissionReview, PluginManifest,
    TrustStatus,
};
pub use skill::{parse_skill, SkillFile};

/// Structured parse failure (file kind + message).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseError {
    /// File kind (`agents`, `claude`, `skill`, `heartbeat`, `plugin`).
    pub kind: String,
    /// Human-readable message.
    pub message: String,
}

impl ParseError {
    /// Build an error.
    #[must_use]
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} parse: {}", self.kind, self.message)
    }
}

/// Which instruction file a path holds (by file name, case-insensitive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionKind {
    /// AGENTS.md.
    Agents,
    /// CLAUDE.md.
    Claude,
    /// SKILL.md (or a directory holding one).
    Skill,
    /// HEARTBEAT.md.
    Heartbeat,
    /// Not an instruction file.
    Other,
}

/// Classify a file path by name.
#[must_use]
pub fn classify(path: &std::path::Path) -> InstructionKind {
    match path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_uppercase())
        .as_deref()
    {
        Some("AGENTS.MD") => InstructionKind::Agents,
        Some("CLAUDE.MD") => InstructionKind::Claude,
        Some("SKILL.MD") => InstructionKind::Skill,
        Some("HEARTBEAT.MD") => InstructionKind::Heartbeat,
        _ => InstructionKind::Other,
    }
}

/// Parsed instruction file (one variant per kind).
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedInstruction {
    /// AGENTS.md document.
    Agents(AgentsFile),
    /// CLAUDE.md document.
    Claude(ClaudeFile),
    /// SKILL.md document.
    Skill(SkillFile),
    /// HEARTBEAT.md tasks.
    Heartbeat(Vec<ScheduledTask>),
}

/// Parse one instruction file by path classification.
pub fn parse_instruction_file(
    path: &std::path::Path,
    text: &str,
) -> Result<ParsedInstruction, ParseError> {
    let source = Some(path.to_string_lossy().into_owned());
    match classify(path) {
        InstructionKind::Agents => parse_agents(text, source).map(ParsedInstruction::Agents),
        InstructionKind::Claude => parse_claude_level(text, 1, source).map(|level| {
            ParsedInstruction::Claude(ClaudeFile {
                levels: vec![level],
            })
        }),
        InstructionKind::Skill => parse_skill(text, source).map(ParsedInstruction::Skill),
        InstructionKind::Heartbeat => {
            parse_heartbeat(text, source).map(ParsedInstruction::Heartbeat)
        }
        InstructionKind::Other => Err(ParseError::new(
            "unknown",
            "not an instruction file (AGENTS.md, CLAUDE.md, SKILL.md, HEARTBEAT.md)",
        )),
    }
}

/// Read and parse one instruction file from disk (bounded).
pub fn read_instruction_file(path: &std::path::Path) -> Result<ParsedInstruction, ParseError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| ParseError::new("io", format!("read {}: {err}", path.display())))?;
    parse_instruction_file(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_name() {
        assert_eq!(
            classify(std::path::Path::new("x/AGENTS.md")),
            InstructionKind::Agents
        );
        assert_eq!(
            classify(std::path::Path::new("x/claude.md")),
            InstructionKind::Claude
        );
        assert_eq!(
            classify(std::path::Path::new("x/SKILL.md")),
            InstructionKind::Skill
        );
        assert_eq!(
            classify(std::path::Path::new("x/heartbeat.md")),
            InstructionKind::Heartbeat
        );
        assert_eq!(
            classify(std::path::Path::new("x/README.md")),
            InstructionKind::Other
        );
    }

    #[test]
    fn dispatches_parse_by_kind() {
        let agents =
            parse_instruction_file(std::path::Path::new("AGENTS.md"), "# Global\nShip it.\n")
                .unwrap();
        assert!(matches!(agents, ParsedInstruction::Agents(_)));
        let skill = parse_instruction_file(
            std::path::Path::new("SKILL.md"),
            "---\nname: s\ndescription: d\n---\n1. Go.\n",
        )
        .unwrap();
        assert!(matches!(skill, ParsedInstruction::Skill(_)));
        assert!(parse_instruction_file(std::path::Path::new("README.md"), "hi").is_err());
    }
}
