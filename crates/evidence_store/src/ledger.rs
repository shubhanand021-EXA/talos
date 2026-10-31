//! Ledger integrity: hash-chained event log, claim lifecycle, policy audit
//! trail, and compensation plans.
//!
//! Design notes:
//! - `event_log` and the evidence `seq`/`prev_hash` columns form two
//!   independent SHA-256 chains (genesis `GENESIS`). Inserts run inside write
//!   transactions so concurrent appends cannot fork a chain.
//! - Status columns (`claims.status`, `evidence_records.status`,
//!   `compensation_*status`) change only through the transition functions
//!   below — never ad-hoc edits. That is the explicitly modeled exception to
//!   the append-only rule.
//! - Foreign keys are declared in the schema and enforced pool-wide
//!   (`EvidenceDb::connect`); repository functions additionally check parents
//!   so violations surface as `InvalidId` instead of raw SQLite errors.

use sha2::{Digest, Sha256};
use talos_common::{now_iso, Result, TalosError};

use crate::db::EvidenceDb;
use crate::models::{
    ClaimRow, CompensationPlanRow, CompensationStepRow, EventLogRow, NewActionRecord,
    NewClaimPending, NewCompensationPlan, NewEvent,
};
use crate::repo::{hash_payload, list_recent_evidence};

/// Chain hash over all link fields: `seq|id|kind|payload|prev|created_at`.
#[must_use]
pub fn chain_hash(
    seq: i64,
    id: &str,
    kind: &str,
    payload_json: &str,
    prev_hash: &str,
    created_at: &str,
) -> String {
    let raw = format!("{seq}|{id}|{kind}|{payload_json}|{prev_hash}|{created_at}");
    format!("sha256:{}", hex::encode(Sha256::digest(raw.as_bytes())))
}

/// Append one entry to the hash-chained `event_log`.
pub async fn append_event(db: &EvidenceDb, event: NewEvent) -> Result<EventLogRow> {
    if event.id.is_empty() || event.id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "event_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if event.kind.is_empty() {
        return Err(TalosError::InvalidId {
            what: "event_kind",
            reason: "must not be empty".to_owned(),
        });
    }
    let created_at = now_iso();
    let payload_json = serde_json::to_string(&event.payload_json)?;
    let mut tx = db
        .pool()
        .begin()
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    let prev: Option<(i64, String)> =
        sqlx::query_as("SELECT seq, hash FROM event_log ORDER BY seq DESC LIMIT 1")
            .fetch_optional(&mut *tx)
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    let (seq, prev_hash) = match prev {
        Some((last_seq, last_hash)) => (last_seq + 1, last_hash),
        None => (1, crate::GENESIS_HASH.to_owned()),
    };
    let hash = chain_hash(
        seq,
        &event.id,
        &event.kind,
        &payload_json,
        &prev_hash,
        &created_at,
    );
    sqlx::query(
        "INSERT INTO event_log (seq, id, kind, payload_json, prev_hash, hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(seq)
    .bind(&event.id)
    .bind(&event.kind)
    .bind(&payload_json)
    .bind(&prev_hash)
    .bind(&hash)
    .bind(&created_at)
    .execute(&mut *tx)
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    tx.commit()
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(EventLogRow {
        seq,
        id: event.id,
        kind: event.kind,
        payload_json,
        prev_hash,
        hash,
        created_at,
    })
}

/// Result of walking one hash chain and recomputing every link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReport {
    /// False on the first recomputation or linkage mismatch.
    pub valid: bool,
    /// Links recomputed (rows carrying a chain position).
    pub checked: i64,
    /// Highest chain position observed, if any.
    pub last_seq: Option<i64>,
    /// Hash at `last_seq`, if any.
    pub last_hash: Option<String>,
    /// Chain position of the first bad link, if any.
    pub first_bad_seq: Option<i64>,
    /// Human-readable summary.
    pub detail: String,
}

/// Verify the `event_log` chain: linkage plus recomputed hashes.
pub async fn verify_chain(db: &EvidenceDb) -> Result<ChainReport> {
    let rows: Vec<EventLogRow> =
        sqlx::query_as("SELECT seq, id, kind, payload_json, prev_hash, hash, created_at FROM event_log ORDER BY seq")
            .fetch_all(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    let mut expected_prev = crate::GENESIS_HASH.to_owned();
    let mut checked: i64 = 0;
    for row in &rows {
        let recomputed = chain_hash(
            row.seq,
            &row.id,
            &row.kind,
            &row.payload_json,
            &row.prev_hash,
            &row.created_at,
        );
        if row.prev_hash != expected_prev || recomputed != row.hash {
            return Ok(ChainReport {
                valid: false,
                checked,
                last_seq: rows.last().map(|r| r.seq),
                last_hash: rows.last().map(|r| r.hash.clone()),
                first_bad_seq: Some(row.seq),
                detail: format!("chain broken at seq {}", row.seq),
            });
        }
        expected_prev = row.hash.clone();
        checked += 1;
    }
    Ok(ChainReport {
        valid: true,
        checked,
        last_seq: rows.last().map(|r| r.seq),
        last_hash: rows.last().map(|r| r.hash.clone()),
        first_bad_seq: None,
        detail: format!("{checked} links verified"),
    })
}

/// Verify the evidence chain: payload hashes for every row plus linkage for
/// chained rows (`seq IS NOT NULL`; pre-chain legacy rows check content only).
pub async fn verify_evidence_chain(db: &EvidenceDb) -> Result<ChainReport> {
    let rows = list_recent_evidence(db, 1000).await?;
    // Oldest-first for linkage checks.
    let mut ordered = rows;
    ordered.reverse();
    let mut expected_prev = crate::GENESIS_HASH.to_owned();
    let mut checked: i64 = 0;
    let mut chained = 0_i64;
    for row in &ordered {
        let payload: serde_json::Value = row
            .payload_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(TalosError::Serialization)?
            .unwrap_or(serde_json::Value::Null);
        if hash_payload(&payload) != row.hash {
            return Ok(ChainReport {
                valid: false,
                checked,
                last_seq: row.seq,
                last_hash: Some(row.hash.clone()),
                first_bad_seq: row.seq,
                detail: format!("payload hash mismatch for evidence {}", row.id),
            });
        }
        checked += 1;
        if let (Some(seq), Some(prev)) = (row.seq, row.prev_hash.as_deref()) {
            if prev != expected_prev {
                return Ok(ChainReport {
                    valid: false,
                    checked,
                    last_seq: Some(seq),
                    last_hash: Some(row.hash.clone()),
                    first_bad_seq: Some(seq),
                    detail: format!("evidence chain broken at seq {seq}"),
                });
            }
            expected_prev = row.hash.clone();
            chained += 1;
            let _ = seq;
        }
    }
    let _ = chained;
    Ok(ChainReport {
        valid: true,
        checked,
        last_seq: ordered.iter().filter_map(|r| r.seq).max(),
        last_hash: ordered
            .iter()
            .filter(|r| r.seq.is_some())
            .max_by_key(|r| r.seq)
            .map(|r| r.hash.clone()),
        first_bad_seq: None,
        detail: format!("{checked} evidence rows verified"),
    })
}

/// Count rows in a ledger table.
async fn count_rows(db: &EvidenceDb, table: &str) -> Result<i64> {
    let allowed = [
        "event_log",
        "events",
        "evidence_records",
        "claims",
        "compensation_plans",
    ];
    if !allowed.contains(&table) {
        return Err(TalosError::InvalidId {
            what: "table",
            reason: "count is restricted to ledger tables".to_owned(),
        });
    }
    sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Full ledger integrity snapshot: both chains plus table counts.
#[derive(Debug, Clone)]
pub struct IntegrityReport {
    /// Combined verdict (both chains valid).
    pub healthy: bool,
    /// Event-log chain report.
    pub chain: ChainReport,
    /// Evidence chain report.
    pub evidence_chain: ChainReport,
    /// Rows in `event_log`.
    pub event_count: i64,
    /// Rows in `evidence_records`.
    pub evidence_count: i64,
    /// Rows in `claims`.
    pub claim_count: i64,
    /// Rows in `compensation_plans`.
    pub plan_count: i64,
}

/// Compute [`IntegrityReport`] for the whole ledger.
pub async fn get_integrity_report(db: &EvidenceDb) -> Result<IntegrityReport> {
    let chain = verify_chain(db).await?;
    let evidence_chain = verify_evidence_chain(db).await?;
    let event_count = count_rows(db, "event_log").await?;
    let evidence_count = count_rows(db, "evidence_records").await?;
    let claim_count = count_rows(db, "claims").await?;
    let plan_count = count_rows(db, "compensation_plans").await?;
    Ok(IntegrityReport {
        healthy: chain.valid && evidence_chain.valid,
        chain,
        evidence_chain,
        event_count,
        evidence_count,
        claim_count,
        plan_count,
    })
}

/// Create a bare claim (`pending`, no evidence yet).
pub async fn create_claim(db: &EvidenceDb, claim: NewClaimPending) -> Result<ClaimRow> {
    if claim.id.is_empty() || claim.id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "claim_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if claim.statement.is_empty() {
        return Err(TalosError::InvalidId {
            what: "claim_statement",
            reason: "must not be empty".to_owned(),
        });
    }
    let created_at = now_iso();
    sqlx::query(
        "INSERT INTO claims (id, statement, evidence_ids_json, created_by_agent_id, task_id, created_at, status)
         VALUES (?, ?, '[]', ?, ?, ?, 'pending')",
    )
    .bind(&claim.id)
    .bind(&claim.statement)
    .bind(&claim.created_by_agent_id)
    .bind(&claim.task_id)
    .bind(&created_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(ClaimRow {
        id: claim.id,
        statement: claim.statement,
        evidence_ids_json: "[]".to_owned(),
        created_by_agent_id: claim.created_by_agent_id,
        task_id: claim.task_id,
        created_at,
        status: "pending".to_owned(),
    })
}

/// Attach one evidence row to a claim (link table + JSON mirror kept in sync).
pub async fn attach_evidence_to_claim(
    db: &EvidenceDb,
    claim_id: &str,
    evidence_id: &str,
) -> Result<()> {
    for (what, value) in [("claim_id", claim_id), ("evidence_id", evidence_id)] {
        if value.is_empty() || value.len() > 128 {
            return Err(TalosError::InvalidId {
                what,
                reason: "must be 1..=128 chars".to_owned(),
            });
        }
    }
    let claim_exists: Option<String> = sqlx::query_scalar("SELECT id FROM claims WHERE id = ?")
        .bind(claim_id)
        .fetch_optional(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    if claim_exists.is_none() {
        return Err(TalosError::InvalidId {
            what: "claim_id",
            reason: "claim does not exist".to_owned(),
        });
    }
    let evidence_exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM evidence_records WHERE id = ?")
            .bind(evidence_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    if evidence_exists.is_none() {
        return Err(TalosError::InvalidId {
            what: "evidence_id",
            reason: "evidence does not exist".to_owned(),
        });
    }
    let attached_at = now_iso();
    sqlx::query(
        "INSERT OR IGNORE INTO claim_evidence (claim_id, evidence_id, attached_at) VALUES (?, ?, ?)",
    )
    .bind(claim_id)
    .bind(evidence_id)
    .bind(&attached_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    // Keep the JSON mirror in sync with the link table.
    let linked: Vec<String> = sqlx::query_scalar(
        "SELECT evidence_id FROM claim_evidence WHERE claim_id = ? ORDER BY attached_at",
    )
    .bind(claim_id)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    let mirror = serde_json::to_string(&linked)?;
    sqlx::query("UPDATE claims SET evidence_ids_json = ? WHERE id = ?")
        .bind(&mirror)
        .bind(claim_id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(())
}

/// Recompute a claim's lifecycle status from its linked evidence and persist
/// the transition. Rules: no evidence → `pending`; any rejected with none
/// verified → `rejected`; rejected mixed with verified → `conflicting`; all
/// verified (≥1) → `verified`; otherwise → `pending`.
pub async fn evaluate_claim_status(db: &EvidenceDb, claim_id: &str) -> Result<String> {
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT e.status FROM claim_evidence c JOIN evidence_records e ON e.id = c.evidence_id WHERE c.claim_id = ?",
    )
    .bind(claim_id)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    let any_verified = statuses.iter().any(|s| s == "verified");
    let any_rejected = statuses.iter().any(|s| s == "rejected");
    let status = if statuses.is_empty() {
        "pending"
    } else if any_rejected && any_verified {
        "conflicting"
    } else if any_rejected {
        "rejected"
    } else if any_verified && statuses.iter().all(|s| s == "verified") {
        "verified"
    } else {
        "pending"
    };
    sqlx::query("UPDATE claims SET status = ? WHERE id = ?")
        .bind(status)
        .bind(claim_id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(status.to_owned())
}

/// Transition one evidence row's status (modeled state change, not an edit).
///
/// Allowed: `pending` → `verified` | `rejected`; anything else is rejected.
pub async fn transition_evidence_status(
    db: &EvidenceDb,
    evidence_id: &str,
    to: &str,
) -> Result<()> {
    if !matches!(to, "verified" | "rejected") {
        return Err(TalosError::InvalidId {
            what: "evidence_status",
            reason: "evidence may only transition pending -> verified|rejected".to_owned(),
        });
    }
    let current: Option<String> =
        sqlx::query_scalar("SELECT status FROM evidence_records WHERE id = ?")
            .bind(evidence_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    match current.as_deref() {
        Some("pending") => {}
        Some(other) => {
            return Err(TalosError::InvalidId {
                what: "evidence_status",
                reason: format!("cannot transition evidence from {other}"),
            })
        }
        None => {
            return Err(TalosError::InvalidId {
                what: "evidence_id",
                reason: "evidence does not exist".to_owned(),
            })
        }
    }
    let verified_at = now_iso();
    sqlx::query("UPDATE evidence_records SET status = ?, verified_at = ? WHERE id = ?")
        .bind(to)
        .bind(&verified_at)
        .bind(evidence_id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(())
}

/// Record an evaluated action request (idempotent by request id).
pub async fn record_action_request(
    db: &EvidenceDb,
    request: &NewActionRecord,
    policy_version: &str,
    decision: &str,
    reason: &str,
    required_checks: &[String],
) -> Result<()> {
    let payload_json = serde_json::to_string(&request.payload)?;
    sqlx::query(
        "INSERT OR IGNORE INTO action_requests
         (id, agent_id, task_id, capability, intent, target, payload_json, risk_level, reversible, requested_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&request.id)
    .bind(&request.agent_id)
    .bind(&request.task_id)
    .bind(&request.capability)
    .bind(&request.intent)
    .bind(&request.target)
    .bind(&payload_json)
    .bind(&request.risk_level)
    .bind(i64::from(request.reversible))
    .bind(&request.requested_at)
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    let checks_json = serde_json::to_string(required_checks)?;
    let evaluation_id = format!("eval-{}", request.id);
    sqlx::query(
        "INSERT OR REPLACE INTO policy_evaluations
         (id, action_request_id, decision, reason, policy_version, required_checks_json, evaluated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&evaluation_id)
    .bind(&request.id)
    .bind(decision)
    .bind(reason)
    .bind(policy_version)
    .bind(&checks_json)
    .bind(now_iso())
    .execute(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(())
}

/// True when a compensation plan already covers `action_request_id`.
pub async fn has_compensation_plan(db: &EvidenceDb, action_request_id: &str) -> Result<bool> {
    let id: Option<String> =
        sqlx::query_scalar("SELECT id FROM compensation_plans WHERE action_request_id = ? LIMIT 1")
            .bind(action_request_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(id.is_some())
}

/// True when at least one evidence row is linked to `task_id`.
pub async fn has_evidence_for_task(db: &EvidenceDb, task_id: &str) -> Result<bool> {
    let id: Option<String> =
        sqlx::query_scalar("SELECT id FROM evidence_records WHERE task_id = ? LIMIT 1")
            .bind(task_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(id.is_some())
}

/// Create a compensation plan with ordered steps (status `proposed`).
pub async fn create_compensation_plan(
    db: &EvidenceDb,
    plan: NewCompensationPlan,
) -> Result<CompensationPlanRow> {
    if plan.id.is_empty() || plan.id.len() > 128 {
        return Err(TalosError::InvalidId {
            what: "compensation_plan_id",
            reason: "must be 1..=128 chars".to_owned(),
        });
    }
    if plan.steps.is_empty() {
        return Err(TalosError::InvalidId {
            what: "compensation_steps",
            reason: "at least one step is required".to_owned(),
        });
    }
    let parent: Option<String> = sqlx::query_scalar("SELECT id FROM action_requests WHERE id = ?")
        .bind(&plan.action_request_id)
        .fetch_optional(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    if parent.is_none() {
        return Err(TalosError::InvalidId {
            what: "action_request_id",
            reason: "action request must be recorded first".to_owned(),
        });
    }
    let now = now_iso();
    let mut tx = db
        .pool()
        .begin()
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    sqlx::query(
        "INSERT INTO compensation_plans (id, action_request_id, strategy, status, reason, created_at, updated_at)
         VALUES (?, ?, ?, 'proposed', ?, ?, ?)",
    )
    .bind(&plan.id)
    .bind(&plan.action_request_id)
    .bind(&plan.strategy)
    .bind(&plan.reason)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))?;
    for (index, step) in plan.steps.iter().enumerate() {
        let payload_json = serde_json::to_string(&step.payload)?;
        sqlx::query(
            "INSERT INTO compensation_steps (id, plan_id, step_index, action_type, payload_json, status)
             VALUES (?, ?, ?, ?, ?, 'pending')",
        )
        .bind(format!("{}-step-{index}", plan.id))
        .bind(&plan.id)
        .bind(index as i64)
        .bind(&step.action_type)
        .bind(&payload_json)
        .execute(&mut *tx)
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    }
    tx.commit()
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(CompensationPlanRow {
        id: plan.id,
        action_request_id: plan.action_request_id,
        strategy: plan.strategy,
        status: "proposed".to_owned(),
        reason: plan.reason,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Allowed plan transitions: proposed→approved, approved→executed|failed,
/// executed→rolled_back.
pub async fn transition_compensation_plan(
    db: &EvidenceDb,
    plan_id: &str,
    to: &str,
) -> Result<CompensationPlanRow> {
    let row: Option<CompensationPlanRow> =
        sqlx::query_as("SELECT id, action_request_id, strategy, status, reason, created_at, updated_at FROM compensation_plans WHERE id = ?")
            .bind(plan_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    let mut row = row.ok_or_else(|| TalosError::InvalidId {
        what: "compensation_plan_id",
        reason: "plan does not exist".to_owned(),
    })?;
    let allowed = matches!(
        (row.status.as_str(), to),
        ("proposed", "approved")
            | ("approved", "executed")
            | ("approved", "failed")
            | ("executed", "rolled_back")
    );
    if !allowed {
        return Err(TalosError::InvalidId {
            what: "compensation_plan_status",
            reason: format!("illegal transition {} -> {to}", row.status),
        });
    }
    let now = now_iso();
    sqlx::query("UPDATE compensation_plans SET status = ?, updated_at = ? WHERE id = ?")
        .bind(to)
        .bind(&now)
        .bind(plan_id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    row.status = to.to_owned();
    row.updated_at = now;
    Ok(row)
}

/// Allowed step transitions: pending→in_progress|done|failed|skipped,
/// in_progress→done|failed.
pub async fn transition_compensation_step(db: &EvidenceDb, step_id: &str, to: &str) -> Result<()> {
    let current: Option<String> =
        sqlx::query_scalar("SELECT status FROM compensation_steps WHERE id = ?")
            .bind(step_id)
            .fetch_optional(db.pool())
            .await
            .map_err(|err| TalosError::Storage(err.to_string()))?;
    let current = current.ok_or_else(|| TalosError::InvalidId {
        what: "compensation_step_id",
        reason: "step does not exist".to_owned(),
    })?;
    let allowed = matches!(
        (current.as_str(), to),
        ("pending", "in_progress")
            | ("pending", "done")
            | ("pending", "failed")
            | ("pending", "skipped")
            | ("in_progress", "done")
            | ("in_progress", "failed")
    );
    if !allowed {
        return Err(TalosError::InvalidId {
            what: "compensation_step_status",
            reason: format!("illegal transition {current} -> {to}"),
        });
    }
    sqlx::query("UPDATE compensation_steps SET status = ? WHERE id = ?")
        .bind(to)
        .bind(step_id)
        .execute(db.pool())
        .await
        .map_err(|err| TalosError::Storage(err.to_string()))?;
    Ok(())
}

/// Newest-first claims listing (bounded, capped at 1000).
pub async fn list_recent_claims(db: &EvidenceDb, limit: i64) -> Result<Vec<ClaimRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, ClaimRow>(
        "SELECT id, statement, evidence_ids_json, created_by_agent_id, task_id, created_at, status
         FROM claims ORDER BY created_at DESC, rowid DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Newest-first chained events listing.
pub async fn list_recent_log(db: &EvidenceDb, limit: i64) -> Result<Vec<EventLogRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, EventLogRow>(
        "SELECT seq, id, kind, payload_json, prev_hash, hash, created_at
         FROM event_log ORDER BY seq DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Newest-first compensation plans listing.
pub async fn list_compensation_plans(
    db: &EvidenceDb,
    limit: i64,
) -> Result<Vec<CompensationPlanRow>> {
    let limit = limit.clamp(1, 1000);
    sqlx::query_as::<_, CompensationPlanRow>(
        "SELECT id, action_request_id, strategy, status, reason, created_at, updated_at
         FROM compensation_plans ORDER BY created_at DESC, rowid DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

/// Steps for one plan, in execution order.
pub async fn list_plan_steps(db: &EvidenceDb, plan_id: &str) -> Result<Vec<CompensationStepRow>> {
    sqlx::query_as::<_, CompensationStepRow>(
        "SELECT id, plan_id, step_index, action_type, payload_json, status
         FROM compensation_steps WHERE plan_id = ? ORDER BY step_index",
    )
    .bind(plan_id)
    .fetch_all(db.pool())
    .await
    .map_err(|err| TalosError::Storage(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{insert_evidence, list_recent_events};
    use crate::{NewEvent, NewEvidence};

    async fn memory_db() -> EvidenceDb {
        EvidenceDb::connect_memory()
            .await
            .expect("in-memory database connects")
    }

    fn sample_event(id: &str) -> NewEvent {
        NewEvent {
            id: id.to_owned(),
            kind: "task_created".to_owned(),
            payload_json: serde_json::json!({ "task": "t-1" }),
        }
    }

    #[tokio::test]
    async fn chain_links_in_order() {
        let db = memory_db().await;
        let first = append_event(&db, sample_event("evt-1"))
            .await
            .expect("append works");
        let second = append_event(&db, sample_event("evt-2"))
            .await
            .expect("append works");
        assert_eq!(first.seq, 1);
        assert_eq!(first.prev_hash, crate::GENESIS_HASH);
        assert_eq!(second.seq, 2);
        assert_eq!(second.prev_hash, first.hash);
        let report = verify_chain(&db).await.expect("verify works");
        assert!(report.valid);
        assert_eq!(report.checked, 2);
        assert_eq!(report.last_seq, Some(2));
    }

    #[tokio::test]
    async fn tamper_detection_fires_on_altered_payload() {
        let db = memory_db().await;
        append_event(&db, sample_event("evt-1"))
            .await
            .expect("append works");
        append_event(&db, sample_event("evt-2"))
            .await
            .expect("append works");
        // Adversarial edit straight through SQL, bypassing the repository.
        sqlx::query("UPDATE event_log SET payload_json = ? WHERE seq = 2")
            .bind(r#"{"task":"forged"}"#)
            .execute(db.pool())
            .await
            .expect("tamper executes");
        let report = verify_chain(&db).await.expect("verify works");
        assert!(!report.valid);
        assert_eq!(report.first_bad_seq, Some(2));
    }

    #[tokio::test]
    async fn tamper_detection_fires_on_altered_evidence() {
        let db = memory_db().await;
        insert_evidence(
            &db,
            NewEvidence {
                id: "ev-1".to_owned(),
                task_id: None,
                agent_id: None,
                session_id: None,
                kind: "log".to_owned(),
                label: None,
                summary: None,
                uri: None,
                mime_type: None,
                payload: serde_json::json!({ "ok": true }),
            },
        )
        .await
        .expect("insert works");
        sqlx::query("UPDATE evidence_records SET payload_json = ? WHERE id = 'ev-1'")
            .bind(r#"{"ok":false}"#)
            .execute(db.pool())
            .await
            .expect("tamper executes");
        let report = verify_evidence_chain(&db).await.expect("verify works");
        assert!(!report.valid);
    }

    #[tokio::test]
    async fn claims_link_evidence_and_evaluate() {
        let db = memory_db().await;
        for id in ["ev-a", "ev-b"] {
            insert_evidence(
                &db,
                NewEvidence {
                    id: id.to_owned(),
                    task_id: None,
                    agent_id: None,
                    session_id: None,
                    kind: "log".to_owned(),
                    label: None,
                    summary: None,
                    uri: None,
                    mime_type: None,
                    payload: serde_json::json!({}),
                },
            )
            .await
            .expect("insert works");
        }
        let claim = create_claim(
            &db,
            NewClaimPending {
                id: "claim-1".to_owned(),
                statement: "diagnostic passed".to_owned(),
                created_by_agent_id: None,
                task_id: None,
            },
        )
        .await
        .expect("claim created");
        assert_eq!(claim.status, "pending");
        // No evidence yet: still pending.
        assert_eq!(
            evaluate_claim_status(&db, "claim-1")
                .await
                .expect("evaluates"),
            "pending"
        );
        attach_evidence_to_claim(&db, "claim-1", "ev-a")
            .await
            .expect("attach works");
        // Pending evidence only: still pending.
        assert_eq!(
            evaluate_claim_status(&db, "claim-1")
                .await
                .expect("evaluates"),
            "pending"
        );
        transition_evidence_status(&db, "ev-a", "verified")
            .await
            .expect("transition works");
        transition_evidence_status(&db, "ev-b", "verified")
            .await
            .expect("transition works");
        attach_evidence_to_claim(&db, "claim-1", "ev-b")
            .await
            .expect("attach works");
        assert_eq!(
            evaluate_claim_status(&db, "claim-1")
                .await
                .expect("evaluates"),
            "verified"
        );
    }

    #[tokio::test]
    async fn conflicting_evidence_yields_conflicting_claim() {
        let db = memory_db().await;
        for id in ["ev-a", "ev-b"] {
            insert_evidence(
                &db,
                NewEvidence {
                    id: id.to_owned(),
                    task_id: None,
                    agent_id: None,
                    session_id: None,
                    kind: "log".to_owned(),
                    label: None,
                    summary: None,
                    uri: None,
                    mime_type: None,
                    payload: serde_json::json!({}),
                },
            )
            .await
            .expect("insert works");
        }
        create_claim(
            &db,
            NewClaimPending {
                id: "claim-1".to_owned(),
                statement: "mixed signals".to_owned(),
                created_by_agent_id: None,
                task_id: None,
            },
        )
        .await
        .expect("claim created");
        attach_evidence_to_claim(&db, "claim-1", "ev-a")
            .await
            .expect("attach works");
        attach_evidence_to_claim(&db, "claim-1", "ev-b")
            .await
            .expect("attach works");
        transition_evidence_status(&db, "ev-a", "verified")
            .await
            .expect("transition works");
        transition_evidence_status(&db, "ev-b", "rejected")
            .await
            .expect("transition works");
        assert_eq!(
            evaluate_claim_status(&db, "claim-1")
                .await
                .expect("evaluates"),
            "conflicting"
        );
    }

    #[tokio::test]
    async fn attach_rejects_unknown_sides() {
        let db = memory_db().await;
        create_claim(
            &db,
            NewClaimPending {
                id: "claim-1".to_owned(),
                statement: "x".to_owned(),
                created_by_agent_id: None,
                task_id: None,
            },
        )
        .await
        .expect("claim created");
        assert!(attach_evidence_to_claim(&db, "claim-1", "missing-ev")
            .await
            .is_err());
        assert!(attach_evidence_to_claim(&db, "missing-claim", "missing-ev")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn compensation_plan_lifecycle() {
        let db = memory_db().await;
        // Plan requires a recorded action request first.
        record_action_request(
            &db,
            &crate::NewActionRecord {
                id: "req-1".to_owned(),
                agent_id: "a-1".to_owned(),
                task_id: "t-1".to_owned(),
                capability: "SystemControl".to_owned(),
                intent: "reboot".to_owned(),
                target: "node-1".to_owned(),
                payload: serde_json::json!({}),
                risk_level: "Irreversible".to_owned(),
                reversible: false,
                requested_at: now_iso(),
            },
            "talos-policy-broker/1",
            "require_compensation",
            "r",
            &[],
        )
        .await
        .expect("request recorded");
        assert!(!has_compensation_plan(&db, "req-1").await.expect("checks"));
        let plan = create_compensation_plan(
            &db,
            NewCompensationPlan {
                id: "plan-1".to_owned(),
                action_request_id: "req-1".to_owned(),
                strategy: "rollback_command".to_owned(),
                reason: "reboot needs a way back".to_owned(),
                steps: vec![
                    crate::NewCompensationStep {
                        action_type: "rollback_command".to_owned(),
                        payload: serde_json::json!({ "cmd": "wake" }),
                    },
                    crate::NewCompensationStep {
                        action_type: "manual_review".to_owned(),
                        payload: serde_json::json!({}),
                    },
                ],
            },
        )
        .await
        .expect("plan created");
        assert_eq!(plan.status, "proposed");
        let steps = list_plan_steps(&db, "plan-1").await.expect("lists steps");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].step_index, 0);
        assert!(has_compensation_plan(&db, "req-1").await.expect("checks"));
        // Illegal jump proposed -> executed is refused.
        assert!(transition_compensation_plan(&db, "plan-1", "executed")
            .await
            .is_err());
        let approved = transition_compensation_plan(&db, "plan-1", "approved")
            .await
            .expect("approves");
        assert_eq!(approved.status, "approved");
        transition_compensation_step(&db, "plan-1-step-0", "done")
            .await
            .expect("step completes");
        let executed = transition_compensation_plan(&db, "plan-1", "executed")
            .await
            .expect("executes");
        assert_eq!(executed.status, "executed");
    }

    #[tokio::test]
    async fn integrity_report_counts_tables() {
        let db = memory_db().await;
        append_event(&db, sample_event("evt-1"))
            .await
            .expect("append works");
        let report = get_integrity_report(&db).await.expect("reports");
        assert!(report.healthy);
        assert_eq!(report.event_count, 1);
        assert_eq!(report.chain.checked, 1);
    }

    #[tokio::test]
    async fn legacy_events_still_list() {
        // The pre-chain `events` table keeps working alongside `event_log`.
        let db = memory_db().await;
        crate::insert_event(
            &db,
            NewEvent {
                id: "legacy-1".to_owned(),
                kind: "x".to_owned(),
                payload_json: serde_json::json!({}),
            },
        )
        .await
        .expect("insert works");
        let rows = list_recent_events(&db, 10).await.expect("lists");
        assert_eq!(rows.len(), 1);
    }
}
