import { useSyncExternalStore } from "react";
import { msUntilNextDay, msUntilNextMinute, startOfDay } from "../lib/dateFormat";

// One shared clock for the whole island. It never ticks per second: while anything
// subscribes to the minute it sleeps in a single minute-aligned timeout, while only the
// day is subscribed (the collapsed island) it sleeps until the next midnight but never
// longer than a minute, and with no subscribers there is no timer at all.
//
// Timeouts are never trusted to be on time. The webview's timers do not count time the PC
// spent asleep, and the non-activating island gets no focus or visibility events on resume,
// so one long sleep could leave yesterday's date up for hours. Capping every sleep at a
// minute bounds that staleness, and every wake-up (timer, visibilitychange, focus)
// recomputes everything from Date.now().

export interface MinuteSnapshot {
  /** Start of the current minute, local time (ms since epoch). */
  minuteStart: number;
}

export interface DaySnapshot {
  /** Start of the current local day (ms since epoch). */
  dayStart: number;
}

type Listener = () => void;

/** Fires slightly after the boundary so Date.now() is safely on the far side of it. */
const BOUNDARY_SLACK_MS = 15;
/** Longest single sleep: bounds how stale the day can be after the PC resumed from sleep. */
const MAX_SLEEP_MS = 60_000;

function minuteStartOf(nowMs: number): number {
  const d = new Date(nowMs);
  return nowMs - (d.getSeconds() * 1000 + d.getMilliseconds());
}

export function createClockStore(now: () => number = () => Date.now()) {
  let minuteSnap: MinuteSnapshot = { minuteStart: minuteStartOf(now()) };
  let daySnap: DaySnapshot = { dayStart: startOfDay(now()) };
  const minuteListeners = new Set<Listener>();
  const dayListeners = new Set<Listener>();
  let timer: ReturnType<typeof setTimeout> | null = null;
  let listening = false;

  /** Recomputes both snapshots; returns which channels changed. */
  function sync(): { minute: boolean; day: boolean } {
    const t = now();
    const minuteStart = minuteStartOf(t);
    const dayStart = startOfDay(t);
    const minute = minuteStart !== minuteSnap.minuteStart;
    const day = dayStart !== daySnap.dayStart;
    if (minute) minuteSnap = { minuteStart };
    if (day) daySnap = { dayStart };
    return { minute, day };
  }

  function clearTimer() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function schedule() {
    clearTimer();
    if (minuteListeners.size === 0 && dayListeners.size === 0) return;
    const t = now();
    const wait = minuteListeners.size > 0 ? msUntilNextMinute(t) : Math.min(msUntilNextDay(t), MAX_SLEEP_MS);
    timer = setTimeout(wake, wait + BOUNDARY_SLACK_MS);
  }

  function wake() {
    // Also entered from subscribe/visibility/focus while a timeout is pending: drop it, schedule() re-arms.
    clearTimer();
    const changed = sync();
    // Copy first: a listener may unsubscribe while we notify.
    if (changed.minute) [...minuteListeners].forEach((l) => l());
    if (changed.day) [...dayListeners].forEach((l) => l());
    schedule();
  }

  function onVisibility() {
    if (typeof document !== "undefined" && document.hidden) return;
    wake();
  }

  function attachWakeEvents() {
    if (listening || typeof document === "undefined") return;
    listening = true;
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("focus", wake);
  }

  function detachWakeEvents() {
    if (!listening) return;
    listening = false;
    document.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("focus", wake);
  }

  function subscribeTo(set: Set<Listener>, listener: Listener): () => void {
    set.add(listener);
    attachWakeEvents();
    // Catch up before the first render after a long unsubscribed gap, then (re)arm.
    wake();
    return () => {
      set.delete(listener);
      if (minuteListeners.size === 0 && dayListeners.size === 0) {
        clearTimer();
        detachWakeEvents();
      } else {
        schedule();
      }
    };
  }

  return {
    subscribeMinute: (listener: Listener) => subscribeTo(minuteListeners, listener),
    subscribeDay: (listener: Listener) => subscribeTo(dayListeners, listener),
    getMinute(): MinuteSnapshot {
      // With nobody subscribed no timer keeps the snapshot fresh, so refresh on read.
      if (timer === null) sync();
      return minuteSnap;
    },
    getDay(): DaySnapshot {
      if (timer === null) sync();
      return daySnap;
    },
  };
}

const clock = createClockStore();

/** Re-renders once per minute. Returns the start of the current minute as a Date. */
export function useMinute(): Date {
  const { minuteStart } = useSyncExternalStore(clock.subscribeMinute, clock.getMinute);
  return new Date(minuteStart);
}

/** Re-renders only when the local date changes (midnight, or after resume). */
export function useToday(): Date {
  const { dayStart } = useSyncExternalStore(clock.subscribeDay, clock.getDay);
  return new Date(dayStart);
}
