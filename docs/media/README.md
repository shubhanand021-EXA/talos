# Talos media kit

All assets are **original, deterministic, lightweight, and dark/neon themed** — generated offline by [`scripts/media/generate-brand.mjs`](../../scripts/media/generate-brand.mjs). No external downloads, no copyrighted inputs, no native dependencies.

## Assets

| File               | What it suggests                                                                                                                                                                                                               | Size class                 |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------- |
| `talos-logo.svg`   | Hex shield + tri-node agent graph (cyan/violet/green on void)                                                                                                                                                                  | ~1.5 KB                    |
| `talos-banner.svg` | Wordmark + grid + agent constellation + capability pills                                                                                                                                                                       | ~3 KB                      |
| `architecture.svg` | Control-loop diagram (UI → IPC → Policy → Evidence → Audit)                                                                                                                                                                    | ~4 KB                      |
| `hero.gif`         | Void-black command center: neon pulse rings, orbiting agent network, task squares cycling queued → running → waiting → completed, tool/browser/computer diamonds, amber policy gate, green ledger blocks, scanline, status bar | ~63 KB, 320×180, 24 frames |
| `evidence.gif`     | Ledger verification: 4-block hash chain, progressive checks, policy ALLOW/DENY states                                                                                                                                          | ~42 KB, 320×180, 16 frames |

Regenerate:

```sh
pnpm media:generate
git status --porcelain -- docs/media  # must be clean (deterministic)
```

CI (`media-check`) verifies: files exist, every `docs/media/*` reference in `README.md` resolves locally, GIFs start with `GIF89a`, and regeneration leaves no diff.

## Tauri icons — placeholder note

`apps/desktop/src-tauri/icons/*` are still the Batch 1 generated placeholders. The new SVG kit is icon-ready, but a full multi-size `icon.png`/`icon.ico`/`icon.icns` replacement has **not** been applied in this commit to avoid an unverified binary churn.

Exact next step (maintainer, on a GUI machine):

```sh
# from repo root, with the official Tauri CLI:
pnpm --filter desktop exec tauri icon docs/media/talos-logo.svg
# verifies: apps/desktop/src-tauri/icons/icon.{png,ico,icns} + 32x32/128x128 refresh
# then: pnpm build:desktop (needs WebView2 + WiX/NSIS on Windows) and attach screenshots
```

Until then, keep current icons — they load, they pass `cargo check`, and CI does not gate on pixel content.
