-- Talos ledger upgrade: hash-chained event log, claim status, claim links,
-- policy audit trail, compensation plans. Append-only; status changes are
-- explicit transitions performed by repository functions, never ad-hoc edits.

-- Hash-chained immutable event log (genesis prev_hash = 'GENESIS').
CREATE TABLE IF NOT EXISTS event_log (
    seq          INTEGER PRIMARY KEY AUTOINCREMENT,
    id           TEXT NOT NULL UNIQUE,
    kind         TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    prev_hash    TEXT NOT NULL,
    hash         TEXT NOT NULL,
    created_at   TEXT NOT NULL
);

-- Evidence hash chain columns (populated for every new row).
ALTER TABLE evidence_records ADD COLUMN seq INTEGER;
ALTER TABLE evidence_records ADD COLUMN prev_hash TEXT;

-- Claim lifecycle status: pending | verified | rejected | conflicting.
ALTER TABLE claims ADD COLUMN status TEXT NOT NULL DEFAULT 'pending';

-- Normalized claim <-> evidence links (evidence_ids_json stays in sync).
CREATE TABLE IF NOT EXISTS claim_evidence (
    claim_id    TEXT NOT NULL REFERENCES claims(id),
    evidence_id TEXT NOT NULL REFERENCES evidence_records(id),
    attached_at TEXT NOT NULL,
    PRIMARY KEY (claim_id, evidence_id)
);

-- Policy audit trail: every evaluated request is recorded.
CREATE TABLE IF NOT EXISTS action_requests (
    id           TEXT PRIMARY KEY,
    agent_id     TEXT NOT NULL,
    task_id      TEXT NOT NULL,
    capability   TEXT NOT NULL,
    intent       TEXT NOT NULL,
    target       TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    risk_level   TEXT NOT NULL,
    reversible   INTEGER NOT NULL,
    requested_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS policy_evaluations (
    id                   TEXT PRIMARY KEY,
    action_request_id    TEXT NOT NULL REFERENCES action_requests(id),
    decision             TEXT NOT NULL,
    reason               TEXT NOT NULL,
    policy_version       TEXT NOT NULL,
    required_checks_json TEXT NOT NULL,
    evaluated_at         TEXT NOT NULL
);

-- Compensation plans for irreversible actions.
CREATE TABLE IF NOT EXISTS compensation_plans (
    id                TEXT PRIMARY KEY,
    action_request_id TEXT NOT NULL REFERENCES action_requests(id),
    strategy          TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'proposed',
    reason            TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS compensation_steps (
    id           TEXT PRIMARY KEY,
    plan_id      TEXT NOT NULL REFERENCES compensation_plans(id),
    step_index   INTEGER NOT NULL,
    action_type  TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending'
);

CREATE INDEX IF NOT EXISTS idx_event_log_created_at ON event_log (created_at);
CREATE INDEX IF NOT EXISTS idx_evidence_seq ON evidence_records (seq);
CREATE INDEX IF NOT EXISTS idx_claim_evidence_claim ON claim_evidence (claim_id);
CREATE INDEX IF NOT EXISTS idx_compensation_steps_plan ON compensation_steps (plan_id);
CREATE INDEX IF NOT EXISTS idx_policy_evaluations_request ON policy_evaluations (action_request_id);
