// Days the user browses to in the Calendar tab. The regular sync only covers now .. +48 h (what
// reminders need), so any other day is read from Outlook on demand, one day at a time, and kept
// for a short while. Memory only.

import { useEffect, useSyncExternalStore } from "react";
import { startOfDay } from "../dateFormat";
import { ipc } from "../ipc";
import { compareEvents, normalizeEvent } from "./normalize";
import type { CalendarEventDto } from "./types";

/** A day read this recently is shown as it is; an older one is shown and read again. */
export const DAY_FRESH_MS = 2 * 60_000;
/** Days kept in memory at most (a couple of weeks of browsing). */
const MAX_DAYS = 21;

export type DayState =
  | { state: "loading"; events: readonly CalendarEventDto[] | null }
  | { state: "ready"; events: readonly CalendarEventDto[]; fetchedAt: number }
  | { state: "error"; events: readonly CalendarEventDto[] | null };

/** The local day after `dayStartMs` (DST-safe: +36 h always lands inside the next day). */
export function nextDayStart(dayStartMs: number): number {
  return startOfDay(dayStartMs + 36 * 3_600_000);
}

/** Events of the day starting at `dayStartMs` from a list that may cover other days too. */
export function eventsOfDay(events: readonly CalendarEventDto[], dayStartMs: number): CalendarEventDto[] {
  const end = nextDayStart(dayStartMs);
  return events.filter((e) => Date.parse(e.startUtc) < end && Date.parse(e.endUtc) > dayStartMs);
}

type Fetch = (fromUtc: string, toUtc: string) => Promise<unknown>;

export interface DayCache {
  /** Read the day unless a fresh copy is there (or a read is already running). */
  load(dayStartMs: number, now?: number): void;
  get(dayStartMs: number): DayState | undefined;
  subscribe(listener: () => void): () => void;
  getSnapshot(): ReadonlyMap<number, DayState>;
}

export function createDayCache(fetch: Fetch = ipc.calendarGetRange): DayCache {
  let days: ReadonlyMap<number, DayState> = new Map();
  const listeners = new Set<() => void>();
  const set = (day: number, value: DayState) => {
    const next = new Map(days);
    next.delete(day);
    next.set(day, value);
    // Oldest entries first in insertion order: drop them beyond the limit.
    while (next.size > MAX_DAYS) next.delete(next.keys().next().value as number);
    days = next;
    listeners.forEach((listener) => listener());
  };
  return {
    load(day, now = Date.now()) {
      const current = days.get(day);
      if (current?.state === "loading") return;
      if (current?.state === "ready" && now - current.fetchedAt < DAY_FRESH_MS) return;
      const previous = current?.events ?? null;
      set(day, { state: "loading", events: previous });
      void fetch(new Date(day).toISOString(), new Date(nextDayStart(day)).toISOString())
        .then((raw) => {
          if (!Array.isArray(raw)) {
            set(day, { state: "error", events: previous });
            return;
          }
          const events = raw
            .map(normalizeEvent)
            .filter((e): e is CalendarEventDto => e !== null)
            .sort(compareEvents);
          set(day, { state: "ready", events: eventsOfDay(events, day), fetchedAt: Date.now() });
        })
        .catch(() => set(day, { state: "error", events: previous }));
    },
    get: (day) => days.get(day),
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => days,
  };
}

export const dayCache = createDayCache();

/** The day's state, loading it (or refreshing a stale copy) when `enabled`. */
export function useCalendarDay(dayStartMs: number, enabled: boolean, cache: DayCache = dayCache): DayState | undefined {
  const all = useSyncExternalStore(cache.subscribe, cache.getSnapshot, cache.getSnapshot);
  useEffect(() => {
    if (enabled) cache.load(dayStartMs);
  }, [cache, dayStartMs, enabled]);
  return all.get(dayStartMs);
}
