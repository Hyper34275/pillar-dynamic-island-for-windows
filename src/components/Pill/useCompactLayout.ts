import { useMemo } from "react";
import { shortDate, weekdayLong, weekdayShort } from "../../lib/dateFormat";
import { getLocaleTag } from "../../lib/i18n";
import { layoutCompact } from "../../lib/island/compactLayout";
import { measureText } from "../../lib/textMeasure";
import { pillDimensions } from "./animations";

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
    const locale = getLocaleTag();
    const key = `${locale}|${dayKey}`;
    const hit = cache.get(key);
    if (hit) return hit;

    const date = new Date(dayKey);
    const font = resolveFont();
    const dateText = shortDate(date, locale);
    const { weekday, contentWidth } = layoutCompact({
      date: dateText,
      weekdayLong: weekdayLong(date, locale),
      weekdayShort: weekdayShort(date, locale),
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
