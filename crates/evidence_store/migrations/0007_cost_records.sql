-- Talos Batch 3 (telemetry): real cost tracking ledger.
--
-- cost_records is the append-only spend ledger for non-LLM resources
-- (tool executions, browser sessions, computer-use actions). LLM spend
-- stays in usage_records; dashboards sum both. Only metadata and amounts
-- land here — never prompts, file contents, or secrets.

CREATE TABLE IF NOT EXISTS cost_records (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    cost_type TEXT NOT NULL,
    amount_usd REAL NOT NULL,
    resource TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cost_records_task ON cost_records(task_id);
CREATE INDEX IF NOT EXISTS idx_cost_records_agent ON cost_records(agent_id);
CREATE INDEX IF NOT EXISTS idx_cost_records_type ON cost_records(cost_type);
CREATE INDEX IF NOT EXISTS idx_cost_records_created ON cost_records(created_at);
