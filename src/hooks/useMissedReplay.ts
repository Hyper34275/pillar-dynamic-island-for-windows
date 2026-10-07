import { useEffect, useRef } from "react";
import type { IslandNotification } from "../lib/ipc";
import { doNotDisturb as defaultDnd, type DoNotDisturb } from "../lib/island/dnd";
import { notificationHistory, type NotificationHistory } from "../lib/notifications/history";
import { missedQueue, missedReplay, type MissedQueue } from "../lib/notifications/missed";

interface Stores {
  dnd?: DoNotDisturb;
  queue?: MissedQueue;
  history?: NotificationHistory;
}

/**
 * When Do not disturb ends (the bell, or Windows' quick settings), shows "You missed N
 * notifications" and then every notification it held, as ordinary toasts. They are all handed to
 * the island at once, in order: its notification session queues and sequences them, so the island
 * opens once and closes once for the whole replay (the queue keeps the summary and the latest
 * eight; the Notification Center lists every one). There is no waiting for a free island here: a
 * meeting alert holds the queue behind it, and the open panel simply does not show normal
 * notifications, which is also how looking at the Notifications tab ends the replay.
 * A new Do not disturb starts a new batch.
 */
export function useMissedReplay(
  show: (notification: IslandNotification) => void,
  { dnd = defaultDnd, queue = missedQueue, history = notificationHistory }: Stores = {}
): void {
  const showRef = useRef(show);
  showRef.current = show;

  useEffect(() => {
    let previous = dnd.getSnapshot();
    return dnd.subscribe(() => {
      const next = dnd.getSnapshot();
      if (next === true && previous !== true) {
        queue.clear();
        history.clearMissed();
      } else if (next === false && previous === true) {
        for (const notification of missedReplay(queue, Date.now())) showRef.current(notification);
      }
      previous = next;
    });
  }, [dnd, queue, history]);
}
