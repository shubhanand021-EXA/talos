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

| Agent                       | What it checks                                                                                                                                                                                   |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| SecurityAuditAgent          | no `unsafe` Rust, least-privilege Tauri capability, CSP without remote hosts, no secrets, no denylisted deps; tool/browser/computer execution defaults safe, dangerous capabilities human-gated  |
| PrivacyAuditAgent           | no telemetry endpoints/SDKs, no non-allowlisted remote URLs, browser profile isolation documented                                                                                                |
| ReliabilityAuditAgent       | toolchain present, required scripts wired, lockfiles present, CI gates declared; agent-runtime lifecycle + cancellation suites executed; startup probe timed                                     |
| PerformanceAuditAgent       | dist exists + budget, main-JS gzip budget, void-neon lazy-chunk budget, heavy-dep watchlist; owns baseline regression gates                                                                      |
| ArchitectureAuditAgent      | monorepo layout, `@talos/contracts` present, no backend imports in UI, raw `invoke` only in `src/bridge`, policy centrality; tools via gated runtime, browser/computer via policy, RAG read-only |
| EvidenceIntegrityAuditAgent | chain verifiers + tamper tests + migrations present; tool receipts linked; compaction receipts carry proof hashes; executes the targeted cargo tests (warns when the toolchain is unavailable)   |
| DependencyHealthAuditAgent  | `pnpm audit` (high+), license allowlist via `pnpm licenses`, manifest license compatibility, lockfile duplicate scan                                                                             |
| UiUxAuditAgent              | routes wired, design-system complete, pages non-blank without lorem ipsum, critical actions reachable, emergency stop visible, async pages carry empty/loading/error states, a11y basics         |
| ProductAlignmentAuditAgent  | vision + feature keywords in README, living docs, Batch 2 objectives, Batch 1 artifacts, docs mention shipped systems, UI baseline snapshot in sync, no scope drift                              |

## Baselines (`.talos/baselines.json`)

Measured values with regression gates. Agents record deterministic metrics (`ctx.record`) while they run — bundle bytes, gzip estimates, cargo subset durations, startup probe time; the runner adds `swarm.totalDurationMs`. After all agents finish, the runner compares every collected metric against `.talos/baselines.json` and merges the verdicts into the performance agent (WARN past `warnAt`, FAIL past `failAt`; `referenceOnly` entries are recorded, never compared). The Markdown report gains a Baselines table, the console summary prints `metric=value` pairs, and CI re-checks critical regressions from the JSON report plus uploads the baselines file as an artifact. Missing/unparseable baselines → one warning, never a silent skip.

Update values with `pnpm audit:swarm` output (`metrics:` line): keep `value` near the measured number, `warnAt` ≈ 1.25× headroom (fail ≈ 1.6×) for sizes, generous wall-clock gates for durations.

## Configuration

`.talos/audit.config.json`: thresholds, allowed licenses, forbidden/telemetry patterns, required files/scripts/routes/components, heavy-dependency notes, forbidden top-level dirs, vision + feature keywords, cargo timeout.

## Add an agent

1. Create `src/agents/<name>.ts` exporting an `AuditAgent` (`name` + `run(ctx)` returning `Finding[]`).
2. Register it in `src/runner.ts` (`AGENTS`, fixed order).
3. Add hermetic fixture-based tests in `tests/` (`node:test`, no network/toolchain).
4. Document its checks in the table above.
