# Report template (per-batch audit)

One report per prompt batch in `reports/NNN-slug.md`. Evidence-backed: commands
with exit codes and durations, never from memory (`AGENTS.md` §7).

## 1. Objective

What the prompt asked for (one paragraph).

## 2. Files created / modified

| File | Change | Lines |
| ---- | ------ | ----- |

Zero non-doc files changed: prove with `git status --porcelain` file list.

## 3. Verification commands + outputs

List every command run and its outcome (exit code + duration + key output).
Minimum for docs batches: `prettier --check`, `git status --porcelain`,
required check set per `docs/CI.md` (six ruleset checks plus
convention-required `awakening`, `hardening`, `stress-shutdown`) on PR and
post-merge `main`.

## 4. Deviations

Any place the prompt's assumptions differed from repo reality, and what was done.

## 5. UNKNOWNs

Ambiguities the architect must resolve. Never invent decisions.

## 6. Commit SHA + CI URLs

| Item                      | Value  |
| ------------------------- | ------ |
| Commit SHA                |        |
| PR URL                    |        |
| CI run (PR)               | status |
| CI run (main, post-merge) | status |
