# Report 014 — Migration UI panel, bridge, docs, WCAG (Prompt 014)

- Date: 2026-11-14 (UTC)
- Agent: OpenCode (sole engineer)
- Batch: 2 / Prompt: 014 (WP-011B completion: migration Phase E surface)
- Read order followed: `AGENTS.md` → `VISION.md` (P5) → `reports/011-migration-kit.md` →
  `docs/CI.md` (+ bridge/store/design-system sources as code truth)
- Branch: `feat/migration-ui-prompt-014` (from `main` `fb0b0e7`)

## 0. Scope note (009/011/013 precedent)

~1102 insertions across 12 files — over the ~600 target. The whole diff is
additive UI surface + docs + tests; zero backend/product changes (the 011
core, its fence, and its IPC are byte-identical — verified by `git status`:
no `.rs` files touched). Splitting would half-build the panel the prompt
exists to ship, so the overage is documented, not split.

## 1. Bridge/panel inventory

| Layer  | Artifact                                                                                                                                                     | Status                    |
| ------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------- |
| Bridge | `migrationScan/Preview/Apply` in `api.ts` (contract-validated I/O) + re-exports in `index.ts`                                                                | new, lint/typecheck clean |
| Events | `MigrationScanCompleted/ImportCompleted` payload schemas + typed subscription                                                                                | new                       |
| Store  | `stores/migrationStore.ts` (zustand: scan/preview/apply/error/announcement + event subscriptions, backend-unavailable safe)                                  | new                       |
| Panel  | `features/migration/MigrationPanel.tsx` (scan → grouped findings → preview diff → per-category approve w/ confirm modal → result; live-region announcements) | new                       |
| Wiring | Settings → Migration kit (lazy section); onboarding Done-step entry; README checklist bullet + `docs/MIGRATION.md` link                                      | new                       |
| Docs   | `docs/MIGRATION.md` (what imports / NEVER imports / fence / re-enable flow / UI map)                                                                         | new                       |

## 2. Flow test results

`MigrationPanel.test.tsx` (4 tests, mocked bridge): scan renders grouped
findings; preview shows per-category diff; approve → confirm modal → apply
with exactly the approved categories; result shows created/skipped counts +
evidence ids.

- **No auto-enable (criterion 2):** `approvePlugin`, `validateSkill`,
  `publishSkill`, `toggleSchedule` mocked and asserted **never called**
  through the whole flow; every created item asserted
  `quarantined|disabled`. PASS.
- **Fence regression:** flagged fixture renders risk labels +
  secret-stripped badges; the never-enables notice renders. PASS.
- **Idempotent re-scan:** two scans → both dispatched, state refreshes, no
  stuck results. PASS.

## 3. a11y evidence (P5)

Panel built WCAG-consistent: native labeled inputs, `role=status/alert`
live regions for scan/preview/apply announcements, focus-managed confirm
dialog (existing Modal: autofocus + Escape), keyboard-operable checkboxes
and buttons, visible focus styles from the design system.
`a11y.test.tsx` (automated, no new deps — lockfile frozen, so axe is out;
"equivalent" per the prompt): every button has a real accessible name via
jest-dom's ARIA-algorithm matcher; every input is labelled; no positive
tabindex; results announced in an `aria-live="polite"` region; confirm
dialog is `aria-modal` with initial focus on Confirm and Escape dismissal;
key text pairs (slate-200/300, cyan-200, rose-300 on void-950) computed
≥4.5:1. 4/4 PASS.

## 4. Fence regression

Core untouched (`git status`: zero `.rs`/contracts changes). Fence
behavior re-verified at the surface: the flagged `prompt-injection`
finding and `secret-stripped` MCP entry render their risk labels and
`disabled`/`quarantined` badges; import of the approved category still
lands quarantined and the skipped category stays skipped. Rust fence
tests + contract tests unchanged and green in CI.

## 5. Docs delta

- `docs/MIGRATION.md` (new): mapping table, NEVER-imports list, fence,
  re-enable flow, UI map. Linked from README checklist + onboarding Done
  step. `prettier --write` clean.

## 6. Follow-up WPs

- None required by this prompt. Open threads it touches: WP-012C (device
  truth), WP-F3 (HEARTBEAT hot-reload — imported schedules need daemon
  restart; noted in MIGRATION.md re-enable flow).

## 7. Commit SHA + CI URLs + status

| Item   | Value                                                                                                    |
| ------ | -------------------------------------------------------------------------------------------------------- |
| Commit | `feat(migration): UI panel, bridge, docs, WCAG [Prompt-014]` (SHA recorded on push)                      |
| Branch | `feat/migration-ui-prompt-014` (from `fb0b0e7`)                                                          |
| CI     | PR run URL + status recorded before merge (attempt-1 red → fix → green counts as the 2 bounded attempts) |
| Status | **AWAITING CI** (local gates green; updated on result)                                                   |
