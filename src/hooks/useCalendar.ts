import { WAITING_SNAPSHOT, type CalendarSnapshot } from "../lib/calendar/types";

/**
 * Calendar data for the UI. Placeholder until the Outlook provider lands: always the
 * 'waiting' snapshot, in exactly the shape of the real one (docs/ENTERPRISE_DESIGN.md §1).
 */
export function useCalendar(): CalendarSnapshot {
  return WAITING_SNAPSHOT;
}
