import { uiDirection } from "../../../design/direction";
import { color, icon } from "../../../design/tokens";
import { startOfDay, weekdayLetter } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { RoundButton } from "../ui/controls";
import { ChevronLeftIcon, ChevronRightIcon } from "../ui/icons";

/** How far back and ahead the strip goes. */
export const PAST_DAYS = 14;
export const FUTURE_DAYS = 60;

/** Local midnight `n` days after `dayStartMs` (DST-safe: built from calendar fields). */
export function addDays(dayStartMs: number, n: number): number {
  const d = new Date(dayStartMs);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n).getTime();
}

/** The seven days (Sunday first) of the week containing `dayStartMs`. */
export function weekOf(dayStartMs: number): number[] {
  const start = addDays(dayStartMs, -new Date(dayStartMs).getDay());
  return Array.from({ length: 7 }, (_, i) => addDays(start, i));
}

export function clampDay(dayStartMs: number, todayMs: number): number {
  return Math.min(addDays(todayMs, FUTURE_DAYS), Math.max(addDays(todayMs, -PAST_DAYS), startOfDay(dayStartMs)));
}

interface WeekStripProps {
  selected: number;
  today: number;
  /** Days known to have meetings (from the regular sync or a day already read). */
  busyDays: ReadonlySet<number>;
  onSelect: (dayStartMs: number) => void;
}

/**
 * The week of the selected day, in the layout's own direction: in Hebrew (rtl) Sunday is on the
 * right and the days run leftwards, as in a Hebrew calendar. The first arrow is "previous week"
 * and sits on the leading side where earlier days are, so its chevron points that way. A dot
 * marks a day with meetings.
 */
export function WeekStrip({ selected, today, busyDays, onSelect }: WeekStripProps) {
  const rtl = uiDirection() === "rtl";
  const days = weekOf(selected);
  const first = addDays(today, -PAST_DAYS);
  const last = addDays(today, FUTURE_DAYS);
  const canPrev = days[0] > first;
  const canNext = days[6] < last;
  const go = (weeks: number) => onSelect(clampDay(addDays(selected, weeks * 7), today));
  const Back = rtl ? ChevronRightIcon : ChevronLeftIcon;
  const Forward = rtl ? ChevronLeftIcon : ChevronRightIcon;

  const arrow = (enabled: boolean, label: string, onClick: () => void, Icon: typeof Back) => (
    <RoundButton fill="none" tint={color.fgTertiary} ariaLabel={label} disabled={!enabled} onPress={onClick}>
      <Icon size={icon.medium} strokeWidth={2.4} />
    </RoundButton>
  );

  return (
    <div className="flex items-center gap-1" role="group" aria-label={t("tab.calendar")}>
      {arrow(canPrev, t("calendar.prevWeek"), () => go(-1), Back)}
      <div className="flex-1 grid grid-cols-7 gap-1">
        {days.map((day) => {
          const isSelected = day === selected;
          const isToday = day === today;
          const outOfRange = day < first || day > last;
          const date = new Date(day);
          const todayTint = isToday && !isSelected ? color.warning : undefined;
          return (
            <button
              key={day}
              type="button"
              disabled={outOfRange}
              aria-pressed={isSelected}
              aria-current={isToday ? "date" : undefined}
              // z-10: a day's visible cell wins over the week arrows' invisible 44px hit extension, which
              // otherwise reached over the first / last day's edge (measured in the real WebView2).
              className={`hit-area z-10 h-11 rounded-control flex flex-col items-center justify-center transition-colors disabled:opacity-25 ${
                isSelected ? "ci-selected bg-fg text-island" : "text-fg-secondary hover:bg-surface-hover"
              } ${day < today && !isSelected ? "opacity-60" : ""}`}
              onClick={() => onSelect(day)}
            >
              <span className="text-micro" style={{ color: todayTint }}>
                {weekdayLetter(date)}
              </span>
              <span className="text-label tabular-nums" style={{ color: todayTint }}>
                {date.getDate()}
              </span>
              <span
                className="ci-mark rounded-full"
                // 4px: the smallest mark that still reads as a dot next to 14px digits; always
                // reserved so the digits sit at the same height with or without meetings.
                style={{ width: 4, height: 4, background: busyDays.has(day) ? (isSelected ? color.island : color.fgTertiary) : "transparent" }}
                aria-hidden="true"
              />
            </button>
          );
        })}
      </div>
      {arrow(canNext, t("calendar.nextWeek"), () => go(1), Forward)}
    </div>
  );
}
