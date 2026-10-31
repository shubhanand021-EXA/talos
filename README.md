# Talos

Enterprise-grade, open-source, all-in-one AI agent ecosystem and AAA-grade desktop application.

> Current phase: **Batch 1 / Green Baseline** — Foundation and Shield baseline only. No product features yet.

## Vision

Talos is a single monorepo for an auditable, policy-governed agent platform:

- One desktop app (Tauri 2 + Rust + React)
- One shared contract layer (TypeScript)
- One policy broker (Rust)
- One audit swarm (9 agents)
- One evidence ledger (claims vs. proof)

Short-term goal: a clean, reproducible engineering environment where every claim is backed by runnable evidence.

## Repository layout

```text
talos/
  apps/
    desktop/      # Tauri 2 + React shell (Batch 2+)
    ui/           # Frontend UI package (Batch 2+)
  crates/         # Rust backend / policy broker (Batch 2+)
  packages/
    contracts/    # Shared TypeScript contracts (Batch 2+)
  tools/          # Audit tooling and smoke tests
  docs/           # Architecture, progress, evidence, roadmap
  .github/
    workflows/   # CI
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the Pentagon Architecture.

## Prerequisites

- git >= 2.40
- Node.js LTS (see [.nvmrc](.nvmrc), currently v26)
- pnpm >= 10 (pinned via `packageManager`)
- Rust stable toolchain + cargo
- [just](https://github.com/casey/just) >= 1.30

## Setup

```sh
just setup
just doctor
```

Or without `just`:

```sh
pnpm install
pnpm typecheck
pnpm lint
pnpm test
```

## Commands

| Command          | Description                            |
| ---------------- | -------------------------------------- |
| `just setup`     | Install deps and verify tool versions  |
| `just doctor`    | Print environment readiness            |
| `just lint`      | `prettier --check` + workspace lints   |
| `just format`    | `prettier --write`                     |
| `just typecheck` | Strict `tsc --noEmit`                  |
| `just test`      | `node --test` smoke + workspace tests  |
| `just build`     | Workspace builds (no-op until Batch 2) |
| `just audit`     | `pnpm audit`                           |

## Docs

- [Architecture](docs/ARCHITECTURE.md)
- [Progress](docs/PROGRESS.md)
- [Evidence](docs/EVIDENCE.md)
- [Roadmap](docs/ROADMAP.md)

## License

MIT — see [LICENSE](LICENSE).
