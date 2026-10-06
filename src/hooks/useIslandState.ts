import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { TabId } from "../components/Pill/tabs";
import type { IslandNotification } from "../lib/ipc";
import { initialIslandState, islandReducer, selectView, type IslandState, type IslandView, type Ringer } from "../lib/island/state";
import { ALERT_MS, NOTIFICATION_MS } from "../lib/island/timing";
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
  dismissNotification: () => void;
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
/** An invitation stays longer than a toast: it has buttons to press. */
export const INVITE_TOAST_MS = 9_000;

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
      dismissNotification: () => dispatch({ type: "NOTIFICATION_DONE" }),
      expand: (tab?: TabId) => dispatch({ type: "USER_EXPAND", tab }),
      pin: (tab?: TabId) => dispatch({ type: "PIN", tab }),
      collapse: () => dispatch({ type: "USER_COLLAPSE" }),
      setHovering,
      showRinger: (ringer: Omit<Ringer, "toggles">) => dispatch({ type: "RINGER_SHOW", ringer }),
      toggleRinger: () => dispatch({ type: "RINGER_TOGGLE" }),
      dismissRinger: () => dispatch({ type: "RINGER_DONE" }),
    }),
    []
  );

  useCountdown(state.alert?.key ?? null, ALERT_MS, paused, actions.dismissAlert);
  // Behind a higher state a temporary one keeps its time: it has not been seen yet.
  useCountdown(
    state.notification ? String(state.notification.notification.id) : null,
    state.notification?.notification.invite ? INVITE_TOAST_MS : NOTIFICATION_MS,
    paused || view.kind !== "notification",
    actions.dismissNotification
  );
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
