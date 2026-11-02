//! CLAUDE.md parser: 6-level hierarchy (global → project root → nested dirs).
//!
//! Each level contributes categorized lines:
//! - `build:` lines are build commands,
//! - `convention:` lines are code conventions,
//! - `gotcha:` lines are known pitfalls,
//! - `constraint:` lines are hard constraints,
//! - every other line is a general instruction.
//!
//! Merging is deterministic: levels apply outermost-first, and the merged
//! view preserves that order verbatim for context assembly.

/// Hierarchy depth (global + root + 4 nested levels).
pub const HIERARCHY_DEPTH: usize = 6;

/// One level of CLAUDE.md content.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeLevel {
    /// Level index (0 = global, 1 = project root, 2..=5 = nested).
    pub level: usize,
    /// Source path the level was read from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    /// Build commands (`build:` lines, verbatim suffix).
    pub build_commands: Vec<String>,
    /// Conventions (`convention:` lines).
    pub conventions: Vec<String>,
    /// Gotchas (`gotcha:` lines).
    pub gotchas: Vec<String>,
    /// Constraints (`constraint:` lines).
    pub constraints: Vec<String>,
    /// General instructions (all other lines).
    pub instructions: Vec<String>,
}

/// Parsed CLAUDE.md document (one or more levels).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeFile {
    /// Levels outermost-first (sorted by `level`).
    pub levels: Vec<ClaudeLevel>,
}

impl ClaudeFile {
    /// Merge all levels outermost-first into one verbatim instruction block.
    #[must_use]
    pub fn merged_instructions(&self) -> Vec<String> {
        let mut out = Vec::new();
        for level in &self.levels {
            out.extend(level.instructions.iter().cloned());
            for command in &level.build_commands {
                out.push(format!("build: {command}"));
            }
            for convention in &level.conventions {
                out.push(format!("convention: {convention}"));
            }
            for gotcha in &level.gotchas {
                out.push(format!("gotcha: {gotcha}"));
            }
            for constraint in &level.constraints {
                out.push(format!("constraint: {constraint}"));
            }
        }
        out
    }

    /// All constraints across levels (for the task contract block).
    #[must_use]
    pub fn merged_constraints(&self) -> Vec<String> {
        self.levels
            .iter()
            .flat_map(|level| level.constraints.iter().cloned())
            .collect()
    }

    /// All build commands across levels.
    #[must_use]
    pub fn merged_build_commands(&self) -> Vec<String> {
        self.levels
            .iter()
            .flat_map(|level| level.build_commands.iter().cloned())
            .collect()
    }
}

/// Parse one CLAUDE.md level body.
pub fn parse_claude_level(
    text: &str,
    level: usize,
    source_path: Option<String>,
) -> Result<ClaudeLevel, crate::ParseError> {
    if level >= HIERARCHY_DEPTH {
        return Err(crate::ParseError::new(
            "claude",
            "level must be 0..=5 (global, root, 4 nested)",
        ));
    }
    if text.len() > 262_144 {
        return Err(crate::ParseError::new(
            "claude",
            "CLAUDE.md must be at most 256 KiB",
        ));
    }
    let mut parsed = ClaudeLevel {
        level,
        source_path,
        build_commands: Vec::new(),
        conventions: Vec::new(),
        gotchas: Vec::new(),
        constraints: Vec::new(),
        instructions: Vec::new(),
    };
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let body = line.strip_prefix('-').map(str::trim).unwrap_or(line);
        let lower = body.to_ascii_lowercase();
        if let Some(value) = strip_prefix_ci(&lower, body, "build:") {
            push_bounded(&mut parsed.build_commands, value, "build command")?;
        } else if let Some(value) = strip_prefix_ci(&lower, body, "convention:") {
            push_bounded(&mut parsed.conventions, value, "convention")?;
        } else if let Some(value) = strip_prefix_ci(&lower, body, "gotcha:") {
            push_bounded(&mut parsed.gotchas, value, "gotcha")?;
        } else if let Some(value) = strip_prefix_ci(&lower, body, "constraint:") {
            push_bounded(&mut parsed.constraints, value, "constraint")?;
        } else {
            push_bounded(&mut parsed.instructions, body, "instruction")?;
        }
    }
    Ok(parsed)
}

fn push_bounded(
    target: &mut Vec<String>,
    value: &str,
    what: &str,
) -> Result<(), crate::ParseError> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err(crate::ParseError::new(
            "claude",
            format!("{what} lines must be 1..=4096 chars"),
        ));
    }
    target.push(value.to_owned());
    Ok(())
}

/// Parse a full hierarchy: `texts[i]` is level `i` (missing levels are empty).
/// At most 6 levels; returns levels sorted outermost-first.
pub fn parse_claude_hierarchy(
    texts: Vec<(String, Option<String>)>,
) -> Result<ClaudeFile, crate::ParseError> {
    if texts.len() > HIERARCHY_DEPTH {
        return Err(crate::ParseError::new(
            "claude",
            "at most 6 hierarchy levels",
        ));
    }
    let mut levels = Vec::new();
    for (index, (text, path)) in texts.into_iter().enumerate() {
        levels.push(parse_claude_level(&text, index, path)?);
    }
    Ok(ClaudeFile { levels })
}

/// Discover CLAUDE.md files from `workspace_root` outward: global
/// (`~/.talos/CLAUDE.md`), project root, then nested `dir` parts (max 4).
/// Missing files are skipped; found files parse deterministically.
pub fn discover_claude_files(
    workspace_root: &std::path::Path,
    nested: Option<&std::path::Path>,
) -> Vec<(std::path::PathBuf, usize)> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        let global = home.join(".talos").join("CLAUDE.md");
        if global.is_file() {
            out.push((global, 0));
        }
    }
    let root = workspace_root.join("CLAUDE.md");
    if root.is_file() {
        out.push((root, 1));
    }
    if let Some(nested) = nested {
        let mut current = workspace_root.to_path_buf();
        for component in nested.components().take(4) {
            current = current.join(component.as_os_str());
            let candidate = current.join("CLAUDE.md");
            if candidate.is_file() {
                let level = 2 + out.iter().filter(|(_, level)| *level >= 2).count();
                if level < HIERARCHY_DEPTH {
                    out.push((candidate, level));
                }
            }
        }
    }
    out.sort_by_key(|(_, level)| *level);
    out
}

fn strip_prefix_ci<'a>(lower: &str, original: &'a str, prefix: &str) -> Option<&'a str> {
    if lower.starts_with(prefix) {
        Some(original[prefix.len()..].trim())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Project\nbuild: cargo test\nconvention: use snake_case\ngotcha: tests need network off\nconstraint: never commit secrets\nShip it.\n";

    #[test]
    fn parses_categories() {
        let level = parse_claude_level(SAMPLE, 1, None).unwrap();
        assert_eq!(level.build_commands, vec!["cargo test".to_owned()]);
        assert_eq!(level.conventions, vec!["use snake_case".to_owned()]);
        assert_eq!(level.gotchas, vec!["tests need network off".to_owned()]);
        assert_eq!(level.constraints, vec!["never commit secrets".to_owned()]);
        assert_eq!(level.instructions, vec!["Ship it.".to_owned()]);
    }

    #[test]
    fn six_level_hierarchy_works() {
        let texts = (0..6)
            .map(|index| (format!("level {index} instruction"), None))
            .collect();
        let file = parse_claude_hierarchy(texts).unwrap();
        assert_eq!(file.levels.len(), 6);
        assert_eq!(file.levels[5].level, 5);
        assert_eq!(file.merged_instructions().len(), 6);
        // Deterministic: same input, same output.
        let texts_again = (0..6)
            .map(|index| (format!("level {index} instruction"), None))
            .collect();
        assert_eq!(file, parse_claude_hierarchy(texts_again).unwrap());
    }

    #[test]
    fn rejects_seventh_level() {
        let texts = (0..7).map(|_| (String::new(), None)).collect();
        assert!(parse_claude_hierarchy(texts).is_err());
        assert!(parse_claude_level("x", 6, None).is_err());
    }

    #[test]
    fn merges_constraints_and_builds() {
        let file = parse_claude_hierarchy(vec![
            ("constraint: a".to_owned(), None),
            ("build: make\nconstraint: b".to_owned(), None),
        ])
        .unwrap();
        assert_eq!(file.merged_constraints().len(), 2);
        assert_eq!(file.merged_build_commands(), vec!["make".to_owned()]);
    }
}
