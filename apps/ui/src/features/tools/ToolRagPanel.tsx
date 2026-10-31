import { useState } from "react";
import type { ToolCandidate } from "@talos/contracts";
import {
  EmptyState,
  GlassPanel,
  NeonButton,
  StatusPill,
  type PillTone,
} from "../../design-system/index.js";
import {
  isBackendUnavailable,
  queryToolCandidates,
} from "../../bridge/index.js";

const RISK_TONE: Record<string, PillTone> = {
  Low: "ok",
  low: "ok",
  Medium: "warn",
  medium: "warn",
  High: "error",
  high: "error",
  Irreversible: "error",
  irreversible: "error",
};

const CAPABILITIES = [
  "any",
  "ReadFile",
  "WriteFile",
  "ExecuteCommand",
  "OpenBrowser",
  "UiAction",
  "NetworkRequest",
  "SystemControl",
] as const;

const RISK_LEVELS = ["any", "low", "medium", "high", "irreversible"] as const;

/** Local-first Tool-RAG panel: intent in, ranked candidates out. */
export function ToolRagPanel({
  onSelectTool,
}: {
  onSelectTool: (toolId: string) => void;
}) {
  const [intent, setIntent] = useState("read a file");
  const [capability, setCapability] =
    useState<(typeof CAPABILITIES)[number]>("any");
  const [riskCeiling, setRiskCeiling] =
    useState<(typeof RISK_LEVELS)[number]>("any");
  const [requireEvidence, setRequireEvidence] = useState(false);
  const [topK, setTopK] = useState("5");
  const [candidates, setCandidates] = useState<ToolCandidate[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const runQuery = async () => {
    setSearching(true);
    setNotice(null);
    try {
      const top = Number.parseInt(topK, 10);
      const next = await queryToolCandidates({
        intent,
        capabilities: capability === "any" ? [] : [capability],
        riskCeiling: riskCeiling === "any" ? undefined : riskCeiling,
        requireEvidence,
        topK: Number.isFinite(top) ? top : undefined,
      });
      setCandidates(next);
    } catch (error) {
      setCandidates(null);
      setNotice(
        isBackendUnavailable(error)
          ? "Backend unavailable — launch the desktop shell."
          : error instanceof Error
            ? error.message
            : "Ranking failed",
      );
    } finally {
      setSearching(false);
    }
  };

  return (
    <GlassPanel label="Tool-RAG routing">
      <div className="flex flex-col gap-2">
        <label className="text-xs text-slate-400" htmlFor="rag-intent">
          Intent (plain words — never executed)
        </label>
        <input
          id="rag-intent"
          value={intent}
          onChange={(event) => {
            setIntent(event.target.value);
          }}
          className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
        />
        <div className="grid grid-cols-2 gap-2">
          <div className="flex flex-col gap-1">
            <label className="text-xs text-slate-400" htmlFor="rag-capability">
              Capability
            </label>
            <select
              id="rag-capability"
              value={capability}
              onChange={(event) => {
                setCapability(
                  event.target.value as (typeof CAPABILITIES)[number],
                );
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
            >
              {CAPABILITIES.map((entry) => (
                <option key={entry} value={entry}>
                  {entry}
                </option>
              ))}
            </select>
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs text-slate-400" htmlFor="rag-ceiling">
              Risk ceiling
            </label>
            <select
              id="rag-ceiling"
              value={riskCeiling}
              onChange={(event) => {
                setRiskCeiling(
                  event.target.value as (typeof RISK_LEVELS)[number],
                );
              }}
              className="rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
            >
              {RISK_LEVELS.map((entry) => (
                <option key={entry} value={entry}>
                  {entry}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-2 text-xs text-slate-300">
            <input
              type="checkbox"
              checked={requireEvidence}
              onChange={(event) => {
                setRequireEvidence(event.target.checked);
              }}
            />
            Evidence-backed only
          </label>
          <label className="ml-auto text-xs text-slate-400" htmlFor="rag-topk">
            Top K
          </label>
          <input
            id="rag-topk"
            value={topK}
            onChange={(event) => {
              setTopK(event.target.value);
            }}
            inputMode="numeric"
            className="w-16 rounded-md border border-white/10 bg-black/30 px-2 py-1 text-sm"
          />
        </div>
        <div>
          <NeonButton
            onClick={() => {
              void runQuery();
            }}
            loading={searching}
          >
            Rank tools
          </NeonButton>
        </div>
        {notice !== null ? (
          <p role="status" className="text-xs text-amber-300">
            {notice}
          </p>
        ) : null}
        {candidates === null ? (
          <EmptyState
            title="No ranking yet"
            message="Describe an intent to rank the 22 registered tools locally — no network, no embeddings."
          />
        ) : candidates.length === 0 ? (
          <EmptyState
            title="No candidates"
            message="Filters excluded every tool. Widen the capability or raise the risk ceiling."
          />
        ) : (
          <div className="flex flex-col gap-2">
            {candidates.map((candidate) => (
              <div
                key={candidate.toolId}
                className="rounded-md border border-white/10 bg-black/30 p-2"
              >
                <div className="flex items-center justify-between gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      onSelectTool(candidate.toolId);
                    }}
                    className="font-mono text-sm text-cyan-300 underline decoration-dotted underline-offset-4"
                    aria-label={`Select ${candidate.toolId}`}
                  >
                    {candidate.toolId}
                  </button>
                  <span className="flex items-center gap-1">
                    <StatusPill
                      tone={RISK_TONE[candidate.riskLevel] ?? "neutral"}
                    >
                      {candidate.riskLevel}
                    </StatusPill>
                    <StatusPill tone="info">
                      {candidate.score.toFixed(2)}
                    </StatusPill>
                  </span>
                </div>
                <p className="mt-1 text-xs text-slate-300">
                  {candidate.reason}
                </p>
                {candidate.matchedFields.length > 0 ? (
                  <p className="mt-1 font-mono text-xs text-slate-500">
                    matched: {candidate.matchedFields.join(", ")}
                  </p>
                ) : null}
                {candidate.requiredApprovals.length > 0 ? (
                  <p className="mt-1 text-xs text-amber-300">
                    approvals: {candidate.requiredApprovals.join(", ")}
                  </p>
                ) : null}
              </div>
            ))}
          </div>
        )}
        <p className="text-xs text-slate-500">
          Local lexical scoring: keyword overlap + capability match, boosted by
          successful receipts, penalized by failures. Risk ceiling filters
          first; disabled tools never rank.
        </p>
      </div>
    </GlassPanel>
  );
}
