// Date/time formatting. Everything comes from Intl for the given locale — nothing is
// translated or reordered by hand. Formatters are created once per locale and cached.

import { getLocaleTag } from "./i18n";

interface Formatters {
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

function build(locale: string): Formatters {
  // Never pass hour12:false — some engines then render midnight as "24:00". Take the
  // locale's own cycle and only normalise h24 away.
  const resolved = (new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions() as { hourCycle?: string }).hourCycle;
  const hourCycle = (resolved === "h24" ? "h23" : resolved) as Intl.DateTimeFormatOptions["hourCycle"];
  return {
    fullNumeric: new Intl.DateTimeFormat(locale, { day: "numeric", month: "numeric", year: "numeric" }),
    weekdayLong: new Intl.DateTimeFormat(locale, { weekday: "long" }),
    weekdayShort: new Intl.DateTimeFormat(locale, { weekday: "short" }),
    // 24-hour locales get a zero-padded hour ("00:05"); 12-hour ones don't ("12:05 AM").
    time: new Intl.DateTimeFormat(locale, {
      hour: hourCycle === "h23" ? "2-digit" : "numeric",
      minute: "2-digit",
      ...(hourCycle ? { hourCycle } : {}),
    }),
    fullDate: new Intl.DateTimeFormat(locale, { weekday: "long", year: "numeric", month: "long", day: "numeric" }),
    dateTime: new Intl.DateTimeFormat(locale, { dateStyle: "short", timeStyle: "short", ...(hourCycle ? { hourCycle } : {}) }),
    relative: new Intl.RelativeTimeFormat(locale, { numeric: "auto", style: "short" }),
  };
}

function formatters(locale: string): Formatters {
  let f = cache.get(locale);
  if (!f) {
    f = build(locale);
    cache.set(locale, f);
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

export function shortDate(date: Date, locale: string = getLocaleTag()): string {
  return stripYear(formatters(locale).fullNumeric.formatToParts(date));
}

export function weekdayLong(date: Date, locale: string = getLocaleTag()): string {
  return formatters(locale).weekdayLong.format(date);
}

export function weekdayShort(date: Date, locale: string = getLocaleTag()): string {
  return formatters(locale).weekdayShort.format(date);
}

export function fullDate(date: Date, locale: string = getLocaleTag()): string {
  return formatters(locale).fullDate.format(date);
}

export function formatDateTime(date: Date, locale: string = getLocaleTag()): string {
  return formatters(locale).dateTime.format(date);
}

export function formatTime(date: Date, locale: string = getLocaleTag()): string {
  return formatters(locale).time.format(date);
}

/** Time split so a 12-hour day period ("AM") can be styled smaller than the digits. */
export function timeParts(date: Date, locale: string = getLocaleTag()): { digits: string; period: string | null } {
  const parts = formatters(locale).time.formatToParts(date);
  const period = parts.find((p) => p.type === "dayPeriod")?.value ?? null;
  if (!period) return { digits: parts.map((p) => p.value).join(""), period: null };
  const digits = parts
    .filter((p) => p.type !== "dayPeriod")
    .map((p) => p.value)
    .join("")
    .trim();
  return { digits, period };
}

/** "in 25 min." / "בעוד 25 דק׳" — whole minutes from now, rolled up to hours/days. */
export function relativeMinutes(minutes: number, locale: string = getLocaleTag()): string {
  const rtf = formatters(locale).relative;
  const rounded = Math.round(minutes);
  const abs = Math.abs(rounded);
  if (abs >= 24 * 60) return rtf.format(Math.round(rounded / (24 * 60)), "day");
  if (abs >= 60) return rtf.format(Math.round(rounded / 60), "hour");
  return rtf.format(rounded, "minute");
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
export function dayLabel(date: Date, nowMs: number, locale: string = getLocaleTag()): string | null {
  const days = Math.round((startOfDay(date.getTime()) - startOfDay(nowMs)) / 86_400_000);
  if (days <= 0) return null;
  const text = days === 1 ? formatters(locale).relative.format(1, "day") : weekdayLong(date, locale);
  return text.charAt(0).toLocaleUpperCase(locale) + text.slice(1);
}
