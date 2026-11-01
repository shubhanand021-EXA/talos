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
}
