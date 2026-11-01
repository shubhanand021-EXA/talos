# Talos UI — command-center shell

Strict React + Tailwind application shell for Talos. Desktop-grade layout, Void Neon design tokens, Zustand stores, and React Router pages. Live backend wiring arrives in later batches; Batch 1 renders a deterministic mock snapshot.

## Commands

```sh
pnpm --filter ui dev       # local dev server (:5173)
pnpm --filter ui test      # vitest run
pnpm --filter ui build     # production build to dist/
pnpm --filter ui typecheck # strict tsc
pnpm --filter ui lint      # eslint, zero warnings
```

## Structure

```text
src/
  app/            # App, router, Shell + chrome (TopBar, LeftRail, RightInspector, BottomStatusBar)
  components/     # (reserved) feature-local components
  design-system/  # GlassPanel, NeonButton, StatusPill, SectionHeader, EmptyState,
                  # KeyValueList, LogStream, DataTable, Spinner, ErrorBoundary,
                  # Tooltip, Modal, CommandPalette
  features/       # dashboard, agents, evidence, audit, settings pages
                  # + void-neon/ 3D spatial viewport (see below)
                  # + approval/ ApprovalSheet, DiffViewer, PlanView,
                  #   CompensationSheet, queue panel (see below)
  lib/            # cn, format helpers
  stores/         # appStore (system status), uiStore (selection/theme), mockStore (fixtures)
  styles/         # globals.css + Tailwind v4 @theme tokens
  types/          # NavItem, ActivityEvent, SelectedEntity
  test/           # app, components, stores, format suites
```

## Design tokens

Void Neon philosophy: near-black `void-*` surfaces keep attention on content; neon
`cyan/violet/emerald` accents mark action and health; glass panels (`backdrop-blur`,
soft borders, deep shadows) float above the void.

Defined once in `styles/globals.css` via Tailwind v4 `@theme`:

| Token group | Examples                                                                  |
| ----------- | ------------------------------------------------------------------------- |
| color       | `void-950…700`, `neon-cyan/violet/emerald/amber/rose`, `mist-100/400/600` |
| radius      | `panel` 14px, `control` 10px                                              |
| shadow      | `glow-cyan/violet`, deep `panel`                                          |
| typography  | display / body / mono stacks                                              |
| motion      | `instant` 90ms, `fast` 160ms, `normal` 260ms                              |

Components consume tokens only — no hardcoded hex outside `globals.css`.

## Void Neon viewport

`features/void-neon/` renders the command-center topology with Three.js +
React Three Fiber: emissive Talos core, status-colored agent nodes, additive
core→agent links with traveling pulses, seeded starfield + grid, bloom
postprocessing (medium/high quality only).

- Data: `buildTopology(agents)` is a pure deterministic function of the mock
  store (golden-angle ring, seeded particles) aligned to `@talos/contracts`
  `SceneNode`/`SceneEdge`. Live events replace the mock source here later.
- Performance: `low/medium/high` quality (DPR, bloom, particle count), FPS
  meter in the HUD, `prefers-reduced-motion` disables animation, rendering
  pauses when the tab is hidden, and the whole viewport is `React.lazy`
  code-split off the main bundle.
- Fallback: no WebGL → polished “Viewport unavailable” panel; render faults
  are contained by `SceneErrorBoundary` without touching the dashboard.

## Backend bridge (`src/bridge/`)

Typed Tauri IPC for the local control plane — no untyped `invoke` calls:

- `client.ts`: `isTauri()` + `invokeCommand(cmd, args?, schema?)` with Zod validation
- `api.ts`: `getAppInfo`, `getSystemStatus`, `requestAction`, `runDiagnostic`, `verifyEvidenceLedger`
- `events.ts`: `subscribeTalosEvent` with per-event Zod payload schemas
- `useTalosSync.ts`: mount-once hook (in `Shell`) that hydrates stores and routes the five lifecycle events into them
- Outside Tauri (plain browser) every call fails closed as `BridgeError[backend_unavailable]`; the UI shows the disconnected banner and keeps the mock snapshot.

## Approval experience (`src/features/approval/`)

Human-in-the-loop trust interface for every `require_human_review` verdict:

- `ApprovalSheet.tsx` — non-dismissable modal overlay (approve / deny / modify,
  Enter / Escape / M shortcuts, focus trap, screen-reader labels, high-contrast,
  reduced-motion safe). Nine sections: summary, exact target, effect class
  (`READ`…`PRIVILEGED` badge), reversibility, payload/diff (renderer per
  `file_edit` / `shell` / `browser` / `network`), risk assessment, compensation
  plan, cost + scope/duration, deny reason. High-risk items are visually
  prominent (rose ring + alert role).
- `DiffViewer.tsx` (+ `diff.ts`) — unified diff with line numbers, word-level
  highlight, collapsible unchanged regions, one tab per file for multi-file
  edits, language badge per path. Deterministic built-in renderer (Monaco
  diff mode is the production upgrade path).
- `PlanView.tsx` — Plan / Act / Auto mode toggle per task (default Plan) +
  ordered review list with per-step approve and low-risk batch approve.
- `CompensationSheet.tsx` — rollback confirm surface with per-step progress;
  failures escalate to `failed_needs_manual_review` and alert the reviewer.
- `ApprovalQueuePanel.tsx` + `useApprovalActions.ts` + `stores/approvalStore.ts`
  — ordered queue, batch low-risk, Tauri IPC with deterministic browser-preview
  fallback. `Shell` mounts the overlay globally; status bar pulses amber while
  approvals wait; the Void Neon policy gate opens the sheet on click;
  Dashboard and Agents surface the queue and `waiting_policy` tasks; the
  command palette gains “Show pending approvals”.
