import type { ReminderAlert } from "../../lib/reminders/types";
import { measureText } from "../../lib/textMeasure";
import { meetingAlertSize, pillDimensions, type IslandSize } from "./animations";
import { uiFontFamily } from "./useCompactLayout";

export const ALERT_SUBJECT_FONT_SIZE = 17;
const ALERT_SUBJECT_FONT_WEIGHT = 600;
/** Wrapping wastes some of every line, so a subject filling the line is already two lines. */
const WRAP_SLACK = 0.96;

/** One or two lines for the subject (CSS clamps anything longer with an ellipsis). */
export function alertSubjectLines(subject: string): 1 | 2 {
  const a = pillDimensions.alert;
  const available = (a.width - a.paddingX * 2) * WRAP_SLACK;
  const width = measureText(subject, `${ALERT_SUBJECT_FONT_WEIGHT} ${ALERT_SUBJECT_FONT_SIZE}px ${uiFontFamily()}`, ALERT_SUBJECT_FONT_SIZE);
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
