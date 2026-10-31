/** Severity model: info notes, warnings advise, criticals fail the run. */
export type Severity = "info" | "warning" | "critical";

/** One check outcome inside an agent result. */
export interface Finding {
  severity: Severity;
  check: string;
  message: string;
  evidence: string;
}

/** Per-agent outcome. Status derives from the worst finding severity. */
export interface AgentResult {
  agent: string;
  status: "pass" | "warn" | "fail";
  findings: Finding[];
  durationMs: number;
}

/** Overall report status. */
export type ReportStatus = "PASS" | "WARN" | "FAIL";

/** Unified audit report (written as JSON + Markdown). */
export interface AuditReport {
  generatedAt: string;
  root: string;
  configVersion: number;
  status: ReportStatus;
  summary: {
    agents: number;
    passed: number;
    warned: number;
    failed: number;
    findings: number;
    critical: number;
    warnings: number;
    infos: number;
  };
  agents: AgentResult[];
}

/** Execution hook (injectable for hermetic tests). */
export interface ExecResult {
  ok: boolean;
  stdout: string;
  stderr: string;
  exitCode: number | null;
  timedOut: boolean;
}

export type ExecFn = (
  command: string,
  args: string[],
  options?: { timeoutMs?: number; cwd?: string },
) => ExecResult;

/** What every audit agent receives. */
export interface AuditContext {
  root: string;
  config: AuditConfig;
  exec: ExecFn;
}

/** A single audit agent: stable name + finding producer. */
export interface AuditAgent {
  name: string;
  run: (ctx: AuditContext) => Promise<Finding[]>;
}

/** Threshold + rule configuration (mirrors .talos/audit.config.json). */
export interface AuditConfig {
  version: number;
  thresholds: {
    maxDistBytes: number;
    maxMainJsGzipBytes: number;
    minPageBytes: number;
  };
  allowedLicenses: string[];
  forbiddenPatterns: string[];
  telemetryPatterns: string[];
  telemetryDeps: string[];
  allowedRemoteDomains: string[];
  heavyDependencies: Record<string, string>;
  suspiciousDependencies: string[];
  requiredFiles: string[];
  requiredScripts: string[];
  requiredUiScripts: string[];
  requiredRoutes: string[];
  requiredComponents: string[];
  forbiddenTopLevelDirs: string[];
  visionKeywords: string[];
  cargoTestTimeoutMs: number;
}
