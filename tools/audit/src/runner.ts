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
  const ctx: AuditContext = { root, config, exec: defaultExec };
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
export function formatMarkdown(report: AuditReport): string {
  const failedChecks = report.agents.flatMap((a) =>
    a.findings
      .filter((f) => f.severity !== "info")
      .map(
        (f) =>
          `- [${f.severity}] ${a.agent} / ${f.check}: ${f.message} (${f.evidence})`,
      ),
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
    "",
    failedChecks.length > 0
      ? "## Failed checks & remediation"
      : "## Failed checks & remediation",
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
  mkdirSync(docsDir, { recursive: true });
  const date = report.generatedAt.slice(0, 10);
  const md = formatMarkdown(report);
  const json = `${JSON.stringify(report, null, 2)}\n`;
  const latestMd = join(docsDir, "audit-latest.md");
  const latestJson = join(docsDir, "audit-latest.json");
  writeFileSync(latestMd, md);
  writeFileSync(latestJson, json);
  writeFileSync(join(docsDir, `audit-${date}.md`), md);
  writeFileSync(join(docsDir, `audit-${date}.json`), json);
  return { md: latestMd, json: latestJson };
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
}
