import { describe, expect, it } from "vitest";
import { isRealMeeting, mergeSnapshots, selectAllDay, selectEvents, selectUpcoming } from "./select";
import { WAITING_SNAPSHOT, type CalendarEventDto, type CalendarSnapshot } from "./types";

const NOW = Date.UTC(2026, 9, 6, 10, 0, 0);
const iso = (offsetMin: number) => new Date(NOW + offsetMin * 60_000).toISOString();

function event(id: string, startMin: number, endMin: number, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "cal",
    subject: `Subject ${id}`,
    startUtc: iso(startMin),
    endUtc: iso(endMin),
    allDay: false,
    location: null,
    organizer: null,
    isRecurring: false,
    meetingUrl: null,
    busyStatus: "busy",
    responseStatus: "accepted",
    ...extra,
  };
}

describe("selectUpcoming", () => {
  it("keeps upcoming and in-progress meetings in order, up to the limit", () => {
    const events = [event("a", -10, 20), event("b", 30, 60), event("c", 90, 120), event("d", 150, 180)];
    expect(selectUpcoming(events, NOW, 3).map((e) => e.id)).toEqual(["a", "b", "c"]);
  });

  it("drops meetings that already ended", () => {
    const events = [event("done", -60, -30), event("ends-now", -30, 0), event("next", 15, 45)];
    expect(selectUpcoming(events, NOW, 3).map((e) => e.id)).toEqual(["next"]);
  });

  it("skips all-day, free and declined events like the reminder engine does", () => {
    const events = [
      event("allday", -600, 600, { allDay: true }),
      event("free", 10, 40, { busyStatus: "free" }),
      event("declined", 20, 50, { responseStatus: "declined" }),
      event("tentative", 30, 60, { busyStatus: "tentative", responseStatus: "tentative" }),
      event("oof", 40, 70, { busyStatus: "oof" }),
    ];
    expect(selectUpcoming(events, NOW, 5).map((e) => e.id)).toEqual(["tentative", "oof"]);
  });

  it("returns an empty list for no events", () => {
    expect(selectUpcoming([], NOW, 3)).toEqual([]);
  });
});

describe("isRealMeeting", () => {
  it("is false for all-day, free and declined events", () => {
    expect(isRealMeeting(event("a", 0, 30))).toBe(true);
    expect(isRealMeeting(event("a", 0, 30, { allDay: true }))).toBe(false);
    expect(isRealMeeting(event("a", 0, 30, { busyStatus: "free" }))).toBe(false);
    expect(isRealMeeting(event("a", 0, 30, { responseStatus: "declined" }))).toBe(false);
  });
});

describe("selectAllDay", () => {
  const localMidnight = (dayOffset: number) => new Date(2026, 9, 6 + dayOffset).getTime();
  const allDay = (id: string, from: number, to: number, extra: Partial<CalendarEventDto> = {}) =>
    event(id, 0, 0, { allDay: true, startUtc: new Date(from).toISOString(), endUtc: new Date(to).toISOString(), ...extra });
  const now = new Date(2026, 9, 6, 15, 30).getTime();

  it("keeps all-day events that cover today, including free ones", () => {
    const events = [
      allDay("holiday", localMidnight(0), localMidnight(1), { busyStatus: "free" }),
      allDay("multi", localMidnight(-1), localMidnight(2)),
    ];
    expect(selectAllDay(events, now, 5).map((e) => e.id)).toEqual(["holiday", "multi"]);
  });

  it("drops tomorrow's, yesterday's, declined and timed events, and honours the limit", () => {
    const events = [
      allDay("yesterday", localMidnight(-1), localMidnight(0)),
      allDay("tomorrow", localMidnight(1), localMidnight(2)),
      allDay("declined", localMidnight(0), localMidnight(1), { responseStatus: "declined" }),
      event("timed", 10, 40),
      allDay("a", localMidnight(0), localMidnight(1)),
      allDay("b", localMidnight(0), localMidnight(1)),
      allDay("c", localMidnight(0), localMidnight(1)),
    ];
    expect(selectAllDay(events, now, 2).map((e) => e.id)).toEqual(["a", "b"]);
  });
});

describe("selectEvents / mergeSnapshots", () => {
  const snap = (events: CalendarEventDto[], extra: Partial<CalendarSnapshot> = {}): CalendarSnapshot => ({
    ...WAITING_SNAPSHOT,
    status: "connected",
    cachedCount: events.length,
    events,
    ...extra,
  });

  it("merges calendars into one list sorted by start, de-duplicating the same event", () => {
    const a = snap([event("x", 60, 90, { calendarId: "A" }), event("y", 10, 20, { calendarId: "A" })]);
    const b = snap([event("z", 30, 40, { calendarId: "B" }), event("x", 60, 90, { calendarId: "A" })]);
    expect(selectEvents([a, b]).map((e) => e.id)).toEqual(["y", "z", "x"]);
  });

  it("keeps events of different calendars even with the same id", () => {
    const merged = selectEvents([snap([event("same", 10, 20, { calendarId: "A" })]), snap([event("same", 10, 20, { calendarId: "B" })])]);
    expect(merged).toHaveLength(2);
  });

  it("passes a single snapshot through untouched and an empty set to waiting", () => {
    const only = snap([event("a", 10, 20)]);
    expect(mergeSnapshots([only])).toBe(only);
    expect(mergeSnapshots([])).toBe(WAITING_SNAPSHOT);
  });

  it("prefers a connected provider's status and combines counts and times", () => {
    const failed = snap([], { status: "failed", errorCode: "OUTLOOK-108", lastSyncUnixMs: 5, nextRetryUnixMs: 100, cachedCount: 1 });
    const ok = snap([event("a", 10, 20)], { lastSyncUnixMs: 9, nextRetryUnixMs: 50 });
    expect(mergeSnapshots([failed, ok])).toMatchObject({ status: "connected", errorCode: null, lastSyncUnixMs: 9, nextRetryUnixMs: 50, cachedCount: 2 });
    expect(mergeSnapshots([failed, snap([], { status: "waiting" })])).toMatchObject({ status: "failed", errorCode: "OUTLOOK-108" });
  });
});
