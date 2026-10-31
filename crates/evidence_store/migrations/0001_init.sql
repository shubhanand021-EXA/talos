-- Talos evidence ledger, initial schema.
-- Append-only mindset: rows are INSERTed and SELECTed, never UPDATEd or DELETEd.

CREATE TABLE IF NOT EXISTS events (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS evidence_records (
    id          TEXT PRIMARY KEY,
    task_id     TEXT,
    agent_id    TEXT,
    session_id  TEXT,
    kind        TEXT NOT NULL,
    status      TEXT NOT NULL,
    label       TEXT,
    summary     TEXT,
    hash        TEXT NOT NULL,
    uri         TEXT,
    mime_type   TEXT,
    payload_json TEXT,
    created_at  TEXT NOT NULL,
    verified_at TEXT
);

CREATE TABLE IF NOT EXISTS claims (
    id                  TEXT PRIMARY KEY,
    statement           TEXT NOT NULL,
    evidence_ids_json   TEXT NOT NULL,
    created_by_agent_id TEXT,
    task_id             TEXT,
    created_at          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_created_at ON events (created_at);
CREATE INDEX IF NOT EXISTS idx_evidence_created_at ON evidence_records (created_at);
