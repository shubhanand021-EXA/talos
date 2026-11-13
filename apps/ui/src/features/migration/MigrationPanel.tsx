import { useEffect, useMemo, useState } from "react";
import type {
  MigrationCategory,
  MigrationFinding,
  MigrationPreviewItem,
} from "@talos/contracts";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  Modal,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import { useMigrationStore } from "../../stores/migrationStore.js";

const CATEGORIES: MigrationCategory[] = [
  "instructions",
  "skills",
  "schedules",
  "mcp-servers",
];

const CATEGORY_LABEL: Record<MigrationCategory, string> = {
  instructions: "Instructions (AGENTS.md / CLAUDE.md)",
  skills: "Skills (SKILL.md)",
  schedules: "Schedules (HEARTBEAT.md)",
  "mcp-servers": "MCP servers (.mcp.json / mcpServers)",
};

const KIND_LABEL: Record<MigrationFinding["kind"], string> = {
  agents: "AGENTS.md",
  claude: "CLAUDE.md",
  skill: "SKILL.md",
  heartbeat: "HEARTBEAT.md",
  "mcp-config": "MCP config",
  unknown: "Unknown format",
};

const STATE_TONE: Record<string, PillTone> = {
  quarantined: "warn",
  disabled: "neutral",
};

function RiskLabels({ labels }: { labels: string[] }) {
  if (labels.length === 0) {
    return <StatusPill tone="ok">clean</StatusPill>;
  }
  return (
    <span className="flex flex-wrap gap-1">
      {labels.map((label) => (
        <StatusPill key={label} tone="error">
          {label}
        </StatusPill>
      ))}
    </span>
  );
}

export function MigrationPanel() {
  const scan = useMigrationStore((state) => state.scan);
  const preview = useMigrationStore((state) => state.preview);
  const applyResult = useMigrationStore((state) => state.applyResult);
  const busy = useMigrationStore((state) => state.busy);
  const error = useMigrationStore((state) => state.error);
  const announcement = useMigrationStore((state) => state.announcement);
  const runScan = useMigrationStore((state) => state.runScan);
  const runPreview = useMigrationStore((state) => state.runPreview);
  const runApply = useMigrationStore((state) => state.runApply);
  const dismissError = useMigrationStore((state) => state.dismissError);
  const subscribeEvents = useMigrationStore((state) => state.subscribeEvents);

  const [workspacePath, setWorkspacePath] = useState("");
  const [approved, setApproved] = useState<Set<MigrationCategory>>(new Set());
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    void subscribeEvents();
  }, [subscribeEvents]);

  const grouped = useMemo(() => {
    const groups = new Map<MigrationFinding["kind"], MigrationFinding[]>();
    for (const finding of scan?.findings ?? []) {
      const list = groups.get(finding.kind) ?? [];
      list.push(finding);
      groups.set(finding.kind, list);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [scan]);

  const toggleCategory = (category: MigrationCategory) => {
    setApproved((current) => {
      const next = new Set(current);
      if (next.has(category)) {
        next.delete(category);
      } else {
        next.add(category);
      }
      return next;
    });
  };

  const previewByCategory = useMemo(() => {
    const groups = new Map<MigrationCategory, MigrationPreviewItem[]>();
    for (const item of preview?.items ?? []) {
      const list = groups.get(item.category) ?? [];
      list.push(item);
      groups.set(item.category, list);
    }
    return groups;
  }, [preview]);

  return (
    <GlassPanel label="Migration kit">
      <SectionHeader
        title="Import from other tools"
        subtitle="Scan → preview → per-category approve. Everything lands quarantined or disabled."
      />
      <p className="text-xs text-slate-400" role="note">
        Imported content is untrusted data: it never executes, never grants
        permissions, and nothing here enables anything. Enabling an imported
        skill, schedule, or MCP server is always a separate approval afterwards.
      </p>
      {announcement !== null ? (
        <p role="status" aria-live="polite" className="text-xs text-slate-300">
          {announcement}
        </p>
      ) : null}
      {error !== null ? (
        <p role="alert" className="text-xs text-rose-300">
          {error}{" "}
          <NeonButton variant="ghost" onClick={dismissError}>
            Dismiss
          </NeonButton>
        </p>
      ) : null}

      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void runScan(workspacePath);
        }}
      >
        <label
          className="flex min-w-0 flex-1 flex-col gap-1 text-xs text-slate-400"
          htmlFor="migration-workspace-path"
        >
          Workspace path to scan (read-only)
          <input
            id="migration-workspace-path"
            value={workspacePath}
            onChange={(event) => {
              setWorkspacePath(event.target.value);
            }}
            placeholder="/path/to/workspace"
            autoComplete="off"
            spellCheck={false}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1.5 font-mono text-sm text-slate-100"
          />
        </label>
        <NeonButton
          type="submit"
          disabled={busy || workspacePath.trim() === ""}
          loading={busy}
        >
          Scan
        </NeonButton>
        <NeonButton
          disabled={busy || scan === null}
          onClick={() => {
            void runPreview();
          }}
        >
          Preview import
        </NeonButton>
      </form>

      {busy ? <Spinner label="Working on migration step" /> : null}

      {scan === null && !busy ? (
        <EmptyState
          title="No scan yet"
          message="Enter a workspace path and run Scan. Detection covers AGENTS.md, CLAUDE.md, SKILL.md, HEARTBEAT.md, and MCP configs. Nothing executes."
        />
      ) : null}

      {grouped.length > 0 ? (
        <div className="flex flex-col gap-3">
          {grouped.map(([kind, findings]) => (
            <section key={kind} aria-label={`Findings: ${KIND_LABEL[kind]}`}>
              <h3 className="mb-1 font-display text-sm text-slate-200">
                {KIND_LABEL[kind]} · {findings.length}
              </h3>
              <DataTable<MigrationFinding>
                caption={`${KIND_LABEL[kind]} findings`}
                rows={findings}
                keyOf={(row) => row.sourcePath}
                columns={[
                  { header: "Source", render: (row) => row.sourcePath },
                  { header: "Category", render: (row) => row.category },
                  {
                    header: "Risk",
                    render: (row) => <RiskLabels labels={row.riskLabels} />,
                  },
                  {
                    header: "Secrets",
                    render: (row) =>
                      row.secretStripped ? (
                        <StatusPill tone="warn">stripped</StatusPill>
                      ) : (
                        <StatusPill tone="neutral">none found</StatusPill>
                      ),
                  },
                ]}
              />
            </section>
          ))}
        </div>
      ) : null}
      {scan !== null && scan.findings.length === 0 ? (
        <EmptyState
          title="No importable files"
          message="No instruction, skill, heartbeat, or MCP artifacts found in this workspace."
        />
      ) : null}

      {preview !== null ? (
        <section aria-label="Import preview">
          <h3 className="mb-1 font-display text-sm text-slate-200">
            Preview: what import would create ({preview.items.length} items)
          </h3>
          <div className="flex flex-col gap-3">
            {CATEGORIES.map((category) => {
              const items = previewByCategory.get(category) ?? [];
              if (items.length === 0) {
                return null;
              }
              return (
                <div key={category}>
                  <label className="mb-1 flex cursor-pointer items-center gap-2 text-xs text-slate-300">
                    <input
                      type="checkbox"
                      checked={approved.has(category)}
                      onChange={() => {
                        toggleCategory(category);
                      }}
                      aria-label={`Approve ${CATEGORY_LABEL[category]} for import`}
                    />
                    <span>
                      Approve {CATEGORY_LABEL[category]} ({items.length} items)
                    </span>
                  </label>
                  <DataTable
                    caption={`Preview: ${CATEGORY_LABEL[category]}`}
                    rows={items}
                    keyOf={(row) => row.target}
                    columns={[
                      { header: "Action", render: (row) => row.action },
                      { header: "Target", render: (row) => row.target },
                      {
                        header: "Default state",
                        render: (row) => (
                          <StatusPill
                            tone={STATE_TONE[row.defaultState] ?? "neutral"}
                          >
                            {row.defaultState}
                          </StatusPill>
                        ),
                      },
                      {
                        header: "Risk",
                        render: (row) => <RiskLabels labels={row.riskLabels} />,
                      },
                    ]}
                  />
                </div>
              );
            })}
          </div>
          <div className="mt-2">
            <NeonButton
              variant="primary"
              disabled={busy || approved.size === 0}
              onClick={() => {
                setConfirming(true);
              }}
            >
              Import approved categories
            </NeonButton>
          </div>
        </section>
      ) : null}

      {applyResult !== null ? (
        <section aria-label="Import result">
          <h3 className="mb-1 font-display text-sm text-slate-200">
            Import result
          </h3>
          <p role="status" className="text-xs text-slate-300">
            Imported {applyResult.created.length} artifacts
            (quarantined/disabled), skipped {applyResult.skipped.length}.
            Evidence: {applyResult.evidenceIds.join(", ") || "—"}. Re-running
            imports nothing new. Nothing was enabled — enabling stays a separate
            approval.
          </p>
        </section>
      ) : null}

      {confirming ? (
        <Modal
          title={`Import ${approved.size} categor${approved.size === 1 ? "y" : "ies"}?`}
          onClose={() => {
            setConfirming(false);
          }}
        >
          <div className="flex flex-col gap-3 text-sm text-slate-300">
            <p>
              This creates quarantined/disabled artifacts only. Imported content
              never executes, and nothing is enabled by this import — enabling
              stays a separate approval afterwards.
            </p>
            <ul className="flex list-disc flex-col gap-1 pl-5">
              {[...approved].map((category) => (
                <li key={category}>
                  {CATEGORY_LABEL[category]} (
                  {(previewByCategory.get(category) ?? []).length} items)
                </li>
              ))}
            </ul>
            <div className="flex gap-2">
              <NeonButton
                variant="primary"
                data-autofocus
                disabled={busy}
                onClick={() => {
                  setConfirming(false);
                  void runApply([...approved]);
                }}
              >
                Confirm import
              </NeonButton>
              <NeonButton
                variant="ghost"
                onClick={() => {
                  setConfirming(false);
                }}
              >
                Cancel
              </NeonButton>
            </div>
          </div>
        </Modal>
      ) : null}
    </GlassPanel>
  );
}
