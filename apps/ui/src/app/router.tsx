import { createBrowserRouter } from "react-router-dom";
import { Shell } from "./Shell.js";
import { AgentsPage } from "../features/agents/AgentsPage.js";
import { AuditPage } from "../features/audit/AuditPage.js";
import { DashboardPage } from "../features/dashboard/DashboardPage.js";
import { EvidencePage } from "../features/evidence/EvidencePage.js";
import { SettingsPage } from "../features/settings/SettingsPage.js";

export const router = createBrowserRouter([
  {
    path: "/",
    element: <Shell />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: "agents", element: <AgentsPage /> },
      { path: "evidence", element: <EvidencePage /> },
      { path: "audit", element: <AuditPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);
