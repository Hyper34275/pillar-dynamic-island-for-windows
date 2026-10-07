import { describe, expect, it } from "vitest";
import { layoutCompact } from "./compactLayout";

// 8px per character keeps the arithmetic obvious.
const measure = (text: string) => text.length * 8;
const base = { measure, paddingX: 15, gap: 14, maxWidth: 220 };

describe("layoutCompact", () => {
  it("uses the long weekday when it fits", () => {
    const result = layoutCompact({ ...base, date: "10/6", weekdayLong: "Tuesday", weekdayShort: "Tue" });
    expect(result.weekday).toBe("Tuesday");
    expect(result.contentWidth).toBe(4 * 8 + 14 + 7 * 8);
  });

  it("falls back to the short weekday when the long one is too wide", () => {
    const result = layoutCompact({ ...base, date: "10/6", weekdayLong: "Donnerstagabendsonntag", weekdayShort: "Do" });
    expect(result.weekday).toBe("Do");
    expect(result.contentWidth).toBe(4 * 8 + 14 + 2 * 8);
  });

  it("clamps to the budget and keeps the short form when even that overflows", () => {
    const result = layoutCompact({ ...base, date: "10/6", weekdayLong: "x".repeat(40), weekdayShort: "y".repeat(30) });
    expect(result.weekday).toBe("y".repeat(30));
    expect(result.contentWidth).toBe(220 - 30);
  });

  it("never reports a width that would push the pill past its maximum", () => {
    for (const long of ["a", "abcdef", "abcdefghijklmnopqrstuvwxyz"]) {
      const { contentWidth } = layoutCompact({ ...base, date: "06/10", weekdayLong: long, weekdayShort: long.slice(0, 3) });
      expect(contentWidth + base.paddingX * 2).toBeLessThanOrEqual(base.maxWidth);
    }
  });
});

describe("layoutCompact, displays", () => {
  const withClock = { ...base, timeWidth: 30, gapFull: 10 };

  it("full: date, clock and weekday with the full gaps between them", () => {
    const result = layoutCompact({ ...withClock, display: "full", date: "10/6", weekdayLong: "Tuesday", weekdayShort: "Tue" });
    expect(result.weekday).toBe("Tuesday");
    expect(result.contentWidth).toBe(4 * 8 + 10 + 30 + 10 + 7 * 8);
  });

  it("full: falls back to the short weekday when the long one is over the budget", () => {
    // Budget is 220 - 30 = 190: 32 + 10 + 30 + 10 = 82 leaves 108 for the weekday.
    const result = layoutCompact({ ...withClock, display: "full", date: "10/6", weekdayLong: "x".repeat(14), weekdayShort: "Wed" });
    expect(result.weekday).toBe("Wed");
    expect(result.contentWidth).toBe(82 + 3 * 8);
  });

  it("full: never exceeds the pill's maximum, even when only an ellipsis can save it", () => {
    for (const long of ["a", "abcdef", "abcdefghijklmnopqrstuvwxyz", "x".repeat(60)]) {
      const { contentWidth } = layoutCompact({ ...withClock, display: "full", date: "06/10", weekdayLong: long, weekdayShort: long.slice(0, 40) });
      expect(contentWidth + base.paddingX * 2).toBeLessThanOrEqual(base.maxWidth);
    }
  });

  it("clock: only the clock's slot, no weekday", () => {
    const result = layoutCompact({ ...withClock, display: "clock", date: "10/6", weekdayLong: "Tuesday", weekdayShort: "Tue" });
    expect(result).toEqual({ weekday: "", contentWidth: 30 });
  });

  it("date: unchanged by the clock's inputs", () => {
    const plain = layoutCompact({ ...base, date: "10/6", weekdayLong: "Tuesday", weekdayShort: "Tue" });
    const result = layoutCompact({ ...withClock, display: "date", date: "10/6", weekdayLong: "Tuesday", weekdayShort: "Tue" });
    expect(result).toEqual(plain);
  });
});
