import { describe, expect, it } from "vitest";
import { normalizeEvent, normalizeInvite, normalizeSnapshot, normalizeSources } from "./normalize";

const good = {
  id: "abc123",
  calendarId: "cal1",
  calendarName: "צוות תמיכה",
  sourceKind: "shared",
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
  color: null,
};

describe("normalizeEvent", () => {
  it("keeps a complete event as it is", () => {
    expect(normalizeEvent(good)).toEqual(good);
  });

  it("fills absent fields with safe defaults", () => {
    expect(normalizeEvent({ id: "x", startUtc: good.startUtc, endUtc: good.endUtc })).toEqual({
      id: "x",
      calendarId: "default",
      calendarName: "",
      sourceKind: "primary",
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
      color: null,
    });
  });

  it("keeps a #RRGGBB category color and drops anything else", () => {
    expect(normalizeEvent({ ...good, color: "#3267B8" })?.color).toBe("#3267B8");
    for (const color of ["red", "#fff", "#12345G", "url(x)", "#123456;background:red", 5]) {
      expect(normalizeEvent({ ...good, color })?.color, String(color)).toBeNull();
    }
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
      invites: [],
      events: [],
      sources: null,
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
      invites: [],
    });
  });

  it("treats a non-array events field as empty", () => {
    expect(normalizeSnapshot({ status: "connected", events: "many" })?.events).toEqual([]);
  });
});

describe("normalizeInvite", () => {
  const invite = {
    id: "inv1",
    subject: "Design review",
    organizer: "Dana",
    startUtc: "2026-10-07T09:00:00Z",
    endUtc: "2026-10-07T10:00:00Z",
    location: "Room 2",
    receivedUtc: "2026-10-06T18:30:00Z",
  };

  it("keeps a complete invite as it is", () => {
    expect(normalizeInvite(invite)).toEqual(invite);
  });

  it("needs an id and a receive time", () => {
    for (const raw of [null, [], { ...invite, id: "" }, { ...invite, receivedUtc: "soon" }, { ...invite, receivedUtc: undefined }]) {
      expect(normalizeInvite(raw)).toBeNull();
    }
  });

  it("drops only the meeting time when it is missing or backwards", () => {
    expect(normalizeInvite({ ...invite, startUtc: null })).toMatchObject({ startUtc: null, endUtc: null, subject: "Design review" });
    expect(normalizeInvite({ ...invite, startUtc: invite.endUtc, endUtc: invite.startUtc })).toMatchObject({ startUtc: null, endUtc: null });
  });

  it("is read from the snapshot, broken entries dropped, at most ten", () => {
    const many = Array.from({ length: 14 }, (_, i) => ({ ...invite, id: `inv${i}` }));
    const snapshot = normalizeSnapshot({ status: "connected", invites: [null, { id: "x" }, ...many] });
    expect(snapshot?.invites.map((i) => i.id)).toEqual(many.slice(0, 10).map((i) => i.id));
    expect(normalizeSnapshot({ status: "connected", invites: "lots" })?.invites).toEqual([]);
  });
});

describe("normalizeSources", () => {
  const source = {
    id: "0123456789abcdef",
    name: "  לוח משמרות — Support ",
    group: "shared",
    kind: "shared",
    selected: true,
    active: true,
    pendingInOutlook: false,
    state: "ok",
    errorCode: null,
    eventCount: 4,
    lastReadUnixMs: 5,
  };

  it("keeps a complete report and trims names without touching their text", () => {
    const report = normalizeSources({ sources: [source], selection: "remembered", groups: 3, listener: true, discoveredUnixMs: 9 });
    expect(report).toEqual({
      sources: [{ ...source, name: "לוח משמרות — Support" }],
      selection: "remembered",
      groups: 3,
      listener: true,
      discoveredUnixMs: 9,
    });
  });

  it("drops sources without an id and reads unknown values the safe way", () => {
    const report = normalizeSources({ sources: [null, { name: "no id" }, { id: "x", kind: "boss", state: "??", active: "yes" }], selection: "x" });
    expect(report?.sources).toEqual([
      { id: "x", name: "", group: "unknown", kind: "shared", selected: false, active: false, pendingInOutlook: false, state: "unavailable", errorCode: null, eventCount: 0, lastReadUnixMs: null },
    ]);
    expect(report?.selection).toBe("primaryOnly");
    expect(report?.listener).toBe(false);
  });

  it("reaches the snapshot, and is null when the backend has none yet", () => {
    expect(normalizeSnapshot({ status: "connected", sources: { sources: [source], selection: "outlook" } })?.sources?.sources).toHaveLength(1);
    expect(normalizeSnapshot({ status: "waiting" })?.sources).toBeNull();
  });
});
