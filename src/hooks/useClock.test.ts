// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createClockStore } from "./useClock";

const at = (h: number, m: number, s = 0, ms = 0) => new Date(2026, 9, 6, h, m, s, ms);

describe("clock store", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(at(10, 15, 20));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("sleeps while nobody is subscribed", () => {
    createClockStore();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("wakes once per minute, aligned to the minute boundary, never per second", () => {
    const store = createClockStore();
    const listener = vi.fn();
    store.subscribeMinute(listener);
    expect(vi.getTimerCount()).toBe(1);

    vi.advanceTimersByTime(39_000); // 10:15:59
    expect(listener).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1_100); // just past 10:16:00
    expect(listener).toHaveBeenCalledTimes(1);
    expect(new Date(store.getMinute().minuteStart).getMinutes()).toBe(16);

    vi.advanceTimersByTime(60_000);
    expect(listener).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(1);
  });

  it("recomputes from the real clock after a long sleep instead of counting ticks", () => {
    const store = createClockStore();
    const listener = vi.fn();
    store.subscribeMinute(listener);

    // Machine sleeps for 3 hours; the pending timeout fires late.
    vi.setSystemTime(at(13, 7, 30));
    vi.advanceTimersByTime(60_000);

    expect(listener).toHaveBeenCalled();
    const snap = new Date(store.getMinute().minuteStart);
    expect(snap.getHours()).toBe(13);
    expect(snap.getMinutes()).toBe(8);
  });

  it("with only the day subscribed, still catches a midnight that passed while the PC slept", () => {
    vi.setSystemTime(at(14, 0, 0));
    const store = createClockStore();
    const onDay = vi.fn();
    store.subscribeDay(onDay);

    // Slept overnight: the pending timeout did not count the sleep, so it has not fired yet.
    vi.setSystemTime(new Date(2026, 9, 7, 7, 0, 0));
    vi.advanceTimersByTime(61_000);

    expect(onDay).toHaveBeenCalledTimes(1);
    expect(new Date(store.getDay().dayStart).getDate()).toBe(7);
  });

  it("with only the day subscribed, switches day at midnight", () => {
    vi.setSystemTime(at(23, 59, 30));
    const store = createClockStore();
    const onDay = vi.fn();
    store.subscribeDay(onDay);
    expect(vi.getTimerCount()).toBe(1);

    vi.advanceTimersByTime(29_000);
    expect(onDay).not.toHaveBeenCalled();
    vi.advanceTimersByTime(2_000); // past midnight
    expect(onDay).toHaveBeenCalledTimes(1);
    expect(new Date(store.getDay().dayStart).getDate()).toBe(7);
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(60_000);
    expect(onDay).toHaveBeenCalledTimes(1);
  });

  it("does not notify minute subscribers about the day change channel and vice versa", () => {
    const store = createClockStore();
    const onMinute = vi.fn();
    const onDay = vi.fn();
    store.subscribeMinute(onMinute);
    store.subscribeDay(onDay);
    vi.advanceTimersByTime(41_000);
    expect(onMinute).toHaveBeenCalledTimes(1);
    expect(onDay).not.toHaveBeenCalled();
  });

  it("clears its timer when the last subscriber leaves", () => {
    const store = createClockStore();
    const offMinute = store.subscribeMinute(() => {});
    const offDay = store.subscribeDay(() => {});
    offMinute();
    expect(vi.getTimerCount()).toBe(1); // day subscriber keeps one timer
    offDay();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("catches up when the page becomes visible again", () => {
    const store = createClockStore();
    const listener = vi.fn();
    store.subscribeMinute(listener);

    vi.setSystemTime(at(10, 45, 5)); // timers were throttled while hidden
    document.dispatchEvent(new Event("visibilitychange"));

    expect(listener).toHaveBeenCalledTimes(1);
    expect(new Date(store.getMinute().minuteStart).getMinutes()).toBe(45);
  });

  it("returns stable snapshots between changes and fresh ones when idle", () => {
    const store = createClockStore();
    expect(store.getMinute()).toBe(store.getMinute());
    const before = store.getMinute();
    vi.setSystemTime(at(10, 20, 1)); // nobody subscribed, so reads refresh
    expect(store.getMinute()).not.toBe(before);
    expect(new Date(store.getMinute().minuteStart).getMinutes()).toBe(20);
  });
});
