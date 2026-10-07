// The collapsed island's content for the tour: the same layout and size functions PillShell uses
// (layoutCompact, timeSlotWidth, compactContentSize), fed a fixed moment instead of the clock
// store and the settings (useCompactLabels would read the settings, which means the backend).

import {
  COMPACT_FONT_SIZE,
  CLOCK_FONT_SIZE,
  STATUS_FONT_SIZE,
  compactContentSize,
  timeSlotWidth,
  uiFontFamily,
  widestDigit,
  withWidestDigits,
  type CompactContent,
  type CompactLabels,
} from "../components/Pill/useCompactLayout";
import { pillDimensions } from "../components/Pill/animations";
import { meetingStatusLabel, type MeetingStatus } from "../lib/calendar/meetingStatus";
import { shortDate, timeParts, weekdayLong, weekdayShort } from "../lib/dateFormat";
import { getWordTag, t } from "../lib/i18n";
import { layoutCompact } from "../lib/island/compactLayout";
import { measureText } from "../lib/textMeasure";

// The same slack the island uses (useCompactLayout.ts).
const MEASURE_SLACK_PX = 4;

// Like the island's own measuring (useCompactLayout): the labels render tabular digits, so every
// digit is measured as the widest one.
function measureAt(sizePx: number): (text: string) => number {
  const font = `600 ${sizePx}px ${uiFontFamily()}`;
  const raw = (text: string) => measureText(text, font, sizePx);
  const digit = widestDigit(raw, getWordTag());
  return (text) => raw(withWidestDigits(text, digit)) + MEASURE_SLACK_PX;
}

interface TourCompactInput {
  nowMs: number;
  /** Unseen notifications (the indicator). */
  unseen?: number;
  status?: MeetingStatus | null;
  silent?: boolean;
}

/** What the collapsed island shows at `nowMs` in the "full" display (date, clock, weekday). */
export function tourCompactContent({ nowMs, unseen = 0, status = null, silent = false }: TourCompactInput): CompactContent {
  const now = new Date(nowMs);
  const dateText = shortDate(now);
  const timeWidth = timeSlotWidth({
    display: "full",
    measure: (text, sizePx) => measureAt(sizePx)(text),
    digit: widestDigit(measureAt(CLOCK_FONT_SIZE.full), getWordTag()),
    parts: (date) => timeParts(date),
  });
  const { weekday, contentWidth } = layoutCompact({
    date: dateText,
    weekdayLong: weekdayLong(now),
    weekdayShort: weekdayShort(now),
    measure: measureAt(COMPACT_FONT_SIZE),
    paddingX: pillDimensions.compact.paddingX,
    gap: pillDimensions.compact.gap,
    gapFull: pillDimensions.compact.gapFull,
    maxWidth: pillDimensions.compact.maxWidth,
    display: "full",
    timeWidth,
  });
  const labels: CompactLabels = { date: dateText, weekday, contentWidth, display: "full", timeWidth };
  const statusText = status ? meetingStatusLabel(status) : null;
  const size = compactContentSize(labels, statusText === null ? null : measureAt(STATUS_FONT_SIZE)(statusText), unseen, silent);
  return { labels, status, statusText, unseen, silent, size, time: timeParts(now), ariaLabel: statusText ?? t("island.open") };
}
