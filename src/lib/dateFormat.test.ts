import { afterEach, describe, expect, it } from "vitest";
import { detectLocale, getFormatTag, getLocale, getLocaleTag, setFixedLocale, setFormatLocale, t } from "./i18n";
import {
  dayLabel,
  formatTime,
  fullDate,
  msUntilNextDay,
  msUntilNextMinute,
  relativeMinutes,
  relativePast,
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

  it("says one and two of every unit in correct Hebrew, future and past", () => {
    expect(relativeMinutes(1, "he-IL")).toBe("בעוד דקה");
    expect(relativeMinutes(-1, "he-IL")).toBe("לפני דקה");
    expect(relativeMinutes(60, "he-IL")).toBe("בעוד שעה");
    expect(relativeMinutes(120, "he-IL")).toBe("בעוד שעתיים");
    expect(relativeMinutes(-120, "he-IL")).toBe("לפני שעתיים");
    expect(relativeMinutes(180, "he-IL")).toBe("בעוד 3 שע׳");
    expect(relativeMinutes(24 * 60, "he-IL")).toBe("מחר");
    expect(relativeMinutes(48 * 60, "he-IL")).toBe("מחרתיים");
    expect(relativeMinutes(-24 * 60, "he-IL")).toBe("אתמול");
    expect(relativeMinutes(72 * 60, "he-IL")).toBe("בעוד 3 ימים");
    for (const m of [1, 60, 120, -60, -120]) expect(relativeMinutes(m, "he-IL")).not.toMatch(/\(\d+\)/);
  });
});

describe("relativePast", () => {
  const NOW = new Date(2026, 9, 6, 15, 30, 0).getTime();
  const ago = (ms: number) => NOW - ms;
  const MIN = 60_000;
  const HOUR = 60 * MIN;
  const DAY = 24 * HOUR;

  it("says now for the first minute, and for a time in the future", () => {
    expect(relativePast(ago(0), NOW, "en-GB")).toBe("now");
    expect(relativePast(ago(59_000), NOW, "en-GB")).toBe("now");
    expect(relativePast(NOW + 5 * MIN, NOW, "en-GB")).toBe("now");
    expect(relativePast(ago(0), NOW, "he-IL")).toBe("עכשיו");
  });

  it("rolls up from minutes to hours, days, weeks, months and years", () => {
    expect(relativePast(ago(MIN), NOW, "en-GB")).toBe("1 min ago");
    expect(relativePast(ago(59 * MIN + 59_000), NOW, "en-GB")).toBe("59 min ago");
    expect(relativePast(ago(HOUR), NOW, "en-GB")).toBe("1 hr ago");
    expect(relativePast(ago(5 * HOUR + 40 * MIN), NOW, "en-GB")).toBe("5 hr ago");
    expect(relativePast(ago(DAY), NOW, "en-GB")).toBe("yesterday");
    expect(relativePast(ago(3 * DAY), NOW, "en-GB")).toBe("3 days ago");
    expect(relativePast(ago(14 * DAY), NOW, "en-GB")).toMatch(/^2 wks? ago$/);
    expect(relativePast(ago(65 * DAY), NOW, "en-GB")).toMatch(/^2 mo(?:nth)?s? ago$/);
    expect(relativePast(ago(800 * DAY), NOW, "en-GB")).toMatch(/^2 yrs? ago$/);
  });

  it("says one and two of every unit in correct Hebrew, with no stray count in parentheses", () => {
    const he = (ms: number) => relativePast(ago(ms), NOW, "he-IL");
    expect(he(MIN)).toBe("לפני דקה");
    expect(he(2 * MIN)).toBe("לפני 2 דק׳");
    expect(he(HOUR)).toBe("לפני שעה");
    expect(he(2 * HOUR)).toBe("לפני שעתיים");
    expect(he(3 * HOUR)).toBe("לפני 3 שע׳");
    expect(he(DAY)).toBe("אתמול");
    expect(he(2 * DAY)).toBe("שלשום");
    expect(he(3 * DAY)).toBe("לפני 3 ימים");
    expect(he(7 * DAY)).toBe("לפני שבוע");
    expect(he(14 * DAY)).toBe("לפני שבועיים");
    expect(he(21 * DAY)).toBe("לפני 3 שב׳");
    expect(he(30 * DAY)).toBe("לפני חודש");
    expect(he(65 * DAY)).toBe("לפני חודשיים");
    expect(he(100 * DAY)).toBe("לפני 3 חודשים");
    expect(he(365 * DAY)).toBe("לפני שנה");
    expect(he(800 * DAY)).toBe("לפני שנתיים");
    expect(he(1200 * DAY)).toBe("לפני 3 שנים");
    for (const ms of [MIN, HOUR, 2 * HOUR, 14 * DAY, 65 * DAY, 800 * DAY]) expect(he(ms)).not.toMatch(/\(\d+\)/);
  });

  it("counts calendar days from a day on, so 47 hours ago is the day before yesterday", () => {
    const morning = new Date(2026, 9, 6, 9, 0, 0).getTime();
    const evening = (daysBack: number) => new Date(2026, 9, 6 - daysBack, 21, 0, 0).getTime();
    expect(relativePast(evening(2), morning, "he-IL")).toBe("שלשום"); // 36 hours, two midnights
    expect(relativePast(evening(1), morning, "he-IL")).toBe("לפני 12 שע׳"); // under a day: hours
    expect(relativePast(new Date(2026, 9, 4, 10, 0, 0).getTime(), morning, "he-IL")).toBe("שלשום"); // 47 hours
    expect(relativePast(evening(2), morning, "en-GB")).toBe("2 days ago");
    // 25 hours ago, but yesterday evening's neighbour: one midnight only
    expect(relativePast(new Date(2026, 9, 5, 8, 0, 0).getTime(), morning, "he-IL")).toBe("אתמול");
  });

  it("uses Hebrew words with the UI pinned to Hebrew", () => {
    expect(relativePast(ago(5 * MIN), NOW, "he-IL")).toBe("לפני 5 דק׳");
    expect(relativePast(ago(DAY), NOW, "he-IL")).toBe("אתמול");
    setFixedLocale("he");
    try {
      expect(relativePast(ago(5 * MIN), NOW)).toBe("לפני 5 דק׳");
    } finally {
      setFixedLocale(null);
    }
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

describe("regional format locale", () => {
  afterEach(() => setFormatLocale(null));

  it("drives the default date and weekday, independently of the UI language", () => {
    // Without a regional format the display language (navigator.language) is used.
    expect(shortDate(TUESDAY)).toBe(shortDate(TUESDAY, getLocaleTag()));
    setFormatLocale("he-IL");
    expect(shortDate(TUESDAY)).toBe("6.10");
    expect(weekdayLong(TUESDAY)).toBe("יום שלישי");
    expect(getLocale()).toBe(detectLocale(getLocaleTag())); // strings keep following the UI language
  });

  it("ignores Windows sort-order suffixes and tags Intl rejects", () => {
    setFormatLocale("en-GB_tradnl");
    expect(getFormatTag()).toBe("en-GB");
    setFormatLocale("not a tag!");
    expect(getFormatTag()).toBe(getLocaleTag());
    setFormatLocale(null);
    expect(getFormatTag()).toBe(getLocaleTag());
  });
});

describe("Hebrew UI on an English Windows", () => {
  afterEach(() => {
    setFixedLocale(null);
    setFormatLocale(null);
  });

  // 4-10 October 2026 is Sunday to Saturday.
  const week = Array.from({ length: 7 }, (_, i) => new Date(2026, 9, 4 + i, 12, 0));

  it("names every day in Hebrew: Sunday is יום ראשון ... Saturday is יום שבת", () => {
    setFixedLocale("he");
    setFormatLocale("en-US");
    expect(week.map((d) => weekdayLong(d))).toEqual(["יום ראשון", "יום שני", "יום שלישי", "יום רביעי", "יום חמישי", "יום שישי", "יום שבת"]);
    expect(weekdayShort(week[2])).toBe("יום ג׳");
  });

  it("keeps the Windows number format but words the rest in Hebrew", () => {
    setFixedLocale("he");
    setFormatLocale("en-US");
    const tuesday = week[2];
    expect(shortDate(tuesday)).toBe("10/6"); // US order, as Windows is set
    expect(fullDate(tuesday)).toBe("יום שלישי, 6 באוקטובר 2026");
    expect(relativeMinutes(25)).toMatch(/^בעוד 25/);
    expect(dayLabel(new Date(2026, 9, 7, 9, 0), tuesday.getTime())).toBe("מחר");
    expect(dayLabel(new Date(2026, 9, 8, 9, 0), tuesday.getTime())).toBe("יום חמישי");
    expect(t("tab.calendar")).toBe("יומן");
  });

  it("follows Windows again once unpinned", () => {
    setFormatLocale("en-US");
    expect(weekdayLong(week[0])).toBe("Sunday");
    expect(t("tab.calendar")).toBe("Calendar");
  });
});

describe("Hebrew 12-hour clock", () => {
  afterEach(() => {
    setFixedLocale(null);
    setFormatLocale(null);
  });

  it("says before/after noon in Hebrew when Windows uses a 12-hour clock", () => {
    setFixedLocale("he");
    setFormatLocale("en-US");
    expect(formatTime(new Date(2026, 9, 6, 9, 5))).toContain("לפנה״צ");
    expect(formatTime(new Date(2026, 9, 6, 23, 12))).toContain("אחה״צ");
    expect(formatTime(new Date(2026, 9, 6, 23, 12))).not.toMatch(/AM|PM/);
    expect(timeParts(new Date(2026, 9, 6, 23, 12)).period).toBe("אחה״צ");
  });

  it("keeps a 24-hour clock without any day period", () => {
    setFixedLocale("he");
    setFormatLocale("he-IL");
    expect(formatTime(new Date(2026, 9, 6, 23, 12))).toBe("23:12");
  });
});
