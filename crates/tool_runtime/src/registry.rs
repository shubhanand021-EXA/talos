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
        manifest(
            COMPUTER_MOVE_MOUSE,
            "computer.move_mouse",
            "Move the pointer on the virtual screen. Evidence-gated, reversible; dry-run validates bounds without moving.",
            vec![Capability::UiAction],
            RiskLevel::Medium,
            true,
            true,
            false,
            false,
            schema(
                &["x", "y"],
                serde_json::json!({
                    "x": { "type": "integer", "description": "X in virtual-screen pixels" },
                    "y": { "type": "integer", "description": "Y in virtual-screen pixels" },
                }),
                "computer.move_mouse input",
            ),
            output_schema(
                "computer.move_mouse output",
                serde_json::json!({
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_CLICK,
            "computer.click",
            "Single click at coordinates or a selector. Human-review gate; dry-run resolves without clicking.",
            vec![Capability::UiAction],
            RiskLevel::High,
            false,
            true,
            false,
            false,
            schema(
                &[],
                serde_json::json!({
                    "x": { "type": "integer", "description": "X in virtual-screen pixels (or selector)" },
                    "y": { "type": "integer", "description": "Y in virtual-screen pixels (or selector)" },
                    "selector": { "type": "string", "description": "Element selector, 1..=512 chars" },
                    "appHint": { "type": "string", "description": "Application hint for dangerous-zone matching" },
                    "highRisk": { "type": "boolean", "description": "Explicit high-risk flag (default false)" },
                }),
                "computer.click input",
            ),
            output_schema(
                "computer.click output",
                serde_json::json!({
                    "boundingBox": { "type": "object" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_DOUBLE_CLICK,
            "computer.double_click",
            "Double click at coordinates or a selector. Human-review gate; dry-run resolves without clicking.",
            vec![Capability::UiAction],
            RiskLevel::High,
            false,
            true,
            false,
            false,
            schema(
                &[],
                serde_json::json!({
                    "x": { "type": "integer", "description": "X in virtual-screen pixels (or selector)" },
                    "y": { "type": "integer", "description": "Y in virtual-screen pixels (or selector)" },
                    "selector": { "type": "string", "description": "Element selector, 1..=512 chars" },
                    "appHint": { "type": "string", "description": "Application hint for dangerous-zone matching" },
                    "highRisk": { "type": "boolean", "description": "Explicit high-risk flag (default false)" },
                }),
                "computer.double_click input",
            ),
            output_schema(
                "computer.double_click output",
                serde_json::json!({
                    "boundingBox": { "type": "object" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_DRAG,
            "computer.drag",
            "Press-drag-release gesture. Human-review gate; dry-run validates bounds without moving.",
            vec![Capability::UiAction],
            RiskLevel::High,
            false,
            true,
            false,
            false,
            schema(
                &["x", "y", "x2", "y2"],
                serde_json::json!({
                    "x": { "type": "integer", "description": "Start X in virtual-screen pixels" },
                    "y": { "type": "integer", "description": "Start Y in virtual-screen pixels" },
                    "x2": { "type": "integer", "description": "End X in virtual-screen pixels" },
                    "y2": { "type": "integer", "description": "End Y in virtual-screen pixels" },
                    "appHint": { "type": "string", "description": "Application hint for dangerous-zone matching" },
                    "highRisk": { "type": "boolean", "description": "Explicit high-risk flag (default false)" },
                }),
                "computer.drag input",
            ),
            output_schema(
                "computer.drag output",
                serde_json::json!({
                    "from": { "type": "object" },
                    "to": { "type": "object" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_TYPE_TEXT,
            "computer.type_text",
            "Type text, optionally at a focused element. Human-review gate; dry-run validates without typing.",
            vec![Capability::UiAction],
            RiskLevel::High,
            false,
            true,
            false,
            false,
            schema(
                &["text"],
                serde_json::json!({
                    "text": { "type": "string", "description": "Text to type, 1..=4096 chars" },
                    "x": { "type": "integer", "description": "Optional focus X" },
                    "y": { "type": "integer", "description": "Optional focus Y" },
                    "selector": { "type": "string", "description": "Optional focus selector, 1..=512 chars" },
                    "appHint": { "type": "string", "description": "Application hint for dangerous-zone matching" },
                    "highRisk": { "type": "boolean", "description": "Explicit high-risk flag (default false)" },
                }),
                "computer.type_text input",
            ),
            output_schema(
                "computer.type_text output",
                serde_json::json!({
                    "chars": { "type": "integer" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_HOTKEY,
            "computer.hotkey",
            "Press a hotkey chord. Human-review gate; dry-run validates without pressing.",
            vec![Capability::UiAction],
            RiskLevel::High,
            false,
            true,
            false,
            false,
            schema(
                &["keys"],
                serde_json::json!({
                    "keys": { "type": "string", "description": "Hotkey chord, e.g. ctrl+c (1..=64 chars)" },
                    "appHint": { "type": "string", "description": "Application hint for dangerous-zone matching" },
                    "highRisk": { "type": "boolean", "description": "Explicit high-risk flag (default false)" },
                }),
                "computer.hotkey input",
            ),
            output_schema(
                "computer.hotkey output",
                serde_json::json!({
                    "keys": { "type": "string" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_SCROLL,
            "computer.scroll",
            "Scroll at a position. Evidence-gated, reversible; dry-run validates without scrolling.",
            vec![Capability::UiAction],
            RiskLevel::Medium,
            true,
            true,
            false,
            false,
            schema(
                &[],
                serde_json::json!({
                    "x": { "type": "integer", "description": "Optional X anchor" },
                    "y": { "type": "integer", "description": "Optional Y anchor" },
                    "direction": { "type": "string", "description": "up | down | left | right (default down)" },
                    "pixels": { "type": "integer", "description": "Pixels 1..10000 (default 800)" },
                }),
                "computer.scroll input",
            ),
            output_schema(
                "computer.scroll output",
                serde_json::json!({
                    "direction": { "type": "string" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_CAPTURE_SCREEN,
            "computer.capture_screen",
            "Capture the virtual screen hash. Read-only; no pixels stored.",
            vec![Capability::UiAction],
            RiskLevel::Low,
            true,
            false,
            false,
            false,
            schema(
                &[],
                serde_json::json!({
                    "region": { "type": "object", "description": "Optional bounding box {x,y,width,height}" },
                }),
                "computer.capture_screen input",
            ),
            output_schema(
                "computer.capture_screen output",
                serde_json::json!({
                    "screenHash": { "type": "string" },
                    "viewport": { "type": "object" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            COMPUTER_LOCATE_ELEMENT,
            "computer.locate_element",
            "Locate an element by selector with confidence scoring. Read-only.",
            vec![Capability::UiAction],
            RiskLevel::Low,
            true,
            false,
            false,
            false,
            schema(
                &["selector"],
                serde_json::json!({
                    "selector": { "type": "string", "description": "Element selector, 1..=512 chars" },
                }),
                "computer.locate_element input",
            ),
            output_schema(
                "computer.locate_element output",
                serde_json::json!({
                    "selector": { "type": "string" },
                    "boundingBox": { "type": "object" },
                    "confidence": { "type": "number" },
                    "screenHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_NAVIGATE,
            "browser.navigate",
            "Navigate the isolated browser profile to an http(s) URL. Read-only planning; no personal profile reuse.",
            vec![Capability::OpenBrowser],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["url"],
                serde_json::json!({
                    "url": { "type": "string", "description": "http(s) URL, max 2048 chars" },
                    "timeoutMs": { "type": "integer", "description": "Timeout 100..60000 (default 10000)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.navigate input",
            ),
            output_schema(
                "browser.navigate output",
                serde_json::json!({
                    "url": { "type": "string" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_CLICK,
            "browser.click",
            "Click an element by selector. Evidence-gated mutation; high-risk actions need human approval; dry-run resolves without mutating.",
            vec![Capability::OpenBrowser],
            RiskLevel::Medium,
            false,
            true,
            false,
            true,
            schema(
                &["selector"],
                serde_json::json!({
                    "url": { "type": "string", "description": "Page URL context (optional)" },
                    "selector": { "type": "string", "description": "Element selector, 1..=512 chars" },
                    "timeoutMs": { "type": "integer", "description": "Timeout 100..60000 (default 10000)" },
                    "highRisk": { "type": "boolean", "description": "Form submit / purchase / settings / upload / destructive (default false)" },
                    "highRiskKind": { "type": "string", "description": "form_submit | purchase | settings_change | file_upload | file_download | destructive | other" },
                    "approved": { "type": "boolean", "description": "Explicit human approval for high-risk live runs (default false)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.click input",
            ),
            output_schema(
                "browser.click output",
                serde_json::json!({
                    "selector": { "type": "string" },
                    "resolved": { "type": "object" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_TYPE,
            "browser.type",
            "Type text into an element by selector. Evidence-gated mutation; high-risk actions need human approval.",
            vec![Capability::OpenBrowser],
            RiskLevel::Medium,
            false,
            true,
            false,
            true,
            schema(
                &["selector", "text"],
                serde_json::json!({
                    "url": { "type": "string", "description": "Page URL context (optional)" },
                    "selector": { "type": "string", "description": "Element selector, 1..=512 chars" },
                    "text": { "type": "string", "description": "Text to type, 1..=4096 chars" },
                    "submit": { "type": "boolean", "description": "Press Enter after typing (implies high-risk, default false)" },
                    "timeoutMs": { "type": "integer", "description": "Timeout 100..60000 (default 10000)" },
                    "highRisk": { "type": "boolean", "description": "Explicit high-risk flag (default false)" },
                    "highRiskKind": { "type": "string", "description": "form_submit | purchase | settings_change | file_upload | file_download | destructive | other" },
                    "approved": { "type": "boolean", "description": "Explicit human approval for high-risk live runs (default false)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.type input",
            ),
            output_schema(
                "browser.type output",
                serde_json::json!({
                    "selector": { "type": "string" },
                    "resolved": { "type": "object" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_SCROLL,
            "browser.scroll",
            "Scroll the isolated browser viewport. Reversible; no content mutation.",
            vec![Capability::OpenBrowser],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &[],
                serde_json::json!({
                    "url": { "type": "string", "description": "Page URL context (optional)" },
                    "direction": { "type": "string", "description": "up | down | top | bottom (default down)" },
                    "pixels": { "type": "integer", "description": "Pixels 1..10000 (default 800)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.scroll input",
            ),
            output_schema(
                "browser.scroll output",
                serde_json::json!({
                    "direction": { "type": "string" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_SELECT,
            "browser.select",
            "Select an option in a form control. Evidence-gated mutation.",
            vec![Capability::OpenBrowser],
            RiskLevel::Medium,
            false,
            true,
            false,
            true,
            schema(
                &["selector"],
                serde_json::json!({
                    "url": { "type": "string", "description": "Page URL context (optional)" },
                    "selector": { "type": "string", "description": "Element selector, 1..=512 chars" },
                    "value": { "type": "string", "description": "Option value or label, max 1024 chars" },
                    "timeoutMs": { "type": "integer", "description": "Timeout 100..60000 (default 10000)" },
                    "approved": { "type": "boolean", "description": "Explicit human approval for high-risk live runs (default false)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.select input",
            ),
            output_schema(
                "browser.select output",
                serde_json::json!({
                    "selector": { "type": "string" },
                    "resolved": { "type": "object" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_EXTRACT,
            "browser.extract",
            "Extract visible text from the isolated browser DOM. Read-only; bounded output.",
            vec![Capability::OpenBrowser],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["url"],
                serde_json::json!({
                    "url": { "type": "string", "description": "http(s) URL, max 2048 chars" },
                    "selector": { "type": "string", "description": "Scope selector (optional)" },
                    "maxChars": { "type": "integer", "description": "Cap 1..20000 (default 5000)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.extract input",
            ),
            output_schema(
                "browser.extract output",
                serde_json::json!({
                    "url": { "type": "string" },
                    "text": { "type": "string" },
                    "textHash": { "type": "string" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_SCREENSHOT,
            "browser.screenshot",
            "Capture a viewport screenshot hash from the isolated browser. Read-only; no pixels stored.",
            vec![Capability::OpenBrowser],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["url"],
                serde_json::json!({
                    "url": { "type": "string", "description": "http(s) URL, max 2048 chars" },
                    "fullPage": { "type": "boolean", "description": "Full-page capture (default false)" },
                    "thumbnail": { "type": "boolean", "description": "Return a bounded JPEG thumbnail data URL (default false)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.screenshot input",
            ),
            output_schema(
                "browser.screenshot output",
                serde_json::json!({
                    "url": { "type": "string" },
                    "screenshotHash": { "type": "string" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_SNAPSHOT,
            "browser.snapshot",
            "Capture an accessibility DOM snapshot + hash from the isolated browser. Read-only.",
            vec![Capability::OpenBrowser],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["url"],
                serde_json::json!({
                    "url": { "type": "string", "description": "http(s) URL, max 2048 chars" },
                    "selector": { "type": "string", "description": "Scope selector (optional)" },
                    "allowedOrigins": { "type": "array", "description": "Extra origin allowlist, max 16 http(s) origins" },
                }),
                "browser.snapshot input",
            ),
            output_schema(
                "browser.snapshot output",
                serde_json::json!({
                    "url": { "type": "string" },
                    "dom": { "type": "object" },
                    "domHash": { "type": "string" },
                    "simulated": { "type": "boolean" },
                }),
            ),
        ),
        manifest(
            BROWSER_SESSION,
            "browser.session",
            "Manage the warm browser session: launch a fresh isolated session, report status, or close it. Read-only; sessions auto-close after 5 minutes idle.",
            vec![Capability::OpenBrowser],
            RiskLevel::Low,
            true,
            false,
            false,
            true,
            schema(
                &["operation"],
                serde_json::json!({
                    "operation": { "type": "string", "description": "launch | status | close" },
                }),
                "browser.session input",
            ),
            output_schema(
                "browser.session output",
                serde_json::json!({
                    "operation": { "type": "string" },
                    "simulated": { "type": "boolean" },
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
    /// `browser.navigate { url, timeoutMs?, allowedOrigins? }`.
    BrowserNavigate {
        /// Target URL.
        url: String,
        /// Timeout.
        timeout_ms: u64,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.click { url?, selector, timeoutMs?, highRisk?, highRiskKind?, approved?, allowedOrigins? }`.
    BrowserClick {
        /// Page URL context.
        url: Option<String>,
        /// Element selector.
        selector: String,
        /// Timeout.
        timeout_ms: u64,
        /// Explicit high-risk flag.
        high_risk: bool,
        /// High-risk category.
        high_risk_kind: Option<String>,
        /// Explicit human approval for high-risk live runs.
        approved: bool,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.type { url?, selector, text, submit?, timeoutMs?, highRisk?, highRiskKind?, approved?, allowedOrigins? }`.
    BrowserType {
        /// Page URL context.
        url: Option<String>,
        /// Element selector.
        selector: String,
        /// Text to type.
        text: String,
        /// Press Enter after typing.
        submit: bool,
        /// Timeout.
        timeout_ms: u64,
        /// Explicit high-risk flag.
        high_risk: bool,
        /// High-risk category.
        high_risk_kind: Option<String>,
        /// Explicit human approval for high-risk live runs.
        approved: bool,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.scroll { url?, direction?, pixels?, allowedOrigins? }`.
    BrowserScroll {
        /// Page URL context.
        url: Option<String>,
        /// Scroll direction.
        direction: String,
        /// Pixels.
        pixels: i64,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.select { url?, selector, value?, timeoutMs?, approved?, allowedOrigins? }`.
    BrowserSelect {
        /// Page URL context.
        url: Option<String>,
        /// Element selector.
        selector: String,
        /// Option value or label.
        value: Option<String>,
        /// Timeout.
        timeout_ms: u64,
        /// Explicit human approval for high-risk live runs.
        approved: bool,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.extract { url, selector?, maxChars?, allowedOrigins? }`.
    BrowserExtract {
        /// Target URL.
        url: String,
        /// Scope selector.
        selector: Option<String>,
        /// Output cap.
        max_chars: usize,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.screenshot { url, fullPage?, thumbnail?, allowedOrigins? }`.
    BrowserScreenshot {
        /// Target URL.
        url: String,
        /// Full-page capture.
        full_page: bool,
        /// Return a bounded JPEG thumbnail data URL.
        thumbnail: bool,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.snapshot { url, selector?, allowedOrigins? }`.
    BrowserSnapshot {
        /// Target URL.
        url: String,
        /// Scope selector.
        selector: Option<String>,
        /// Extra origin allowlist merged with the environment list.
        allowed_origins: Vec<String>,
    },
    /// `browser.session { operation }` (`launch` | `status` | `close`).
    BrowserSession {
        /// Session operation.
        operation: String,
    },
    /// `computer.move_mouse { x, y, appHint?, windowHint? }`.
    ComputerMoveMouse {
        /// X in virtual-screen pixels.
        x: i64,
        /// Y in virtual-screen pixels.
        y: i64,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
    },
    /// `computer.click { x?, y?, selector?, appHint?, highRisk? }`.
    ComputerClick {
        /// X in virtual-screen pixels.
        x: Option<i64>,
        /// Y in virtual-screen pixels.
        y: Option<i64>,
        /// Element selector.
        selector: Option<String>,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
        /// Explicit high-risk flag.
        high_risk: bool,
    },
    /// `computer.double_click { x?, y?, selector?, appHint?, highRisk? }`.
    ComputerDoubleClick {
        /// X in virtual-screen pixels.
        x: Option<i64>,
        /// Y in virtual-screen pixels.
        y: Option<i64>,
        /// Element selector.
        selector: Option<String>,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
        /// Explicit high-risk flag.
        high_risk: bool,
    },
    /// `computer.drag { x, y, x2, y2, appHint?, highRisk? }`.
    ComputerDrag {
        /// Start X.
        x: i64,
        /// Start Y.
        y: i64,
        /// End X.
        x2: i64,
        /// End Y.
        y2: i64,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
        /// Explicit high-risk flag.
        high_risk: bool,
    },
    /// `computer.type_text { text, x?, y?, selector?, appHint?, highRisk? }`.
    ComputerTypeText {
        /// Text to type.
        text: String,
        /// Focus X.
        x: Option<i64>,
        /// Focus Y.
        y: Option<i64>,
        /// Focus selector.
        selector: Option<String>,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
        /// Explicit high-risk flag.
        high_risk: bool,
    },
    /// `computer.hotkey { keys, appHint?, highRisk? }`.
    ComputerHotkey {
        /// Hotkey chord.
        keys: String,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
        /// Explicit high-risk flag.
        high_risk: bool,
    },
    /// `computer.scroll { x?, y?, direction?, pixels?, appHint?, windowHint? }`.
    ComputerScroll {
        /// Anchor X.
        x: Option<i64>,
        /// Anchor Y.
        y: Option<i64>,
        /// Scroll direction.
        direction: String,
        /// Pixels.
        pixels: i64,
        /// Application hint.
        app_hint: Option<String>,
        /// Window title hint.
        window_hint: Option<String>,
    },
    /// `computer.capture_screen { region? }`.
    ComputerCaptureScreen {
        /// Optional bounding-box region as JSON.
        region: Option<serde_json::Value>,
    },
    /// `computer.locate_element { selector }`.
    ComputerLocateElement {
        /// Element selector.
        selector: String,
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

fn get_bool(params: &serde_json::Value, key: &str) -> Result<bool> {
    match params.get(key) {
        None => Ok(false),
        Some(serde_json::Value::Bool(b)) => Ok(*b),
        Some(_) => Err(err(&format!("param '{key}' must be a boolean"))),
    }
}

/// Validate an optional origin allowlist (http(s) origins, max 16 entries).
fn get_allowed_origins(params: &serde_json::Value) -> Result<Vec<String>> {
    let Some(list) = params.get("allowedOrigins") else {
        return Ok(Vec::new());
    };
    let entries = list
        .as_array()
        .ok_or_else(|| err("allowedOrigins must be an array of origins"))?;
    if entries.len() > 16 {
        return Err(err("allowedOrigins must have at most 16 entries"));
    }
    let mut origins = Vec::with_capacity(entries.len());
    for entry in entries {
        let origin = entry
            .as_str()
            .ok_or_else(|| err("allowedOrigins entries must be strings"))?;
        if origin.is_empty() || origin.len() > 2048 {
            return Err(err("allowedOrigins entries must be 1..=2048 chars"));
        }
        let lower = origin.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err(err("allowedOrigins entries must use http(s)"));
        }
        origins.push(origin.to_owned());
    }
    Ok(origins)
}

/// Validate an http(s) URL (1..=2048 chars, no credentials, no javascript:).
fn get_url(params: &serde_json::Value, key: &str, required: bool) -> Result<Option<String>> {
    let value = get_str(params, key, required)?.unwrap_or_default();
    if !required && value.is_empty() {
        return Ok(None);
    }
    if value.is_empty() || value.len() > 2048 {
        return Err(err("url must be 1..=2048 chars"));
    }
    let lower = value.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(err("url must use http(s)"));
    }
    if value.contains('@') {
        return Err(err("url must not embed credentials"));
    }
    Ok(Some(value))
}

/// Validate a selector (1..=512 chars, no NUL/control games).
fn get_selector(params: &serde_json::Value, key: &str, required: bool) -> Result<Option<String>> {
    let value = get_str(params, key, required)?.unwrap_or_default();
    if !required && value.is_empty() {
        return Ok(None);
    }
    if value.trim().is_empty() || value.len() > 512 {
        return Err(err("selector must be 1..=512 chars"));
    }
    if value.contains('\0') {
        return Err(err("selector contains NUL"));
    }
    Ok(Some(value))
}

/// Validate an optional screen coordinate (integer; viewport bounds are
/// enforced by the computer-use safety envelope, not here).
fn get_coord(params: &serde_json::Value, key: &str) -> Result<Option<i64>> {
    match params.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| err(&format!("param '{key}' must be an integer"))),
        Some(_) => Err(err(&format!("param '{key}' must be an integer"))),
    }
}

/// Validate a scroll direction for computer tools.
fn get_computer_direction(params: &serde_json::Value) -> Result<String> {
    let direction = get_str(params, "direction", false)?.unwrap_or_else(|| "down".to_owned());
    if !["up", "down", "left", "right"].contains(&direction.as_str()) {
        return Err(err("direction must be up|down|left|right"));
    }
    Ok(direction)
}

/// Validate an optional region object `{x, y, width, height}` (all integers).
fn get_region(params: &serde_json::Value) -> Result<Option<serde_json::Value>> {
    match params.get("region") {
        None => Ok(None),
        Some(value @ serde_json::Value::Object(_)) => {
            for key in ["x", "y", "width", "height"] {
                match value.get(key) {
                    Some(serde_json::Value::Number(n)) if n.as_i64().is_some() => {}
                    _ => return Err(err(&format!("region.{key} must be an integer"))),
                }
            }
            Ok(Some(value.clone()))
        }
        Some(_) => Err(err("region must be an object")),
    }
}

/// Validate an optional application/window hint (1..=256 chars when present).
fn opt_hint(params: &serde_json::Value, key: &str) -> Result<Option<String>> {
    match get_str(params, key, false)? {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => {
            if value.len() > 256 {
                return Err(err(&format!("param '{key}' must be <=256 chars")));
            }
            Ok(Some(value))
        }
    }
}

/// Validate a click target: either x+y coordinates or a selector (or both).
fn get_point_or_selector(
    params: &serde_json::Value,
) -> Result<(Option<i64>, Option<i64>, Option<String>)> {
    let x = get_coord(params, "x")?;
    let y = get_coord(params, "y")?;
    let selector = get_selector(params, "selector", false)?;
    if x.is_none() && y.is_none() && selector.is_none() {
        return Err(err("click requires x/y or selector"));
    }
    if x.is_some() != y.is_some() {
        return Err(err("x and y must be provided together"));
    }
    Ok((x, y, selector))
}

/// Validate the high-risk category when `highRisk` is set.
fn get_high_risk_kind(params: &serde_json::Value) -> Result<Option<String>> {
    match params.get("highRiskKind") {
        None => Ok(None),
        Some(serde_json::Value::String(kind)) => {
            const KINDS: &[&str] = &[
                "form_submit",
                "purchase",
                "settings_change",
                "file_upload",
                "file_download",
                "destructive",
                "other",
            ];
            if !KINDS.contains(&kind.as_str()) {
                return Err(err("unknown highRiskKind"));
            }
            if kind.len() > 64 {
                return Err(err("highRiskKind too long"));
            }
            Ok(Some(kind.clone()))
        }
        Some(_) => Err(err("highRiskKind must be a string")),
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
        BROWSER_NAVIGATE => &["url", "timeoutMs", "allowedOrigins"],
        BROWSER_CLICK => &[
            "url",
            "selector",
            "timeoutMs",
            "highRisk",
            "highRiskKind",
            "approved",
            "allowedOrigins",
        ],
        BROWSER_TYPE => &[
            "url",
            "selector",
            "text",
            "submit",
            "timeoutMs",
            "highRisk",
            "highRiskKind",
            "approved",
            "allowedOrigins",
        ],
        BROWSER_SCROLL => &["url", "direction", "pixels", "allowedOrigins"],
        BROWSER_SELECT => &[
            "url",
            "selector",
            "value",
            "timeoutMs",
            "approved",
            "allowedOrigins",
        ],
        BROWSER_EXTRACT => &["url", "selector", "maxChars", "allowedOrigins"],
        BROWSER_SCREENSHOT => &["url", "fullPage", "thumbnail", "allowedOrigins"],
        BROWSER_SNAPSHOT => &["url", "selector", "allowedOrigins"],
        BROWSER_SESSION => &["operation"],
        COMPUTER_MOVE_MOUSE => &["x", "y", "appHint", "windowHint"],
        COMPUTER_CLICK => &[
            "x",
            "y",
            "selector",
            "appHint",
            "windowHint",
            "highRisk",
            "highRiskKind",
        ],
        COMPUTER_DOUBLE_CLICK => &[
            "x",
            "y",
            "selector",
            "appHint",
            "windowHint",
            "highRisk",
            "highRiskKind",
        ],
        COMPUTER_DRAG => &["x", "y", "x2", "y2", "appHint", "windowHint", "highRisk"],
        COMPUTER_TYPE_TEXT => &[
            "text",
            "x",
            "y",
            "selector",
            "appHint",
            "windowHint",
            "highRisk",
        ],
        COMPUTER_HOTKEY => &["keys", "appHint", "windowHint", "highRisk"],
        COMPUTER_SCROLL => &["x", "y", "direction", "pixels", "appHint", "windowHint"],
        COMPUTER_CAPTURE_SCREEN => &["region"],
        COMPUTER_LOCATE_ELEMENT => &["selector"],
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
        BROWSER_NAVIGATE => {
            let url = get_url(params, "url", true)?.unwrap_or_default();
            let timeout_ms = get_int(params, "timeoutMs")?.unwrap_or(10_000);
            if !(100..=60_000).contains(&timeout_ms) {
                return Err(err("timeoutMs must be 100..60000"));
            }
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserNavigate {
                url,
                timeout_ms: timeout_ms as u64,
                allowed_origins,
            })
        }
        BROWSER_CLICK => {
            let url = get_url(params, "url", false)?;
            let selector = get_selector(params, "selector", true)?.unwrap_or_default();
            let timeout_ms = get_int(params, "timeoutMs")?.unwrap_or(10_000);
            if !(100..=60_000).contains(&timeout_ms) {
                return Err(err("timeoutMs must be 100..60000"));
            }
            let high_risk = get_bool(params, "highRisk")?;
            let high_risk_kind = get_high_risk_kind(params)?;
            if high_risk_kind.is_some() && !high_risk {
                return Err(err("highRiskKind requires highRisk: true"));
            }
            let approved = get_bool(params, "approved")?;
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserClick {
                url,
                selector,
                timeout_ms: timeout_ms as u64,
                high_risk,
                high_risk_kind,
                approved,
                allowed_origins,
            })
        }
        BROWSER_TYPE => {
            let url = get_url(params, "url", false)?;
            let selector = get_selector(params, "selector", true)?.unwrap_or_default();
            let text = get_str(params, "text", true)?.unwrap_or_default();
            if text.is_empty() || text.len() > 4096 {
                return Err(err("text must be 1..=4096 chars"));
            }
            let submit = get_bool(params, "submit")?;
            let timeout_ms = get_int(params, "timeoutMs")?.unwrap_or(10_000);
            if !(100..=60_000).contains(&timeout_ms) {
                return Err(err("timeoutMs must be 100..60000"));
            }
            // Submitting a form is consequential: treat as high-risk.
            let mut high_risk = get_bool(params, "highRisk")?;
            if submit {
                high_risk = true;
            }
            let mut high_risk_kind = get_high_risk_kind(params)?;
            if submit && high_risk_kind.is_none() {
                high_risk_kind = Some("form_submit".to_owned());
            }
            if high_risk_kind.is_some() && !high_risk {
                return Err(err("highRiskKind requires highRisk: true"));
            }
            let approved = get_bool(params, "approved")?;
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserType {
                url,
                selector,
                text,
                submit,
                timeout_ms: timeout_ms as u64,
                high_risk,
                high_risk_kind,
                approved,
                allowed_origins,
            })
        }
        BROWSER_SCROLL => {
            let url = get_url(params, "url", false)?;
            let direction =
                get_str(params, "direction", false)?.unwrap_or_else(|| "down".to_owned());
            if !["up", "down", "top", "bottom"].contains(&direction.as_str()) {
                return Err(err("direction must be up|down|top|bottom"));
            }
            let pixels = get_int(params, "pixels")?.unwrap_or(800);
            if !(1..=10_000).contains(&pixels) {
                return Err(err("pixels must be 1..10000"));
            }
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserScroll {
                url,
                direction,
                pixels,
                allowed_origins,
            })
        }
        BROWSER_SELECT => {
            let url = get_url(params, "url", false)?;
            let selector = get_selector(params, "selector", true)?.unwrap_or_default();
            let value = get_str(params, "value", false)?.filter(|value| !value.is_empty());
            if let Some(value) = &value {
                if value.len() > 1024 {
                    return Err(err("value must be <=1024 chars"));
                }
            }
            let timeout_ms = get_int(params, "timeoutMs")?.unwrap_or(10_000);
            if !(100..=60_000).contains(&timeout_ms) {
                return Err(err("timeoutMs must be 100..60000"));
            }
            let approved = get_bool(params, "approved")?;
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserSelect {
                url,
                selector,
                value,
                timeout_ms: timeout_ms as u64,
                approved,
                allowed_origins,
            })
        }
        BROWSER_EXTRACT => {
            let url = get_url(params, "url", true)?.unwrap_or_default();
            let selector = get_selector(params, "selector", false)?;
            let max_chars = get_int(params, "maxChars")?.unwrap_or(5_000);
            if !(1..=20_000).contains(&max_chars) {
                return Err(err("maxChars must be 1..20000"));
            }
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserExtract {
                url,
                selector,
                max_chars: max_chars as usize,
                allowed_origins,
            })
        }
        BROWSER_SCREENSHOT => {
            let url = get_url(params, "url", true)?.unwrap_or_default();
            let full_page = get_bool(params, "fullPage")?;
            let thumbnail = get_bool(params, "thumbnail")?;
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserScreenshot {
                url,
                full_page,
                thumbnail,
                allowed_origins,
            })
        }
        BROWSER_SNAPSHOT => {
            let url = get_url(params, "url", true)?.unwrap_or_default();
            let selector = get_selector(params, "selector", false)?;
            let allowed_origins = get_allowed_origins(params)?;
            Ok(ValidatedParams::BrowserSnapshot {
                url,
                selector,
                allowed_origins,
            })
        }
        BROWSER_SESSION => {
            let operation = get_str(params, "operation", true)?.unwrap_or_default();
            if !["launch", "status", "close"].contains(&operation.as_str()) {
                return Err(err("operation must be launch|status|close"));
            }
            Ok(ValidatedParams::BrowserSession { operation })
        }
        COMPUTER_MOVE_MOUSE => {
            let x = get_coord(params, "x")?.ok_or_else(|| err("missing required param 'x'"))?;
            let y = get_coord(params, "y")?.ok_or_else(|| err("missing required param 'y'"))?;
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            Ok(ValidatedParams::ComputerMoveMouse {
                x,
                y,
                app_hint,
                window_hint,
            })
        }
        COMPUTER_CLICK => {
            let (x, y, selector) = get_point_or_selector(params)?;
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            let high_risk = get_bool(params, "highRisk")?;
            Ok(ValidatedParams::ComputerClick {
                x,
                y,
                selector,
                app_hint,
                window_hint,
                high_risk,
            })
        }
        COMPUTER_DOUBLE_CLICK => {
            let (x, y, selector) = get_point_or_selector(params)?;
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            let high_risk = get_bool(params, "highRisk")?;
            Ok(ValidatedParams::ComputerDoubleClick {
                x,
                y,
                selector,
                app_hint,
                window_hint,
                high_risk,
            })
        }
        COMPUTER_DRAG => {
            let x = get_coord(params, "x")?.ok_or_else(|| err("missing required param 'x'"))?;
            let y = get_coord(params, "y")?.ok_or_else(|| err("missing required param 'y'"))?;
            let x2 = get_coord(params, "x2")?.ok_or_else(|| err("missing required param 'x2'"))?;
            let y2 = get_coord(params, "y2")?.ok_or_else(|| err("missing required param 'y2'"))?;
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            let high_risk = get_bool(params, "highRisk")?;
            Ok(ValidatedParams::ComputerDrag {
                x,
                y,
                x2,
                y2,
                app_hint,
                window_hint,
                high_risk,
            })
        }
        COMPUTER_TYPE_TEXT => {
            let text = get_str(params, "text", true)?.unwrap_or_default();
            if text.is_empty() || text.len() > 4096 {
                return Err(err("text must be 1..=4096 chars"));
            }
            let x = get_coord(params, "x")?;
            let y = get_coord(params, "y")?;
            let selector = get_selector(params, "selector", false)?;
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            let high_risk = get_bool(params, "highRisk")?;
            Ok(ValidatedParams::ComputerTypeText {
                text,
                x,
                y,
                selector,
                app_hint,
                window_hint,
                high_risk,
            })
        }
        COMPUTER_HOTKEY => {
            let keys = get_str(params, "keys", true)?.unwrap_or_default();
            if keys.trim().is_empty() || keys.len() > 64 {
                return Err(err("keys must be 1..=64 chars"));
            }
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            let high_risk = get_bool(params, "highRisk")?;
            Ok(ValidatedParams::ComputerHotkey {
                keys,
                app_hint,
                window_hint,
                high_risk,
            })
        }
        COMPUTER_SCROLL => {
            let x = get_coord(params, "x")?;
            let y = get_coord(params, "y")?;
            let direction = get_computer_direction(params)?;
            let pixels = get_int(params, "pixels")?.unwrap_or(800);
            if !(1..=10_000).contains(&pixels) {
                return Err(err("pixels must be 1..10000"));
            }
            let app_hint = opt_hint(params, "appHint")?;
            let window_hint = opt_hint(params, "windowHint")?;
            Ok(ValidatedParams::ComputerScroll {
                x,
                y,
                direction,
                pixels,
                app_hint,
                window_hint,
            })
        }
        COMPUTER_CAPTURE_SCREEN => {
            let region = get_region(params)?;
            Ok(ValidatedParams::ComputerCaptureScreen { region })
        }
        COMPUTER_LOCATE_ELEMENT => {
            let selector = get_selector(params, "selector", true)?.unwrap_or_default();
            Ok(ValidatedParams::ComputerLocateElement { selector })
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
    /// Build a registry with the baseline tools registered.
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
    fn registry_lists_all_builtins() {
        let registry = ToolRegistry::with_builtins();
        let listed = registry.list();
        let ids: Vec<&str> = listed.iter().map(|manifest| manifest.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                BROWSER_CLICK,
                BROWSER_EXTRACT,
                BROWSER_NAVIGATE,
                BROWSER_SCREENSHOT,
                BROWSER_SCROLL,
                BROWSER_SELECT,
                BROWSER_SESSION,
                BROWSER_SNAPSHOT,
                BROWSER_TYPE,
                COMPUTER_CAPTURE_SCREEN,
                COMPUTER_CLICK,
                COMPUTER_DOUBLE_CLICK,
                COMPUTER_DRAG,
                COMPUTER_HOTKEY,
                COMPUTER_LOCATE_ELEMENT,
                COMPUTER_MOVE_MOUSE,
                COMPUTER_SCROLL,
                COMPUTER_TYPE_TEXT,
                FS_LIST,
                FS_READ,
                FS_SEARCH,
                FS_WRITE,
                PROCESS_EXEC
            ]
        );
    }

    #[test]
    fn computer_tools_disabled_by_default() {
        let registry = ToolRegistry::with_builtins();
        for id in COMPUTER_TOOLS {
            assert!(!registry.is_enabled(id), "{id} must be disabled by default");
        }
    }

    #[test]
    fn exec_disabled_by_default() {
        let registry = ToolRegistry::with_builtins();
        assert!(registry.is_enabled(FS_READ));
        assert!(!registry.is_enabled(PROCESS_EXEC));
    }

    #[test]
    fn browser_session_and_live_keys_validate() {
        let registry = ToolRegistry::with_builtins();
        assert!(registry.get(crate::model::BROWSER_SESSION).is_some());
        assert!(registry.is_enabled(crate::model::BROWSER_SESSION));
        assert!(registry
            .validate(
                crate::model::BROWSER_SESSION,
                &serde_json::json!({ "operation": "status" }),
            )
            .is_ok());
        assert!(registry
            .validate(
                crate::model::BROWSER_SESSION,
                &serde_json::json!({ "operation": "reboot" }),
            )
            .is_err());
        assert!(registry
            .validate(
                crate::model::BROWSER_CLICK,
                &serde_json::json!({
                    "selector": "button",
                    "approved": true,
                    "allowedOrigins": ["https://example.com"],
                }),
            )
            .is_ok());
        assert!(registry
            .validate(
                crate::model::BROWSER_SCREENSHOT,
                &serde_json::json!({ "url": "https://example.com", "thumbnail": true }),
            )
            .is_ok());
        assert!(registry
            .validate(
                crate::model::BROWSER_CLICK,
                &serde_json::json!({ "selector": "button", "allowedOrigins": ["ftp://x"] }),
            )
            .is_err());
        assert!(registry
            .validate(
                crate::model::BROWSER_CLICK,
                &serde_json::json!({ "selector": "button", "approved": "yes" }),
            )
            .is_err());
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
        // Browser URLs must be http(s) without credentials.
        assert!(registry
            .validate(
                BROWSER_NAVIGATE,
                &serde_json::json!({ "url": "javascript:alert(1)" })
            )
            .is_err());
        assert!(registry
            .validate(
                BROWSER_CLICK,
                &serde_json::json!({ "selector": "button", "highRiskKind": "purchase" })
            )
            .is_err());
        assert!(registry
            .validate(
                BROWSER_CLICK,
                &serde_json::json!({
                    "selector": "button",
                    "highRisk": true,
                    "highRiskKind": "purchase"
                })
            )
            .is_ok());
        // Computer clicks need coordinates or a selector, never neither.
        assert!(registry
            .validate(COMPUTER_CLICK, &serde_json::json!({}))
            .is_err());
        assert!(registry
            .validate(COMPUTER_CLICK, &serde_json::json!({ "x": 10 }))
            .is_err());
        assert!(registry
            .validate(COMPUTER_CLICK, &serde_json::json!({ "x": 10, "y": 20 }))
            .is_ok());
        assert!(registry
            .validate(COMPUTER_DRAG, &serde_json::json!({ "x": 1, "y": 2 }))
            .is_err());
        assert!(registry
            .validate(COMPUTER_TYPE_TEXT, &serde_json::json!({ "text": "" }))
            .is_err());
    }
}
