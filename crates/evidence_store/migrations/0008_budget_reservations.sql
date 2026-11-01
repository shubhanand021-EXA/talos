-- Talos child-budget reservations (Batch 4): a Supervisor reserves an equal
-- share of the parent budget per child at spawn; settlement records actual
-- spend and returns the unused share. Enforcement itself rides on
-- runtime_tasks.max_spend_usd (set to the share at spawn), which the LLM
-- gateway and tool executor already consult; this table is the audit trail
-- plus the remaining-budget source for explicit checks.

CREATE TABLE IF NOT EXISTS budget_reservations (
    reservation_id TEXT PRIMARY KEY,
    parent_task_id TEXT NOT NULL,
    child_task_id  TEXT NOT NULL,
    reserved_usd   REAL NOT NULL,
    spent_usd      REAL NOT NULL DEFAULT 0,
    status         TEXT NOT NULL DEFAULT 'active',
    created_at     TEXT NOT NULL,
    settled_at     TEXT
);

CREATE INDEX IF NOT EXISTS idx_budget_reservations_child
    ON budget_reservations (child_task_id, status);
CREATE INDEX IF NOT EXISTS idx_budget_reservations_parent
    ON budget_reservations (parent_task_id);
