import { startOfDay, weekdayLetter } from "../../../lib/dateFormat";
import { getLocale, t } from "../../../lib/i18n";
import { ChevronLeftIcon, ChevronRightIcon } from "../ui/icons";
import { SYSTEM_COLORS } from "../ui/primitives";

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
 * The week of the selected day. In Hebrew it runs right to left, Sunday on the right, as a
 * Hebrew calendar does; the arrows move a week. A dot marks a day with meetings.
 */
export function WeekStrip({ selected, today, busyDays, onSelect }: WeekStripProps) {
  const rtl = getLocale() === "he";
  const days = weekOf(selected);
  const first = addDays(today, -PAST_DAYS);
  const last = addDays(today, FUTURE_DAYS);
  const canPrev = days[0] > first;
  const canNext = days[6] < last;
  const go = (weeks: number) => onSelect(clampDay(addDays(selected, weeks * 7), today));
  // Physical arrows: in a right-to-left strip "earlier" is on the right.
  const Back = rtl ? ChevronRightIcon : ChevronLeftIcon;
  const Forward = rtl ? ChevronLeftIcon : ChevronRightIcon;

  const arrow = (enabled: boolean, label: string, onClick: () => void, Icon: typeof Back) => (
    <button
      type="button"
      className="w-6 h-9 flex items-center justify-center rounded-full text-white/45 hover:text-white disabled:opacity-25 disabled:pointer-events-none flex-shrink-0"
      aria-label={label}
      disabled={!enabled}
      onClick={onClick}
    >
      <Icon size={15} strokeWidth={2.4} />
    </button>
  );

  return (
    <div dir={rtl ? "rtl" : "ltr"} className="flex items-center gap-0.5" role="group" aria-label={t("tab.calendar")}>
      {arrow(canPrev, t("calendar.prevWeek"), () => go(-1), Back)}
      <div className="flex-1 grid grid-cols-7 gap-0.5">
        {days.map((day) => {
          const isSelected = day === selected;
          const isToday = day === today;
          const outOfRange = day < first || day > last;
          const date = new Date(day);
          return (
            <button
              key={day}
              type="button"
              disabled={outOfRange}
              aria-pressed={isSelected}
              aria-current={isToday ? "date" : undefined}
              className={`relative h-9 rounded-[12px] flex flex-col items-center justify-center leading-none transition-colors disabled:opacity-25 ${
                isSelected ? "bg-white text-black" : "text-white/70 hover:bg-white/[0.08]"
              } ${day < today && !isSelected ? "opacity-60" : ""}`}
              onClick={() => onSelect(day)}
            >
              <span className="text-[9.5px] font-semibold" style={{ color: isToday && !isSelected ? SYSTEM_COLORS.orange : undefined }}>
                {weekdayLetter(date)}
              </span>
              <span className="mt-[3px] text-[13px] font-semibold tabular-nums" style={{ color: isToday && !isSelected ? SYSTEM_COLORS.orange : undefined }}>
                {date.getDate()}
              </span>
              {busyDays.has(day) && (
                <span
                  className="absolute bottom-[3px] w-[3px] h-[3px] rounded-full"
                  style={{ background: isSelected ? "#000" : "rgba(255,255,255,0.55)" }}
                  aria-hidden="true"
                />
              )}
            </button>
          );
        })}
      </div>
      {arrow(canNext, t("calendar.nextWeek"), () => go(1), Forward)}
    </div>
  );
}
