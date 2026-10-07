import { describe, expect, it } from "vitest";
import { meetingCountdown, minutesUntil } from "./countdown";

const START = Date.UTC(2026, 9, 7, 11, 0);
const at = (minutes: number, seconds: number) => START - (minutes * 60 + seconds) * 1000;

describe("meetingCountdown", () => {
  it.each([
    [4, 1, 5],
    [4, 0, 4],
    [3, 59, 4],
    [3, 1, 4],
    [3, 0, 3],
    [2, 59, 3],
    [1, 1, 2],
    [1, 0, 1],
    [0, 59, 1],
    [0, 1, 1],
  ])("%i:%s before the start → in %i min", (m, s, expected) => {
    expect(meetingCountdown(START, at(m, s))).toEqual({ kind: "upcoming", minutes: expected });
    expect(minutesUntil(START, at(m, s))).toBe(expected);
  });

  it("has started at 0:00 and after", () => {
    expect(meetingCountdown(START, START)).toEqual({ kind: "started" });
    expect(meetingCountdown(START, START + 30_000)).toEqual({ kind: "started" });
    expect(minutesUntil(START, START)).toBe(0);
  });

  it("treats an invalid start as started", () => {
    expect(meetingCountdown(Number.NaN, START)).toEqual({ kind: "started" });
  });

  it("never goes up as time passes", () => {
    let last = Infinity;
    for (let s = 6 * 60; s >= 0; s -= 1) {
      const n = minutesUntil(START, START - s * 1000);
      expect(n).toBeLessThanOrEqual(last);
      last = n;
    }
  });
});
