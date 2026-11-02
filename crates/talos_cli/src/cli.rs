//! CLI argument model (`talos <command> ...`).
//!
//! Kept in the binary: parsing is a thin shell over the library functions in
//! `crate` root so integration tests exercise logic without spawning.

use clap::{Parser, Subcommand, ValueEnum};

/// Stable headless contract for installable, scriptable Talos.
#[derive(Debug, Parser)]
#[command(
    name = "talos",
    version,
    about = "Talos headless CLI — policy-gated task execution"
)]
pub struct Cli {
    /// Output shape: human text or JSONL for CI/CD parsing.
    #[arg(long, global = true, value_enum, default_value = "text")]
    pub format: FormatArg,
    /// Command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Output formats accepted on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    /// Human-readable lines.
    Text,
    /// Structured JSON lines (one object per line).
    Json,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Execute a task headlessly, streaming progress to stdout.
    Run {
        /// Goal to execute.
        goal: String,
        /// Model hint (recorded as evidence; routing stays policy-gated).
        #[arg(long)]
        model: Option<String>,
        /// Per-task spend cap in USD.
        #[arg(long)]
        budget: Option<f64>,
    },
    /// Durable task queue inspection and control.
    Task {
        #[command(subcommand)]
        op: TaskOp,
    },
    /// Evidence ledger operations.
    Evidence {
        #[command(subcommand)]
        op: EvidenceOp,
    },
    /// Safety + integrity audit (policy selftest + chain verification).
    Audit {
        #[command(subcommand)]
        op: AuditOp,
    },
    /// CLI-managed generic config keys (backed by shared `app_state`).
    Config {
        #[command(subcommand)]
        op: ConfigOp,
    },
    /// LLM provider registry (CLI mirror of Settings → Model providers).
    Provider {
        #[command(subcommand)]
        op: ProviderOp,
    },
    /// MCP server registry (CLI mirror of the MCP page).
    Mcp {
        #[command(subcommand)]
        op: McpOp,
    },
    /// Skill registry (CLI mirror of the Memory page).
    Skill {
        #[command(subcommand)]
        op: SkillOp,
    },
    /// Print the CLI + shared-crate version.
    Version,
    /// Check the local environment (toolchain, database, storage).
    Doctor,
}

/// `talos task ...` operations.
#[derive(Debug, Subcommand)]
pub enum TaskOp {
    /// List durable tasks, newest first.
    List {
        /// Filter by lifecycle status.
        #[arg(long)]
        status: Option<String>,
    },
    /// Fetch one task by id.
    Get {
        /// Task identifier.
        task_id: String,
    },
    /// Request cancellation for one task.
    Cancel {
        /// Task identifier.
        task_id: String,
    },
}

/// `talos evidence ...` operations.
#[derive(Debug, Subcommand)]
pub enum EvidenceOp {
    /// Verify the hash-chained event + evidence ledgers.
    Verify,
}

/// `talos audit ...` operations.
#[derive(Debug, Subcommand)]
pub enum AuditOp {
    /// Run the policy selftest + ledger verification.
    Run,
}

/// `talos config ...` operations.
#[derive(Debug, Subcommand)]
pub enum ConfigOp {
    /// Read one config key.
    Get {
        /// Config key.
        key: String,
    },
    /// Write one config key.
    Set {
        /// Config key.
        key: String,
        /// Config value (stored as a JSON string).
        value: String,
    },
}

/// `talos provider ...` operations.
#[derive(Debug, Subcommand)]
pub enum ProviderOp {
    /// List known providers and whether a key is configured.
    List,
    /// Register a provider entry (model hint + endpoint).
    Add {
        /// Provider id (`openai` | `anthropic` | `ollama` | custom).
        name: String,
        /// Optional `key=value` pairs (`model=...`, `base_url=...`).
        #[arg(long)]
        set: Vec<String>,
    },
    /// Remove a provider entry.
    Remove {
        /// Provider id.
        name: String,
    },
}

/// `talos mcp ...` operations.
#[derive(Debug, Subcommand)]
pub enum McpOp {
    /// List registered MCP servers.
    List,
    /// Register an MCP server (`transport=stdio|sse command=... url=...`).
    Add {
        /// Server name.
        name: String,
        /// Optional `key=value` pairs.
        #[arg(long)]
        set: Vec<String>,
    },
    /// Remove an MCP server.
    Remove {
        /// Server name.
        name: String,
    },
}

/// `talos skill ...` operations.
#[derive(Debug, Subcommand)]
pub enum SkillOp {
    /// List skills, optionally filtered by status.
    List {
        /// Status filter.
        #[arg(long)]
        status: Option<String>,
    },
    /// Validate a quarantined skill (`quarantined` → `validated`).
    Validate {
        /// Skill id.
        skill_id: String,
    },
    /// Publish a validated skill (`validated` → `published`).
    Publish {
        /// Skill id.
        skill_id: String,
    },
}
