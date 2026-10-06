import { describe, expect, it } from "vitest";
import { meetingStatus, meetingStatusLabel, SOON_MS } from "./meetingStatus";
import type { CalendarEventDto } from "./types";

const NOW = Date.UTC(2026, 9, 6, 10, 0, 0);
const iso = (min: number) => new Date(NOW + min * 60_000).toISOString();

function event(id: string, startMin: number, endMin: number, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "c",
    subject: `Meeting ${id}`,
    startUtc: iso(startMin),
    endUtc: iso(endMin),
    allDay: false,
    location: null,
    organizer: null,
    isRecurring: false,
    meetingUrl: null,
    busyStatus: "busy",
    responseStatus: "accepted",
    color: null,
    ...extra,
  };
}

describe("meetingStatus", () => {
  it("is nothing without a meeting soon or now", () => {
    expect(meetingStatus([], NOW)).toBeNull();
    expect(meetingStatus([event("later", 6, 30), event("past", -60, -1)], NOW)).toBeNull();
  });

  it("counts down from five minutes before the start, in whole minutes rounded up", () => {
    const status = meetingStatus([event("a", 5, 30)], NOW);
    expect(status).toMatchObject({ kind: "soon", minutes: 5 });
    expect(meetingStatus([event("a", 0.5, 30)], NOW)).toMatchObject({ kind: "soon", minutes: 1 });
    expect(SOON_MS).toBe(5 * 60_000);
  });

  it("shows the meeting in progress with how far it is", () => {
    const status = meetingStatus([event("a", -15, 15)], NOW);
    expect(status).toMatchObject({ kind: "now", progress: 0.5 });
  });

  it("prefers the next meeting about to start over the one running (back to back)", () => {
    expect(meetingStatus([event("running", -30, 3), event("next", 3, 30)], NOW)?.event.id).toBe("next");
  });

  it("of several in progress takes the one that started last", () => {
    expect(meetingStatus([event("long", -120, 120), event("short", -10, 20)], NOW)?.event.id).toBe("short");
  });

  it("ignores what is not a real meeting: all-day, free, declined", () => {
    const skipped = [event("a", -10, 10, { allDay: true }), event("b", -10, 10, { busyStatus: "free" }), event("c", 2, 10, { responseStatus: "declined" })];
    expect(meetingStatus(skipped, NOW)).toBeNull();
  });

  it("says when it starts and what it is, or until when it runs", () => {
    expect(meetingStatusLabel(meetingStatus([event("a", 5, 30, { subject: "Standup" })], NOW)!)).toMatch(/5 min.* · Standup$/);
    expect(meetingStatusLabel(meetingStatus([event("a", 5, 30, { subject: "  " })], NOW)!)).toContain("(No subject)");
    expect(meetingStatusLabel(meetingStatus([event("a", -5, 30)], NOW)!)).toMatch(/^In a meeting until /);
  });
});
