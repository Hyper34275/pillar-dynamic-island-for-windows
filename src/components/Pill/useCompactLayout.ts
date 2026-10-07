import { useMemo } from "react";
import { useMinute } from "../../hooks/useClock";
import { useSettings } from "../../hooks/useSettings";
import { formatTime, fullDate, shortDate, timeParts, weekdayLong, weekdayShort } from "../../lib/dateFormat";
import { getFormatTag, getWordTag, t } from "../../lib/i18n";
import type { IslandDisplay } from "../../lib/ipc";
import { layoutCompact } from "../../lib/island/compactLayout";
import { measureText } from "../../lib/textMeasure";
import { badgeWidth, compactSize, pillDimensions, type IslandSize } from "./animations";
import { meetingStatusLabel, type MeetingStatus } from "../../lib/calendar/meetingStatus";

export const COMPACT_FONT_SIZE = 14;
const COMPACT_FONT_WEIGHT = 600;
/** The clock's digits: a small companion to the date in "full", the only label in "clock". */
export const CLOCK_FONT_SIZE = { full: 12, clock: 15 } as const;
/** The 12-hour day period ("PM") is smaller than the digits it follows. */
export const PERIOD_FONT_SIZE = 11;
/** Space between the clock's digits and the period. */
export const PERIOD_GAP = 3;
// The labels render tabular digits (all as wide as the widest), so every digit is measured as the
// widest one (`measureAt`). What is left is sub-pixel rounding between canvas and DOM text.
const MEASURE_SLACK_PX = 4;

export interface CompactLabels {
  date: string;
  /** Empty in the "clock" display. */
  weekday: string;
  /** Width the labels need, the clock's slot included (never the width of the current time). */
  contentWidth: number;
  display: IslandDisplay;
  /** Width reserved for the clock (0 in the "date" display). */
  timeWidth: number;
}

const cache = new Map<string, CompactLabels>();
let fontFamily: string | null = null;

/** The island's UI font stack, for canvas text measurement. */
export function uiFontFamily(): string {
  if (fontFamily === null) {
    fontFamily = typeof document === "undefined" ? "sans-serif" : getComputedStyle(document.documentElement).fontFamily || "sans-serif";
  }
  return fontFamily;
}

function fontOf(sizePx: number): string {
  return `${COMPACT_FONT_WEIGHT} ${sizePx}px ${uiFontFamily()}`;
}

/** Raw canvas width of a text at a size in the island's font (proportional digits as the font has them). */
function rawMeasureAt(sizePx: number): (text: string) => number {
  const font = fontOf(sizePx);
  return (text) => measureText(text, font, sizePx);
}

/** Every digit (any script) of a text replaced by `digit`: what a tabular-nums run is as wide as. */
export function withWidestDigits(text: string, digit: string): string {
  return text.replace(/\p{Nd}/gu, digit);
}

const tabularDigitCache = new Map<string, string>();

/**
 * The digit a tabular run is as wide as at a size: the widest glyph of the regional format's
 * digits (the date) and of the words locale's digits (the time), whichever is wider.
 */
function tabularDigitAt(sizePx: number): string {
  const format = getFormatTag();
  const words = getWordTag();
  const key = `${fontOf(sizePx)}|${format}|${words}`;
  let digit = tabularDigitCache.get(key);
  if (digit === undefined) {
    const raw = rawMeasureAt(sizePx);
    digit = widestDigit(raw, words);
    if (format !== words) {
      const other = widestDigit(raw, format);
      if (raw(other) > raw(digit)) digit = other;
    }
    if (tabularDigitCache.size > 16) tabularDigitCache.clear();
    tabularDigitCache.set(key, digit);
  }
  return digit;
}

/**
 * Measures text at a size in the island's font the way the DOM renders it: the labels use
 * tabular-nums, so each digit counts as the widest digit (a proportional "1" is a few px
 * narrower), plus a little slack for sub-pixel rounding.
 */
function measureAt(sizePx: number): (text: string) => number {
  const raw = rawMeasureAt(sizePx);
  const digit = tabularDigitAt(sizePx);
  return (text) => raw(withWidestDigits(text, digit)) + MEASURE_SLACK_PX;
}

// -----------------------------------------------------------------------------
// The clock's slot. Its width must never depend on the time shown, or the island would jitter
// every minute: it is measured on a template with the widest digit in every position.
// -----------------------------------------------------------------------------

/** The widest of the digits 0-9 in the words locale (the glyphs `formatTime` prints). */
export function widestDigit(measure: (text: string) => number, locale: string): string {
  const numbers = new Intl.NumberFormat(locale, { useGrouping: false });
  let widest = "0";
  let widestWidth = -1;
  for (let n = 0; n <= 9; n++) {
    const glyph = numbers.format(n);
    const width = measure(glyph);
    if (width > widestWidth) {
      widest = glyph;
      widestWidth = width;
    }
  }
  return widest;
}

export interface TimeSlotInput {
  display: "full" | "clock";
  /** Width of a text at a font size (px), in the island's font. */
  measure: (text: string, sizePx: number) => number;
  /** The widest digit glyph of the words locale. */
  digit: string;
  /** The time as the app prints it (`timeParts`); a non-null period means a 12-hour clock. */
  parts: (date: Date) => { digits: string; period: string | null };
}

/**
 * The width the clock needs whatever minute it is. Two hour digits are always reserved (so 9:59
 * to 10:00, or 12:59 to 1:00 in a 12-hour region, never resizes the island) and the period
 * takes the longer of its AM and PM forms.
 */
export function timeSlotWidth({ display, measure, digit, parts }: TimeSlotInput): number {
  const size = CLOCK_FONT_SIZE[display];
  // 10:00 has two hour digits in every cycle; each digit is then swapped for the widest one.
  const reference = parts(new Date(2000, 0, 1, 10, 0));
  const template = reference.digits.replace(/\p{Nd}/gu, digit);
  let width = measure(template, size);
  if (reference.period !== null) {
    const am = parts(new Date(2000, 0, 1, 8, 0)).period ?? "";
    const pm = parts(new Date(2000, 0, 1, 20, 0)).period ?? "";
    width += PERIOD_GAP + Math.max(measure(am, PERIOD_FONT_SIZE), measure(pm, PERIOD_FONT_SIZE));
  }
  return width;
}

const digitCache = new Map<string, string>();

function measuredTimeSlot(display: "full" | "clock"): number {
  const size = CLOCK_FONT_SIZE[display];
  const words = getWordTag();
  const key = `${fontOf(size)}|${words}`;
  let digit = digitCache.get(key);
  if (digit === undefined) {
    digit = widestDigit(rawMeasureAt(size), words);
    if (digitCache.size > 16) digitCache.clear();
    digitCache.set(key, digit);
  }
  return timeSlotWidth({ display, measure: (text, sizePx) => measureAt(sizePx)(text), digit, parts: (date) => timeParts(date) });
}

/**
 * The collapsed island's labels and the width they need. Measured once per locale, display and
 * day (the text only changes at midnight) and cached. `display` defaults to the setting; the
 * boot dot (before the settings arrive) uses the default "full".
 */
export function useCompactLabels(today: Date, displayOverride?: IslandDisplay): CompactLabels {
  const { settings, loaded } = useSettings();
  const display = displayOverride ?? (loaded ? settings.islandDisplay : "full");
  const dayKey = today.getTime();
  return useMemo(() => {
    // Numbers follow the regional format, the weekday the (pinned) UI language.
    const locale = `${getFormatTag()}|${getWordTag()}`;
    const key = `${locale}|${display}|${dayKey}`;
    const hit = cache.get(key);
    if (hit) return hit;

    const date = new Date(dayKey);
    const dateText = shortDate(date);
    const timeWidth = display === "date" ? 0 : measuredTimeSlot(display);
    const { weekday, contentWidth } = layoutCompact({
      date: dateText,
      weekdayLong: weekdayLong(date),
      weekdayShort: weekdayShort(date),
      measure: measureAt(COMPACT_FONT_SIZE),
      paddingX: pillDimensions.compact.paddingX,
      gap: pillDimensions.compact.gap,
      gapFull: pillDimensions.compact.gapFull,
      maxWidth: pillDimensions.compact.maxWidth,
      display,
      timeWidth,
    });
    const labels: CompactLabels = { date: dateText, weekday, contentWidth, display, timeWidth };
    if (cache.size > 16) cache.clear();
    cache.set(key, labels);
    return labels;
  }, [dayKey, display]);
}

// -----------------------------------------------------------------------------
// The collapsed island's whole content: the date and clock labels, or a meeting status, plus the
// unseen indicator. One size function serves PillShell's target and the layer, so they always agree.
// -----------------------------------------------------------------------------

export const STATUS_FONT_SIZE = 13;
const STATUS_DOT = 8;
const STATUS_GAP = 8;
const SILENT_ICON = 14;

export interface CompactContent {
  labels: CompactLabels;
  /** Shown instead of the date when a meeting is about to start or running. */
  status: MeetingStatus | null;
  statusText: string | null;
  unseen: number;
  silent: boolean;
  size: IslandSize;
  /** The time to show, from the clock store's minute (the digits and, in a 12-hour region, the period). */
  time: { digits: string; period: string | null };
  /** What a screen reader hears for the collapsed island: the full date and time, or the meeting status. */
  ariaLabel: string;
}

export function compactContentSize(labels: CompactLabels, statusTextWidth: number | null, unseen: number, silent: boolean): IslandSize {
  const badge = badgeWidth(unseen);
  if (statusTextWidth !== null) {
    const content = STATUS_DOT + STATUS_GAP + statusTextWidth + (silent ? STATUS_GAP + SILENT_ICON : 0) + badge;
    return compactSize(content, pillDimensions.compactMeeting.maxWidth);
  }
  const minWidth = labels.display === "clock" ? pillDimensions.compact.clockMinWidth : pillDimensions.compact.minWidth;
  return compactSize(labels.contentWidth + badge, pillDimensions.compact.maxWidth + badge, minWidth);
}

export function useCompactContent(labels: CompactLabels, status: MeetingStatus | null, unseen: number, silent: boolean): CompactContent {
  // The same minute the shell follows: one clock store, one timer, whoever subscribes.
  const minute = useMinute().getTime();
  const statusText = status ? meetingStatusLabel(status) : null;
  const size = useMemo(() => {
    const width = statusText === null ? null : measureAt(STATUS_FONT_SIZE)(statusText);
    return compactContentSize(labels, width, unseen, silent);
  }, [labels, statusText, unseen, silent]);
  return useMemo(() => {
    const now = new Date(minute);
    const open = t("island.open");
    const base = statusText ? `${statusText}. ${open}` : `${fullDate(now)}, ${formatTime(now)}. ${open}`;
    const ariaLabel = unseen > 0 ? `${base}. ${t("notif.unread", { n: unseen })}` : base;
    return { labels, status, statusText, unseen, silent, size, time: timeParts(now), ariaLabel };
  }, [labels, status, statusText, unseen, silent, size, minute]);
}
