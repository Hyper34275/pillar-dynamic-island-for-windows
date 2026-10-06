import { describe, expect, it } from "vitest";
import {
  dayLabel,
  formatTime,
  fullDate,
  msUntilNextDay,
  msUntilNextMinute,
  relativeMinutes,
  shortDate,
  startOfDay,
  stripYear,
  timeParts,
  weekdayLong,
  weekdayShort,
} from "./dateFormat";

// Local-time dates, so the assertions hold in any time zone the tests run in.
const TUESDAY = new Date(2026, 9, 6, 15, 30); // Tue 6 Oct 2026, 15:30
const MIDNIGHT = new Date(2026, 9, 6, 0, 5); // Tue 6 Oct 2026, 00:05

describe("shortDate", () => {
  it("keeps the order Intl gives each locale and never shows the year", () => {
    expect(shortDate(TUESDAY, "en-US")).toBe("10/6"); // month first
    expect(shortDate(TUESDAY, "en-GB")).toBe("06/10"); // day first
    expect(shortDate(TUESDAY, "he-IL")).toBe("6.10");
    expect(shortDate(TUESDAY, "he")).toBe("6.10");
    expect(shortDate(TUESDAY, "de")).toBe("6.10");
    expect(shortDate(TUESDAY, "ja")).toBe("10/6"); // year-first locale: year and its separator dropped
    expect(shortDate(TUESDAY, "ko")).toBe("10. 6.");
    expect(shortDate(TUESDAY, "bg")).toBe("6.10"); // year suffix dropped too
  });

  it("drops the year for Arabic, whatever the digit system", () => {
    for (const locale of ["ar", "ar-EG"]) {
      const text = shortDate(TUESDAY, locale);
      expect(text).not.toMatch(/2026|٢٠٢٦/);
      expect(text.length).toBeGreaterThan(2);
    }
  });

  it("differs between locales that order day and month differently", () => {
    expect(shortDate(TUESDAY, "en-US")).not.toBe(shortDate(TUESDAY, "he-IL"));
  });
});

describe("stripYear", () => {
  const part = (type: Intl.DateTimeFormatPartTypes, value: string): Intl.DateTimeFormatPart => ({ type, value });

  it("handles a year in the middle", () => {
    expect(stripYear([part("day", "6"), part("literal", "/"), part("year", "2026"), part("literal", "/"), part("month", "10")])).toBe("6/10");
  });

  it("returns the text unchanged when there is no year", () => {
    expect(stripYear([part("day", "6"), part("literal", "."), part("month", "10")])).toBe("6.10");
  });
});

describe("weekday", () => {
  it("returns long and short names from Intl", () => {
    expect(weekdayLong(TUESDAY, "en-US")).toBe("Tuesday");
    expect(weekdayShort(TUESDAY, "en-US")).toBe("Tue");
    expect(weekdayLong(TUESDAY, "he-IL")).toBe("יום שלישי");
    expect(weekdayShort(TUESDAY, "he-IL")).toBe("יום ג׳");
    expect(weekdayLong(TUESDAY, "de")).toBe("Dienstag");
    expect(weekdayLong(TUESDAY, "ja")).toBe("火曜日");
    expect(weekdayLong(TUESDAY, "ar")).toBe("الثلاثاء");
  });
});

describe("time", () => {
  it("uses the locale's hour cycle and zero-pads 24-hour clocks", () => {
    expect(formatTime(TUESDAY, "en-GB")).toBe("15:30");
    expect(formatTime(TUESDAY, "he-IL")).toBe("15:30");
    expect(formatTime(TUESDAY, "de")).toBe("15:30");
    expect(formatTime(TUESDAY, "ja")).toBe("15:30");
    expect(formatTime(TUESDAY, "en-US")).toMatch(/^3:30\sPM$/);
  });

  it("never renders midnight as 24:00", () => {
    expect(formatTime(MIDNIGHT, "en-GB")).toBe("00:05");
    expect(formatTime(MIDNIGHT, "he-IL")).toBe("00:05");
    expect(formatTime(MIDNIGHT, "de")).toBe("00:05");
    expect(formatTime(MIDNIGHT, "ja")).toBe("00:05");
    expect(formatTime(MIDNIGHT, "en-US")).toMatch(/^12:05\sAM$/);
    for (const locale of ["en-US", "en-GB", "he-IL", "ar", "de", "ja", "fr", "ru"]) {
      expect(formatTime(MIDNIGHT, locale), locale).not.toMatch(/(^|\D)24:/);
    }
  });

  it("splits the day period from the digits for 12-hour clocks", () => {
    const us = timeParts(TUESDAY, "en-US");
    expect(us.digits).toBe("3:30");
    expect(us.period).toBe("PM");
    expect(timeParts(TUESDAY, "he-IL")).toEqual({ digits: "15:30", period: null });
  });
});

describe("fullDate", () => {
  it("formats the full date in the locale's own order", () => {
    expect(fullDate(TUESDAY, "en-US")).toBe("Tuesday, October 6, 2026");
    expect(fullDate(TUESDAY, "en-GB")).toBe("Tuesday, 6 October 2026");
    expect(fullDate(TUESDAY, "he-IL")).toBe("יום שלישי, 6 באוקטובר 2026");
    expect(fullDate(TUESDAY, "de")).toBe("Dienstag, 6. Oktober 2026");
    expect(fullDate(TUESDAY, "ja")).toBe("2026年10月6日火曜日");
  });
});

describe("relativeMinutes", () => {
  it("rolls minutes up to hours and days using Intl", () => {
    expect(relativeMinutes(25, "en-GB")).toBe("in 25 min");
    expect(relativeMinutes(-3, "en-GB")).toBe("3 min ago");
    expect(relativeMinutes(150, "en-GB")).toBe("in 3 hr");
    expect(relativeMinutes(60 * 24 * 3, "en-GB")).toBe("in 3 days");
    expect(relativeMinutes(25, "he-IL")).toBe("בעוד 25 דק׳");
  });
});

describe("minute and day helpers", () => {
  it("msUntilNextMinute counts down to the next minute boundary", () => {
    expect(msUntilNextMinute(new Date(2026, 9, 6, 10, 15, 0, 0).getTime())).toBe(60_000);
    expect(msUntilNextMinute(new Date(2026, 9, 6, 10, 15, 30, 250).getTime())).toBe(29_750);
    expect(msUntilNextMinute(new Date(2026, 9, 6, 23, 59, 59, 500).getTime())).toBe(500);
  });

  it("msUntilNextDay lands exactly on the next local midnight", () => {
    const evening = new Date(2026, 9, 6, 23, 59, 59, 0).getTime();
    expect(msUntilNextDay(evening)).toBe(1000);
    const noon = new Date(2026, 9, 6, 12, 0, 0, 0).getTime();
    expect(new Date(noon + msUntilNextDay(noon)).getTime()).toBe(new Date(2026, 9, 7).getTime());
  });

  it("crosses month and year ends", () => {
    const lastDay = new Date(2026, 11, 31, 18, 0, 0, 0).getTime();
    expect(new Date(lastDay + msUntilNextDay(lastDay)).getTime()).toBe(new Date(2027, 0, 1).getTime());
  });

  it("is always strictly positive, even exactly on a boundary", () => {
    const midnight = new Date(2026, 9, 7, 0, 0, 0, 0).getTime();
    expect(msUntilNextMinute(midnight)).toBeGreaterThan(0);
    expect(msUntilNextDay(midnight)).toBeGreaterThan(0);
  });

  it("startOfDay is local midnight", () => {
    expect(startOfDay(new Date(2026, 9, 6, 17, 45).getTime())).toBe(new Date(2026, 9, 6).getTime());
  });
});

describe("dayLabel", () => {
  const now = TUESDAY.getTime();

  it("is null for today and anything earlier", () => {
    expect(dayLabel(new Date(2026, 9, 6, 23, 59), now, "en-GB")).toBeNull();
    expect(dayLabel(new Date(2026, 9, 5, 22, 0), now, "en-GB")).toBeNull();
  });

  it("says tomorrow through Intl, then falls back to the weekday", () => {
    expect(dayLabel(new Date(2026, 9, 7, 9, 0), now, "en-GB")).toBe("Tomorrow");
    expect(dayLabel(new Date(2026, 9, 7, 9, 0), now, "he-IL")).toBe("מחר");
    expect(dayLabel(new Date(2026, 9, 8, 9, 0), now, "en-GB")).toBe("Thursday");
  });

  it("counts calendar days, not 24-hour blocks, across a DST change", () => {
    const beforeDst = new Date(2026, 9, 24, 22, 0).getTime(); // Europe and the US change within days of this
    expect(dayLabel(new Date(2026, 9, 25, 7, 0), beforeDst, "en-GB")).toBe("Tomorrow");
  });
});
