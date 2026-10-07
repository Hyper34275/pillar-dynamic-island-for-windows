// The notifications of this app session, for the Notifications tab. Memory only, like the toasts
// themselves: never persisted, never logged, gone when the app restarts.

import { useSyncExternalStore } from "react";
import type { IslandNotification } from "../ipc";

/** Newest kept; older ones fall off. */
export const HISTORY_MAX = 50;

export interface HistoryEntry {
  notification: IslandNotification;
  receivedAt: number;
  /** It arrived while notifications were silenced (a meeting, or Do not disturb), so it never popped up. */
  silenced: boolean;
  /** It arrived during the latest Do not disturb: the Notifications tab lists it under "Missed while muted". */
  missed?: boolean;
}

export interface NotificationHistory {
  add(notification: IslandNotification, receivedAt: number, silenced: boolean, missed?: boolean): void;
  /** A new Do not disturb has started: what the previous one held is ordinary history now. */
  clearMissed(): void;
  remove(id: number): void;
  clear(): void;
  /**
   * The island was open and has just closed: everything received so far counts as seen from now
   * on. Until then an entry is unread when it arrived after the previous close, so the dots stay
   * put for the whole time the panel is open (and are right on its first frame).
   */
  markViewed(now: number): void;
  /** When the panel was last closed (0: never); entries received after it are unread. */
  getLastViewedAt(): number;
  subscribe(listener: () => void): () => void;
  /** Newest first. The array is replaced on every change, so it can be compared by identity. */
  getSnapshot(): readonly HistoryEntry[];
}

export function createNotificationHistory(max = HISTORY_MAX): NotificationHistory {
  let entries: readonly HistoryEntry[] = [];
  let lastViewedAt = 0;
  const listeners = new Set<() => void>();
  const set = (next: readonly HistoryEntry[]) => {
    entries = next;
    listeners.forEach((listener) => listener());
  };
  return {
    add(notification, receivedAt, silenced, missed = false) {
      const rest = entries.filter((entry) => entry.notification.id !== notification.id);
      set([{ notification, receivedAt, silenced, ...(missed ? { missed } : {}) }, ...rest].slice(0, max));
    },
    clearMissed() {
      if (entries.some((entry) => entry.missed)) set(entries.map((entry) => (entry.missed ? { notification: entry.notification, receivedAt: entry.receivedAt, silenced: entry.silenced } : entry)));
    },
    remove(id) {
      if (entries.some((entry) => entry.notification.id === id)) set(entries.filter((entry) => entry.notification.id !== id));
    },
    clear() {
      if (entries.length > 0) set([]);
    },
    markViewed(now) {
      lastViewedAt = now;
    },
    getLastViewedAt: () => lastViewedAt,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => entries,
  };
}

/** Whether an entry arrived since the panel was last closed. */
export function isUnread(entry: HistoryEntry, lastViewedAt: number): boolean {
  return entry.receivedAt > lastViewedAt;
}

export const notificationHistory = createNotificationHistory();

export function useNotificationHistory(history: NotificationHistory = notificationHistory): readonly HistoryEntry[] {
  return useSyncExternalStore(history.subscribe, history.getSnapshot, history.getSnapshot);
}
