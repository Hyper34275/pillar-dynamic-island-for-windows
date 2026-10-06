import { useCalendar } from "../../../hooks/useCalendar";
import { useMinute } from "../../../hooks/useClock";
import { formatTime, relativeMinutes } from "../../../lib/dateFormat";
import { t, type MessageKey } from "../../../lib/i18n";
import { selectUpcoming } from "../../../lib/calendar/select";
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

function timeRange(event: CalendarEventDto): string {
  return `${formatTime(new Date(event.startUtc))} – ${formatTime(new Date(event.endUtc))}`;
}

function subjectOf(event: CalendarEventDto): string {
  return event.subject.trim() || t("calendar.noSubject");
}

function NextMeetingCard({ event, nowMs }: { event: CalendarEventDto; nowMs: number }) {
  const startMs = Date.parse(event.startUtc);
  const when = startMs <= nowMs ? t("calendar.inProgress") : relativeMinutes((startMs - nowMs) / 60_000);
  return (
    <section dir="ltr" className="rounded-[22px] bg-white/[0.08] px-4 py-3.5 flex flex-col gap-1.5" aria-label={t("calendar.next")}>
      <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-white/45" dir="auto">
        {t("calendar.next")}
      </span>
      <h3 className="text-[18px] font-semibold leading-snug text-white line-clamp-2" dir="auto" style={{ unicodeBidi: "plaintext" }}>
        {subjectOf(event)}
      </h3>
      <div className="flex items-baseline justify-between gap-3 text-[13px] tabular-nums">
        <span className="text-white/75 whitespace-nowrap">{timeRange(event)}</span>
        <span className="text-white/45 truncate" dir="auto">
          {when}
        </span>
      </div>
      {event.location && (
        <span className="text-[12px] text-white/45 truncate" dir="auto" style={{ unicodeBidi: "plaintext" }}>
          {event.location}
        </span>
      )}
    </section>
  );
}

function MeetingRow({ event }: { event: CalendarEventDto }) {
  return (
    <li dir="ltr" className="flex items-baseline gap-3 px-1">
      <span className="w-[64px] flex-shrink-0 text-[12.5px] font-medium tabular-nums text-white/55 whitespace-nowrap">
        {formatTime(new Date(event.startUtc))}
      </span>
      <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-white/85" dir="auto" style={{ unicodeBidi: "plaintext" }}>
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
  const [next, ...later] = selectUpcoming(snapshot.events, nowMs, MAX_MEETINGS);
  const degraded = snapshot.status !== "connected";

  if (!next) {
    const copy = degraded ? STATUS_COPY[snapshot.status as Exclude<CalendarStatus, "connected">] : null;
    return (
      <div dir="ltr" className="flex-1 flex flex-col items-center justify-center">
        <EmptyState
          icon={<CalendarIcon size={22} />}
          title={t(copy ? copy.title : "calendar.noEvents")}
          subtitle={copy ? (copy.hint ? t(copy.hint) : undefined) : t("calendar.noEventsHint")}
        >
          {degraded && snapshot.errorCode && snapshot.status !== "waiting" && (
            <span className="mt-1 text-[11px] tabular-nums text-white/35">{t("calendar.code", { code: snapshot.errorCode })}</span>
          )}
        </EmptyState>
      </div>
    );
  }

  return (
    <div dir="ltr" className="flex flex-col gap-3">
      {snapshot.status !== "connected" && <StatusLine status={snapshot.status} errorCode={snapshot.errorCode} />}
      <NextMeetingCard event={next} nowMs={nowMs} />
      {later.length > 0 && (
        <ul className="flex flex-col gap-2" aria-label={t("calendar.later")}>
          {later.map((event) => (
            <MeetingRow key={event.id} event={event} />
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
