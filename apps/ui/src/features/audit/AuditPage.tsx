import {
  GlassPanel,
  KeyValueList,
  SectionHeader,
  StatusPill,
} from "../../design-system/index.js";

export function AuditPage() {
  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Audit"
        subtitle="9-agent swarm baseline — deterministic summary"
      />
      <div className="grid grid-cols-1 gap-4 2xl:grid-cols-2">
        <GlassPanel label="Latest report">
          <div className="flex items-center justify-between">
            <h3 className="font-display text-sm font-semibold">
              Batch 1 green baseline
            </h3>
            <StatusPill tone="warn">warning</StatusPill>
          </div>
          <p className="mt-2 text-sm text-slate-400">
            Type, test, and hygiene gates pass. One advisory: test suite exceeds
            the future time budget — tracked, not blocking.
          </p>
          <div className="mt-3">
            <KeyValueList
              entries={[
                { key: "passed", value: "8" },
                { key: "failed", value: "1" },
                { key: "findings", value: "3" },
                { key: "auditor", value: "audit-swarm/baseline" },
              ]}
            />
          </div>
        </GlassPanel>
        <GlassPanel label="Swarm status">
          <SectionHeader
            title="Swarm agents"
            subtitle="Full orchestration arrives later"
          />
          <KeyValueList
            entries={[
              { key: "security", value: "scheduled" },
              { key: "dependencies", value: "passing" },
              { key: "types", value: "passing" },
              { key: "evidence", value: "passing" },
            ]}
          />
        </GlassPanel>
      </div>
    </div>
  );
}
