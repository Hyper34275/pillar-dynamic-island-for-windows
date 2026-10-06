import { describe, expect, it } from "vitest";
import type { IslandNotification } from "../ipc";
import type { ReminderAlert } from "../reminders/types";
import {
  initialIslandState,
  islandReducer,
  MAX_QUEUED_ALERTS,
  NOTIFICATION_STALE_MS,
  PRIORITY,
  selectView,
  viewPriority,
  type IslandEvent,
  type IslandState,
} from "./state";

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

const notification = (id: number): IslandNotification => ({ id, appName: "Teams", title: "t", body: "b", timestamp: 0, aumid: null });

const run = (events: IslandEvent[], from: IslandState = initialIslandState) => events.reduce(islandReducer, from);
const kind = (state: IslandState) => selectView(state).kind;

describe("priorities", () => {
  it("orders meetingAlert > notification > userExpanded > idle", () => {
    expect(PRIORITY.meetingAlert).toBeGreaterThan(PRIORITY.notification);
    expect(PRIORITY.notification).toBeGreaterThan(PRIORITY.userExpanded);
    expect(PRIORITY.userExpanded).toBeGreaterThan(PRIORITY.idle);
  });

  // [what is showing, what arrives, what shows afterwards]
  const matrix: Array<[string, IslandEvent[], IslandEvent, string]> = [
    ["idle", [], { type: "USER_EXPAND" }, "userExpanded"],
    ["idle", [], { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }, "notification"],
    ["idle", [], { type: "ALERT_SHOW", alert: alert("a") }, "meetingAlert"],
    ["userExpanded", [{ type: "USER_EXPAND" }], { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }, "notification"],
    ["userExpanded", [{ type: "USER_EXPAND" }], { type: "ALERT_SHOW", alert: alert("a") }, "meetingAlert"],
    ["notification", [{ type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }], { type: "ALERT_SHOW", alert: alert("a") }, "meetingAlert"],
    ["notification", [{ type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }], { type: "USER_EXPAND" }, "notification"],
    ["notification", [{ type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }], { type: "PIN" }, "notification"],
    ["meetingAlert", [{ type: "ALERT_SHOW", alert: alert("a") }], { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }, "meetingAlert"],
    ["meetingAlert", [{ type: "ALERT_SHOW", alert: alert("a") }], { type: "USER_EXPAND" }, "meetingAlert"],
    ["meetingAlert", [{ type: "ALERT_SHOW", alert: alert("a") }], { type: "PIN" }, "meetingAlert"],
  ];

  it.each(matrix)("%s + %j then arrival -> %s", (_showing, setup, arrival, expected) => {
    const state = run([arrival], run(setup));
    expect(kind(state)).toBe(expected);
  });

  it("never lowers the visible priority by itself: only an end event does", () => {
    let state = run([{ type: "ALERT_SHOW", alert: alert("a") }]);
    const before = viewPriority(selectView(state));
    for (const event of [
      { type: "USER_EXPAND" },
      { type: "USER_COLLAPSE" },
      { type: "PIN" },
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "NOTIFICATION_DONE" },
      { type: "TICK", at: 1_000_000 },
    ] satisfies IslandEvent[]) {
      state = islandReducer(state, event);
      expect(viewPriority(selectView(state))).toBe(before);
    }
  });
});

describe("restoring the user's state", () => {
  it("shows the same tab again after a meeting alert", () => {
    const state = run([
      { type: "PIN", tab: "calendar" },
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "ALERT_DONE", at: 8000 },
    ]);
    expect(selectView(state)).toEqual({ kind: "userExpanded", tab: "calendar", pinned: true });
  });

  it("shows the same tab again after a notification", () => {
    const state = run([
      { type: "USER_EXPAND", tab: "about" },
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "NOTIFICATION_DONE" },
    ]);
    expect(selectView(state)).toEqual({ kind: "userExpanded", tab: "about", pinned: false });
  });

  it("does not leave the island expanded when the user collapsed it while an alert was showing", () => {
    const state = run([
      { type: "PIN", tab: "calendar" },
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "USER_COLLAPSE" }, // pointer left while the alert showed
      { type: "ALERT_DONE", at: 8000 },
    ]);
    expect(kind(state)).toBe("idle");
  });

  it("returns to idle after a temporary state when nothing was open", () => {
    expect(kind(run([{ type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_DONE", at: 1 }]))).toBe("idle");
    expect(kind(run([{ type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }, { type: "NOTIFICATION_DONE" }]))).toBe("idle");
  });

  it("remembers the last used tab across collapse and reopen", () => {
    const state = run([{ type: "USER_EXPAND", tab: "about" }, { type: "USER_COLLAPSE" }, { type: "USER_EXPAND" }]);
    expect(selectView(state)).toEqual({ kind: "userExpanded", tab: "about", pinned: false });
  });

  it("collapse clears the pin", () => {
    expect(run([{ type: "PIN" }, { type: "USER_COLLAPSE" }, { type: "USER_EXPAND" }]).pinned).toBe(false);
  });
});

describe("meeting alert queue", () => {
  it("queues a second alert and shows it when the first is done", () => {
    let state = run([{ type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_SHOW", alert: alert("b") }]);
    expect(state.alert?.key).toBe("a");
    expect(state.alertQueue.map((a) => a.key)).toEqual(["b"]);
    state = islandReducer(state, { type: "ALERT_DONE", at: 8000 });
    expect(state.alert?.key).toBe("b");
    expect(state.alertQueue).toEqual([]);
    expect(kind(islandReducer(state, { type: "ALERT_DONE", at: 16000 }))).toBe("idle");
  });

  it("ignores the same alert twice", () => {
    const state = run([{ type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_SHOW", alert: alert("b") }, { type: "ALERT_SHOW", alert: alert("b") }]);
    expect(state.alertQueue).toHaveLength(1);
  });

  it("bounds the queue", () => {
    const events: IslandEvent[] = Array.from({ length: MAX_QUEUED_ALERTS + 5 }, (_, i) => ({ type: "ALERT_SHOW", alert: alert(`k${i}`) }));
    expect(run(events).alertQueue).toHaveLength(MAX_QUEUED_ALERTS);
  });

  it("ALERT_DONE without an alert changes nothing", () => {
    expect(islandReducer(initialIslandState, { type: "ALERT_DONE", at: 0 })).toBe(initialIslandState);
  });
});

describe("notifications around meeting alerts", () => {
  it("queues the latest notification that arrives during an alert and shows it afterwards", () => {
    const state = run([
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 1000 },
      { type: "NOTIFICATION_SHOW", notification: notification(2), at: 2000 },
      { type: "ALERT_DONE", at: 8000 },
    ]);
    expect(selectView(state)).toMatchObject({ kind: "notification", notification: { id: 2 } });
    expect(state.waiting).toBeNull();
  });

  it("drops a queued notification that went stale during the alert", () => {
    const state = run([
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "ALERT_DONE", at: NOTIFICATION_STALE_MS },
    ]);
    expect(kind(state)).toBe("idle");
    expect(state.waiting).toBeNull();
  });

  it("keeps a queued notification just inside the freshness limit", () => {
    const state = run([
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "ALERT_DONE", at: NOTIFICATION_STALE_MS - 1 },
    ]);
    expect(kind(state)).toBe("notification");
  });

  it("puts a notification that was on screen back in line when an alert preempts it", () => {
    const state = run([
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "ALERT_DONE", at: 8000 },
    ]);
    expect(selectView(state)).toMatchObject({ kind: "notification", notification: { id: 1 } });
  });

  it("does not show the queued notification between two alerts", () => {
    const state = run([
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "ALERT_SHOW", alert: alert("b") },
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "ALERT_DONE", at: 8000 },
    ]);
    expect(selectView(state)).toMatchObject({ kind: "meetingAlert", alert: { key: "b" } });
    expect(state.waiting?.notification.id).toBe(1);
  });

  it("replaces a toast on screen with a newer one", () => {
    const state = run([
      { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 },
      { type: "NOTIFICATION_SHOW", notification: notification(2), at: 1000 },
    ]);
    expect(selectView(state)).toMatchObject({ notification: { id: 2 } });
  });
});

describe("TICK", () => {
  it("forgets a stale waiting notification and leaves everything else alone", () => {
    const base = run([{ type: "ALERT_SHOW", alert: alert("a") }, { type: "NOTIFICATION_SHOW", notification: notification(1), at: 0 }]);
    expect(islandReducer(base, { type: "TICK", at: NOTIFICATION_STALE_MS - 1 })).toBe(base);
    const ticked = islandReducer(base, { type: "TICK", at: NOTIFICATION_STALE_MS });
    expect(ticked.waiting).toBeNull();
    expect(ticked.alert?.key).toBe("a");
  });
});

describe("TICK and alerts that waited", () => {
  const MIN = 60_000;
  const start = Date.parse("2026-10-06T10:30:00.000Z");
  const shown = () => run([{ type: "ALERT_SHOW", alert: alert("a") }]);

  it("lowers the minutes to what is actually left", () => {
    const ticked = islandReducer(shown(), { type: "TICK", at: start - 12 * MIN });
    expect(ticked.alert?.minutesRemaining).toBe(12);
  });

  it("never raises the minutes", () => {
    const base = shown();
    expect(islandReducer(base, { type: "TICK", at: start - 45 * MIN })).toBe(base);
  });

  it("drops an alert whose meeting has started, and shows the next one", () => {
    const later = { ...alert("b"), startUtc: "2026-10-06T11:30:00.000Z", endUtc: "2026-10-06T12:00:00.000Z", minutesRemaining: 60 };
    const state = run([
      { type: "ALERT_SHOW", alert: alert("a") },
      { type: "ALERT_SHOW", alert: later },
    ]);
    const ticked = islandReducer(state, { type: "TICK", at: start + 5 * MIN });
    expect(ticked.alert?.key).toBe("b");
    expect(ticked.alert?.minutesRemaining).toBe(55);
    expect(ticked.alertQueue).toHaveLength(0);
  });

  it("returns to idle when the only alert is over", () => {
    expect(kind(islandReducer(shown(), { type: "TICK", at: start + 2 * MIN }))).toBe("idle");
  });

  it("keeps an alert for 'starting now' through its first minute", () => {
    const now = { ...alert("n"), minutesRemaining: 0, reminderType: { kind: "beforeStart" as const, minutes: 0 } };
    const base = run([{ type: "ALERT_SHOW", alert: now }]);
    expect(islandReducer(base, { type: "TICK", at: start + 30_000 })).toBe(base);
  });
});
