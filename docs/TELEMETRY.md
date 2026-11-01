# Talos Observability: tracing, cost tracking, and operations

Enterprise-grade observability with a local-first privacy rule: **no trace
or cost data ever leaves the machine** unless the operator explicitly
exports a JSON file.

## Tracing (`crates/talos_telemetry`)

OpenTelemetry-compatible spans without external OTel crates (see
ADR 0009 for why):

- **Naming:** `talos.{component}.{operation}` — e.g.
  `talos.policy_broker.evaluate`, `talos.llm_gateway.model_call`,
  `talos.tool_runtime.invoke`, `talos.ipc.get_cost_breakdown`.
- **Instrumentation points:** policy evaluation (task, capability, risk,
  decision) · event append · chain verification · task lifecycle
  transitions · tool invocation (tool id, mode, duration, status) ·
  browser-worker actions · model calls (provider, model, tokens, latency,
  cost) · context assembly (duration, layers, compactions) ·
  computer-use safety checks (confidence, verification result) · IPC
  commands (name, duration, success/error).
- **Attributes (privacy-safe only):** `talos.task_id`, `talos.agent_id`,
  `talos.component`, `talos.operation`, durations, status, plus small
  numerics (`tokens.*`, `cost.usd`, `latency_ms`, `count.*`,
  `policy.*`, `tool.*`, `model.*`, `browser.*`, `computer.*`,
  `ipc.command`, `chain.*`, `error.code`). Never: API keys, prompt
  content, file contents, personal data, screenshots.
- **Propagation:** W3C `traceparent`-compatible `{trace_id, span_id}`
  (`TraceContext::child`, `Traceparent::encode/decode`) across IPC.
- **Collector:** bounded in-memory ring (5,000 spans), 7-day retention
  (`retain_old`), process-wide global installed by the desktop shell;
  `record_now` is a no-op when unset (zero overhead elsewhere).
- **Export:** `export_telemetry` writes newline-delimited OTLP-shaped JSON
  to a local file; old exports auto-purged past 7 days.

## Cost tracking (`crates/cost_tracker` + `cost_records`)

Real-time calculation, not estimates:

| Resource     | Formula                                    |
| ------------ | ------------------------------------------ |
| LLM calls    | tokens × catalog price (`ModelDescriptor`) |
| Tool exec    | duration × $0.0000001/ms (~$0.36/h)        |
| Browser      | duration × $0.0000002/ms (~$0.72/h)        |
| Computer-use | actions × $0.0005/action                   |

Attribution per task / agent / model / tool over `usage_records` (LLM) +
`cost_records` (everything else), surfaced by `get_cost_breakdown`.

Budget enforcement:

- **Task cap** (`runtime_tasks.max_spend_usd`): live tool actions return a
  `Denied/budget_exceeded` receipt; the gateway returns `BudgetExceeded`.
  Dry-runs are always free.
- **Session cap** (`set_session_budget`, default uncapped): IPC layer
  denies new tool invocations and model calls once exceeded.
- **Warnings** at 80% of either cap (`llmBudgetWarning` events for tasks;
  amber budget meter on the dashboard for sessions).

## Operational metrics (`get_operational_metrics`)

Task success/failure rates with top failure reasons (`last_error`
attribution) · avg task duration · model latency p50/p95/p99 (overall +
per model) · per-tool success rates · policy denial rate · chain
verification time (measured live) · session tokens and cost.

Known gap: approval (HITL) wait time is not yet reported — approvals live
in the in-memory `ApprovalStore` (`crates/agent_runtime`), so no durable
wait-time distribution exists. Approval lifecycle events
(`approvalRequested/Granted/Denied/Expired`) already flow; durable
wait-time percentiles arrive when approvals persist to the ledger.

## Operations dashboard (`/operations`)

Cost-by-task/model/tool bars · success and denial cards · p95 latency ·
per-model latency table · tool health table · recent traces (metadata
only) · session budget meter + setter · one-click local trace export.
All data is local; the page shows a browser-preview notice outside Tauri.

## Audit coverage (10th agent)

`TelemetryAuditAgent` verifies: workspace membership · 17 recording sites
across 7 crates · naming convention · operation coverage · no forbidden
attribute keys at recording sites · `cost_records` migration · cost/usage
reconciliation · 80% warn + deny paths · telemetry suites green ·
`/operations` route · no external endpoints. Baseline gates:
`telemetry.suiteMs` (warn 30s, fail 120s), `telemetry.costDriftUsd`
(reference 0), `telemetry.instrumentationSites` (reference 17).

## Retention and privacy

- Traces retained 7 days (configurable via `CollectorConfig`), auto-purged.
- Export requires explicit user action; nothing is transmitted.
- Redaction runs twice: at record time and defensively at export.
