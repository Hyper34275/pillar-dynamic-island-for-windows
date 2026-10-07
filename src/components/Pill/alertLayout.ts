import { smallExpanded, type as typeRoles } from "../../design/tokens";
import type { ReminderAlert } from "../../lib/reminders/types";
import { measureText } from "../../lib/textMeasure";
import { meetingAlertSize, type IslandSize } from "./animations";
import { uiFontFamily } from "./useCompactLayout";

/** The subject is the `title` role. */
const SUBJECT = typeRoles.title;
/** Wrapping wastes some of every line, so a subject filling the line is already two lines. */
const WRAP_SLACK = 0.96;

/** One or two lines for the subject (CSS clamps anything longer with an ellipsis). */
export function alertSubjectLines(subject: string): 1 | 2 {
  const available = (smallExpanded.width - smallExpanded.padding * 2) * WRAP_SLACK;
  const width = measureText(subject, `${SUBJECT.weight} ${SUBJECT.size}px ${uiFontFamily()}`, SUBJECT.size);
  return width > available ? 2 : 1;
}

/** Snoozing for 5 minutes only makes sense while more than that is left before the start. */
export const SNOOZE_MINUTES = 5;

export function canSnooze(alert: ReminderAlert): boolean {
  return alert.minutesRemaining > SNOOZE_MINUTES;
}

/** The alert gets a row of buttons when there is something to press: join and/or snooze. */
export function alertHasActions(alert: ReminderAlert): boolean {
  return !!alert.meetingUrl || canSnooze(alert);
}

export function alertIslandSize(alert: ReminderAlert): IslandSize {
  return meetingAlertSize(alertSubjectLines(alert.subject), !!alert.location, alertHasActions(alert));
}
