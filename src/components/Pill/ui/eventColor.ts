import type { CalendarEventDto } from "../../../lib/calendar/types";
import { palette } from "../../../design/tokens";

/** Events without a colored Outlook category get the default calendar color. */
export const DEFAULT_EVENT_COLOR = palette.accent;

export function colorOf(event: CalendarEventDto): string {
  return event.color ?? DEFAULT_EVENT_COLOR;
}
