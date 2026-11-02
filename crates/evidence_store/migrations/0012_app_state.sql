-- Shared key-value state for the distribution layer (Batch 4).
-- Small, auditable flags shared by the desktop shell and the headless CLI
-- through the same file-backed database: onboarding completion, update
-- channel preference, and CLI-managed provider/MCP registries.
-- Values are JSON strings (validated by callers, never executed).
CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
