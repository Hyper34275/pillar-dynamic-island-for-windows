// @vitest-environment jsdom
// Do not disturb holds notifications back; turning it off says how many were missed and replays them.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFixedLocale } from "../lib/i18n";
import type { IslandNotification } from "../lib/ipc";
import { createDoNotDisturb, type DoNotDisturb } from "../lib/island/dnd";
import { createSilence } from "../lib/island/silence";
import { initialIslandState, type IslandState } from "../lib/island/state";
import { createNotificationHistory, type NotificationHistory } from "../lib/notifications/history";
import { createMissedQueue, type MissedQueue } from "../lib/notifications/missed";
import { deliverNotification } from "./useNotifications";
import { REPLAY_GAP_MS, useMissedReplay } from "./useMissedReplay";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const note = (id: number, title = `n${id}`): IslandNotification => ({ id, appName: "Teams", title, body: "", timestamp: 0, aumid: null });

type Island = Pick<IslandState, "notification" | "alert" | "ringer" | "expanded" | "tab">;

function Harness({ island, show, dnd, queue, history }: { island: Island; show: (n: IslandNotification) => void; dnd: DoNotDisturb; queue: MissedQueue; history: NotificationHistory }) {
  useMissedReplay(island, show, { dnd, queue, history });
  return null;
}

let container: HTMLDivElement;
let root: Root;
let windowsOn: boolean;
let dnd: DoNotDisturb;
let queue: MissedQueue;
let history: NotificationHistory;
let shown: IslandNotification[];
let island: Island;
const last = () => shown[shown.length - 1];

const render = () => act(() => root.render(<Harness island={island} show={(n) => shown.push(n)} dnd={dnd} queue={queue} history={history} />));
const arrive = (n: IslandNotification) => deliverNotification(n, Date.now(), history, createSilence(), (x) => shown.push(x), dnd, queue);
const setWindows = async (on: boolean) => {
  windowsOn = on;
  await act(async () => {
    await dnd.refresh();
  });
};
/** The toast on screen runs out (NOTIFICATION_DONE), and the gap passes. */
const toastDone = () => {
  island = { ...island, notification: null };
  render();
  act(() => vi.advanceTimersByTime(REPLAY_GAP_MS));
};
const toastShown = (n: IslandNotification) => {
  island = { ...island, notification: { notification: n, receivedAt: Date.now() } };
  render();
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
  island = { ...initialIslandState };
  render();
  await setWindows(false);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  setFixedLocale(null);
  vi.useRealTimers();
});

describe("missed notifications", () => {
  it("are held while Do not disturb is on, then replayed after a 'You missed N' summary, one at a time", async () => {
    await setWindows(true);
    expect(arrive(note(1))).toBe(false);
    expect(arrive(note(2))).toBe(false);
    expect(arrive(note(3))).toBe(false);
    expect(shown).toHaveLength(0);
    expect(history.getSnapshot().every((e) => e.missed && e.silenced)).toBe(true);

    await setWindows(false);
    act(() => vi.advanceTimersByTime(REPLAY_GAP_MS));
    expect(shown).toHaveLength(1);
    expect(shown[0].missedSummary).toEqual({ count: 3 });
    expect(shown[0].title).toBe("פספסת 3 התראות");

    // Nothing more while a toast is on screen.
    toastShown(shown[0]);
    act(() => vi.advanceTimersByTime(10_000));
    expect(shown).toHaveLength(1);

    for (const id of [1, 2, 3]) {
      toastDone();
      expect(last().id).toBe(id);
      toastShown(last());
    }
    toastDone();
    expect(shown.map((n) => n.id)).toEqual([shown[0].id, 1, 2, 3]);
  });

  it("all of them, even many, and the summary counts every one", async () => {
    await setWindows(true);
    for (let id = 1; id <= 15; id++) arrive(note(id));
    await setWindows(false);
    act(() => vi.advanceTimersByTime(REPLAY_GAP_MS));
    expect(shown[0].missedSummary).toEqual({ count: 15 });
    for (let i = 0; i < 15; i++) {
      toastShown(last());
      toastDone();
    }
    expect(shown.slice(1).map((n) => n.id)).toEqual(Array.from({ length: 15 }, (_, i) => i + 1));
  });

  it("wait while the panel is open on another tab, and stop once the Notifications tab is shown", async () => {
    await setWindows(true);
    arrive(note(1));
    arrive(note(2));
    island = { ...island, expanded: true, tab: "calendar" };
    render();
    // Turned off from the bell in the open panel: nothing covers the panel.
    await setWindows(false);
    act(() => vi.advanceTimersByTime(5_000));
    expect(shown).toHaveLength(0);
    // Closed: the replay starts.
    island = { ...island, expanded: false };
    render();
    act(() => vi.advanceTimersByTime(REPLAY_GAP_MS));
    expect(shown[0].missedSummary).toEqual({ count: 2 });
    // The summary was clicked: the Notifications tab lists them, so the replay ends.
    island = { ...island, notification: null, expanded: true, tab: "notifications" };
    render();
    island = { ...island, expanded: false };
    render();
    act(() => vi.advanceTimersByTime(5_000));
    expect(shown).toHaveLength(1);
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
    await setWindows(true);
    expect(history.getSnapshot()[0].missed).toBeUndefined();
    arrive(note(2));
    await setWindows(false);
    act(() => vi.advanceTimersByTime(REPLAY_GAP_MS));
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
