import { useCallback, useEffect, useState } from "react";
import type { RunnerStatusView } from "@talos/contracts";
import { isTauri } from "../../bridge/client.js";
import {
  addRunner,
  listRunners,
  removeRunner,
  testRunner,
} from "../../bridge/gateway.js";
import {
  GlassPanel,
  NeonButton,
  SectionHeader,
  Spinner,
  StatusPill,
} from "../../design-system/index.js";

/**
 * Runners tab (Settings): BYOC remote execution targets. SSH runners copy
 * the workspace over (`tar` pipe, no rsync) and execute under an
 * unprivileged user in one isolated dir; Docker runners bind-mount exactly
 * the staged workspace with `--network none` in a fresh `--rm` container.
 * Every remote tool call passes the local policy broker first — receipts
 * come back `live_remote` with a sha256 output hash into the local ledger.
 */
export function RunnersPanel() {
  const [runners, setRunners] = useState<RunnerStatusView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [form, setForm] = useState({
    id: "",
    kind: "ssh",
    host: "",
    user: "talos",
    keyPath: "~/.ssh/talos_ed25519",
    remoteDir: "~/.talos-runs",
    dockerImage: "talos-runner:1",
  });

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      setError("Runners live in the Tauri shell — launch the desktop app.");
      setRunners([]);
      return;
    }
    try {
      setRunners(await listRunners());
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Runners unreachable");
      setRunners([]);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!isTauri()) {
        if (!cancelled) {
          setError("Runners live in the Tauri shell — launch the desktop app.");
          setRunners([]);
        }
        return;
      }
      try {
        const rows = await listRunners();
        if (!cancelled) {
          setRunners(rows);
          setError(null);
        }
      } catch (cause) {
        if (!cancelled) {
          setRunners([]);
          setError(
            cause instanceof Error ? cause.message : "Runners unreachable",
          );
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const add = useCallback(async () => {
    const id = form.id.trim();
    if (id === "") {
      setError("Runner id is required (a-z, 0-9, -, _).");
      return;
    }
    setBusy("add");
    try {
      const runner =
        form.kind === "docker"
          ? {
              id,
              kind: "docker" as const,
              dockerImage: form.dockerImage.trim() || "talos-runner:1",
              enabled: true,
            }
          : {
              id,
              kind: "ssh" as const,
              host: form.host.trim(),
              user: form.user.trim() || "talos",
              keyPath: form.keyPath.trim(),
              remoteDir: form.remoteDir.trim(),
              enabled: true,
            };
      setRunners(await addRunner(runner));
      setForm((prev) => ({ ...prev, id: "", host: "" }));
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Add failed");
    } finally {
      setBusy(null);
    }
  }, [form]);

  const remove = useCallback(async (id: string) => {
    setBusy(id);
    try {
      setRunners(await removeRunner(id));
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Remove failed");
    } finally {
      setBusy(null);
    }
  }, []);

  const test = useCallback(async (id: string) => {
    setBusy(id);
    try {
      const result = await testRunner(id);
      setRunners((prev) =>
        prev === null
          ? prev
          : prev.map((row) =>
              row.id === id
                ? {
                    ...row,
                    lastTestOk: result.lastTestOk,
                    lastTestMessage: result.lastTestMessage,
                  }
                : row,
            ),
      );
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Test failed");
    } finally {
      setBusy(null);
    }
  }, []);

  return (
    <GlassPanel label="Runners">
      <SectionHeader
        title="Remote runners (BYOC)"
        subtitle="SSH / Docker targets for heavy tools — policy stays local, receipts come home"
      />
      {runners === null ? (
        <Spinner label="Loading runners" />
      ) : runners.length === 0 ? (
        <p className="text-sm text-slate-400" role="status">
          {error ?? "No runners yet — add one below."}
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {runners.map((runner) => (
            <li
              key={runner.id}
              className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-white/10 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <StatusPill tone={runner.enabled ? "ok" : "neutral"}>
                    {runner.enabled ? "enabled" : "disabled"}
                  </StatusPill>
                  <span className="font-medium text-slate-100">
                    {runner.id}
                  </span>
                  <span className="font-mono text-xs text-slate-400">
                    {runner.kind}
                  </span>
                  {runner.lastTestOk !== null &&
                  runner.lastTestOk !== undefined ? (
                    <StatusPill tone={runner.lastTestOk ? "ok" : "error"}>
                      {runner.lastTestOk
                        ? "reachable"
                        : (runner.lastTestMessage ?? "unreachable")}
                    </StatusPill>
                  ) : null}
                </div>
              </div>
              <div className="flex items-center gap-2">
                <NeonButton
                  variant="ghost"
                  disabled={busy === runner.id}
                  onClick={() => {
                    void test(runner.id);
                  }}
                >
                  {busy === runner.id ? "Testing…" : "Test connection"}
                </NeonButton>
                <NeonButton
                  variant="ghost"
                  disabled={busy === runner.id}
                  onClick={() => {
                    void remove(runner.id);
                  }}
                >
                  Remove
                </NeonButton>
              </div>
            </li>
          ))}
        </ul>
      )}
      <div className="mt-3 rounded-md border border-white/10 p-3">
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
          Add runner
        </h3>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Runner id
            <input
              aria-label="Runner id"
              value={form.id}
              onChange={(event) => {
                setForm((prev) => ({ ...prev, id: event.target.value }));
              }}
              placeholder="gpu-box"
              className="rounded-md border border-white/15 bg-void-950 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-600"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-slate-400">
            Transport
            <select
              aria-label="Transport"
              value={form.kind}
              onChange={(event) => {
                setForm((prev) => ({ ...prev, kind: event.target.value }));
              }}
              className="rounded-md border border-white/15 bg-void-950 px-2 py-1 text-sm text-slate-100"
            >
              <option value="ssh">SSH</option>
              <option value="docker">Docker</option>
            </select>
          </label>
          {form.kind === "ssh" ? (
            <>
              <label className="flex flex-col gap-1 text-xs text-slate-400">
                Host
                <input
                  aria-label="SSH host"
                  value={form.host}
                  onChange={(event) => {
                    setForm((prev) => ({ ...prev, host: event.target.value }));
                  }}
                  placeholder="gpu.local"
                  className="rounded-md border border-white/15 bg-void-950 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-600"
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-slate-400">
                Key path (never leaves disk)
                <input
                  aria-label="SSH key path"
                  value={form.keyPath}
                  onChange={(event) => {
                    setForm((prev) => ({
                      ...prev,
                      keyPath: event.target.value,
                    }));
                  }}
                  className="rounded-md border border-white/15 bg-void-950 px-2 py-1 text-sm text-slate-100"
                />
              </label>
            </>
          ) : (
            <label className="flex flex-col gap-1 text-xs text-slate-400">
              Image
              <input
                aria-label="Docker image"
                value={form.dockerImage}
                onChange={(event) => {
                  setForm((prev) => ({
                    ...prev,
                    dockerImage: event.target.value,
                  }));
                }}
                placeholder="talos-runner:1"
                className="rounded-md border border-white/15 bg-void-950 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-600"
              />
            </label>
          )}
        </div>
        <div className="mt-2">
          <NeonButton
            variant="primary"
            disabled={busy === "add"}
            onClick={() => {
              void add();
            }}
          >
            {busy === "add" ? "Adding…" : "Add runner"}
          </NeonButton>
        </div>
      </div>
      {error !== null ? (
        <p className="mt-2 text-xs text-amber-300" role="status">
          {error}
        </p>
      ) : null}
      <div className="mt-2">
        <NeonButton
          variant="ghost"
          onClick={() => {
            void refresh();
          }}
        >
          Refresh
        </NeonButton>
      </div>
    </GlassPanel>
  );
}
