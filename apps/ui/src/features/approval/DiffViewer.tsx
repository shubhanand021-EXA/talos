import { useMemo, useState } from "react";
import type { DiffFile } from "@talos/contracts";
import { diffLines, languageForPath } from "./diff.js";

/** Split a changed line into shared/changed word spans vs its counterpart. */
function wordSpans(text: string, other: string): Array<{ word: string; changed: boolean }> {
  const words = text.split(/(\s+)/);
  const otherSet = new Set(other.split(/(\s+)/));
  return words.map((word) => ({
    word,
    changed: word.trim() !== "" && !otherSet.has(word),
  }));
}

function ChangedLine({
  text,
  counterpart,
  kind,
}: {
  text: string;
  counterpart: string;
  kind: "del" | "add";
}) {
  const spans = useMemo(() => wordSpans(text, counterpart), [text, counterpart]);
  return (
    <code
      className={
        kind === "del" ? "text-rose-200" : "text-emerald-200"
      }
    >
      {spans.map((span, i) =>
        span.changed ? (
          <mark
            key={`chg-${i}`}
            className={
              kind === "del"
                ? "rounded bg-rose-500/40 px-0.5"
                : "rounded bg-emerald-500/40 px-0.5"
            }
          >
            {span.word}
          </mark>
        ) : (
          <span key={`same-${i}`}>{span.word}</span>
        ),
      )}
    </code>
  );
}

function FileDiff({
  file,
  defaultCollapsed,
}: {
  file: DiffFile;
  defaultCollapsed: boolean;
}) {
  const rows = useMemo(
    () => diffLines(file.original, file.proposed),
    [file.original, file.proposed],
  );
  const [showUnchanged, setShowUnchanged] = useState(!defaultCollapsed);
  const unchanged = rows.filter((r) => r.kind === "same").length;
  const visible = showUnchanged ? rows : rows.filter((r) => r.kind !== "same");
  const delText = rows.find((r) => r.kind === "del")?.text ?? "";
  const addText = rows.find((r) => r.kind === "add")?.text ?? "";

  return (
    <div aria-label={`Diff for ${file.path}`}>
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="font-mono text-xs text-slate-400">
          {unchanged} unchanged lines
        </span>
        <button
          type="button"
          onClick={() => {
            setShowUnchanged((v) => !v);
          }}
          aria-expanded={showUnchanged}
          aria-label={showUnchanged ? "Collapse unchanged regions" : "Expand unchanged regions"}
          className="rounded-md border border-white/10 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/5"
        >
          {showUnchanged ? "Collapse unchanged" : "Expand unchanged"}
        </button>
      </div>
      <div
        role="table"
        aria-label={`Unified diff for ${file.path}`}
        className="overflow-x-auto rounded-md border border-white/10 bg-black/40 font-mono text-xs leading-5"
      >
        {visible.map((row) => (
          <div
            key={`${row.kind}-${row.oldNo ?? "x"}-${row.newNo ?? "y"}-${row.text.slice(0, 24)}`}
            role="row"
            aria-label={`${row.kind} line ${row.oldNo ?? row.newNo ?? "?"}`}
            className={
              row.kind === "del"
                ? "flex bg-rose-500/10"
                : row.kind === "add"
                  ? "flex bg-emerald-500/10"
                  : "flex text-slate-500"
            }
          >
            <span className="w-10 shrink-0 select-none px-2 text-right text-slate-600">
              {row.oldNo ?? ""}
            </span>
            <span className="w-10 shrink-0 select-none px-2 text-right text-slate-600">
              {row.newNo ?? ""}
            </span>
            <span className="w-6 shrink-0 select-none px-1 text-slate-500">
              {row.kind === "del" ? "−" : row.kind === "add" ? "+" : " "}
            </span>
            <span className="whitespace-pre-wrap break-all px-2">
              {row.kind === "same" ? (
                <code className="text-slate-400">{row.text}</code>
              ) : (
                <ChangedLine
                  text={row.text}
                  counterpart={row.kind === "del" ? addText : delText}
                  kind={row.kind}
                />
              )}
            </span>
          </div>
        ))}
        {visible.length === 0 ? (
          <p className="px-3 py-2 text-slate-500">No changes in this view.</p>
        ) : null}
      </div>
    </div>
  );
}

/**
 * Diff viewer: tab per file for multi-file edits, language badge per path,
 * collapsible unchanged regions, word-level highlighting. Monaco Editor in
 * diff mode is the production renderer when available (lazy import); this
 * built-in view is the deterministic fallback used in tests and preview.
 */
export function DiffViewer({
  files,
  original,
  proposed,
  target,
}: {
  files: DiffFile[];
  original?: string | null | undefined;
  proposed?: string | null | undefined;
  target: string;
}) {
  const effective: DiffFile[] = useMemo(() => {
    if (files.length > 0) return files;
    if (original !== undefined && original !== null && proposed !== undefined && proposed !== null) {
      return [{ path: target, original, proposed }];
    }
    return [];
  }, [files, original, proposed, target]);
  const [tab, setTab] = useState(0);
  if (effective.length === 0) {
    return (
      <p className="text-xs text-slate-500">
        No file diff available for this action.
      </p>
    );
  }
  const current = effective[Math.min(tab, effective.length - 1)];
  if (current === undefined) {
    return (
      <p className="text-xs text-slate-500">
        No file diff available for this action.
      </p>
    );
  }
  return (
    <div>
      {effective.length > 1 ? (
        <div role="tablist" aria-label="Changed files" className="mb-2 flex flex-wrap gap-1">
          {effective.map((file, i) => (
            <button
              key={`${file.path}-${i}`}
              type="button"
              role="tab"
              aria-selected={i === tab}
              onClick={() => {
                setTab(i);
              }}
              className={
                i === tab
                  ? "rounded-md bg-cyan-400/20 px-2 py-1 font-mono text-xs text-cyan-200"
                  : "rounded-md border border-white/10 px-2 py-1 font-mono text-xs text-slate-400 hover:bg-white/5"
              }
            >
              {file.path}
            </button>
          ))}
        </div>
      ) : null}
      <div className="mb-2 flex items-center gap-2">
        <span className="font-mono text-xs text-slate-300">{current.path}</span>
        <span
          aria-label={`Language ${languageForPath(current.path)}`}
          className="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[10px] text-slate-400"
        >
          {languageForPath(current.path)}
        </span>
      </div>
      <FileDiff file={current} defaultCollapsed={effective.length > 1} />
    </div>
  );
}
