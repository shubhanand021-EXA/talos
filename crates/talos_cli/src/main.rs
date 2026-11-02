//! `talos` binary: argument parsing + exit-code mapping.
//!
//! All logic lives in the `talos_cli` library (testable without spawning);
//! this shell only parses args, opens the shared database, renders output,
//! and maps outcomes to the stable exit-code contract
//! (`0` success / `1` failure / `2` policy denied).

mod cli;

use clap::Parser;
use talos_cli::{
    audit_run, config_get, config_set, doctor_report, emit_jsonl, evidence_verify, mcp_add,
    mcp_list, mcp_remove, open_shared_db, print_text, provider_add, provider_list, provider_remove,
    run_task, skill_list, skill_publish, skill_validate, task_cancel, task_get, task_list,
    OutputFormat, EXIT_FAILURE, EXIT_POLICY_DENIED, EXIT_SUCCESS,
};

use cli::{Command, FormatArg};

fn format_of(arg: FormatArg) -> OutputFormat {
    match arg {
        FormatArg::Text => OutputFormat::Text,
        FormatArg::Json => OutputFormat::Json,
    }
}

fn emit(format: OutputFormat, event_type: &str, value: serde_json::Value) {
    match format {
        OutputFormat::Json => emit_jsonl(event_type, value),
        OutputFormat::Text => print_text(&format!(
            "{event_type}: {}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        )),
    }
}

fn emit_error(format: OutputFormat, message: &str) {
    match format {
        OutputFormat::Json => emit_jsonl("error", serde_json::json!({"message": message})),
        OutputFormat::Text => eprintln!("talos: error: {message}"),
    }
}

#[tokio::main]
async fn main() {
    let args = cli::Cli::parse();
    let format = format_of(args.format);
    let code = run(args.command, format).await;
    std::process::exit(code);
}

async fn run(command: Command, format: OutputFormat) -> i32 {
    // `version` and parse-time failures never need the database.
    if let Command::Version = command {
        let value = serde_json::json!({
            "cli": env!("CARGO_PKG_VERSION"),
            "policy_broker": "shared-crate",
            "evidence_store": "shared-crate",
        });
        emit(format, "version", value);
        return EXIT_SUCCESS;
    }
    let cli = match open_shared_db().await {
        Ok(cli) => cli,
        Err(err) => {
            emit_error(format, &err.to_string());
            return EXIT_FAILURE;
        }
    };
    match command {
        Command::Version => EXIT_SUCCESS,
        Command::Run {
            goal,
            model,
            budget,
        } => {
            let (outcome, code) = run_task(&cli, &goal, model.as_deref(), budget).await;
            for (kind, payload) in outcome.events {
                emit(format, &kind, payload);
            }
            code
        }
        Command::Task { op } => match op {
            cli::TaskOp::List { status } => match task_list(&cli.db, status.as_deref()).await {
                Ok(tasks) => {
                    emit(
                        format,
                        "tasks",
                        serde_json::to_value(&tasks).unwrap_or_default(),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::TaskOp::Get { task_id } => match task_get(&cli.db, &task_id).await {
                Ok(task) => {
                    emit(
                        format,
                        "task",
                        serde_json::to_value(&task).unwrap_or_default(),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::TaskOp::Cancel { task_id } => match task_cancel(&cli.db, &task_id).await {
                Ok(task) => {
                    emit(
                        format,
                        "task_canceled",
                        serde_json::to_value(&task).unwrap_or_default(),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
        },
        Command::Evidence { op } => match op {
            cli::EvidenceOp::Verify => match evidence_verify(&cli.db).await {
                Ok(report) => {
                    emit(
                        format,
                        "evidence_verified",
                        serde_json::to_value(&report).unwrap_or_default(),
                    );
                    if report.event_chain_valid && report.evidence_chain_valid {
                        EXIT_SUCCESS
                    } else {
                        EXIT_FAILURE
                    }
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
        },
        Command::Audit { op } => match op {
            cli::AuditOp::Run => match audit_run(&cli.db).await {
                Ok(report) => {
                    emit(
                        format,
                        "audit",
                        serde_json::to_value(&report).unwrap_or_default(),
                    );
                    if report.selftest_ok {
                        EXIT_SUCCESS
                    } else {
                        EXIT_FAILURE
                    }
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
        },
        Command::Config { op } => match op {
            cli::ConfigOp::Get { key } => match config_get(&cli.db, &key).await {
                Ok(value) => {
                    emit(
                        format,
                        "config",
                        serde_json::json!({"key": key, "value": value}),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::ConfigOp::Set { key, value } => match config_set(&cli.db, &key, &value).await {
                Ok(()) => {
                    emit(
                        format,
                        "config",
                        serde_json::json!({"key": key, "value": value}),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
        },
        Command::Provider { op } => match op {
            cli::ProviderOp::List => match provider_list(&cli.db).await {
                Ok(providers) => {
                    emit(format, "providers", serde_json::Value::Array(providers));
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::ProviderOp::Add { name, set } => match provider_add(&cli.db, &name, &set).await {
                Ok(entry) => {
                    emit(
                        format,
                        "provider_added",
                        serde_json::json!({"name": name, "entry": entry}),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::ProviderOp::Remove { name } => match provider_remove(&cli.db, &name).await {
                Ok(removed) => {
                    emit(
                        format,
                        "provider_removed",
                        serde_json::json!({"name": name, "removed": removed}),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
        },
        Command::Mcp { op } => match op {
            cli::McpOp::List => match mcp_list(&cli.db).await {
                Ok(servers) => {
                    emit(format, "mcp_servers", serde_json::Value::Array(servers));
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::McpOp::Add { name, set } => match mcp_add(&cli.db, &name, &set).await {
                Ok(entry) => {
                    emit(
                        format,
                        "mcp_added",
                        serde_json::json!({"name": name, "entry": entry}),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::McpOp::Remove { name } => match mcp_remove(&cli.db, &name).await {
                Ok(removed) => {
                    emit(
                        format,
                        "mcp_removed",
                        serde_json::json!({"name": name, "removed": removed}),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
        },
        Command::Skill { op } => match op {
            cli::SkillOp::List { status } => match skill_list(&cli.db, status.as_deref()).await {
                Ok(skills) => {
                    emit(
                        format,
                        "skills",
                        serde_json::to_value(&skills).unwrap_or_default(),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    EXIT_FAILURE
                }
            },
            cli::SkillOp::Validate { skill_id } => match skill_validate(&cli.db, &skill_id).await {
                Ok(skill) => {
                    emit(
                        format,
                        "skill_validated",
                        serde_json::to_value(&skill).unwrap_or_default(),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    // A failed promotion is a policy/lifecycle denial, not a crash.
                    if err.to_string().contains("transition") || err.to_string().contains("status")
                    {
                        EXIT_POLICY_DENIED
                    } else {
                        EXIT_FAILURE
                    }
                }
            },
            cli::SkillOp::Publish { skill_id } => match skill_publish(&cli.db, &skill_id).await {
                Ok(skill) => {
                    emit(
                        format,
                        "skill_published",
                        serde_json::to_value(&skill).unwrap_or_default(),
                    );
                    EXIT_SUCCESS
                }
                Err(err) => {
                    emit_error(format, &err.to_string());
                    if err.to_string().contains("transition") || err.to_string().contains("status")
                    {
                        EXIT_POLICY_DENIED
                    } else {
                        EXIT_FAILURE
                    }
                }
            },
        },
        Command::Doctor => match doctor_report(&cli).await {
            Ok(report) => {
                emit(
                    format,
                    "doctor",
                    serde_json::to_value(&report).unwrap_or_default(),
                );
                if report.chain_valid && report.database_ok {
                    EXIT_SUCCESS
                } else {
                    EXIT_FAILURE
                }
            }
            Err(err) => {
                emit_error(format, &err.to_string());
                EXIT_FAILURE
            }
        },
    }
}
