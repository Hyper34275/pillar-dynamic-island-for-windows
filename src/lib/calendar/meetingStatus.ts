// What the collapsed island says about meetings instead of the date: one that starts within a
// few minutes, or the one in progress. Pure, for tests.

import { formatTime, relativeMinutes } from "../dateFormat";
import { t } from "../i18n";
import { minutesUntil } from "../island/countdown";
import { isRealMeeting } from "./select";
import type { CalendarEventDto } from "./types";

/** "In 5 min" shows from this long before the start. */
export const SOON_MS = 5 * 60_000;

export type MeetingStatus =
  | { kind: "soon"; event: CalendarEventDto; minutes: number }
  | { kind: "now"; event: CalendarEventDto; endMs: number; progress: number };

/**
 * A meeting about to start wins over the one in progress (back to back, the next one is what
 * matters). Of several in progress, the one that started last.
 */
export function meetingStatus(events: readonly CalendarEventDto[], nowMs: number): MeetingStatus | null {
  let soon: CalendarEventDto | null = null;
  let current: CalendarEventDto | null = null;
  for (const event of events) {
    if (!isRealMeeting(event)) continue;
    const start = Date.parse(event.startUtc);
    const end = Date.parse(event.endUtc);
    if (start > nowMs && start - nowMs <= SOON_MS) {
      if (!soon || start < Date.parse(soon.startUtc)) soon = event;
    } else if (start <= nowMs && end > nowMs) {
      if (!current || start > Date.parse(current.startUtc)) current = event;
    }
  }
  if (soon) return { kind: "soon", event: soon, minutes: Math.max(1, minutesUntil(Date.parse(soon.startUtc), nowMs)) };
  if (current) {
    const start = Date.parse(current.startUtc);
    const end = Date.parse(current.endUtc);
    return { kind: "now", event: current, endMs: end, progress: Math.min(1, Math.max(0, (nowMs - start) / (end - start))) };
  }
  return null;
}

/** "in 5 min · Standup" / "In a meeting until 22:00". */
export function meetingStatusLabel(status: MeetingStatus): string {
  if (status.kind === "now") return t("status.inMeetingUntil", { time: formatTime(new Date(status.endMs)) });
  const subject = status.event.subject.trim() || t("calendar.noSubject");
  return `${relativeMinutes(status.minutes)} · ${subject}`;
}
