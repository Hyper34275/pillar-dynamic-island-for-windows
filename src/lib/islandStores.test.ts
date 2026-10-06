import { describe, expect, it, vi } from "vitest";
import { createInviteAnswers } from "./calendar/inviteAnswers";
import { createSilence } from "./island/silence";
import type { IslandNotification } from "./ipc";
import { createNotificationHistory } from "./notifications/history";

const note = (id: number, extra: Partial<IslandNotification> = {}): IslandNotification => ({
  id,
  appName: "Teams",
  title: `Title ${id}`,
  body: "",
  timestamp: 0,
  aumid: null,
  ...extra,
});

describe("notification history", () => {
  it("keeps the newest first, replaces a repeated id and drops the oldest beyond the limit", () => {
    const history = createNotificationHistory(3);
    history.add(note(1), 10, false);
    history.add(note(2), 20, true);
    history.add(note(1, { title: "again" }), 30, false);
    expect(history.getSnapshot().map((e) => e.notification.id)).toEqual([1, 2]);
    expect(history.getSnapshot()[0].notification.title).toBe("again");
    expect(history.getSnapshot()[1].silenced).toBe(true);
    history.add(note(3), 40, false);
    history.add(note(4), 50, false);
    expect(history.getSnapshot().map((e) => e.notification.id)).toEqual([4, 3, 1]);
  });

  it("removes one or clears all, and tells subscribers only about real changes", () => {
    const history = createNotificationHistory();
    const listener = vi.fn();
    history.subscribe(listener);
    history.add(note(1), 1, false);
    history.add(note(2), 2, false);
    history.remove(1);
    history.remove(99);
    expect(history.getSnapshot().map((e) => e.notification.id)).toEqual([2]);
    history.clear();
    history.clear();
    expect(history.getSnapshot()).toEqual([]);
    expect(listener).toHaveBeenCalledTimes(4);
  });
});

describe("silence", () => {
  it("is silent until the given time, then rings again by itself", () => {
    const silence = createSilence();
    expect(silence.isSilent(0)).toBe(false);
    silence.until(1000);
    expect(silence.isSilent(999)).toBe(true);
    expect(silence.getSnapshot()).toBe(1000);
    expect(silence.isSilent(1000)).toBe(false);
    expect(silence.getSnapshot()).toBeNull();
  });

  it("can be cleared by hand", () => {
    const silence = createSilence();
    const listener = vi.fn();
    silence.subscribe(listener);
    silence.until(5000);
    silence.clear();
    expect(silence.isSilent(1)).toBe(false);
    expect(listener).toHaveBeenCalledTimes(2);
  });
});

describe("invite answers", () => {
  it("goes sending -> done and does not answer twice", async () => {
    const send = vi.fn(async () => true);
    const answers = createInviteAnswers(send);
    const pending = answers.respond("inv", "accept");
    expect(answers.get("inv")).toEqual({ state: "sending", response: "accept" });
    expect(await pending).toBe(true);
    expect(answers.get("inv")).toEqual({ state: "done", response: "accept" });
    expect(await answers.respond("inv", "decline")).toBe(true);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("reports a failure and lets the user try again", async () => {
    const send = vi.fn().mockResolvedValueOnce(false).mockRejectedValueOnce(new Error("x")).mockResolvedValueOnce(true);
    const answers = createInviteAnswers(send);
    expect(await answers.respond("inv", "decline")).toBe(false);
    expect(answers.get("inv")?.state).toBe("failed");
    expect(await answers.respond("inv", "decline")).toBe(false);
    expect(await answers.respond("inv", "tentative")).toBe(true);
    expect(answers.get("inv")).toEqual({ state: "done", response: "tentative" });
  });
});
