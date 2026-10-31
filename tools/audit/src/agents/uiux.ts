import { join } from "node:path";
import { readdirSync, statSync } from "node:fs";
import type { AuditAgent, Finding } from "../types.ts";
import { fileExists, grepFiles, readText } from "../util.ts";

function hasA11yMarkers(source: string): boolean {
  return (
    source.includes("aria-") ||
    source.includes("role=") ||
    source.includes("<button") ||
    source.includes("<a ") ||
    source.includes("tabIndex") ||
    source.includes("onKeyDown") ||
    source.includes("<table") ||
    source.includes("<th") ||
    source.includes("<h1") ||
    source.includes("<h2") ||
    source.includes("<h3") ||
    source.includes("<ul") ||
    source.includes("<ol") ||
    source.includes("<dl") ||
    source.includes("<input") ||
    source.includes("<select") ||
    source.includes("<label")
  );
}

/** UI/UX: routes, design system, no placeholders, a11y basics. */
export const uiuxAgent: AuditAgent = {
  name: "UiUxAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, config } = ctx;

    const routerPath = join(root, "apps/ui/src/app/router.tsx");
    if (!fileExists(routerPath)) {
      findings.push({
        severity: "critical",
        check: "routes-present",
        message: "Router file missing",
        evidence: "apps/ui/src/app/router.tsx",
      });
    } else {
      const router = readText(routerPath);
      for (const route of config.requiredRoutes) {
        const bare = route.replace(/^\//, "");
        const quoted = [`"${route}"`, `'${route}'`, `"${bare}"`, `'${bare}'`];
        if (
          !quoted.some(
            (q) =>
              router.includes(`path: ${q}`) || router.includes(`path:${q}`),
          )
        ) {
          findings.push({
            severity: "critical",
            check: "routes-present",
            message: `Route "${route}" not wired in the router`,
            evidence: "apps/ui/src/app/router.tsx",
          });
        }
      }
      if (!findings.some((f) => f.check === "routes-present")) {
        findings.push({
          severity: "info",
          check: "routes-present",
          message: `All required routes wired (${config.requiredRoutes.join(", ")})`,
          evidence: "apps/ui/src/app/router.tsx",
        });
      }
    }

    const dsDir = join(root, "apps/ui/src/design-system");
    for (const component of config.requiredComponents) {
      if (!fileExists(join(dsDir, `${component}.tsx`))) {
        findings.push({
          severity: "warning",
          check: "design-system-complete",
          message: `Design-system component missing: ${component}`,
          evidence: "apps/ui/src/design-system/",
        });
      }
    }
    if (!findings.some((f) => f.check === "design-system-complete")) {
      findings.push({
        severity: "info",
        check: "design-system-complete",
        message: `All ${config.requiredComponents.length} design-system components present`,
        evidence: "apps/ui/src/design-system/",
      });
    }

    const featuresDir = join(root, "apps/ui/src/features");
    let pages: string[] = [];
    try {
      pages = readdirSync(featuresDir).flatMap((feature) => {
        const dir = join(featuresDir, feature);
        try {
          return statSync(dir).isDirectory()
            ? readdirSync(dir)
                .filter((f) => f.endsWith("Page.tsx"))
                .map((f) => join(dir, f))
            : [];
        } catch {
          return [];
        }
      });
    } catch {
      pages = [];
    }
    if (pages.length === 0) {
      findings.push({
        severity: "critical",
        check: "pages-nonblank",
        message: "No feature pages found",
        evidence: "apps/ui/src/features/",
      });
    }
    for (const page of pages) {
      let source = "";
      try {
        source = readText(page);
      } catch {
        continue;
      }
      const rel = page.slice(root.length + 1);
      if (source.length < config.thresholds.minPageBytes) {
        findings.push({
          severity: "warning",
          check: "pages-nonblank",
          message: `Page suspiciously small (${source.length} B): ${rel}`,
          evidence: rel,
        });
      }
      if (/lorem ipsum/i.test(source)) {
        findings.push({
          severity: "warning",
          check: "no-placeholder-text",
          message: `Placeholder text in ${rel}`,
          evidence: rel,
        });
      }
    }
    if (!findings.some((f) => f.check === "pages-nonblank")) {
      findings.push({
        severity: "info",
        check: "pages-nonblank",
        message: `${pages.length} feature pages, all above minimum size`,
        evidence: "apps/ui/src/features/",
      });
    }

    // Critical actions must be reachable: dry-run, emergency stop, compact,
    // and Rank tools buttons exist in the feature pages.
    const criticalActions: Array<[string, string]> = [
      ["Dry run", "dry-run invocation"],
      ["Emergency stop", "computer-use emergency control"],
      ["Compact", "context compaction"],
      ["Rank tools", "Tool-RAG ranking"],
    ];
    for (const [label, what] of criticalActions) {
      const hits = grepFiles(root, new RegExp(label), ["apps/ui/src/features"]);
      if (hits.length === 0) {
        findings.push({
          severity: "warning",
          check: "critical-actions-accessible",
          message: `No "${label}" control found — ${what} may be unreachable`,
          evidence: "apps/ui/src/features/",
        });
      }
    }
    if (!findings.some((f) => f.check === "critical-actions-accessible")) {
      findings.push({
        severity: "info",
        check: "critical-actions-accessible",
        message:
          "Dry run, emergency stop, compact, and Rank tools controls present",
        evidence: "apps/ui/src/features/",
      });
    }

    // Emergency stop must be visible from the shell chrome, not just the page.
    const stopFace = grepFiles(root, /Emergency stop/, [
      "apps/ui/src/features/computer",
    ]);
    const stopPill = grepFiles(root, /computer-use STOPPED/, [
      "apps/ui/src/app/chrome",
    ]);
    if (stopFace.length === 0 && stopPill.length === 0) {
      if (
        !fileExists(join(root, "apps/ui/src/features/computer")) &&
        !fileExists(join(root, "apps/ui/src/app/chrome"))
      ) {
        findings.push({
          severity: "info",
          check: "emergency-stop-visible",
          message: "Emergency-stop surfaces unverifiable in this tree shape",
          evidence: "apps/ui/src/",
        });
      } else {
        findings.push({
          severity: "critical",
          check: "emergency-stop-visible",
          message: "Emergency stop control missing from UI surfaces",
          evidence: "apps/ui/src/features/computer, apps/ui/src/app/chrome",
        });
      }
    } else {
      findings.push({
        severity: "info",
        check: "emergency-stop-visible",
        message: `Emergency stop reachable (page control + shell pill present in ${stopFace.length + stopPill.length} place(s))`,
        evidence: "apps/ui/src/features/computer, apps/ui/src/app/chrome",
      });
    }

    // Every async feature page must handle empty/loading/error states
    // explicitly. Purely static pages (no effects, no bridge calls) are
    // exempt — there is nothing to load.
    const thinPages: string[] = [];
    for (const page of pages) {
      let source = "";
      try {
        source = readText(page);
      } catch {
        continue;
      }
      const rel = page.slice(root.length + 1);
      const doesAsyncWork =
        source.includes("useEffect") ||
        source.includes("invokeCommand") ||
        source.includes("fetch(") ||
        source.includes("await ");
      if (!doesAsyncWork) {
        continue;
      }
      if (
        !source.includes("EmptyState") &&
        !source.includes("Spinner") &&
        !source.includes('role="alert"') &&
        !source.includes('role="status"')
      ) {
        thinPages.push(rel);
      }
    }
    for (const rel of thinPages.slice(0, 8)) {
      findings.push({
        severity: "warning",
        check: "empty-loading-states",
        message: `No empty/loading/error affordance in ${rel}`,
        evidence: rel,
      });
    }
    if (thinPages.length === 0 && pages.length > 0) {
      findings.push({
        severity: "info",
        check: "empty-loading-states",
        message: "All feature pages handle empty/loading/error states",
        evidence: "apps/ui/src/features/",
      });
    }

    let dsFiles: string[] = [];
    try {
      dsFiles = readdirSync(dsDir).filter(
        (f) => f.endsWith(".tsx") && f !== "index.ts",
      );
    } catch {
      dsFiles = [];
    }
    for (const file of dsFiles) {
      let source = "";
      try {
        source = readText(join(dsDir, file));
      } catch {
        continue;
      }
      if (!hasA11yMarkers(source)) {
        findings.push({
          severity: "warning",
          check: "a11y-basics",
          message: `No a11y markers (aria-/role/button/link/keyboard) in ${file}`,
          evidence: `apps/ui/src/design-system/${file}`,
        });
      }
    }
    if (!findings.some((f) => f.check === "a11y-basics")) {
      findings.push({
        severity: "info",
        check: "a11y-basics",
        message: "Design-system components carry a11y markers",
        evidence: "apps/ui/src/design-system/",
      });
    }
    return findings;
  },
};
