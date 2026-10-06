import { describe, expect, it } from "vitest";
import { normalizeEvent, normalizeSnapshot } from "./normalize";

const good = {
  id: "abc123",
  calendarId: "cal1",
  subject: "Planning",
  startUtc: "2026-10-06T10:00:00.000Z",
  endUtc: "2026-10-06T11:00:00.000Z",
  allDay: false,
  location: "Room 4",
  organizer: "Someone",
  isRecurring: true,
  meetingUrl: null,
  busyStatus: "tentative",
  responseStatus: "accepted",
};

describe("normalizeEvent", () => {
  it("keeps a complete event as it is", () => {
    expect(normalizeEvent(good)).toEqual(good);
  });

  it("fills absent fields with safe defaults", () => {
    expect(normalizeEvent({ id: "x", startUtc: good.startUtc, endUtc: good.endUtc })).toEqual({
      id: "x",
      calendarId: "default",
      subject: "",
      startUtc: good.startUtc,
      endUtc: good.endUtc,
      allDay: false,
      location: null,
      organizer: null,
      isRecurring: false,
      meetingUrl: null,
      busyStatus: "busy",
      responseStatus: "none",
    });
  });

  it("treats blank location, organizer and url as absent and unknown enums as the safe default", () => {
    const event = normalizeEvent({ ...good, location: "  ", organizer: "", meetingUrl: 5, busyStatus: "meh", responseStatus: 9 });
    expect(event).toMatchObject({ location: null, organizer: null, meetingUrl: null, busyStatus: "busy", responseStatus: "none" });
  });

  it("rejects what can be neither scheduled nor shown", () => {
    for (const raw of [
      null,
      "x",
      [],
      { ...good, id: "" },
      { ...good, id: 5 },
      { ...good, startUtc: "yesterday" },
      { ...good, endUtc: undefined },
      { ...good, startUtc: good.endUtc, endUtc: good.startUtc },
    ]) {
      expect(normalizeEvent(raw)).toBeNull();
    }
  });
});

describe("normalizeSnapshot", () => {
  it("rejects payloads that are not objects", () => {
    for (const raw of [null, undefined, 3, "connected", []]) expect(normalizeSnapshot(raw)).toBeNull();
  });

  it("maps an unknown status to failed with a code", () => {
    expect(normalizeSnapshot({ status: "levitating" })).toMatchObject({ status: "failed", errorCode: "OUTLOOK-108" });
    expect(normalizeSnapshot({ status: 7, errorCode: "OUTLOOK-110" })).toMatchObject({ status: "failed", errorCode: "OUTLOOK-110" });
  });

  it("keeps known statuses and their code", () => {
    expect(normalizeSnapshot({ status: "elevationMismatch", errorCode: "OUTLOOK-103" })).toMatchObject({
      status: "elevationMismatch",
      errorCode: "OUTLOOK-103",
    });
    expect(normalizeSnapshot({ status: "waiting" })).toMatchObject({ status: "waiting", errorCode: null });
  });

  it("copes with every field missing", () => {
    expect(normalizeSnapshot({})).toEqual({
      status: "failed",
      errorCode: "OUTLOOK-108",
      lastSyncUnixMs: null,
      cachedCount: 0,
      nextRetryUnixMs: null,
      events: [],
    });
  });

  it("sorts events by start, drops the broken ones and defaults the cached count", () => {
    const later = { ...good, id: "later", startUtc: "2026-10-06T12:00:00.000Z", endUtc: "2026-10-06T13:00:00.000Z" };
    const snapshot = normalizeSnapshot({ status: "connected", events: [later, null, { id: "broken" }, good] });
    expect(snapshot?.events.map((e) => e.id)).toEqual(["abc123", "later"]);
    expect(snapshot?.cachedCount).toBe(2);
  });

  it("ignores non-numeric timestamps and counts", () => {
    expect(normalizeSnapshot({ status: "connected", lastSyncUnixMs: "now", cachedCount: -4, nextRetryUnixMs: Number.NaN })).toMatchObject({
      lastSyncUnixMs: null,
      cachedCount: 0,
      nextRetryUnixMs: null,
    });
  });

  it("treats a non-array events field as empty", () => {
    expect(normalizeSnapshot({ status: "connected", events: "many" })?.events).toEqual([]);
  });
});
