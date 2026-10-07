import { describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../ipc";
import { createNotificationHistory, isUnread } from "./history";

const note = (id: number): IslandNotification => ({ id, appName: "Teams", title: `t${id}`, body: "", timestamp: 0, aumid: null });

describe("notification history unread state", () => {
  it("counts everything as unread until the panel has been closed once", () => {
    const history = createNotificationHistory();
    history.add(note(1), 1000, false);
    const [entry] = history.getSnapshot();
    expect(history.getLastViewedAt()).toBe(0);
    expect(isUnread(entry, history.getLastViewedAt())).toBe(true);
  });

  it("keeps entries unread while the panel is open and clears them when it closes", () => {
    const history = createNotificationHistory();
    history.add(note(1), 1000, false);
    history.add(note(2), 2000, false);
    history.markViewed(3000);
    history.add(note(3), 4000, false);
    const unread = history.getSnapshot().map((entry) => [entry.notification.id, isUnread(entry, history.getLastViewedAt())]);
    expect(unread).toEqual([
      [3, true],
      [2, false],
      [1, false],
    ]);
  });

  it("does not notify subscribers when only the viewed time changes", () => {
    const history = createNotificationHistory();
    const listener = vi.fn();
    history.subscribe(listener);
    history.markViewed(10);
    expect(listener).not.toHaveBeenCalled();
  });
});
