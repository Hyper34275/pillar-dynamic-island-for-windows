import { createContext, useCallback, useContext, useSyncExternalStore } from "react";
import { getCalendarService, type CalendarService } from "../lib/calendar/service";
import type { CalendarEventDto, CalendarSnapshot } from "../lib/calendar/types";

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

/** Only the events: re-renders when the list changes, not on every sync-time or status update. */
export function useCalendarEvents(): readonly CalendarEventDto[] {
  const service = useCalendarService();
  const getEvents = useCallback(() => service.getSnapshot().events, [service]);
  return useSyncExternalStore(service.subscribe, getEvents);
}
