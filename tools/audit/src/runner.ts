import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { architectureAgent } from "./agents/architecture.ts";
import { dependenciesAgent } from "./agents/dependencies.ts";
import { evidenceAgent } from "./agents/evidence.ts";
import { performanceAgent } from "./agents/performance.ts";
import { privacyAgent } from "./agents/privacy.ts";
import { productAgent } from "./agents/product.ts";
import { reliabilityAgent } from "./agents/reliability.ts";
import { securityAgent } from "./agents/security.ts";
import { uiuxAgent } from "./agents/uiux.ts";
import {
  evaluateBaselines,
  loadBaselines,
  summarizeBaselines,
} from "./baselines.ts";
import type { Baselines } from "./baselines.ts";
import { loadConfig } from "./config.ts";
import type {
  AgentResult,
  AuditAgent,
  AuditContext,
  AuditReport,
  Finding,
  ReportStatus,
} from "./types.ts";
import { defaultExec } from "./util.ts";

export const AGENTS: AuditAgent[] = [
  securityAgent,
  privacyAgent,
  reliabilityAgent,
  performanceAgent,
  architectureAgent,
  evidenceAgent,
  dependenciesAgent,
  uiuxAgent,
  productAgent,
];

function statusOf(findings: Finding[]): AgentResult["status"] {
  if (findings.some((f) => f.severity === "critical")) {
    return "fail";
  }
  if (findings.some((f) => f.severity === "warning")) {
    return "warn";
  }
  return "pass";
}

/** Run all nine agents in a fixed order against `root`. */
export async function runAudit(root: string): Promise<AuditReport> {
  const config = loadConfig(root);
  const metrics: Record<string, number> = {};
  const ctx: AuditContext = {
    root,
    config,
    exec: defaultExec,
    record: (metric, value) => {
      metrics[metric] = value;
    },
  };
  const agents: AgentResult[] = [];
  for (const agent of AGENTS) {
    const started = Date.now();
    let findings: Finding[];
    try {
      findings = await agent.run(ctx);
    } catch (error) {
      findings = [
        {
          severity: "critical",
          check: "agent-crashed",
          message: `Agent threw instead of reporting: ${error instanceof Error ? error.message : String(error)}`,
          evidence: agent.name,
        },
      ];
    }
    agents.push({
      agent: agent.name,
      status: statusOf(findings),
      findings,
      durationMs: Date.now() - started,
    });
  }
  metrics["swarm.totalDurationMs"] = agents.reduce(
    (sum, a) => sum + a.durationMs,
    0,
  );
  // Baseline regression gates live with the performance agent: it owns all
  // budget/threshold findings, so WARN/FAIL attribution stays in one place.
  const baselines = loadBaselines(root);
  const baselineFindings = evaluateBaselines(metrics, baselines);
  const perf = agents.find((a) => a.agent === performanceAgent.name);
  if (perf !== undefined) {
    perf.findings.push(...baselineFindings);
    perf.status = statusOf(perf.findings);
  }
  const failed = agents.filter((a) => a.status === "fail").length;
  const warned = agents.filter((a) => a.status === "warn").length;
  const critical = agents
    .flatMap((a) => a.findings)
    .filter((f) => f.severity === "critical").length;
  const warnings = agents
    .flatMap((a) => a.findings)
    .filter((f) => f.severity === "warning").length;
  const infos = agents
    .flatMap((a) => a.findings)
    .filter((f) => f.severity === "info").length;
  const status: ReportStatus =
    failed > 0 ? "FAIL" : warned > 0 ? "WARN" : "PASS";
  return {
    generatedAt: new Date().toISOString(),
    root,
    configVersion: config.version,
    status,
    summary: {
      agents: agents.length,
      passed: agents.length - failed - warned,
      warned,
      failed,
      findings: critical + warnings + infos,
      critical,
      warnings,
      infos,
    },
    agents,
    metrics,
  };
}

/** Exit code contract: critical findings fail, warnings do not. */
export function exitCodeFor(status: ReportStatus): number {
  return status === "FAIL" ? 1 : 0;
}

function escMarkdown(text: string): string {
  return text.replace(/\|/g, "\\|");
}

function formatAgent(agent: AgentResult): string {
  const lines = [
    `### ${agent.agent} — ${agent.status.toUpperCase()} (${agent.durationMs}ms)`,
    "",
    "| Severity | Check | Message | Evidence |",
    "| -------- | ----- | ------- | -------- |",
  ];
  if (agent.findings.length === 0) {
    lines.push("| — | — | no findings | — |");
  }
  for (const f of agent.findings) {
    lines.push(
      `| ${f.severity} | ${escMarkdown(f.check)} | ${escMarkdown(f.message)} | ${escMarkdown(f.evidence)} |`,
    );
  }
  lines.push("");
  return lines.join("\n");
}

/** Render the human-readable Markdown report. */
export function formatMarkdown(
  report: AuditReport,
  baselines: Baselines | null = null,
): string {
  const failedChecks = report.agents.flatMap((a) =>
    a.findings
      .filter((f) => f.severity !== "info")
      .map(
        (f) =>
          `- [${f.severity}] ${a.agent} / ${f.check}: ${f.message} (${f.evidence})`,
      ),
  );
  const baselineRows = summarizeBaselines(report.metrics, baselines).map(
    (b) =>
      `| ${b.name} | ${b.value ?? "—"} | ${b.baseline ?? "—"} | ${b.unit} | ${b.status} |`,
  );
  return [
    "# Talos Audit Report",
    "",
    `- Generated: ${report.generatedAt}`,
    `- Root: \`${report.root}\``,
    `- Config version: ${report.configVersion}`,
    `- **Status: ${report.status}**`,
    "",
    "## Summary",
    "",
    `- Agents: ${report.summary.agents} (passed ${report.summary.passed}, warned ${report.summary.warned}, failed ${report.summary.failed})`,
    `- Findings: ${report.summary.findings} (critical ${report.summary.critical}, warnings ${report.summary.warnings}, info ${report.summary.infos})`,
    `- Measured metrics: ${Object.keys(report.metrics).length}`,
    "",
    "## Baselines",
    "",
    "| Metric | Measured | Baseline | Unit | Gate |",
    "| ------ | -------- | -------- | ---- | ---- |",
    ...(baselineRows.length > 0
      ? baselineRows
      : ["| — | no metrics recorded | — |"]),
    "",
    "Gates live in `.talos/baselines.json` (`baseline-ok` info, `baseline-regression` warning/critical).",
    "",
    "## Failed checks & remediation",
    "",
    ...(failedChecks.length > 0
      ? failedChecks
      : ["None — every check is informational."]),
    "",
    ...report.agents.flatMap((a) => [formatAgent(a)]),
  ].join("\n");
}

/** Write latest + dated report copies under `docs/reports/`. */
export function writeReports(
  report: AuditReport,
  docsDir: string,
): { md: string; json: string } {
  const baselines = loadBaselines(report.root);
  mkdirSync(docsDir, { recursive: true });
  const date = report.generatedAt.slice(0, 10);
  const md = formatMarkdown(report, baselines);
  const json = `${JSON.stringify(report, null, 2)}\n`;
  const latestMd = join(docsDir, "audit-latest.md");
  const latestJson = join(docsDir, "audit-latest.json");
  writeFileSync(latestMd, md);
  writeFileSync(latestJson, json);
  writeFileSync(join(docsDir, `audit-${date}.md`), md);
  writeFileSync(join(docsDir, `audit-${date}.json`), json);
  syncPublicSnapshot(report.root, json);
  return { md: latestMd, json: latestJson };
}

/**
 * Best-effort dev-server copy of the latest report JSON for the Audit page
 * (`apps/ui/public/reports/audit-latest.json`, gitignored). Never fails the
 * audit: the UI falls back to the committed `docs/reports` snapshot.
 */
function syncPublicSnapshot(root: string, json: string): void {
  try {
    const dir = join(root, "apps", "ui", "public", "reports");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "audit-latest.json"), json);
  } catch {
    // Dev convenience only — the committed docs/reports copy is canonical.
  }
}

/** One-line console summary per agent. */
export function printSummary(report: AuditReport): void {
  console.log(
    `Talos audit: ${report.status} — ${report.summary.passed}/${report.summary.agents} agents clean`,
  );
  for (const agent of report.agents) {
    const critical = agent.findings.filter(
      (f) => f.severity === "critical",
    ).length;
    const warnings = agent.findings.filter(
      (f) => f.severity === "warning",
    ).length;
    console.log(
      `  [${agent.status.toUpperCase()}] ${agent.agent} (critical ${critical}, warnings ${warnings})`,
    );
  }
  const measured = Object.keys(report.metrics);
  if (measured.length > 0) {
    console.log(
      `  metrics: ${measured
        .sort()
        .map((m) => `${m}=${report.metrics[m]}`)
        .join(", ")}`,
    );
  }
}
