# @talos/browser-use

Talos browser-use foundation: isolated sessions, DOM-first safety, deterministic mock adapter, optional live Chromium.

## Architecture

TypeScript owns the browser worker (`packages/browser-use`); Rust stays authoritative for policy, evidence, and compensation. The Rust tool framework (`crates/tool_runtime`) exposes the same eight actions (`browser.navigate/click/type/scroll/select/extract/screenshot/snapshot`) through the generic `invoke_tool*` IPC — no new Tauri commands, no broad shell access. A future step can orchestrate this worker via the controlled `process.exec` tool; until then the Rust mock and this package share deterministic DOM semantics (FNV-1a seeded DOM, `sha256:` hashes).

## Safety defaults

- Isolated profile per task under `<sandbox>/browser-profiles/<task>`; never a personal profile; no saved passwords; no cookies persisted unless explicitly enabled.
- Dry-run first: selector resolution + validation without page mutation.
- Read-only actions (`navigate/snapshot/extract/screenshot/scroll`) run Low risk; mutations (`click/type/select`) are evidence-gated Medium; `highRisk` flags (form submit, purchase, settings, upload/download, destructive) escalate to human review.
- Live Chromium is **disabled by default** and CI never touches the network.

## Live mode (opt-in, local only)

```sh
pnpm add -D playwright
npx playwright install chromium
TALOS_BROWSER_LIVE=1 pnpm --filter @talos/browser-use run worker --live '{"action":"snapshot_dom","url":"https://example.com"}'
```

Without `TALOS_BROWSER_LIVE=1` or without Playwright installed, live calls return `{ available: false }` with a reason — never a silent pass.

## Worker CLI

```sh
pnpm --filter @talos/browser-use run build
node packages/browser-use/dist/worker.js '{"action":"snapshot_dom","url":"https://example.com"}'
echo '{"action":"extract_text","url":"https://example.com","maxChars":100}' | node packages/browser-use/dist/worker.js
```

Input is a single JSON action (`action`, `url`, `selector`, `text`, `direction`, `maxChars`, `highRisk`, `highRiskKind`, ...); output is a receipt-shaped JSON object. Deterministic, no display required.
