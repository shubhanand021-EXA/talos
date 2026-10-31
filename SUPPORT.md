# Support

## Getting help

1. Start with the docs:
   - [README.md](README.md) — vision, quickstart, architecture, capability matrix
   - [docs/RUNBOOK.md](docs/RUNBOOK.md) — setup, run, test, diagnose
   - [docs/CI.md](docs/CI.md) — what CI checks and how to read green/red
   - [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — Pentagon deep dive
   - [CHANGELOG.md](CHANGELOG.md) — what changed and what is next
2. Search existing [Issues](https://github.com/shubhanand021-EXA/talos/issues) and [docs/reports](docs/reports) before filing.
3. File with the matching template in [.github/ISSUE_TEMPLATE](.github/ISSUE_TEMPLATE): bug, feature, question, or security (private channel preferred — see [SECURITY.md](SECURITY.md)).

Include in every request: commit SHA (`git rev-parse --short HEAD`), OS + toolchain versions (`node --version`, `pnpm --version`, `cargo --version`, `rustc --version`), exact command, and verbatim output.

## Response expectations

This is a maintainer-led private repo. Issues are triaged on a best-effort basis; security reports via `security@talos.dev` (placeholder) or GitHub private vulnerability reporting are prioritized (acknowledgement within 72 hours).

## What is out of scope

- Cloud accounts, external LLM keys in-repo, mobile targets (see [docs/ROADMAP.md](docs/ROADMAP.md) non-goals).
- GUI debugging without reproduction evidence — attach `docs/reports/audit-latest.md` and exact logs.
