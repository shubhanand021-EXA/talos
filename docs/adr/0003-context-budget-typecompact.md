# ADR 0003: Context budgeting and TypeCompact foundation

Date: 2026-10-31
Status: accepted

## Context

Talos must feed large agent workflows to LLMs or local models without losing
safety constraints, task intent, evidence references, compensation plans, or
typed structure. Naive truncation is unacceptable: dropping a safety row or an
evidence link silently would break the policy/ledger guarantees Batch 1 and
Batch 2 established.

## Decision

**Rust-first deterministic engine** (`crates/context_budget`), contracts-first
wire shapes, generic IPC, dedicated UI page:

1. **Layered model** (`model.rs`): seven layers in keep order — safety,
   system, task, session, evidence, tool, scratchpad. Each layer carries a
   priority, a max budget, a compression policy, a discard policy, and a
   protected flag. Safety/system/task/session are protected and verbatim;
   evidence trims but never drops while it carries references; tool and
   scratchpad absorb overflow first.
2. **Local estimator, no external calls** (`estimator.rs`):
   `tokens = ceil(chars / 4)`, Unicode scalar count, `0` for empty text.
   Deterministic on every platform; the proof hash depends on it.
3. **Budget engine** (`budget.rs`): per-layer accounting, overflow detection
   (any layer over max, or total over budget), priority-ordered rebalancing
   that never touches protected layers.
4. **TypeCompact** (`compact.rs`): protected/safety-critical/safety-layer
   segments are verbatim; only reference-free segments on discardable layers
   may be dropped (lowest priority first, while over budget); compressible
   survivors dedupe repeated lines then trim the tail with an explicit
   `[…trimmed {n} chars…]` marker — never silent.
5. **Invariant gate** (`invariants.rs`): required fields, ID set equality,
   safety-body byte equality, evidence/compensation/policy/constraint/schema
   set equality. Any loss rejects the whole compaction with a structured
   `CompactionError` (`invariant_*` codes); nothing is persisted on failure.
6. **Deterministic builders** (`snapshot.rs`): task state + ledger rows +
   live safety summary map to the seven layers the same way every time.
7. **Runtime integration**: three desktop commands —
   `context_budget_status` (read-only), `context_compact` (compact + persist
   verified `context_compaction` evidence + emit `contextCompacted`),
   `context_diagnostic_snapshot` (diagnostic loop → snapshot with claim and
   plan refs → compact → persist → emit). Task detail reads the budget;
   the diagnostic flow produces a compacted snapshot; the ledger stores the
   receipt.
8. **UI** (`/context`): budget bars per layer, compacted preview, invariant
   checklist, compaction receipt panel, protected items list; feed entry on
   every `contextCompacted` event.

## Consequences

- Compaction is boring on purpose: verbatim fast path within budget,
  aggressive cuts only under overflow, invariants enforced by a separate
  module the compactor cannot skip.
- The estimator is a char fallback, not a tokenizer: conservative,
  explainable, and stable. A real tokenizer can replace it later behind the
  same function signature without changing any invariant.
- Human-approval surfacing for dropped High-risk content is out of scope:
  drops are restricted to reference-free segments, and anything with a
  reference is carried, so there is nothing to approve today.
