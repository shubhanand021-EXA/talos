import type { NavItem } from "../../types/index.js";

export const NAV_ITEMS: NavItem[] = [
  { label: "Dashboard", to: "/", hint: "Fleet overview and activity" },
  { label: "Agents", to: "/agents", hint: "Roster, roles, and status" },
  { label: "Evidence", to: "/evidence", hint: "Ledger records and claims" },
  { label: "Audit", to: "/audit", hint: "Swarm reports and findings" },
  { label: "Settings", to: "/settings", hint: "Preferences and connection" },
];

export const GLYPHS: Record<string, string> = {
  Dashboard: "◈",
  Agents: "⬡",
  Evidence: "▣",
  Audit: "◎",
  Settings: "⚙",
};
