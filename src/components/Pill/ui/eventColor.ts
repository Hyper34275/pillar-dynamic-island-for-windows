import type { CalendarEventDto } from "../../../lib/calendar/types";
import { SYSTEM_COLORS } from "./primitives";

/** Events without a colored Outlook category get the default calendar color. */
export const DEFAULT_EVENT_COLOR = SYSTEM_COLORS.blue;

export function colorOf(event: CalendarEventDto): string {
  return event.color ?? DEFAULT_EVENT_COLOR;
}
