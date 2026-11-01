//! Deterministic, versioned prompt templates.
//!
//! Every assembly stamps [`TEMPLATE_VERSION`]; identical inputs always render
//! identical text (sorted sections, fixed separators, no timestamps).

/// Prompt template version stamped on every assembly.
pub const TEMPLATE_VERSION: &str = "ctx-assembler/1";

/// Fixed section separators (ASCII, tokenizer-stable).
pub const SECTION_OPEN: &str = "## ";
pub const CAPABILITY_LINE: &str =
    "You are a Talos task agent. You act only through policy-gated tools, cite evidence for claims, and never bypass safety rules.";

/// Verbatim safety preamble: rules are stated, never summarized.
pub const SAFETY_PREAMBLE: &str = "Safety rules (verbatim, inviolable):";

/// Output contract appended to every system prompt.
pub const OUTPUT_CONTRACT: &str = "Output: concise final answer, or exactly one tool call in the required schema. No other text when calling a tool.";

/// Render the system prompt: role + capabilities + safety (verbatim) + output contract.
#[must_use]
pub fn render_system_prompt(safety_rules: &[String]) -> String {
    let mut out = String::from(CAPABILITY_LINE);
    out.push('\n');
    out.push_str(SAFETY_PREAMBLE);
    for rule in safety_rules {
        out.push('\n');
        out.push_str("- ");
        out.push_str(rule);
    }
    out.push('\n');
    out.push_str(OUTPUT_CONTRACT);
    out
}

/// Render the task context block.
#[must_use]
pub fn render_task_context(
    goal: &str,
    acceptance: &[String],
    state: &str,
    constraints: &[String],
) -> String {
    let mut out = format!("{SECTION_OPEN}Task\nGoal: {goal}\nState: {state}");
    if !acceptance.is_empty() {
        out.push_str("\nAcceptance:");
        for item in acceptance {
            out.push_str("\n- ");
            out.push_str(item);
        }
    }
    if !constraints.is_empty() {
        out.push_str("\nConstraints:");
        for item in constraints {
            out.push_str("\n- ");
            out.push_str(item);
        }
    }
    out
}

/// Render one evidence claim with its source ref.
#[must_use]
pub fn render_claim(statement: &str, evidence_id: &str) -> String {
    format!("- {statement} [evidence:{evidence_id}]")
}

/// Render the tool catalog block (rank order preserved).
#[must_use]
pub fn render_tool_context(tools: &[(String, String)]) -> String {
    let mut out = format!("{SECTION_OPEN}Tools");
    for (name, description) in tools {
        out.push('\n');
        out.push_str("- ");
        out.push_str(name);
        out.push_str(": ");
        out.push_str(description);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_states_rules_verbatim() {
        let prompt = render_system_prompt(&["deny writes without evidence".to_owned()]);
        assert!(prompt.contains(SAFETY_PREAMBLE));
        assert!(prompt.contains("- deny writes without evidence"));
        assert!(prompt.contains(OUTPUT_CONTRACT));
        // Deterministic: same inputs, byte-identical output.
        assert_eq!(
            prompt,
            render_system_prompt(&["deny writes without evidence".to_owned()])
        );
    }

    #[test]
    fn task_and_tool_blocks_are_deterministic() {
        let task = render_task_context("ship it", &["green".to_owned()], "executing", &[]);
        assert!(task.contains("Goal: ship it"));
        assert_eq!(
            render_tool_context(&[
                ("b".to_owned(), "B".to_owned()),
                ("a".to_owned(), "A".to_owned())
            ]),
            render_tool_context(&[
                ("b".to_owned(), "B".to_owned()),
                ("a".to_owned(), "A".to_owned())
            ])
        );
        assert_eq!(render_claim("s", "e-1"), "- s [evidence:e-1]");
    }
}
