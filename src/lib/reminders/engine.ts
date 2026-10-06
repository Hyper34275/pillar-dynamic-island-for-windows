// Reminder engine: decides when a meeting reminder fires. Pure apart from the injected
// clock, store and timer, so it is deterministic under test.
//
// - One timer, aimed at the earliest due instant and never longer than MAX_SLEEP_MS. Webview
//   timers do not count time the PC spent asleep, so every wake-up recomputes from now().
// - Every reminder fires at most once, ever: its key (event id | start | type) goes into the
//   fired set in memory first, then to the store. The persisted set is loaded before anything
//   is scheduled, so a restart never re-alerts. A moved meeting has a new start, hence a new key.
// - All arithmetic is on UTC instants (ISO strings parsed with Date.parse): DST and time-zone
//   changes cannot shift a reminder.

import { isRealMeeting } from "../calendar/select";
import type { CalendarEventDto } from "../calendar/types";
import { dlog } from "../debugLog";
import { describeError } from "../errors";
import { reminderKey, type ReminderAlert, type ReminderSettings, type ReminderStore, type ReminderType } from "./types";

/** Longest single sleep; bounds how late a reminder can be after the PC resumed from sleep. */
export const MAX_SLEEP_MS = 60_000;
/** Up to this late is still "on time" (timer jitter): the reminder fires whatever is left. */
export const ON_TIME_TOLERANCE_MS = 5_000;
/**
 * A reminder that came due earlier (PC asleep, Outlook attached late, app just started) still
 * fires while the meeting has not started and at least this much is left; otherwise it is skipped.
 */
export const MIN_REMAINING_MS = 60_000;
/** Fired-set entries older than this are dropped (events are only read 48 h ahead). */
export const FIRED_RETENTION_MS = 7 * 24 * 3_600_000;

export interface ReminderEngineOptions {
  now: () => number;
  store: ReminderStore;
  onFire: (alert: ReminderAlert) => void;
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

export interface ReminderEngine {
  /** Replaces the schedule. Cheap to call on every calendar or settings change. */
  update(events: readonly CalendarEventDto[], settings: ReminderSettings): void;
  dispose(): void;
}

interface Candidate {
  event: CalendarEventDto;
  type: ReminderType;
  key: string;
  startMs: number;
  dueMs: number;
}

export function createReminderEngine(options: ReminderEngineOptions): ReminderEngine {
  const { now, store, onFire } = options;
  const setTimer = options.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
  const clearTimer = options.clearTimer ?? ((handle) => clearTimeout(handle as ReturnType<typeof setTimeout>));

  let fired: Record<string, number> = {};
  let ready = false;
  let disposed = false;
  let current: { events: readonly CalendarEventDto[]; settings: ReminderSettings } | null = null;
  let timer: unknown = null;
  let saving: Promise<void> = Promise.resolve();

  function persist(): void {
    const cutoff = now() - FIRED_RETENTION_MS;
    fired = Object.fromEntries(Object.entries(fired).filter(([, at]) => at >= cutoff));
    const snapshot = { ...fired };
    saving = saving
      .then(() => store.save(snapshot))
      .catch((error) => dlog("warn", "reminders", `saving the fired set failed: ${describeError(error)}`));
  }

  function candidates(events: readonly CalendarEventDto[], offsets: readonly number[], at: number): Candidate[] {
    const out: Candidate[] = [];
    for (const event of events) {
      if (!isRealMeeting(event)) continue;
      const startMs = Date.parse(event.startUtc);
      if (!(startMs > at) || !(Date.parse(event.endUtc) > at)) continue;
      for (const minutes of offsets) {
        if (!Number.isFinite(minutes) || minutes < 0) continue;
        const type: ReminderType = { kind: "beforeStart", minutes };
        const key = reminderKey(event.id, event.startUtc, type);
        if (key in fired) continue;
        out.push({ event, type, key, startMs, dueMs: startMs - minutes * 60_000 });
      }
    }
    return out.sort((a, b) => a.dueMs - b.dueMs || a.startMs - b.startMs);
  }

  function run(): void {
    if (timer !== null) {
      clearTimer(timer);
      timer = null;
    }
    if (disposed || !ready || !current || !current.settings.enabled) return;

    const at = now();
    const pending = candidates(current.events, current.settings.offsetsMinutes, at);
    const due: Candidate[] = [];
    let nextDueMs = Number.POSITIVE_INFINITY;
    let skipped = 0;
    for (const candidate of pending) {
      if (candidate.dueMs > at) {
        nextDueMs = candidate.dueMs; // sorted: the first future one is the earliest
        break;
      }
      fired[candidate.key] = at;
      const lateMs = at - candidate.dueMs;
      const worthIt = lateMs <= ON_TIME_TOLERANCE_MS || candidate.startMs - at >= MIN_REMAINING_MS;
      if (worthIt) due.push(candidate);
      else skipped++;
    }

    if (due.length + skipped > 0) {
      persist();
      if (skipped > 0) dlog("info", "reminders", `${skipped} reminder(s) skipped: too late`);
    }
    for (const candidate of due) {
      const { event, type, key } = candidate;
      dlog("info", "reminders", `fire ${key.slice(0, 8)} (${type.minutes} min before)`);
      try {
        onFire({
          key,
          eventId: event.id,
          subject: event.subject,
          startUtc: event.startUtc,
          endUtc: event.endUtc,
          location: event.location,
          meetingUrl: event.meetingUrl,
          minutesRemaining: Math.max(0, Math.round((candidate.startMs - at) / 60_000)),
          reminderType: type,
        });
      } catch (error) {
        dlog("error", "reminders", `onFire threw: ${describeError(error)}`);
      }
    }

    if (nextDueMs !== Number.POSITIVE_INFINITY && !disposed) {
      timer = setTimer(run, Math.max(1, Math.min(nextDueMs - at, MAX_SLEEP_MS)));
    }
  }

  // The fired set must be in memory before the first schedule, whether or not it could be read.
  void Promise.resolve()
    .then(() => store.load())
    .then(
      (loaded) => {
        const cutoff = now() - FIRED_RETENTION_MS;
        fired = Object.fromEntries(Object.entries(loaded).filter(([, at]) => at >= cutoff));
      },
      (error) => dlog("warn", "reminders", `loading the fired set failed: ${describeError(error)}`)
    )
    .then(() => {
      ready = true;
      run();
    });

  return {
    update(events, settings) {
      current = { events, settings };
      run();
    },
    dispose() {
      disposed = true;
      if (timer !== null) clearTimer(timer);
      timer = null;
    },
  };
}
