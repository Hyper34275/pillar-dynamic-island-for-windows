import { describe, expect, it } from "vitest";
import { selectUpcoming } from "./select";
import type { CalendarEventDto } from "./types";

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
