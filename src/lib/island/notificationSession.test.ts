// The notification session: one open, N payload changes, one close for any burst.
import { describe, expect, it } from "vitest";
import type { IslandNotification } from "../ipc";
import type { ReminderAlert } from "../reminders/types";
import {
  assertNotificationInvariants,
  initialIslandState,
  islandReducer,
  MAX_QUEUED_NOTIFICATIONS,
  notificationLevel,
  selectView,
  type IslandEvent,
  type IslandState,
} from "./state";
import { notificationDwellMs } from "./timing";

const alert = (key: string): ReminderAlert => ({
  key,
  eventId: key,
  subject: `Subject ${key}`,
  startUtc: "2026-10-06T10:30:00.000Z",
  endUtc: "2026-10-06T11:00:00.000Z",
  location: null,
  minutesRemaining: 30,
  reminderType: { kind: "beforeStart", minutes: 30 },
});

const notification = (id: number, extra: Partial<IslandNotification> = {}): IslandNotification => ({
  id,
  appName: "Teams",
  title: "t",
  body: "b",
  timestamp: 0,
  aumid: null,
  ...extra,
});

const run = (events: IslandEvent[], from: IslandState = initialIslandState) => events.reduce(islandReducer, from);
const kind = (state: IslandState) => selectView(state).kind;
const show = (id: number, at: number, extra: Partial<IslandNotification> = {}): IslandEvent => ({ type: "NOTIFICATION_SHOW", notification: notification(id, extra), at });
const invite = (id: number, at: number) => show(id, at, { invite: { id: `i${id}`, startUtc: null } });
const dwell = (at: number): IslandEvent => ({ type: "NOTIFICATION_DWELL_DONE", at });
const grace: IslandEvent = { type: "NOTIFICATION_GRACE_DONE" };
const currentId = (state: IslandState) => state.notification?.notification.id ?? null;
const queueIds = (state: IslandState) => state.notificationQueue.map((q) => q.notification.id);

/** Counts how often the island opens (anything else to notification) and closes (notification to idle). */
function transitions(events: IslandEvent[], from: IslandState = initialIslandState) {
  let state = from;
  let opens = 0;
  let closes = 0;
  const order: number[] = [];
  for (const event of events) {
    const before = selectView(state).kind;
    state = islandReducer(state, event);
    const view = selectView(state);
    if (before !== "notification" && view.kind === "notification") opens += 1;
    if (before === "notification" && view.kind === "idle") closes += 1;
    if (view.kind === "notification" && order[order.length - 1] !== view.notification.id) order.push(view.notification.id);
  }
  return { state, opens, closes, order };
}

describe("dwell policy", () => {
  it("shortens with the backlog but never below the floor", () => {
    const n = notification(1);
    expect([0, 1, 3, 4, 6, 7, 20].map((pending) => notificationDwellMs(n, pending))).toEqual([3500, 2800, 2800, 2200, 2200, 1800, 1800]);
  });

  it("invitations stay longer, the summary is brief", () => {
    expect(notificationDwellMs(notification(1, { invite: { id: "i", startUtc: null } }), 0)).toBe(9000);
    expect(notificationDwellMs(notification(1, { invite: { id: "i", startUtc: null } }), 2)).toBe(5000);
    expect(notificationDwellMs(notification(-1, { missedSummary: { count: 3 } }), 5)).toBe(2500);
  });
});

describe("bursts", () => {
  it("5 notifications 100 ms apart: one open, all five in order, never idle between, one close after the grace", () => {
    const events: IslandEvent[] = [show(1, 0), show(2, 100), show(3, 200), show(4, 300), show(5, 400)];
    for (let i = 0; i < 5; i += 1) events.push(dwell(1_000 + i));
    events.push(grace);
    const { state, opens, closes, order } = transitions(events);
    expect(order).toEqual([1, 2, 3, 4, 5]);
    expect([opens, closes]).toEqual([1, 1]);
    expect(kind(state)).toBe("idle");
    expect(state.sessionId).toBe(1);
  });

  it("the last toast lingers after its dwell, still visible", () => {
    const state = run([show(1, 0), dwell(3500)]);
    expect(selectView(state)).toMatchObject({ kind: "notification", phase: "lingering", notification: { id: 1 }, queueLength: 0 });
  });

  it("10 notifications in under a second: order kept, cap applied, one open, one close, the session ends", () => {
    const events: IslandEvent[] = Array.from({ length: 10 }, (_, i) => show(i + 1, i * 90));
    const burst = run(events);
    expect(currentId(burst)).toBe(1);
    expect(queueIds(burst)).toEqual([3, 4, 5, 6, 7, 8, 9, 10]);
    expect(burst.notificationQueue).toHaveLength(MAX_QUEUED_NOTIFICATIONS);
    expect(burst.droppedFromQueue).toBe(1);

    const ends: IslandEvent[] = Array.from({ length: 9 }, (_, i) => dwell(10_000 + i));
    ends.push(grace);
    const { state, opens, closes, order } = transitions([...events, ...ends]);
    expect(order).toEqual([1, 3, 4, 5, 6, 7, 8, 9, 10]);
    expect([opens, closes]).toEqual([1, 1]);
    expect(kind(state)).toBe("idle");
  });

  it("does not drop items for age during an ordinary long backlog", () => {
    const state = run([show(1, 0), show(2, 10), show(3, 20), dwell(20_000)]);
    expect(currentId(state)).toBe(2);
    expect(queueIds(state)).toEqual([3]);
  });

  it("a notification arriving during the grace is shown at once, with no close", () => {
    const { state, opens, closes } = transitions([show(1, 0), dwell(3500), show(2, 4000)]);
    expect(currentId(state)).toBe(2);
    expect(selectView(state)).toMatchObject({ phase: "showing" });
    expect([opens, closes]).toEqual([1, 0]);
    expect(state.sessionId).toBe(1);
    // The old grace firing late changes nothing.
    expect(currentId(islandReducer(state, grace))).toBe(2);
  });

  it("an arrival right after the session ended starts a new session at once", () => {
    const { state, opens, closes } = transitions([show(1, 0), dwell(3500), grace, show(2, 4500)]);
    expect(currentId(state)).toBe(2);
    expect([opens, closes]).toEqual([2, 1]);
    expect(state.sessionId).toBe(2);
  });
});

describe("duplicates", () => {
  it("the same id as the current one updates inline: no dwell reset, no new session", () => {
    const before = run([show(1, 0)]);
    const after = islandReducer(before, show(1, 2000, { title: "edited" }));
    expect(after.notification?.notification.title).toBe("edited");
    expect(after.notification?.shownAt).toBe(before.notification?.shownAt);
    expect(after.shownGeneration).toBe(before.shownGeneration);
    expect(after.sessionId).toBe(before.sessionId);
    expect(after.notificationQueue).toEqual([]);
  });

  it("the same id as a queued one replaces it in place", () => {
    const state = run([show(1, 0), show(2, 10), show(3, 20), show(2, 30, { title: "newer" })]);
    expect(queueIds(state)).toEqual([2, 3]);
    expect(state.notificationQueue[0].notification.title).toBe("newer");
  });
});

describe("the open panel", () => {
  it("a normal notification neither shows nor replaces the panel", () => {
    const state = run([{ type: "PIN", tab: "about" }, show(1, 0)]);
    expect(selectView(state)).toEqual({ kind: "userExpanded", tab: "about", pinned: true });
    expect(state.notification).toBeNull();
    expect(state.deferred).toEqual([]);
  });

  it("a meeting invitation is deferred and starts a session when the panel closes, if fresh", () => {
    const state = run([{ type: "USER_EXPAND" }, invite(1, 0), { type: "USER_COLLAPSE", at: 59_999 }]);
    expect(selectView(state)).toMatchObject({ kind: "notification", notification: { id: 1 } });
    expect(state.deferred).toEqual([]);
  });

  it("a deferred invitation older than 60 s is dropped", () => {
    expect(kind(run([{ type: "USER_EXPAND" }, invite(1, 0), { type: "USER_COLLAPSE", at: 60_000 }]))).toBe("idle");
  });

  it("keeps only the latest 3 deferred, in order", () => {
    const state = run([{ type: "USER_EXPAND" }, invite(1, 0), invite(2, 1), invite(3, 2), invite(4, 3)]);
    expect(state.deferred.map((d) => d.notification.id)).toEqual([2, 3, 4]);
    const closed = islandReducer(state, { type: "USER_COLLAPSE", at: 10 });
    expect(currentId(closed)).toBe(2);
    expect(queueIds(closed)).toEqual([3, 4]);
  });

  it("only an invitation is time-sensitive", () => {
    expect(notificationLevel(notification(1))).toBe("normal");
    expect(notificationLevel(notification(1, { invite: { id: "i", startUtc: null } }))).toBe("timeSensitive");
  });

  it("USER_EXPAND and PIN during a session end it (current and queue) and open the panel", () => {
    for (const open of [{ type: "USER_EXPAND" }, { type: "PIN" }] satisfies IslandEvent[]) {
      const state = run([show(1, 0), show(2, 1), open]);
      expect(kind(state)).toBe("userExpanded");
      expect(state.notification).toBeNull();
      expect(state.notificationQueue).toEqual([]);
    }
  });
});

describe("meeting alerts", () => {
  it("the toast goes back to the front of the queue, which continues after the alert", () => {
    let state = run([show(1, 0), show(2, 10), show(3, 20), { type: "ALERT_SHOW", alert: alert("a") }]);
    expect(state.notification).toBeNull();
    expect(queueIds(state)).toEqual([1, 2, 3]);
    state = run([show(4, 30), { type: "ALERT_DONE", at: 5_000 }], state);
    expect(currentId(state)).toBe(1);
    expect(queueIds(state)).toEqual([2, 3, 4]);
  });

  it("the toast shown again gets a new generation (its dwell restarts)", () => {
    const before = run([show(1, 0)]);
    const after = run([{ type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_DONE", at: 8000 }], before);
    expect(currentId(after)).toBe(1);
    expect(after.shownGeneration).toBeGreaterThan(before.shownGeneration);
  });

  it("drops what went stale behind the alert and continues with the fresh ones", () => {
    const state = run([show(1, 0), show(2, 100), { type: "ALERT_SHOW", alert: alert("a") }, show(3, 14_000), { type: "ALERT_DONE", at: 16_000 }]);
    expect(currentId(state)).toBe(3);
    expect(queueIds(state)).toEqual([]);
  });

  it("goes idle when everything went stale", () => {
    const state = run([show(1, 0), show(2, 1), { type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_DONE", at: 60_000 }]);
    expect(kind(state)).toBe("idle");
    expect(state.notificationQueue).toEqual([]);
  });

  it("a toast that already had its dwell is not shown again after an alert", () => {
    expect(kind(run([show(1, 0), dwell(3500), { type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_DONE", at: 8000 }]))).toBe("idle");
  });

  it("an alert preempts the open panel as before", () => {
    expect(kind(run([{ type: "PIN" }, { type: "ALERT_SHOW", alert: alert("a") }]))).toBe("meetingAlert");
  });

  it("TICK after a hidden window drops a stale current toast and the stale queue", () => {
    expect(kind(run([show(1, 0), show(2, 100), { type: "TICK", at: 60_000 }]))).toBe("idle");
  });

  it("TICK leaves an ordinary backlog alone", () => {
    const base = run([show(1, 0), show(2, 100), dwell(9_000)]);
    expect(islandReducer(base, { type: "TICK", at: 12_000 })).toBe(base);
  });
});

describe("user actions", () => {
  it("NOTIFICATION_DONE shows the next at once when there is one", () => {
    const state = run([show(1, 0), show(2, 10), { type: "NOTIFICATION_DONE", at: 500 }]);
    expect(currentId(state)).toBe(2);
    expect(kind(state)).toBe("notification");
  });

  it("NOTIFICATION_DONE with nothing pending ends the session with no grace", () => {
    expect(kind(run([show(1, 0), { type: "NOTIFICATION_DONE" }]))).toBe("idle");
    expect(kind(run([show(1, 0), dwell(3500), { type: "NOTIFICATION_DONE" }]))).toBe("idle");
  });

  it("NOTIFICATION_SESSION_END clears the current one and the queue", () => {
    const state = run([show(1, 0), show(2, 10), show(3, 20), { type: "NOTIFICATION_SESSION_END" }]);
    expect(kind(state)).toBe("idle");
    expect(state.notificationQueue).toEqual([]);
  });

  it("the missed summary is never the one dropped by the cap", () => {
    const events = [show(100, 0), show(-1, 0, { missedSummary: { count: 9 } }), ...Array.from({ length: 12 }, (_, i) => show(i + 1, i + 1))];
    const state = run(events);
    expect(state.notificationQueue).toHaveLength(MAX_QUEUED_NOTIFICATIONS);
    expect(state.notificationQueue[0].notification.id).toBe(-1);
  });
});

describe("invariants", () => {
  it("throws on a queue with nothing showing, duplicates and an over-long queue", () => {
    const q = (id: number) => ({ notification: notification(id), receivedAt: 0 });
    expect(() => assertNotificationInvariants({ ...initialIslandState, notificationQueue: [q(1)] })).toThrow();
    const current = { ...q(1), shownAt: 0, phase: "showing" as const };
    expect(() => assertNotificationInvariants({ ...initialIslandState, notification: current, notificationQueue: [q(1)] })).toThrow();
    expect(() => assertNotificationInvariants({ ...initialIslandState, notification: current, notificationQueue: [q(2), q(2)] })).toThrow();
    const long = Array.from({ length: MAX_QUEUED_NOTIFICATIONS + 1 }, (_, i) => q(i + 10));
    expect(() => assertNotificationInvariants({ ...initialIslandState, notification: current, notificationQueue: long })).toThrow();
    expect(() => assertNotificationInvariants({ ...initialIslandState, alert: alert("a"), notificationQueue: [q(5)] })).not.toThrow();
  });

  it("holds across 2000 random events of every type (seeded)", () => {
    let seed = 20261007;
    const rand = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 2 ** 32;
    };
    let state = initialIslandState;
    let clock = 0;
    let sessions = 0;
    for (let i = 0; i < 2000; i += 1) {
      clock += Math.floor(rand() * 4_000);
      const id = Math.floor(rand() * 14);
      const events: IslandEvent[] = [
        show(id, clock),
        show(id, clock),
        show(id, clock),
        invite(id, clock),
        show(-id - 1, clock, { missedSummary: { count: id } }),
        dwell(clock),
        dwell(clock),
        grace,
        { type: "NOTIFICATION_DONE", at: clock },
        { type: "NOTIFICATION_DONE" },
        { type: "NOTIFICATION_SESSION_END" },
        { type: "ALERT_SHOW", alert: alert(`k${id % 4}`) },
        { type: "ALERT_DONE", at: clock },
        { type: "USER_EXPAND" },
        { type: "PIN", tab: "about" },
        { type: "USER_COLLAPSE", at: clock },
        { type: "USER_COLLAPSE" },
        { type: "RINGER_SHOW", ringer: { key: "r", phase: "start", silent: false, untilMs: 0 } },
        { type: "RINGER_TOGGLE" },
        { type: "RINGER_DONE" },
        { type: "TICK", at: clock },
      ];
      state = islandReducer(state, events[Math.floor(rand() * events.length)]);
      assertNotificationInvariants(state);
      sessions = state.sessionId;
    }
    expect(sessions).toBeGreaterThan(5);
  });
});
