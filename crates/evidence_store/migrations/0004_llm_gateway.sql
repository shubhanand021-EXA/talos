-- Talos Batch 3: LLM gateway usage + interaction records.
--
-- usage_records is the append-only spend ledger (one row per provider call).
-- model_interactions stores request/response SHA-256 hashes only — raw
-- prompts and completions are never persisted, keeping the ledger private
-- by construction. runtime_tasks gains an optional per-task spend cap.

CREATE TABLE IF NOT EXISTS usage_records (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    estimated_cost_usd REAL NOT NULL,
    latency_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_interactions (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    response_hash TEXT NOT NULL,
    finish_reason TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_usage_records_task ON usage_records(task_id);
CREATE INDEX IF NOT EXISTS idx_usage_records_created ON usage_records(created_at);
CREATE INDEX IF NOT EXISTS idx_model_interactions_task ON model_interactions(task_id);
CREATE INDEX IF NOT EXISTS idx_model_interactions_created ON model_interactions(created_at);

ALTER TABLE runtime_tasks ADD COLUMN max_spend_usd REAL;
