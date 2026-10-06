import { useCallback, useEffect, useRef, useState } from "react";
import { APP_VERSION } from "../lib/appInfo";
import { describeError, stackFrames } from "../lib/errors";
import { createLogger } from "../lib/logger";

const log = createLogger("crash-recovery");

const STORAGE_KEY = "companyisland_crash_history";
// Earlier builds stored error messages and the user agent here; drop it on sight.
const LEGACY_STORAGE_KEY = "pillar_crash_history";
const HISTORY_MAX_AGE_MS = 24 * 60 * 60 * 1000;

// =============================================================================
// Types
// =============================================================================

export type CrashSeverity = "minor" | "moderate" | "severe" | "critical";

/** Privacy-safe by construction: error name and stack frames, never message text. */
export interface CrashReport {
  id: string;
  timestamp: number;
  errorName: string;
  severity: CrashSeverity;
  component?: string;
  action?: string;
  frames: string;
  appVersion: string;
}

export interface CrashRecoveryConfig {
  maxCrashReports?: number;
  crashThreshold?: number; // crashes within timeWindow that count as a crash loop
  timeWindow?: number; // ms
  autoRecoveryDelay?: number; // ms before auto-recovery attempt
  enableAutoRecovery?: boolean;
}

export interface UseCrashRecoveryReturn {
  reportCrash: (
    error: unknown,
    options?: { severity?: CrashSeverity; component?: string; action?: string }
  ) => void;
  crashHistory: CrashReport[];
  isCrashLoopDetected: boolean;
  triggerRecovery: () => Promise<void>;
  clearCrashHistory: () => void;
}

// =============================================================================
// Hook
// =============================================================================

export function useCrashRecovery(config: CrashRecoveryConfig = {}): UseCrashRecoveryReturn {
  const {
    maxCrashReports = 50,
    crashThreshold = 3,
    timeWindow = 60_000,
    autoRecoveryDelay = 3000,
    enableAutoRecovery = true,
  } = config;

  const [crashHistory, setCrashHistory] = useState<CrashReport[]>([]);
  const recoveringRef = useRef(false);
  const recoveryTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isMountedRef = useRef(true);
  // Synchronous mirror of crashHistory so loop detection inside reportCrash always
  // counts every recorded crash, even rapid back-to-back ones.
  const crashHistoryRef = useRef<CrashReport[]>([]);

  useEffect(() => {
    isMountedRef.current = true;
    try {
      localStorage.removeItem(LEGACY_STORAGE_KEY);
      const saved = localStorage.getItem(STORAGE_KEY);
      const parsed: unknown = saved ? JSON.parse(saved) : null;
      if (Array.isArray(parsed)) {
        // Recent crashes only, with a shape check so one corrupt entry can't crash the loader.
        const recent = parsed
          .filter(
            (crash): crash is CrashReport =>
              !!crash &&
              typeof crash === "object" &&
              typeof (crash as CrashReport).id === "string" &&
              typeof (crash as CrashReport).timestamp === "number" &&
              Date.now() - (crash as CrashReport).timestamp < HISTORY_MAX_AGE_MS
          )
          .slice(0, maxCrashReports);
        crashHistoryRef.current = recent;
        setCrashHistory(recent);
      }
    } catch {
      // storage unavailable or corrupt: start with an empty history
    }

    return () => {
      isMountedRef.current = false;
      if (recoveryTimeoutRef.current) clearTimeout(recoveryTimeoutRef.current);
    };
  }, [maxCrashReports]);

  const saveCrashHistory = useCallback((history: CrashReport[]) => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(history));
    } catch {
      // storage unavailable: history stays in memory only
    }
  }, []);

  const triggerRecovery = useCallback(async () => {
    if (recoveringRef.current) return;
    recoveringRef.current = true;
    log.warn("Starting recovery sequence");

    try {
      // Cache storage only: nothing a render crash can corrupt lives in localStorage.
      if (typeof window !== "undefined" && "caches" in window) {
        const cacheNames = await caches.keys();
        await Promise.all(cacheNames.map((name) => caches.delete(name)));
      }
      setTimeout(() => window.location.reload(), 500);
    } catch (error) {
      log.error("Recovery failed", error);
      recoveringRef.current = false;
    }
  }, []);

  const reportCrash = useCallback(
    (error: unknown, options: { severity?: CrashSeverity; component?: string; action?: string } = {}) => {
      const { severity = "moderate", component, action } = options;

      const crashReport: CrashReport = {
        id: `crash_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`,
        timestamp: Date.now(),
        errorName: describeError(error),
        severity,
        component,
        action,
        frames: stackFrames(error),
        appVersion: APP_VERSION,
      };

      const newHistory = [crashReport, ...crashHistoryRef.current].slice(0, maxCrashReports);
      crashHistoryRef.current = newHistory;
      saveCrashHistory(newHistory);
      setCrashHistory(newHistory);

      // Route through the logger so the backend gets a persistent record.
      log.error("Crash reported", crashReport);

      // Auto-recover from severe crashes unless we are already in a crash loop:
      // reloading into a deterministic crash would otherwise loop forever.
      if (enableAutoRecovery && (severity === "severe" || severity === "critical")) {
        const now = Date.now();
        const recentCount = newHistory.filter((c) => now - c.timestamp < timeWindow).length;

        if (recentCount >= crashThreshold) {
          log.warn(`Crash loop detected (${recentCount} crashes in ${Math.round(timeWindow / 1000)}s); auto-recovery suspended`);
          return;
        }

        if (recoveryTimeoutRef.current) clearTimeout(recoveryTimeoutRef.current);
        recoveryTimeoutRef.current = setTimeout(() => {
          if (isMountedRef.current) void triggerRecovery();
        }, autoRecoveryDelay);
      }
    },
    [maxCrashReports, enableAutoRecovery, autoRecoveryDelay, timeWindow, crashThreshold, saveCrashHistory, triggerRecovery]
  );

  const clearCrashHistory = useCallback(() => {
    crashHistoryRef.current = [];
    setCrashHistory([]);
    saveCrashHistory([]);
  }, [saveCrashHistory]);

  const isCrashLoopDetected = crashHistory.filter((crash) => Date.now() - crash.timestamp < timeWindow).length >= crashThreshold;

  return { reportCrash, crashHistory, isCrashLoopDetected, triggerRecovery, clearCrashHistory };
}
