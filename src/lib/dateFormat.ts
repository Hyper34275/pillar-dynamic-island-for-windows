// Date/time formatting. Formatters are created once per locale pair and cached.
//
// Two locales are involved (see i18n.ts): the regional *format* (number order, separators,
// 12/24-hour clock) and the locale of the *words* (weekday and month names, "tomorrow",
// "in 5 min", AM/PM). They are the same unless the UI language is pinned, as the app pins
// Hebrew: an English Windows then still gets "יום שלישי", from the table below.
// A function given an explicit `locale` uses it for both.

import { getFormatTag, getWordTag } from "./i18n";

/**
 * Hebrew day names by JS day number, whatever language Windows (or Intl) speaks:
 * Sunday -> יום ראשון ... Saturday -> יום שבת.
 */
export const HEBREW_WEEKDAYS = [
  "יום ראשון", // Sunday
  "יום שני", // Monday
  "יום שלישי", // Tuesday
  "יום רביעי", // Wednesday
  "יום חמישי", // Thursday
  "יום שישי", // Friday
  "יום שבת", // Saturday
] as const;

/** The short forms, for when the long name does not fit the collapsed island. */
export const HEBREW_WEEKDAYS_SHORT = ["יום א׳", "יום ב׳", "יום ג׳", "יום ד׳", "יום ה׳", "יום ו׳", "שבת"] as const;

export const HEBREW_MONTHS = ["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר"] as const;

function isHebrew(locale: string): boolean {
  const lang = locale.toLowerCase().split(/[-_]/)[0];
  return lang === "he" || lang === "iw";
}

interface Formatters {
  /** Words come out in Hebrew from the tables above, not from Intl. */
  hebrew: boolean;
  /** numeric day + month + year, used only to derive the year-less short date */
  fullNumeric: Intl.DateTimeFormat;
  weekdayLong: Intl.DateTimeFormat;
  weekdayShort: Intl.DateTimeFormat;
  time: Intl.DateTimeFormat;
  fullDate: Intl.DateTimeFormat;
  dateTime: Intl.DateTimeFormat;
  relative: Intl.RelativeTimeFormat;
}

const cache = new Map<string, Formatters>();

function build(format: string, words: string): Formatters {
  // Never pass hour12:false — some engines then render midnight as "24:00". Take the
  // regional format's own cycle and only normalise h24 away.
  const resolved = (new Intl.DateTimeFormat(format, { hour: "numeric" }).resolvedOptions() as { hourCycle?: string }).hourCycle;
  const hourCycle = (resolved === "h24" ? "h23" : resolved) as Intl.DateTimeFormatOptions["hourCycle"];
  return {
    hebrew: isHebrew(words),
    fullNumeric: new Intl.DateTimeFormat(format, { day: "numeric", month: "numeric", year: "numeric" }),
    weekdayLong: new Intl.DateTimeFormat(words, { weekday: "long" }),
    weekdayShort: new Intl.DateTimeFormat(words, { weekday: "short" }),
    // 24-hour clocks get a zero-padded hour ("00:05"); 12-hour ones don't ("12:05 AM").
    time: new Intl.DateTimeFormat(words, {
      hour: hourCycle === "h23" ? "2-digit" : "numeric",
      minute: "2-digit",
      ...(hourCycle ? { hourCycle } : {}),
    }),
    fullDate: new Intl.DateTimeFormat(words, { weekday: "long", year: "numeric", month: "long", day: "numeric" }),
    dateTime: new Intl.DateTimeFormat(format, { dateStyle: "short", timeStyle: "short", ...(hourCycle ? { hourCycle } : {}) }),
    relative: new Intl.RelativeTimeFormat(words, { numeric: "auto", style: "short" }),
  };
}

function formatters(locale?: string): Formatters {
  const format = locale ?? getFormatTag();
  const words = locale ?? getWordTag();
  const key = `${format}|${words}`;
  let f = cache.get(key);
  if (!f) {
    f = build(format, words);
    cache.set(key, f);
  }
  return f;
}

type Part = Intl.DateTimeFormatPart;

/**
 * Drops the year (and the separator that belonged to it) from a formatted numeric date
 * while keeping the locale's own order and separators: 10/6/2026 -> 10/6,
 * 6.10.2026 -> 6.10, 2026/10/6 -> 10/6, 2026. 10. 06. -> 10. 06.
 */
export function stripYear(parts: Part[]): string {
  const yearAt = parts.findIndex((p) => p.type === "year");
  if (yearAt < 0) return parts.map((p) => p.value).join("");

  const kept = parts.slice();
  const yearFirst = parts.slice(0, yearAt).every((p) => p.type === "literal");
  const yearLast = parts.slice(yearAt + 1).every((p) => p.type === "literal");
  if (yearFirst) {
    // year, literal... -> drop the year and the literals that followed it
    let end = yearAt + 1;
    while (end < kept.length && kept[end].type === "literal") end++;
    kept.splice(0, end);
  } else if (yearLast) {
    // ..., literal, year, suffix -> drop the literals before it and any suffix
    let start = yearAt;
    while (start > 0 && kept[start - 1].type === "literal") start--;
    kept.splice(start);
  } else {
    // year in the middle: drop it and the separator that followed
    kept.splice(yearAt, kept[yearAt + 1]?.type === "literal" ? 2 : 1);
  }
  return kept.map((p) => p.value).join("");
}

export function shortDate(date: Date, locale?: string): string {
  return stripYear(formatters(locale).fullNumeric.formatToParts(date));
}

export function weekdayLong(date: Date, locale?: string): string {
  const f = formatters(locale);
  return f.hebrew ? HEBREW_WEEKDAYS[date.getDay()] : f.weekdayLong.format(date);
}

export function weekdayShort(date: Date, locale?: string): string {
  const f = formatters(locale);
  return f.hebrew ? HEBREW_WEEKDAYS_SHORT[date.getDay()] : f.weekdayShort.format(date);
}

const HEBREW_WEEKDAY_LETTERS = ["א׳", "ב׳", "ג׳", "ד׳", "ה׳", "ו׳", "ש׳"] as const;

/** One letter or so per day, for a week strip: "א׳" … "ש׳" / "S" … "S". */
export function weekdayLetter(date: Date, locale?: string): string {
  const f = formatters(locale);
  if (f.hebrew) return HEBREW_WEEKDAY_LETTERS[date.getDay()];
  return new Intl.DateTimeFormat(locale ?? getWordTag(), { weekday: "narrow" }).format(date);
}

/** "Tuesday, October 6, 2026" / "יום שלישי, 6 באוקטובר 2026". */
export function fullDate(date: Date, locale?: string): string {
  const f = formatters(locale);
  if (f.hebrew) return `${HEBREW_WEEKDAYS[date.getDay()]}, ${date.getDate()} ב${HEBREW_MONTHS[date.getMonth()]} ${date.getFullYear()}`;
  return f.fullDate.format(date);
}

export function formatDateTime(date: Date, locale?: string): string {
  return formatters(locale).dateTime.format(date);
}

/** ICU's Hebrew 12-hour clock says "AM"/"PM"; a Hebrew UI says it in Hebrew. */
const HEBREW_DAY_PERIODS = ["לפנה״צ", "אחה״צ"] as const;

function timeFormatParts(date: Date, locale?: string): Part[] {
  const f = formatters(locale);
  const parts = f.time.formatToParts(date);
  if (!f.hebrew) return parts;
  return parts.map((p) => (p.type === "dayPeriod" ? { ...p, value: HEBREW_DAY_PERIODS[date.getHours() < 12 ? 0 : 1] } : p));
}

export function formatTime(date: Date, locale?: string): string {
  return timeFormatParts(date, locale)
    .map((p) => p.value)
    .join("");
}

/** Time split so a 12-hour day period ("AM") can be styled smaller than the digits. */
export function timeParts(date: Date, locale?: string): { digits: string; period: string | null } {
  const parts = timeFormatParts(date, locale);
  const period = parts.find((p) => p.type === "dayPeriod")?.value ?? null;
  if (!period) return { digits: parts.map((p) => p.value).join(""), period: null };
  const digits = parts
    .filter((p) => p.type !== "dayPeriod")
    .map((p) => p.value)
    .join("")
    .trim();
  return { digits, period };
}

type RelativeUnit = "minute" | "hour" | "day" | "week" | "month" | "year";

/**
 * Hebrew amounts by unit: 1 and 2 have their own words (the dual), the rest "N" plus the short
 * plural. ICU's own Hebrew output appends the count, "לפני שעתיים (2)", so it is not used.
 */
const HEBREW_UNITS: Record<RelativeUnit, { one: string; two: string | null; many: string }> = {
  minute: { one: "דקה", two: null, many: "דק׳" },
  hour: { one: "שעה", two: "שעתיים", many: "שע׳" },
  day: { one: "יום", two: null, many: "ימים" },
  week: { one: "שבוע", two: "שבועיים", many: "שב׳" },
  month: { one: "חודש", two: "חודשיים", many: "חודשים" },
  year: { one: "שנה", two: "שנתיים", many: "שנים" },
};

/** A signed, non-zero count of a unit in Hebrew: "לפני דקה", "בעוד שעתיים", "אתמול", "לפני 3 שע׳". */
function hebrewRelative(count: number, unit: RelativeUnit): string {
  const abs = Math.abs(count);
  const future = count > 0;
  if (unit === "day" && abs <= 2) return abs === 1 ? (future ? "מחר" : "אתמול") : future ? "מחרתיים" : "שלשום";
  const u = HEBREW_UNITS[unit];
  const amount = abs === 1 ? u.one : abs === 2 && u.two ? u.two : `${abs} ${u.many}`;
  return `${future ? "בעוד" : "לפני"} ${amount}`;
}

/** A signed count of a unit in the words locale; Hebrew from the table, the rest from Intl. */
function relativeText(f: Formatters, count: number, unit: RelativeUnit): string {
  return f.hebrew && count !== 0 ? hebrewRelative(count, unit) : f.relative.format(count, unit);
}

/** "in 25 min." / "בעוד 25 דק׳" — whole minutes from now, rolled up to hours/days. */
export function relativeMinutes(minutes: number, locale?: string): string {
  const f = formatters(locale);
  const rounded = Math.round(minutes);
  const abs = Math.abs(rounded);
  if (abs >= 24 * 60) return relativeText(f, Math.round(rounded / (24 * 60)), "day");
  if (abs >= 60) return relativeText(f, Math.round(rounded / 60), "hour");
  return relativeText(f, rounded, "minute");
}

/**
 * How long ago `thenMs` was, as one coarse unit: "now", "5 min. ago", "3 hr. ago", "yesterday",
 * "2 days ago", "3 wk. ago", "last month", "last year" / "עכשיו", "לפני 5 דק׳", "לפני שעתיים",
 * "אתמול". Minutes and hours count elapsed time; from a day on they count calendar days (a note
 * from 47 hours ago is "the day before yesterday" when that was two midnights ago, not "yesterday").
 * Hebrew comes from a table (see HEBREW_UNITS), other languages from Intl in the words locale. A
 * time in the future counts as now.
 */
export function relativePast(thenMs: number, nowMs: number, locale?: string): string {
  const f = formatters(locale);
  const minutes = Math.floor(Math.max(0, nowMs - thenMs) / 60_000);
  if (minutes < 1) return f.relative.format(0, "second");
  if (minutes < 60) return relativeText(f, -minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return relativeText(f, -hours, "hour");
  const days = Math.max(1, Math.round((startOfDay(nowMs) - startOfDay(thenMs)) / 86_400_000));
  if (days < 7) return relativeText(f, -days, "day");
  if (days < 30) return relativeText(f, -Math.floor(days / 7), "week");
  if (days < 365) return relativeText(f, -Math.floor(days / 30), "month");
  return relativeText(f, -Math.floor(days / 365), "year");
}

/** Milliseconds until the local wall clock next shows a new minute (always > 0). */
export function msUntilNextMinute(nowMs: number = Date.now()): number {
  const d = new Date(nowMs);
  return 60_000 - (d.getSeconds() * 1000 + d.getMilliseconds());
}

/** Start of the local day containing `nowMs`. */
export function startOfDay(nowMs: number = Date.now()): number {
  const d = new Date(nowMs);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Milliseconds until the next local midnight (DST-safe: built from calendar fields). */
export function msUntilNextDay(nowMs: number = Date.now()): number {
  const d = new Date(nowMs);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime() - nowMs;
}

/**
 * Which day a time falls on, relative to `nowMs`: null for today (or earlier), "Tomorrow"
 * (Intl.RelativeTimeFormat) for the next day, the weekday name beyond that.
 */
export function dayLabel(date: Date, nowMs: number, locale?: string): string | null {
  const days = Math.round((startOfDay(date.getTime()) - startOfDay(nowMs)) / 86_400_000);
  if (days <= 0) return null;
  const text = days === 1 ? formatters(locale).relative.format(1, "day") : weekdayLong(date, locale);
  return text.charAt(0).toLocaleUpperCase(locale ?? getWordTag()) + text.slice(1);
}
