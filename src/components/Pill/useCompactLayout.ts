import { useMemo, useSyncExternalStore } from "react";
import { useMinute } from "../../hooks/useClock";
import { useSettings } from "../../hooks/useSettings";
import { formatTime, fullDate, shortDate, timeParts, weekdayLong, weekdayShort } from "../../lib/dateFormat";
import { getFormatTag, getWordTag, t } from "../../lib/i18n";
import type { IslandDisplay } from "../../lib/ipc";
import { layoutCompact } from "../../lib/island/compactLayout";
import { measureText } from "../../lib/textMeasure";
import { badge as badgeTokens, compact as compactTokens, icon as iconTokens, progress as progressTokens, type as typeRoles } from "../../design/tokens";
import { compactSize, pillDimensions, type IslandSize } from "./animations";
import { countBadgeWidth } from "./ui/identity";
import { meetingStatusLabel, type MeetingStatus } from "../../lib/calendar/meetingStatus";

/** Date, weekday (secondary) and meeting status: the label role (14/600). */
export const COMPACT_FONT_SIZE = typeRoles.label.size;
/** The clock is the primary label of the compact island: the headline role (15/600), in every display. */
export const CLOCK_FONT_SIZE = typeRoles.headline.size;
/**
 * The 12-hour day period ("PM") is the meta role's size (12): the smallest text the compact island
 * shows. It is rendered at the label's weight (600) so Hebrew, which has no 500, matches Latin.
 */
export const PERIOD_FONT_SIZE = typeRoles.meta.size;
/** Digits and their period are a tight pair. */
export const PERIOD_GAP = compactTokens.gapTight;
const COMPACT_FONT_WEIGHT = typeRoles.label.weight;
// The labels render tabular digits (all as wide as the widest), so every digit is measured as the
// widest one (`measureAt`). What is left is sub-pixel rounding between canvas and DOM text.
// The layout spreads the slack into the gaps (justify-between), so it is never visible.
const MEASURE_SLACK_PX = 2;

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

// -----------------------------------------------------------------------------
// Fonts. The measuring below runs on a canvas with the same stack the DOM renders (uiFontFamily),
// which starts with "CI Hebrew" (design/fonts.css): local() faces that the browser loads lazily.
// A canvas measures with a face that is not loaded yet as if it were the fallback, so the faces
// are loaded first (preloadUiFonts, started when this module loads) and, when they arrive, every
// measured size is dropped and recomputed (fontsVersion, a dependency of the hooks below).
// -----------------------------------------------------------------------------

let fontsVersion = 0;
const fontListeners = new Set<() => void>();
const subscribeFonts = (listener: () => void) => {
  fontListeners.add(listener);
  return () => void fontListeners.delete(listener);
};
const readFontsVersion = () => fontsVersion;

function fontsChanged(): void {
  cache.clear();
  digitCache.clear();
  tabularDigitCache.clear();
  fontFamily = null;
  fontsVersion++;
  fontListeners.forEach((listener) => listener());
}

/** The faces of the Hebrew family at the three weights it maps, one Hebrew glyph each. */
const HEBREW_SAMPLE = "א";
const FONT_WEIGHTS = [400, 600, 700] as const;

/** Loads the Hebrew faces (a no-op without document.fonts) and re-measures once they are there. */
export function preloadUiFonts(): Promise<void> {
  const fonts = typeof document === "undefined" ? undefined : document.fonts;
  if (!fonts?.load) return Promise.resolve();
  return Promise.all(FONT_WEIGHTS.map((weight) => fonts.load(`${weight} 14px "CI Hebrew"`, HEBREW_SAMPLE)))
    .then(() => fonts.ready)
    .then(fontsChanged, () => undefined);
}
void preloadUiFonts();

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
export function timeSlotWidth({ measure, digit, parts }: TimeSlotInput): number {
  const size = CLOCK_FONT_SIZE;
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

function measuredTimeSlot(): number {
  const size = CLOCK_FONT_SIZE;
  const words = getWordTag();
  const key = `${fontOf(size)}|${words}`;
  let digit = digitCache.get(key);
  if (digit === undefined) {
    digit = widestDigit(rawMeasureAt(size), words);
    if (digitCache.size > 16) digitCache.clear();
    digitCache.set(key, digit);
  }
  return timeSlotWidth({ measure: (text, sizePx) => measureAt(sizePx)(text), digit, parts: (date) => timeParts(date) });
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
  const fonts = useSyncExternalStore(subscribeFonts, readFontsVersion, readFontsVersion);
  return useMemo(() => {
    // Numbers follow the regional format, the weekday the (pinned) UI language.
    const locale = `${getFormatTag()}|${getWordTag()}`;
    const key = `${locale}|${display}|${dayKey}`;
    const hit = cache.get(key);
    if (hit) return hit;

    const date = new Date(dayKey);
    const dateText = shortDate(date);
    const timeWidth = display === "date" ? 0 : measuredTimeSlot();
    const { weekday, contentWidth } = layoutCompact({
      date: dateText,
      weekdayLong: weekdayLong(date),
      weekdayShort: weekdayShort(date),
      measure: measureAt(COMPACT_FONT_SIZE),
      paddingX: compactTokens.paddingX,
      gap: compactTokens.gap,
      gapFull: compactTokens.gap,
      maxWidth: compactTokens.maxWidth,
      display,
      timeWidth,
    });
    const labels: CompactLabels = { date: dateText, weekday, contentWidth, display, timeWidth };
    if (cache.size > 16) cache.clear();
    cache.set(key, labels);
    return labels;
  }, [dayKey, display, fonts]);
}

// -----------------------------------------------------------------------------
// The collapsed island's whole content: the date and clock labels, or a meeting status, plus the
// unseen indicator. One size function serves PillShell's target and the layer, so they always agree.
// -----------------------------------------------------------------------------

export const STATUS_FONT_SIZE = typeRoles.label.size;

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

/**
 * What a badge adds beyond the symmetric padding. Without a badge the island is padding + content +
 * padding; with one it is paddingX + content + gap + badge + endInset (the badge is concentric with
 * the end cap, so it sits closer to the end than text does). `compactSize` adds paddingX twice, so
 * it is given the content plus this.
 */
export function badgeExtra(unseen: number): number {
  const badge = countBadgeWidth(unseen);
  return badge === 0 ? 0 : compactTokens.gap + badge + badgeTokens.endInset - compactTokens.paddingX;
}

export function compactContentSize(labels: CompactLabels, statusTextWidth: number | null, unseen: number, silent: boolean, progressRing = false): IslandSize {
  const extra = badgeExtra(unseen);
  if (statusTextWidth !== null) {
    const content = (progressRing ? progressTokens.ring : compactTokens.statusDot) + compactTokens.gap + statusTextWidth + (silent ? compactTokens.gap + iconTokens.small : 0);
    // A long meeting name ellipsizes: the island never goes past its maximum, badge or not.
    return compactSize(content + extra, pillDimensions.compactMeeting.maxWidth);
  }
  const minWidth = labels.display === "clock" ? pillDimensions.compact.clockMinWidth : pillDimensions.compact.minWidth;
  return compactSize(labels.contentWidth + extra, pillDimensions.compact.maxWidth + extra, minWidth);
}

export function useCompactContent(labels: CompactLabels, status: MeetingStatus | null, unseen: number, silent: boolean): CompactContent {
  // The same minute the shell follows: one clock store, one timer, whoever subscribes.
  const minute = useMinute().getTime();
  const statusText = status ? meetingStatusLabel(status) : null;
  const fonts = useSyncExternalStore(subscribeFonts, readFontsVersion, readFontsVersion);
  const size = useMemo(() => {
    const width = statusText === null ? null : measureAt(STATUS_FONT_SIZE)(statusText);
    return compactContentSize(labels, width, unseen, silent, status?.kind === "now");
  }, [labels, status?.kind, statusText, unseen, silent, fonts]);
  return useMemo(() => {
    const now = new Date(minute);
    const open = t("island.open");
    const base = statusText ? `${statusText}. ${open}` : `${fullDate(now)}, ${formatTime(now)}. ${open}`;
    const ariaLabel = unseen > 0 ? `${base}. ${t("notif.unread", { n: unseen })}` : base;
    return { labels, status, statusText, unseen, silent, size, time: timeParts(now), ariaLabel };
  }, [labels, status, statusText, unseen, silent, size, minute]);
}
