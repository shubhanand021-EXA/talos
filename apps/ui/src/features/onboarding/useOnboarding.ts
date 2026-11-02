import { useCallback, useEffect, useState } from "react";
import {
  getOnboardingState,
  isTauri,
  setOnboardingState,
} from "../../bridge/index.js";

const STORAGE_KEY = "talos.onboarding.completed";
const WORKSPACE_KEY = "talos.onboarding.workspaceDir";

/** Read the local (browser-mode) completion flag. */
function readLocalFlag(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

/** Persist the local (browser-mode) completion flag. */
function writeLocalFlag(completed: boolean): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, completed ? "true" : "false");
  } catch {
    // Private mode / blocked storage: the wizard simply reappears.
  }
}

export interface Onboarding {
  /** True while the completion flag is being resolved. */
  loading: boolean;
  /** Whether the wizard has been completed (skip the wizard when true). */
  completed: boolean;
  /** Mark onboarding complete (persists to the database when available). */
  complete: () => Promise<void>;
  /** Reset completion (Settings → "Replay onboarding"). */
  reset: () => Promise<void>;
  /** Workspace directory chosen during onboarding. */
  workspaceDir: string;
  /** Update the workspace directory choice. */
  setWorkspaceDir: (dir: string) => void;
}

/**
 * First-run state. Inside Tauri the flag lives in shared `app_state`
 * (`onboarding_completed`), so the desktop and the headless CLI agree; in
 * plain-browser mode it falls back to `localStorage`.
 */
export function useOnboarding(): Onboarding {
  const [loading, setLoading] = useState(() => isTauri());
  // Browser-mode flag is readable synchronously; the Tauri path corrects
  // it from the shared database once the async fetch resolves.
  const [completed, setCompleted] = useState(() => readLocalFlag());
  // Workspace choice is readable synchronously; no effect needed.
  const [workspaceDir, setWorkspaceDirState] = useState(() => {
    try {
      return window.localStorage.getItem(WORKSPACE_KEY) ?? "";
    } catch {
      return "";
    }
  });

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let cancelled = false;
    const load = async () => {
      try {
        const done = await getOnboardingState();
        if (!cancelled) {
          setCompleted(done);
          writeLocalFlag(done);
        }
      } catch {
        if (!cancelled) {
          setCompleted(readLocalFlag());
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  const setWorkspaceDir = useCallback((dir: string) => {
    setWorkspaceDirState(dir);
    try {
      window.localStorage.setItem(WORKSPACE_KEY, dir);
    } catch {
      // Ignore unwritable storage.
    }
  }, []);

  const complete = useCallback(async () => {
    writeLocalFlag(true);
    if (isTauri()) {
      try {
        await setOnboardingState(true);
      } catch {
        // Local flag already set; the next launch retries the ledger write.
      }
    }
    setCompleted(true);
  }, []);

  const reset = useCallback(async () => {
    writeLocalFlag(false);
    if (isTauri()) {
      try {
        await setOnboardingState(false);
      } catch {
        // Local flag already cleared.
      }
    }
    setCompleted(false);
  }, []);

  return { loading, completed, complete, reset, workspaceDir, setWorkspaceDir };
}
