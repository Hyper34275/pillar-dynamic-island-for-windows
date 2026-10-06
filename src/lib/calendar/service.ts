import { mergeSnapshots } from "./select";
import type { CalendarProvider } from "./provider";
import { createRegisteredProviders, registerDefaultProviders } from "./registry";
import type { CalendarSnapshot } from "./types";

/**
 * The one place that owns the providers' lifecycle. It stays subscribed for as long as the
 * app runs (reminders need events while the island is collapsed) and hands the UI a single
 * merged snapshot with a stable identity between changes.
 */
export interface CalendarService {
  subscribe(listener: () => void): () => void;
  getSnapshot(): CalendarSnapshot;
  /** Asks every provider to sync (each throttles itself). Never rejects. */
  refresh(): Promise<void>;
  dispose(): void;
}

export function createCalendarService(providers: readonly CalendarProvider[]): CalendarService {
  const listeners = new Set<() => void>();
  const latest = providers.map((provider) => provider.getSnapshot());
  let merged = mergeSnapshots(latest);

  const unsubscribes = providers.map((provider, index) =>
    provider.subscribe((snapshot) => {
      latest[index] = snapshot;
      merged = mergeSnapshots(latest);
      for (const listener of [...listeners]) listener();
    })
  );

  const refresh = async () => {
    await Promise.all(providers.map((provider) => provider.refresh()));
  };

  // A hidden/shown island (fullscreen app, session unlock) may have missed pushes.
  const onVisibility = () => {
    if (!document.hidden) void refresh();
  };
  if (typeof document !== "undefined") document.addEventListener("visibilitychange", onVisibility);

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => merged,
    refresh,
    dispose() {
      if (typeof document !== "undefined") document.removeEventListener("visibilitychange", onVisibility);
      unsubscribes.forEach((off) => off());
      providers.forEach((provider) => provider.dispose());
      listeners.clear();
    },
  };
}

let instance: CalendarService | null = null;

/** The app-wide service, started on first use. */
export function getCalendarService(): CalendarService {
  if (!instance) {
    registerDefaultProviders();
    instance = createCalendarService(createRegisteredProviders());
  }
  return instance;
}
