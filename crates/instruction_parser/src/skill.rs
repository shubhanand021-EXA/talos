//! SKILL.md parser: YAML-ish frontmatter (`name`, `description`, `triggers`,
//! `required_tools`) plus a procedure body of numbered steps.
//!
//! The parsed skill converts to a Talos `NewSkill`-shaped payload
//! (`quarantined` by birth) so it can be stored in the `skills` table.

/// Parsed SKILL.md document.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
    /// Skill name (frontmatter `name`).
    pub name: String,
    /// What the skill does (frontmatter `description`).
    pub description: String,
    /// Trigger phrases that should recall this skill.
    pub triggers: Vec<String>,
    /// Tool ids the procedure needs.
    pub required_tools: Vec<String>,
    /// Procedure steps in order (body, verbatim).
    pub steps: Vec<String>,
    /// Full body text (verbatim, for `procedure_text`).
    pub body: String,
    /// Source path the skill was read from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

impl SkillFile {
    /// Skill id derived from the name (lowercase, non-alnum → `-`).
    #[must_use]
    pub fn skill_id(&self) -> String {
        let mut id = String::new();
        let mut last_dash = true;
        for ch in self.name.to_ascii_lowercase().chars() {
            if ch.is_ascii_alphanumeric() {
                id.push(ch);
                last_dash = false;
            } else if !last_dash {
                id.push('-');
                last_dash = true;
            }
        }
        let trimmed = id.trim_matches('-').to_owned();
        if trimmed.is_empty() {
            "skill".to_owned()
        } else {
            trimmed.chars().take(64).collect()
        }
    }

    /// Permission ceiling heuristic: irreversible when the body mentions
    /// delete/destroy/publish, high for exec/commit, else low/medium.
    #[must_use]
    pub fn permission_ceiling(&self) -> String {
        let body = self.body.to_ascii_lowercase();
        if body.contains("delete") || body.contains("destroy") || body.contains("publish") {
            "irreversible".to_owned()
        } else if body.contains("exec") || body.contains("commit") || body.contains("write") {
            "high".to_owned()
        } else if self.required_tools.iter().any(|tool| {
            tool.starts_with("fs.") && tool != "fs.read" && tool != "fs.list" && tool != "fs.search"
        }) {
            "medium".to_owned()
        } else {
            "low".to_owned()
        }
    }

    /// Convert to an evidence-store skill payload (caller inserts it).
    #[must_use]
    pub fn to_skill_payload(&self, version: &str) -> serde_json::Value {
        serde_json::json!({
            "id": self.skill_id(),
            "name": self.name,
            "description": self.description,
            "procedure_text": self.body,
            "version": version,
            "required_tools": self.required_tools,
            "permission_ceiling": self.permission_ceiling(),
        })
    }
}

/// Parse SKILL.md text (frontmatter between `---` fences + body).
pub fn parse_skill(
    text: &str,
    source_path: Option<String>,
) -> Result<SkillFile, crate::ParseError> {
    if text.len() > 262_144 {
        return Err(crate::ParseError::new(
            "skill",
            "SKILL.md must be at most 256 KiB",
        ));
    }
    let (frontmatter, body) = split_frontmatter(text)?;
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut triggers: Vec<String> = Vec::new();
    let mut required_tools: Vec<String> = Vec::new();
    for raw_line in frontmatter.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "name" => name = Some(bounded(value.trim(), "name", 128)?),
            "description" => description = Some(bounded(value.trim(), "description", 2048)?),
            "triggers" => triggers = parse_list(value.trim())?,
            "required_tools" => required_tools = parse_list(value.trim())?,
            _ => {}
        }
    }
    let name = name
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ParseError::new("skill", "frontmatter requires a non-empty name"))?;
    let description = description
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::ParseError::new("skill", "frontmatter requires a non-empty description")
        })?;
    if body.trim().is_empty() {
        return Err(crate::ParseError::new("skill", "body must not be empty"));
    }
    let steps = parse_steps(&body);
    if steps.is_empty() {
        return Err(crate::ParseError::new(
            "skill",
            "body must contain at least one step",
        ));
    }
    Ok(SkillFile {
        name,
        description,
        triggers,
        required_tools,
        steps,
        body: body.trim().to_owned(),
        source_path,
    })
}

fn split_frontmatter(text: &str) -> Result<(String, String), crate::ParseError> {
    let all: Vec<&str> = text.lines().collect();
    if all.first().map(|line| line.trim()) != Some("---") {
        return Err(crate::ParseError::new(
            "skill",
            "missing opening --- frontmatter fence",
        ));
    }
    let mut close_index = None;
    for (index, line) in all.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            close_index = Some(index);
            break;
        }
    }
    let Some(close) = close_index else {
        return Err(crate::ParseError::new(
            "skill",
            "missing closing --- frontmatter fence",
        ));
    };
    Ok((all[1..close].join("\n"), all[close + 1..].join("\n")))
}

fn bounded(value: &str, what: &str, max: usize) -> Result<String, crate::ParseError> {
    if value.is_empty() || value.len() > max {
        return Err(crate::ParseError::new(
            "skill",
            format!("{what} must be 1..={max} chars"),
        ));
    }
    Ok(value.to_owned())
}

fn parse_list(value: &str) -> Result<Vec<String>, crate::ParseError> {
    let inner = value.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for item in inner.split(',') {
        let cleaned = item.trim().trim_matches('"').trim_matches('\'').trim();
        if cleaned.is_empty() || cleaned.len() > 128 {
            return Err(crate::ParseError::new(
                "skill",
                "list items must be 1..=128 chars",
            ));
        }
        if out.len() >= 32 {
            return Err(crate::ParseError::new(
                "skill",
                "lists hold at most 32 items",
            ));
        }
        out.push(cleaned.to_owned());
    }
    Ok(out)
}

fn parse_steps(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            // Strip `1. ` / `- ` / `* ` markers, keep the text verbatim after.
            let mut text = line;
            if let Some(dot) = text.find(". ") {
                let (number, _) = text.split_at(dot);
                if number.chars().all(|ch| ch.is_ascii_digit()) {
                    text = text[dot + 2..].trim();
                }
            }
            text.strip_prefix("- ")
                .map(str::trim)
                .or_else(|| text.strip_prefix("* ").map(str::trim))
                .unwrap_or(text)
                .to_owned()
        })
        .filter(|step| !step.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\nname: review-queue\ndescription: Review the task queue.\ntriggers: [review, queue]\nrequired_tools: [fs.read, fs.list]\n---\n1. List the queue.\n2. Read each task.\n3. Summarize findings.\n";

    #[test]
    fn frontmatter_plus_body_parsed() {
        let skill = parse_skill(SAMPLE, None).unwrap();
        assert_eq!(skill.name, "review-queue");
        assert_eq!(skill.description, "Review the task queue.");
        assert_eq!(
            skill.triggers,
            vec!["review".to_owned(), "queue".to_owned()]
        );
        assert_eq!(
            skill.required_tools,
            vec!["fs.read".to_owned(), "fs.list".to_owned()]
        );
        assert_eq!(skill.steps.len(), 3);
        assert_eq!(skill.skill_id(), "review-queue");
        assert_eq!(skill.permission_ceiling(), "low");
    }

    #[test]
    fn deterministic_parse() {
        assert_eq!(
            parse_skill(SAMPLE, None).unwrap(),
            parse_skill(SAMPLE, None).unwrap()
        );
    }

    #[test]
    fn rejects_missing_frontmatter() {
        assert!(parse_skill("no fences here", None).is_err());
        assert!(parse_skill("---\nname: x\n", None).is_err());
    }

    #[test]
    fn rejects_missing_name() {
        let bad = "---\ndescription: no name\n---\n1. Do it.\n";
        assert!(parse_skill(bad, None).is_err());
    }
}
