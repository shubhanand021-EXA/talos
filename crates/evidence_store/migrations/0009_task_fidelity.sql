-- Talos execution-fidelity tracking (Batch 4): every runtime task carries
-- its execution fidelity (`mock` | `live` | `verified`). Unset history
-- defaults to `mock` — nothing completed before this system existed may
-- claim verification it never had.

ALTER TABLE runtime_tasks ADD COLUMN fidelity TEXT NOT NULL DEFAULT 'mock';
