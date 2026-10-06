// The notifications of this app session, for the Notifications tab. Memory only, like the toasts
// themselves: never persisted, never logged, gone when the app restarts.

import { useSyncExternalStore } from "react";
import type { IslandNotification } from "../ipc";

/** Newest kept; older ones fall off. */
export const HISTORY_MAX = 50;

export interface HistoryEntry {
  notification: IslandNotification;
  receivedAt: number;
  /** It arrived while notifications were silenced for a meeting, so it never popped up. */
  silenced: boolean;
}

export interface NotificationHistory {
  add(notification: IslandNotification, receivedAt: number, silenced: boolean): void;
  remove(id: number): void;
  clear(): void;
  subscribe(listener: () => void): () => void;
  /** Newest first. The array is replaced on every change, so it can be compared by identity. */
  getSnapshot(): readonly HistoryEntry[];
}

export function createNotificationHistory(max = HISTORY_MAX): NotificationHistory {
  let entries: readonly HistoryEntry[] = [];
  const listeners = new Set<() => void>();
  const set = (next: readonly HistoryEntry[]) => {
    entries = next;
    listeners.forEach((listener) => listener());
  };
  return {
    add(notification, receivedAt, silenced) {
      const rest = entries.filter((entry) => entry.notification.id !== notification.id);
      set([{ notification, receivedAt, silenced }, ...rest].slice(0, max));
    },
    remove(id) {
      if (entries.some((entry) => entry.notification.id === id)) set(entries.filter((entry) => entry.notification.id !== id));
    },
    clear() {
      if (entries.length > 0) set([]);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => entries,
  };
}

export const notificationHistory = createNotificationHistory();

export function useNotificationHistory(history: NotificationHistory = notificationHistory): readonly HistoryEntry[] {
  return useSyncExternalStore(history.subscribe, history.getSnapshot, history.getSnapshot);
}
