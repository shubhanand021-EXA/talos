# Pull Request

## Summary

<!-- What changes, why, and which Pentagon pillar(s). Keep scoped. -->

## Linked issue

<!-- Closes #<id> or Related to #<id>. -->

## Screenshots / GIFs

<!-- Required for UI/Void Neon/media changes. Attach before/after or link docs/media assets. -->

| Before | After |
| ------ | ----- |
|        |       |

## Tests

<!-- Exact commands + results. -->

- [ ] `pnpm typecheck`
- [ ] `pnpm lint`
- [ ] `pnpm test`
- [ ] `pnpm build`
- [ ] `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace`
- [ ] Desktop shell check (`cargo check --all-targets` in `apps/desktop/src-tauri`)
- [ ] New/updated tests for the change (list files):

```text
<paste verbatim key output>
```

## Audit impact

- [ ] `pnpm audit --audit-level high`
- [ ] `pnpm audit:swarm` (attach `docs/reports/audit-latest.md` summary; note warnings with reasons)
- [ ] New swarm coverage or thresholds (if any):

## Risk assessment

<!-- Blast radius, flags, migrations, platform caveats (Windows MSVC, WebView2, headless). -->

## Rollback / compensation notes

<!-- How to revert safely. For irreversible or ledger-touching changes, describe the compensation plan (proposed → approved → executed → rolled_back) or why none is needed. -->

## Docs

- [ ] `docs/PROGRESS.md` updated
- [ ] `docs/EVIDENCE.md` updated (command, output, commit, verification)
- [ ] `docs/ROADMAP.md` / `docs/ARCHITECTURE.md` / `docs/CI.md` updated if scope changed
- [ ] `CHANGELOG.md` Unreleased updated

## Checklist

- [ ] No secrets, tokens, or machine-local absolute paths
- [ ] `pnpm media:generate` is deterministic (`git status --porcelain` clean except intended files)
- [ ] Conventional Commit title (`feat|fix|chore|docs(<scope>): …`)
