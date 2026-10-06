// "Silent" for the rest of a meeting: while it lasts, Windows notifications and meeting
// invitations do not pop up in the island (they still go to the Notifications tab and the
// unseen count). Meeting reminders are not affected. Memory only: a restart rings again.

import { useSyncExternalStore } from "react";

export interface Silence {
  /** Silent until `untilMs` (the end of the meeting). */
  until(untilMs: number): void;
  clear(): void;
  isSilent(nowMs: number): boolean;
  subscribe(listener: () => void): () => void;
  /** When the silence ends, or null when the island rings. */
  getSnapshot(): number | null;
}

export function createSilence(): Silence {
  let untilMs: number | null = null;
  const listeners = new Set<() => void>();
  const set = (next: number | null) => {
    if (next === untilMs) return;
    untilMs = next;
    listeners.forEach((listener) => listener());
  };
  return {
    until: (ms) => set(ms),
    clear: () => set(null),
    isSilent(nowMs) {
      if (untilMs !== null && nowMs >= untilMs) set(null);
      return untilMs !== null;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => untilMs,
  };
}

export const silence = createSilence();

/** The end of the current silence (null = ringing). Re-renders when it is set or cleared. */
export function useSilenceUntil(store: Silence = silence): number | null {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
