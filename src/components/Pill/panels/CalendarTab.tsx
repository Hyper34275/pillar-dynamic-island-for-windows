import { useCalendar } from "../../../hooks/useCalendar";
import { useMinute } from "../../../hooks/useClock";
import { dayLabel, formatTime, relativeMinutes } from "../../../lib/dateFormat";
import { t, type MessageKey } from "../../../lib/i18n";
import { selectAllDay, selectUpcoming } from "../../../lib/calendar/select";
import type { CalendarEventDto, CalendarSnapshot, CalendarStatus } from "../../../lib/calendar/types";
import { EmptyState, SYSTEM_COLORS } from "../ui/primitives";
import { CalendarIcon } from "../ui/icons";

const STATUS_COPY: Record<Exclude<CalendarStatus, "connected">, { title: MessageKey; hint?: MessageKey }> = {
  waiting: { title: "calendar.waiting", hint: "calendar.waitingHint" },
  connecting: { title: "calendar.connecting" },
  newOutlookOnly: { title: "calendar.newOutlook", hint: "calendar.newOutlookHint" },
  elevationMismatch: { title: "calendar.elevation", hint: "calendar.elevationHint" },
  unresponsive: { title: "calendar.unresponsive", hint: "calendar.unresponsiveHint" },
  failed: { title: "calendar.failed", hint: "calendar.failedHint" },
};

/** How many meetings the tab shows: the next one plus up to two after it. */
const MAX_MEETINGS = 3;
/** Meetings already in progress that are listed next to the next one (a long "Busy" block must not hide it). */
const MAX_IN_PROGRESS = 2;
const MAX_ALL_DAY = 2;

function timeRange(event: CalendarEventDto): string {
  return `${formatTime(new Date(event.startUtc))} – ${formatTime(new Date(event.endUtc))}`;
}

function subjectOf(event: CalendarEventDto): string {
  return event.subject.trim() || t("calendar.noSubject");
}

function withDay(event: CalendarEventDto, nowMs: number, text: string): string {
  const day = dayLabel(new Date(event.startUtc), nowMs);
  return day ? `${day}, ${text}` : text;
}

function NextMeetingCard({ event, nowMs }: { event: CalendarEventDto; nowMs: number }) {
  const startMs = Date.parse(event.startUtc);
  const ongoing = startMs <= nowMs;
  const caption = t(ongoing ? "calendar.now" : "calendar.next");
  return (
    <section dir="ltr" className="rounded-[22px] bg-white/[0.08] px-4 py-3.5 flex flex-col gap-1.5" aria-label={caption}>
      <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-white/45" dir="auto">
        {caption}
      </span>
      <h3 className="text-[18px] font-semibold leading-snug text-white line-clamp-2" dir="auto" style={{ unicodeBidi: "plaintext" }}>
        {subjectOf(event)}
      </h3>
      <div className="flex items-baseline justify-between gap-3 text-[13px] tabular-nums">
        <span className="text-white/75 truncate">{withDay(event, nowMs, timeRange(event))}</span>
        {!ongoing && (
          <span className="flex-shrink-0 text-white/45" dir="auto">
            {relativeMinutes((startMs - nowMs) / 60_000)}
          </span>
        )}
      </div>
      {event.location && (
        <span className="text-[12px] text-white/45 truncate" dir="auto" style={{ unicodeBidi: "plaintext" }}>
          {event.location}
        </span>
      )}
    </section>
  );
}

function MeetingRow({ event, nowMs }: { event: CalendarEventDto; nowMs: number }) {
  const day = dayLabel(new Date(event.startUtc), nowMs);
  return (
    <li dir="ltr" className="flex items-baseline gap-3 px-1">
      <span className="w-[64px] flex-shrink-0 text-[12.5px] font-medium tabular-nums text-white/55 whitespace-nowrap">
        {formatTime(new Date(event.startUtc))}
      </span>
      <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-white/85" dir="auto" style={{ unicodeBidi: "plaintext" }}>
        {subjectOf(event)}
      </span>
      {day && (
        <span className="flex-shrink-0 text-[11px] text-white/40" dir="auto">
          {day}
        </span>
      )}
    </li>
  );
}

function AllDayRow({ event }: { event: CalendarEventDto }) {
  return (
    <li dir="ltr" className="flex items-baseline gap-3 px-1">
      <span className="w-[64px] flex-shrink-0 text-[11px] font-semibold uppercase tracking-[0.06em] text-white/40 truncate" dir="auto">
        {t("calendar.allDay")}
      </span>
      <span className="min-w-0 flex-1 truncate text-[12.5px] text-white/70" dir="auto" style={{ unicodeBidi: "plaintext" }}>
        {subjectOf(event)}
      </span>
    </li>
  );
}

function StatusLine({ status, errorCode }: { status: Exclude<CalendarStatus, "connected">; errorCode: string | null }) {
  return (
    <div dir="ltr" className="flex items-center gap-2 px-1 text-[11.5px] text-white/50">
      <span className="w-1.5 h-1.5 rounded-full flex-shrink-0" style={{ background: SYSTEM_COLORS.orange }} aria-hidden="true" />
      <span className="truncate" dir="auto">
        {t(STATUS_COPY[status].title)}
      </span>
      {errorCode && status !== "waiting" && <span className="ml-auto tabular-nums text-white/35">{errorCode}</span>}
    </div>
  );
}

interface CalendarViewProps {
  snapshot: CalendarSnapshot;
  nowMs: number;
}

/** Pure rendering of a calendar snapshot — the tab wires it to live data. */
export function CalendarView({ snapshot, nowMs }: CalendarViewProps) {
  const meetings = selectUpcoming(snapshot.events, nowMs, MAX_MEETINGS + MAX_IN_PROGRESS);
  const started = meetings.filter((m) => Date.parse(m.startUtc) <= nowMs);
  const future = meetings.filter((m) => Date.parse(m.startUtc) > nowMs).slice(0, MAX_MEETINGS);
  // The card is the next meeting that has not started; one that is already running only takes it
  // when nothing else is coming, and otherwise shows above it as a compact "Now" row.
  const [next, ...later] = future.length > 0 ? future : started.slice(0, MAX_MEETINGS);
  const inProgress = future.length > 0 ? started.slice(0, MAX_IN_PROGRESS) : [];
  const allDay = selectAllDay(snapshot.events, nowMs, MAX_ALL_DAY);
  const degraded = snapshot.status !== "connected";
  const allDayList = allDay.length > 0 && (
    <ul className="flex flex-col gap-1.5" aria-label={t("calendar.allDay")}>
      {allDay.map((event) => (
        <AllDayRow key={event.id} event={event} />
      ))}
    </ul>
  );

  if (!next) {
    const copy = degraded ? STATUS_COPY[snapshot.status as Exclude<CalendarStatus, "connected">] : null;
    return (
      <div dir="ltr" className="flex-1 flex flex-col gap-3">
        {allDayList}
        <div className="flex-1 flex flex-col items-center justify-center">
          <EmptyState
            icon={<CalendarIcon size={22} />}
            title={t(copy ? copy.title : "calendar.noEvents")}
            subtitle={copy ? (copy.hint ? t(copy.hint) : undefined) : t("calendar.noEventsHint")}
          >
            {degraded && snapshot.errorCode && snapshot.status !== "waiting" && (
              <span dir="auto" className="mt-1 text-[11px] tabular-nums text-white/35" style={{ unicodeBidi: "plaintext" }}>
                {t("calendar.code", { code: snapshot.errorCode })}
              </span>
            )}
          </EmptyState>
        </div>
      </div>
    );
  }

  return (
    <div dir="ltr" className="flex flex-col gap-3">
      {snapshot.status !== "connected" && <StatusLine status={snapshot.status} errorCode={snapshot.errorCode} />}
      {allDayList}
      {inProgress.length > 0 && (
        <ul className="flex flex-col gap-1.5" aria-label={t("calendar.now")}>
          {inProgress.map((event) => (
            <li key={event.id} dir="ltr" className="flex items-baseline gap-3 px-1">
              <span className="w-[64px] flex-shrink-0 text-[11px] font-semibold uppercase tracking-[0.06em] text-white/40 truncate" dir="auto">
                {t("calendar.now")}
              </span>
              <span className="min-w-0 flex-1 truncate text-[12.5px] text-white/70" dir="auto" style={{ unicodeBidi: "plaintext" }}>
                {subjectOf(event)}
              </span>
            </li>
          ))}
        </ul>
      )}
      <NextMeetingCard event={next} nowMs={nowMs} />
      {later.length > 0 && (
        <ul className="flex flex-col gap-2" aria-label={t("calendar.later")}>
          {later.map((event) => (
            <MeetingRow key={event.id} event={event} nowMs={nowMs} />
          ))}
        </ul>
      )}
    </div>
  );
}

export function CalendarTab() {
  const snapshot = useCalendar();
  // Minute resolution is enough: it refreshes the relative time and drops finished meetings.
  const nowMs = useMinute().getTime();
  return <CalendarView snapshot={snapshot} nowMs={nowMs} />;
}
