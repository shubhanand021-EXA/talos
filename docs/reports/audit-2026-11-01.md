# Talos Audit Report

- Generated: 2026-11-01T17:15:00.488Z
- Root: `talos`
- Config version: 1
- **Status: WARN**

## Summary

- Agents: 10 (passed 8, warned 2, failed 0)
- Findings: 96 (critical 0, warnings 3, info 93)
- Measured metrics: 9

## Baselines

| Metric | Measured | Baseline | Unit | Gate |
| ------ | -------- | -------- | ---- | ---- |
| build.uiSeconds | — | 10 | seconds | reference |
| rust.agentRuntimeSubsetMs | — | 858 | ms | missing |
| rust.evidenceSubsetMs | 22721 | 1283 | ms | ok |
| startup.toolchainProbeMs | 2901 | 789 | ms | ok |
| swarm.totalDurationMs | 49834 | 4886 | ms | ok |
| telemetry.costDriftUsd | 0 | 0 | USD | reference |
| telemetry.instrumentationSites | 17 | 17 | count | reference |
| telemetry.suiteMs | 8968 | 5019 | ms | ok |
| tests.browserUse | — | 46 | count | reference |
| tests.contracts | — | 50 | count | reference |
| tests.desktop | — | 30 | count | reference |
| tests.root | — | 29 | count | reference |
| tests.rustWorkspace | — | 132 | count | reference |
| tests.swarm | — | 25 | count | reference |
| tests.ui | — | 89 | count | reference |
| ui.distBytes | 1729816 | 1554488 | bytes | ok |
| ui.mainJsGzipBytes | 178974 | 146685 | bytes | ok |
| ui.voidNeonChunkGzipBytes | 270225 | 270170 | bytes | ok |

Gates live in `.talos/baselines.json` (`baseline-ok` info, `baseline-regression` warning/critical).

## Failed checks & remediation

- [warning] PrivacyAuditAgent / no-remote-endpoints: Non-allowlisted https:// URL in code — confirm it is not a runtime call (apps/ui/src/features/approval/mockApprovals.ts:35: endpoint: "https://mail.internal/send",)
- [warning] PrivacyAuditAgent / no-remote-endpoints: Non-allowlisted https:// URL in code — confirm it is not a runtime call (crates/awakening_demo/src/main.rs:415: API_URL = "https://worldtimeapi.org/api/ip")
- [warning] DependencyHealthAuditAgent / duplicate-dependencies: 20 package(s) resolve to multiple versions (showing 10): aria-query(5.3.0,5.3.2); dom-accessibility-api(0.5.16,0.6.3); eslint-visitor-keys(3.4.3,5.0.1); hookified(1.15.1,2.2.0); ignore(5.3.2,7.0.14); lightningcss-android-arm64(1.32.0,1.33.0); lightningcss-darwin-arm64(1.32.0,1.33.0); lightningcss-darwin-x64(1.32.0,1.33.0); lightningcss-freebsd-x64(1.32.0,1.33.0); lightningcss-linux-arm-gnueabihf(1.32.0,1.33.0) (pnpm-lock.yaml)

### SecurityAuditAgent — PASS (4805ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | no-unsafe-rust | No `unsafe` blocks in crates/ | grep \bunsafe\b crates/ |
| info | tauri-least-privilege | Default capability grants zero extra permissions | capabilities/default.json: permissions: [] |
| info | csp-no-remote | Production CSP has no remote hosts (only self + ipc.localhost) | tauri.conf.json:app.security.csp |
| info | no-dangerous-deps | No denylisted dependencies in workspace manifests | package.json × 3 |
| info | tool-execution-defaults | process.exec + all computer.* tools disabled by default (live registry tests pass) | cargo test -p tool_runtime -- exec_disabled computer_tools_disabled |
| info | browser-live-disabled | Live Chromium gated behind TALOS_BROWSER_LIVE=1 with a disabled-by-default message | packages/browser-use/src/live.ts:117 |
| info | computer-real-disabled | Real OS control is feature-gated (default builds ship only the disabled stub) with env opt-in | crates/computer_use/src/lib.rs:23, crates/computer_use/src/lib.rs:29 |
| info | dangerous-capabilities-gated | High-risk actions escalate to human review (3 gate reference(s)) | crates/policy_broker/src/engine.rs:186, crates/policy_broker/src/engine.rs:695 |

### PrivacyAuditAgent — WARN (3669ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | no-telemetry | No telemetry/analytics patterns in code | 8 patterns scanned |
| warning | no-remote-endpoints | Non-allowlisted https:// URL in code — confirm it is not a runtime call | apps/ui/src/features/approval/mockApprovals.ts:35: endpoint: "https://mail.internal/send", |
| warning | no-remote-endpoints | Non-allowlisted https:// URL in code — confirm it is not a runtime call | crates/awakening_demo/src/main.rs:415: API_URL = "https://worldtimeapi.org/api/ip" |
| info | no-analytics-sdk | No analytics SDKs in workspace manifests | package.json, apps/ui/package.json, apps/desktop/package.json |
| info | browser-profile-isolation | Browser sessions stay profile-isolated (4 marker(s)) | packages/browser-use/src/live.ts:15, packages/browser-use/src/live.ts:125 |

### ReliabilityAuditAgent — PASS (5107ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | toolchain-present | node resolvable (v26.13.0) | node --version |
| info | toolchain-present | pnpm resolvable (10.34.7) | pnpm --version |
| info | toolchain-present | cargo resolvable (cargo 1.99.1 (5f94df478 2026-08-27)) | cargo --version |
| info | toolchain-present | rustc resolvable (rustc 1.99.1 (58cf8f9c3 2026-10-13)) | rustc --version |
| info | toolchain-present | just resolvable (just 1.59.0) | just --version |
| info | toolchain-present | git resolvable (git version 2.56.0.windows.4) | git --version |
| info | scripts-wired | Root script "setup" present | package.json:scripts.setup |
| info | scripts-wired | Root script "lint" present | package.json:scripts.lint |
| info | scripts-wired | Root script "format" present | package.json:scripts.format |
| info | scripts-wired | Root script "typecheck" present | package.json:scripts.typecheck |
| info | scripts-wired | Root script "test" present | package.json:scripts.test |
| info | scripts-wired | Root script "build" present | package.json:scripts.build |
| info | scripts-wired | Root script "audit" present | package.json:scripts.audit |
| info | scripts-wired | Root script "audit:swarm" present | package.json:scripts.audit:swarm |
| info | lockfiles-present | All workspace lockfiles present | pnpm-lock.yaml + 2 Cargo.lock |
| info | startup-probe-time | Toolchain readiness probe took 2901ms (baseline-gated) | node pnpm cargo rustc just git |
| info | runtime-lifecycle-tests | agent_runtime lifecycle/transitions suites pass (1245ms) | cargo test -p agent_runtime -- lifecycle transitions |
| info | cancellation-tests | agent_runtime cancel suites pass (957ms) | cargo test -p agent_runtime -- cancel |
| info | ci-gates | CI workflow covers typecheck, clippy, cargo test, and audit | .github/workflows/ci.yml |

### PerformanceAuditAgent — PASS (218ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | dist-present | UI production bundle present | apps/ui/dist/index.html |
| info | dist-budget | dist is 1729816 bytes (budget 6291456) | apps/ui/dist (1729816 B) |
| info | main-bundle-gzip | ProviderSettingsPanel-G7wyh2kR.js gzip ≈ 1494 B (budget 524288) | apps/ui/dist/assets/ProviderSettingsPanel-G7wyh2kR.js |
| info | main-bundle-gzip | StoragePanel-D-0QesTN.js gzip ≈ 1616 B (budget 524288) | apps/ui/dist/assets/StoragePanel-D-0QesTN.js |
| info | main-bundle-gzip | index-CNe8GoNJ.js gzip ≈ 178974 B (budget 524288) | apps/ui/dist/assets/index-CNe8GoNJ.js |
| info | void-neon-chunk-gzip | Lazy chunk void-neon-ZJ0090lV.js gzip ≈ 270225 B | apps/ui/dist/assets/void-neon-ZJ0090lV.js |
| info | heavy-deps | Heavy dependency "three" present — Void Neon 3D viewport (allowed: lazy-loaded chunk) | apps/ui/package.json |
| info | heavy-deps | Heavy dependency "@react-three/fiber" present — Void Neon 3D viewport (allowed: lazy-loaded chunk) | apps/ui/package.json |
| info | heavy-deps | Heavy dependency "@react-three/postprocessing" present — Void Neon bloom (allowed: quality-gated) | apps/ui/package.json |
| info | baselines-present | Baselines v1: 18 metrics with gates | .talos/baselines.json |
| info | baseline-ok | rust.evidenceSubsetMs = 22721 ms within budget (baseline 1283) | .talos/baselines.json:rust.evidenceSubsetMs |
| info | baseline-ok | startup.toolchainProbeMs = 2901 ms within budget (baseline 789) | .talos/baselines.json:startup.toolchainProbeMs |
| info | baseline-ok | swarm.totalDurationMs = 49834 ms within budget (baseline 4886) | .talos/baselines.json:swarm.totalDurationMs |
| info | baseline-ok | telemetry.suiteMs = 8968 ms within budget (baseline 5019) | .talos/baselines.json:telemetry.suiteMs |
| info | baseline-ok | ui.distBytes = 1729816 bytes within budget (baseline 1554488) | .talos/baselines.json:ui.distBytes |
| info | baseline-ok | ui.mainJsGzipBytes = 178974 bytes within budget (baseline 146685) | .talos/baselines.json:ui.mainJsGzipBytes |
| info | baseline-ok | ui.voidNeonChunkGzipBytes = 270225 bytes within budget (baseline 270170) | .talos/baselines.json:ui.voidNeonChunkGzipBytes |

### ArchitectureAuditAgent — PASS (434ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | monorepo-layout | All required monorepo directories present | apps/desktop, apps/ui, crates, crates/talos_common, crates/policy_broker, crates/evidence_store, crates/agent_runtime, packages/contracts, tools, tools/audit, docs |
| info | contracts-package | @talos/contracts present as single source of truth | packages/contracts/package.json |
| info | no-backend-in-ui | UI imports no Rust backend crates | apps/ui/src/**/*.{ts,tsx} |
| info | bridge-only-invoke | All Tauri calls flow through src/bridge | apps/ui/src/bridge/ |
| info | policy-centrality | Policy broker referenced 6× in the Tauri shell — central authority intact | apps/desktop/src-tauri/src/approval.rs |
| info | tools-through-runtime | All shell tool calls flow through the gated executor (4 call site(s)) | apps/desktop/src-tauri/src/commands.rs:1121, apps/desktop/src-tauri/src/commands.rs:2264 |
| info | browser-through-policy | Browser actions gated via OpenBrowser (2 executor arm(s)) | crates/tool_runtime/src/executor.rs:669, crates/tool_runtime/src/lib.rs:30 |
| info | computer-through-policy | Computer actions gated via UiAction safety envelope (12 executor arm(s)) | crates/tool_runtime/src/executor.rs:1380, crates/tool_runtime/src/executor.rs:1389 |
| info | rag-no-execution | Tool-RAG ranking is read-only — no execution path | crates/tool_rag/src |

### EvidenceIntegrityAuditAgent — PASS (22748ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | chain-verifier-exists | Hash-chain verifiers present (event_log + evidence chain) | crates/evidence_store/src/ledger.rs:183, crates/evidence_store/src/ledger.rs:223, crates/evidence_store/src/ledger.rs:265 |
| info | tamper-tests-exist | 6 tamper-related test reference(s) found | crates/evidence_store/src/ledger.rs:885, crates/evidence_store/src/ledger.rs:898, crates/evidence_store/src/ledger.rs:905, crates/evidence_store/src/ledger.rs:928 |
| info | tamper-tests-pass | Targeted evidence_store tests pass (tamper/chain/claim/compensation/integrity) | 
running 9 tests
.........
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 0.12s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

 |
| info | migrations-present | Evidence migrations 0001–0003 present | 0001_init.sql, 0002_ledger.sql, 0003_runtime.sql |
| info | tool-receipts-linked | Tool receipts persist as tool_receipt evidence with evidence_id linkage (28 reference(s)) | crates/tool_runtime/src/executor.rs |
| info | compaction-receipts-preserved | TypeCompact receipts persist as verified context_compaction evidence with sha256 proof hashes | apps/desktop/src-tauri/src/commands.rs:1592, apps/desktop/src-tauri/src/commands.rs:1780 |

### DependencyHealthAuditAgent — WARN (1811ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | vulnerability-scan | pnpm audit: no high-or-worse vulnerabilities | pnpm audit --audit-level high |
| info | license-allowlist | All dependency licenses within allowlist (15 SPDX ids) | pnpm licenses list |
| info | license-compatibility | All 6 workspace manifests declare allowlisted licenses | package.json, apps/ui/package.json, apps/desktop/package.json, packages/contracts/package.json, packages/browser-use/package.json, tools/audit/package.json |
| warning | duplicate-dependencies | 20 package(s) resolve to multiple versions (showing 10): aria-query(5.3.0,5.3.2); dom-accessibility-api(0.5.16,0.6.3); eslint-visitor-keys(3.4.3,5.0.1); hookified(1.15.1,2.2.0); ignore(5.3.2,7.0.14); lightningcss-android-arm64(1.32.0,1.33.0); lightningcss-darwin-arm64(1.32.0,1.33.0); lightningcss-darwin-x64(1.32.0,1.33.0); lightningcss-freebsd-x64(1.32.0,1.33.0); lightningcss-linux-arm-gnueabihf(1.32.0,1.33.0) | pnpm-lock.yaml |

### UiUxAuditAgent — PASS (474ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | routes-present | All required routes wired (/, /agents, /tools, /browser, /computer, /context, /evidence, /audit, /settings) | apps/ui/src/app/router.tsx |
| info | design-system-complete | All 13 design-system components present | apps/ui/src/design-system/ |
| info | pages-nonblank | 12 feature pages, all above minimum size | apps/ui/src/features/ |
| info | critical-actions-accessible | Dry run, emergency stop, compact, and Rank tools controls present | apps/ui/src/features/ |
| info | emergency-stop-visible | Emergency stop reachable (page control + shell pill present in 4 place(s)) | apps/ui/src/features/computer, apps/ui/src/app/chrome |
| info | empty-loading-states | All feature pages handle empty/loading/error states | apps/ui/src/features/ |
| info | a11y-basics | Design-system components carry a11y markers | apps/ui/src/design-system/ |

### ProductAlignmentAuditAgent — PASS (12ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | vision-readme | README carries the Talos vision keywords | README.md |
| info | docs-maintained | PROGRESS.md carries 32 dated entries | docs/PROGRESS.md |
| info | batch1-artifacts | All Batch 1 artifacts present | apps/desktop, apps/ui, crates/policy_broker, crates/evidence_store, crates/agent_runtime, packages/contracts, tools/audit |
| info | no-scope-drift | No out-of-scope top-level directories | apps/mobile, services, blockchain, wallet, apps/blockchain |
| info | readme-matches-features | README documents all tracked Batch 2 features | README.md |
| info | batch2-objectives | PROGRESS.md carries 32 dated entries covering Batch 2 | docs/PROGRESS.md |
| info | docs-updated | ARCHITECTURE.md covers Tool-RAG, TypeCompact, and Void Neon | docs/ARCHITECTURE.md |
| info | ui-baseline-snapshot-in-sync | Audit page reads the live report + baselines files | apps/ui/src/features/audit/AuditPage.tsx |

### TelemetryAuditAgent — PASS (10556ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | telemetry-crates | Workspace member "talos_telemetry" present | crates/Cargo.toml |
| info | telemetry-crates | Workspace member "cost_tracker" present | crates/Cargo.toml |
| info | spans-produced | 17 span recording site(s) across 7 crates | record_now call sites |
| info | span-naming | Span naming convention present (5 marker(s)) | crates/talos_telemetry/src/span.rs |
| info | span-coverage | All required span operations are recorded | policy_broker, evaluate, event_append, chain_verify, invoke, model_call, assemble, task_transition, browser_action, check_action |
| info | spans-privacy-safe | No forbidden attribute keys at span recording sites | prompt, params, output, screenshot, file_content, api_key, password, secret |
| info | cost-ledger | cost_records ledger migration present (task/agent attribution) | 0007_cost_records.sql |
| info | cost-matches-usage | cost_tracker aggregates usage_records (9 reference(s)) | crates/cost_tracker/src |
| info | budget-enforcement | 80% budget-warning threshold present | crates/cost_tracker/src/budget.rs:1, crates/cost_tracker/src/budget.rs:16 |
| info | budget-enforcement | Budget-deny paths present (15 marker(s)) | budget_exceeded / BudgetDecision::Deny |
| info | telemetry-tests | talos_telemetry + cost_tracker suites pass (8968ms) | cargo test -p talos_telemetry -p cost_tracker --lib |
| info | operations-dashboard | Operations dashboard route present (/operations) | apps/ui/src/app/router.tsx |
| info | telemetry-local-first | Telemetry stays local (file export only, no external endpoints) | crates/talos_telemetry/src |
