// @vitest-environment jsdom
// The notification session at hook level: the timers that drive the reducer's dwell and grace.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../lib/ipc";
import { NOTIFICATION_DWELL_QUEUED_MS, NOTIFICATION_GRACE_MS, NOTIFICATION_MS } from "../lib/island/timing";
import { useIslandState, type IslandController } from "./useIslandState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const note = (id: number): IslandNotification => ({ id, appName: "App", title: `n${id}`, body: "", timestamp: 0, aumid: null });

let container: HTMLDivElement;
let root: Root;
let island: IslandController;
let kinds: string[];
let suppressed = false;

function Harness() {
  island = useIslandState({ suppressed });
  const kind = island.view.kind;
  if (kinds[kinds.length - 1] !== kind) kinds.push(kind);
  return null;
}

const mount = () => act(() => root.render(<Harness />));
/**
 * Time passes in small steps with React flushed between them: the grace countdown is only
 * created by the render that follows the dwell ending, as it is in real time.
 */
const ms = (n: number) => {
  for (let left = n; left > 0; left -= 50) act(() => vi.advanceTimersByTime(Math.min(50, left)));
};
const opens = () => kinds.filter((k, i) => k === "notification" && kinds[i - 1] !== "notification").length;
const closes = () => kinds.filter((k, i) => k === "idle" && kinds[i - 1] === "notification").length;
const shownId = () => (island.view.kind === "notification" ? island.view.notification.id : null);

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.setSystemTime(1_000_000);
  suppressed = false;
  kinds = [];
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  mount();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

describe("notification session timers", () => {
  it("5 notifications 100 ms apart open the island once, show every one, and close it once", () => {
    const seen: number[] = [];
    for (let id = 1; id <= 5; id += 1) {
      act(() => island.showNotification(note(id)));
      ms(100);
    }
    // 4 pending when the first became current's queue grew: it keeps the dwell it started with (lone).
    for (let t = 0; t < 40_000; t += 50) {
      const id = shownId();
      if (id !== null && seen[seen.length - 1] !== id) seen.push(id);
      ms(50);
    }
    expect(seen).toEqual([1, 2, 3, 4, 5]);
    expect(opens()).toBe(1);
    expect(closes()).toBe(1);
    expect(island.view.kind).toBe("idle");
  });

  it("a lone toast stays dwell + grace, the same total as before", () => {
    act(() => island.showNotification(note(1)));
    ms(NOTIFICATION_MS - 100);
    expect(island.view.kind).toBe("notification");
    ms(200);
    expect(island.view.kind).toBe("idle");
  });

  it("keeps the dwell of the moment it became current when the queue grows", () => {
    act(() => island.showNotification(note(1)));
    ms(2_000);
    act(() => island.showNotification(note(2)));
    ms(1_400);
    expect(shownId()).toBe(1);
    ms(200);
    expect(shownId()).toBe(2);
    // The second became current with nothing behind it: a lone dwell.
    ms(NOTIFICATION_DWELL_QUEUED_MS);
    expect(shownId()).toBe(2);
  });

  it("an arrival during the grace is shown at once, with no close", () => {
    act(() => island.showNotification(note(1)));
    ms(NOTIFICATION_MS - NOTIFICATION_GRACE_MS + 200);
    act(() => island.showNotification(note(2)));
    expect(shownId()).toBe(2);
    ms(NOTIFICATION_MS + 10);
    expect(island.view.kind).toBe("idle");
    expect([opens(), closes()]).toEqual([1, 1]);
  });

  it("pausing (a hidden window) holds dwell and grace", () => {
    act(() => island.showNotification(note(1)));
    ms(3_000);
    suppressed = true;
    mount();
    ms(60_000);
    expect(shownId()).toBe(1);
    suppressed = false;
    mount();
    ms(NOTIFICATION_MS);
    expect(island.view.kind).toBe("idle");
  });

  it("hovering holds the countdown too", () => {
    act(() => island.showNotification(note(1)));
    act(() => island.setHovering(true));
    ms(30_000);
    expect(shownId()).toBe(1);
    act(() => island.setHovering(false));
    ms(NOTIFICATION_MS + 10);
    expect(island.view.kind).toBe("idle");
  });

  it("endNotificationSession drops the queue, dismissNotification shows the next", () => {
    act(() => {
      island.showNotification(note(1));
      island.showNotification(note(2));
      island.showNotification(note(3));
    });
    act(() => island.dismissNotification());
    expect(shownId()).toBe(2);
    act(() => island.endNotificationSession());
    expect(island.view.kind).toBe("idle");
    expect(island.state.notificationQueue).toEqual([]);
  });

  it("an alert during a session: the toast comes back afterwards", () => {
    act(() => island.showNotification(note(1)));
    act(() => island.showNotification(note(2)));
    act(() =>
      island.showAlert({
        key: "a",
        eventId: "a",
        subject: "s",
        startUtc: new Date(Date.now() + 30 * 60_000).toISOString(),
        endUtc: new Date(Date.now() + 60 * 60_000).toISOString(),
        location: null,
        minutesRemaining: 30,
        reminderType: { kind: "beforeStart", minutes: 30 },
      })
    );
    expect(island.view.kind).toBe("meetingAlert");
    act(() => island.dismissAlert());
    expect(shownId()).toBe(1);
  });
});
