//! Tool registry: registration, metadata, and parameter validation.
//!
//! The registry owns the five baseline manifests and per-tool parameter
//! validators. Execution lives in [`crate::executor`]; adapters in
//! [`crate::adapters`]. Schemas are exposed verbatim for UI forms and future
//! Tool-RAG retrieval.

use std::collections::HashMap;

use talos_common::{Result, TalosError};

use crate::model::*;

fn schema(
    required: &[&str],
    properties: serde_json::Value,
    description: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": description,
        "required": required,
        "properties": properties,
        "additionalProperties": false,
    })
}

fn output_schema(description: &str, properties: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": description,
        "properties": properties,
    })
}

/// Manifest builder (arity is inherent to the metadata shape).
#[allow(clippy::too_many_arguments)]
fn manifest(
    id: &str,
    name: &str,
    description: &str,
    capabilities: Vec<policy_broker::Capability>,
    risk_level: policy_broker::RiskLevel,
    reversible: bool,
    requires_evidence: bool,
    requires_compensation: bool,
    enabled_by_default: bool,
    input_schema: serde_json::Value,
    output_schema: serde_json::Value,
) -> ToolManifest {
    ToolManifest {
        id: id.to_owned(),
        name: name.to_owned(),
        version: "0.1.0".to_owned(),
        description: description.to_owned(),
        capabilities,
        risk_level,
        reversible,
        requires_evidence,
        requires_compensation,
        enabled_by_default,
        input_schema,
        output_schema,
    }
}

/// Baseline manifests (registration order is stable).
fn builtin_manifests() -> Vec<ToolManifest> {
    use policy_broker::{Capability, RiskLevel};
    vec![
        manifest(
            FS_READ,
            "fs.read",
            "Read a file under the sandbox root; returns metadata, hash, and UTF-8 text when decodable.",
            vec![Capability::ReadFile],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["path"],
                serde_json::json!({ "path": { "type": "string", "description": "Sandbox-relative file path" } }),
                "fs.read input",
            ),
            output_schema(
                "fs.read output",
                serde_json::json!({
                    "path": { "type": "string" },
                    "bytes": { "type": "integer" },
                    "hash": { "type": "string" },
                    "truncated": { "type": "boolean" },
                    "text": { "type": ["string", "null"] },
                }),
            ),
        ),
        manifest(
            FS_LIST,
            "fs.list",
            "List a directory under the sandbox root (bounded depth, no symlink escape).",
            vec![Capability::ReadFile],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["path"],
                serde_json::json!({
                    "path": { "type": "string", "description": "Sandbox-relative directory (default .)" },
                    "depth": { "type": "integer", "description": "Recursion depth 0..=4 (default 1)" },
                }),
                "fs.list input",
            ),
            output_schema(
                "fs.list output",
                serde_json::json!({
                    "path": { "type": "string" },
                    "entries": { "type": "array" },
                    "truncated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            FS_SEARCH,
            "fs.search",
            "Search UTF-8 text content under the sandbox root (bounded files and matches).",
            vec![Capability::ReadFile],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["query"],
                serde_json::json!({
                    "query": { "type": "string", "description": "Substring to find (1..=256 chars)" },
                    "path": { "type": "string", "description": "Subdirectory scope (default .)" },
                    "maxMatches": { "type": "integer", "description": "Cap 1..=50 (default 20)" },
                }),
                "fs.search input",
            ),
            output_schema(
                "fs.search output",
                serde_json::json!({
                    "query": { "type": "string" },
                    "matches": { "type": "array" },
                    "truncated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            FS_WRITE,
            "fs.write",
            "Write a file under the sandbox root. Requires policy approval + evidence; creates backup/compensation metadata first.",
            vec![Capability::WriteFile],
            RiskLevel::Medium,
            true,
            true,
            true,
            true,
            schema(
                &["path", "content"],
                serde_json::json!({
                    "path": { "type": "string", "description": "Sandbox-relative file path" },
                    "content": { "type": "string", "description": "UTF-8 content (max 262144 chars)" },
                    "createParents": { "type": "boolean", "description": "Create missing parents (default true)" },
                }),
                "fs.write input",
            ),
            output_schema(
                "fs.write output",
                serde_json::json!({
                    "path": { "type": "string" },
                    "bytes": { "type": "integer" },
                    "hash": { "type": "string" },
                    "overwrote": { "type": "boolean" },
                    "backup": { "type": "object" },
                }),
            ),
        ),
        manifest(
            PROCESS_EXEC,
            "process.exec",
            "Execute a subprocess. Disabled by default; dry-run first; allowlisted commands, timeout, captured output.",
            vec![Capability::ExecuteCommand],
            RiskLevel::High,
            false,
            true,
            false,
            false,
            schema(
                &["command"],
                serde_json::json!({
                    "command": { "type": "string", "description": "Executable name (must be allowlisted)" },
                    "args": { "type": "array", "description": "Arguments (max 32, each max 1024 chars)" },
                    "timeoutMs": { "type": "integer", "description": "Timeout 100..60000 (default 5000)" },
                }),
                "process.exec input",
            ),
            output_schema(
                "process.exec output",
                serde_json::json!({
                    "command": { "type": "string" },
                    "exitCode": { "type": ["integer", "null"] },
                    "stdout": { "type": "string" },
                    "stderr": { "type": "string" },
                    "timedOut": { "type": "boolean" },
                }),
            ),
        ),
    ]
}

/// Validated, tool-specific parameters.
#[derive(Debug, Clone)]
pub enum ValidatedParams {
    /// `fs.read { path }`.
    FsRead {
        /// Requested path.
        path: String,
    },
    /// `fs.list { path?, depth? }`.
    FsList {
        /// Directory scope.
        path: String,
        /// Recursion depth.
        depth: u8,
    },
    /// `fs.search { query, path?, maxMatches? }`.
    FsSearch {
        /// Substring query.
        query: String,
        /// Scope directory.
        path: String,
        /// Match cap.
        max_matches: usize,
    },
    /// `fs.write { path, content, createParents? }`.
    FsWrite {
        /// Destination path.
        path: String,
        /// UTF-8 content.
        content: String,
        /// Create parents.
        create_parents: bool,
    },
    /// `process.exec { command, args?, timeoutMs? }`.
    ProcessExec {
        /// Executable.
        command: String,
        /// Arguments.
        args: Vec<String>,
        /// Timeout.
        timeout_ms: u64,
    },
}

fn err(msg: &str) -> TalosError {
    TalosError::Storage(msg.to_owned())
}

fn get_str(params: &serde_json::Value, key: &str, required: bool) -> Result<Option<String>> {
    match params.get(key) {
        None => {
            if required {
                return Err(err(&format!("missing required param '{key}'")));
            }
            Ok(None)
        }
        Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(err(&format!("param '{key}' must be a string"))),
    }
}

fn get_int(params: &serde_json::Value, key: &str) -> Result<Option<i64>> {
    match params.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| err(&format!("param '{key}' must be an integer"))),
        Some(_) => Err(err(&format!("param '{key}' must be an integer"))),
    }
}

/// Validate raw params against a tool's manifest (strict: no unknown keys).
pub fn validate_params(tool_id: &str, params: &serde_json::Value) -> Result<ValidatedParams> {
    let object = params
        .as_object()
        .ok_or_else(|| err("params must be an object"))?;
    let allowed: &[&str] = match tool_id {
        FS_READ => &["path"],
        FS_LIST => &["path", "depth"],
        FS_SEARCH => &["query", "path", "maxMatches"],
        FS_WRITE => &["path", "content", "createParents"],
        PROCESS_EXEC => &["command", "args", "timeoutMs"],
        _ => return Err(err("unknown tool")),
    };
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(err(&format!("unknown param '{key}'")));
        }
    }
    match tool_id {
        FS_READ => {
            let path = get_str(params, "path", true)?.unwrap_or_default();
            if path.trim().is_empty() || path.len() > 1024 {
                return Err(err("path must be 1..=1024 chars"));
            }
            Ok(ValidatedParams::FsRead { path })
        }
        FS_LIST => {
            let path = get_str(params, "path", false)?.unwrap_or_else(|| ".".to_owned());
            if path.is_empty() || path.len() > 1024 {
                return Err(err("path must be 1..=1024 chars"));
            }
            let depth = get_int(params, "depth")?.unwrap_or(1);
            if !(0..=4).contains(&depth) {
                return Err(err("depth must be 0..=4"));
            }
            Ok(ValidatedParams::FsList {
                path,
                depth: depth as u8,
            })
        }
        FS_SEARCH => {
            let query = get_str(params, "query", true)?.unwrap_or_default();
            if query.trim().is_empty() || query.len() > 256 {
                return Err(err("query must be 1..=256 chars"));
            }
            let path = get_str(params, "path", false)?.unwrap_or_else(|| ".".to_owned());
            if path.len() > 1024 {
                return Err(err("path must be <=1024 chars"));
            }
            let max_matches = get_int(params, "maxMatches")?.unwrap_or(20);
            if !(1..=50).contains(&max_matches) {
                return Err(err("maxMatches must be 1..=50"));
            }
            Ok(ValidatedParams::FsSearch {
                query,
                path,
                max_matches: max_matches as usize,
            })
        }
        FS_WRITE => {
            let path = get_str(params, "path", true)?.unwrap_or_default();
            if path.trim().is_empty() || path.len() > 1024 {
                return Err(err("path must be 1..=1024 chars"));
            }
            let content = get_str(params, "content", true)?.unwrap_or_default();
            if content.len() > 262_144 {
                return Err(err("content exceeds 262144 chars"));
            }
            let create_parents = match params.get("createParents") {
                None => true,
                Some(serde_json::Value::Bool(b)) => *b,
                Some(_) => return Err(err("createParents must be a boolean")),
            };
            Ok(ValidatedParams::FsWrite {
                path,
                content,
                create_parents,
            })
        }
        PROCESS_EXEC => {
            let command = get_str(params, "command", true)?.unwrap_or_default();
            if command.trim().is_empty() || command.len() > 256 {
                return Err(err("command must be 1..=256 chars"));
            }
            let args = match params.get("args") {
                None => Vec::new(),
                Some(serde_json::Value::Array(items)) => {
                    if items.len() > 32 {
                        return Err(err("args exceeds 32 items"));
                    }
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            serde_json::Value::String(s) => {
                                if s.len() > 1024 {
                                    return Err(err("arg exceeds 1024 chars"));
                                }
                                out.push(s.clone());
                            }
                            _ => return Err(err("args must be strings")),
                        }
                    }
                    out
                }
                Some(_) => return Err(err("args must be an array")),
            };
            let timeout_ms = get_int(params, "timeoutMs")?.unwrap_or(5_000);
            if !(100..=60_000).contains(&timeout_ms) {
                return Err(err("timeoutMs must be 100..60000"));
            }
            Ok(ValidatedParams::ProcessExec {
                command,
                args,
                timeout_ms: timeout_ms as u64,
            })
        }
        _ => Err(err("unknown tool")),
    }
}

/// Tool registry: manifests + enablement + validation + metadata exposure.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    manifests: HashMap<String, ToolManifest>,
    enabled: HashMap<String, bool>,
}

impl ToolRegistry {
    /// Build a registry with the five baseline tools registered.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut manifests = HashMap::new();
        let mut enabled = HashMap::new();
        for manifest in builtin_manifests() {
            enabled.insert(manifest.id.clone(), manifest.enabled_by_default);
            manifests.insert(manifest.id.clone(), manifest);
        }
        Self { manifests, enabled }
    }

    /// Register (or replace) a manifest (custom tools arrive later).
    pub fn register(&mut self, manifest: ToolManifest) {
        self.enabled
            .entry(manifest.id.clone())
            .or_insert(manifest.enabled_by_default);
        self.manifests.insert(manifest.id.clone(), manifest);
    }

    /// List all manifests in stable (id-sorted) order.
    #[must_use]
    pub fn list(&self) -> Vec<ToolManifest> {
        let mut out: Vec<ToolManifest> = self.manifests.values().cloned().collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Fetch one manifest by id.
    #[must_use]
    pub fn get(&self, tool_id: &str) -> Option<&ToolManifest> {
        self.manifests.get(tool_id)
    }

    /// True when live execution is allowed (dry-run is always evaluated).
    #[must_use]
    pub fn is_enabled(&self, tool_id: &str) -> bool {
        self.enabled.get(tool_id).copied().unwrap_or(false)
    }

    /// Explicitly enable or disable a tool (e.g. opt into `process.exec`).
    pub fn set_enabled(&mut self, tool_id: &str, enabled: bool) -> Result<()> {
        if !self.manifests.contains_key(tool_id) {
            return Err(err("unknown tool"));
        }
        self.enabled.insert(tool_id.to_owned(), enabled);
        Ok(())
    }

    /// Validate an invocation's params against its manifest.
    pub fn validate(&self, tool_id: &str, params: &serde_json::Value) -> Result<ValidatedParams> {
        if !self.manifests.contains_key(tool_id) {
            return Err(err("unknown tool"));
        }
        validate_params(tool_id, params)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_five_builtins() {
        let registry = ToolRegistry::with_builtins();
        let listed = registry.list();
        let ids: Vec<&str> = listed.iter().map(|manifest| manifest.id.as_str()).collect();
        assert_eq!(ids, [FS_LIST, FS_READ, FS_SEARCH, FS_WRITE, PROCESS_EXEC]);
    }

    #[test]
    fn exec_disabled_by_default() {
        let registry = ToolRegistry::with_builtins();
        assert!(registry.is_enabled(FS_READ));
        assert!(!registry.is_enabled(PROCESS_EXEC));
    }

    #[test]
    fn invalid_params_rejected() {
        let registry = ToolRegistry::with_builtins();
        assert!(registry.validate(FS_READ, &serde_json::json!({})).is_err());
        assert!(registry
            .validate(FS_READ, &serde_json::json!({ "path": "a", "extra": 1 }))
            .is_err());
        assert!(registry.validate("nope", &serde_json::json!({})).is_err());
        assert!(registry
            .validate(FS_SEARCH, &serde_json::json!({ "query": "", "path": "." }))
            .is_err());
    }
}
