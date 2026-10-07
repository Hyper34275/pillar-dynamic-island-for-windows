import { afterEach, describe, expect, it } from "vitest";
import { meetingStatus } from "../../lib/calendar/meetingStatus";
import type { CalendarEventDto } from "../../lib/calendar/types";
import { setFixedLocale } from "../../lib/i18n";
import type { ReminderAlert } from "../../lib/reminders/types";
import { meetingAlertLabel } from "./MeetingAlert";

afterEach(() => setFixedLocale(null));

const START = Date.UTC(2026, 9, 7, 11, 0);
const END = START + 30 * 60_000;
const event = { id: "m", subject: "Standup", startUtc: new Date(START).toISOString(), endUtc: new Date(END).toISOString(), isAllDay: false, responseStatus: "accepted", busyStatus: "busy", isOnlineMeeting: true } as unknown as CalendarEventDto;
// The engine fired it with a rounded value; the label must not trust that once a clock is given.
const alert = { key: "k", eventId: "m", subject: "Standup", startUtc: event.startUtc, endUtc: event.endUtc, minutesRemaining: 3, location: null, meetingUrl: null, reminderType: { kind: "beforeStart", minutes: 5 } } as unknown as ReminderAlert;

describe("the reminder and the closed island count down the same minutes", () => {
  it.each([
    [4, 1],
    [4, 0],
    [3, 59],
    [3, 1],
    [3, 0],
    [2, 59],
    [1, 1],
    [1, 0],
    [0, 59],
    [0, 1],
  ])("%i:%s before the start", (m, s) => {
    setFixedLocale("en");
    const now = START - (m * 60 + s) * 1000;
    const status = meetingStatus([event], now);
    expect(status?.kind).toBe("soon");
    const minutes = status?.kind === "soon" ? status.minutes : -1;
    expect(meetingAlertLabel(alert, now)).toMatch(new RegExp(`\\b${minutes}\\b`));
  });

  it("says starting now from the start on", () => {
    setFixedLocale("en");
    expect(meetingAlertLabel(alert, START)).toBe(meetingAlertLabel({ ...alert, minutesRemaining: 0 }));
  });
});
