import { useEffect, useRef, useState } from "react";
import { createReminderEngine, type ReminderEngine } from "../lib/reminders/engine";
import { createIpcReminderStore } from "../lib/reminders/store";
import type { ReminderAlert, ReminderStore } from "../lib/reminders/types";
import { useCalendarEvents } from "./useCalendar";
import { useSettings } from "./useSettings";

/**
 * Meeting reminders for the whole app: mount it exactly once (PillShell does). The engine
 * follows the calendar snapshot and the reminder settings; `onAlert` is called once per
 * reminder, ever. `store` is injectable for tests.
 */
export function useReminders(onAlert: (alert: ReminderAlert) => void, store?: ReminderStore): void {
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

  const { meetingReminderEnabled, reminderMinutes } = settings;
  // Not before the user's own settings are in: a reminder they turned off (or set to 15 minutes)
  // must not fire once with the defaults first.
  useEffect(() => {
    if (loaded) engine?.update(events, { enabled: meetingReminderEnabled, offsetsMinutes: [reminderMinutes] });
  }, [engine, events, loaded, meetingReminderEnabled, reminderMinutes]);
}
