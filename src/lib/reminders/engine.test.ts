import { afterEach, describe, expect, it } from "vitest";
import type { CalendarEventDto } from "../calendar/types";
import { createReminderEngine, FIRED_RETENTION_MS, MAX_REMINDERS_PER_BURST, MAX_SLEEP_MS } from "./engine";
import { reminderKey, reminderTypeId, remindsFor, type ReminderAlert, type ReminderSettings, type ReminderStore } from "./types";

// The test tsconfig has no node types; process.env.TZ is how the time zone is switched at run time.
const env = (globalThis as unknown as { process: { env: Record<string, string | undefined> } }).process.env;

const MIN = 60_000;
const T0 = Date.UTC(2026, 9, 6, 10, 0, 0);
const iso = (ms: number) => new Date(ms).toISOString();

function event(id: string, startMin: number, durationMin = 30, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "cal",
    subject: `Subject ${id}`,
    startUtc: iso(T0 + startMin * MIN),
    endUtc: iso(T0 + (startMin + durationMin) * MIN),
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

const on = (...offsetsMinutes: number[]): ReminderSettings => ({ enabled: true, offsetsMinutes });

function memoryStore(initial: Record<string, number> = {}) {
  const state = { data: { ...initial }, saves: [] as Array<Record<string, number>>, failLoad: false, failSave: false };
  const store: ReminderStore = {
    async load() {
      if (state.failLoad) throw new Error("load failed");
      return { ...state.data };
    },
    async save(fired) {
      if (state.failSave) throw new Error("save failed");
      state.saves.push({ ...fired });
      state.data = { ...fired };
    },
  };
  return { store, state };
}

async function settle() {
  for (let i = 0; i < 12; i++) await Promise.resolve();
}

/** A manual clock and timer queue: sleeping (jump) moves the clock without running timers, like a suspended PC. */
function harness(store: ReminderStore = memoryStore().store, startAt = T0) {
  let clock = startAt;
  let nextId = 1;
  const timers = new Map<number, { at: number; fn: () => void }>();
  const fired: ReminderAlert[] = [];
  const engine = createReminderEngine({
    now: () => clock,
    store,
    onFire: (alert) => fired.push(alert),
    setTimer: (fn, ms) => {
      const id = nextId++;
      timers.set(id, { at: clock + ms, fn });
      return id;
    },
    clearTimer: (handle) => {
      timers.delete(handle as number);
    },
  });
  const runDueTimers = () => {
    for (const [id, timer] of [...timers]) {
      if (timer.at <= clock && timers.delete(id)) timer.fn();
    }
  };
  return {
    engine,
    fired,
    timers,
    now: () => clock,
    /** Time passes normally: every timer fires when due. */
    advance(ms: number) {
      const target = clock + ms;
      for (;;) {
        const next = [...timers.values()].reduce<number | null>((min, t) => (t.at <= target && (min === null || t.at < min) ? t.at : min), null);
        if (next === null) break;
        clock = Math.max(clock, next);
        runDueTimers();
      }
      clock = target;
    },
    /** The PC sleeps: the clock jumps, timers only fire afterwards, late. */
    sleep(ms: number) {
      clock += ms;
      runDueTimers();
    },
  };
}

describe("ReminderEngine", () => {
  it("fires exactly once, at the offset before the start, with the alert payload", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 60, 45, { location: "Room 1" })], on(30));

    h.advance(29 * MIN);
    expect(h.fired).toHaveLength(0);
    h.advance(1 * MIN);
    expect(h.fired).toEqual([
      {
        key: reminderKey("a", iso(T0 + 60 * MIN), { kind: "beforeStart", minutes: 30 }),
        eventId: "a",
        calendarId: "cal",
        subject: "Subject a",
        startUtc: iso(T0 + 60 * MIN),
        endUtc: iso(T0 + 105 * MIN),
        location: "Room 1",
        meetingUrl: null,
        minutesRemaining: 30,
        reminderType: { kind: "beforeStart", minutes: 30 },
      },
    ]);
    expect(h.fired[0].key).toBe(`a|${iso(T0 + 60 * MIN)}|minutes-30`);

    h.advance(60 * MIN);
    h.engine.update([event("a", 60, 45)], on(30)); // the calendar re-emits the same snapshot
    h.advance(MIN);
    expect(h.fired).toHaveLength(1);
  });

  it("never fires again after a restart because the fired set is loaded before scheduling", async () => {
    const { store, state } = memoryStore();
    const first = harness(store);
    await settle();
    first.engine.update([event("a", 40)], on(30));
    first.advance(10 * MIN);
    expect(first.fired).toHaveLength(1);
    await settle();
    expect(Object.keys(state.data)).toEqual([`a|${iso(T0 + 40 * MIN)}|minutes-30`]);

    const second = harness(store, T0 + 10 * MIN); // restarted at the moment the reminder was due
    await settle();
    second.engine.update([event("a", 40)], on(30));
    second.advance(60 * MIN);
    expect(second.fired).toHaveLength(0);
  });

  it("does not schedule anything until the persisted set has loaded", async () => {
    let release!: (value: Record<string, number>) => void;
    const store: ReminderStore = { load: () => new Promise((resolve) => (release = resolve)), save: async () => {} };
    const h = harness(store);
    h.engine.update([event("a", 30)], on(30)); // due right now
    await settle();
    expect(h.fired).toHaveLength(0);
    expect(h.timers.size).toBe(0);

    release({ [`a|${iso(T0 + 30 * MIN)}|minutes-30`]: T0 - MIN });
    await settle();
    expect(h.fired).toHaveLength(0); // it had already fired in a previous run
  });

  it("treats instances of a recurring series as separate reminders (same id, different start)", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("series", 40, 30, { isRecurring: true }), event("series", 24 * 60 + 40, 30, { isRecurring: true })], on(30));
    h.advance(10 * MIN);
    expect(h.fired).toHaveLength(1);
    h.advance(24 * 60 * MIN);
    expect(h.fired).toHaveLength(2);
    expect(h.fired[1].startUtc).toBe(iso(T0 + (24 * 60 + 40) * MIN));
  });

  it("fires again for a rescheduled meeting (new start, new key) but not for the old time", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 50)], on(30));
    h.advance(20 * MIN);
    expect(h.fired).toHaveLength(1);

    h.engine.update([event("a", 120)], on(30)); // moved two hours out
    h.advance(100 * MIN);
    expect(h.fired).toHaveLength(2);
    expect(h.fired[1].startUtc).toBe(iso(T0 + 120 * MIN));
    expect(h.fired[1].minutesRemaining).toBe(30);
  });

  it("skips declined, free, all-day, already started and already ended meetings", async () => {
    const h = harness();
    await settle();
    h.engine.update(
      [
        event("declined", 40, 30, { responseStatus: "declined" }),
        event("free", 41, 30, { busyStatus: "free" }),
        event("allday", 42, 30, { allDay: true }),
        event("started", -5, 60),
        event("ended", -90, 30),
      ],
      on(30)
    );
    h.advance(120 * MIN);
    expect(h.fired).toHaveLength(0);
    expect(h.timers.size).toBe(0);
  });

  it("does not mark an ineligible meeting as fired, so accepting it later still reminds", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 60, 30, { responseStatus: "declined" })], on(30));
    h.advance(10 * MIN);
    h.engine.update([event("a", 60)], on(30));
    h.advance(30 * MIN);
    expect(h.fired).toHaveLength(1);
  });

  describe("late fire", () => {
    it("fires late with the real remaining minutes", async () => {
      const h = harness();
      await settle();
      h.engine.update([event("a", 40)], on(30)); // due at +10
      h.sleep(14 * MIN); // PC slept through the due time; woke 4 minutes late
      expect(h.fired).toHaveLength(1);
      expect(h.fired[0].minutesRemaining).toBe(26);
    });

    it("still fires a far overdue reminder (app or Outlook started late) while the meeting is ahead", async () => {
      const h = harness();
      await settle();
      h.engine.update([event("a", 40)], on(30)); // due at +10
      h.sleep(25 * MIN); // 15 minutes late, meeting 15 minutes away
      expect(h.fired).toHaveLength(1);
      expect(h.fired[0].minutesRemaining).toBe(15);
    });

    it("skips (and remembers) an overdue reminder once the meeting is less than a minute away", async () => {
      const { store, state } = memoryStore();
      const h = harness(store);
      await settle();
      h.engine.update([event("a", 40)], on(30));
      h.sleep(39 * MIN + 30_000); // 29.5 minutes late, 30 s before the start
      expect(h.fired).toHaveLength(0);
      await settle();
      expect(Object.keys(state.data)).toEqual([`a|${iso(T0 + 40 * MIN)}|minutes-30`]);

      h.engine.update([event("a", 40)], on(30));
      h.advance(60 * MIN);
      expect(h.fired).toHaveLength(0);
    });

    it("skips when less than a minute is left before the start", async () => {
      const h = harness();
      await settle();
      h.engine.update([event("a", 30)], on(1)); // due at +29
      h.sleep(29 * MIN + 30_000); // 30 s late, 30 s before the start
      expect(h.fired).toHaveLength(0);
    });

    it("fires when exactly one minute is left", async () => {
      const h = harness();
      await settle();
      h.engine.update([event("a", 30)], on(5)); // due at +25
      h.sleep(29 * MIN); // 4 minutes late
      expect(h.fired).toHaveLength(1);
      expect(h.fired[0].minutesRemaining).toBe(1);
    });

    it("fires on time even when less than a minute of lead time was configured", async () => {
      const h = harness();
      await settle();
      h.engine.update([event("a", 30)], on(1));
      h.advance(29 * MIN + 2); // the timer ran 2 ms late
      expect(h.fired).toHaveLength(1);
      expect(h.fired[0].minutesRemaining).toBe(1);
    });
  });

  it("wakes at most every 60 seconds so a resume from sleep is noticed", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 600)], on(30)); // due in 570 minutes
    expect(h.timers.size).toBe(1);
    expect([...h.timers.values()][0].at - h.now()).toBeLessThanOrEqual(MAX_SLEEP_MS);
  });

  it("keeps a single timer aimed at the earliest due instant", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("late", 90), event("soon", 30.5), event("mid", 60)], on(30));
    expect(h.timers.size).toBe(1);
    expect([...h.timers.values()][0].at).toBe(T0 + 30_000);
  });

  it("schedules nothing while reminders are disabled, and picks up when they are enabled", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 40)], { enabled: false, offsetsMinutes: [30] });
    expect(h.timers.size).toBe(0);
    h.advance(60 * MIN);
    expect(h.fired).toHaveLength(0);

    const h2 = harness();
    await settle();
    h2.engine.update([event("a", 100)], { enabled: false, offsetsMinutes: [30] });
    h2.advance(10 * MIN);
    h2.engine.update([event("a", 100)], on(30));
    h2.advance(60 * MIN);
    expect(h2.fired).toHaveLength(1);
  });

  it("drops the schedule when reminders are switched off", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 100)], on(30));
    expect(h.timers.size).toBe(1);
    h.engine.update([event("a", 100)], { enabled: false, offsetsMinutes: [30] });
    expect(h.timers.size).toBe(0);
  });

  it("stops everything on dispose", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 40)], on(30));
    h.engine.dispose();
    expect(h.timers.size).toBe(0);
    h.advance(60 * MIN);
    h.engine.update([event("b", 40)], on(30));
    expect(h.fired).toHaveLength(0);
    expect(h.timers.size).toBe(0);
  });

  it("supports several offsets per meeting, each firing once", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 45)], on(30, 15, 5));
    h.advance(45 * MIN);
    expect(h.fired.map((alert) => alert.reminderType.minutes)).toEqual([30, 15, 5]);
    expect(h.fired.map((alert) => alert.minutesRemaining)).toEqual([30, 15, 5]);
    expect(h.fired.map((alert) => reminderTypeId(alert.reminderType))).toEqual(["minutes-30", "minutes-15", "minutes-5"]);
  });

  it("only marks reminders whose time has come, so a new offset still fires for a known meeting", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 45)], on(30));
    h.advance(15 * MIN);
    expect(h.fired).toHaveLength(1);
    h.engine.update([event("a", 45)], on(30, 10)); // user adds a second offset
    h.advance(30 * MIN);
    expect(h.fired.map((alert) => alert.reminderType.minutes)).toEqual([30, 10]);
  });

  it("ignores invalid offsets", async () => {
    const h = harness();
    await settle();
    h.engine.update([event("a", 45)], on(-5, Number.NaN, 30));
    h.advance(60 * MIN);
    expect(h.fired).toHaveLength(1);
  });

  describe("storage and callback failures", () => {
    it("still fires once in this session when saving fails, without throwing", async () => {
      const { store, state } = memoryStore();
      state.failSave = true;
      const h = harness(store);
      await settle();
      h.engine.update([event("a", 40)], on(30));
      h.advance(10 * MIN);
      await settle();
      h.engine.update([event("a", 40)], on(30));
      h.advance(60 * MIN);
      expect(h.fired).toHaveLength(1);
    });

    it("treats an unreadable store as empty", async () => {
      const { store, state } = memoryStore();
      state.failLoad = true;
      const h = harness(store);
      await settle();
      h.engine.update([event("a", 40)], on(30));
      h.advance(10 * MIN);
      expect(h.fired).toHaveLength(1);
    });

    it("keeps going, and counts the reminder as fired, when onFire throws", async () => {
      let calls = 0;
      let clock = T0;
      const timers: Array<() => void> = [];
      const engine = createReminderEngine({
        now: () => clock,
        store: memoryStore().store,
        onFire: () => {
          calls++;
          throw new Error("boom");
        },
        setTimer: (fn) => timers.push(fn),
        clearTimer: () => {},
      });
      await settle();
      engine.update([event("a", 40), event("b", 41)], on(30));
      clock += 11 * MIN;
      timers.splice(0).forEach((fn) => fn());
      expect(calls).toBe(2);
      engine.update([event("a", 40), event("b", 41)], on(30));
      expect(calls).toBe(2);
    });

    it("prunes fired entries older than the retention period when saving", async () => {
      const old = `old|${iso(T0 - 9 * 24 * 60 * MIN)}|minutes-30`;
      const recent = `recent|${iso(T0 - 60 * MIN)}|minutes-30`;
      const { store, state } = memoryStore({ [old]: T0 - FIRED_RETENTION_MS - MIN, [recent]: T0 - 2 * 60 * MIN });
      const h = harness(store);
      await settle();
      h.engine.update([event("a", 40)], on(30));
      h.advance(10 * MIN);
      await settle();
      expect(Object.keys(state.data).sort()).toEqual([`a|${iso(T0 + 40 * MIN)}|minutes-30`, recent].sort());
    });
  });

  describe("time zones and DST", () => {
    const originalTz = env.TZ;
    afterEach(() => {
      if (originalTz === undefined) delete env.TZ;
      else env.TZ = originalTz;
    });

    it("fires at the same instant in zones that spring forward, fall back and have odd offsets", async () => {
      // US spring-forward: 2026-03-08 07:00Z. The meeting starts 30 minutes after the jump.
      const start = Date.UTC(2026, 2, 8, 7, 30);
      for (const tz of ["America/New_York", "Europe/London", "Asia/Kolkata", "Pacific/Chatham", "UTC"]) {
        env.TZ = tz;
        const { store } = memoryStore();
        let clock = start - 31 * MIN;
        const timers: Array<{ at: number; fn: () => void }> = [];
        const fired: ReminderAlert[] = [];
        const engine = createReminderEngine({
          now: () => clock,
          store,
          onFire: (alert) => fired.push(alert),
          setTimer: (fn, ms) => timers.push({ at: clock + ms, fn }),
          clearTimer: () => {},
        });
        await settle();
        const dst = { ...event("dst", 0), startUtc: iso(start), endUtc: iso(start + 30 * MIN) };
        engine.update([dst], on(30));
        clock = start - 30 * MIN;
        timers.splice(0).forEach((t) => t.fn());
        expect(fired, tz).toHaveLength(1);
        expect(fired[0].minutesRemaining, tz).toBe(30);
      }
    });

    it("is not disturbed by the user's time zone changing while a reminder is pending", async () => {
      env.TZ = "America/New_York";
      const h = harness();
      await settle();
      h.engine.update([event("a", 60)], on(30));
      h.advance(10 * MIN);
      env.TZ = "Asia/Tokyo"; // laptop travelled
      h.advance(19 * MIN);
      expect(h.fired).toHaveLength(0);
      h.advance(MIN);
      expect(h.fired).toHaveLength(1);
      expect(h.fired[0].minutesRemaining).toBe(30);
    });
  });

  it("handles a thousand events quickly with a single timer", async () => {
    const h = harness();
    await settle();
    const events = Array.from({ length: 1000 }, (_, i) => event(`e${i}`, 40 + i, 30));
    const startedAt = performance.now();
    h.engine.update(events, on(30, 15, 5));
    expect(performance.now() - startedAt).toBeLessThan(500);
    expect(h.timers.size).toBe(1);
    h.advance(20 * MIN);
    expect(h.fired.length).toBeGreaterThan(0);
  });
});

describe("reminder keys", () => {
  it("uses minutes-N, and start for a zero offset", () => {
    expect(reminderTypeId({ kind: "beforeStart", minutes: 30 })).toBe("minutes-30");
    expect(reminderTypeId({ kind: "beforeStart", minutes: 0 })).toBe("start");
  });
});


describe("ReminderEngine and calendar sources", () => {
  const shared = (id: string, startMin: number) => event(id, startMin, 30, { calendarId: "team", calendarName: "Team", sourceKind: "shared" });

  it("reminds for checked shared calendars and carries their name to the alert", async () => {
    const h = harness();
    await settle();
    h.engine.update([shared("s", 40)], { ...on(30), sources: { kind: "all" } });
    h.advance(10 * MIN);
    expect(h.fired.map((a) => [a.eventId, a.calendarName, a.sourceKind])).toEqual([["s", "Team", "shared"]]);
  });

  it("with shared reminders off, shared events stay quiet and own ones still remind", async () => {
    const h = harness();
    await settle();
    h.engine.update([shared("s", 40), event("mine", 40)], { ...on(30), sources: { kind: "own" } });
    h.advance(10 * MIN);
    expect(h.fired.map((a) => a.eventId)).toEqual(["mine"]);
  });

  it("can follow a list of calendars", () => {
    expect(remindsFor({ kind: "calendars", calendarIds: ["team"] }, { calendarId: "team", sourceKind: "shared" })).toBe(true);
    expect(remindsFor({ kind: "calendars", calendarIds: ["team"] }, { calendarId: "cal", sourceKind: "primary" })).toBe(false);
    expect(remindsFor(undefined, { calendarId: "x", sourceKind: "other" })).toBe(true);
    expect(remindsFor({ kind: "own" }, { calendarId: "x", sourceKind: "personal" })).toBe(true);
    expect(remindsFor({ kind: "own" }, { calendarId: "x" })).toBe(true);
  });

  it("shows at most a few reminders for one moment, own calendars first, and never re-fires the rest", async () => {
    const h = harness();
    await settle();
    const burst = [shared("s1", 40), shared("s2", 40), shared("s3", 40), shared("s4", 40), event("mine", 40)];
    h.engine.update(burst, on(30));
    h.advance(10 * MIN);
    expect(h.fired).toHaveLength(MAX_REMINDERS_PER_BURST);
    expect(h.fired[0].eventId).toBe("mine");
    h.engine.update(burst, on(30));
    h.advance(5 * MIN);
    expect(h.fired).toHaveLength(MAX_REMINDERS_PER_BURST);
  });
});
