import type { AuditAgent, Finding } from "../types.ts";

/** Parity matrix + charter gate: the constitution's measurable half.
 * Runs the dependency-free validator (schema, evidence existence,
 * deterministic SUMMARY.md, competitor-cells-unknown, license/talos.dev/
 * reports/FLAKES charter checks). Any violation is critical: drift or an
 * invented competitor cell must fail the run, never warn-and-continue. */
export const parityAgent: AuditAgent = {
  name: "ParityAuditAgent",
  run: async (ctx) => {
    const findings: Finding[] = [];
    const { root, exec, record } = ctx;
    const started = Date.now();
    const result = exec(
      "node",
      ["scripts/parity/validate-matrix.mjs", "--check"],
      { timeoutMs: 120_000, cwd: root },
    );
    record("swarm.parityMs", Date.now() - started);
    const output = `${result.stdout}\n${result.stderr}`.trim().slice(-800);
    if (result.ok) {
      findings.push({
        severity: "info",
        check: "parity-matrix-green",
        message:
          "Parity matrix validates: schema, evidence, SUMMARY.md deterministic, charter checks green",
        evidence: output || "validate-matrix.mjs exit 0",
      });
    } else {
      findings.push({
        severity: "critical",
        check: "parity-matrix-green",
        message: `Parity/charter gate failed (drift or invented data): ${output}`,
        evidence: "scripts/parity/validate-matrix.mjs --check",
      });
    }
    return findings;
  },
};
