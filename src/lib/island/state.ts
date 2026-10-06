// Island state manager: a pure reducer that decides what the island shows.
//
// Priority (docs/ENTERPRISE_DESIGN.md section 0):  meetingAlert (3) > notification (2) > userExpanded (1) > idle (0)
//  - A higher state preempts a lower one; a lower one never interrupts a higher one.
//  - What the user had open (expanded, which tab, pinned) lives in its own layer and is never
//    touched by temporary states, so it is simply shown again when they end.
//  - A notification that is preempted or arrives during a meeting alert waits (at most one, the
//    latest) and is shown afterwards only if it is still fresh.
//  - Timing belongs to the caller (useIslandState): every event that needs a clock carries `at`.

import type { TabId } from "../../components/Pill/tabs";
import type { IslandNotification } from "../ipc";
import type { ReminderAlert } from "../reminders/types";

export const PRIORITY = { meetingAlert: 3, notification: 2, userExpanded: 1, idle: 0 } as const;

/** A queued notification older than this is dropped instead of shown late. */
export const NOTIFICATION_STALE_MS = 15_000;
/** An alert whose meeting started longer ago than this is dropped instead of shown late. */
export const ALERT_STALE_AFTER_START_MS = 60_000;
/** Meeting alerts waiting behind the current one. Far above anything real; bounds a runaway producer. */
export const MAX_QUEUED_ALERTS = 8;

interface ReceivedNotification {
  notification: IslandNotification;
  receivedAt: number;
}

export interface IslandState {
  /** Last tab the user used; hover and toggles reopen on it. */
  tab: TabId;
  expanded: boolean;
  /** Opened by a click or a toggle: stays open longer once the pointer leaves. */
  pinned: boolean;
  alert: ReminderAlert | null;
  alertQueue: readonly ReminderAlert[];
  notification: ReceivedNotification | null;
  /** At most one: the latest notification that could not be shown yet. */
  waiting: ReceivedNotification | null;
}

export type IslandEvent =
  | { type: "ALERT_SHOW"; alert: ReminderAlert }
  | { type: "ALERT_DONE"; at: number }
  | { type: "NOTIFICATION_SHOW"; notification: IslandNotification; at: number }
  | { type: "NOTIFICATION_DONE" }
  | { type: "USER_EXPAND"; tab?: TabId }
  | { type: "USER_COLLAPSE" }
  | { type: "PIN"; tab?: TabId }
  /** Time passed with nothing else happening (e.g. the island was hidden): refreshes or drops stale alerts and forgets a stale waiting notification. */
  | { type: "TICK"; at: number };

export type IslandView =
  | { kind: "meetingAlert"; alert: ReminderAlert }
  | { kind: "notification"; notification: IslandNotification }
  | { kind: "userExpanded"; tab: TabId; pinned: boolean }
  | { kind: "idle" };

export const initialIslandState: IslandState = {
  tab: "datetime",
  expanded: false,
  pinned: false,
  alert: null,
  alertQueue: [],
  notification: null,
  waiting: null,
};

export function selectView(state: IslandState): IslandView {
  if (state.alert) return { kind: "meetingAlert", alert: state.alert };
  if (state.notification) return { kind: "notification", notification: state.notification.notification };
  if (state.expanded) return { kind: "userExpanded", tab: state.tab, pinned: state.pinned };
  return { kind: "idle" };
}

export function viewPriority(view: IslandView): number {
  return PRIORITY[view.kind];
}

function isFresh(received: ReceivedNotification | null, at: number): received is ReceivedNotification {
  return received !== null && at - received.receivedAt < NOTIFICATION_STALE_MS;
}

/**
 * An alert that waited behind a hidden window must not say "in 30 minutes" about a meeting that
 * is 10 minutes away or already over: the minutes only ever go down, and a meeting that started
 * is dropped (null).
 */
function refreshAlert(alert: ReminderAlert, at: number): ReminderAlert | null {
  const startMs = Date.parse(alert.startUtc);
  if (Number.isNaN(startMs)) return alert;
  if (startMs + ALERT_STALE_AFTER_START_MS <= at) return null;
  const minutesRemaining = Math.min(alert.minutesRemaining, Math.max(0, Math.round((startMs - at) / 60_000)));
  return minutesRemaining === alert.minutesRemaining ? alert : { ...alert, minutesRemaining };
}

export function islandReducer(state: IslandState, event: IslandEvent): IslandState {
  switch (event.type) {
    case "ALERT_SHOW": {
      const { alert } = event;
      if (state.alert?.key === alert.key || state.alertQueue.some((queued) => queued.key === alert.key)) return state;
      if (state.alert) return { ...state, alertQueue: [...state.alertQueue, alert].slice(-MAX_QUEUED_ALERTS) };
      // Preempts a toast that is on screen: it waits and comes back if it is still fresh.
      return { ...state, alert, notification: null, waiting: state.notification ?? state.waiting };
    }

    case "ALERT_DONE": {
      if (!state.alert) return state;
      const [next, ...rest] = state.alertQueue;
      if (next) return { ...state, alert: next, alertQueue: rest };
      return {
        ...state,
        alert: null,
        alertQueue: [],
        waiting: null,
        notification: isFresh(state.waiting, event.at) ? state.waiting : null,
      };
    }

    case "NOTIFICATION_SHOW": {
      const received = { notification: event.notification, receivedAt: event.at };
      if (state.alert) return { ...state, waiting: received };
      return { ...state, notification: received };
    }

    case "NOTIFICATION_DONE":
      return state.notification ? { ...state, notification: null } : state;

    case "USER_EXPAND":
      return { ...state, expanded: true, tab: event.tab ?? state.tab };

    case "PIN":
      return { ...state, expanded: true, pinned: true, tab: event.tab ?? state.tab };

    case "USER_COLLAPSE":
      return state.expanded || state.pinned ? { ...state, expanded: false, pinned: false } : state;

    case "TICK": {
      let next = state;
      const queue = state.alertQueue.map((queued) => refreshAlert(queued, event.at));
      if (queue.some((queued, i) => queued !== state.alertQueue[i])) {
        next = { ...next, alertQueue: queue.filter((queued): queued is ReminderAlert => queued !== null) };
      }
      if (next.alert) {
        const refreshed = refreshAlert(next.alert, event.at);
        if (refreshed === null) next = islandReducer(next, { type: "ALERT_DONE", at: event.at });
        else if (refreshed !== next.alert) next = { ...next, alert: refreshed };
      }
      return next.waiting && !isFresh(next.waiting, event.at) ? { ...next, waiting: null } : next;
    }
  }
}
