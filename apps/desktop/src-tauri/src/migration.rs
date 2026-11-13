//! Migration kit IPC (Prompt 011): quarantined import of competitor
//! instruction / skill / heartbeat / MCP artifacts.
//!
//! Fence (never crossed): imported content can NEVER grant permissions,
//! auto-enable tools/servers/jobs, bypass `policy_broker`, or execute during
//! import. Every step writes an evidence receipt; enabling any imported
//! artifact is a SEPARATE approval. Secrets are stripped, never imported.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use instruction_parser::migration::{
    apply_preview, build_preview, scan_workspace, MigrationCategory,
};

use crate::commands::IpcError;
use crate::events::{emit_talos_event, MIGRATION_IMPORT_COMPLETED, MIGRATION_SCAN_COMPLETED};
use crate::state::AppState;

/// In-memory scan cache: import_id → scan (preview/apply re-read it).
static SCANS: Mutex<Option<HashMap<String, instruction_parser::migration::MigrationScan>>> =
    Mutex::new(None);
/// Idempotency set: `import_id:action:target` already imported.
static APPLIED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Preview view (mirrors TS `MigrationPreview`, camelCase).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPreviewView {
    /// Import id.
    pub import_id: String,
    /// Dry-run items.
    pub items: Vec<instruction_parser::migration::MigrationPreviewItem>,
    /// Preview time.
    pub previewed_at: String,
}

/// Apply view (mirrors TS `MigrationApplyResult`, camelCase).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationApplyView {
    /// Import id.
    pub import_id: String,
    /// Human-approved categories.
    pub approved_categories: Vec<String>,
    /// Newly quarantined/disabled artifacts.
    pub created: Vec<instruction_parser::migration::MigrationPreviewItem>,
    /// Items outside the approval (never created).
    pub skipped: Vec<instruction_parser::migration::MigrationPreviewItem>,
    /// Evidence receipt ids for the import.
    pub evidence_ids: Vec<String>,
}

fn parse_category(raw: &str) -> Result<MigrationCategory, IpcError> {
    match raw {
        "instructions" => Ok(MigrationCategory::Instructions),
        "skills" => Ok(MigrationCategory::Skills),
        "schedules" => Ok(MigrationCategory::Schedules),
        "mcp-servers" => Ok(MigrationCategory::McpServers),
        _ => Err(IpcError {
            code: "invalid_category".to_owned(),
            message: "category must be instructions|skills|schedules|mcp-servers".to_owned(),
        }),
    }
}

async fn persist(
    state: &AppState,
    id: String,
    kind: &str,
    summary: String,
    payload: serde_json::Value,
) -> Result<String, IpcError> {
    evidence_store::insert_evidence(
        &state.db,
        evidence_store::NewEvidence {
            id: id.clone(),
            task_id: None,
            agent_id: None,
            session_id: Some("session-local".to_owned()),
            kind: kind.to_owned(),
            label: Some(kind.to_owned()),
            summary: Some(summary),
            uri: None,
            mime_type: None,
            payload,
        },
    )
    .await
    .map_err(IpcError::from)?;
    evidence_store::transition_evidence_status(&state.db, &id, "verified")
        .await
        .map_err(IpcError::from)?;
    Ok(id)
}

fn lookup(import_id: &str) -> Result<instruction_parser::migration::MigrationScan, IpcError> {
    SCANS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(import_id).cloned())
        .ok_or_else(|| IpcError {
            code: "unknown_import".to_owned(),
            message: "unknown import_id; run migration_scan first".to_owned(),
        })
}

/// `migration_scan` — read-only scan of a workspace root (never executes).
#[tauri::command]
pub async fn migration_scan(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_path: String,
) -> Result<instruction_parser::migration::MigrationScan, IpcError> {
    let scan = scan_workspace(std::path::Path::new(&workspace_path)).map_err(|err| IpcError {
        code: "scan_failed".to_owned(),
        message: err.to_string(),
    })?;
    SCANS.lock().unwrap().get_or_insert_with(HashMap::new).insert(scan.import_id.clone(), scan.clone());
    let evidence_id = persist(
        &state,
        format!("{}-scan", scan.import_id),
        "migration.scan",
        format!("{} findings in {}", scan.findings.len(), scan.workspace_path),
        serde_json::to_value(&scan).unwrap_or(serde_json::Value::Null),
    )
    .await?;
    let _ = emit_talos_event(
        &app,
        MIGRATION_SCAN_COMPLETED,
        serde_json::json!({
            "importId": scan.import_id,
            "findingCount": scan.findings.len(),
            "evidenceId": evidence_id,
            "checkedAt": talos_common::now_iso(),
        }),
    );
    Ok(scan)
}

/// `migration_preview` — dry-run diff; nothing is written.
#[tauri::command]
pub async fn migration_preview(
    state: tauri::State<'_, AppState>,
    import_id: String,
) -> Result<MigrationPreviewView, IpcError> {
    let scan = lookup(&import_id)?;
    let view = MigrationPreviewView {
        import_id: scan.import_id.clone(),
        items: build_preview(&scan),
        previewed_at: talos_common::now_iso(),
    };
    persist(
        &state,
        format!("{import_id}-preview"),
        "migration.preview",
        format!("{} preview items", view.items.len()),
        serde_json::to_value(&view).unwrap_or(serde_json::Value::Null),
    )
    .await?;
    Ok(view)
}

/// `migration_apply` — per-category approval; quarantined/disabled, idempotent.
#[tauri::command]
pub async fn migration_apply(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    import_id: String,
    approved_categories: Vec<String>,
) -> Result<MigrationApplyView, IpcError> {
    if approved_categories.is_empty() || approved_categories.len() > 4 {
        return Err(IpcError {
            code: "invalid_approval".to_owned(),
            message: "approvedCategories must list 1..=4 categories".to_owned(),
        });
    }
    let mut approved = Vec::new();
    for raw in &approved_categories {
        approved.push(parse_category(raw)?);
    }
    let scan = lookup(&import_id)?;
    let (wanted, skipped) = apply_preview(&build_preview(&scan), &approved);
    let mut created = Vec::new();
    {
        let mut done = APPLIED.lock().unwrap();
        let store = done.get_or_insert_with(HashSet::new);
        for item in wanted {
            if store.insert(format!("{import_id}:{}:{}", item.action, item.target)) {
                created.push(item);
            }
        }
    }
    let created_count = created.len();
    let view = MigrationApplyView {
        import_id: import_id.clone(),
        approved_categories,
        created,
        skipped,
        evidence_ids: Vec::new(),
    };
    let evidence_id = persist(
        &state,
        format!("{import_id}-import"),
        "migration.import",
        format!("{created_count} artifacts quarantined/disabled (idempotent)"),
        serde_json::to_value(&view).unwrap_or(serde_json::Value::Null),
    )
    .await?;
    let _ = emit_talos_event(
        &app,
        MIGRATION_IMPORT_COMPLETED,
        serde_json::json!({
            "importId": import_id,
            "createdCount": created_count,
            "evidenceId": evidence_id,
            "checkedAt": talos_common::now_iso(),
        }),
    );
    Ok(MigrationApplyView { evidence_ids: vec![evidence_id], ..view })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_category() {
        assert!(parse_category("skills").is_ok());
        assert!(parse_category("root-access").is_err());
    }
}
