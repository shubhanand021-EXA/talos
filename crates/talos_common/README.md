# talos_common

Shared Rust primitives for all Talos crates: `TalosError`/`Result`, typed IDs (`AgentId`, `TaskId`, `EvidenceId`, `CompensationPlanId`), UTC timestamp helpers, JSON helpers, and a shared `tracing` bootstrap.

No layer may redefine these primitives.
