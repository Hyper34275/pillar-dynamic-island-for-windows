// Fits the collapsed island's two labels (date left, weekday right) into its width budget.

export interface CompactLayoutInput {
  date: string;
  weekdayLong: string;
  weekdayShort: string;
  measure: (text: string) => number;
  /** Horizontal padding on each side of the pill. */
  paddingX: number;
  /** Minimum space between the date and the weekday. */
  gap: number;
  maxWidth: number;
}

export interface CompactLayout {
  weekday: string;
  /** Width the two labels need, excluding padding (clamped so the pill never exceeds maxWidth). */
  contentWidth: number;
}

/** Prefers the long weekday, falls back to the short one, and finally lets CSS ellipsize. */
export function layoutCompact({ date, weekdayLong, weekdayShort, measure, paddingX, gap, maxWidth }: CompactLayoutInput): CompactLayout {
  const dateWidth = measure(date);
  const budget = maxWidth - paddingX * 2;
  for (const weekday of [weekdayLong, weekdayShort]) {
    const contentWidth = dateWidth + gap + measure(weekday);
    if (contentWidth <= budget) return { weekday, contentWidth };
  }
  return { weekday: weekdayShort, contentWidth: budget };
}
