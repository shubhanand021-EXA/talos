//! Workspace instruction files in context assembly.
//!
//! When assembling context for a task, the caller collects
//! [`InstructionBundle`] from the workspace root (AGENTS.md / CLAUDE.md /
//! SKILL.md) and merges it into the [`GatheredContext`](crate::gather::GatheredContext):
//! instruction text lands in `safety_rules` (verbatim, never compacted) and
//! skill procedures land in `tool`-layer segments marked
//! `protected + safety_critical` so TypeCompact carries them byte-identical.

use crate::gather::{GatheredContext, SegmentInput};

/// Instructions collected from one workspace root.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstructionBundle {
    /// Verbatim safety rules (AGENTS.md global + agent scope, CLAUDE.md merge).
    pub safety_rules: Vec<String>,
    /// Task constraints from CLAUDE.md (`constraint:` lines).
    pub constraints: Vec<String>,
    /// Build commands from CLAUDE.md (`build:` lines).
    pub build_commands: Vec<String>,
    /// Skill procedures for the tool layer (SKILL.md bodies, verbatim).
    pub skill_procedures: Vec<SkillProcedure>,
    /// Files read, in deterministic order (audit trail).
    pub sources: Vec<String>,
}

/// One skill procedure for the tool layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillProcedure {
    /// Skill name.
    pub name: String,
    /// Full procedure body (verbatim).
    pub body: String,
    /// Required tools.
    pub required_tools: Vec<String>,
}

/// Collect instruction files from `workspace_root` for `agent_id`.
///
/// Reads `<root>/AGENTS.md`, `<root>/CLAUDE.md`, `<root>/SKILL.md`, and every
/// `*.md` file under `<root>/skills/` (sorted). Missing files are skipped;
/// malformed files are skipped with the reason recorded in `sources` as
/// `path (skipped: reason)` — never fatal to assembly.
pub fn collect_workspace_instructions(
    workspace_root: &std::path::Path,
    agent_id: Option<&str>,
) -> InstructionBundle {
    let mut bundle = InstructionBundle::default();
    let agents_path = workspace_root.join("AGENTS.md");
    if agents_path.is_file() {
        match std::fs::read_to_string(&agents_path) {
            Ok(text) => {
                match instruction_parser::parse_agents(&text, Some(display(&agents_path))) {
                    Ok(file) => {
                        bundle.safety_rules.push(format!(
                            "[AGENTS.md global] {}",
                            file.global.instructions.join(" ")
                        ));
                        if let Some(agent) = agent_id {
                            let scoped = file.instructions_for(agent);
                            let extra = scoped.len().saturating_sub(file.global.instructions.len());
                            if extra > 0 {
                                bundle.safety_rules.push(format!(
                                    "[AGENTS.md agent:{agent}] {}",
                                    scoped[file.global.instructions.len()..].join(" ")
                                ));
                            }
                        }
                        for tool in file.deny_for(agent_id.unwrap_or("")) {
                            bundle
                                .constraints
                                .push(format!("AGENTS.md denies tool: {tool}"));
                        }
                        bundle.sources.push(display(&agents_path));
                    }
                    Err(err) => bundle
                        .sources
                        .push(format!("{} (skipped: {err})", display(&agents_path))),
                }
            }
            Err(err) => bundle
                .sources
                .push(format!("{} (skipped: {err})", display(&agents_path))),
        }
    }
    let claude_path = workspace_root.join("CLAUDE.md");
    if claude_path.is_file() {
        match std::fs::read_to_string(&claude_path) {
            Ok(text) => {
                match instruction_parser::parse_claude_level(&text, 1, Some(display(&claude_path)))
                {
                    Ok(level) => {
                        for instruction in level.instructions {
                            bundle
                                .safety_rules
                                .push(format!("[CLAUDE.md] {instruction}"));
                        }
                        for convention in level.conventions {
                            bundle
                                .safety_rules
                                .push(format!("[CLAUDE.md convention] {convention}"));
                        }
                        for gotcha in level.gotchas {
                            bundle
                                .safety_rules
                                .push(format!("[CLAUDE.md gotcha] {gotcha}"));
                        }
                        for constraint in &level.constraints {
                            bundle.constraints.push(format!("[CLAUDE.md] {constraint}"));
                        }
                        bundle.build_commands.extend(level.build_commands);
                        bundle.sources.push(display(&claude_path));
                    }
                    Err(err) => bundle
                        .sources
                        .push(format!("{} (skipped: {err})", display(&claude_path))),
                }
            }
            Err(err) => bundle
                .sources
                .push(format!("{} (skipped: {err})", display(&claude_path))),
        }
    }
    // Root SKILL.md plus every markdown file under <root>/skills/ (sorted).
    let mut skill_paths = Vec::new();
    let root_skill = workspace_root.join("SKILL.md");
    if root_skill.is_file() {
        skill_paths.push(root_skill);
    }
    let skills_dir = workspace_root.join("skills");
    if skills_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            let mut names: Vec<std::path::PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                })
                .collect();
            names.sort();
            skill_paths.extend(names);
        }
    }
    for path in skill_paths {
        match std::fs::read_to_string(&path) {
            Ok(text) => match instruction_parser::parse_skill(&text, Some(display(&path))) {
                Ok(skill) => {
                    bundle.skill_procedures.push(SkillProcedure {
                        name: skill.name.clone(),
                        body: skill.body.clone(),
                        required_tools: skill.required_tools.clone(),
                    });
                    bundle.sources.push(display(&path));
                }
                Err(err) => bundle
                    .sources
                    .push(format!("{} (skipped: {err})", display(&path))),
            },
            Err(err) => bundle
                .sources
                .push(format!("{} (skipped: {err})", display(&path))),
        }
    }
    bundle
        .safety_rules
        .retain(|rule| !rule.trim().is_empty() && !rule.ends_with(']'));
    bundle
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Merge a bundle into gathered context: safety rules extend verbatim, skill
/// procedures become `tool`-layer segments marked `protected + safety_critical`
/// (TypeCompact verbatim), constraints extend the task contract.
pub fn apply_to_gathered(gathered: &mut GatheredContext, bundle: &InstructionBundle) {
    gathered
        .safety_rules
        .extend(bundle.safety_rules.iter().cloned());
    gathered
        .constraints
        .extend(bundle.constraints.iter().cloned());
    for procedure in &bundle.skill_procedures {
        gathered.segments.push(SegmentInput {
            id: format!("skill-{}", slug(&procedure.name)),
            layer: "tool",
            body: format!("# Skill: {}\n{}", procedure.name, procedure.body),
            protected: true,
            safety_critical: true,
            evidence_refs: Vec::new(),
            tool_ids: procedure.required_tools.clone(),
        });
    }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "skill".to_owned()
    } else {
        trimmed.chars().take(64).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("parents");
            }
            std::fs::write(&path, content).expect("write");
        }
        dir
    }

    #[test]
    fn collects_agents_claude_and_skills() {
        let dir = workspace(&[
            ("AGENTS.md", "# Global\nShip clean code.\n- deny: process.exec\n"),
            (
                "CLAUDE.md",
                "build: cargo test\nconstraint: never commit secrets\nFollow the style guide.\n",
            ),
            (
                "skills/review.md",
                "---\nname: review\ndescription: Review code.\nrequired_tools: [fs.read]\n---\n1. Read the diff.\n2. Summarize.\n",
            ),
        ]);
        let bundle = collect_workspace_instructions(dir.path(), Some("coder"));
        assert!(bundle
            .safety_rules
            .iter()
            .any(|rule| rule.contains("Ship clean code.")));
        assert!(bundle
            .safety_rules
            .iter()
            .any(|rule| rule.contains("Follow the style guide.")));
        assert!(bundle
            .constraints
            .iter()
            .any(|rule| rule.contains("never commit secrets")));
        assert!(bundle
            .constraints
            .iter()
            .any(|rule| rule.contains("process.exec")));
        assert_eq!(bundle.build_commands, vec!["cargo test".to_owned()]);
        assert_eq!(bundle.skill_procedures.len(), 1);
        assert_eq!(bundle.skill_procedures[0].name, "review");
        assert_eq!(bundle.sources.len(), 3);
    }

    #[test]
    fn missing_files_yield_empty_bundle() {
        let dir = workspace(&[]);
        let bundle = collect_workspace_instructions(dir.path(), None);
        assert!(bundle.safety_rules.is_empty());
        assert!(bundle.skill_procedures.is_empty());
    }

    #[test]
    fn malformed_files_never_fail_assembly() {
        let dir = workspace(&[("AGENTS.md", &"x".repeat(300_000))]);
        let bundle = collect_workspace_instructions(dir.path(), None);
        assert!(bundle.safety_rules.is_empty());
        assert!(bundle
            .sources
            .iter()
            .any(|source| source.contains("skipped")));
    }

    #[test]
    fn skill_segments_are_safety_critical() {
        let dir = workspace(&[(
            "SKILL.md",
            "---\nname: deploy\ndescription: Deploy.\n---\n1. Run checks.\n2. Ship.\n",
        )]);
        let bundle = collect_workspace_instructions(dir.path(), None);
        let mut gathered = GatheredContext {
            task_id: "task-1".to_owned(),
            ..GatheredContext::default()
        };
        apply_to_gathered(&mut gathered, &bundle);
        let segment = gathered
            .segments
            .iter()
            .find(|segment| segment.layer == "tool")
            .expect("skill segment");
        assert!(segment.protected);
        assert!(segment.safety_critical);
        assert!(segment.body.contains("Run checks."));
    }

    #[test]
    fn instruction_content_survives_typecompact() {
        // Safety-critical segments are verbatim under any budget pressure.
        let snapshot = context_budget::ContextSnapshot {
            task_id: "task-1".to_owned(),
            session_id: "session-1".to_owned(),
            segments: vec![
                context_budget::ContextSegment {
                    id: "skill-deploy".to_owned(),
                    layer: context_budget::ContextLayer::Tool,
                    body: "# Skill: deploy\n1. Run checks.\n2. Ship.".to_owned(),
                    protected: true,
                    safety_critical: true,
                    schema_type: None,
                    evidence_refs: Vec::new(),
                    compensation_refs: Vec::new(),
                    policy_refs: Vec::new(),
                    action_constraints: Vec::new(),
                },
                context_budget::ContextSegment {
                    id: "noise".to_owned(),
                    layer: context_budget::ContextLayer::Scratchpad,
                    body: "noise ".repeat(4000),
                    protected: false,
                    safety_critical: false,
                    schema_type: None,
                    evidence_refs: Vec::new(),
                    compensation_refs: Vec::new(),
                    policy_refs: Vec::new(),
                    action_constraints: Vec::new(),
                },
            ],
        };
        let config = context_budget::BudgetConfig {
            total_budget: 100,
            layers: context_budget::layer_defaults(),
        };
        let result = context_budget::compact_snapshot(&snapshot, &config).expect("compacts");
        let skill = result
            .segments
            .iter()
            .find(|row| row.id == "skill-deploy")
            .expect("skill kept");
        assert!(!skill.dropped && !skill.trimmed);
        assert_eq!(skill.body, "# Skill: deploy\n1. Run checks.\n2. Ship.");
    }
}
