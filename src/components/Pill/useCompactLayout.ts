import { useMemo } from "react";
import { shortDate, weekdayLong, weekdayShort } from "../../lib/dateFormat";
import { getFormatTag, getWordTag } from "../../lib/i18n";
import { layoutCompact } from "../../lib/island/compactLayout";
import { measureText } from "../../lib/textMeasure";
import { badgeWidth, compactSize, pillDimensions, type IslandSize } from "./animations";
import { meetingStatusLabel, type MeetingStatus } from "../../lib/calendar/meetingStatus";

export const COMPACT_FONT_SIZE = 14;
const COMPACT_FONT_WEIGHT = 600;
// Canvas measures proportional digits, the labels render tabular ones, so the DOM text can
// come out a pixel or two wider. Without slack the weekday would be ellipsized for it.
const MEASURE_SLACK_PX = 2;

export interface CompactLabels {
  date: string;
  weekday: string;
  contentWidth: number;
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

function resolveFont(): string {
  return `${COMPACT_FONT_WEIGHT} ${COMPACT_FONT_SIZE}px ${uiFontFamily()}`;
}

/**
 * The collapsed island's labels and the width they need. Measured once per locale and
 * day (the text only changes at midnight) and cached.
 */
export function useCompactLabels(today: Date): CompactLabels {
  const dayKey = today.getTime();
  return useMemo(() => {
    // Numbers follow the regional format, the weekday the (pinned) UI language.
    const locale = `${getFormatTag()}|${getWordTag()}`;
    const key = `${locale}|${dayKey}`;
    const hit = cache.get(key);
    if (hit) return hit;

    const date = new Date(dayKey);
    const font = resolveFont();
    const dateText = shortDate(date);
    const { weekday, contentWidth } = layoutCompact({
      date: dateText,
      weekdayLong: weekdayLong(date),
      weekdayShort: weekdayShort(date),
      measure: (text) => measureText(text, font, COMPACT_FONT_SIZE) + MEASURE_SLACK_PX,
      paddingX: pillDimensions.compact.paddingX,
      gap: pillDimensions.compact.gap,
      maxWidth: pillDimensions.compact.maxWidth,
    });
    const labels = { date: dateText, weekday, contentWidth };
    if (cache.size > 16) cache.clear();
    cache.set(key, labels);
    return labels;
  }, [dayKey]);
}

// -----------------------------------------------------------------------------
// The collapsed island's whole content: the date labels, or a meeting status, plus the unseen
// count. One size function serves PillShell's target and the layer, so they always agree.
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
}

export function compactContentSize(labels: CompactLabels, statusTextWidth: number | null, unseen: number, silent: boolean): IslandSize {
  const badge = badgeWidth(unseen);
  if (statusTextWidth !== null) {
    const content = STATUS_DOT + STATUS_GAP + statusTextWidth + (silent ? STATUS_GAP + SILENT_ICON : 0) + badge;
    return compactSize(content, pillDimensions.compactMeeting.maxWidth);
  }
  return compactSize(labels.contentWidth + badge, pillDimensions.compact.maxWidth + badge);
}

export function useCompactContent(labels: CompactLabels, status: MeetingStatus | null, unseen: number, silent: boolean): CompactContent {
  const statusText = status ? meetingStatusLabel(status) : null;
  return useMemo(() => {
    const width = statusText === null ? null : measureText(statusText, `${COMPACT_FONT_WEIGHT} ${STATUS_FONT_SIZE}px ${uiFontFamily()}`, STATUS_FONT_SIZE) + MEASURE_SLACK_PX;
    return { labels, status, statusText, unseen, silent, size: compactContentSize(labels, width, unseen, silent) };
  }, [labels, status, statusText, unseen, silent]);
}
