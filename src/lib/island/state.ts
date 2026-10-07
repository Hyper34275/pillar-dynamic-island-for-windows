// Island state manager: a pure reducer that decides what the island shows.
//
// Priority (docs/ENTERPRISE_DESIGN.md section 0):  meetingAlert (3) > ringer (2.5) > notification (2) > userExpanded (1) > idle (0)
//  - A higher state preempts a lower one; a lower one never interrupts a higher one.
//  - What the user had open (expanded, which tab, pinned) lives in its own layer and is never
//    touched by temporary states, so it is simply shown again when they end.
//  - Notifications form one SESSION: the island opens once for a burst, shows every notification
//    in turn (`notification` is the current one, `notificationQueue` the pending ones, FIFO) and
//    closes once, after a short grace in which a late arrival is still shown without a close and a
//    reopen in between. Nothing here delays a new session: the geometry engine retargets from
//    whatever shape the island is in, which is how a close is "reversed" by a new arrival.
//  - While the user has the panel open a normal notification is not shown at all (it is in the
//    history already); a time-sensitive one (a meeting invitation) is deferred until it closes.
//  - Timing belongs to the caller (useIslandState): every event that needs a clock carries `at`.

import type { TabId } from "../../components/Pill/tabs";
import type { IslandNotification } from "../ipc";
import type { ReminderAlert } from "../reminders/types";

export const PRIORITY = { meetingAlert: 3, ringer: 2.5, notification: 2, userExpanded: 1, idle: 0 } as const;

/**
 * A notification that WAITED (behind a meeting alert or a hidden window) longer than this is
 * dropped instead of shown late. A long backlog that simply plays in order is never judged by it.
 */
export const NOTIFICATION_STALE_MS = 15_000;
/** An alert whose meeting started longer ago than this is dropped instead of shown late. */
export const ALERT_STALE_AFTER_START_MS = 60_000;
/** Meeting alerts waiting behind the current one. Far above anything real; bounds a runaway producer. */
export const MAX_QUEUED_ALERTS = 8;
/**
 * Notifications waiting behind the current one. Beyond it the oldest pending are dropped: they are
 * in the Notification Center history already, and nobody reads a ninth toast in a row.
 */
export const MAX_QUEUED_NOTIFICATIONS = 8;
/** Time-sensitive notifications kept while the panel is open (the latest few). */
export const MAX_DEFERRED_NOTIFICATIONS = 3;
/** A deferred notification still starts a session when the panel closes within this long of its arrival. */
export const DEFERRED_FRESH_MS = 60_000;

/**
 * How much it may interrupt. There is no priority information from Windows, so only a meeting
 * invitation (it has buttons and a deadline) is above normal; "critical" is the meeting alert
 * channel, which does not go through notifications at all.
 */
export type NotificationLevel = "normal" | "timeSensitive" | "critical";

export function notificationLevel(notification: IslandNotification): NotificationLevel {
  return notification.invite ? "timeSensitive" : "normal";
}

/**
 * The ring / silent pill a meeting's start (or end) brings up. A click toggles silent, which
 * holds notifications until `untilMs` (the end of the meeting); "end" just says it rings again.
 */
export interface Ringer {
  key: string;
  phase: "start" | "end";
  silent: boolean;
  untilMs: number;
  /** How often it was toggled: each toggle restarts (and shortens) its time on screen. */
  toggles: number;
}

export interface ReceivedNotification {
  notification: IslandNotification;
  receivedAt: number;
}

/** The notification the session is on now. */
export interface CurrentNotification extends ReceivedNotification {
  /** When it became current (a clock reading): a current one that is far older than its dwell sat behind a hidden window. */
  shownAt: number;
  /** "showing": its dwell is running. "lingering": the dwell is over, nothing is pending, the grace runs. */
  phase: "showing" | "lingering";
}

export interface IslandState {
  /** Last tab the user used; hover and toggles reopen on it. */
  tab: TabId;
  expanded: boolean;
  /** Opened by a click or a toggle: stays open longer once the pointer leaves. */
  pinned: boolean;
  alert: ReminderAlert | null;
  alertQueue: readonly ReminderAlert[];
  /** The session's current notification. */
  notification: CurrentNotification | null;
  /** Pending notifications of the session, first to show first. At most MAX_QUEUED_NOTIFICATIONS. */
  notificationQueue: readonly ReceivedNotification[];
  /** Time-sensitive notifications that arrived while the panel was open (at most MAX_DEFERRED_NOTIFICATIONS). */
  deferred: readonly ReceivedNotification[];
  /** Counts the sessions an arrival opened on a free island (instrumentation). */
  sessionId: number;
  /** Changes whenever a notification becomes current (again): keys its dwell countdown. */
  shownGeneration: number;
  /** Pending notifications dropped because of the cap (diagnostics). */
  droppedFromQueue: number;
  ringer: Ringer | null;
}

export type IslandEvent =
  | { type: "ALERT_SHOW"; alert: ReminderAlert }
  | { type: "ALERT_DONE"; at: number }
  | { type: "NOTIFICATION_SHOW"; notification: IslandNotification; at: number }
  /** The current toast's dwell ran out: the next one shows at once, or the grace starts. */
  | { type: "NOTIFICATION_DWELL_DONE"; at: number }
  /** The grace ran out with nothing arriving: the session ends. */
  | { type: "NOTIFICATION_GRACE_DONE" }
  /** The user dismissed the current toast (X, Delete, Escape on it): the next shows at once, or the session ends with no grace. */
  | { type: "NOTIFICATION_DONE"; at?: number }
  /** The user left the whole session (swipe, Escape on the island, a click that opens the source app): the queue goes with it. */
  | { type: "NOTIFICATION_SESSION_END" }
  | { type: "USER_EXPAND"; tab?: TabId }
  /** `at` lets a deferred notification start a session; without it the deferred ones are forgotten. */
  | { type: "USER_COLLAPSE"; at?: number }
  | { type: "PIN"; tab?: TabId }
  | { type: "RINGER_SHOW"; ringer: Omit<Ringer, "toggles"> }
  | { type: "RINGER_TOGGLE" }
  | { type: "RINGER_DONE" }
  /** Time passed with nothing else happening (e.g. the island was hidden): refreshes or drops stale alerts and notifications that waited. */
  | { type: "TICK"; at: number };

export type IslandView =
  | { kind: "meetingAlert"; alert: ReminderAlert }
  | { kind: "ringer"; ringer: Ringer }
  | {
      kind: "notification";
      notification: IslandNotification;
      /** Notifications waiting behind this one. */
      queueLength: number;
      phase: "showing" | "lingering";
      sessionId: number;
    }
  | { kind: "userExpanded"; tab: TabId; pinned: boolean }
  | { kind: "idle" };

export const initialIslandState: IslandState = {
  tab: "calendar",
  expanded: false,
  pinned: false,
  alert: null,
  alertQueue: [],
  notification: null,
  notificationQueue: [],
  deferred: [],
  sessionId: 0,
  shownGeneration: 0,
  droppedFromQueue: 0,
  ringer: null,
};

export function selectView(state: IslandState): IslandView {
  if (state.alert) return { kind: "meetingAlert", alert: state.alert };
  if (state.ringer) return { kind: "ringer", ringer: state.ringer };
  // The panel the user opened is never hidden by a toast: opening it ends the session, and while
  // it is open nothing becomes current, so the two never coexist in the state; this order only
  // makes that explicit.
  if (state.expanded) return { kind: "userExpanded", tab: state.tab, pinned: state.pinned };
  if (state.notification) {
    return {
      kind: "notification",
      notification: state.notification.notification,
      queueLength: state.notificationQueue.length,
      phase: state.notification.phase,
      sessionId: state.sessionId,
    };
  }
  return { kind: "idle" };
}

export function viewPriority(view: IslandView): number {
  return PRIORITY[view.kind];
}

function isFresh(received: ReceivedNotification, at: number): boolean {
  return at - received.receivedAt < NOTIFICATION_STALE_MS;
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

// ---------------------------------------------------------------------------
// Notification session helpers
// ---------------------------------------------------------------------------

/** `received` becomes the current notification. `opens`: this arrival opened the session. */
function makeCurrent(state: IslandState, received: ReceivedNotification, at: number, opens: boolean): IslandState {
  return {
    ...state,
    notification: { ...received, shownAt: at, phase: "showing" },
    shownGeneration: state.shownGeneration + 1,
    sessionId: opens ? state.sessionId + 1 : state.sessionId,
  };
}

/** The first of the queue becomes current, or (null) the session has nothing more. */
function advance(state: IslandState, at: number): IslandState | null {
  const [next, ...rest] = state.notificationQueue;
  return next ? makeCurrent({ ...state, notificationQueue: rest }, next, at, false) : null;
}

/**
 * Appends to the queue, over the cap the oldest pending goes (it is in the history). The "You
 * missed N" summary is never the one dropped: it is the whole point of a replay.
 */
function enqueue(state: IslandState, queue: readonly ReceivedNotification[]): IslandState {
  let next = queue;
  let dropped = 0;
  while (next.length > MAX_QUEUED_NOTIFICATIONS) {
    const index = next.findIndex((received) => !received.notification.missedSummary);
    const at = index === -1 ? 0 : index;
    next = next.filter((_, i) => i !== at);
    dropped += 1;
  }
  return { ...state, notificationQueue: next, droppedFromQueue: state.droppedFromQueue + dropped };
}

/** A notification that may be shown now (the panel is not open): current, queued or an inline update. */
function admit(state: IslandState, received: ReceivedNotification, at: number): IslandState {
  const { id } = received.notification;
  const current = state.notification;
  // Same id as the one on screen: the payload changes in place, nothing restarts.
  if (current && current.notification.id === id) {
    return { ...state, notification: { ...current, notification: received.notification } };
  }
  // Same id as a pending one: replaced where it stands.
  const index = state.notificationQueue.findIndex((queued) => queued.notification.id === id);
  if (index !== -1) {
    return { ...state, notificationQueue: state.notificationQueue.map((queued, i) => (i === index ? received : queued)) };
  }
  // Behind a meeting alert (or behind pending ones): wait in line.
  if (state.alert || state.notificationQueue.length > 0) return enqueue(state, [...state.notificationQueue, received]);
  if (!current) return makeCurrent(state, received, at, true);
  // Grace: the session is still open, the late arrival simply takes the screen.
  if (current.phase === "lingering") return makeCurrent(state, received, at, false);
  return enqueue(state, [...state.notificationQueue, received]);
}

/** The user chose the panel (or left the session): everything of the session goes, it is all in the history. */
function clearSession(state: IslandState): IslandState {
  return state.notification || state.notificationQueue.length > 0 ? { ...state, notification: null, notificationQueue: [] } : state;
}

/**
 * Throws if the notification session is in a state it must never reach. Run by the reducer in
 * development and tests (import.meta.env.DEV): a session that goes to Compact with pending items
 * would silently lose them.
 */
export function assertNotificationInvariants(state: IslandState): void {
  const { notification, notificationQueue: queue } = state;
  if (queue.length > 0 && !notification && !state.alert && !state.ringer) {
    throw new Error("notification session: the queue is not empty but nothing is showing");
  }
  if (queue.length > MAX_QUEUED_NOTIFICATIONS) throw new Error(`notification session: the queue holds ${queue.length} (max ${MAX_QUEUED_NOTIFICATIONS})`);
  const seen = new Set<number>();
  if (notification) seen.add(notification.notification.id);
  for (const { notification: queued } of queue) {
    if (seen.has(queued.id)) throw new Error(`notification session: id ${queued.id} is in the session twice`);
    seen.add(queued.id);
  }
  if (state.deferred.length > MAX_DEFERRED_NOTIFICATIONS) throw new Error("notification session: too many deferred notifications");
}

function reduce(state: IslandState, event: IslandEvent): IslandState {
  switch (event.type) {
    case "ALERT_SHOW": {
      const { alert } = event;
      if (state.alert?.key === alert.key || state.alertQueue.some((queued) => queued.key === alert.key)) return state;
      if (state.alert) return { ...state, alertQueue: [...state.alertQueue, alert].slice(-MAX_QUEUED_ALERTS) };
      // Preempts the toast on screen, panel included: the toast goes back to the FRONT of the
      // line (its dwell starts over) and the queue waits behind the alert. One that already had
      // its whole dwell (lingering) is not shown a second time.
      const current = state.notification;
      const back = current && current.phase === "showing" ? [{ notification: current.notification, receivedAt: current.receivedAt }] : [];
      return { ...enqueue(state, [...back, ...state.notificationQueue]), alert, notification: null };
    }

    case "ALERT_DONE": {
      if (!state.alert) return state;
      const [next, ...rest] = state.alertQueue;
      if (next) return { ...state, alert: next, alertQueue: rest };
      // The session continues with what is still fresh: these waited behind the alert.
      const fresh = state.notificationQueue.filter((received) => isFresh(received, event.at));
      const done = { ...state, alert: null, alertQueue: [], notificationQueue: fresh };
      return advance(done, event.at) ?? done;
    }

    case "NOTIFICATION_SHOW": {
      const received = { notification: event.notification, receivedAt: event.at };
      if (state.expanded) {
        // The panel is open: it is never covered. Normal ones are in the history already; a
        // time-sensitive one waits for the panel to close.
        if (notificationLevel(event.notification) === "normal") return state;
        const rest = state.deferred.filter((d) => d.notification.id !== event.notification.id);
        return { ...state, deferred: [...rest, received].slice(-MAX_DEFERRED_NOTIFICATIONS) };
      }
      return admit(state, received, event.at);
    }

    case "NOTIFICATION_DWELL_DONE": {
      const current = state.notification;
      if (!current || current.phase !== "showing") return state;
      const advanced = advance(state, event.at);
      if (advanced) return advanced;
      return { ...state, notification: { ...current, phase: "lingering" } };
    }

    case "NOTIFICATION_GRACE_DONE":
      return state.notification?.phase === "lingering" ? { ...state, notification: null } : state;

    case "NOTIFICATION_DONE": {
      if (!state.notification) return state;
      return advance({ ...state, notification: null }, event.at ?? state.notification.shownAt) ?? { ...state, notification: null };
    }

    case "NOTIFICATION_SESSION_END":
      return clearSession(state);

    case "RINGER_SHOW":
      return { ...state, ringer: { ...event.ringer, toggles: 0 } };

    case "RINGER_TOGGLE":
      return state.ringer ? { ...state, ringer: { ...state.ringer, silent: !state.ringer.silent, toggles: state.ringer.toggles + 1 } } : state;

    case "RINGER_DONE":
      return state.ringer ? { ...state, ringer: null } : state;

    case "USER_EXPAND":
      return { ...clearSession(state), expanded: true, tab: event.tab ?? state.tab };

    case "PIN":
      return { ...clearSession(state), expanded: true, pinned: true, tab: event.tab ?? state.tab };

    case "USER_COLLAPSE": {
      if (!state.expanded && !state.pinned) return state;
      let next: IslandState = { ...state, expanded: false, pinned: false, deferred: [] };
      const { at } = event;
      if (at !== undefined) {
        for (const received of state.deferred) {
          if (at - received.receivedAt < DEFERRED_FRESH_MS) next = admit(next, received, at);
        }
      }
      return next;
    }

    case "TICK": {
      let next = state;
      const queue = state.alertQueue.map((queued) => refreshAlert(queued, event.at));
      if (queue.some((queued, i) => queued !== state.alertQueue[i])) {
        next = { ...next, alertQueue: queue.filter((queued): queued is ReminderAlert => queued !== null) };
      }
      if (next.alert) {
        const refreshed = refreshAlert(next.alert, event.at);
        if (refreshed === null) next = reduce(next, { type: "ALERT_DONE", at: event.at });
        else if (refreshed !== next.alert) next = { ...next, alert: refreshed };
      }
      const deferred = next.deferred.filter((received) => event.at - received.receivedAt < DEFERRED_FRESH_MS);
      if (deferred.length !== next.deferred.length) next = { ...next, deferred };

      // What waited (behind an alert, or a window that was hidden) may have gone stale. The current
      // toast is judged by how long it has been current: a normal backlog never lets one go
      // past a dwell, so only a hidden window gets here, and then the queue waited with it.
      const current = next.notification;
      const waitedQueue = next.notificationQueue.filter((received) => isFresh(received, event.at));
      if (current && event.at - current.shownAt >= NOTIFICATION_STALE_MS) {
        const rest = { ...next, notification: null, notificationQueue: waitedQueue };
        return advance(rest, event.at) ?? rest;
      }
      if (!current && waitedQueue.length !== next.notificationQueue.length) next = { ...next, notificationQueue: waitedQueue };
      return next;
    }
  }
}

export function islandReducer(state: IslandState, event: IslandEvent): IslandState {
  const next = reduce(state, event);
  if (import.meta.env.DEV) assertNotificationInvariants(next);
  return next;
}
