# `context_assembler` — budgeted prompt construction

Builds the optimal prompt for each model call from every Talos system:
task contract, verbatim safety rules, evidence ledger, Tool-RAG ranking,
session history, memory facts, and scratchpad notes.

## Flow (`assembly::assemble`)

1. Validate input (model, window, task linkage, allocation sums to 100).
2. Budget = model window minus the reserved output headroom (default 15%),
   split 20/25/35/10/10 across contracts / tool-state / evidence / memory / plan.
3. Count each layer with the model-appropriate estimator
   (`estimator`: tiktoken `cl100k` for OpenAI-family, char/4 fallback otherwise;
   the active estimator is stamped on every assembly).
4. Overflow in a non-safety layer triggers isolated TypeCompact on that layer
   only. Safety is never compacted; invariant failure falls back to
   deterministic truncation and marks the assembly degraded.
5. Render the versioned template (`ctx-assembler/1`), hash the canonical
   messages (`sha256:`), and return counts, tool/evidence refs, and
   per-layer targets — never raw content beyond the messages themselves.

Over the hard model limit after all compaction → `HardLimitExceeded`: the
caller fails the task gracefully and emits `contextBudgetExceeded`.

## Privacy

`assembly_evidence_payload` records structure, counts, hashes, and receipts
only. Prompt text never lands in evidence rows.
