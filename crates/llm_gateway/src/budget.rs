//! Per-task spend tracking against optional budgets.
//!
//! Spend is derived from the `usage_records` table (the append-only source of
//! truth). Deny decisions happen before any provider call, so an exhausted
//! budget can never trigger network traffic.

/// Precision floor: estimates below one micro-dollar round to zero.
pub const MAX_SPEND_PRECISION_USD: f64 = 0.000_001;

/// Read-only budget view over recorded usage.
pub struct BudgetTracker<'a> {
    db: &'a evidence_store::EvidenceDb,
}

impl<'a> BudgetTracker<'a> {
    /// Wrap an evidence database handle.
    pub fn new(db: &'a evidence_store::EvidenceDb) -> Self {
        Self { db }
    }

    /// Cumulative estimated spend for one task in USD.
    pub async fn task_spend_usd(&self, task_id: &str) -> Result<f64, String> {
        evidence_store::sum_task_spend(self.db, task_id)
            .await
            .map_err(|err| err.to_string())
    }

    /// True when `task_spend_usd + projected` would exceed `budget_usd`.
    /// A `None` budget and zero/negative budgets never deny (`None` = uncapped).
    pub async fn would_exceed(
        &self,
        task_id: &str,
        budget_usd: Option<f64>,
        projected_usd: f64,
    ) -> Result<bool, String> {
        let Some(budget) = budget_usd else {
            return Ok(false);
        };
        if budget <= 0.0 {
            return Ok(false);
        }
        let spent = self.task_spend_usd(task_id).await?;
        Ok(spent + projected_usd.max(0.0) > budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn uncapped_tasks_never_deny() {
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db");
        let tracker = BudgetTracker::new(&db);
        assert_eq!(tracker.task_spend_usd("task-1").await.unwrap(), 0.0);
        assert!(!tracker.would_exceed("task-1", None, 999.0).await.unwrap());
        assert!(!tracker
            .would_exceed("task-1", Some(0.0), 999.0)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn spend_accumulates_and_caps() {
        let db = evidence_store::EvidenceDb::connect_memory()
            .await
            .expect("memory db");
        evidence_store::insert_usage_record(
            &db,
            &evidence_store::NewUsageRecord {
                id: "u-1",
                task_id: "task-9",
                agent_id: "agent-1",
                provider_id: "openai",
                model_id: "gpt-4o-mini",
                prompt_tokens: 1000,
                completion_tokens: 500,
                total_tokens: 1500,
                estimated_cost_usd: 0.00045,
                latency_ms: 100,
            },
        )
        .await
        .expect("insert");
        let tracker = BudgetTracker::new(&db);
        let spent = tracker.task_spend_usd("task-9").await.unwrap();
        assert!((spent - 0.00045).abs() < MAX_SPEND_PRECISION_USD);
        assert!(tracker
            .would_exceed("task-9", Some(0.0004), 0.0)
            .await
            .unwrap());
        assert!(!tracker
            .would_exceed("task-9", Some(1.0), 0.0)
            .await
            .unwrap());
        assert!(tracker
            .would_exceed("task-9", Some(0.0005), 0.0001)
            .await
            .unwrap());
    }
}
