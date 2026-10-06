import { describe, expect, it, vi } from "vitest";
import { createDayCache, DAY_FRESH_MS, eventsOfDay, nextDayStart } from "./dayRange";
import type { CalendarEventDto } from "./types";

const DAY = new Date(2026, 9, 12).getTime(); // local midnight, Monday 12 Oct 2026
const at = (hour: number, day = 0) => new Date(2026, 9, 12 + day, hour).toISOString();

function event(id: string, start: string, end: string, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "c",
    subject: id,
    startUtc: start,
    endUtc: end,
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

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("eventsOfDay / nextDayStart", () => {
  it("keeps what overlaps the local day, including what spans midnight", () => {
    expect(nextDayStart(DAY)).toBe(new Date(2026, 9, 13).getTime());
    const events = [
      event("yesterday", at(9, -1), at(10, -1)),
      event("overnight", at(23, -1), at(1)),
      event("morning", at(9), at(10)),
      event("tomorrow", at(0, 1), at(1, 1)),
    ];
    expect(eventsOfDay(events, DAY).map((e) => e.id)).toEqual(["overnight", "morning"]);
  });
});

describe("day cache", () => {
  it("reads a day once, normalises and keeps only that day's events, sorted", async () => {
    const fetch = vi.fn(async () => [event("b", at(14), at(15)), null, event("a", at(9), at(10)), event("next", at(9, 1), at(10, 1))]);
    const cache = createDayCache(fetch);
    cache.load(DAY, 0);
    expect(cache.get(DAY)).toEqual({ state: "loading", events: null });
    cache.load(DAY, 0); // already loading
    await flush();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledWith(new Date(DAY).toISOString(), new Date(nextDayStart(DAY)).toISOString());
    const day = cache.get(DAY);
    expect(day?.state).toBe("ready");
    expect(day?.events?.map((e) => e.id)).toEqual(["a", "b"]);
  });

  it("does not read a fresh day again, and re-reads a stale one while still showing it", async () => {
    const fetch = vi.fn(async () => [event("a", at(9), at(10))]);
    const cache = createDayCache(fetch);
    cache.load(DAY);
    await flush();
    const readAt = (cache.get(DAY) as { fetchedAt: number }).fetchedAt;
    cache.load(DAY, readAt + DAY_FRESH_MS - 1);
    expect(fetch).toHaveBeenCalledTimes(1);
    cache.load(DAY, readAt + DAY_FRESH_MS);
    expect(cache.get(DAY)?.state).toBe("loading");
    expect(cache.get(DAY)?.events?.map((e) => e.id)).toEqual(["a"]);
    await flush();
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("marks a failed read as an error and keeps what it had", async () => {
    const fetch = vi.fn().mockResolvedValueOnce([event("a", at(9), at(10))]).mockResolvedValueOnce(null);
    const cache = createDayCache(fetch);
    cache.load(DAY, 0);
    await flush();
    cache.load(DAY, Number.MAX_SAFE_INTEGER);
    await flush();
    expect(cache.get(DAY)).toMatchObject({ state: "error" });
    expect(cache.get(DAY)?.events?.map((e) => e.id)).toEqual(["a"]);
  });
});
