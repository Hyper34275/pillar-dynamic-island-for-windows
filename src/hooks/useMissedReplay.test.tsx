// @vitest-environment jsdom
// Do not disturb holds notifications back; turning it off says how many were missed and replays them.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFixedLocale } from "../lib/i18n";
import type { IslandNotification } from "../lib/ipc";
import { createDoNotDisturb, type DoNotDisturb } from "../lib/island/dnd";
import { createSilence } from "../lib/island/silence";
import { initialIslandState, islandReducer, selectView, MAX_QUEUED_NOTIFICATIONS, type IslandState } from "../lib/island/state";
import { createNotificationHistory, type NotificationHistory } from "../lib/notifications/history";
import { createMissedQueue, type MissedQueue } from "../lib/notifications/missed";
import { deliverNotification } from "./useNotifications";
import { useMissedReplay } from "./useMissedReplay";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const note = (id: number, title = `n${id}`): IslandNotification => ({ id, appName: "Teams", title, body: "", timestamp: 0, aumid: null });

function Harness({ show, dnd, queue, history }: { show: (n: IslandNotification) => void; dnd: DoNotDisturb; queue: MissedQueue; history: NotificationHistory }) {
  useMissedReplay(show, { dnd, queue, history });
  return null;
}

let container: HTMLDivElement;
let root: Root;
let windowsOn: boolean;
let dnd: DoNotDisturb;
let queue: MissedQueue;
let history: NotificationHistory;
let shown: IslandNotification[];

const render = () => act(() => root.render(<Harness show={(n) => shown.push(n)} dnd={dnd} queue={queue} history={history} />));
const arrive = (n: IslandNotification) => deliverNotification(n, Date.now(), history, createSilence(), (x) => shown.push(x), dnd, queue);
const setWindows = async (on: boolean) => {
  windowsOn = on;
  await act(async () => {
    await dnd.refresh();
  });
};

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  setFixedLocale("he");
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  windowsOn = false;
  dnd = createDoNotDisturb({ get: async () => windowsOn, set: async (on) => (windowsOn = on) });
  queue = createMissedQueue();
  history = createNotificationHistory();
  shown = [];
  render();
  await setWindows(false);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  setFixedLocale(null);
  vi.useRealTimers();
});

/** What the island does with what the replay hands it: the real reducer, all at one instant. */
const intoIsland = (notifications: IslandNotification[], from: IslandState = initialIslandState) =>
  notifications.reduce((state, notification) => islandReducer(state, { type: "NOTIFICATION_SHOW", notification, at: 0 }), from);

describe("missed notifications", () => {
  it("are held while Do not disturb is on, then handed over at once, after a 'You missed N' summary", async () => {
    await setWindows(true);
    expect(arrive(note(1))).toBe(false);
    expect(arrive(note(2))).toBe(false);
    expect(arrive(note(3))).toBe(false);
    expect(shown).toHaveLength(0);
    expect(history.getSnapshot().every((e) => e.missed && e.silenced)).toBe(true);

    await setWindows(false);
    expect(shown).toHaveLength(4);
    expect(shown[0].missedSummary).toEqual({ count: 3 });
    expect(shown[0].title).toBe("פספסת 3 התראות");
    expect(shown.slice(1).map((n) => n.id)).toEqual([1, 2, 3]);

    // The session plays them in order: the summary first, the island free.
    const state = intoIsland(shown);
    expect(state.notification?.notification.missedSummary).toBeDefined();
    expect(state.notificationQueue.map((q) => q.notification.id)).toEqual([1, 2, 3]);
  });

  it("with many, the summary counts every one and the session never drops the summary", async () => {
    await setWindows(true);
    for (let id = 1; id <= 50; id++) arrive(note(id));
    await setWindows(false);
    expect(shown[0].missedSummary).toEqual({ count: 50 });
    expect(shown).toHaveLength(51);

    const free = intoIsland(shown);
    expect(free.notification?.notification.missedSummary).toBeDefined();
    expect(free.notificationQueue).toHaveLength(MAX_QUEUED_NOTIFICATIONS);

    // Island busy with another toast: the summary waits in line and is still not the one dropped.
    const busy = intoIsland(shown, intoIsland([note(900)]));
    expect(busy.notificationQueue).toHaveLength(MAX_QUEUED_NOTIFICATIONS);
    expect(busy.notificationQueue.some((q) => q.notification.missedSummary)).toBe(true);
  });

  it("an open panel is not covered by the replay", async () => {
    await setWindows(true);
    arrive(note(1));
    await setWindows(false);
    const open = islandReducer(initialIslandState, { type: "USER_EXPAND", tab: "notifications" });
    expect(selectView(intoIsland(shown, open)).kind).toBe("userExpanded");
  });

  it("nothing is replayed when nothing was missed, and a new Do not disturb starts a new batch", async () => {
    await setWindows(true);
    await setWindows(false);
    act(() => vi.advanceTimersByTime(5_000));
    expect(shown).toHaveLength(0);

    await setWindows(true);
    arrive(note(1));
    expect(history.getSnapshot()[0].missed).toBe(true);
    // Turned on again before the user came back: the first batch is ordinary history now.
    await setWindows(false);
    shown.length = 0;
    await setWindows(true);
    expect(history.getSnapshot()[0].missed).toBeUndefined();
    arrive(note(2));
    await setWindows(false);
    expect(shown[0].missedSummary).toEqual({ count: 1 });
  });

  it("a meeting silence alone holds nothing for a replay", () => {
    const silent = createSilence();
    silent.until(Date.now() + 60_000);
    expect(deliverNotification(note(1), Date.now(), history, silent, (x) => shown.push(x), dnd, queue)).toBe(false);
    expect(queue.size()).toBe(0);
    expect(history.getSnapshot()[0]).toMatchObject({ silenced: true });
    expect(history.getSnapshot()[0].missed).toBeUndefined();
  });
});
