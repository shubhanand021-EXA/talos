# `llm_gateway` — policy-gated BYOK model gateway

Single entry point (`LlmGateway`) for every model call: request validation,
budget enforcement, policy gating, execution with retry, and evidence +
usage recording. Raw prompts and completions are never persisted.

## Flow (`gateway.rs`)

1. `model::validate_request` + `TaskId`/`AgentId` linkage (no anonymous calls).
2. Resolve the adapter (`provider/model` prefix or capability match).
3. `BudgetTracker` checks the task cap from `usage_records` —
   over-budget denies with zero provider contact.
4. Record an `llm.request` evidence receipt (metadata + hashes, no content),
   then evaluate a `NetworkRequest` action (Rule 5e: allow with evidence).
5. `complete`/`stream` with exponential-backoff retry on transient failures.
6. Persist `usage_records` + `model_interactions` (hashes only), return the
   `ProviderReceipt` (usage, latency, estimated cost).

## Modules

- `model` — request/response/stream/receipt shapes (mirror `llm.ts`).
- `provider` — `LlmProvider` trait + structured `LlmError`.
- `providers/{openai,anthropic,ollama}` — chat adapters (SSE/NDJSON).
- `http` — injectable client (`ReqwestHttpClient`, `MockHttpClient`).
- `registry` — hardcoded `ModelDescriptor` catalog + cheapest-adequate `ModelRouter`.
- `budget` — ledger-backed spend checks.

## Providers

| Provider    | Models                                         | Key          | Streaming |
| ----------- | ---------------------------------------------- | ------------ | --------- |
| `openai`    | `gpt-4o`, `gpt-4o-mini`, `o3-mini`             | OS keychain  | SSE       |
| `anthropic` | `claude-sonnet-4-20250514`, `claude-3-5-haiku` | OS keychain  | SSE       |
| `ollama`    | dynamic via `/api/tags` (+ `llama*`, …)        | none (local) | NDJSON    |
