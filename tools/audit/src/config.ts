import { join } from "node:path";
import type { AuditConfig } from "./types.ts";
import { fileExists, readJson } from "./util.ts";

/** Built-in defaults; the repo config overrides field-by-field. */
export const DEFAULT_CONFIG: AuditConfig = {
  version: 1,
  thresholds: {
    maxDistBytes: 6291456,
    maxMainJsGzipBytes: 524288,
    minPageBytes: 500,
  },
  allowedLicenses: ["MIT", "Apache-2.0", "ISC"],
  forbiddenPatterns: [],
  telemetryPatterns: [],
  telemetryDeps: [],
  allowedRemoteDomains: ["localhost", "127.0.0.1"],
  heavyDependencies: {},
  suspiciousDependencies: [],
  requiredFiles: [],
  requiredScripts: [],
  requiredUiScripts: [],
  requiredRoutes: [],
  requiredComponents: [],
  forbiddenTopLevelDirs: [],
  visionKeywords: [],
  featureKeywords: [],
  cargoTestTimeoutMs: 180000,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Load `.talos/audit.config.json` and validate its shape. */
export function loadConfig(root: string): AuditConfig {
  const path = join(root, ".talos", "audit.config.json");
  if (!fileExists(path)) {
    throw new Error(`audit config not found: ${path}`);
  }
  const raw: unknown = readJson(path);
  if (!isRecord(raw) || typeof raw["version"] !== "number") {
    throw new Error(`audit config invalid: ${path} (missing numeric version)`);
  }
  if (!isRecord(raw["thresholds"])) {
    throw new Error(`audit config invalid: ${path} (missing thresholds)`);
  }
  const merged: AuditConfig = {
    ...DEFAULT_CONFIG,
    ...(raw as Partial<AuditConfig>),
    thresholds: {
      ...DEFAULT_CONFIG.thresholds,
      ...((raw as { thresholds?: object }).thresholds as object),
    },
  };
  const lists: Array<keyof AuditConfig> = [
    "allowedLicenses",
    "forbiddenPatterns",
    "telemetryPatterns",
    "telemetryDeps",
    "allowedRemoteDomains",
    "suspiciousDependencies",
    "requiredFiles",
    "requiredScripts",
    "requiredUiScripts",
    "requiredRoutes",
    "requiredComponents",
    "forbiddenTopLevelDirs",
    "visionKeywords",
    "featureKeywords",
  ];
  for (const key of lists) {
    const value: unknown = (merged as unknown as Record<string, unknown>)[
      key as string
    ];
    if (
      !Array.isArray(value) ||
      !value.every((item) => typeof item === "string")
    ) {
      throw new Error(
        `audit config invalid: ${path} (${String(key)} must be string[])`,
      );
    }
  }
  return merged;
}
