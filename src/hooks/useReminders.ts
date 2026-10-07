import { useCallback, useEffect, useRef, useState } from "react";
import { createReminderEngine, type ReminderEngine } from "../lib/reminders/engine";
import { createIpcReminderStore } from "../lib/reminders/store";
import type { ReminderAlert, ReminderStore } from "../lib/reminders/types";
import { useCalendarEvents } from "./useCalendar";
import { useSettings } from "./useSettings";

/** "Remind me in 5 min". */
export const SNOOZE_MS = 5 * 60_000;

export interface UseRemindersResult {
  /** Show `alert` again in 5 minutes (with the minutes then left), unless the meeting has started by then. */
  snooze: (alert: ReminderAlert) => void;
}

/**
 * Meeting reminders for the whole app: mount it exactly once (PillShell does). The engine
 * follows the calendar snapshot and the reminder settings; `onAlert` is called once per
 * reminder, ever, and again for each snooze. Snoozes live in memory only. `store` is
 * injectable for tests.
 */
export function useReminders(onAlert: (alert: ReminderAlert) => void, store?: ReminderStore): UseRemindersResult {
  const events = useCalendarEvents();
  const { settings, loaded } = useSettings();
  const [engine, setEngine] = useState<ReminderEngine | null>(null);
  const onAlertRef = useRef(onAlert);
  onAlertRef.current = onAlert;

  useEffect(() => {
    const created = createReminderEngine({
      now: () => Date.now(),
      store: store ?? createIpcReminderStore(),
      onFire: (alert) => onAlertRef.current(alert),
    });
    setEngine(created);
    return () => {
      created.dispose();
      setEngine(null);
    };
  }, [store]);

  const { meetingReminderEnabled, reminderMinutes, sharedCalendarReminders } = settings;
  // Not before the user's own settings are in: a reminder they turned off (or set to 15 minutes)
  // must not fire once with the defaults first.
  useEffect(() => {
    if (loaded) {
      engine?.update(events, {
        enabled: meetingReminderEnabled,
        offsetsMinutes: [reminderMinutes],
        sources: sharedCalendarReminders ? { kind: "all" } : { kind: "own" },
      });
    }
  }, [engine, events, loaded, meetingReminderEnabled, reminderMinutes, sharedCalendarReminders]);

  const snoozes = useRef(new Set<ReturnType<typeof setTimeout>>());
  useEffect(() => {
    const pending = snoozes.current;
    return () => {
      pending.forEach(clearTimeout);
      pending.clear();
    };
  }, []);

  const snooze = useCallback((alert: ReminderAlert) => {
    const handle = setTimeout(() => {
      snoozes.current.delete(handle);
      const minutesLeft = Math.round((Date.parse(alert.startUtc) - Date.now()) / 60_000);
      if (minutesLeft < 1) return;
      onAlertRef.current({ ...alert, key: `${alert.key}|snooze-${Date.now()}`, minutesRemaining: minutesLeft });
    }, SNOOZE_MS);
    snoozes.current.add(handle);
  }, []);

  return { snooze };
}
