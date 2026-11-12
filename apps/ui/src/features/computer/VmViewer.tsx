import { useCallback, useEffect, useRef, useState } from "react";
import {
  GlassPanel,
  KeyValueList,
  NeonButton,
  StatusPill,
} from "../../design-system/index.js";
import {
  destroyComputerVm,
  computerVmFrame,
  computerVmStatus,
  isBackendUnavailable,
  launchComputerVm,
  type ComputerVmSession,
  type VncStreamEvent,
} from "../../bridge/index.js";

interface VmViewerProps {
  /** Owning task (container name linkage). */
  taskId: string;
  /** Compact card for the Agents tab (`false` = full panel with controls). */
  compact?: boolean;
}

const POLL_MS = 1000;

function frameSrc(frame: VncStreamEvent | null): string | null {
  if (frame?.pngBase64) {
    return `data:image/png;base64,${frame.pngBase64}`;
  }
  return null;
}

/**
 * Live VNC viewer for the ephemeral computer-use VM. Polls
 * `computerVmFrame` at 1fps (screenshots, never a raw VNC socket — no
 * binary protocol ever reaches the browser) and surfaces lifecycle
 * controls: Launch (read-only workspace mount), Refresh, Destroy.
 */
export function VmViewer({ taskId, compact = false }: VmViewerProps) {
  const [session, setSession] = useState<ComputerVmSession | null>(null);
  const [frame, setFrame] = useState<VncStreamEvent | null>(null);
  const [running, setRunning] = useState(false);
  const [streaming, setStreaming] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setSession(await computerVmStatus());
    } catch {
      setSession(null);
    }
  }, []);

  const pollFrame = useCallback(async () => {
    try {
      const next = await computerVmFrame(true);
      setFrame(next);
    } catch {
      // A missing container simply yields no frame; the status line explains.
    }
  }, []);

  useEffect(() => {
    // Initial status only; Refresh button reuses the same fetcher.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refreshStatus();
  }, [refreshStatus]);

  useEffect(() => {
    if (!streaming) {
      return;
    }
    // Prime the viewer, then poll at 1fps while streaming.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void pollFrame();
    timer.current = setInterval(() => {
      void pollFrame();
    }, POLL_MS);
    return () => {
      if (timer.current !== null) {
        clearInterval(timer.current);
        timer.current = null;
      }
    };
  }, [streaming, pollFrame]);

  const launch = async () => {
    setRunning(true);
    setNotice(null);
    try {
      const view = await launchComputerVm(taskId);
      setSession(view);
      await pollFrame();
    } catch (error) {
      setNotice(
        isBackendUnavailable(error)
          ? "Backend unavailable — launch the desktop shell."
          : error instanceof Error
            ? error.message
            : "Launch failed",
      );
    } finally {
      // The launch reply is the fresh status; a trailing refresh would
      // race it against the pre-launch snapshot.
      setRunning(false);
    }
  };

  const destroy = async () => {
    setRunning(true);
    setNotice(null);
    try {
      await destroyComputerVm();
      setFrame(null);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Destroy failed");
    } finally {
      setRunning(false);
      void refreshStatus();
    }
  };

  const src = frameSrc(frame);

  return (
    <GlassPanel label={compact ? "VM" : "Ephemeral VM (VNC viewer)"}>
      <div className="flex flex-wrap items-center gap-2">
        <StatusPill tone={session?.running ? "ok" : "neutral"}>
          {session?.running ? "container running" : "no container"}
        </StatusPill>
        {session?.vncUrl ? (
          <span className="font-mono text-xs text-slate-400">
            {session.vncUrl}
          </span>
        ) : null}
        <label className="flex items-center gap-2 text-xs text-slate-300">
          <input
            type="checkbox"
            checked={streaming}
            onChange={(event) => {
              setStreaming(event.target.checked);
            }}
          />
          Stream (1fps screenshots)
        </label>
      </div>
      {src !== null ? (
        <img
          src={src}
          alt="Ephemeral VM desktop"
          className="mt-2 h-auto w-full rounded-md border border-white/10 bg-black/40"
        />
      ) : (
        <p className="mt-2 text-xs text-slate-500">
          No frame yet — launch the container to watch the agent operate inside
          the isolated desktop. The host screen is never touched.
        </p>
      )}
      {frame !== null ? (
        <p className="mt-1 font-mono text-xs text-slate-500">
          seq {frame.seq} · {frame.hash.slice(0, 18)}… · {frame.width}x
          {frame.height}
        </p>
      ) : null}
      {notice !== null ? (
        <p role="status" className="mt-2 text-xs text-amber-300">
          {notice}
        </p>
      ) : null}
      {!compact && session !== null ? (
        <KeyValueList
          entries={[
            {
              key: "container",
              value: session.containerName ?? session.containerId ?? "—",
            },
            {
              key: "resolution",
              value: `${session.width}x${session.height}`,
            },
            { key: "backend", value: session.backend },
            { key: "started", value: session.startedAt ?? "—" },
          ]}
        />
      ) : null}
      <div className="mt-3 flex flex-wrap gap-2">
        <NeonButton
          onClick={() => {
            void launch();
          }}
          loading={running}
        >
          Launch VM
        </NeonButton>
        <NeonButton
          onClick={() => {
            void destroy();
          }}
          loading={running}
        >
          Destroy VM
        </NeonButton>
        <NeonButton
          onClick={() => {
            void (async () => {
              await pollFrame();
              await refreshStatus();
            })();
          }}
          loading={running}
        >
          Refresh
        </NeonButton>
      </div>
      {!compact ? (
        <p className="mt-2 text-xs text-slate-500">
          Read-only <span className="font-mono">/workspace</span> mount; no host
          paths visible; network isolated; verified artifacts leave only via{" "}
          <span className="font-mono">computer.export_file</span>.
        </p>
      ) : null}
    </GlassPanel>
  );
}
