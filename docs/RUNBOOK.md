# Talos Runbook (Batch 2 — v0.2.0)

Single-machine operations. All commands run from the repo root (wherever you cloned it).

> Just recipes use kebab-case (`just demo-batch2`); the underlying pnpm scripts use colons (`pnpm demo:batch2`). Both are listed below.

## Setup

```sh
git clone <remote> talos && cd talos
just setup        # pnpm install + toolchain version check
just doctor       # environment readiness report
```

Requires: git, Node.js LTS (see `.nvmrc`), pnpm 10 (pinned via `packageManager`), Rust stable + cargo, [just](https://github.com/casey/just). On Windows, Rust builds also need MSVC Build Tools + Windows SDK (the `link.exe` linker); run from a "Developer PowerShell for VS" or set `LIB`/`INCLUDE` accordingly.

## Run the desktop app

```sh
just dev-desktop  # or: pnpm dev:desktop — Vite (:5173) + Tauri window (needs display + WebView2)
just dev-ui       # or: pnpm dev:ui — browser-only UI dev server (mock snapshot, disconnected banner)
pnpm build:desktop # production installer (needs WiX/NSIS on Windows, icons, signing identity)
```

## Run tests

```sh
just test              # root smoke + all workspace suites (contracts, UI, audit)
pnpm --filter ui test  # UI only (vitest)
pnpm --filter @talos/contracts test
pnpm --filter @talos/audit-swarm run audit  # same as: pnpm audit:swarm
cd crates && cargo test --workspace          # Rust (needs MSVC env on Windows)
cd apps/desktop/src-tauri && cargo test      # Tauri shell tests
```

## Run the end-to-end Batch 2 demo

```sh
just demo-batch2  # or: pnpm demo:batch2 — headless: Rust flow + topology tests + audit swarm
```

The demo submits a task, routes it through Tool-RAG, dry-runs browser/computer tools, compacts context, creates a compensation plan, verifies the ledger, and re-runs the swarm. Structured result lands in `docs/evidence/batch2-demo-latest.json`; exit code is non-zero on the first failing stage.

## Run the audit

```sh
just audit        # dep audit (high+) + 9-agent swarm; reports land in docs/reports/
pnpm audit:swarm  # swarm only; exit 1 on critical, 0 on warn/pass
```

Reports: `docs/reports/audit-latest.{md,json}` plus a dated copy per run. CI uploads them as the `talos-audit-reports` artifact.

## Diagnose common failures

| Symptom                             | Likely cause                       | Fix                                                                          |
| ----------------------------------- | ---------------------------------- | ---------------------------------------------------------------------------- |
| `link.exe not found` (cargo)        | MSVC env missing on Windows        | Run from Developer PowerShell, or set `LIB`/`INCLUDE` to BuildTools + SDK 10 |
| `pnpm audit:swarm` exits 1          | Critical finding — see report      | Open `docs/reports/audit-latest.md`, fix the `critical` rows, re-run         |
| Swarm `tamper-tests-pass` warning   | cargo unavailable in this shell    | Run with the MSVC env set (Windows) or in CI; presence checks still ran      |
| UI shows "disconnected" + mock data | Not running inside Tauri           | Expected in browsers; launch via `just dev-desktop` for the live loop        |
| `verifyEvidenceLedger` fails        | Backend unreachable                | Same as above — Evidence page falls back to the mock ledger                  |
| Viewport shows "unavailable"        | No WebGL (headless/VM)             | Expected fallback; agent data above remains fully usable                     |
| `pnpm licenses` warnings            | Missing native exe for pnpm itself | Re-run `pnpm install`; agent degrades to a warning with reason               |
| Prettier check fails on new files   | Formatting drift                   | Run `pnpm format` (writes in place), then re-run checks                      |
