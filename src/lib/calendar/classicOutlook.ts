import { dlog } from "../debugLog";
import { ipc, onEvent } from "../ipc";
import { normalizeSnapshot } from "./normalize";
import type { CalendarProvider } from "./provider";
import { WAITING_SNAPSHOT, type CalendarSnapshot } from "./types";

/** The Rust side of the provider; faked in tests. Payloads are raw: the provider normalises them. */
export interface CalendarBackend {
  getSnapshot(): Promise<unknown>;
  refresh(): Promise<boolean>;
  onSnapshot(handler: (raw: unknown) => void): () => void;
}

export const tauriCalendarBackend: CalendarBackend = {
  getSnapshot: () => ipc.calendarGetSnapshot(),
  refresh: () => ipc.calendarRefresh(),
  onSnapshot: (handler) => onEvent<unknown>("calendar-snapshot", handler),
};

export interface ClassicOutlookOptions {
  backend?: CalendarBackend;
  now?: () => number;
  /** Refresh requests closer together than this are dropped: expanding the island repeatedly must not hammer Outlook. */
  minRefreshIntervalMs?: number;
}

/** Classic Outlook through the Rust COM worker: initial snapshot, then pushed 'calendar-snapshot' events. */
export function createClassicOutlookProvider(options: ClassicOutlookOptions = {}): CalendarProvider {
  const backend = options.backend ?? tauriCalendarBackend;
  const now = options.now ?? (() => Date.now());
  const minRefreshIntervalMs = options.minRefreshIntervalMs ?? 5_000;

  let snapshot: CalendarSnapshot = WAITING_SNAPSHOT;
  let eventsKey = JSON.stringify(snapshot.events);
  let disposed = false;
  let pushed = false;
  let inFlight: Promise<void> | null = null;
  let lastRefreshAt = Number.NEGATIVE_INFINITY;
  const listeners = new Set<(snapshot: CalendarSnapshot) => void>();

  function publish(raw: unknown): void {
    const parsed = normalizeSnapshot(raw);
    if (!parsed) {
      if (raw !== null) dlog("warn", "calendar", "ignored a snapshot that is not an object");
      return;
    }
    // Every sync re-sends the snapshot (its sync time moves), nearly always with the same
    // meetings. Keep the old events array then, so what depends on it (reminder schedule,
    // memoised lists) is not recomputed once a minute for nothing.
    const key = JSON.stringify(parsed.events);
    const next = key === eventsKey ? { ...parsed, events: snapshot.events } : parsed;
    if (JSON.stringify(next) === JSON.stringify(snapshot)) return;
    eventsKey = key;
    if (next.status !== snapshot.status || next.errorCode !== snapshot.errorCode) {
      dlog("info", "calendar", `status ${snapshot.status} -> ${next.status}${next.errorCode ? ` [${next.errorCode}]` : ""}, ${next.events.length} events`);
    }
    snapshot = next;
    for (const listener of [...listeners]) {
      try {
        listener(snapshot);
      } catch {
        // one broken subscriber must not starve the others
      }
    }
  }

  const unlisten = backend.onSnapshot((raw) => {
    pushed = true;
    publish(raw);
  });

  // A push that beats the initial read is newer than it, so it wins.
  void (async () => {
    try {
      const raw = await backend.getSnapshot();
      if (!disposed && !pushed) publish(raw);
    } catch {
      // the next push (or refresh) delivers the state
    }
  })();

  return {
    id: "classic-outlook",
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => snapshot,
    refresh() {
      if (disposed) return Promise.resolve();
      if (inFlight) return inFlight;
      const at = now();
      if (at - lastRefreshAt < minRefreshIntervalMs) return Promise.resolve();
      lastRefreshAt = at;
      inFlight = (async () => {
        try {
          if (!(await backend.refresh())) dlog("debug", "calendar", "refresh not accepted by the backend");
        } catch {
          dlog("warn", "calendar", "refresh failed");
        } finally {
          inFlight = null;
        }
      })();
      return inFlight;
    },
    dispose() {
      disposed = true;
      listeners.clear();
      unlisten();
    },
  };
}
