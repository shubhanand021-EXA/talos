-- Talos multi-agent delegation: parent-child task hierarchy.
-- Batch 3: Supervisor spawns specialized sub-agents with isolated scopes.
-- Children reference their parent; roots have NULL parent_task_id.

ALTER TABLE runtime_tasks ADD COLUMN parent_task_id TEXT REFERENCES runtime_tasks(id);

CREATE INDEX IF NOT EXISTS idx_runtime_tasks_parent ON runtime_tasks (parent_task_id);
