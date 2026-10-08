// The smart-search card in the island's reducer: priority 2.6, pre-emption both ways, in-place
// updates of one query, and the notification session waiting behind it.
import { describe, expect, it } from "vitest";
import type { AssistantCard } from "../assistant/types";
import type { IslandNotification } from "../ipc";
import type { ReminderAlert } from "../reminders/types";
import { assertNotificationInvariants, initialIslandState, islandReducer, PRIORITY, selectView, viewPriority, type IslandEvent, type IslandState } from "./state";
import { ASSISTANT_ANSWER_MS, ASSISTANT_CHOICES_MS, ASSISTANT_PROCESSING_MAX_MS, assistantDwellMs } from "./timing";

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
const notification = (id: number, extra: Partial<IslandNotification> = {}): IslandNotification => ({ id, appName: "Teams", title: "t", body: "b", timestamp: 0, aumid: null, ...extra });
const card = (queryId: string, extra: Partial<AssistantCard> = {}): AssistantCard => ({
  queryId,
  query: "",
  phase: "processing",
  lang: "he",
  title: "",
  summary: "",
  question: null,
  choices: [],
  items: [],
  total: 0,
  partial: false,
  canExtend: false,
  errorCode: null,
  sources: [],
  createdAt: 0,
  followUp: false,
  ...extra,
});

const run = (events: IslandEvent[], from: IslandState = initialIslandState) => events.reduce(islandReducer, from);
const kind = (state: IslandState) => selectView(state).kind;
const show = (c: AssistantCard, at = 0): IslandEvent => ({ type: "ASSISTANT_SHOW", card: c, at });
const update = (c: AssistantCard, at = 0): IslandEvent => ({ type: "ASSISTANT_UPDATE", card: c, at });
const toast = (id: number, at = 0): IslandEvent => ({ type: "NOTIFICATION_SHOW", notification: notification(id), at });

describe("assistant priority", () => {
  it("sits between the meeting alert and the ringer", () => {
    expect(PRIORITY.assistant).toBe(2.6);
    expect(PRIORITY.meetingAlert).toBeGreaterThan(PRIORITY.assistant);
    expect(PRIORITY.assistant).toBeGreaterThan(PRIORITY.ringer);
    expect(PRIORITY.assistant).toBeGreaterThan(PRIORITY.notification);
    expect(viewPriority({ kind: "assistant", card: card("q") })).toBe(2.6);
  });

  it("shows on a free island, over the panel, over a ringer", () => {
    expect(kind(run([show(card("q"))]))).toBe("assistant");
    const opened = run([{ type: "USER_EXPAND", tab: "notes" }, show(card("q"))]);
    expect(kind(opened)).toBe("assistant");
    expect(opened.expanded).toBe(false);
    expect(opened.pinned).toBe(false);
    expect(kind(run([{ type: "RINGER_SHOW", ringer: { key: "r", phase: "start", silent: false, untilMs: 1 } }, show(card("q"))]))).toBe("assistant");
  });

  it("a notification, a ringer and the panel never cover it", () => {
    const base = run([show(card("q"))]);
    expect(kind(run([toast(1)], base))).toBe("assistant");
    expect(kind(run([{ type: "RINGER_SHOW", ringer: { key: "r", phase: "start", silent: false, untilMs: 1 } }], base))).toBe("assistant");
  });
});

describe("assistant updates", () => {
  it("the same query id changes the card in place: nothing else moves", () => {
    const first = run([toast(1), show(card("q"))]);
    const next = run([update(card("q", { phase: "answer", title: "מחר יש לאיציק 3 פגישות" }), 50)], first);
    expect(next.assistant?.phase).toBe("answer");
    expect(next.sessionId).toBe(first.sessionId);
    expect(next.shownGeneration).toBe(first.shownGeneration);
    expect(next.notificationQueue).toEqual(first.notificationQueue);
    expect(selectView(next)).toEqual({ kind: "assistant", card: next.assistant });
  });

  it("a show of the same id is the same in-place update, and an update of a new id is a show", () => {
    const shown = run([show(card("q"))]);
    expect(run([show(card("q", { phase: "answer" }))], shown).assistant?.phase).toBe("answer");
    const other = run([update(card("p"))], shown);
    expect(other.assistant?.queryId).toBe("p");
  });

  it("a new query replaces the old card", () => {
    expect(run([show(card("a")), show(card("b"))]).assistant?.queryId).toBe("b");
  });
});

describe("assistant and the toast session", () => {
  it("sends the toast on screen back to the front of the line, and shows it again when the card ends", () => {
    const state = run([toast(1), toast(2), show(card("q"), 100)]);
    expect(kind(state)).toBe("assistant");
    expect(state.notification).toBeNull();
    expect(state.notificationQueue.map((q) => q.notification.id)).toEqual([1, 2]);
    const done = run([{ type: "ASSISTANT_DONE", at: 200 }], state);
    expect(kind(done)).toBe("notification");
    expect(done.notification?.notification.id).toBe(1);
    expect(done.notificationQueue.map((q) => q.notification.id)).toEqual([2]);
  });

  it("a notification that arrives under the card waits in the queue", () => {
    const state = run([show(card("q")), toast(7, 10)]);
    expect(state.notification).toBeNull();
    expect(state.notificationQueue.map((q) => q.notification.id)).toEqual([7]);
    expect(() => assertNotificationInvariants(state)).not.toThrow();
    expect(kind(run([{ type: "ASSISTANT_DONE", at: 100 }], state))).toBe("notification");
  });

  it("what waited longer than the stale limit is dropped when the card ends", () => {
    const state = run([show(card("q")), toast(7, 0)]);
    expect(kind(run([{ type: "ASSISTANT_DONE", at: 60_000 }], state))).toBe("idle");
  });

  it("a time-sensitive notification that waited for the panel joins the line instead of being lost", () => {
    const invite = notification(9, { invite: { id: "e", startUtc: null } });
    const state = run([{ type: "USER_EXPAND" }, { type: "NOTIFICATION_SHOW", notification: invite, at: 0 }, show(card("q"), 100)]);
    expect(state.deferred).toEqual([]);
    expect(state.notificationQueue.map((q) => q.notification.id)).toEqual([9]);
    expect(kind(run([{ type: "ASSISTANT_DONE", at: 200 }], state))).toBe("notification");
  });
});

describe("a meeting alert and the card", () => {
  it("the alert pre-empts the card, and the card comes back after it", () => {
    const state = run([show(card("q", { phase: "answer", title: "x" })), { type: "ALERT_SHOW", alert: alert("a") }]);
    expect(kind(state)).toBe("meetingAlert");
    expect(state.assistant?.queryId).toBe("q");
    const back = run([{ type: "ALERT_DONE", at: 100 }], state);
    expect(kind(back)).toBe("assistant");
    expect(back.assistant?.title).toBe("x");
  });

  it("an update that arrives under the alert is kept for when the alert ends", () => {
    const state = run([show(card("q")), { type: "ALERT_SHOW", alert: alert("a") }, update(card("q", { phase: "answer", title: "done" }))]);
    expect(kind(state)).toBe("meetingAlert");
    expect(kind(run([{ type: "ALERT_DONE", at: 0 }], state))).toBe("assistant");
    expect(run([{ type: "ALERT_DONE", at: 0 }], state).assistant?.title).toBe("done");
  });

  it("a card that arrives while an alert shows waits under it", () => {
    const state = run([{ type: "ALERT_SHOW", alert: alert("a") }, show(card("q"))]);
    expect(kind(state)).toBe("meetingAlert");
    expect(kind(run([{ type: "ALERT_DONE", at: 0 }], state))).toBe("assistant");
  });

  it("the toast queue keeps waiting behind the card after the alert", () => {
    const state = run([toast(1), show(card("q"), 10), { type: "ALERT_SHOW", alert: alert("a") }, { type: "ALERT_DONE", at: 20 }]);
    expect(kind(state)).toBe("assistant");
    expect(state.notification).toBeNull();
    expect(state.notificationQueue.map((q) => q.notification.id)).toEqual([1]);
    expect(kind(run([{ type: "ASSISTANT_DONE", at: 30 }], state))).toBe("notification");
  });

  it("ending the card while an alert shows does not start a toast under it", () => {
    const state = run([toast(1), show(card("q"), 10), { type: "ALERT_SHOW", alert: alert("a") }, { type: "ASSISTANT_DONE", at: 20 }]);
    expect(kind(state)).toBe("meetingAlert");
    expect(state.notification).toBeNull();
    expect(kind(run([{ type: "ALERT_DONE", at: 30 }], state))).toBe("notification");
  });
});

describe("ending the card", () => {
  it("goes back to idle, and ASSISTANT_DONE with nothing shown is a no-op", () => {
    expect(kind(run([show(card("q")), { type: "ASSISTANT_DONE", at: 0 }]))).toBe("idle");
    expect(run([{ type: "ASSISTANT_DONE", at: 0 }])).toBe(initialIslandState);
  });

  it("a done for an older query never ends a newer card", () => {
    const state = run([show(card("a")), show(card("b")), { type: "ASSISTANT_DONE", at: 0, queryId: "a" }]);
    expect(state.assistant?.queryId).toBe("b");
    expect(run([{ type: "ASSISTANT_DONE", at: 0, queryId: "b" }], state).assistant).toBeNull();
  });

  it("the panel the person opens afterwards shows (the shell ends the card first)", () => {
    const state = run([show(card("q")), { type: "ASSISTANT_DONE", at: 0 }, { type: "PIN", tab: "notes" }]);
    expect(kind(state)).toBe("userExpanded");
  });
});

describe("invariants", () => {
  it("allow a queue behind the card, and still reject a queue with nothing showing", () => {
    const withCard = run([show(card("q")), toast(1)]);
    expect(() => assertNotificationInvariants(withCard)).not.toThrow();
    expect(() => assertNotificationInvariants({ ...withCard, assistant: null })).toThrow(/queue is not empty/);
  });
});

describe("timing", () => {
  it("an answer 12 s, a question 60 s, a working card only a backstop", () => {
    expect(assistantDwellMs("answer")).toBe(ASSISTANT_ANSWER_MS);
    expect(assistantDwellMs("error")).toBe(ASSISTANT_ANSWER_MS);
    expect(assistantDwellMs("choices")).toBe(ASSISTANT_CHOICES_MS);
    expect(assistantDwellMs("processing")).toBe(ASSISTANT_PROCESSING_MAX_MS);
    expect(ASSISTANT_ANSWER_MS).toBe(12_000);
    expect(ASSISTANT_CHOICES_MS).toBe(60_000);
    expect(ASSISTANT_PROCESSING_MAX_MS).toBeGreaterThan(45_000);
  });
});
