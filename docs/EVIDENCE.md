# Talos Evidence Log

Every claim in PROGRESS.md must link to an entry here. Format per entry:

```text
## <date> — <title>
- Command: <exact command>
- Output: <key output lines, verbatim>
- Commit: <message + hash from `git log -1`>
- Verification: <how to reproduce / what was checked>
- Notes: <limitations, if any>
```

---

## 2026-10-31 — Prompt 07 runtime bridge

- Commands:
  - `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all` (crates)
  - `cargo check/clippy/test` in `src-tauri` (MSVC env); `tauri info` unchanged
  - `pnpm --filter @talos/contracts build` (rebuild dist for `requiredChecks`), `pnpm --filter ui test|typecheck|lint|build`
- Outputs (verbatim, key lines):
  - `cargo test`: workspace `4+4+7+5 = 20 passed`; desktop `7 passed` (incl. orchestrator-backed diagnostic + contracts-shape serialization)
  - `vitest`: `Test Files 8 passed (8)` / `Tests 35 passed (35)` (bridge 9 + sync 2 new)
  - `vite build`: main + lazy 3D chunk; `pnpm audit --audit-level high`: exit 0
- Commit: `feat(runtime): add local orchestration bridge and echo agent loop` (see `git log -1 --oneline`)
- Verification:
  - `grep \.(unwrap|expect)\(` in new Rust code hits only `#[cfg(test)]`
  - Rust `PolicyEvaluation` → IPC DTO maps field-for-field to TS schema (test asserts all 7 keys; `requiredChecks` added to contracts as optional)
  - Event flow: `run_diagnostic` → 6 ordered emissions (`taskCreated`, `agentSpawned`, `taskUpdated`, `policyEvaluated`, `evidenceUpdated`, `taskUpdated`) asserted in test
- Notes (manual verification, GUI):
  - `pnpm dev:desktop` → Dashboard → **Run Diagnostic** → button loads, activity feed + inspector + viewport node update, status bar counts go 0 → 1.
  - Browser preview stays honest: banner + `disconnected` pills, diagnostic fails closed with reason.
  - No LLMs, no network, no cloud: echo agent proposes a fixed low-risk read; reruns differ only in counter/timestamps.

## 2026-10-31 — Prompt 06 void-neon viewport

- Commands:
  - `pnpm add three @react-three/fiber @react-three/postprocessing` + `pnpm add -D @types/three`
  - `pnpm --filter ui test|typecheck|lint|build`; repo-wide gates
- Outputs (verbatim, key lines):
  - `vitest`: `Test Files 6 passed (6)` / `Tests 24 passed (24)`
  - `vite build`: main `430196` B + lazy `void-neon-*.js 1011526` B, `built in ~0.6s`
  - `eslint . --max-warnings=0`: exit 0 (fixed set-state-in-effect, void-expression, refresh boundaries)
  - `pnpm audit --audit-level high`: exit 0
- Commit: `feat(ui): add void neon 3d baseline viewport` (see `git log -1 --oneline`)
- Verification:
  - jsdom has no WebGL: `isWebGLAvailable()` false → fallback panel test passes; scene logic (topology, colors, settings) unit-tested without GL
  - No `Math.random` in scene (seeded LCG); no network textures/models; `dist/` gitignored
  - Selection writes `uiStore.selectedEntity` (inspector contract); reduced-motion + hidden-tab pauses verified by code path
- Notes (visual limitation):
  - Headless environment: no GPU browser capture available. Fidelity verified by build + tests + code review; interactive check via `pnpm --filter ui dev` on a GPU machine.
  - drei intentionally omitted (hand-rolled lines/particles/grid); bloom via `@react-three/postprocessing`, disabled on `low` quality and reduced-motion.

## 2026-10-31 — Prompt 05 desktop shell

- Commands:
  - `pnpm exec tauri --version` → `tauri-cli 2.12.4`; `tauri info` (env + config report)
  - `cargo check --all-targets` / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --check` / `cargo test` (in `src-tauri`, MSVC env)
  - `pnpm --filter ui test` / `build`; node config assertion script (identifier, window, CSP, capabilities, icons)
- Outputs (verbatim, key lines):
  - `cargo test`: `6 passed; 0 failed` (state, commands, events suites)
  - `cargo clippy`: exit 0, no warnings (fixed `io::Error::other`, `Emitter`/`Manager` imports)
  - `tauri info`: `WebView2: 156.0.4314.50`, `MSVC: Visual Studio Build Tools 2022`, `frontendDist: ../ui/dist`, `devUrl: http://localhost:5173/`
  - `tauri config: OK (5 icons verified)`; `vite build`: `153 modules transformed`
- Commit: `feat(desktop): add tauri shell and secure ipc foundation` (see `git log -1 --oneline`)
- Verification:
  - `grep \.(unwrap|expect)\(` in `src-tauri/src` hits only `#[cfg(test)]` (justified)
  - CSP allows no `https://` and only `http://ipc.localhost`; capabilities `permissions: []`
  - UI switched `createBrowserRouter` → `createHashRouter` (Tauri asset protocol has no history fallback)
- Notes (GUI limitation):
  - Headless environment: `tauri dev` / `tauri build` GUI steps NOT run (they open windows / need installer toolchains).
  - Manual run: `pnpm dev:desktop` (needs display + WebView2); bundle: `pnpm build:desktop` (needs WiX/NSIS on Windows).
  - Icons are generated placeholders; replace before release signing. `@tauri-apps/api` not yet installed in UI (no frontend invoke calls in Batch 1).

## 2026-10-31 — Prompt 04 UI shell

- Commands:
  - `pnpm --filter ui test` / `typecheck` / `lint` / `build`
  - Repo-wide `pnpm typecheck|lint|test|build`, `pnpm audit --audit-level high`
- Outputs (verbatim, key lines):
  - `vitest`: `Test Files 4 passed (4)` / `Tests 15 passed (15)`
  - `vite build`: `153 modules transformed`, `dist/index.html 0.49 kB`, `dist/assets/index-*.js 427.74 kB`, `built in 658ms`
  - `eslint . --max-warnings=0`: exit 0
  - `pnpm audit --audit-level high`: exit 0
- Commit: `feat(ui): add react app shell and design system foundation` (see `git log -1 --oneline`)
- Verification:
  - 5 routes render non-blank design-system states; navigation covered by test
  - `dist/` gitignored; no lorem ipsum; no dead routes/buttons
  - Stores typed via `@talos/contracts` (`workspace:*`); no untyped state
- Notes:
  - `pnpm audit` (default level) reports 1 moderate transitive advisory: `minimatch <11.0.1` via `eslint` (GHSA-g3hv-75wg-m249). Below the `high` gate; upstream fix pending, no action taken.
  - Screenshots: headless environment — no browser capture taken. Visual check via `pnpm --filter ui dev` (:5173) recommended on Prompt 05.

## 2026-10-31 — Prompt 03 Rust workspace

- Commands:
  - `winget install -e --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --quiet --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` (provides `link.exe`; clean machine had no C linker)
  - `cargo fmt --all` / `cargo fmt --all -- --check`
  - `cargo check --workspace --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `cargo run -p policy_broker --bin talos-policy-broker -- --selftest`
- Outputs (verbatim, key lines):
  - `clippy`: `Finished dev profile`, no warnings, exit 0
  - `cargo test`: `evidence_store 4 passed`, `policy_broker 7 passed`, `talos_common 5 passed`, exit 0
  - `selftest`: `talos-policy-broker selftest: 6/6 passed` (sqlite round-trip, low-risk allow, write→evidence, irreversible→compensation, malformed→deny, unknown→deny), exit 0
  - `cargo fmt --check`: exit 0
- Commit: `feat(rust): add workspace and policy broker core` (see `git log -1 --oneline`)
- Verification:
  - `grep \.(unwrap|expect)\(` hits only `#[cfg(test)]` modules (justified test use)
  - `sqlx::migrate!` runs `migrations/0001_init.sql` (events, evidence_records, claims)
  - Dependency graph acyclic: `policy_broker` → `evidence_store` → `talos_common`
  - No network calls, no hardcoded secrets
- Notes:
  - Linker env (MSVC 14.44 + SDK 10.0.26100.0) set per-shell; consider `rustup`/CI images with preinstalled toolchain later.
  - `crates/Cargo.lock` is now committed (workspace ships a binary); `.gitignore` updated.

## 2026-10-31 — Prompt 02 contracts

- Commands:
  - `pnpm add zod` / `pnpm add -D tsup vitest typescript prettier` (in `packages/contracts`)
  - `pnpm --filter @talos/contracts typecheck` / `test` / `build` / `lint`
  - `pnpm typecheck` / `pnpm lint` / `pnpm test` / `pnpm build` / `pnpm audit --audit-level high` (repo root)
  - `just lint` / `just typecheck` / `just test` / `just build` / `just audit` / `just doctor`
- Outputs (verbatim, key lines):
  - `vitest`: `Test Files 4 passed (4)` / `Tests 27 passed (27)`
  - `tsup`: `ESM dist/index.js 9.13 KB` + `DTS dist/index.d.ts 17.42 KB`, `Build success`
  - `pnpm audit`: `No known vulnerabilities found`
  - `pnpm lint`: `All matched files use Prettier code style!`
- Commit: `feat(contracts): add shared talos contracts package` (see `git log -1 --oneline`)
- Verification:
  - `grep \bany\b src/*.ts` matches only the test title "no any leakage" (a string literal, not a type)
  - `dist/index.js` + `dist/index.d.ts` emitted; `dist/` gitignored
  - Dependency graph acyclic: domains import only `identity.js` + `zod`
- Notes:
  - Zod 4.6.7 (`z.iso.datetime()` for ISO dates, `.strict()` objects, `.brand()` IDs, `z.unknown()` payloads).
  - Last line of Prompt 01 notes is now superseded: contracts placeholder replaced by full schemas.

## 2026-10-31 — Prompt 01 bootstrap

- Commands:
  - `winget install -e --id Git.Git`
  - `winget install -e --id OpenJS.NodeJS.LTS`
  - `winget install -e --id Rustlang.Rustup`
  - `winget install -e --id Casey.Just`
  - `winget install -e --id pnpm.pnpm`
  - `pnpm install`
  - `pnpm typecheck` / `pnpm lint` / `pnpm test` / `pnpm build` / `pnpm audit --audit-level high`
  - `just setup` / `just doctor` / `just lint` / `just typecheck` / `just test` / `just build` / `just audit`
- Outputs (verbatim, key lines):
  - `git version 2.56.0.windows.4`
  - `v26.13.0` (node)
  - `10.34.7` (pnpm, pinned in `packageManager`)
  - `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
  - `cargo 1.99.1 (5f94df478 2026-08-27)`
  - `just 1.59.0`
  - `pnpm install`: `devDependencies: + prettier 3.9.10 + typescript 5.9.3`
  - `pnpm lint`: `All matched files use Prettier code style!`
  - `pnpm test`: `pass 4, fail 0`
  - `pnpm audit`: `No known vulnerabilities found`
  - `just doctor`: `doctor: ok`
- Commit: `chore: bootstrap talos monorepo foundation` (see `git log -1 --oneline`)
- Verification:
  - `git status --porcelain` clean after commit
  - `just doctor` prints git/node/pnpm/rustc/cargo/just versions
  - `README.md` links to `docs/*.md`, `LICENSE`, `.nvmrc` all resolve; no dead links
- Notes:
  - Clean Windows 11 machine; all toolchains installed via official winget packages.
  - `pnpm --version` resolves to npm-global 10.34.7 (winget ships 12.16.1); pinned to active 10.34.7.
  - `packages/contracts` is a minimal Batch 1 placeholder (real schemas in Prompt 05) so `tsc` has real inputs.
