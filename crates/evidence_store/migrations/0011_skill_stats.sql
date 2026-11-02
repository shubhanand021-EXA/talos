-- GEPA skill evolution: usage statistics, confidence tracking, version
-- lineage, and the full lifecycle (quarantined -> validated -> published,
-- published -> deprecated, any -> archived).

ALTER TABLE skills ADD COLUMN usage_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE skills ADD COLUMN success_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE skills ADD COLUMN failure_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE skills ADD COLUMN confidence_score REAL NOT NULL DEFAULT 0.0;
ALTER TABLE skills ADD COLUMN last_used_at TEXT;
ALTER TABLE skills ADD COLUMN parent_version TEXT;

CREATE INDEX IF NOT EXISTS idx_skills_usage ON skills (usage_count);
