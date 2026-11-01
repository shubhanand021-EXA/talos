import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useApprovalStore } from "../stores/approvalStore.js";
import { Modal } from "./Modal.js";

const ACTIONS = [
  { label: "Go to Dashboard", to: "/", keywords: "home overview" },
  { label: "Go to Agents", to: "/agents", keywords: "agent bot runner" },
  {
    label: "Go to Code",
    to: "/code",
    keywords: "code workspace shell tests diff git worktree",
  },
  {
    label: "Go to Tools",
    to: "/tools",
    keywords: "tool catalog dry-run receipt",
  },
  {
    label: "Go to Browser",
    to: "/browser",
    keywords: "browser automation navigate snapshot dry-run",
  },
  {
    label: "Go to Computer",
    to: "/computer",
    keywords: "computer desktop control safety emergency mock",
  },
  {
    label: "Go to Context",
    to: "/context",
    keywords: "context budget typecompact compaction tokens layers",
  },
  { label: "Go to Evidence", to: "/evidence", keywords: "ledger proof claim" },
  {
    label: "Go to Memory",
    to: "/memory",
    keywords: "memory semantic recall vector skills hybrid search",
  },
  { label: "Go to Audit", to: "/audit", keywords: "review report swarm" },
  { label: "Go to Settings", to: "/settings", keywords: "config theme" },
] as const;

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const navigate = useNavigate();
  const openSheet = useApprovalStore((s) => s.openSheet);
  const pendingCount = useApprovalStore((s) => s.pending.length);
  const [query, setQuery] = useState("");
  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (needle === "") {
      return ACTIONS;
    }
    return ACTIONS.filter((action) =>
      `${action.label} ${action.keywords}`.toLowerCase().includes(needle),
    );
  }, [query]);

  return (
    <Modal title="Command palette" onClose={onClose}>
      <input
        data-autofocus
        type="search"
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
        }}
        placeholder="Type a command…"
        aria-label="Command search"
        className="mb-2 w-full rounded-control border border-white/10 bg-void-950 px-3 py-2 text-sm outline-none placeholder:text-slate-600 focus:border-cyan-300/50"
      />
      <button
        type="button"
        onClick={() => {
          openSheet();
          onClose();
        }}
        aria-label={`Show pending approvals (${pendingCount} waiting)`}
        className="mb-2 flex w-full items-center justify-between rounded-md border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-left text-sm text-amber-200 hover:bg-amber-400/20"
      >
        Show pending approvals
        <kbd className="font-mono text-xs text-amber-300/70">
          {pendingCount} waiting
        </kbd>
      </button>
      <ul
        role="listbox"
        aria-label="Commands"
        className="max-h-64 overflow-y-auto"
      >
        {matches.length === 0 ? (
          <li className="px-3 py-4 text-center text-sm text-slate-500">
            No matching command
          </li>
        ) : (
          matches.map((action) => (
            <li key={action.to}>
              <button
                type="button"
                role="option"
                aria-selected="false"
                onClick={() => {
                  void navigate(action.to);
                  onClose();
                }}
                className="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm text-slate-200 hover:bg-cyan-400/10"
              >
                {action.label}
                <kbd className="font-mono text-xs text-slate-500">
                  {action.to}
                </kbd>
              </button>
            </li>
          ))
        )}
      </ul>
    </Modal>
  );
}
