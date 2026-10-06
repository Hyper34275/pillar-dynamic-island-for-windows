import { createContext, useContext, useSyncExternalStore } from "react";
import { getCalendarService, type CalendarService } from "../lib/calendar/service";
import type { CalendarSnapshot } from "../lib/calendar/types";

/** Lets tests (and, later, previews) run the UI against a fake service. */
export const CalendarServiceContext = createContext<CalendarService | null>(null);

export function useCalendarService(): CalendarService {
  return useContext(CalendarServiceContext) ?? getCalendarService();
}

/** Calendar data for the UI: the merged snapshot of every provider (docs/ENTERPRISE_DESIGN.md section 1). */
export function useCalendar(): CalendarSnapshot {
  const service = useCalendarService();
  return useSyncExternalStore(service.subscribe, service.getSnapshot);
}
