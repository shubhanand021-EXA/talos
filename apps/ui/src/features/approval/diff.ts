/** Language hint from file extension (badge + future Monaco mode). */
export function languageForPath(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (["ts", "tsx", "mts", "cts"].includes(ext)) return "typescript";
  if (["js", "jsx", "mjs", "cjs"].includes(ext)) return "javascript";
  if (ext === "rs") return "rust";
  if (ext === "py") return "python";
  if (["json", "jsonc"].includes(ext)) return "json";
  if (["md", "mdx"].includes(ext)) return "markdown";
  if (["yml", "yaml"].includes(ext)) return "yaml";
  if (ext === "toml") return "toml";
  if (ext === "css") return "css";
  return "plaintext";
}

export interface LineRow {
  kind: "same" | "del" | "add";
  text: string;
  oldNo: number | null;
  newNo: number | null;
}

/**
 * Minimal deterministic line diff: common prefix/suffix stay collapsed,
 * the middle is rendered as del/add pairs. Word-level highlighting applies
 * inside changed lines. Deterministic for tests and cheap for large files.
 */
export function diffLines(original: string, proposed: string): LineRow[] {
  const a = original.split("\n");
  const b = proposed.split("\n");
  let prefix = 0;
  while (prefix < a.length && prefix < b.length && a[prefix] === b[prefix]) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < a.length - prefix &&
    suffix < b.length - prefix &&
    a[a.length - 1 - suffix] === b[b.length - 1 - suffix]
  ) {
    suffix += 1;
  }
  const rows: LineRow[] = [];
  for (let i = 0; i < prefix; i += 1) {
    rows.push({ kind: "same", text: a[i] ?? "", oldNo: i + 1, newNo: i + 1 });
  }
  const aMid = a.slice(prefix, a.length - suffix);
  const bMid = b.slice(prefix, b.length - suffix);
  const max = Math.max(aMid.length, bMid.length);
  for (let i = 0; i < max; i += 1) {
    if (i < aMid.length) {
      rows.push({
        kind: "del",
        text: aMid[i] ?? "",
        oldNo: prefix + i + 1,
        newNo: null,
      });
    }
    if (i < bMid.length) {
      rows.push({
        kind: "add",
        text: bMid[i] ?? "",
        oldNo: null,
        newNo: prefix + i + 1,
      });
    }
  }
  for (let i = 0; i < suffix; i += 1) {
    const idx = a.length - suffix + i;
    rows.push({
      kind: "same",
      text: a[idx] ?? "",
      oldNo: idx + 1,
      newNo: b.length - suffix + i + 1,
    });
  }
  return rows;
}
