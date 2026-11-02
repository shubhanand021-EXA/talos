import { useState } from "react";
import {
  GlassPanel,
  NeonButton,
  SectionHeader,
  StatusPill,
} from "../../design-system/index.js";
import {
  getSandboxProfileForTask,
  getSandboxStatus,
  type SandboxProfileForTask,
  type SandboxStatus,
} from "../../bridge/index.js";

const PROFILE_TONE: Record<
  string,
  "ok" | "warn" | "error" | "neutral" | "info"
> = {
  read_only: "neutral",
  workspace_write: "ok",
  danger_full_access: "error",
};

/** OS sandbox status + per-task profile lookup (Computer page section). */
export function SandboxPanel() {
  const [status, setStatus] = useState<SandboxStatus | null>(null);
  const [profile, setProfile] = useState<SandboxProfileForTask | null>(null);
  const [taskId, setTaskId] = useState("");
  const [loading, setLoading] = useState(false);
  const [lookingUp, setLookingUp] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
    setNotice(null);
    try {
      setStatus(await getSandboxStatus());
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Status load failed");
    } finally {
      setLoading(false);
    }
  };

  const lookup = async () => {
    if (taskId.trim() === "") {
      setNotice("Enter a task id to resolve its profile.");
      return;
    }
    setLookingUp(true);
    setNotice(null);
    try {
      setProfile(await getSandboxProfileForTask(taskId.trim()));
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Lookup failed");
    } finally {
      setLookingUp(false);
    }
  };

  const anyAvailable = status?.backends.some((b) => b.available) ?? false;

  return (
    <GlassPanel label="OS sandbox">
      <SectionHeader
        title="OS sandbox"
        subtitle="Namespace / Seatbelt / Job-Object confinement for shell execution · fail closed"
        action={
          <NeonButton
            onClick={() => {
              void refresh();
            }}
            loading={loading}
          >
            Refresh status
          </NeonButton>
        }
      />
      {notice !== null ? (
        <p role="status" className="mb-2 text-xs text-amber-300">
          {notice}
        </p>
      ) : null}
      {status === null ? (
        <p className="text-xs text-slate-500">
          Load the status to see platform backends and per-task profiles.
        </p>
      ) : (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-xs text-cyan-300">
              {status.platform}
            </span>
            <StatusPill tone={status.failClosed ? "ok" : "error"}>
              {status.failClosed ? "fail closed" : "NOT fail closed"}
            </StatusPill>
            {!anyAvailable ? (
              <StatusPill tone="warn">
                sandbox unavailable — execution denied
              </StatusPill>
            ) : null}
          </div>
          <ul className="flex flex-col gap-1">
            {status.backends.map((backend) => (
              <li
                key={backend.name}
                data-testid={`sandbox-backend-${backend.name}`}
                className="rounded-md border border-white/10 p-2"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-mono text-xs">{backend.name}</span>
                  <StatusPill tone={backend.available ? "ok" : "error"}>
                    {backend.available ? "available" : "unavailable"}
                  </StatusPill>
                </div>
                <p className="mt-1 text-xs text-slate-400">
                  {backend.enforcementMethod}
                </p>
                <p className="mt-1 font-mono text-xs text-slate-500">
                  {backend.detail}
                </p>
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-3">
        <SectionHeader
          title="Per-task profile"
          subtitle="read_only · workspace_write · danger_full_access"
        />
        <div className="mb-2 flex flex-wrap gap-2">
          <input
            aria-label="Task id for profile lookup"
            value={taskId}
            onChange={(event) => {
              setTaskId(event.target.value);
            }}
            placeholder="task id"
            className="min-w-52 flex-1 rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-xs"
          />
          <NeonButton
            onClick={() => {
              void lookup();
            }}
            loading={lookingUp}
          >
            Resolve profile
          </NeonButton>
        </div>
        {profile !== null ? (
          <div
            data-testid="sandbox-profile-result"
            className="rounded-md border border-white/10 p-2"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-mono text-xs text-cyan-300">
                {profile.taskId}
              </span>
              <StatusPill tone={PROFILE_TONE[profile.profile] ?? "neutral"}>
                {profile.profile}
              </StatusPill>
              <StatusPill tone={profile.enforced ? "ok" : "error"}>
                {profile.enforced ? "enforced" : "denied"}
              </StatusPill>
            </div>
            <p className="mt-1 text-xs text-slate-400">
              {profile.enforcementMethod}
            </p>
            <p className="mt-1 font-mono text-xs text-slate-500">
              {profile.reason}
            </p>
          </div>
        ) : null}
      </div>
    </GlassPanel>
  );
}
