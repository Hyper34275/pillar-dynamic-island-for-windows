import { useEffect, useState } from "react";
import { ipc, type Diagnostics, type SystemInfo } from "../lib/ipc";

/** Never refetch more often than this, however often About is opened. */
const REFRESH_MS = 60_000;

interface Cached {
  info: SystemInfo | null;
  diagnostics: Diagnostics | null;
  fetchedAt: number;
}

// Survives About being closed and reopened; only ever read while About is mounted.
let cached: Cached | null = null;
let inFlight: Promise<Cached> | null = null;

function load(): Promise<Cached> {
  if (cached && Date.now() - cached.fetchedAt < REFRESH_MS) return Promise.resolve(cached);
  inFlight ??= Promise.all([ipc.getSystemInfo(), ipc.getDiagnostics()])
    .then(([info, diagnostics]) => {
      cached = {
        // Keep the last good value when a refresh fails (e.g. a transient IP lookup error).
        info: info ?? cached?.info ?? null,
        diagnostics: diagnostics ?? cached?.diagnostics ?? null,
        fetchedAt: Date.now(),
      };
      return cached;
    })
    .finally(() => {
      inFlight = null;
    });
  return inFlight;
}

export interface UseSystemInfoResult {
  info: SystemInfo | null;
  diagnostics: Diagnostics | null;
}

/** Fetches on mount, reuses the cache, and refreshes at most every 60 s while mounted and visible. */
export function useSystemInfo(): UseSystemInfoResult {
  const [state, setState] = useState<Cached | null>(cached);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const run = () => {
      timer = null;
      if (document.hidden) {
        // Nothing visible to update; the next visibilitychange resumes the cycle.
        return;
      }
      void load().then((next) => {
        if (disposed) return;
        setState(next);
        timer = setTimeout(run, Math.max(1000, REFRESH_MS - (Date.now() - next.fetchedAt)));
      });
    };

    const onVisible = () => {
      if (!document.hidden && timer === null && !disposed) run();
    };

    run();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      disposed = true;
      if (timer !== null) clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

  return { info: state?.info ?? null, diagnostics: state?.diagnostics ?? null };
}
