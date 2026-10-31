import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Modal } from "./Modal.js";

const ACTIONS = [
  { label: "Go to Dashboard", to: "/", keywords: "home overview" },
  { label: "Go to Agents", to: "/agents", keywords: "agent bot runner" },
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
  { label: "Go to Evidence", to: "/evidence", keywords: "ledger proof claim" },
  { label: "Go to Audit", to: "/audit", keywords: "review report swarm" },
  { label: "Go to Settings", to: "/settings", keywords: "config theme" },
] as const;

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const navigate = useNavigate();
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
