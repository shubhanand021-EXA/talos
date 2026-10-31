// Talos brand/media generator — deterministic, dependency-free, offline.
// Generates original SVG + GIF assets under docs/media/.
// Run: pnpm media:generate
// No external downloads, no copyrighted assets, no native deps.
// GIFs are written by a minimal built-in GIF89a encoder (global palette + LZW).

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const outDir = join(root, "docs", "media");
mkdirSync(outDir, { recursive: true });

// Palette: void-black + neon accents (shared by SVG + GIF).
const C = {
  bg: "#05070d",
  panel: "#0a0f1c",
  cyan: "#00e5ff",
  violet: "#b537f2",
  green: "#00ff9d",
  amber: "#ffb300",
  red: "#ff3b5c",
  white: "#e8f1ff",
  slate: "#1a2333",
};

// ---- SVG assets (original, hand-authored) ----

const logoSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256" role="img" aria-label="Talos logo">
  <defs>
    <radialGradient id="void" cx="50%" cy="45%" r="65%">
      <stop offset="0%" stop-color="#0a0f1c"/>
      <stop offset="100%" stop-color="#05070d"/>
    </radialGradient>
    <linearGradient id="edge" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#00e5ff"/>
      <stop offset="55%" stop-color="#b537f2"/>
      <stop offset="100%" stop-color="#00ff9d"/>
    </linearGradient>
  </defs>
  <rect width="256" height="256" rx="56" fill="url(#void)"/>
  <rect x="1.5" y="1.5" width="253" height="253" rx="54.5" fill="none" stroke="#1a2333" stroke-width="3"/>
  <g fill="none" stroke="url(#edge)" stroke-width="7" stroke-linejoin="round" stroke-linecap="round">
    <path d="M128 34 L208 80 L208 176 L128 222 L48 176 L48 80 Z"/>
  </g>
  <g fill="none" stroke="#00e5ff" stroke-width="3" opacity="0.9">
    <circle cx="128" cy="128" r="16"/>
  </g>
  <g stroke-linecap="round" stroke-width="4">
    <line x1="128" y1="128" x2="128" y2="66" stroke="#00e5ff"/>
    <line x1="128" y1="128" x2="181" y2="159" stroke="#b537f2"/>
    <line x1="128" y1="128" x2="75" y2="159" stroke="#00ff9d"/>
  </g>
  <g fill="#e8f1ff">
    <circle cx="128" cy="62" r="7"/>
    <circle cx="185" cy="161" r="7"/>
    <circle cx="71" cy="161" r="7"/>
  </g>
  <g fill="#00e5ff">
    <circle cx="128" cy="62" r="2.6"/>
    <circle cx="185" cy="161" r="2.6"/>
    <circle cx="71" cy="161" r="2.6"/>
  </g>
</svg>
`;

const bannerSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="400" viewBox="0 0 1600 400" role="img" aria-label="Talos banner">
  <defs>
    <radialGradient id="bg" cx="50%" cy="50%" r="75%">
      <stop offset="0%" stop-color="#0a0f1c"/>
      <stop offset="100%" stop-color="#05070d"/>
    </radialGradient>
    <linearGradient id="title" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#e8f1ff"/>
      <stop offset="55%" stop-color="#00e5ff"/>
      <stop offset="100%" stop-color="#b537f2"/>
    </linearGradient>
    <linearGradient id="hex" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#00e5ff"/>
      <stop offset="100%" stop-color="#b537f2"/>
    </linearGradient>
  </defs>
  <rect width="1600" height="400" fill="url(#bg)"/>
  <g stroke="#1a2333" stroke-width="1" opacity="0.9">
    <path d="M0 80 H1600 M0 160 H1600 M0 240 H1600 M0 320 H1600 M160 0 V400 M400 0 V400 M640 0 V400 M880 0 V400 M1120 0 V400 M1360 0 V400"/>
  </g>
  <g fill="none" stroke="url(#hex)" stroke-width="6" stroke-linejoin="round">
    <path d="M170 130 L235 167 L235 243 L170 280 L105 243 L105 167 Z"/>
  </g>
  <circle cx="170" cy="205" r="12" fill="none" stroke="#00e5ff" stroke-width="3"/>
  <line x1="170" y1="205" x2="170" y2="152" stroke="#00e5ff" stroke-width="4" stroke-linecap="round"/>
  <line x1="170" y1="205" x2="216" y2="232" stroke="#b537f2" stroke-width="4" stroke-linecap="round"/>
  <line x1="170" y1="205" x2="124" y2="232" stroke="#00ff9d" stroke-width="4" stroke-linecap="round"/>
  <text x="290" y="195" font-family="Inter, Segoe UI, Arial, sans-serif" font-size="96" font-weight="800" letter-spacing="6" fill="url(#title)">TALOS</text>
  <text x="292" y="250" font-family="Inter, Segoe UI, Arial, sans-serif" font-size="26" font-weight="500" letter-spacing="1.5" fill="#8b98b3">All-in-one AI agent ecosystem — policy-gated, evidence-backed, audit-green.</text>
  <g font-family="Inter, Segoe UI, Arial, sans-serif" font-size="20" font-weight="600">
    <rect x="292" y="278" width="150" height="40" rx="20" fill="none" stroke="#00e5ff" stroke-width="2"/>
    <text x="316" y="304" fill="#00e5ff">DESKTOP</text>
    <rect x="454" y="278" width="170" height="40" rx="20" fill="none" stroke="#b537f2" stroke-width="2"/>
    <text x="478" y="304" fill="#b537f2">EVIDENCE</text>
    <rect x="636" y="278" width="130" height="40" rx="20" fill="none" stroke="#00ff9d" stroke-width="2"/>
    <text x="662" y="304" fill="#00ff9d">AUDIT</text>
  </g>
  <g fill="#00e5ff" opacity="0.85">
    <circle cx="1330" cy="120" r="5"/>
    <circle cx="1410" cy="200" r="7"/>
    <circle cx="1360" cy="290" r="4"/>
    <circle cx="1490" cy="140" r="4"/>
    <circle cx="1455" cy="265" r="6"/>
  </g>
  <g stroke="#00e5ff" stroke-width="1.5" opacity="0.5">
    <line x1="1330" y1="120" x2="1410" y2="200"/>
    <line x1="1410" y1="200" x2="1360" y2="290"/>
    <line x1="1410" y1="200" x2="1490" y2="140"/>
    <line x1="1360" y1="290" x2="1455" y2="265"/>
  </g>
</svg>
`;

const architectureSvg = `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="620" viewBox="0 0 1200 620" role="img" aria-label="Talos architecture diagram">
  <defs>
    <linearGradient id="flow" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#00e5ff"/>
      <stop offset="100%" stop-color="#b537f2"/>
    </linearGradient>
  </defs>
  <rect width="1200" height="620" fill="#05070d"/>
  <text x="40" y="52" font-family="Inter, Segoe UI, Arial, sans-serif" font-size="30" font-weight="800" fill="#e8f1ff">TALOS CONTROL LOOP</text>
  <text x="40" y="78" font-family="Inter, Segoe UI, Arial, sans-serif" font-size="16" fill="#8b98b3">UI -&gt; IPC -&gt; Policy -&gt; Evidence -&gt; Events -&gt; UI (local-first, default-deny)</text>
  <g font-family="Inter, Segoe UI, Arial, sans-serif" font-size="20" font-weight="700">
    <rect x="40" y="120" width="200" height="110" rx="16" fill="#0a0f1c" stroke="#00e5ff" stroke-width="2"/>
    <text x="66" y="168" fill="#e8f1ff">React UI</text>
    <text x="66" y="194" font-size="14" font-weight="500" fill="#8b98b3">Void Neon + 5 routes</text>
    <rect x="280" y="120" width="200" height="110" rx="16" fill="#0a0f1c" stroke="#00e5ff" stroke-width="2"/>
    <text x="306" y="168" fill="#e8f1ff">Tauri IPC</text>
    <text x="306" y="194" font-size="14" font-weight="500" fill="#8b98b3">5 typed commands</text>
    <rect x="520" y="120" width="200" height="110" rx="16" fill="#0a0f1c" stroke="#ffb300" stroke-width="2"/>
    <text x="546" y="168" fill="#e8f1ff">Policy Broker</text>
    <text x="546" y="194" font-size="14" font-weight="500" fill="#8b98b3">default-deny gates</text>
    <rect x="760" y="120" width="200" height="110" rx="16" fill="#0a0f1c" stroke="#00ff9d" stroke-width="2"/>
    <text x="786" y="168" fill="#e8f1ff">Evidence Store</text>
    <text x="786" y="194" font-size="14" font-weight="500" fill="#8b98b3">hash-chained SQLite</text>
    <rect x="1000" y="120" width="160" height="110" rx="16" fill="#0a0f1c" stroke="#b537f2" stroke-width="2"/>
    <text x="1020" y="168" fill="#e8f1ff">Audit x9</text>
    <text x="1020" y="194" font-size="14" font-weight="500" fill="#8b98b3">swarm reports</text>
  </g>
  <g stroke="url(#flow)" stroke-width="3" fill="none" marker-end="none">
    <line x1="240" y1="175" x2="280" y2="175"/>
    <line x1="480" y1="175" x2="520" y2="175"/>
    <line x1="720" y1="175" x2="760" y2="175"/>
    <line x1="960" y1="175" x2="1000" y2="175"/>
  </g>
  <g font-family="Inter, Segoe UI, Arial, sans-serif" font-size="18" font-weight="600">
    <rect x="40" y="280" width="330" height="90" rx="14" fill="#0a0f1c" stroke="#1a2333" stroke-width="2"/>
    <text x="60" y="314" fill="#00e5ff">Agent Runtime</text>
    <text x="60" y="340" font-size="14" font-weight="500" fill="#8b98b3">LocalOrchestrator + echo loop</text>
    <rect x="400" y="280" width="360" height="90" rx="14" fill="#0a0f1c" stroke="#1a2333" stroke-width="2"/>
    <text x="420" y="314" fill="#ffb300">Compensation Plans</text>
    <text x="420" y="340" font-size="14" font-weight="500" fill="#8b98b3">proposed -&gt; approved -&gt; executed</text>
    <rect x="790" y="280" width="370" height="90" rx="14" fill="#0a0f1c" stroke="#1a2333" stroke-width="2"/>
    <text x="810" y="314" fill="#00ff9d">Claims vs Proof</text>
    <text x="810" y="340" font-size="14" font-weight="500" fill="#8b98b3">pending / verified / rejected</text>
  </g>
  <g font-family="Inter, Segoe UI, Arial, sans-serif" font-size="16">
    <rect x="40" y="410" width="1120" height="160" rx="14" fill="#0a0f1c" stroke="#1a2333" stroke-width="2"/>
    <text x="60" y="444" font-weight="700" fill="#e8f1ff">Batch 2+ track</text>
    <text x="60" y="472" fill="#8b98b3">Browser-use foundation (scoped capability, allow-list) + Computer-use safety (human-review gates, redaction)</text>
    <text x="60" y="500" fill="#8b98b3">Tool integrations (fs/shell behind policy) + Context budgets / TypeCompact + richer Void Neon selection</text>
    <text x="60" y="528" fill="#5b6b88">Non-goals: cloud accounts, external LLM keys in-repo, mobile targets.</text>
  </g>
</svg>
`;

// ---- Minimal GIF89a encoder (deterministic, palette-based) ----

const PALETTE = [
  [0x05, 0x07, 0x0d], // 0 void black
  [0x00, 0xe5, 0xff], // 1 neon cyan
  [0xb5, 0x37, 0xf2], // 2 violet
  [0x00, 0xff, 0x9d], // 3 green
  [0xff, 0xb3, 0x00], // 4 amber
  [0xff, 0x3b, 0x5c], // 5 red
  [0xe8, 0xf1, 0xff], // 6 white
  [0x1a, 0x23, 0x33], // 7 slate
];

function lzwCompress(minCodeSize, indices) {
  const clear = 1 << minCodeSize;
  const eoi = clear + 1;
  let codeSize = minCodeSize + 1;
  let nextCode = clear + 2;
  let dict = new Map();
  const outBytes = [];
  let acc = 0;
  let bits = 0;
  function emit(code) {
    acc |= code << bits;
    bits += codeSize;
    while (bits >= 8) {
      outBytes.push(acc & 0xff);
      acc >>= 8;
      bits -= 8;
    }
  }
  function resetDict() {
    dict = new Map();
    codeSize = minCodeSize + 1;
    nextCode = clear + 2;
  }
  emit(clear);
  let prefix = indices[0];
  for (let i = 1; i < indices.length; i++) {
    const k = indices[i];
    const key = prefix * 256 + k;
    const found = dict.get(key);
    if (found !== undefined) {
      prefix = found;
    } else {
      emit(prefix);
      if (nextCode < 4096) {
        dict.set(key, nextCode++);
        if (nextCode >= 1 << codeSize && codeSize < 12) codeSize++;
      } else {
        emit(clear);
        resetDict();
      }
      prefix = k;
    }
  }
  emit(prefix);
  emit(eoi);
  if (bits > 0) outBytes.push(acc & 0xff);
  return Uint8Array.from(outBytes);
}

function encodeGif(width, height, frames, delays) {
  const minCodeSize = 3; // 8 colors
  const bytes = [];
  const pushStr = (s) => {
    for (const ch of s) bytes.push(ch.charCodeAt(0));
  };
  const push16 = (v) => {
    bytes.push(v & 0xff, (v >> 8) & 0xff);
  };
  pushStr("GIF89a");
  push16(width);
  push16(height);
  bytes.push(0xf2, 0x00, 0x00); // GCT flag + 8-color table, bg 0, aspect 0
  for (const [r, g, b] of PALETTE) bytes.push(r, g, b);
  // Netscape looping (forever)
  bytes.push(0x21, 0xff, 0x0b);
  pushStr("NETSCAPE2.0");
  bytes.push(0x03, 0x01, 0x00, 0x00, 0x00);
  for (let f = 0; f < frames.length; f++) {
    const delay = delays[f] ?? 8;
    bytes.push(
      0x21,
      0xf9,
      0x04,
      0x08,
      delay & 0xff,
      (delay >> 8) & 0xff,
      0x00,
      0x00,
    );
    bytes.push(0x2c, 0x00, 0x00, 0x00, 0x00);
    push16(width);
    push16(height);
    bytes.push(0x00);
    const compressed = lzwCompress(minCodeSize, frames[f]);
    bytes.push(minCodeSize);
    for (let i = 0; i < compressed.length; i += 255) {
      const chunk = Math.min(255, compressed.length - i);
      bytes.push(chunk);
      for (let j = 0; j < chunk; j++) bytes.push(compressed[i + j]);
    }
    bytes.push(0x00);
  }
  bytes.push(0x3b);
  return Buffer.from(bytes);
}

// ---- Frame painters (all deterministic, no randomness) ----

const W = 320;
const H = 180;

function newFrame() {
  return new Uint8Array(W * H).fill(0);
}
function setPx(frame, x, y, color) {
  if (x < 0 || y < 0 || x >= W || y >= H) return;
  frame[y * W + x] = color;
}
function fillRect(frame, x0, y0, w, h, color) {
  for (let y = y0; y < y0 + h; y++)
    for (let x = x0; x < x0 + w; x++) setPx(frame, x, y, color);
}
function line(frame, x0, y0, x1, y1, color) {
  let dx = Math.abs(x1 - x0);
  let dy = Math.abs(y1 - y0);
  let sx = x0 < x1 ? 1 : -1;
  let sy = y0 < y1 ? 1 : -1;
  let err = dx - dy;
  let x = x0;
  let y = y0;
  for (let i = 0; i < 2000; i++) {
    setPx(frame, x, y, color);
    if (x === x1 && y === y1) break;
    const e2 = 2 * err;
    if (e2 > -dy) {
      err -= dy;
      x += sx;
    }
    if (e2 < dx) {
      err += dx;
      y += sy;
    }
  }
}
function circle(frame, cx, cy, r, color) {
  for (let a = 0; a < 360; a += 2) {
    const rad = (a * Math.PI) / 180;
    setPx(
      frame,
      Math.round(cx + r * Math.cos(rad)),
      Math.round(cy + r * Math.sin(rad)),
      color,
    );
  }
}
function dot3(frame, cx, cy, color) {
  fillRect(frame, cx - 1, cy - 1, 3, 3, color);
  setPx(frame, cx, cy, 6);
}
function square(frame, cx, cy, half, color) {
  for (let i = -half; i <= half; i++) {
    setPx(frame, cx + i, cy - half, color);
    setPx(frame, cx + i, cy + half, color);
    setPx(frame, cx - half, cy + i, color);
    setPx(frame, cx + half, cy + i, color);
  }
}
function diamond(frame, cx, cy, half, color) {
  line(frame, cx, cy - half, cx + half, cy, color);
  line(frame, cx + half, cy, cx, cy + half, color);
  line(frame, cx, cy + half, cx - half, cy, color);
  line(frame, cx - half, cy, cx, cy - half, color);
}
function grid(frame) {
  for (let x = 0; x < W; x += 32)
    for (let y = 0; y < H; y++) setPx(frame, x, y, 7);
  for (let y = 0; y < H; y += 30)
    for (let x = 0; x < W; x++) setPx(frame, x, y, 7);
}

function heroFrames() {
  const N = 24;
  const frames = [];
  const nodes = 6;
  for (let f = 0; f < N; f++) {
    const frame = newFrame();
    grid(frame);
    const cx = 160;
    const cy = 88;
    // pulse rings (two-phase)
    const r1 = 12 + ((f * 5) % 72);
    const r2 = 12 + (((f + 12) * 5) % 72);
    circle(frame, cx, cy, r1, 1);
    circle(frame, cx, cy, r2, 2);
    dot3(frame, cx, cy, 1);
    // orbiting agent nodes
    const pts = [];
    for (let n = 0; n < nodes; n++) {
      const base = (n * 360) / nodes;
      const ang = ((base + f * 9) * Math.PI) / 180;
      const rad = 46 + 14 * Math.sin(((n * 73 + f * 7) * Math.PI) / 180);
      const x = Math.round(cx + rad * Math.cos(ang));
      const y = Math.round(cy + rad * 0.62 * Math.sin(ang));
      pts.push([x, y]);
    }
    for (let i = 0; i < pts.length; i++) {
      const [x1, y1] = pts[i];
      const [x2, y2] = pts[(i + 1) % pts.length];
      line(frame, x1, y1, x2, y2, 7);
    }
    for (let i = 0; i < pts.length; i++) {
      const [x, y] = pts[i];
      const col = [1, 2, 3, 1, 2, 3][i % 6];
      // glow cross
      setPx(frame, x - 2, y, col);
      setPx(frame, x + 2, y, col);
      setPx(frame, x, y - 2, col);
      setPx(frame, x, y + 2, col);
      dot3(frame, x, y, col);
    }
    // command-center scanline
    const sy = (f * 7) % H;
    for (let x = 0; x < W; x += 2) setPx(frame, x, sy, 1);
    // live topology ring: task squares cycle queued → running → waiting → completed
    const taskCols = [7, 1, 4, 3];
    for (let n = 0; n < 4; n++) {
      const tx = 30 + n * 24;
      const ty = 122;
      const col = taskCols[(Math.floor(f / 6) + n) % 4];
      square(frame, tx, ty, 4, col);
      if (col === 4) setPx(frame, tx, ty, 4); // amber gate dot
    }
    // tool diamonds: tool (cyan), browser (violet), computer-use (green)
    const toolCols = [1, 2, 3];
    for (let n = 0; n < 3; n++) {
      const tx = 214 + n * 22;
      const ty = 122;
      diamond(frame, tx, ty, 5, toolCols[n]);
      if ((f + n * 8) % 24 < 12) setPx(frame, tx, ty, toolCols[n]);
    }
    // policy gate diamond (amber pulse, top-right)
    diamond(frame, 292, 30, 6, 4);
    if (f % 12 < 6) setPx(frame, 292, 30, 4);
    // evidence ledger: three verified blocks (bottom-right)
    for (let n = 0; n < 3; n++) {
      fillRect(frame, 262 + n * 14, 138, 10, 6, 3);
      setPx(frame, 264 + n * 14, 140, 3);
    }
    // status bar
    fillRect(frame, 0, H - 14, W, 14, 7);
    fillRect(frame, 2, H - 12, W - 4, 10, 0);
    const pulse =
      40 + Math.round(60 * (0.5 + 0.5 * Math.sin((f * Math.PI * 2) / N)));
    fillRect(frame, 6, H - 9, pulse, 4, 3);
    fillRect(frame, 6 + pulse + 4, H - 9, 50, 4, 1);
    fillRect(frame, 60 + pulse + 4, H - 9, 30, 4, 2);
    frames.push(frame);
  }
  return { frames, delays: new Array(N).fill(8) };
}

function evidenceFrames() {
  const N = 16;
  const frames = [];
  const boxes = [
    { x: 22, y: 66 },
    { x: 94, y: 66 },
    { x: 166, y: 66 },
    { x: 238, y: 66 },
  ];
  for (let f = 0; f < N; f++) {
    const frame = newFrame();
    grid(frame);
    // chain links
    for (let i = 0; i < boxes.length - 1; i++) {
      const x1 = boxes[i].x + 58;
      const x2 = boxes[i + 1].x;
      line(frame, x1, 82, x2, 82, 7);
      setPx(frame, Math.round((x1 + x2) / 2), 82, 1);
    }
    const verifiedCount = Math.min(4, Math.floor(f / 3) + 1);
    for (let b = 0; b < boxes.length; b++) {
      const { x, y } = boxes[b];
      const verified = b < verifiedCount;
      const denied = f >= 13 && b === 3;
      const border = denied ? 5 : verified ? 3 : 4;
      // box
      for (let i = 0; i < 58; i++) {
        setPx(frame, x + i, y, border);
        setPx(frame, x + i, y + 32, border);
      }
      for (let j = 0; j <= 32; j++) {
        setPx(frame, x, y + j, border);
        setPx(frame, x + 57, y + j, border);
      }
      // hash bars (deterministic pattern per box)
      for (let row = 0; row < 4; row++) {
        const bw = 12 + ((b * 7 + row * 5 + f) % 30);
        fillRect(
          frame,
          x + 5,
          y + 5 + row * 6,
          Math.min(48, bw),
          3,
          verified && !denied ? 3 : 7,
        );
      }
      // status glyph
      if (denied) {
        line(frame, x + 22, y + 38, x + 36, y + 52, 5);
        line(frame, x + 36, y + 38, x + 22, y + 52, 5);
      } else if (verified) {
        line(frame, x + 20, y + 45, x + 27, y + 52, 3);
        line(frame, x + 27, y + 52, x + 39, y + 38, 3);
      } else {
        circle(frame, x + 29, y + 45, 6, 4);
      }
    }
    // policy bar: ALLOW vs DENY / REQUIRE states
    fillRect(frame, 0, H - 26, W, 26, 7);
    fillRect(frame, 2, H - 24, W - 4, 22, 0);
    const showDeny = f >= 12;
    fillRect(frame, 8, H - 20, 120, 14, showDeny ? 7 : 3);
    fillRect(frame, 136, H - 20, 60, 14, 4);
    fillRect(frame, 204, H - 20, 108, 14, showDeny ? 5 : 7);
    // progress ticks
    for (let i = 0; i < 4; i++) {
      fillRect(frame, 8 + i * 30, H - 5, 24, 2, i < verifiedCount ? 3 : 7);
    }
    frames.push(frame);
  }
  return { frames, delays: new Array(N).fill(18) };
}

// ---- Write everything ----

writeFileSync(join(outDir, "talos-logo.svg"), logoSvg);
writeFileSync(join(outDir, "talos-banner.svg"), bannerSvg);
writeFileSync(join(outDir, "architecture.svg"), architectureSvg);

const hero = heroFrames();
writeFileSync(
  join(outDir, "hero.gif"),
  encodeGif(W, H, hero.frames, hero.delays),
);

const ev = evidenceFrames();
writeFileSync(
  join(outDir, "evidence.gif"),
  encodeGif(W, H, ev.frames, ev.delays),
);

console.log("Talos brand assets generated:");
console.log(" - docs/media/talos-logo.svg");
console.log(" - docs/media/talos-banner.svg");
console.log(" - docs/media/architecture.svg");
console.log(" - docs/media/hero.gif");
console.log(" - docs/media/evidence.gif");
