-- Talos skill memory: project-level procedural knowledge distilled from
-- successful tasks. Skills start quarantined; promotion to validated /
-- published is an explicit status transition (never implicit).

CREATE TABLE IF NOT EXISTS skills (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    procedure_text TEXT NOT NULL,
    source_task_id TEXT REFERENCES runtime_tasks(id),
    version        TEXT NOT NULL DEFAULT '0.1.0',
    test_fixture_ref TEXT,
    required_tools TEXT NOT NULL DEFAULT '[]',
    permission_ceiling TEXT NOT NULL DEFAULT 'low',
    status         TEXT NOT NULL DEFAULT 'quarantined',
    created_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_skills_status ON skills (status);
CREATE INDEX IF NOT EXISTS idx_skills_source_task ON skills (source_task_id);
