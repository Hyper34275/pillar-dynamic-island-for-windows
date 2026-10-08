import type { CalendarEventDto } from "../../../lib/calendar/types";
import { palette } from "../../../design/tokens";

/** Events without a colored Outlook category get the default calendar color. */
export const DEFAULT_EVENT_COLOR = palette.accent;

/** One color for the event where there is room for one: its category's, else its calendar's, else the default. */
export function colorOf(event: CalendarEventDto): string {
  return event.color ?? event.calendarColor ?? DEFAULT_EVENT_COLOR;
}

/**
 * The event's color stripes, leading edge first. With several calendars shown (the event then
 * carries its calendar's color) the calendar's color comes first, so mine and shared tell apart at
 * a glance, then the category's when it has one; otherwise just the one color of {@link colorOf}.
 */
export function stripesOf(event: CalendarEventDto): string[] {
  if (!event.calendarColor) return [colorOf(event)];
  return event.color ? [event.calendarColor, event.color] : [event.calendarColor];
}
