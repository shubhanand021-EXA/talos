//! MCP server configuration: `.talos/mcp.json` parsing and validation.
//!
//! Format:
//! ```json
//! { "servers": { "name": { "command": "...", "args": [...],
//!   "env": {...}, "capabilities": [...] } } }
//! ```
//! Stdio servers use `command` (+ `args`/`env`); SSE servers use
//! `transport: "sse"` plus `url`. Every server declares `capabilities` and a
//! response timeout (default 30s).

use std::collections::HashMap;

use crate::protocol::McpError;

/// Default response timeout (30s, per spec requirement).
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Maximum servers per config file (bounded discovery).
pub const MAX_SERVERS: usize = 64;

/// Transport kind for one MCP server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    /// Spawn a subprocess and speak JSON-RPC over stdio.
    #[default]
    Stdio,
    /// Speak JSON-RPC over HTTP/SSE.
    Sse,
}

/// One configured MCP server.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// Command to spawn (stdio only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Arguments for `command` (stdio only).
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment for the child (stdio only).
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// SSE endpoint URL (SSE only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Transport selector (defaults to stdio).
    #[serde(default)]
    pub transport: TransportKind,
    /// Declared capabilities (must be non-empty).
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Response timeout in milliseconds (default 30s, 100..=300_000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl McpServerConfig {
    /// Effective timeout (default 30s).
    #[must_use]
    pub fn effective_timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)
    }

    /// Validate one server entry.
    pub fn validate(&self, name: &str) -> Result<(), McpError> {
        if name.trim().is_empty() || name.len() > 128 {
            return Err(McpError::invalid("server name must be 1..=128 chars"));
        }
        if self.args.len() > 32 {
            return Err(McpError::invalid("args must have at most 32 entries"));
        }
        for arg in &self.args {
            if arg.len() > 1024 {
                return Err(McpError::invalid("each arg must be at most 1024 chars"));
            }
        }
        if self.env.len() > 64 {
            return Err(McpError::invalid("env must have at most 64 entries"));
        }
        if self.capabilities.is_empty() || self.capabilities.len() > 32 {
            return Err(McpError::invalid(
                "capabilities must declare 1..=32 entries",
            ));
        }
        for capability in &self.capabilities {
            if capability.trim().is_empty() || capability.len() > 128 {
                return Err(McpError::invalid("each capability must be 1..=128 chars"));
            }
        }
        let timeout = self.effective_timeout_ms();
        if !(100..=300_000).contains(&timeout) {
            return Err(McpError::invalid("timeoutMs must be 100..=300000"));
        }
        match self.transport {
            TransportKind::Stdio => {
                let command = self.command.as_deref().unwrap_or("").trim();
                if command.is_empty() || command.len() > 1024 {
                    return Err(McpError::invalid(
                        "stdio servers require command (1..=1024 chars)",
                    ));
                }
                if self.url.is_some() {
                    return Err(McpError::invalid("stdio servers must not set url"));
                }
            }
            TransportKind::Sse => {
                let url = self.url.as_deref().unwrap_or("");
                if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() > 2048
                {
                    return Err(McpError::invalid(
                        "sse servers require an http(s) url (max 2048 chars)",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Whole `.talos/mcp.json` file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpConfigFile {
    /// Server name → config.
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

impl McpConfigFile {
    /// Parse and validate raw JSON text (deterministic, bounded).
    pub fn parse(raw: &str) -> Result<Self, McpError> {
        if raw.len() > 262_144 {
            return Err(McpError::invalid("mcp.json must be at most 256 KiB"));
        }
        let file: Self = serde_json::from_str(raw)
            .map_err(|err| McpError::invalid(format!("mcp.json parse: {err}")))?;
        file.validate()?;
        Ok(file)
    }

    /// Validate every entry.
    pub fn validate(&self) -> Result<(), McpError> {
        if self.servers.len() > MAX_SERVERS {
            return Err(McpError::invalid("at most 64 MCP servers"));
        }
        let mut names: Vec<&String> = self.servers.keys().collect();
        names.sort();
        for name in names {
            let config = &self.servers[name];
            config.validate(name)?;
        }
        Ok(())
    }

    /// Load from `.talos/mcp.json` under `dir` (missing file = empty config).
    pub fn load_from_dir(dir: &std::path::Path) -> Result<Self, McpError> {
        let path = dir.join(".talos").join("mcp.json");
        match std::fs::read_to_string(&path) {
            Ok(raw) => Self::parse(&raw),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                servers: HashMap::new(),
            }),
            Err(err) => Err(McpError::transport(format!("read mcp.json: {err}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(command: &str) -> McpServerConfig {
        McpServerConfig {
            command: Some(command.to_owned()),
            args: vec!["--stdio".to_owned()],
            env: HashMap::new(),
            url: None,
            transport: TransportKind::Stdio,
            capabilities: vec!["tools".to_owned()],
            timeout_ms: None,
        }
    }

    #[test]
    fn parses_stdio_config() {
        let raw = serde_json::json!({
            "servers": {
                "fs": {"command": "npx", "args": ["-y", "mcp-server"], "env": {}, "capabilities": ["tools"]}
            }
        })
        .to_string();
        let file = McpConfigFile::parse(&raw).unwrap();
        assert_eq!(file.servers.len(), 1);
        assert_eq!(
            file.servers["fs"].effective_timeout_ms(),
            DEFAULT_TIMEOUT_MS
        );
    }

    #[test]
    fn parses_sse_config() {
        let raw = serde_json::json!({
            "servers": {
                "remote": {"transport": "sse", "url": "https://example.com/mcp", "capabilities": ["tools", "resources"]}
            }
        })
        .to_string();
        let file = McpConfigFile::parse(&raw).unwrap();
        assert_eq!(file.servers["remote"].transport, TransportKind::Sse);
    }

    #[test]
    fn rejects_missing_capabilities() {
        let mut config = stdio("npx");
        config.capabilities.clear();
        assert!(config.validate("fs").is_err());
    }

    #[test]
    fn rejects_stdio_without_command() {
        let mut config = stdio("npx");
        config.command = None;
        assert!(config.validate("fs").is_err());
    }

    #[test]
    fn rejects_sse_without_url() {
        let config = McpServerConfig {
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            transport: TransportKind::Sse,
            capabilities: vec!["tools".to_owned()],
            timeout_ms: None,
        };
        assert!(config.validate("remote").is_err());
    }

    #[test]
    fn rejects_bad_timeout() {
        let mut config = stdio("npx");
        config.timeout_ms = Some(10);
        assert!(config.validate("fs").is_err());
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("talos-mcp-test-{}", std::process::id()));
        let file = McpConfigFile::load_from_dir(&dir).unwrap();
        assert!(file.servers.is_empty());
    }
}
