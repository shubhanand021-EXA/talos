//! AGENTS.md parser: global + per-agent instructions, tool allow/deny lists,
//! and model preferences.
//!
//! Format (deterministic line grammar):
//! - `#` headings set the scope: `# Global` (default), `## Agent: <name>`.
//! - `- allow: <tool>` / `- deny: <tool>` lines declare tool gates.
//! - `- model: <id>` lines declare a model preference for the scope.
//! - Every other non-empty line is an instruction for the current scope.

/// Instructions scoped to one agent (or global).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentScope {
    /// Agent name (`global` for the default scope).
    pub agent: String,
    /// Verbatim instruction lines.
    pub instructions: Vec<String>,
    /// Allowed tools declared in this scope.
    pub allow_tools: Vec<String>,
    /// Denied tools declared in this scope.
    pub deny_tools: Vec<String>,
    /// Model preference declared in this scope, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Parsed AGENTS.md document.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsFile {
    /// Global instructions (apply to all agents).
    pub global: AgentScope,
    /// Per-agent scopes, sorted by agent name.
    pub agents: Vec<AgentScope>,
    /// Source path the document was read from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

impl AgentsFile {
    /// Instructions visible to `agent` (global + matching scope, verbatim).
    #[must_use]
    pub fn instructions_for(&self, agent: &str) -> Vec<String> {
        let mut out = self.global.instructions.clone();
        if let Some(scope) = self.agents.iter().find(|scope| scope.agent == agent) {
            out.extend(scope.instructions.iter().cloned());
        }
        out
    }

    /// Effective allow-list for `agent` (global + scope, sorted, deduped).
    #[must_use]
    pub fn allow_for(&self, agent: &str) -> Vec<String> {
        let mut out = self.global.allow_tools.clone();
        if let Some(scope) = self.agents.iter().find(|scope| scope.agent == agent) {
            out.extend(scope.allow_tools.iter().cloned());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Effective deny-list for `agent` (global + scope, sorted, deduped).
    #[must_use]
    pub fn deny_for(&self, agent: &str) -> Vec<String> {
        let mut out = self.global.deny_tools.clone();
        if let Some(scope) = self.agents.iter().find(|scope| scope.agent == agent) {
            out.extend(scope.deny_tools.iter().cloned());
        }
        out.sort();
        out.dedup();
        out
    }
}

fn new_scope(agent: &str) -> AgentScope {
    AgentScope {
        agent: agent.to_owned(),
        instructions: Vec::new(),
        allow_tools: Vec::new(),
        deny_tools: Vec::new(),
        model: None,
    }
}

/// Parse AGENTS.md text (deterministic; never executes anything).
pub fn parse_agents(
    text: &str,
    source_path: Option<String>,
) -> Result<AgentsFile, crate::ParseError> {
    if text.len() > 262_144 {
        return Err(crate::ParseError::new(
            "agents",
            "AGENTS.md must be at most 256 KiB",
        ));
    }
    let mut global = new_scope("global");
    let mut agents: std::collections::HashMap<String, AgentScope> =
        std::collections::HashMap::new();
    let mut current: Option<String> = None;

    let push_directive = |current: &Option<String>,
                          global: &mut AgentScope,
                          agents: &mut std::collections::HashMap<String, AgentScope>,
                          apply: &dyn Fn(&mut AgentScope)| {
        if let Some(name) = current {
            apply(
                agents
                    .entry(name.clone())
                    .or_insert_with(|| new_scope(name)),
            );
        } else {
            apply(global);
        }
    };

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        // Scope headings: `# ...` resets to global, `## Agent: <name>` scopes.
        if line.starts_with('#') {
            let heading = line.trim_start_matches('#').trim();
            if let Some(rest) = heading.strip_prefix("Agent:").map(str::trim) {
                if rest.is_empty() || rest.len() > 64 {
                    return Err(crate::ParseError::new(
                        "agents",
                        "agent name must be 1..=64 chars",
                    ));
                }
                current = Some(rest.to_owned());
            } else {
                current = None;
            }
            continue;
        }
        let body = line.strip_prefix('-').map(str::trim).unwrap_or(line);
        let lower = body.to_ascii_lowercase();
        if let Some(tool) = strip_prefix_ci(&lower, body, "allow:") {
            if tool.is_empty() || tool.len() > 128 {
                return Err(crate::ParseError::new(
                    "agents",
                    "allow tool must be 1..=128 chars",
                ));
            }
            let owned = tool.to_owned();
            push_directive(&current, &mut global, &mut agents, &|scope| {
                scope.allow_tools.push(owned.clone());
            });
        } else if let Some(tool) = strip_prefix_ci(&lower, body, "deny:") {
            if tool.is_empty() || tool.len() > 128 {
                return Err(crate::ParseError::new(
                    "agents",
                    "deny tool must be 1..=128 chars",
                ));
            }
            let owned = tool.to_owned();
            push_directive(&current, &mut global, &mut agents, &|scope| {
                scope.deny_tools.push(owned.clone());
            });
        } else if let Some(model) = strip_prefix_ci(&lower, body, "model:") {
            if model.is_empty() || model.len() > 128 {
                return Err(crate::ParseError::new(
                    "agents",
                    "model must be 1..=128 chars",
                ));
            }
            let owned = model.to_owned();
            push_directive(&current, &mut global, &mut agents, &|scope| {
                scope.model = Some(owned.clone());
            });
        } else {
            if body.len() > 4096 {
                return Err(crate::ParseError::new(
                    "agents",
                    "instruction lines must be at most 4096 chars",
                ));
            }
            let owned = body.to_owned();
            push_directive(&current, &mut global, &mut agents, &|scope| {
                scope.instructions.push(owned.clone());
            });
        }
    }

    let mut agent_list: Vec<AgentScope> = agents.into_values().collect();
    for scope in std::iter::once(&mut global).chain(agent_list.iter_mut()) {
        scope.allow_tools.sort();
        scope.allow_tools.dedup();
        scope.deny_tools.sort();
        scope.deny_tools.dedup();
    }
    agent_list.sort_by(|left, right| left.agent.cmp(&right.agent));
    Ok(AgentsFile {
        global,
        agents: agent_list,
        source_path,
    })
}

/// Case-insensitive prefix strip: match on `lower`, slice `original`.
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

    const SAMPLE: &str = "# Global\nBuild with cargo.\n- allow: fs.read\n- model: gpt-4o\n## Agent: coder\nUse workspaces.\n- deny: process.exec\n";

    #[test]
    fn extracts_instructions_correctly() {
        let file = parse_agents(SAMPLE, None).unwrap();
        assert_eq!(
            file.global.instructions,
            vec!["Build with cargo.".to_owned()]
        );
        assert_eq!(file.global.allow_tools, vec!["fs.read".to_owned()]);
        assert_eq!(file.global.model.as_deref(), Some("gpt-4o"));
        assert_eq!(file.agents.len(), 1);
        assert_eq!(file.agents[0].agent, "coder");
        assert_eq!(
            file.agents[0].instructions,
            vec!["Use workspaces.".to_owned()]
        );
        assert_eq!(file.agents[0].deny_tools, vec!["process.exec".to_owned()]);
        let merged = file.instructions_for("coder");
        assert_eq!(merged.len(), 2);
        assert!(file.instructions_for("other") == vec!["Build with cargo.".to_owned()]);
        assert_eq!(file.allow_for("coder"), vec!["fs.read".to_owned()]);
        assert_eq!(file.deny_for("coder"), vec!["process.exec".to_owned()]);
    }

    #[test]
    fn deterministic_scope_order() {
        let text = "## Agent: zeta\n- allow: fs.read\n## Agent: alpha\n- allow: fs.list\n";
        let first = parse_agents(text, None).unwrap();
        let second = parse_agents(text, None).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.agents[0].agent, "alpha");
        assert_eq!(first.agents[1].agent, "zeta");
    }

    #[test]
    fn rejects_oversize() {
        assert!(parse_agents(&"x".repeat(300_000), None).is_err());
    }
}
