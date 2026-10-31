import type { NavItem } from "../../types/index.js";

export const NAV_ITEMS: NavItem[] = [
  { label: "Dashboard", to: "/", hint: "Fleet overview and activity" },
  { label: "Agents", to: "/agents", hint: "Roster, roles, and status" },
  { label: "Tools", to: "/tools", hint: "Policy-gated tool registry" },
  { label: "Browser", to: "/browser", hint: "Isolated browser automation" },
  { label: "Computer", to: "/computer", hint: "Safety-first desktop control" },
  {
    label: "Context",
    to: "/context",
    hint: "Budgets and TypeCompact compaction",
  },
  { label: "Evidence", to: "/evidence", hint: "Ledger records and claims" },
  { label: "Audit", to: "/audit", hint: "Swarm reports and findings" },
  { label: "Settings", to: "/settings", hint: "Preferences and connection" },
];

export const GLYPHS: Record<string, string> = {
  Dashboard: "◈",
  Agents: "⬡",
  Tools: "⬢",
  Browser: "⬣",
  Computer: "⌨",
  Context: "◫",
  Evidence: "▣",
  Audit: "◎",
  Settings: "⚙",
};
