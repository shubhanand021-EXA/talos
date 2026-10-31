# Talos Evidence Log

Every claim in PROGRESS.md must link to an entry here. Format per entry:

```text
## <date> — <title>
- Command: <exact command>
- Output: <key output lines, verbatim>
- Commit: <message + hash from `git log -1`>
- Verification: <how to reproduce / what was checked>
- Notes: <limitations, if any>
```

---

## 2026-10-31 — Prompt 01 bootstrap

- Commands:
  - `winget install -e --id Git.Git`
  - `winget install -e --id OpenJS.NodeJS.LTS`
  - `winget install -e --id Rustlang.Rustup`
  - `winget install -e --id Casey.Just`
  - `winget install -e --id pnpm.pnpm`
  - `pnpm install`
  - `pnpm typecheck` / `pnpm lint` / `pnpm test` / `pnpm build` / `pnpm audit --audit-level high`
  - `just setup` / `just doctor` / `just lint` / `just typecheck` / `just test` / `just build` / `just audit`
- Outputs (verbatim, key lines):
  - `git version 2.56.0.windows.4`
  - `v26.13.0` (node)
  - `10.34.7` (pnpm, pinned in `packageManager`)
  - `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
  - `cargo 1.99.1 (5f94df478 2026-08-27)`
  - `just 1.59.0`
  - `pnpm install`: `devDependencies: + prettier 3.9.10 + typescript 5.9.3`
  - `pnpm lint`: `All matched files use Prettier code style!`
  - `pnpm test`: `pass 4, fail 0`
  - `pnpm audit`: `No known vulnerabilities found`
  - `just doctor`: `doctor: ok`
- Commit: `chore: bootstrap talos monorepo foundation` (see `git log -1 --oneline`)
- Verification:
  - `git status --porcelain` clean after commit
  - `just doctor` prints git/node/pnpm/rustc/cargo/just versions
  - `README.md` links to `docs/*.md`, `LICENSE`, `.nvmrc` all resolve; no dead links
- Notes:
  - Clean Windows 11 machine; all toolchains installed via official winget packages.
  - `pnpm --version` resolves to npm-global 10.34.7 (winget ships 12.16.1); pinned to active 10.34.7.
  - `packages/contracts` is a minimal Batch 1 placeholder (real schemas in Prompt 05) so `tsc` has real inputs.
