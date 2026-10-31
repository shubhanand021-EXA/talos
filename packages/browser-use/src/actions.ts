import type { BrowserAction, HighRiskKind } from "@talos/contracts";

/** Map a browser action to its tool-runtime tool id. */
export function toolIdForAction(action: BrowserAction): string {
  switch (action) {
    case "navigate":
      return "browser.navigate";
    case "click":
      return "browser.click";
    case "type":
      return "browser.type";
    case "scroll":
      return "browser.scroll";
    case "select":
      return "browser.select";
    case "extract_text":
      return "browser.extract";
    case "screenshot":
      return "browser.screenshot";
    case "snapshot_dom":
      return "browser.snapshot";
  }
}

/** High-risk categories that require explicit human approval. */
export const HIGH_RISK_KINDS: HighRiskKind[] = [
  "form_submit",
  "purchase",
  "settings_change",
  "file_upload",
  "file_download",
  "destructive",
  "other",
];

/** True when an action needs an explicit human-approval flag to run live. */
export function requiresApproval(
  action: BrowserAction,
  highRisk?: boolean,
): boolean {
  if (highRisk === true) {
    return true;
  }
  // Mutations that can submit state default to approval-gated live runs.
  return action === "click" || action === "type" || action === "select";
}
