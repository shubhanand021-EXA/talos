import { createHashRouter } from "react-router-dom";
import { Shell } from "./Shell.js";
import { AgentsPage } from "../features/agents/AgentsPage.js";
import { AuditPage } from "../features/audit/AuditPage.js";
import { BrowserPage } from "../features/browser/BrowserPage.js";
import { ComputerPage } from "../features/computer/ComputerPage.js";
import { ContextPage } from "../features/context/ContextPage.js";
import { DashboardPage } from "../features/dashboard/DashboardPage.js";
import { EvidencePage } from "../features/evidence/EvidencePage.js";
import { SettingsPage } from "../features/settings/SettingsPage.js";
import { ToolsPage } from "../features/tools/ToolsPage.js";

// Hash routing: works identically over http(s) and the Tauri asset
// protocol, where no server rewrite exists for history routes.
export const router = createHashRouter([
  {
    path: "/",
    element: <Shell />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: "agents", element: <AgentsPage /> },
      { path: "tools", element: <ToolsPage /> },
      { path: "browser", element: <BrowserPage /> },
      { path: "computer", element: <ComputerPage /> },
      { path: "context", element: <ContextPage /> },
      { path: "evidence", element: <EvidencePage /> },
      { path: "audit", element: <AuditPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);
