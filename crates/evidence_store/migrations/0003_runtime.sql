-- Talos durable runtime: SQLite-backed task queue, attempts, transitions,
-- cancellation requests, and retry metadata. Append-only history; current
-- state lives in `runtime_tasks`, every change is journaled.

CREATE TABLE IF NOT EXISTS runtime_tasks (
    id             TEXT PRIMARY KEY,
    title          TEXT NOT NULL,
    description    TEXT,
    priority       TEXT NOT NULL DEFAULT 'normal',
    status         TEXT NOT NULL DEFAULT 'queued',
    attempts       INTEGER NOT NULL DEFAULT 0,
    max_attempts   INTEGER NOT NULL DEFAULT 3,
    timeout_ms     INTEGER NOT NULL DEFAULT 30000,
    backoff_ms     INTEGER NOT NULL DEFAULT 500,
    scenario       TEXT NOT NULL DEFAULT 'quick_success',
    last_error     TEXT,
    next_retry_at  TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runtime_attempts (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id      TEXT NOT NULL REFERENCES runtime_tasks(id),
    attempt      INTEGER NOT NULL,
    started_at   TEXT NOT NULL,
    finished_at  TEXT,
    outcome      TEXT NOT NULL DEFAULT 'started',
    error        TEXT
);

CREATE TABLE IF NOT EXISTS runtime_transitions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    TEXT NOT NULL REFERENCES runtime_tasks(id),
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS runtime_cancellations (
    task_id      TEXT PRIMARY KEY REFERENCES runtime_tasks(id),
    requested_at TEXT NOT NULL,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    completed    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_runtime_tasks_status ON runtime_tasks (status);
CREATE INDEX IF NOT EXISTS idx_runtime_tasks_priority ON runtime_tasks (priority, created_at);
CREATE INDEX IF NOT EXISTS idx_runtime_attempts_task ON runtime_attempts (task_id, attempt);
CREATE INDEX IF NOT EXISTS idx_runtime_transitions_task ON runtime_transitions (task_id);
