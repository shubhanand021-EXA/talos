# Talos Audit Report

- Generated: 2026-10-31T10:33:24.927Z
- Root: `D:\Private\talos`
- Config version: 1
- **Status: WARN**

## Summary

- Agents: 9 (passed 8, warned 1, failed 0)
- Findings: 48 (critical 0, warnings 1, info 47)

## Failed checks & remediation

- [warning] DependencyHealthAuditAgent / duplicate-dependencies: 20 package(s) resolve to multiple versions (showing 10): aria-query(5.3.0,5.3.2); dom-accessibility-api(0.5.16,0.6.3); eslint-visitor-keys(3.4.3,5.0.1); hookified(1.15.1,2.2.0); ignore(5.3.2,7.0.14); lightningcss-android-arm64(1.32.0,1.33.0); lightningcss-darwin-arm64(1.32.0,1.33.0); lightningcss-darwin-x64(1.32.0,1.33.0); lightningcss-freebsd-x64(1.32.0,1.33.0); lightningcss-linux-arm-gnueabihf(1.32.0,1.33.0) (pnpm-lock.yaml)

### SecurityAuditAgent — PASS (300ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | no-unsafe-rust | No `unsafe` blocks in crates/ | grep \bunsafe\b crates/ |
| info | tauri-least-privilege | Default capability grants zero extra permissions | capabilities/default.json: permissions: [] |
| info | csp-no-remote | Production CSP has no remote hosts (only self + ipc.localhost) | tauri.conf.json:app.security.csp |
| info | no-dangerous-deps | No denylisted dependencies in workspace manifests | package.json × 3 |

### PrivacyAuditAgent — PASS (170ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | no-telemetry | No telemetry/analytics patterns in code | 8 patterns scanned |
| info | no-remote-endpoints | No remote https:// endpoints in code | allowlist: schema.tauri.app, localhost, 127.0.0.1, github.com |
| info | no-analytics-sdk | No analytics SDKs in workspace manifests | package.json, apps/ui/package.json, apps/desktop/package.json |

### ReliabilityAuditAgent — PASS (3131ms)

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
| info | ci-gates | CI workflow covers typecheck, clippy, cargo test, and audit | .github/workflows/ci.yml |

### PerformanceAuditAgent — PASS (54ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | dist-present | UI production bundle present | apps/ui/dist/index.html |
| info | dist-budget | dist is 1497943 bytes (budget 6291456) | apps/ui/dist (1497943 B) |
| info | main-bundle-gzip | index-COYBVfyb.js gzip ≈ 137931 B (budget 524288) | apps/ui/dist/assets/index-COYBVfyb.js |
| info | heavy-deps | Heavy dependency "three" present — Void Neon 3D viewport (allowed: lazy-loaded chunk) | apps/ui/package.json |
| info | heavy-deps | Heavy dependency "@react-three/fiber" present — Void Neon 3D viewport (allowed: lazy-loaded chunk) | apps/ui/package.json |
| info | heavy-deps | Heavy dependency "@react-three/postprocessing" present — Void Neon bloom (allowed: quality-gated) | apps/ui/package.json |

### ArchitectureAuditAgent — PASS (81ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | monorepo-layout | All required monorepo directories present | apps/desktop, apps/ui, crates, crates/talos_common, crates/policy_broker, crates/evidence_store, crates/agent_runtime, packages/contracts, tools, tools/audit, docs |
| info | contracts-package | @talos/contracts present as single source of truth | packages/contracts/package.json |
| info | no-backend-in-ui | UI imports no Rust backend crates | apps/ui/src/**/*.{ts,tsx} |
| info | bridge-only-invoke | All Tauri calls flow through src/bridge | apps/ui/src/bridge/ |
| info | policy-centrality | Policy broker referenced 4× in the Tauri shell — central authority intact | apps/desktop/src-tauri/src/commands.rs |

### EvidenceIntegrityAuditAgent — PASS (1065ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | chain-verifier-exists | Hash-chain verifiers present (event_log + evidence chain) | crates/evidence_store/src/ledger.rs:124, crates/evidence_store/src/ledger.rs:166 |
| info | tamper-tests-exist | 4 tamper-related test reference(s) found | crates/evidence_store/src/ledger.rs:786, crates/evidence_store/src/ledger.rs:799, crates/evidence_store/src/ledger.rs:806, crates/evidence_store/src/ledger.rs:829 |
| info | tamper-tests-pass | Targeted evidence_store tests pass (tamper/chain/claim/compensation/integrity) | 
running 8 tests
........
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.11s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

 |

### DependencyHealthAuditAgent — WARN (2987ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | vulnerability-scan | pnpm audit: no high-or-worse vulnerabilities | pnpm audit --audit-level high |
| info | license-allowlist | All dependency licenses within allowlist (15 SPDX ids) | pnpm licenses list |
| warning | duplicate-dependencies | 20 package(s) resolve to multiple versions (showing 10): aria-query(5.3.0,5.3.2); dom-accessibility-api(0.5.16,0.6.3); eslint-visitor-keys(3.4.3,5.0.1); hookified(1.15.1,2.2.0); ignore(5.3.2,7.0.14); lightningcss-android-arm64(1.32.0,1.33.0); lightningcss-darwin-arm64(1.32.0,1.33.0); lightningcss-darwin-x64(1.32.0,1.33.0); lightningcss-freebsd-x64(1.32.0,1.33.0); lightningcss-linux-arm-gnueabihf(1.32.0,1.33.0) | pnpm-lock.yaml |

### UiUxAuditAgent — PASS (14ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | routes-present | All required routes wired (/, /agents, /evidence, /audit, /settings) | apps/ui/src/app/router.tsx |
| info | design-system-complete | All 13 design-system components present | apps/ui/src/design-system/ |
| info | pages-nonblank | 6 feature pages, all above minimum size | apps/ui/src/features/ |
| info | a11y-basics | Design-system components carry a11y markers | apps/ui/src/design-system/ |

### ProductAlignmentAuditAgent — PASS (5ms)

| Severity | Check | Message | Evidence |
| -------- | ----- | ------- | -------- |
| info | vision-readme | README carries the Talos vision keywords | README.md |
| info | docs-maintained | PROGRESS.md carries 12 dated entries | docs/PROGRESS.md |
| info | batch1-artifacts | All Batch 1 artifacts present | apps/desktop, apps/ui, crates/policy_broker, crates/evidence_store, crates/agent_runtime, packages/contracts, tools/audit |
| info | no-scope-drift | No out-of-scope top-level directories | apps/mobile, services, blockchain, wallet, apps/blockchain |
