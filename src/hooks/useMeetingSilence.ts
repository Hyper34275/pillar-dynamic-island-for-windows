import { useEffect, useRef } from "react";
import { isRealMeeting } from "../lib/calendar/select";
import type { CalendarEventDto } from "../lib/calendar/types";
import { silence as defaultSilence, type Silence } from "../lib/island/silence";
import type { Ringer } from "../lib/island/state";
import { useCalendarEvents } from "./useCalendar";

/** A meeting that started at most this long ago still gets its ring / silent pill (late sync, wake-up). */
export const PROMPT_WINDOW_MS = 60_000;
/** While waiting for a start or a silence end, never sleep longer than this (sleep, clock changes). */
const MAX_SLEEP_MS = 10 * 60_000;
const KEYS_MAX = 100;

function meetingKey(event: CalendarEventDto): string {
  return `${event.id}|${event.startUtc}`;
}

/**
 * Brings up the ring / silent pill when a meeting starts (once per meeting), and a short "ring"
 * when a silence the user chose has ended. `enabled` (the setting) is null until settings have
 * loaded; while it is false nothing is offered, but a silence already set still ends normally.
 */
export function useMeetingSilence(enabled: boolean | null, show: (ringer: Omit<Ringer, "toggles">) => void, store: Silence = defaultSilence): void {
  const events = useCalendarEvents();
  const showRef = useRef(show);
  showRef.current = show;
  const prompted = useRef<string[]>([]);
  const silentUntil = useRef<number | null>(store.getSnapshot());

  useEffect(() => {
    if (enabled === null) return;
    let handle: ReturnType<typeof setTimeout> | null = null;

    const check = () => {
      const now = Date.now();
      let promptedNow = false;
      for (const event of events) {
        if (!isRealMeeting(event)) continue;
        const start = Date.parse(event.startUtc);
        const end = Date.parse(event.endUtc);
        if (start > now || now - start > PROMPT_WINDOW_MS || end <= now) continue;
        const key = meetingKey(event);
        if (prompted.current.includes(key)) continue;
        prompted.current = [...prompted.current, key].slice(-KEYS_MAX);
        if (!enabled) continue;
        showRef.current({ key, phase: "start", silent: store.isSilent(now), untilMs: end });
        promptedNow = true;
      }
      // A silence the user chose has run out: say it rings again (unless a new meeting just took the pill).
      const until = silentUntil.current;
      if (until !== null && now >= until && !store.isSilent(now)) {
        silentUntil.current = null;
        if (!promptedNow) showRef.current({ key: `end|${until}`, phase: "end", silent: false, untilMs: until });
      }
      schedule(now);
    };

    // Only while there is something to wait for: an idle island with no meetings ahead runs no
    // timer (a new calendar snapshot re-runs this effect anyway).
    const schedule = (now: number) => {
      let next = Number.POSITIVE_INFINITY;
      for (const event of events) {
        const start = Date.parse(event.startUtc);
        if (start > now && start < next && isRealMeeting(event)) next = start;
      }
      const until = store.getSnapshot();
      if (until !== null && until > now && until < next) next = until;
      if (handle !== null) clearTimeout(handle);
      handle = null;
      if (next === Number.POSITIVE_INFINITY) return;
      handle = setTimeout(check, Math.min(MAX_SLEEP_MS, Math.max(250, next - now + 50)));
    };

    // Follow the silence the user sets or clears (from the pill or the Calendar tab).
    const off = store.subscribe(() => {
      const until = store.getSnapshot();
      if (until !== null) silentUntil.current = until;
      else if (silentUntil.current !== null && Date.now() < silentUntil.current) silentUntil.current = null; // cleared by hand: no "ring" pill
      schedule(Date.now());
    });

    check();
    return () => {
      off();
      if (handle !== null) clearTimeout(handle);
    };
  }, [events, enabled, store]);
}
