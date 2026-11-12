# Onboarding (first-run wizard)

The wizard (`apps/ui/src/features/onboarding/`) triggers when the app
detects no completed onboarding — a fresh install with no database — and
walks a new user from install to first verified task in a few minutes.

## Steps

| #   | Step                | What happens                                                                                                                                                                                                                                      |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Welcome             | Talos vision: one policy, one ledger, every agent.                                                                                                                                                                                                |
| 2   | Provider setup      | Add one LLM provider key (OpenAI/Anthropic/Ollama). Key goes to the OS keychain (`storeProviderKey` — never displayed back); **Test connection** probes reachability; **Skip** stays fully local (add a key later in Settings → Model providers). |
| 3   | Workspace selection | Default workspace directory for per-task coding roots. Saved locally; task workspaces stay isolated regardless.                                                                                                                                   |
| 4   | Safety briefing     | The contract: policy broker is the only authority; humans gate high-risk actions (Plan/Act/Auto, approval queue, emergency stop); evidence ledger remembers everything; budgets deny before spend.                                                |
| 5   | First task          | Guided demo task: submit → policy evaluation → evidence → completion. Runs the mock diagnostic loop (`runDiagnostic`); nothing touches the system. Shows `passed` + check counts, or the failure inline.                                          |
| 6   | Complete            | Links (docs, `docs/RUNBOOK.md`), keyboard shortcut (`Ctrl/⌘ K`), Void Neon tour pointer, and the headless CLI pointer (`talos run`, `docs/CLI.md`). **Finish setup** persists completion.                                                         |

## State

- **Desktop:** `onboarding_completed` in shared `app_state`
  (`get_onboarding_state` / `set_onboarding_state` IPC) — the desktop and
  the headless CLI agree; `talos doctor` reports the flag.
- **Browser preview (no Tauri):** `localStorage`
  (`talos.onboarding.completed`), with the workspace dir in
  `talos.onboarding.workspaceDir`.
- **Replay:** Settings → "Replay onboarding" clears the flag and reloads
  into the wizard.

## Accessibility

- `role="dialog"` + `aria-modal`, labelled by the step heading; focus moves
  to the heading on every step change for screen-reader announcement.
- Progress list uses `aria-current="step"`; all controls are native buttons
  / inputs (full keyboard operability, visible focus via the design system).
- Decorative blur/transition is gated behind `motion-safe:` — `motion-reduce`
  disables it, and the backdrop blur is removed under reduced motion.

## Testing

`OnboardingWizard.test.tsx` (13 cases with `UpdateBanner.test.tsx`):
wizard renders all six steps in order, back-navigation, keychain save
(mock), connection test (mock), demo run pass + failure paths, completion
persistence, and screen-reader announcements.
