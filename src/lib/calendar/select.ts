import type { CalendarEventDto } from "./types";

/**
 * Meetings worth showing: timed (not all-day), not free, not declined, not yet over.
 * Same skip rules as the reminder engine so the card and the reminders never disagree.
 * Input is assumed sorted by start; output keeps that order.
 */
export function selectUpcoming(events: readonly CalendarEventDto[], nowMs: number, limit: number): CalendarEventDto[] {
  const out: CalendarEventDto[] = [];
  for (const event of events) {
    if (out.length >= limit) break;
    if (event.allDay || event.busyStatus === "free" || event.responseStatus === "declined") continue;
    if (Date.parse(event.endUtc) <= nowMs) continue;
    out.push(event);
  }
  return out;
}
