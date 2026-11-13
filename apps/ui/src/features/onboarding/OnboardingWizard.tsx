import { useCallback, useEffect, useRef, useState } from "react";
import {
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";
import {
  isTauri,
  listProviders,
  runDiagnostic,
  storeProviderKey,
  testProviderConnection,
  type ProviderStatus,
} from "../../bridge/index.js";
import type { Onboarding } from "./useOnboarding.js";

const STEPS = [
  "Welcome",
  "Provider",
  "Workspace",
  "Safety",
  "First task",
  "Done",
] as const;

type StepIndex = 0 | 1 | 2 | 3 | 4 | 5;

interface ProviderStepState {
  providers: ProviderStatus[];
  selected: string;
  keyInput: string;
  notice: string | null;
  busy: boolean;
  loaded: boolean;
}

function useProviderStep(active: boolean): ProviderStepState & {
  setSelected: (id: string) => void;
  setKeyInput: (value: string) => void;
  saveKey: () => Promise<void>;
  testConnection: (id: string) => Promise<void>;
  notice: string | null;
} {
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  const [selected, setSelected] = useState("openai");
  const [keyInput, setKeyInput] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // `loaded` reflects the sync-known case up front (inactive step or no
  // backend); the effect below only resolves the async Tauri fetch.
  const [loaded, setLoaded] = useState(() => !active || !isTauri());

  useEffect(() => {
    if (!active || !isTauri()) {
      return;
    }
    let cancelled = false;
    void listProviders()
      .then((next) => {
        if (!cancelled) {
          setProviders(next);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setNotice(error instanceof Error ? error.message : String(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoaded(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [active]);

  const saveKey = useCallback(async () => {
    const value = keyInput;
    if (value.trim() === "") {
      return;
    }
    setBusy(true);
    setNotice(null);
    try {
      await storeProviderKey(selected, value);
      setKeyInput("");
      setProviders(await listProviders());
      setNotice(`Key stored for ${selected} (OS keychain — never displayed).`);
    } catch (error: unknown) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, [keyInput, selected]);

  const testConnection = useCallback(async (id: string) => {
    setBusy(true);
    setNotice(null);
    try {
      const result = await testProviderConnection(id);
      setNotice(
        `${result.providerId}: ${result.ok ? "reachable" : "unreachable"} — ${result.message}`,
      );
      setProviders(await listProviders());
    } catch (error: unknown) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }, []);

  return {
    providers,
    selected,
    keyInput,
    notice,
    busy,
    loaded,
    setSelected,
    setKeyInput,
    saveKey,
    testConnection,
  };
}

export function OnboardingWizard({ onboarding }: { onboarding: Onboarding }) {
  const [step, setStep] = useState<StepIndex>(0);
  const [demoState, setDemoState] = useState<
    "idle" | "running" | "passed" | "failed"
  >("idle");
  const [demoDetail, setDemoDetail] = useState<string | null>(null);
  const [finishing, setFinishing] = useState(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const provider = useProviderStep(step === 1);

  // Move keyboard focus to the step heading on change (screen-reader
  // announcement for each wizard step).
  useEffect(() => {
    headingRef.current?.focus();
  }, [step]);

  const next = useCallback(() => {
    setStep((current) =>
      current >= 5 ? current : ((current + 1) as StepIndex),
    );
  }, []);

  const back = useCallback(() => {
    setStep((current) =>
      current <= 0 ? current : ((current - 1) as StepIndex),
    );
  }, []);

  const runDemo = useCallback(async () => {
    setDemoState("running");
    setDemoDetail(null);
    if (!isTauri()) {
      // Browser preview: narrate the loop without a backend.
      setDemoState("passed");
      setDemoDetail(
        "Preview mode: task → policy → evidence → completion (connect the desktop backend for a live run).",
      );
      return;
    }
    try {
      const summary = await runDiagnostic();
      const passed = summary.passed === summary.total;
      setDemoState(passed ? "passed" : "failed");
      setDemoDetail(
        `Diagnostic: ${summary.passed}/${summary.total} checks passed — policy ${summary.evaluation.decision}, ledger ${summary.ledger.healthy ? "healthy" : "attention"}.`,
      );
    } catch (error: unknown) {
      setDemoState("failed");
      setDemoDetail(error instanceof Error ? error.message : String(error));
    }
  }, []);

  const finish = useCallback(async () => {
    setFinishing(true);
    try {
      await onboarding.complete();
    } finally {
      setFinishing(false);
    }
  }, [onboarding]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="onboarding-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-void-950/90 p-4 backdrop-blur motion-reduce:backdrop-blur-none"
    >
      <GlassPanel label={`Onboarding — step ${step + 1} of ${STEPS.length}`}>
        <div className="w-[min(36rem,90vw)]">
          <SectionHeader
            title={STEPS[step]}
            subtitle={`Step ${step + 1} of ${STEPS.length}`}
          />
          <h2
            id="onboarding-title"
            ref={headingRef}
            tabIndex={-1}
            className="sr-only"
          >
            {STEPS[step]}
          </h2>
          <ol
            aria-label="Onboarding progress"
            className="mb-4 flex items-center gap-1"
          >
            {STEPS.map((label, index) => (
              <li
                key={label}
                aria-current={index === step ? "step" : undefined}
                aria-label={`${label}${index === step ? " (current)" : ""}`}
                className={`h-1.5 flex-1 rounded-full motion-safe:transition-colors ${
                  index <= step ? "bg-cyan-300" : "bg-white/10"
                }`}
              />
            ))}
          </ol>

          {step === 0 ? (
            <div className="flex flex-col gap-3">
              <p className="font-display text-xl text-slate-100">
                Talos OS — one policy, one ledger, every agent.
              </p>
              <p className="text-sm text-slate-300">
                Talos is a local-first agent command center: coding, browser,
                and computer use run through a default-deny policy broker, and
                every side effect lands in a hash-chained evidence ledger. This
                wizard gets you from install to your first verified task in
                under five minutes.
              </p>
              <ul className="flex flex-col gap-1 text-sm text-slate-400">
                <li>1 · Connect a model provider (or skip and stay local).</li>
                <li>2 · Pick a workspace directory.</li>
                <li>3 · Learn the safety contract.</li>
                <li>4 · Run a guided demo task.</li>
              </ul>
            </div>
          ) : null}

          {step === 1 ? (
            <div className="flex flex-col gap-3">
              <p className="text-sm text-slate-300">
                Add one LLM provider key. Keys live in the OS keychain and are
                never displayed back. Skip to stay fully local (Ollama).
              </p>
              {!provider.loaded ? (
                <Spinner label="Loading providers" />
              ) : !isTauri() ? (
                <p className="text-sm text-slate-400" role="status">
                  Browser preview: provider setup happens in the desktop app
                  (Settings → Model providers). You can safely skip.
                </p>
              ) : (
                <>
                  <div
                    role="radiogroup"
                    aria-label="Provider"
                    className="flex flex-wrap gap-2"
                  >
                    {(provider.providers.length > 0
                      ? provider.providers.map((item) => item.providerId)
                      : ["openai", "anthropic", "ollama"]
                    ).map((id) => (
                      <NeonButton
                        key={id}
                        variant={provider.selected === id ? "primary" : "ghost"}
                        aria-pressed={provider.selected === id}
                        onClick={() => {
                          provider.setSelected(id);
                        }}
                      >
                        {id}
                      </NeonButton>
                    ))}
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <input
                      type="password"
                      aria-label={`API key for ${provider.selected}`}
                      autoComplete="off"
                      spellCheck={false}
                      className="min-w-0 flex-1 rounded border border-slate-700 bg-slate-900 px-2 py-1.5 text-sm text-slate-100"
                      placeholder="Paste API key (never displayed again)"
                      value={provider.keyInput}
                      disabled={provider.busy}
                      onChange={(event) => {
                        provider.setKeyInput(event.target.value);
                      }}
                    />
                    <NeonButton
                      variant="ghost"
                      disabled={
                        provider.busy || provider.keyInput.trim() === ""
                      }
                      onClick={() => {
                        void provider.saveKey();
                      }}
                    >
                      Save key
                    </NeonButton>
                    <NeonButton
                      variant="ghost"
                      disabled={provider.busy}
                      onClick={() => {
                        void provider.testConnection(provider.selected);
                      }}
                    >
                      Test connection
                    </NeonButton>
                  </div>
                </>
              )}
              {provider.notice !== null ? (
                <p className="text-sm text-slate-300" role="status">
                  {provider.notice}
                </p>
              ) : null}
            </div>
          ) : null}

          {step === 2 ? (
            <div className="flex flex-col gap-3">
              <p className="text-sm text-slate-300">
                Choose the default workspace directory for per-task coding roots
                (git worktrees or temp dirs). You can change it later in the
                Code page.
              </p>
              <label
                className="flex flex-col gap-1 text-sm text-slate-300"
                htmlFor="onboarding-workspace"
              >
                Workspace directory
                <input
                  id="onboarding-workspace"
                  type="text"
                  autoComplete="off"
                  spellCheck={false}
                  className="rounded border border-slate-700 bg-slate-900 px-2 py-1.5 font-mono text-sm text-slate-100"
                  placeholder="~/talos-workspace"
                  value={onboarding.workspaceDir}
                  onChange={(event) => {
                    onboarding.setWorkspaceDir(event.target.value);
                  }}
                />
              </label>
              <p className="text-xs text-slate-500">
                Saved locally; task workspaces stay isolated per task regardless
                of this default.
              </p>
            </div>
          ) : null}

          {step === 3 ? (
            <div className="flex flex-col gap-3">
              <p className="font-display text-lg text-slate-100">
                The safety contract.
              </p>
              <ul className="flex list-disc flex-col gap-2 pl-5 text-sm text-slate-300">
                <li>
                  <strong>Policy broker is the only authority.</strong> Reads
                  are free; writes need recorded evidence; irreversible actions
                  need an approved compensation plan.
                </li>
                <li>
                  <strong>Humans gate high-risk actions.</strong> Plan / Act /
                  Auto modes, an approval queue with five-minute expiry, and an
                  emergency stop that latches until you reset it.
                </li>
                <li>
                  <strong>Evidence ledger remembers everything.</strong>
                  Hash-chained, tamper-evident, encrypted at rest — verify it
                  any time on the Evidence page.
                </li>
                <li>
                  <strong>Talos never acts without your permission</strong> for
                  high-risk actions. Budgets deny before spend, never after.
                </li>
              </ul>
            </div>
          ) : null}

          {step === 4 ? (
            <div className="flex flex-col gap-3">
              <p className="text-sm text-slate-300">
                Run the guided demo task: submit → policy evaluation → evidence
                recorded → completion. In the desktop this executes the mock
                diagnostic loop; nothing touches your system.
              </p>
              <div className="flex items-center gap-2">
                <StatusPill
                  tone={
                    demoState === "passed"
                      ? "ok"
                      : demoState === "failed"
                        ? "error"
                        : "neutral"
                  }
                >
                  {demoState === "idle"
                    ? "not run yet"
                    : demoState === "running"
                      ? "running…"
                      : demoState}
                </StatusPill>
                <NeonButton
                  variant="primary"
                  disabled={demoState === "running"}
                  onClick={() => {
                    void runDemo();
                  }}
                >
                  Run demo task
                </NeonButton>
              </div>
              {demoDetail !== null ? (
                <p className="text-sm text-slate-300" role="status">
                  {demoDetail}
                </p>
              ) : null}
            </div>
          ) : null}

          {step === 5 ? (
            <div className="flex flex-col gap-3">
              <p className="font-display text-xl text-slate-100">
                You are awake. What is Talos?
              </p>
              <p className="text-sm text-slate-300">
                One workspace, one policy, one ledger — coding, browser, and
                computer use with receipts for everything. Tour the Void Neon
                viewport (click nodes to inspect), open the Evidence page to
                verify the chain, and set budgets on the Operations page.
              </p>
              <p className="text-sm text-slate-300">
                Coming from another agent? Settings → Migration kit imports your
                AGENTS.md, skills, schedules, and MCP configs — quarantined and
                disabled by default, never auto-enabled. See{" "}
                <span className="font-mono text-xs">docs/MIGRATION.md</span> for
                the full contract.
              </p>
              <ul className="flex flex-col gap-1 text-sm text-slate-400">
                <li>
                  <kbd className="rounded border border-white/10 bg-white/5 px-1 font-mono text-xs">
                    Ctrl/⌘ K
                  </kbd>{" "}
                  command palette · keyboard-first navigation throughout.
                </li>
                <li>Docs live in docs/ — start with docs/RUNBOOK.md.</li>
                <li>
                  Headless? <span className="font-mono text-xs">talos run</span>{" "}
                  streams JSONL for CI (docs/CLI.md).
                </li>
              </ul>
            </div>
          ) : null}

          <div className="mt-5 flex items-center justify-between gap-2">
            <NeonButton
              variant="ghost"
              disabled={step === 0}
              onClick={back}
              aria-label="Previous step"
            >
              Back
            </NeonButton>
            <span className="text-xs text-slate-500">
              {step === 1
                ? "Skipping stays local — add a key later in Settings."
                : ""}
            </span>
            {step < 5 ? (
              <NeonButton
                variant="primary"
                onClick={next}
                aria-label="Next step"
              >
                {step === 1 ? "Skip / Continue" : "Continue"}
              </NeonButton>
            ) : (
              <NeonButton
                variant="primary"
                disabled={finishing}
                onClick={() => {
                  void finish();
                }}
              >
                {finishing ? "Finishing…" : "Finish setup"}
              </NeonButton>
            )}
          </div>
        </div>
      </GlassPanel>
    </div>
  );
}
