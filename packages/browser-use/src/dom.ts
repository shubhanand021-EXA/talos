import { createHash } from "node:crypto";

/** Deterministic FNV-1a hash (matches the Rust mock for cross-layer parity). */
export function fnv1a(input: string): bigint {
  let hash = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  for (const byte of Buffer.from(input, "utf8")) {
    hash ^= BigInt(byte);
    hash = (hash * prime) & mask;
  }
  return hash;
}

export function sha256Hex(input: string): string {
  return `sha256:${createHash("sha256").update(input, "utf8").digest("hex")}`;
}

export interface DomNode {
  role: string;
  label: string;
  selector: string;
  visible: boolean;
  interactable: boolean;
  ref: string;
}

export interface FakeDom {
  url: string;
  scope: string;
  title: string;
  nodes: DomNode[];
  simulated: boolean;
}

/** Deterministic fake DOM derived from the URL (seeded, no network). */
export function mockDom(url: string, scope?: string): FakeDom {
  const seed = fnv1a(url);
  const labels = ["header", "main", "nav", "button#submit", "input#q"];
  const mask = 0xffffffffffffffffn;
  const nodes: DomNode[] = labels.map((label, index) => {
    const visible = !label.includes("hidden");
    const idSeed = (seed + BigInt(index) * 2654435761n) & mask;
    return {
      role: label.split("#")[0] ?? "node",
      label,
      selector: `${label}:nth(${index})`,
      visible,
      interactable:
        visible && (label === "button#submit" || label === "input#q"),
      ref: `node-${idSeed.toString(16).padStart(8, "0")}`,
    };
  });
  return {
    url,
    scope: scope ?? "document",
    title: `Mock page for ${url}`,
    nodes,
    simulated: true,
  };
}

/** Canonical DOM hash (key order matches the Rust mock for parity). */
export function domHash(dom: FakeDom): string {
  return sha256Hex(JSON.stringify(dom));
}

export interface SelectorReport {
  selector: string;
  found: boolean;
  visible: boolean;
  interactable: boolean;
  matchCount: number;
  domHash: string;
}

/** Resolve a selector against the mock DOM (deterministic report). */
export function resolveSelector(
  url: string,
  selector: string,
  scope?: string,
): SelectorReport {
  const dom = mockDom(url, scope);
  const found = !selector.includes("missing");
  const visible = found && !selector.includes("hidden");
  const interactable =
    visible && !selector.includes("disabled") && !selector.includes("hidden");
  const matchCount = found ? Number(fnv1a(selector) % 3n) + 1 : 0;
  return {
    selector,
    found,
    visible,
    interactable,
    matchCount,
    domHash: domHash(dom),
  };
}

/** Interactability gate for mutation dry-runs (no page mutation). */
export function assertActionable(report: SelectorReport, action: string): void {
  if (!report.found) {
    throw new Error(`${action}: selector not found`);
  }
  if (!report.visible) {
    throw new Error(`${action}: element not visible`);
  }
  if (!report.interactable) {
    throw new Error(`${action}: element not interactable`);
  }
}
