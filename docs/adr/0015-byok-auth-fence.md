# ADR 0015: BYOK auth fence (BYOK + Ollama only)

Date: 2026-11-12
Status: accepted

## Context

D6 locks BYOK + local models only with no consumer OAuth at launch and a
quarterly policy review. The in-tree gateway already implements it (ADR 0006):
Rust-first `crates/llm_gateway` with OpenAI / Anthropic / Ollama providers,
keys in `crates/talos_secrets` (OS keychain, service `com.talos.desktop`),
every call policy-gated with `usage_records` as the append-only spend source.
Provider policy is volatile: the governing prompt cites the Apr–May 2026
Anthropic ban-and-reversal timeline — interactive CLI use re-allowed while
headless/SDK use was carved out — as evidence that any single provider's terms
can shift under shipped product.

## Decision

1. **BYOK API keys + Ollama only.** No consumer OAuth at launch
   (hard prohibition, `AGENTS.md` §3); no subscription reuse is ever marketed
   (never imply the user's chat subscription covers API use).
2. **Key custody:** OS keychain via `talos_secrets`; values leave the vault
   only into an HTTP `Authorization` header — never IPC, events, evidence,
   logs, or error strings (ADR 0006).
3. **Quarterly policy review:** re-examine the provider landscape and this fence
   every quarter; changes land as ADR amendments, never silent drift.

## Accepted consequences

- OAuth scope risk and provider-terms coupling stay out of the launch surface.
- Cost transparency (VISION P9) builds on `usage_records` + per-task caps
  rather than provider billing APIs.
- If a future review admits new auth paths, it must update this ADR, D6, and
  the policy engine rule together.
