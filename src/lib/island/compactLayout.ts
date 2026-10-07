// Fits the collapsed island's labels into its width budget. What it shows follows the display
// setting: "full" is date, clock, weekday (physically left to right), "clock" the clock alone,
// "date" the date and the weekday (the look before the clock existed).

import type { IslandDisplay } from "../ipc";

export interface CompactLayoutInput {
  date: string;
  weekdayLong: string;
  weekdayShort: string;
  measure: (text: string) => number;
  /** Horizontal padding on each side of the pill. */
  paddingX: number;
  /** Minimum space between the date and the weekday ("date"). */
  gap: number;
  maxWidth: number;
  /** Which labels are shown. Default "date". */
  display?: IslandDisplay;
  /** Width of the clock's slot (a stable estimate, never the current time's own width). Default 0. */
  timeWidth?: number;
  /** Space between the date, the clock and the weekday ("full"). Default `gap`. */
  gapFull?: number;
}

export interface CompactLayout {
  /** The weekday to show; empty when the display has none. */
  weekday: string;
  /** Width the labels need, excluding padding (clamped so the pill never exceeds maxWidth). */
  contentWidth: number;
}

/** Prefers the long weekday, falls back to the short one, and finally lets CSS ellipsize. */
export function layoutCompact({
  date,
  weekdayLong,
  weekdayShort,
  measure,
  paddingX,
  gap,
  maxWidth,
  display = "date",
  timeWidth = 0,
  gapFull = gap,
}: CompactLayoutInput): CompactLayout {
  if (display === "clock") return { weekday: "", contentWidth: timeWidth };
  const dateWidth = measure(date);
  const budget = maxWidth - paddingX * 2;
  // "full" carries the clock between the two labels, with its own spacing.
  const fixed = display === "full" ? dateWidth + gapFull + timeWidth + gapFull : dateWidth + gap;
  for (const weekday of [weekdayLong, weekdayShort]) {
    const contentWidth = fixed + measure(weekday);
    if (contentWidth <= budget) return { weekday, contentWidth };
  }
  return { weekday: weekdayShort, contentWidth: budget };
}
