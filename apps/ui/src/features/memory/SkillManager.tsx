import { useCallback, useState } from "react";
import {
  DataTable,
  EmptyState,
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import {
  archiveSkill,
  deprecateSkill,
  diffSkillVersions,
  getSkill,
  getSkillStats,
  isBackendUnavailable,
  listSkillVersions,
  publishSkill,
  searchMemory,
  triggerSkillEvaluation,
  validateSkill,
  type Skill,
  type SkillDiffLine,
  type SkillStats,
} from "../../bridge/index.js";
import { useAppStore } from "../../stores/appStore.js";

type SkillStatus = Skill["status"];

function toneForStatus(
  status: SkillStatus,
): "ok" | "info" | "warn" | "neutral" {
  switch (status) {
    case "published":
      return "ok";
    case "validated":
      return "info";
    case "quarantined":
      return "warn";
    default:
      return "neutral";
  }
}

function percent(rate: number): string {
  return `${Math.round(rate * 100)}%`;
}

/** GEPA Skill Manager: lifecycle actions, stats, versions, diffs, search. */
export function SkillManager({
  skills,
  onRefresh,
}: {
  skills: Skill[];
  onRefresh: () => Promise<void>;
}) {
  const appendFeed = useAppStore((state) => state.appendFeed);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<Skill | null>(null);
  const [stats, setStats] = useState<SkillStats | null>(null);
  const [versions, setVersions] = useState<Skill[]>([]);
  const [diffFrom, setDiffFrom] = useState("");
  const [diffTo, setDiffTo] = useState("");
  const [diffLines, setDiffLines] = useState<SkillDiffLine[] | null>(null);
  const [searchQuery, setSearchQuery] = useState("read file");
  const [searchHits, setSearchHits] = useState<
    Array<{ skillId: string; score: number; snippet: string }>
  >([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const fail = useCallback((error: unknown, fallback: string) => {
    setNotice(
      isBackendUnavailable(error)
        ? "Backend unavailable — launch the desktop shell."
        : error instanceof Error
          ? error.message
          : fallback,
    );
  }, []);

  const selectSkill = useCallback(
    async (skillId: string) => {
      setSelectedId(skillId);
      setBusy("detail");
      setNotice(null);
      try {
        const [doc, skillStats, history] = await Promise.all([
          getSkill(skillId),
          getSkillStats(skillId),
          listSkillVersions(skillId),
        ]);
        setDetail(doc);
        setStats(skillStats);
        setVersions(history);
        setDiffFrom(history[0]?.id ?? skillId);
        setDiffTo(history[history.length - 1]?.id ?? skillId);
        setDiffLines(null);
      } catch (error) {
        fail(error, "Skill detail unavailable");
      } finally {
        setBusy(null);
      }
    },
    [fail],
  );

  const runAction = useCallback(
    async (action: string, skillId: string) => {
      setBusy(action);
      setNotice(null);
      try {
        const updated =
          action === "validate"
            ? await validateSkill(skillId)
            : action === "publish"
              ? await publishSkill(skillId)
              : action === "deprecate"
                ? await deprecateSkill(skillId)
                : await archiveSkill(skillId);
        appendFeed({
          id: `skill-${action}-${skillId}-${Date.now()}`,
          at: new Date().toISOString(),
          actor: "skill-manager",
          message: `Skill ${updated.id} → ${updated.status}`,
          tone: "success",
        });
        await onRefresh();
        await selectSkill(skillId);
      } catch (error) {
        fail(error, `Skill ${action} failed`);
      } finally {
        setBusy(null);
      }
    },
    [appendFeed, fail, onRefresh, selectSkill],
  );

  const runDiff = useCallback(async () => {
    if (diffFrom === "" || diffTo === "") {
      return;
    }
    setBusy("diff");
    setNotice(null);
    try {
      setDiffLines(await diffSkillVersions(diffFrom, diffTo));
    } catch (error) {
      fail(error, "Skill diff failed");
    } finally {
      setBusy(null);
    }
  }, [diffFrom, diffTo, fail]);

  const runSkillSearch = useCallback(async () => {
    setBusy("skill-search");
    setNotice(null);
    try {
      const hits = await searchMemory({
        query: searchQuery,
        topK: 5,
        sourceTypes: ["skill"],
        mode: "semantic",
      });
      setSearchHits(
        hits.map((hit) => ({
          skillId: hit.sourceId,
          score: hit.score,
          snippet: hit.snippet,
        })),
      );
    } catch (error) {
      fail(error, "Skill search failed");
    } finally {
      setBusy(null);
    }
  }, [fail, searchQuery]);

  const runEvaluateNow = useCallback(async () => {
    setBusy("evaluate");
    setNotice(null);
    try {
      const trigger = await triggerSkillEvaluation();
      setNotice(
        trigger.due
          ? `GEPA evaluation started in the background (${trigger.receiptCount} receipts observed).`
          : `GEPA cadence not due (${trigger.receiptCount} receipts observed).`,
      );
      appendFeed({
        id: `gepa-trigger-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "skill-manager",
        message: `GEPA evaluation ${trigger.due ? "started" : "skipped (not due)"}`,
        tone: "info",
      });
      await onRefresh();
    } catch (error) {
      fail(error, "Evaluation trigger failed");
    } finally {
      setBusy(null);
    }
  }, [appendFeed, fail, onRefresh]);

  const actionsFor = (status: SkillStatus): string[] => {
    switch (status) {
      case "quarantined":
        return ["validate", "archive"];
      case "validated":
        return ["publish", "archive"];
      case "published":
        return ["deprecate", "archive"];
      case "deprecated":
        return ["archive"];
      default:
        return [];
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <GlassPanel label="Skill Manager">
        <SectionHeader
          title="Skill Manager"
          subtitle={`${skills.length} skills · GEPA learning loop: extract → quarantine → validate → publish → evolve`}
          action={
            <NeonButton
              onClick={() => {
                void runEvaluateNow();
              }}
              loading={busy === "evaluate"}
            >
              Evaluate Now
            </NeonButton>
          }
        />
        {notice !== null ? (
          <p role="status" className="mt-2 text-xs text-amber-300">
            {notice}
          </p>
        ) : null}
        {skills.length === 0 ? (
          <EmptyState
            title="No skills yet"
            message="Quarantined skills are proposed from completed tasks with verified claims."
          />
        ) : (
          <DataTable<Skill>
            caption="Skill registry"
            rows={skills}
            keyOf={(row) => row.id}
            onRowSelect={(row) => {
              void selectSkill(row.id);
            }}
            columns={[
              { header: "Skill", render: (row) => row.name },
              { header: "Version", render: (row) => row.version },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill tone={toneForStatus(row.status)}>
                    {row.status}
                  </StatusPill>
                ),
              },
              {
                header: "Uses",
                render: (row) => String(row.usageCount),
              },
              {
                header: "Success",
                render: (row) =>
                  row.usageCount === 0
                    ? "—"
                    : percent(row.successCount / row.usageCount),
              },
              {
                header: "Confidence",
                render: (row) => percent(row.confidenceScore),
              },
            ]}
          />
        )}
      </GlassPanel>

      {selectedId !== null ? (
        <GlassPanel label="Skill detail">
          {busy === "detail" || detail === null ? (
            <Spinner label="Loading skill…" />
          ) : (
            <div className="flex flex-col gap-3">
              <SectionHeader
                title={`${detail.name} (v${detail.version})`}
                subtitle={`${detail.status} · ceiling ${detail.permissionCeiling} · tools ${detail.requiredTools.join(", ") || "—"}`}
              />
              <p className="text-sm text-slate-200">{detail.description}</p>
              <pre
                aria-label="Skill document"
                className="max-h-64 overflow-auto rounded-md border border-white/10 bg-black/40 p-3 font-mono text-xs text-slate-200"
              >
                {detail.procedureText}
              </pre>
              {stats !== null ? (
                <p className="font-mono text-xs text-slate-400">
                  {stats.usageCount} uses · {stats.successCount} success ·{" "}
                  {stats.failureCount} failure · rate{" "}
                  {percent(stats.successRate)} · confidence{" "}
                  {percent(stats.confidenceScore)}
                  {stats.needsReview ? " · needs review" : ""}
                  {stats.lastUsedAt !== null && stats.lastUsedAt !== undefined
                    ? ` · last used ${stats.lastUsedAt}`
                    : ""}
                </p>
              ) : null}
              <div className="flex flex-wrap gap-2">
                {actionsFor(detail.status).map((action) => (
                  <NeonButton
                    key={action}
                    onClick={() => {
                      void runAction(action, detail.id);
                    }}
                    loading={busy === action}
                  >
                    {action[0]?.toUpperCase()}
                    {action.slice(1)}
                  </NeonButton>
                ))}
              </div>
              {versions.length > 1 ? (
                <div className="flex flex-col gap-2">
                  <p className="text-xs text-slate-400">
                    Version history:{" "}
                    {versions
                      .map((version) => `v${version.version}`)
                      .join(" → ")}
                  </p>
                  <div className="flex flex-wrap items-center gap-2">
                    <select
                      aria-label="Diff from version"
                      value={diffFrom}
                      onChange={(event) => {
                        setDiffFrom(event.target.value);
                      }}
                      className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
                    >
                      {versions.map((version) => (
                        <option key={version.id} value={version.id}>
                          v{version.version}
                        </option>
                      ))}
                    </select>
                    <span className="text-xs text-slate-500">→</span>
                    <select
                      aria-label="Diff to version"
                      value={diffTo}
                      onChange={(event) => {
                        setDiffTo(event.target.value);
                      }}
                      className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
                    >
                      {versions.map((version) => (
                        <option key={version.id} value={version.id}>
                          v{version.version}
                        </option>
                      ))}
                    </select>
                    <NeonButton
                      onClick={() => {
                        void runDiff();
                      }}
                      loading={busy === "diff"}
                    >
                      Show diff
                    </NeonButton>
                  </div>
                  {diffLines !== null ? (
                    <div
                      aria-label="Version diff"
                      className="max-h-48 overflow-auto rounded-md border border-white/10 bg-black/40 p-3 font-mono text-xs"
                    >
                      {diffLines.map((line, index) => (
                        <p
                          key={`${index}-${line.kind}`}
                          className={
                            line.kind === "added"
                              ? "text-emerald-300"
                              : line.kind === "removed"
                                ? "text-rose-300"
                                : "text-slate-400"
                          }
                        >
                          {line.kind === "added"
                            ? "+ "
                            : line.kind === "removed"
                              ? "- "
                              : "  "}
                          {line.text}
                        </p>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>
          )}
        </GlassPanel>
      ) : null}

      <GlassPanel label="Skill search">
        <div className="mb-3 flex flex-wrap gap-2">
          <input
            aria-label="Skill search query"
            value={searchQuery}
            onChange={(event) => {
              setSearchQuery(event.target.value);
            }}
            className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <NeonButton
            onClick={() => {
              void runSkillSearch();
            }}
            loading={busy === "skill-search"}
          >
            Search skills
          </NeonButton>
        </div>
        {searchHits.length === 0 ? (
          <EmptyState
            title="No skill hits yet"
            message="Semantic recall over the skill index (vector_store)."
          />
        ) : (
          <div className="flex flex-col gap-2" aria-label="Skill results">
            {searchHits.map((hit) => (
              <button
                key={hit.skillId}
                type="button"
                onClick={() => {
                  void selectSkill(hit.skillId);
                }}
                className="rounded-md border border-white/10 bg-black/30 p-3 text-left hover:bg-white/5"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono text-xs text-slate-400">
                    {hit.skillId}
                  </span>
                  <span className="font-mono text-xs text-cyan-300">
                    {hit.score.toFixed(2)}
                  </span>
                </div>
                <p className="mt-1 text-sm text-slate-200">{hit.snippet}</p>
              </button>
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
