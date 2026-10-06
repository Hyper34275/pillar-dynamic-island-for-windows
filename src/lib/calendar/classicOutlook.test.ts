import { describe, expect, it, vi } from "vitest";
import { createClassicOutlookProvider, type CalendarBackend } from "./classicOutlook";
import { createCalendarService } from "./service";
import { WAITING_SNAPSHOT, type CalendarSnapshot } from "./types";

const wire = (status: string, extra: Record<string, unknown> = {}) => ({
  status,
  errorCode: null,
  lastSyncUnixMs: 1,
  cachedCount: 0,
  nextRetryUnixMs: null,
  events: [],
  ...extra,
});

const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};

function fakeBackend(initial: unknown = wire("connected")) {
  let push: (raw: unknown) => void = () => {};
  const state = { unlistened: false, refreshes: 0, refreshResult: true as boolean | Error, getResult: initial as unknown | Error };
  const backend: CalendarBackend = {
    async getSnapshot() {
      if (state.getResult instanceof Error) throw state.getResult;
      return state.getResult;
    },
    async refresh() {
      state.refreshes++;
      if (state.refreshResult instanceof Error) throw state.refreshResult;
      return state.refreshResult;
    },
    onSnapshot(handler) {
      push = handler;
      return () => {
        state.unlistened = true;
      };
    },
  };
  return { backend, state, push: (raw: unknown) => push(raw) };
}

describe("ClassicOutlookCalendarProvider", () => {
  it("starts as waiting and loads the initial snapshot", async () => {
    const { backend } = fakeBackend(wire("connecting"));
    const provider = createClassicOutlookProvider({ backend });
    expect(provider.getSnapshot()).toBe(WAITING_SNAPSHOT);
    await flush();
    expect(provider.getSnapshot().status).toBe("connecting");
  });

  it("applies pushed snapshots and notifies subscribers only on change", async () => {
    const { backend, push } = fakeBackend(wire("waiting"));
    const provider = createClassicOutlookProvider({ backend });
    await flush();
    const seen: CalendarSnapshot[] = [];
    provider.subscribe((s) => seen.push(s));

    push(wire("connected"));
    push(wire("connected")); // identical: no notification
    push(wire("failed", { errorCode: "OUTLOOK-108" }));
    expect(seen.map((s) => s.status)).toEqual(["connected", "failed"]);
    expect(provider.getSnapshot().errorCode).toBe("OUTLOOK-108");
  });

  it("keeps the events array identity while only the sync time moves", async () => {
    const meeting = { id: "m1", startUtc: "2026-10-06T10:00:00Z", endUtc: "2026-10-06T10:30:00Z", subject: "Review" };
    const { backend, push } = fakeBackend(wire("connected", { events: [meeting] }));
    const provider = createClassicOutlookProvider({ backend });
    await flush();
    const first = provider.getSnapshot();
    const seen: CalendarSnapshot[] = [];
    provider.subscribe((s) => seen.push(s));

    push(wire("connected", { events: [{ ...meeting }], lastSyncUnixMs: 60_001 })); // next minute's sync, same meeting
    expect(seen).toHaveLength(1);
    expect(seen[0].lastSyncUnixMs).toBe(60_001);
    expect(seen[0].events).toBe(first.events);

    push(wire("connected", { events: [{ ...meeting, subject: "Moved" }], lastSyncUnixMs: 120_002 }));
    expect(seen).toHaveLength(2);
    expect(seen[1].events).not.toBe(first.events);
    expect(seen[1].events[0].subject).toBe("Moved");
  });

  it("lets a push that arrives before the initial read win over it", async () => {
    const { backend, push } = fakeBackend(wire("waiting"));
    const provider = createClassicOutlookProvider({ backend });
    push(wire("connected"));
    await flush();
    expect(provider.getSnapshot().status).toBe("connected");
  });

  it("survives bad payloads without losing the last good snapshot", async () => {
    const { backend, push } = fakeBackend(wire("connected"));
    const provider = createClassicOutlookProvider({ backend });
    await flush();
    for (const bad of [null, undefined, "boom", 42, []]) push(bad);
    expect(provider.getSnapshot().status).toBe("connected");
    push({ status: "levitating" });
    expect(provider.getSnapshot()).toMatchObject({ status: "failed", errorCode: "OUTLOOK-108" });
  });

  it("normalises events: sorted, broken ones dropped", async () => {
    const e = (id: string, start: string) => ({ id, startUtc: start, endUtc: "2026-10-06T23:00:00.000Z" });
    const { backend } = fakeBackend(wire("connected", { events: [e("b", "2026-10-06T12:00:00.000Z"), { id: "x" }, e("a", "2026-10-06T10:00:00.000Z")] }));
    const provider = createClassicOutlookProvider({ backend });
    await flush();
    expect(provider.getSnapshot().events.map((ev) => ev.id)).toEqual(["a", "b"]);
  });

  it("keeps working when the initial read fails or the backend is unavailable (null)", async () => {
    const failing = fakeBackend();
    failing.state.getResult = new Error("nope");
    const a = createClassicOutlookProvider({ backend: failing.backend });
    await flush();
    expect(a.getSnapshot()).toBe(WAITING_SNAPSHOT);

    const absent = fakeBackend(null);
    const b = createClassicOutlookProvider({ backend: absent.backend });
    await flush();
    expect(b.getSnapshot()).toBe(WAITING_SNAPSHOT);
  });

  it("isolates a throwing subscriber", async () => {
    const { backend, push } = fakeBackend(wire("waiting"));
    const provider = createClassicOutlookProvider({ backend });
    await flush();
    const seen: string[] = [];
    provider.subscribe(() => {
      throw new Error("listener bug");
    });
    provider.subscribe((s) => seen.push(s.status));
    expect(() => push(wire("connected"))).not.toThrow();
    expect(seen).toEqual(["connected"]);
  });

  describe("refresh", () => {
    it("never rejects, whether the backend throws or declines", async () => {
      const { backend, state } = fakeBackend();
      const provider = createClassicOutlookProvider({ backend, minRefreshIntervalMs: 0 });
      state.refreshResult = new Error("COM down");
      await expect(provider.refresh()).resolves.toBeUndefined();
      state.refreshResult = false;
      await expect(provider.refresh()).resolves.toBeUndefined();
      expect(state.refreshes).toBe(2);
    });

    it("shares one in-flight refresh and throttles bursts", async () => {
      let clock = 0;
      const { backend, state } = fakeBackend();
      const provider = createClassicOutlookProvider({ backend, now: () => clock, minRefreshIntervalMs: 5000 });
      await Promise.all([provider.refresh(), provider.refresh(), provider.refresh()]);
      expect(state.refreshes).toBe(1);
      clock = 4000;
      await provider.refresh();
      expect(state.refreshes).toBe(1);
      clock = 5000;
      await provider.refresh();
      expect(state.refreshes).toBe(2);
    });
  });

  it("dispose unsubscribes, drops listeners and ignores later calls", async () => {
    const { backend, state, push } = fakeBackend(wire("waiting"));
    const provider = createClassicOutlookProvider({ backend, minRefreshIntervalMs: 0 });
    await flush();
    const listener = vi.fn();
    provider.subscribe(listener);
    provider.dispose();
    expect(state.unlistened).toBe(true);
    push(wire("connected"));
    await provider.refresh();
    expect(listener).not.toHaveBeenCalled();
    expect(state.refreshes).toBe(0);
  });
});

describe("CalendarService", () => {
  it("exposes the provider's snapshot, notifies on change and refreshes every provider", async () => {
    const { backend, push, state } = fakeBackend(wire("waiting"));
    const provider = createClassicOutlookProvider({ backend, minRefreshIntervalMs: 0 });
    const service = createCalendarService([provider]);
    await flush();
    const listener = vi.fn();
    service.subscribe(listener);
    const before = service.getSnapshot();
    expect(service.getSnapshot()).toBe(before); // stable between changes

    push(wire("connected"));
    expect(listener).toHaveBeenCalledTimes(1);
    expect(service.getSnapshot().status).toBe("connected");

    await service.refresh();
    expect(state.refreshes).toBe(1);
    service.dispose();
    expect(state.unlistened).toBe(true);
  });

  it("merges several providers: events de-duplicated and sorted, best status wins", async () => {
    const e = (id: string, start: string, cal: string) => ({ id, calendarId: cal, startUtc: start, endUtc: "2026-10-06T23:00:00.000Z" });
    const a = fakeBackend(wire("failed", { errorCode: "OUTLOOK-108", events: [e("late", "2026-10-06T12:00:00.000Z", "A")], cachedCount: 1 }));
    const b = fakeBackend(wire("connected", { lastSyncUnixMs: 9, events: [e("early", "2026-10-06T09:00:00.000Z", "B"), e("late", "2026-10-06T12:00:00.000Z", "A")], cachedCount: 2 }));
    const service = createCalendarService([createClassicOutlookProvider({ backend: a.backend }), createClassicOutlookProvider({ backend: b.backend })]);
    await flush();
    const merged = service.getSnapshot();
    expect(merged.status).toBe("connected");
    expect(merged.errorCode).toBeNull();
    expect(merged.events.map((ev) => ev.id)).toEqual(["early", "late"]);
    expect(merged.cachedCount).toBe(3);
    expect(merged.lastSyncUnixMs).toBe(9);
  });
});
