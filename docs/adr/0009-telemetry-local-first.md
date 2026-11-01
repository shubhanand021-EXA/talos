# ADR 0009: Local-first OpenTelemetry-compatible observability

Date: 2026-11-01
Status: accepted

## Context

Batch 3 research converges on OpenTelemetry tracing plus real cost
tracking (Perplexity §2.7, ChatGPT §2.3, Manus §3.1), but Talos is
privacy-first and offline-capable: no external telemetry endpoint may
ever be contacted by default, and the build must not require network
access for new dependencies.

## Decision

1. **No `opentelemetry` crates in the default build.** `crates/talos_telemetry`
   implements the OTel surface Talos needs — `talos.{component}.{operation}`
   span names, W3C `traceparent` context propagation, OTLP-shaped JSON —
   with zero new external dependencies. A future OTLP/HTTP bridge can
   translate the JSON export field-for-field; it stays disabled unless an
   operator explicitly opts in.
2. **Spans at every choke point** via thin wrappers (behavior-preserving):
   policy evaluation, event append, chain verification, tool invocation,
   browser-worker calls, model calls, context assembly, task transitions,
   computer-use safety checks, and IPC commands. Recording is a no-op
   until the desktop shell installs the global collector.
3. **Privacy by construction:** allowlisted attribute keys (`talos.*` plus
   small numerics), secret-pattern redaction before export, 512-char
   truncation. The audit swarm's Telemetry agent fails the run on any
   forbidden attribute key at a recording site.
4. **Real cost tracking** (`crates/cost_tracker` + `cost_records` ledger):
   LLM tokens × catalog price, tools × compute rate, browser × browser
   rate, computer-use × per-action rate. Attribution by task/agent/model/
   tool. Task caps deny (tool receipts go `Denied/budget_exceeded`; the
   gateway returns `BudgetExceeded`); session caps deny at the IPC layer;
   warnings fire at 80%.
5. **Local dashboard only** (`/operations` page, 5 IPC commands). Export
   requires explicit user action; 7-day retention with auto-cleanup.

## Consequences

- Telemetry overhead is bounded and tested: 1,000 span records complete
  in microseconds-to-milliseconds against millisecond-scale operations
  (`thousand_spans_stay_far_below_operation_cost`), gated by
  `telemetry.suiteMs` in `.talos/baselines.json`.
- Cost math is exact to 1e-9/1e-12 by unit test; the swarm records
  `telemetry.costDriftUsd = 0` while those suites pass.
