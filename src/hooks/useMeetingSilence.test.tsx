// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarService } from "../lib/calendar/service";
import { WAITING_SNAPSHOT, type CalendarEventDto, type CalendarSnapshot } from "../lib/calendar/types";
import { createSilence, type Silence } from "../lib/island/silence";
import { initialIslandState, islandReducer, selectView, type Ringer } from "../lib/island/state";
import { createNotificationHistory } from "../lib/notifications/history";
import type { IslandNotification } from "../lib/ipc";
import { CalendarServiceContext } from "./useCalendar";
import { useMeetingSilence } from "./useMeetingSilence";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const T0 = Date.UTC(2026, 9, 6, 9, 58, 0);
const MIN = 60_000;

function meeting(id: string, startMs: number, endMs: number): CalendarEventDto {
  return {
    id,
    calendarId: "c",
    subject: id,
    startUtc: new Date(startMs).toISOString(),
    endUtc: new Date(endMs).toISOString(),
    allDay: false,
    location: null,
    organizer: null,
    isRecurring: false,
    meetingUrl: null,
    busyStatus: "busy",
    responseStatus: "accepted",
    color: null,
  };
}

function service(events: CalendarEventDto[]): CalendarService {
  const snapshot: CalendarSnapshot = { ...WAITING_SNAPSHOT, status: "connected", events };
  return { subscribe: () => () => {}, getSnapshot: () => snapshot, refresh: async () => {}, dispose: () => {} };
}

function Harness({ enabled, onShow, store }: { enabled: boolean | null; onShow: (r: Omit<Ringer, "toggles">) => void; store: Silence }) {
  useMeetingSilence(enabled, onShow, store);
  return null;
}

let container: HTMLDivElement;
let root: Root;
let shown: Array<Omit<Ringer, "toggles">>;
let store: Silence;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(T0);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  shown = [];
  store = createSilence();
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

function mount(events: CalendarEventDto[], enabled: boolean | null = true) {
  act(() => {
    root.render(
      <CalendarServiceContext.Provider value={service(events)}>
        <Harness enabled={enabled} onShow={(r) => shown.push(r)} store={store} />
      </CalendarServiceContext.Provider>
    );
  });
}

const advance = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));

describe("useMeetingSilence", () => {
  it("brings up the ring pill once when a meeting starts, ringing, until its end", async () => {
    mount([meeting("standup", T0 + 2 * MIN, T0 + 32 * MIN)]);
    expect(shown).toHaveLength(0);
    await advance(2 * MIN + 100);
    expect(shown).toEqual([{ key: `standup|${new Date(T0 + 2 * MIN).toISOString()}`, phase: "start", silent: false, untilMs: T0 + 32 * MIN }]);
    await advance(5 * MIN);
    expect(shown).toHaveLength(1);
  });

  it("does not offer it for a meeting that started a while ago, or while switched off", async () => {
    mount([meeting("old", T0 - 10 * MIN, T0 + 20 * MIN)]);
    expect(shown).toHaveLength(0);
    act(() => root.unmount());
    root = createRoot(container);
    mount([meeting("next", T0 + MIN, T0 + 30 * MIN)], false);
    await advance(2 * MIN);
    expect(shown).toHaveLength(0);
  });

  it("says it rings again when a silence the user chose runs out", async () => {
    mount([]);
    act(() => store.until(T0 + 3 * MIN));
    await advance(3 * MIN + 100);
    expect(shown).toEqual([{ key: `end|${T0 + 3 * MIN}`, phase: "end", silent: false, untilMs: T0 + 3 * MIN }]);
    expect(store.getSnapshot()).toBeNull();
  });

  it("stays quiet when the user turns silence off by hand, and runs no timer with nothing ahead", async () => {
    mount([]);
    expect(vi.getTimerCount()).toBe(0);
    act(() => store.until(T0 + 3 * MIN));
    act(() => store.clear());
    await advance(5 * MIN);
    expect(shown).toHaveLength(0);
  });
});

describe("ringer in the island state", () => {
  const ringer = { key: "m", phase: "start" as const, silent: false, untilMs: 1000 };

  it("shows above a toast and below a meeting alert, and toggles silent", () => {
    let state = islandReducer(initialIslandState, { type: "RINGER_SHOW", ringer });
    expect(selectView(state)).toEqual({ kind: "ringer", ringer: { ...ringer, toggles: 0 } });
    state = islandReducer(state, { type: "RINGER_TOGGLE" });
    expect(selectView(state)).toMatchObject({ kind: "ringer", ringer: { silent: true, toggles: 1 } });
    const withAlert = islandReducer(state, {
      type: "ALERT_SHOW",
      alert: { key: "a", eventId: "e", subject: "s", startUtc: "x", endUtc: "y", location: null, minutesRemaining: 5, reminderType: { kind: "beforeStart", minutes: 5 } },
    });
    expect(selectView(withAlert).kind).toBe("meetingAlert");
    state = islandReducer(state, { type: "RINGER_DONE" });
    expect(selectView(state).kind).toBe("idle");
  });
});

describe("notifications while silent", () => {
  const note: IslandNotification = { id: 7, appName: "Teams", title: "Hi", body: "", timestamp: 0, aumid: null };

  it("go to the history, marked silenced, without popping up; after the meeting they pop up again", async () => {
    const { deliverNotification } = await import("./useNotifications");
    const history = createNotificationHistory();
    const silent = createSilence();
    const popped: IslandNotification[] = [];
    silent.until(1000);
    expect(deliverNotification(note, 500, history, silent, (n) => popped.push(n))).toBe(false);
    expect(popped).toHaveLength(0);
    expect(history.getSnapshot()[0]).toMatchObject({ silenced: true, receivedAt: 500 });
    expect(deliverNotification({ ...note, id: 8 }, 1000, history, silent, (n) => popped.push(n))).toBe(true);
    expect(popped.map((n) => n.id)).toEqual([8]);
    expect(history.getSnapshot()[0]).toMatchObject({ silenced: false, notification: { id: 8 } });
  });
});
