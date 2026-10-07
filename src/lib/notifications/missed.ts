// What Do not disturb held back. While the bell is on, every notification (and meeting invitation)
// that would have popped up is kept here, in the order it arrived; when Do not disturb is turned
// off (the bell, or Windows' own quick settings) the island says how many were missed and then
// shows them one after another like ordinary notifications (useMissedReplay). Memory only, like
// the rest of the notification history: a restart forgets them.

import { t } from "../i18n";
import type { IslandNotification } from "../ipc";

/** Held notifications kept for the replay (the oldest fall off); the count still counts them all. */
export const MISSED_MAX = 50;

export interface MissedQueue {
  hold(notification: IslandNotification): void;
  /** Everything held, oldest first, plus how many arrived in all; empties the queue. */
  take(): { notifications: IslandNotification[]; count: number };
  /** A new Do not disturb has started (nothing from before it is replayed). */
  clear(): void;
  size(): number;
}

export function createMissedQueue(max = MISSED_MAX): MissedQueue {
  let held: IslandNotification[] = [];
  let count = 0;
  return {
    hold(notification) {
      if (held.some((n) => n.id === notification.id)) return;
      held = [...held, notification].slice(-max);
      count += 1;
    },
    take() {
      const out = { notifications: held, count };
      held = [];
      count = 0;
      return out;
    },
    clear() {
      held = [];
      count = 0;
    },
    size: () => count,
  };
}

export const missedQueue = createMissedQueue();

let summaryIds = 0;

/** The island's own "You missed N notifications" toast (it is not in the history). */
export function missedSummary(count: number, now: number): IslandNotification {
  summaryIds += 1;
  return {
    // Backend ids are positive; the summary's never collide with them.
    id: -summaryIds,
    appName: t("missed.source"),
    title: t("missed.title", { n: count }),
    body: t("missed.body"),
    timestamp: now,
    aumid: null,
    missedSummary: { count },
  };
}

/** What plays when Do not disturb ends: the summary, then every held notification, oldest first. */
export function missedReplay(queue: MissedQueue, now: number): IslandNotification[] {
  const { notifications, count } = queue.take();
  return count === 0 ? [] : [missedSummary(count, now), ...notifications];
}
