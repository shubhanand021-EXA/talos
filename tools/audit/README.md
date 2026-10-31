# Talos Audit Swarm

Nine deterministic local audit agents with a unified report. TypeScript on plain Node (no build step, no extra toolchain): it runs anywhere Node runs and shares nothing with product bundles.

## Why TypeScript/Node (not Rust)

- Zero new toolchain: Node is already required; the swarm runs via `node src/run.ts` with native type stripping.
- Direct access to the JS workspace (package.json, pnpm lockfile, Vite output) without FFI.
- Deterministic file-based checks; anything needing a compiler (Rust tests) is invoked as a subprocess with a timeout and degrades to a warning when the toolchain is unavailable.

## Run it

```sh
pnpm audit:swarm        # from the repo root (writes docs/reports/)
just audit              # dep audit (high+) + swarm
node src/run.ts --root=/path/to/talos   # from tools/audit
TALOS_ROOT=/path/to/talos node src/run.ts
```

Reports: `docs/reports/audit-latest.md` + `.json`, plus a dated copy per run.

## Exit codes

| Status | Meaning                   | Exit |
| ------ | ------------------------- | ---- |
| PASS   | no warnings, no criticals | 0    |
| WARN   | warnings only             | 0    |
| FAIL   | ≥1 critical               | 1    |

## Agents

| Agent                       | What it checks                                                                                                              |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| SecurityAuditAgent          | no `unsafe` Rust, least-privilege Tauri capability, CSP without remote hosts, no secrets, no denylisted deps                |
| PrivacyAuditAgent           | no telemetry endpoints/SDKs, no non-allowlisted remote URLs in code                                                         |
| ReliabilityAuditAgent       | toolchain present, required scripts wired, lockfiles present, CI gates declared                                             |
| PerformanceAuditAgent       | dist exists + budget, main-JS gzip budget, heavy-dep watchlist                                                              |
| ArchitectureAuditAgent      | monorepo layout, `@talos/contracts` present, no backend imports in UI, raw `invoke` only in `src/bridge`, policy centrality |
| EvidenceIntegrityAuditAgent | chain verifiers + tamper tests present; executes the targeted cargo tests (warns when the toolchain is unavailable)         |
| DependencyHealthAuditAgent  | `pnpm audit` (high+), license allowlist via `pnpm licenses`, lockfile duplicate scan                                        |
| UiUxAuditAgent              | routes wired, design-system complete, pages non-blank without lorem ipsum, a11y basics                                      |
| ProductAlignmentAuditAgent  | vision keywords in README, living docs, Batch 1 artifacts, no scope drift                                                   |

## Configuration

`.talos/audit.config.json`: thresholds, allowed licenses, forbidden/telemetry patterns, required files/scripts/routes/components, heavy-dependency notes, forbidden top-level dirs, vision keywords, cargo timeout.

## Add an agent

1. Create `src/agents/<name>.ts` exporting an `AuditAgent` (`name` + `run(ctx)` returning `Finding[]`).
2. Register it in `src/runner.ts` (`AGENTS`, fixed order).
3. Add hermetic fixture-based tests in `tests/` (`node:test`, no network/toolchain).
4. Document its checks in the table above.
