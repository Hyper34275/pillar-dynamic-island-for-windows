import { useEffect, useRef, useState } from "react";
import type { IslandNotification } from "../lib/ipc";
import { doNotDisturb as defaultDnd, type DoNotDisturb } from "../lib/island/dnd";
import type { IslandState } from "../lib/island/state";
import { notificationHistory, type NotificationHistory } from "../lib/notifications/history";
import { missedQueue, missedReplay, type MissedQueue } from "../lib/notifications/missed";

/** A breath between two replayed toasts, so they read as one after another rather than one changing. */
export const REPLAY_GAP_MS = 450;

interface Stores {
  dnd?: DoNotDisturb;
  queue?: MissedQueue;
  history?: NotificationHistory;
}

/**
 * When Do not disturb ends (the bell, or Windows' quick settings), shows "You missed N
 * notifications" and then every notification it held, one after another, as ordinary toasts.
 * Each waits for the island to be free: no toast, meeting alert or ring pill on screen, and the
 * panel closed (a toast would cover it). Looking at the Notifications tab ends the replay: the
 * missed ones are listed there ("Missed while muted"), and that is where the summary leads.
 * A new Do not disturb starts a new batch.
 */
export function useMissedReplay(
  state: Pick<IslandState, "notification" | "alert" | "ringer" | "expanded" | "tab">,
  show: (notification: IslandNotification) => void,
  { dnd = defaultDnd, queue = missedQueue, history = notificationHistory }: Stores = {}
): void {
  const pending = useRef<IslandNotification[]>([]);
  const [batch, setBatch] = useState(0);

  useEffect(() => {
    let previous = dnd.getSnapshot();
    return dnd.subscribe(() => {
      const next = dnd.getSnapshot();
      if (next === true && previous !== true) {
        queue.clear();
        history.clearMissed();
        pending.current = [];
      } else if (next === false && previous === true) {
        pending.current = missedReplay(queue, Date.now());
        if (pending.current.length > 0) setBatch((n) => n + 1);
      }
      previous = next;
    });
  }, [dnd, queue, history]);

  const viewingList = state.expanded && state.tab === "notifications";
  const busy = state.notification !== null || state.alert !== null || state.ringer !== null || state.expanded;

  useEffect(() => {
    if (viewingList) {
      pending.current = [];
      return;
    }
    if (busy || pending.current.length === 0) return;
    const handle = setTimeout(() => {
      const next = pending.current.shift();
      if (next) show(next);
    }, REPLAY_GAP_MS);
    return () => clearTimeout(handle);
  }, [viewingList, busy, batch, show]);
}
