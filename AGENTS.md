# AGENTS.md — Project Talos Standing Orders

> Read by every OpenCode session before any work. The owner performs zero
> engineering. The agent writes every file, runs every command, and verifies
> every result. These orders win over any prompt shortcut.

## 1. Read order (every prompt, in this order)

1. `AGENTS.md` (this file — standing orders)
2. `VISION.md` — product vision (ABSENT as of Prompt 001; use `README.md`
   Why Talos + Capability Matrix + Roadmap as the interim vision source)
3. `docs/DECISIONS.md` — decision log (ABSENT as of Prompt 001; use
   `docs/adr/0001–0012` as the interim decision record)
4. Latest `reports/` entry — most recent prompt truth baseline (ABSENT as of
   Prompt 001 at repo root; use `docs/reports/audit-latest.md` +
   `docs/evidence/batch-*-final-report.md` until `reports/001-*` lands)

Do not start engineering work until all four (or their interim equivalents
above) have been read. Record the absence in the prompt report until the
canonical files exist.

## 2. Invariants (never violate)

- The default-deny `policy_broker` is the ONLY authority for side effects.
  No bypass paths, no direct tool calls from UI or runtime.
- Every side effect writes an `evidence_store` receipt. Claims move
  `pending → verified / rejected / conflicting` only through guarded
  repository transitions.
- The execution fidelity ladder `mock / live / verified` is preserved.
  Never present `mock` as `verified`. Parent fidelity is the minimum of its
  children.
- Mock-first defaults stay default: live browser (`TALOS_BROWSER_LIVE=1`),
  real OS control (`real-os-control` feature), and real model calls
  (`--llm-real`) are explicit opt-ins only.
- Secrets live ONLY in the OS keychain (`talos_secrets`). Never in repo,
  config, telemetry, prompts, receipts, or logs.

## 3. Hard prohibitions

- NEVER access signing private keys or production credentials.
- NEVER implement consumer-subscription OAuth (Claude.ai / ChatGPT session
  tokens or equivalent credential-reuse flows).
- NEVER rename crates. Workspace members in `crates/Cargo.toml` are stable.
- NEVER force-push `main`. Short-lived branches only:
  `feat/*`, `fix/*`, `chore/*`, `docs/*`; squash-merge unless history must
  be preserved.
- NEVER commit secrets, `.env` contents, tokens, or machine-local paths.
- NEVER expose the gateway daemon beyond loopback. No `0.0.0.0` binds,
  no auth bypass, no remote execution without BYOC policy gating.

## 4. Work limits

- ~600-line diff target per prompt (merges exempt). Split larger work
  across prompts; say so in the report.
- Conventional Commits, scoped (`feat(...)`, `fix(...)`, `chore(...)`,
  `docs(...)`). Release commits: `chore(release): talos vX.Y.Z <codename>`.
- One prompt = one report file in `reports/NNN-slug.md` (template in the
  prompt; evidence-backed, commands with exit codes and durations).
- Every user-visible change updates `docs/PROGRESS.md` + `docs/EVIDENCE.md`
  with command, verbatim output, commit, and verification.
- `pnpm-lock.yaml` stays byte-identical unless the prompt explicitly
  authorizes a dependency change (`git diff` clean check before commit).
- `dist/` stays gitignored. Build it (`pnpm build`) before type-aware
  steps; never commit build output.

## 5. Quality gates (local = CI order)

Run in this order (matches `.github/workflows/ci.yml`):

```sh
pnpm install --frozen-lockfile
pnpm --filter @talos/contracts build && pnpm --filter @talos/browser-use build
pnpm lint && pnpm typecheck && pnpm test && pnpm build
pnpm audit && pnpm audit:swarm
cd crates && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Typescript runs on `ubuntu-latest` in CI; the repo ships `.gitattributes`
(`* text=auto eol=lf`, binaries excluded) so Windows checkouts stay LF and
`prettier --check` passes on every OS. Rust pins to `rust-toolchain.toml`
(1.99.1) locally and in every CI job — never float on `stable`. A missing
MSVC linker on a fresh Windows box is an ENVIRONMENT finding (install Build
Tools), not a product bug — record, do not "fix" product code.

## 6. Failure protocol

- A red gate on a clean machine is a FINDING first. Record the exact
  command, exit code, error text, and environment. Environment-level fixes
  only (missing OS dependency, wrong toolchain version).
- Never patch product logic to make a bootstrap/audit gate pass.
- If CI is red after at most 2 bounded fix attempts: revert to green,
  write `reports/NNN-<slug>-BLOCKED.md` with the evidence, and STOP.
  Do not accumulate speculative fixes.
- If the remote is empty/missing on clone: STOP. Do not `git init`.
  The owner decides.

## 7. Evidence discipline

- Commands cited with exit codes + wall-clock durations, never from memory.
- HEAD SHA, commit count, tags, branches, PR numbers + CI status captured
  from `git`/`gh`, never from the prompt text.
- Verbatim quotes for incident strings (e.g. drain_pool) with file path
  and line numbers.
- Final line of every prompt: commit SHA + CI run URL + green status.
  A prompt is not done until CI is green.
