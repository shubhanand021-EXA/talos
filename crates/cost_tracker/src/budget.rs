//! Budget enforcement: per-task caps + session cap + 80% warnings.

use evidence_store::EvidenceDb;
use talos_common::{Result, TalosError};

fn bad(what: &str) -> TalosError {
    TalosError::Storage(what.to_owned())
}

/// Verdict of a budget check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetDecision {
    /// Spend is within limits.
    Allow,
    /// Spend crossed 80% of a limit (still allowed, warn the operator).
    Warn,
    /// Spend exceeded a limit (deny the action).
    Deny,
}

/// One budget evaluation (task cap and/or session cap).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetCheck {
    /// Verdict.
    pub decision: BudgetDecision,
    /// Task spend in USD (LLM + cost_records).
    pub task_spent_usd: f64,
    /// Task cap, if configured.
    pub task_budget_usd: Option<f64>,
    /// Session spend in USD.
    pub session_spent_usd: f64,
    /// Session cap, if configured.
    pub session_budget_usd: Option<f64>,
    /// Human-readable reason.
    pub reason: String,
}

/// Configurable global session budget.
#[derive(Debug, Clone)]
pub struct SessionBudget {
    /// Cap in USD (`None` = uncapped).
    pub max_spend_usd: Option<f64>,
}

impl SessionBudget {
    /// Uncapped session.
    #[must_use]
    pub fn uncapped() -> Self {
        Self {
            max_spend_usd: None,
        }
    }

    /// Capped session.
    #[must_use]
    pub fn capped(max_spend_usd: f64) -> Self {
        Self {
            max_spend_usd: Some(max_spend_usd),
        }
    }
}

/// Budget tracker: evaluates spend against caps.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    /// Session-level budget.
    pub session: SessionBudget,
}

impl BudgetTracker {
    /// Build a tracker with a session budget.
    #[must_use]
    pub fn new(session: SessionBudget) -> Self {
        Self { session }
    }

    /// Evaluate whether `proposed_cost_usd` may proceed for `task_id`.
    ///
    /// Total task spend = `usage_records` sum + `cost_records` sum for the
    /// task. Total session spend = both tables session-wide. Denies when
    /// either cap would be exceeded; warns at 80% of either cap.
    pub async fn check(
        &self,
        db: &EvidenceDb,
        task_id: &str,
        proposed_cost_usd: f64,
    ) -> Result<BudgetCheck> {
        if !proposed_cost_usd.is_finite() || proposed_cost_usd < 0.0 {
            return Err(bad("proposed cost must be finite and >= 0"));
        }
        let task_budget = evidence_store::get_task_budget(db, task_id).await?;
        let task_spent = total_task_spend(db, task_id).await?;
        let session_spent = total_session_spend(db).await?;
        let task_after = task_spent + proposed_cost_usd;
        let session_after = session_spent + proposed_cost_usd;

        let task_denied = task_budget.is_some_and(|cap| task_after > cap);
        let session_denied = self
            .session
            .max_spend_usd
            .is_some_and(|cap| session_after > cap);
        if task_denied || session_denied {
            let reason = if task_denied {
                format!(
                    "task budget exceeded: spent {task_after:.6} > cap {:.6}",
                    task_budget.unwrap_or(0.0)
                )
            } else {
                format!(
                    "session budget exceeded: spent {session_after:.6} > cap {:.6}",
                    self.session.max_spend_usd.unwrap_or(0.0)
                )
            };
            return Ok(BudgetCheck {
                decision: BudgetDecision::Deny,
                task_spent_usd: task_spent,
                task_budget_usd: task_budget,
                session_spent_usd: session_spent,
                session_budget_usd: self.session.max_spend_usd,
                reason,
            });
        }

        let task_warn = task_budget.is_some_and(|cap| cap > 0.0 && task_after >= 0.8 * cap);
        let session_warn = self
            .session
            .max_spend_usd
            .is_some_and(|cap| cap > 0.0 && session_after >= 0.8 * cap);
        if task_warn || session_warn {
            return Ok(BudgetCheck {
                decision: BudgetDecision::Warn,
                task_spent_usd: task_spent,
                task_budget_usd: task_budget,
                session_spent_usd: session_spent,
                session_budget_usd: self.session.max_spend_usd,
                reason: "spend above 80% of budget".to_owned(),
            });
        }
        Ok(BudgetCheck {
            decision: BudgetDecision::Allow,
            task_spent_usd: task_spent,
            task_budget_usd: task_budget,
            session_spent_usd: session_spent,
            session_budget_usd: self.session.max_spend_usd,
            reason: "within budget".to_owned(),
        })
    }
}

/// Combined task spend (LLM usage + tracked tool/browser/computer costs).
pub async fn total_task_spend(db: &EvidenceDb, task_id: &str) -> Result<f64> {
    let llm = evidence_store::sum_task_spend(db, task_id).await?;
    let tracked = evidence_store::sum_task_cost_records(db, task_id).await?;
    Ok(llm + tracked)
}

/// Combined session spend (LLM usage + tracked costs, all tasks).
pub async fn total_session_spend(db: &EvidenceDb) -> Result<f64> {
    let (_, llm_cost, _) = evidence_store::summarize_usage(db).await?;
    let tracked = evidence_store::sum_all_cost_records(db).await?;
    Ok(llm_cost + tracked)
}

/// Child-budget reservation identifier (`rsv-{uuid}`).
pub type ReservationId = String;

/// Outcome of an explicit child-budget check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildBudgetVerdict {
    /// Spend (actual + proposed) fits the reservation (or no reservation).
    Allow,
    /// Spend would exceed the reservation: deny further calls for the child.
    Deny,
}

/// Reserve an equal-share budget for one child (Batch 4 delegation budgets).
///
/// Persists an `active` row in `budget_reservations` and returns its id.
/// Callers also set the child task cap (`set_task_budget`) so the LLM
/// gateway and tool executor deny over-spend automatically; this ledger is
/// the audit trail plus the source for settlement and explicit checks.
pub async fn reserve_budget(
    db: &EvidenceDb,
    parent_task_id: &str,
    child_task_id: &str,
    amount_usd: f64,
) -> Result<ReservationId> {
    let reservation_id = format!("rsv-{}", uuid::Uuid::new_v4());
    evidence_store::insert_budget_reservation(
        db,
        &evidence_store::NewBudgetReservation {
            reservation_id: &reservation_id,
            parent_task_id,
            child_task_id,
            reserved_usd: amount_usd,
        },
    )
    .await?;
    evidence_store::append_event(
        db,
        evidence_store::NewEvent {
            id: format!("budget-reserved-{reservation_id}"),
            kind: "budget.reserved".to_owned(),
            payload_json: serde_json::json!({
                "reservation_id": reservation_id,
                "parent_task_id": parent_task_id,
                "child_task_id": child_task_id,
                "reserved_usd": amount_usd,
            }),
        },
    )
    .await?;
    Ok(reservation_id)
}

/// Remaining budget for a child with an active reservation.
///
/// Returns `None` when the child has no active reservation (uncapped).
pub async fn get_remaining_budget(db: &EvidenceDb, child_task_id: &str) -> Result<Option<f64>> {
    let Some(row) = evidence_store::get_active_reservation_for_child(db, child_task_id).await?
    else {
        return Ok(None);
    };
    let spent = total_task_spend(db, child_task_id).await?;
    Ok(Some(row.reserved_usd - spent))
}

/// Explicit pre-call gate for one child: deny when actual + proposed spend
/// would exceed the active reservation. No reservation → allow.
pub async fn check_child_budget(
    db: &EvidenceDb,
    child_task_id: &str,
    proposed_usd: f64,
) -> Result<ChildBudgetVerdict> {
    if !proposed_usd.is_finite() || proposed_usd < 0.0 {
        return Err(bad("proposed cost must be finite and >= 0"));
    }
    let Some(remaining) = get_remaining_budget(db, child_task_id).await? else {
        return Ok(ChildBudgetVerdict::Allow);
    };
    if proposed_usd > remaining {
        Ok(ChildBudgetVerdict::Deny)
    } else {
        Ok(ChildBudgetVerdict::Allow)
    }
}

/// Active reservation spend detail for one child: `(reserved_usd, spent_usd)`.
/// `None` when the child has no active reservation.
pub async fn get_child_reservation_spend(
    db: &EvidenceDb,
    child_task_id: &str,
) -> Result<Option<(f64, f64)>> {
    let Some(row) = evidence_store::get_active_reservation_for_child(db, child_task_id).await?
    else {
        return Ok(None);
    };
    let spent = total_task_spend(db, child_task_id).await?;
    Ok(Some((row.reserved_usd, spent)))
}

/// Record `budget.child_exceeded` when the child's spend already exceeds its
/// active reservation. Returns true when the event was recorded (false when
/// there is no active reservation or spend still fits).
pub async fn flag_child_overspend(db: &EvidenceDb, child_task_id: &str) -> Result<bool> {
    let Some((reserved_usd, spent_usd)) = get_child_reservation_spend(db, child_task_id).await?
    else {
        return Ok(false);
    };
    if spent_usd <= reserved_usd {
        return Ok(false);
    }
    evidence_store::append_event(
        db,
        evidence_store::NewEvent {
            id: format!("budget-exceeded-{child_task_id}-{}", uuid::Uuid::new_v4()),
            kind: "budget.child_exceeded".to_owned(),
            payload_json: serde_json::json!({
                "child_task_id": child_task_id,
                "reserved_usd": reserved_usd,
                "spent_usd": spent_usd,
            }),
        },
    )
    .await?;
    Ok(true)
}

/// Settle a reservation: record actual spend, mark `settled`, return the
/// unused share (`reserved - actual`, negative on overspend).
///
/// Overspend appends a `budget.child_exceeded` ledger event so the desktop
/// layer can surface `ChildBudgetExceeded`; the child task cap stays in
/// place regardless.
pub async fn settle_budget(
    db: &EvidenceDb,
    reservation_id: &str,
    actual_spend_usd: f64,
) -> Result<f64> {
    let row =
        evidence_store::settle_budget_reservation(db, reservation_id, actual_spend_usd).await?;
    let unused = row.reserved_usd - actual_spend_usd;
    evidence_store::append_event(
        db,
        evidence_store::NewEvent {
            id: format!("budget-settled-{reservation_id}"),
            kind: "budget.settled".to_owned(),
            payload_json: serde_json::json!({
                "reservation_id": reservation_id,
                "parent_task_id": row.parent_task_id,
                "child_task_id": row.child_task_id,
                "reserved_usd": row.reserved_usd,
                "spent_usd": actual_spend_usd,
                "unused_usd": unused,
            }),
        },
    )
    .await?;
    if actual_spend_usd > row.reserved_usd {
        evidence_store::append_event(
            db,
            evidence_store::NewEvent {
                id: format!("budget-exceeded-{reservation_id}"),
                kind: "budget.child_exceeded".to_owned(),
                payload_json: serde_json::json!({
                    "reservation_id": reservation_id,
                    "child_task_id": row.child_task_id,
                    "reserved_usd": row.reserved_usd,
                    "spent_usd": actual_spend_usd,
                }),
            },
        )
        .await?;
    }
    Ok(unused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evidence_store::EnqueueParams;

    async fn seeded_db() -> EvidenceDb {
        let db = EvidenceDb::connect_memory().await.expect("memory db");
        evidence_store::enqueue_task(
            &db,
            EnqueueParams {
                id: "task-budget",
                title: "budget task",
                description: None,
                priority: "normal",
                max_attempts: 3,
                timeout_ms: 5000,
                backoff_ms: 500,
                scenario: "quick_success",
                parent_task_id: None,
            },
        )
        .await
        .expect("enqueue");
        db
    }

    #[tokio::test]
    async fn allows_under_cap_warns_at_80_denies_over() {
        let db = seeded_db().await;
        evidence_store::set_task_budget(&db, "task-budget", Some(1.0))
            .await
            .unwrap();
        evidence_store::insert_cost_record(
            &db,
            &evidence_store::NewCostRecord {
                id: "c-1",
                task_id: "task-budget",
                agent_id: "a-1",
                cost_type: "tool",
                amount_usd: 0.5,
                resource: "fs.read",
            },
        )
        .await
        .unwrap();
        let tracker = BudgetTracker::new(SessionBudget::uncapped());
        // 0.5 spent + 0.1 proposed = 0.6 < 0.8 -> allow.
        let allow = tracker.check(&db, "task-budget", 0.1).await.unwrap();
        assert_eq!(allow.decision, BudgetDecision::Allow);
        // 0.5 + 0.35 = 0.85 >= 0.8 -> warn.
        let warn = tracker.check(&db, "task-budget", 0.35).await.unwrap();
        assert_eq!(warn.decision, BudgetDecision::Warn);
        // 0.5 + 0.6 = 1.1 > 1.0 -> deny.
        let deny = tracker.check(&db, "task-budget", 0.6).await.unwrap();
        assert_eq!(deny.decision, BudgetDecision::Deny);
    }

    #[tokio::test]
    async fn session_cap_enforced() {
        let db = seeded_db().await;
        let tracker = BudgetTracker::new(SessionBudget::capped(0.1));
        let deny = tracker.check(&db, "task-budget", 0.5).await.unwrap();
        assert_eq!(deny.decision, BudgetDecision::Deny);
    }

    async fn spend(db: &EvidenceDb, task: &str, amount: f64) {
        evidence_store::insert_cost_record(
            db,
            &evidence_store::NewCostRecord {
                id: &format!("c-{task}-{amount}"),
                task_id: task,
                agent_id: "a-1",
                cost_type: "tool",
                amount_usd: amount,
                resource: "process.exec",
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn reservation_splits_parent_two_ways() {
        // TAL-004: $1.00 parent, 2 children → $0.50 reservations each.
        let db = seeded_db().await;
        let first = reserve_budget(&db, "task-budget", "child-1", 0.5)
            .await
            .expect("reserve");
        let second = reserve_budget(&db, "task-budget", "child-2", 0.5)
            .await
            .expect("reserve");
        assert_ne!(first, second);
        assert_eq!(
            get_remaining_budget(&db, "child-1").await.unwrap(),
            Some(0.5)
        );
        assert_eq!(
            get_remaining_budget(&db, "child-2").await.unwrap(),
            Some(0.5)
        );
        // No reservation → uncapped.
        assert_eq!(get_remaining_budget(&db, "unknown").await.unwrap(), None);
    }

    #[tokio::test]
    async fn overspend_denies_further_calls() {
        // TAL-004: child spends $0.60 of $0.50 → Deny + exceeded event.
        let db = seeded_db().await;
        let reservation = reserve_budget(&db, "task-budget", "child-1", 0.5)
            .await
            .expect("reserve");
        spend(&db, "child-1", 0.6).await;
        assert_eq!(
            check_child_budget(&db, "child-1", 0.0).await.unwrap(),
            ChildBudgetVerdict::Deny
        );
        // Within budget still allows.
        let db2 = seeded_db().await;
        reserve_budget(&db2, "task-budget", "child-1", 0.5)
            .await
            .expect("reserve");
        spend(&db2, "child-1", 0.2).await;
        assert_eq!(
            check_child_budget(&db2, "child-1", 0.2).await.unwrap(),
            ChildBudgetVerdict::Allow
        );
        assert_eq!(
            check_child_budget(&db2, "child-1", 0.4).await.unwrap(),
            ChildBudgetVerdict::Deny
        );
        // Settlement records the overspend + exceeded ledger event.
        let unused = settle_budget(&db, &reservation, 0.6).await.unwrap();
        assert!((unused - (-0.1)).abs() < 1e-9);
        let log = evidence_store::list_recent_log(&db, 20).await.unwrap();
        assert!(log.iter().any(|e| e.kind == "budget.child_exceeded"));
    }

    #[tokio::test]
    async fn settlement_returns_unused_share() {
        // TAL-004: child spends $0.30 of $0.50 → $0.20 back, settled.
        let db = seeded_db().await;
        let reservation = reserve_budget(&db, "task-budget", "child-1", 0.5)
            .await
            .expect("reserve");
        spend(&db, "child-1", 0.3).await;
        let unused = settle_budget(&db, &reservation, 0.3).await.unwrap();
        assert!((unused - 0.2).abs() < 1e-9);
        // Settled reservations no longer gate.
        assert_eq!(get_remaining_budget(&db, "child-1").await.unwrap(), None);
        assert_eq!(
            check_child_budget(&db, "child-1", 100.0).await.unwrap(),
            ChildBudgetVerdict::Allow
        );
        // Double settlement is refused.
        assert!(settle_budget(&db, &reservation, 0.3).await.is_err());
    }
}
