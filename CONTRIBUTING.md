# Contributing to Talos

Thanks for helping build the all-in-one agent ecosystem. This is a private, local-first repo with public-quality bars: every claim needs evidence, every side effect passes policy, every push stays audit-green.

## Dev setup

Requires: `git >= 2.40`, Node.js LTS (see [.nvmrc](.nvmrc)), `pnpm >= 10` (pinned via `packageManager`), Rust stable + `cargo`, [just](https://github.com/casey/just) `>= 1.30`. On Windows, Rust builds need MSVC Build Tools + Windows SDK (`link.exe`); run from Developer PowerShell or set `LIB`/`INCLUDE`.

```sh
git clone https://github.com/shubhanand021-EXA/talos.git
cd talos
pnpm install          # or: just setup
pnpm typecheck
pnpm lint
pnpm test
pnpm build
pnpm media:generate   # verify brand assets regenerate deterministically
```

Useful:

```sh
pnpm dev:ui           # browser-only UI
pnpm dev:desktop      # Tauri window (needs display + WebView2)
pnpm audit:swarm      # 9-agent swarm only
cd crates && cargo test --workspace
```

Full matrix + headless caveats: [docs/RUNBOOK.md](docs/RUNBOOK.md). CI behavior: [docs/CI.md](docs/CI.md).

## Ground rules

- **Single source of truth**: shared types live in `@talos/contracts`. No layer redefines them — extend the contracts package instead.
- **Policy is central**: every side effect goes through `policy_broker`. No direct tool calls from UI or runtime.
- **Evidence first**: claims without linked evidence stay `pending`. Status changes use repository transition functions only.
- **Audit stays green**: `pnpm audit` + `pnpm audit:swarm` must pass (no criticals) before you push. Warnings need a reason in the PR or an evidence-log entry.

## Branch model

- Default branch: `main` (protected; PRs required when protection is enabled).
- Short-lived branches: `feat/<scope>-<slug>`, `fix/<slug>`, `chore/<slug>`, `docs/<slug>`.
- Keep branches rebased on `main`; squash-merge unless history must be preserved.
- Never force-push to `main`. Never commit secrets, `.env` contents, or machine-local paths.

## Commit convention

Conventional Commits, scoped:

- `feat(ui): …`, `feat(rust): …`, `feat(contracts): …`, `feat(evidence): …`, `feat(audit): …`, `feat(desktop): …`, `feat(media): …`, `feat(ci): …`
- `fix(<scope>): …`, `chore(repo): …`, `chore(release): …`, `docs(<scope>): …`

Each user-visible change updates `docs/PROGRESS.md` + `docs/EVIDENCE.md` (command, output, commit, verification). Release commits use `chore(release): talos vX.Y.Z <codename>`.

## Testing requirements

Minimum before push:

```sh
pnpm typecheck
pnpm lint
pnpm test
pnpm build
pnpm audit
pnpm audit:swarm
pnpm media:generate && git status --porcelain  # media/ must be deterministic
cd crates && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ../../apps/desktop/src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo check --all-targets
```

- TypeScript: strict, no `any`, Zod-validated boundaries, `pnpm format` before push.
- Rust: `cargo fmt`, clippy `-D warnings`, no `unwrap`/`expect` outside `#[cfg(test)]`.
- UI: `vitest run` suites for new routes/components; jsdom has no WebGL — test scene logic, not GL output.
- Contracts: additive schema changes only unless a migration is documented; keep `dist/` gitignored.

## Audit requirements

- `tools/audit` (`@talos/audit-swarm`) runs 9 agents locally with zero extra toolchain.
- Reports land in `docs/reports/audit-latest.{md,json}` plus dated copies; CI uploads them as `talos-audit-reports`.
- Criticals fail the command (exit 1); warnings do not (exit 0) but need triage notes.
- Adding an agent:
  1. Create `tools/audit/src/agents/<name>.ts` exporting `AuditAgent` (`name` + `run(ctx)`).
  2. Register it in `src/runner.ts` (fixed order).
  3. Add hermetic tests in `tools/audit/tests/` (tmpdir fixtures, no network/toolchain).
  4. Document it in `tools/audit/README.md` and `.talos/audit.config.json` if it needs thresholds.
  5. Triage every finding on the real repo before committing.

## PR checklist

Copy from [.github/PULL_REQUEST_TEMPLATE.md](.github/PULL_REQUEST_TEMPLATE.md):

- [ ] Summary + linked issue
- [ ] Screenshots/GIFs for UI changes
- [ ] Tests added/updated (`pnpm test`, `cargo test` where applicable)
- [ ] `pnpm typecheck` + `pnpm lint` + `pnpm build` green
- [ ] Audit impact (`pnpm audit` + `pnpm audit:swarm`, reports committed if changed)
- [ ] Risk assessment + rollback/compensation notes
- [ ] Docs updated (`PROGRESS.md`, `EVIDENCE.md`, `ROADMAP.md` if scope changed)
- [ ] No secrets, no absolute local paths, `git status --porcelain` clean except intended files
