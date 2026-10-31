import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { exitCodeFor, printSummary, runAudit, writeReports } from "./runner.ts";

function findRepoRoot(start: string): string {
  let dir = resolve(start);
  for (let i = 0; i < 6; i += 1) {
    if (existsSync(join(dir, ".talos", "audit.config.json"))) {
      return dir;
    }
    const parent = dirname(dir);
    if (parent === dir) {
      break;
    }
    dir = parent;
  }
  return resolve(start);
}

function resolveRoot(): string {
  const flag = process.argv.find((arg) => arg.startsWith("--root="));
  if (flag !== undefined) {
    return flag.slice("--root=".length);
  }
  const env = process.env["TALOS_ROOT"];
  if (typeof env === "string" && env !== "") {
    return env;
  }
  return findRepoRoot(process.cwd());
}

const root = resolveRoot();
const report = await runAudit(root);
printSummary(report);
const { md, json } = writeReports(report, join(root, "docs", "reports"));
console.log(`Wrote ${md}`);
console.log(`Wrote ${json}`);
process.exitCode = exitCodeFor(report.status);
