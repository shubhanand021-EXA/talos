# Talos Roadmap — Batch 1 (Prompts 01–10)

Batch 1 goal: green, auditable baseline. No product features.

- [x] **Prompt 01 — Bootstrap monorepo foundation** (this prompt)
  - Env validation, `talos/` skeleton, pnpm + TS + just + hygiene + docs + git init
  - Done when: `just setup`, `just doctor` green, repo clean, docs complete

- [ ] **Prompt 02 — Foundation tooling hardening**
  - ESLint, Prettier check in CI, editor settings, `contracts` package skeleton with typecheck

- [ ] **Prompt 03 — Desktop shell skeleton**
  - Tauri 2 + Vite + React in `apps/desktop`, `pnpm build` produces artifact

- [ ] **Prompt 04 — UI package baseline**
  - `apps/ui` component library skeleton, tokens, story or preview page

- [ ] **Prompt 05 — Contracts v0**
  - Shared TS types + validation in `packages/contracts`, consumed by desktop/ui

- [ ] **Prompt 06 — Rust workspace baseline**
  - `crates/` Cargo workspace, policy-broker stub, `cargo test` green

- [ ] **Prompt 07 — Shield baseline**
  - `pnpm audit`, `cargo audit/deny` (or documented equivalent), license check, secret scan in CI

- [ ] **Prompt 08 — Evidence ledger v0**
  - Machine-readable evidence format in `tools/`, `just audit` verifies ledger links

- [ ] **Prompt 09 — CI matrix**
  - OS matrix (win/mac/linux), Node + Rust caching, artifact upload, badge in README

- [ ] **Prompt 10 — Batch 1 gate**
  - Full `just setup && just lint && just typecheck && just test && just build && just audit` green on clean machine, docs frozen, tag `batch1-green`
