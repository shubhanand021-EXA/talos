import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
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
  getIndexStatus,
  isBackendUnavailable,
  isTauri,
  listSkills,
  rebuildIndex,
  searchMemory,
  type IndexStatus,
  type MemorySearchResult,
  type Skill,
} from "../../bridge/index.js";
import { getMockActivity } from "../../stores/mockStore.js";
import { useAppStore } from "../../stores/appStore.js";
import { useUiStore } from "../../stores/uiStore.js";

type LoadState =
  | { phase: "idle" }
  | { phase: "loading" }
  | { phase: "ready" }
  | { phase: "error"; message: string };

const SOURCE_FILTERS = ["claim", "evidence", "task", "skill"] as const;
const MODES = ["hybrid", "lexical", "semantic"] as const;

function toneFor(source: string): "ok" | "info" | "warn" | "neutral" {
  switch (source) {
    case "claim":
      return "ok";
    case "skill":
      return "info";
    case "task":
      return "warn";
    default:
      return "neutral";
  }
}

function LiveMemoryPage() {
  const selectEntity = useUiStore((state) => state.selectEntity);
  const appendFeed = useAppStore((state) => state.appendFeed);
  const [state, setState] = useState<LoadState>({ phase: "loading" });
  const [query, setQuery] = useState("read manifest hash");
  const [mode, setMode] = useState<(typeof MODES)[number]>("hybrid");
  const [sources, setSources] = useState<ReadonlySet<string>>(
    new Set(SOURCE_FILTERS),
  );
  const [verifiedOnly, setVerifiedOnly] = useState(false);
  const [results, setResults] = useState<MemorySearchResult[]>([]);
  const [status, setStatus] = useState<IndexStatus | null>(null);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const fail = useCallback((error: unknown, fallback: string) => {
    if (isBackendUnavailable(error)) {
      setNotice("Backend unavailable — launch the desktop shell.");
    } else {
      setNotice(error instanceof Error ? error.message : fallback);
    }
    setState({ phase: "error", message: fallback });
  }, []);

  const applyReady = useCallback(() => {
    setState({ phase: "ready" });
  }, []);

  const applyError = useCallback((message: string) => {
    setState({ phase: "error", message });
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const [index, registry] = await Promise.all([
          getIndexStatus(),
          listSkills(),
        ]);
        if (cancelled) {
          return;
        }
        setStatus(index);
        setSkills(registry);
        applyReady();
      } catch (error) {
        if (cancelled) {
          return;
        }
        setNotice(
          error instanceof Error ? error.message : "Index status unavailable",
        );
        applyError("Index status unavailable");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [applyReady, applyError]);

  const runSearch = async () => {
    setBusy("search");
    setNotice(null);
    try {
      const hits = await searchMemory({
        query,
        topK: 10,
        sourceTypes:
          sources.size === SOURCE_FILTERS.length
            ? undefined
            : ([...sources] as Array<"claim" | "evidence" | "task" | "skill">),
        verifiedOnly: verifiedOnly || undefined,
        mode,
      });
      setResults(hits);
      appendFeed({
        id: `memory-search-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "memory-indexer",
        message: `Memory search (${mode}): ${hits.length} hits`,
        tone: "info",
      });
    } catch (error) {
      fail(error, "Memory search failed");
    } finally {
      setBusy(null);
    }
  };

  const runRebuild = async () => {
    setBusy("rebuild");
    setNotice(null);
    try {
      const index = await rebuildIndex();
      setStatus(index);
      const registry = await listSkills();
      setSkills(registry);
      appendFeed({
        id: `memory-rebuild-${Date.now()}`,
        at: new Date().toISOString(),
        actor: "memory-indexer",
        message: `Memory index rebuilt: ${index.entries} entries`,
        tone: "success",
      });
    } catch (error) {
      fail(error, "Rebuild failed");
    } finally {
      setBusy(null);
    }
  };

  const toggleSource = (source: string) => {
    setSources((current) => {
      const next = new Set(current);
      if (next.has(source)) {
        next.delete(source);
      } else {
        next.add(source);
      }
      return next;
    });
  };

  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Memory"
        subtitle="Semantic recall over claims, evidence, tasks, and skills — local embeddings, hybrid rerank"
      />
      {notice !== null ? (
        <p role="status" className="text-xs text-amber-300">
          {notice}
        </p>
      ) : null}
      <GlassPanel label="Index status">
        {state.phase === "loading" ? (
          <Spinner label="Loading index status…" />
        ) : status === null ? (
          <EmptyState
            title="No index status"
            message="Launch the desktop shell, then rebuild the index."
          />
        ) : (
          <div className="flex flex-col gap-2">
            <p className="text-sm text-slate-200">
              {status.entries} entries · {status.backend} ·{" "}
              {status.modelLoaded ? "model loaded" : "hash fallback"}
            </p>
            <p className="font-mono text-xs text-slate-500">
              {status.bySourceType
                .map((row) => `${row.sourceType}:${row.count}`)
                .join(" · ")}
              {status.lastIndexedAt !== null &&
              status.lastIndexedAt !== undefined
                ? ` · indexed ${status.lastIndexedAt}`
                : ""}
            </p>
            {status.warning !== "" ? (
              <p className="text-xs text-slate-500">{status.warning}</p>
            ) : null}
            <div>
              <NeonButton
                onClick={() => {
                  void runRebuild();
                }}
                loading={busy === "rebuild"}
              >
                Rebuild Index
              </NeonButton>
            </div>
          </div>
        )}
      </GlassPanel>
      <GlassPanel label="Semantic search">
        <div className="mb-3 flex flex-wrap gap-2">
          <input
            aria-label="Memory query"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
            }}
            className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
          <select
            aria-label="Retrieval mode"
            value={mode}
            onChange={(event) => {
              setMode(event.target.value as typeof mode);
            }}
            className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          >
            {MODES.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <NeonButton
            onClick={() => {
              void runSearch();
            }}
            loading={busy === "search"}
          >
            Search memory
          </NeonButton>
        </div>
        <div className="mb-3 flex flex-wrap items-center gap-3 text-xs">
          {SOURCE_FILTERS.map((source) => (
            <label key={source} className="flex items-center gap-1">
              <input
                type="checkbox"
                checked={sources.has(source)}
                onChange={() => {
                  toggleSource(source);
                }}
              />
              {source}
            </label>
          ))}
          <label className="flex items-center gap-1">
            <input
              type="checkbox"
              checked={verifiedOnly}
              onChange={(event) => {
                setVerifiedOnly(event.target.checked);
              }}
            />
            verified only
          </label>
        </div>
        {results.length === 0 ? (
          <EmptyState
            title="No results yet"
            message="Search the ledger — hybrid lexical + semantic recall with rerank."
          />
        ) : (
          <div className="flex flex-col gap-2" aria-label="Memory results">
            {results.map((hit) => (
              <button
                key={hit.id}
                type="button"
                onClick={() => {
                  selectEntity({
                    kind: hit.sourceType === "evidence" ? "evidence" : "system",
                    id: hit.sourceId,
                    label: `${hit.sourceType}:${hit.sourceId}`,
                    detail: hit.reason,
                  });
                }}
                className="rounded-md border border-white/10 bg-black/30 p-3 text-left hover:bg-white/5"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono text-xs text-slate-400">
                    {hit.sourceType}:{hit.sourceId}
                  </span>
                  <span className="flex items-center gap-2">
                    <StatusPill tone={toneFor(hit.sourceType)}>
                      {hit.sourceType}
                    </StatusPill>
                    <span className="font-mono text-xs text-cyan-300">
                      {hit.score.toFixed(2)}
                    </span>
                    {hit.verified ? (
                      <StatusPill tone="ok">verified</StatusPill>
                    ) : null}
                  </span>
                </div>
                <p className="mt-1 text-sm text-slate-200">{hit.snippet}</p>
                <p className="mt-1 font-mono text-[11px] text-slate-500">
                  {hit.reason}
                </p>
              </button>
            ))}
          </div>
        )}
      </GlassPanel>
      <GlassPanel label="Skill registry">
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
              selectEntity({
                kind: "system",
                id: row.id,
                label: row.name,
                detail: `${row.status} · ${row.permissionCeiling} · ${row.requiredTools.join(", ")}`,
              });
            }}
            columns={[
              { header: "Skill", render: (row) => row.name },
              { header: "Version", render: (row) => row.version },
              {
                header: "Status",
                render: (row) => (
                  <StatusPill
                    tone={
                      row.status === "published"
                        ? "ok"
                        : row.status === "validated"
                          ? "info"
                          : "warn"
                    }
                  >
                    {row.status}
                  </StatusPill>
                ),
              },
              {
                header: "Source task",
                render: (row) => (
                  <span className="font-mono text-xs">
                    {row.sourceTaskId ?? "—"}
                  </span>
                ),
              },
            ]}
          />
        )}
      </GlassPanel>
    </div>
  );
}

function MockMemoryPage() {
  const activity = getMockActivity();
  return (
    <div className="flex flex-col gap-4">
      <SectionHeader
        title="Memory"
        subtitle="Semantic recall — mock snapshot (launch the desktop shell for the live index)"
      />
      <GlassPanel label="Mock index">
        <p className="font-mono text-xs text-slate-500">
          backend local-hash-384/1 · 3 mock entries · model fallback active
        </p>
        <ul className="mt-2 flex flex-col gap-1 text-sm text-slate-300">
          {activity.slice(0, 3).map((event) => (
            <li key={event.id}>· {event.message}</li>
          ))}
        </ul>
      </GlassPanel>
    </div>
  );
}

export function MemoryPage() {
  if (!isTauri()) {
    return <MockMemoryPage />;
  }
  return <LiveMemoryPage />;
}

export function MemorySectionLink() {
  return (
    <p className="text-xs text-slate-400">
      Looking for semantic recall?{" "}
      <Link to="/memory" className="text-cyan-300 hover:underline">
        Open the Memory explorer
      </Link>{" "}
      (hybrid lexical + vector search over claims, evidence, tasks, skills).
    </p>
  );
}
