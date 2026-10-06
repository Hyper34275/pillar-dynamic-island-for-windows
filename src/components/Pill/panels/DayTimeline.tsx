import { formatTime } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import { nextDayStart } from "../../../lib/calendar/dayRange";
import type { CalendarEventDto } from "../../../lib/calendar/types";
import { SYSTEM_COLORS } from "../ui/primitives";

const HOUR_MS = 3_600_000;
/** The working day is always shown; earlier or later meetings stretch it. */
const DEFAULT_FROM_HOUR = 8;
const DEFAULT_TO_HOUR = 18;
const MAX_LANES = 3;
const TRACK_HEIGHT = 22;

export interface TimelineBlock {
  event: CalendarEventDto;
  /** 0..1 along the visible hours. */
  left: number;
  width: number;
  lane: number;
}

export interface TimelineLayout {
  fromHour: number;
  toHour: number;
  lanes: number;
  blocks: TimelineBlock[];
}

/**
 * Where each timed meeting of the day goes on a horizontal track. Overlapping meetings get
 * their own lane (at most three; more share the last one). Pure, for tests.
 */
export function layoutTimeline(events: readonly CalendarEventDto[], dayStartMs: number): TimelineLayout {
  const dayEnd = nextDayStart(dayStartMs);
  const timed = events
    .filter((e) => !e.allDay)
    .map((e) => ({ e, start: Math.max(Date.parse(e.startUtc), dayStartMs), end: Math.min(Date.parse(e.endUtc), dayEnd) }))
    .filter((x) => x.end > x.start)
    .sort((a, b) => a.start - b.start || b.end - a.end);
  const hourOf = (ms: number) => (ms - dayStartMs) / HOUR_MS;
  const dayHours = (dayEnd - dayStartMs) / HOUR_MS;
  let fromHour = DEFAULT_FROM_HOUR;
  let toHour = DEFAULT_TO_HOUR;
  for (const x of timed) {
    fromHour = Math.min(fromHour, Math.floor(hourOf(x.start)));
    toHour = Math.max(toHour, Math.ceil(hourOf(x.end)));
  }
  fromHour = Math.max(0, fromHour);
  toHour = Math.min(dayHours, toHour);
  const span = toHour - fromHour;
  const laneEnds: number[] = [];
  const blocks = timed.map((x) => {
    let lane = laneEnds.findIndex((end) => end <= x.start);
    if (lane === -1) lane = laneEnds.length < MAX_LANES ? laneEnds.length : MAX_LANES - 1;
    laneEnds[lane] = Math.max(laneEnds[lane] ?? 0, x.end);
    return {
      event: x.e,
      left: (hourOf(x.start) - fromHour) / span,
      width: (x.end - x.start) / HOUR_MS / span,
      lane,
    };
  });
  return { fromHour, toHour, lanes: Math.max(1, laneEnds.length), blocks };
}

interface DayTimelineProps {
  events: readonly CalendarEventDto[];
  dayStartMs: number;
  nowMs: number;
  colorOf: (e: CalendarEventDto) => string;
}

/** The day at a glance: one bar from morning to evening with the meetings in their colors. */
export function DayTimeline({ events, dayStartMs, nowMs, colorOf }: DayTimelineProps) {
  const { fromHour, toHour, lanes, blocks } = layoutTimeline(events, dayStartMs);
  const span = toHour - fromHour;
  const step = span > 12 ? 3 : 2;
  const ticks: number[] = [];
  for (let h = Math.ceil(fromHour / step) * step; h <= toHour; h += step) ticks.push(h);
  const nowPos = (nowMs - dayStartMs) / HOUR_MS;
  const showNow = nowPos >= fromHour && nowPos <= toHour;
  const laneHeight = (TRACK_HEIGHT - (lanes - 1) * 2) / lanes;

  return (
    <div dir="ltr" className="flex flex-col gap-1 px-1" role="img" aria-label={t("calendar.timeline")}>
      <div className="relative w-full rounded-[7px] bg-white/[0.06] overflow-hidden" style={{ height: TRACK_HEIGHT }}>
        {ticks.map((h) => (
          <span key={h} className="absolute top-0 bottom-0 w-px bg-white/[0.07]" style={{ left: `${((h - fromHour) / span) * 100}%` }} aria-hidden="true" />
        ))}
        {blocks.map(({ event, left, width, lane }) => (
          <span
            key={event.id}
            className="absolute rounded-[4px]"
            title={`${formatTime(new Date(event.startUtc))} ${event.subject}`}
            style={{
              left: `${left * 100}%`,
              width: `max(3px, calc(${width * 100}% - 1px))`,
              top: lane * (laneHeight + 2),
              height: laneHeight,
              background: colorOf(event),
              opacity: Date.parse(event.endUtc) <= nowMs ? 0.45 : 0.9,
            }}
          />
        ))}
        {showNow && (
          <span
            className="absolute top-0 bottom-0 w-[2px] rounded-full"
            style={{ left: `calc(${((nowPos - fromHour) / span) * 100}% - 1px)`, background: SYSTEM_COLORS.orange }}
            aria-hidden="true"
          />
        )}
      </div>
      <div className="relative h-[11px]" aria-hidden="true">
        {ticks.map((h) => (
          <span
            key={h}
            className="absolute text-[9.5px] font-medium tabular-nums text-white/35 -translate-x-1/2"
            style={{ left: `${((h - fromHour) / span) * 100}%` }}
          >
            {h}
          </span>
        ))}
      </div>
    </div>
  );
}
