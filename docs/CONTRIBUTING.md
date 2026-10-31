# Contributing to Talos (Batch 1)

## Ground rules

- **Single source of truth**: shared types live in `@talos/contracts`. No layer redefines them — extend the contracts package instead.
- **Policy is central**: every side effect goes through `policy_broker`. No direct tool calls from UI or runtime.
- **Evidence first**: claims without linked evidence stay `pending`. Status changes use repository transition functions only.
- **Audit stays green**: `just audit` must pass (no criticals) before you push. Warnings need a reason in the PR or an evidence-log entry.

## Workflow

```sh
just setup
# ... make changes ...
just lint && just typecheck && just test && just build
just audit   # dep audit + 9-agent swarm
```

Keep commits scoped (`feat(ui): …`, `feat(rust): …`, `feat(contracts): …`, `feat(evidence): …`, `feat(audit): …`, `chore(release): …`) and update `docs/PROGRESS.md` + `docs/EVIDENCE.md` with each user-visible change.

## Adding an audit agent

1. Create `tools/audit/src/agents/<name>.ts` exporting an `AuditAgent` (`name` + `run(ctx)` returning findings).
2. Register it in `src/runner.ts` (`AGENTS`, fixed order).
3. Add hermetic tests in `tools/audit/tests/` (tmpdir fixtures, no network/toolchain).
4. Document it in `tools/audit/README.md` and `.talos/audit.config.json` if it needs thresholds.
5. Triage every finding it produces on the real repo before committing — fix the agent or the repo, never silence real signal without a documented reason.

## Code style

- TypeScript: strict, no `any`, Zod-validated boundaries, `pnpm format` before push.
- Rust: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, no `unwrap`/`expect` outside `#[cfg(test)]`.
