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

export function alertIslandSize(alert: ReminderAlert): IslandSize {
  return meetingAlertSize(alertSubjectLines(alert.subject), !!alert.location);
}
