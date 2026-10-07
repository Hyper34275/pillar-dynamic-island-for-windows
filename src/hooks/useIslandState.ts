import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { TabId } from "../components/Pill/tabs";
import type { IslandNotification } from "../lib/ipc";
import { initialIslandState, islandReducer, selectView, type IslandState, type IslandView, type Ringer } from "../lib/island/state";
import { ALERT_MS, INVITE_TOAST_MS, NOTIFICATION_GRACE_MS, notificationDwellMs } from "../lib/island/timing";
import type { ReminderAlert } from "../lib/reminders/types";

/**
 * Runs `onDone` once `durationMs` of un-paused time has passed since `activeKey` appeared.
 * Pausing (hover, hidden island) keeps the time already used; a new key starts over.
 */
function useCountdown(activeKey: string | null, durationMs: number, paused: boolean, onDone: () => void): void {
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  const keyRef = useRef<string | null>(null);
  const remainingRef = useRef(durationMs);

  useEffect(() => {
    if (activeKey === null) {
      keyRef.current = null;
      return;
    }
    if (keyRef.current !== activeKey) {
      keyRef.current = activeKey;
      remainingRef.current = durationMs;
    }
    if (paused) return;
    const startedAt = Date.now();
    const handle = setTimeout(() => doneRef.current(), remainingRef.current);
    return () => {
      clearTimeout(handle);
      remainingRef.current = Math.max(0, remainingRef.current - (Date.now() - startedAt));
    };
  }, [activeKey, durationMs, paused]);
}

export interface IslandController {
  state: IslandState;
  view: IslandView;
  showAlert: (alert: ReminderAlert) => void;
  showNotification: (notification: IslandNotification) => void;
  dismissAlert: () => void;
  /** The user dismissed the current toast: the next one shows at once, or the session ends. */
  dismissNotification: () => void;
  /** The user left the notifications altogether (swipe, Escape, opening the source app): the queue is dropped too. */
  endNotificationSession: () => void;
  /** Expand (to `tab`, or the last used one). */
  expand: (tab?: TabId) => void;
  /** Expand and keep it open until toggled, or the pointer has left for a while. */
  pin: (tab?: TabId) => void;
  collapse: () => void;
  /** The pointer is on the island: temporary states hold their remaining time. */
  setHovering: (hovering: boolean) => void;
  /** The ring / silent pill (a meeting started or ended). */
  showRinger: (ringer: Omit<Ringer, "toggles">) => void;
  toggleRinger: () => void;
  dismissRinger: () => void;
}

/** The ring / silent pill stays this long, and this long after each tap. */
export const RINGER_MS = 6_000;
export const RINGER_AFTER_TOGGLE_MS = 1_800;
export { INVITE_TOAST_MS };

interface UseIslandStateOptions {
  /** The window is hidden (fullscreen app in front): temporary states wait until it is back. */
  suppressed?: boolean;
}

/** The island state reducer plus the timers of its temporary states. The reducer itself has no clock. */
export function useIslandState({ suppressed = false }: UseIslandStateOptions = {}): IslandController {
  const [state, dispatch] = useReducer(islandReducer, initialIslandState);
  const [hovering, setHovering] = useState(false);
  const view = useMemo(() => selectView(state), [state]);
  const paused = hovering || suppressed;

  // Stable for the life of the component: effects can list them without re-running on every state change.
  const actions = useMemo(
    () => ({
      showAlert: (alert: ReminderAlert) => dispatch({ type: "ALERT_SHOW", alert }),
      showNotification: (notification: IslandNotification) => dispatch({ type: "NOTIFICATION_SHOW", notification, at: Date.now() }),
      dismissAlert: () => dispatch({ type: "ALERT_DONE", at: Date.now() }),
      dismissNotification: () => dispatch({ type: "NOTIFICATION_DONE", at: Date.now() }),
      endNotificationSession: () => dispatch({ type: "NOTIFICATION_SESSION_END" }),
      endNotificationDwell: () => dispatch({ type: "NOTIFICATION_DWELL_DONE", at: Date.now() }),
      endNotificationGrace: () => dispatch({ type: "NOTIFICATION_GRACE_DONE" }),
      expand: (tab?: TabId) => dispatch({ type: "USER_EXPAND", tab }),
      pin: (tab?: TabId) => dispatch({ type: "PIN", tab }),
      collapse: () => dispatch({ type: "USER_COLLAPSE", at: Date.now() }),
      setHovering,
      showRinger: (ringer: Omit<Ringer, "toggles">) => dispatch({ type: "RINGER_SHOW", ringer }),
      toggleRinger: () => dispatch({ type: "RINGER_TOGGLE" }),
      dismissRinger: () => dispatch({ type: "RINGER_DONE" }),
    }),
    []
  );

  useCountdown(state.alert?.key ?? null, ALERT_MS, paused, actions.dismissAlert);
  // Behind a higher state a temporary one keeps its time: it has not been seen yet.
  // The current toast's dwell is keyed by its id and the generation in which it became current
  // (it changes when it comes back after an alert), so a growing queue never restarts it. The
  // dwell length is taken from the queue as it was when the toast became current, and kept: the
  // countdown only starts over on a new key, so later arrivals do not shorten or stretch it.
  const current = state.notification;
  const notificationKey = current ? `${current.notification.id}:${state.shownGeneration}` : null;
  const notificationPaused = paused || view.kind !== "notification";
  useCountdown(
    current?.phase === "showing" ? notificationKey : null,
    current ? notificationDwellMs(current.notification, state.notificationQueue.length) : 0,
    notificationPaused,
    actions.endNotificationDwell
  );
  // The grace after the last toast: it stays on screen, a new arrival takes over, else it ends.
  useCountdown(current?.phase === "lingering" ? `grace:${notificationKey}` : null, NOTIFICATION_GRACE_MS, notificationPaused, actions.endNotificationGrace);
  useCountdown(
    state.ringer ? `${state.ringer.key}:${state.ringer.toggles}` : null,
    state.ringer && state.ringer.toggles > 0 ? RINGER_AFTER_TOGGLE_MS : RINGER_MS,
    paused || view.kind !== "ringer",
    actions.dismissRinger
  );

  // Whatever waited behind a hidden window (a fullscreen app) may have gone stale.
  useEffect(() => {
    if (!suppressed) dispatch({ type: "TICK", at: Date.now() });
  }, [suppressed]);

  // After a long hidden stretch (resume from sleep) a waiting notification may have gone stale.
  useEffect(() => {
    const onVisible = () => {
      if (!document.hidden) dispatch({ type: "TICK", at: Date.now() });
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, []);

  return { state, view, ...actions };
}
