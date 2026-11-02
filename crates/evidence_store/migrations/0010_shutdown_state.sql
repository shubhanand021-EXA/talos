-- Crash-recovery shutdown flag. A single row is flipped to 'dirty' on every
-- startup and back to 'clean' on orderly shutdown; a 'dirty' row at startup
-- proves the previous process died mid-flight (kill -9, power loss, crash).
CREATE TABLE IF NOT EXISTS shutdown_state (
    flag TEXT PRIMARY KEY,
    updated_at TEXT NOT NULL
);

-- Checkpoint + receipt lookups filter on (task_id, kind).
CREATE INDEX IF NOT EXISTS idx_evidence_task_kind ON evidence_records (task_id, kind);
