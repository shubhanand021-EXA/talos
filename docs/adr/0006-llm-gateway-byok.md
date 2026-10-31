# ADR 0006: LLM gateway with BYOK keychain and policy-gated routing

Date: 2026-11-01
Status: accepted

## Context

Talos needs real model calls without ever holding raw API keys in code,
config, telemetry, or git. Research converges on BYOK-by-default: the key
remains in the OS keychain (or enterprise secret manager), and requests go
directly to providers or through a minimal local adapter. A Talos-managed
gateway is a future opt-in, not this batch.

## Decision

Rust-first `crates/llm_gateway` behind the existing policy broker and
evidence ledger, with keys in `crates/talos_secrets` (OS keychain):

1. **BYOK keychain** (`talos_secrets`, service `com.talos.desktop`):
   `keyring` backend (Windows Credential Manager / macOS Keychain / Linux
   Secret Service) behind a `SecretBackend` trait; hermetic `MockBackend`
   for tests. Key naming `provider.{id}.api_key`. Values leave the vault
   only into an HTTP `Authorization` header — never into IPC, events,
   evidence, logs, or error strings (structured `SecretUnavailable`).
2. **Provider trait** (`LlmProvider`): `provider_id`, capability flags,
   `complete` + `stream`. Implementations: OpenAI (chat completions + SSE),
   Anthropic (messages API + SSE), Ollama (`/api/chat` + NDJSON, models via
   `/api/tags` discovery, no key). An injectable `HttpClient` keeps every
   adapter hermetically testable (`MockHttpClient`).
3. **Policy gating**: every call is an `ActionRequest` with
   `Capability::NetworkRequest`, evaluated with a recorded request receipt
   as evidence. New engine Rule 5e allows low/medium-risk model egress with
   evidence; high-risk non-reversible egress still escalates to human
   review; malformed linkage still denies. No anonymous calls: valid
   `task_id` + `agent_id` are mandatory.
4. **Budget enforcement**: `usage_records` is the append-only spend source;
   per-task caps (`runtime_tasks.max_spend_usd`, `None` = uncapped) are
   checked before any network traffic. Over-budget calls deny with
   `BudgetExceeded` and zero provider contact (test-asserted).
5. **Evidence privacy**: `usage_records` (counts, cost, latency) plus
   `model_interactions` (SHA-256 of canonical request/response _metadata_).
   Raw prompts and completions are never persisted.
6. **Reliability**: retry with exponential backoff for transient/timeout/
   rate-limit failures only; structured errors (`AuthFailure`,
   `RateLimited`, `Timeout`, `BudgetExceeded`, `PolicyDenied`,
   `SecretUnavailable`); SSE/NDJSON streaming parsed into deltas.
7. **Routing**: hardcoded reviewable catalog (`ModelDescriptor` + list
   prices) with a cheapest-adequate `ModelRouter` (tools/vision/rate/provider
   filters). Dynamic Ollama discovery merges at query time.
8. **Runtime**: `LlmAgent` uses the gateway when a key is configured and
   returns a deterministic mock fallback (with warning) otherwise, so the
   echo loop stays green without keys.

## Consequences

- No network path bypasses the broker: the gateway records the request
  receipt first, then evaluates, then executes — denials are ledgered.
- Costs are estimates from list prices; the ledger records them as such.
- A future Talos-managed gateway arrives as another `LlmProvider` behind
  the same trait, policy rule, and budget checks — no layer changes.
- Provider hosts (`api.openai.com`, `api.anthropic.com`) are documented
  audit boundaries (allowlisted with this rationale); RFC 2606 `.test` /
  `.invalid` joins `.example` as documentation-only TLDs in the scanner.
