import { useMemo, useState } from "react";
import { useCalendar } from "../../../hooks/useCalendar";
import { useMinute, useToday } from "../../../hooks/useClock";
import { dayLabel, formatTime, relativeMinutes, startOfDay } from "../../../lib/dateFormat";
import { t, type MessageKey } from "../../../lib/i18n";
import { useAnsweredInviteIds } from "../../../lib/calendar/inviteAnswers";
import { eventsOfDay, useCalendarDay, type DayState } from "../../../lib/calendar/dayRange";
import { isRealMeeting, selectAllDay, selectUpcoming } from "../../../lib/calendar/select";
import type { CalendarEventDto, CalendarSnapshot, CalendarStatus, MeetingInviteDto } from "../../../lib/calendar/types";
import { silence, useSilenceUntil } from "../../../lib/island/silence";
import { inviteBody } from "../../../hooks/useMeetingInvites";
import { EmptyState, SYSTEM_COLORS } from "../ui/primitives";
import { BellFilledIcon, BellSlashIcon, CalendarIcon, VideoIcon } from "../ui/icons";
import { InviteActions, JoinButton } from "../ui/meetingActions";
import { ipc } from "../../../lib/ipc";
import { colorOf } from "../ui/eventColor";
import { DayTimeline } from "./DayTimeline";
import { addDays, WeekStrip } from "./WeekStrip";

const STATUS_COPY: Record<Exclude<CalendarStatus, "connected">, { title: MessageKey; hint?: MessageKey }> = {
  waiting: { title: "calendar.waiting", hint: "calendar.waitingHint" },
  connecting: { title: "calendar.connecting" },
  newOutlookOnly: { title: "calendar.newOutlook", hint: "calendar.newOutlookHint" },
  elevationMismatch: { title: "calendar.elevation", hint: "calendar.elevationHint" },
  unresponsive: { title: "calendar.unresponsive", hint: "calendar.unresponsiveHint" },
  failed: { title: "calendar.failed", hint: "calendar.failedHint" },
};

/** Meetings already in progress that are listed next to the next one (a long "Busy" block must not hide it). */
const MAX_IN_PROGRESS = 2;
const MAX_ALL_DAY = 3;
/** The backend sends at most 50 events (48 h); all of them can be listed. */
const MAX_LISTED = 50;
/** The regular sync reads now .. +48 h; a day inside that is never read again on demand. */
const SYNC_HORIZON_MS = 48 * 3_600_000;

export { colorOf, DEFAULT_EVENT_COLOR } from "../ui/eventColor";

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

/** The event's Outlook category color, as a thin vertical bar in front of it. */
function ColorBar({ event }: { event: CalendarEventDto }) {
  return <span className="w-[3px] self-stretch min-h-[16px] rounded-full flex-shrink-0" style={{ background: colorOf(event) }} aria-hidden="true" />;
}

/** A small round "join" button for rows. */
function JoinIcon({ event }: { event: CalendarEventDto }) {
  if (!event.meetingUrl) return null;
  const url = event.meetingUrl;
  return (
    <button
      type="button"
      className="w-6 h-6 flex-shrink-0 rounded-full flex items-center justify-center hover:brightness-125"
      style={{ color: SYSTEM_COLORS.green, background: `color-mix(in srgb, ${SYSTEM_COLORS.green} 18%, transparent)` }}
      aria-label={t("calendar.joinAria", { subject: subjectOf(event) })}
      onClick={(e) => {
        e.stopPropagation();
        void ipc.openMeetingUrl(url);
      }}
    >
      <VideoIcon size={13} strokeWidth={2.2} />
    </button>
  );
}

/** Ring / silent for the meeting in progress: silent holds notifications until it ends. */
function SilenceToggle({ event, silenceUntil, nowMs }: { event: CalendarEventDto; silenceUntil: number | null; nowMs: number }) {
  const silent = silenceUntil !== null && silenceUntil > nowMs;
  const label = t(silent ? "calendar.unsilence" : "calendar.silence");
  return (
    <button
      type="button"
      className="w-7 h-7 flex-shrink-0 rounded-full flex items-center justify-center hover:bg-white/10"
      style={{ color: silent ? SYSTEM_COLORS.red : "rgba(255,255,255,0.7)" }}
      aria-label={label}
      aria-pressed={silent}
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        if (silent) silence.clear();
        else silence.until(Date.parse(event.endUtc));
      }}
    >
      {silent ? <BellSlashIcon size={15} /> : <BellFilledIcon size={14} />}
    </button>
  );
}

/**
 * The meeting that matters most. Two rows: caption and countdown on top; time range and subject
 * on one line under it (subject on the right, where a Hebrew subject starts anyway), then the
 * location and the join button. The left edge carries the event's category color.
 */
function NextMeetingCard({ event, nowMs, silenceUntil }: { event: CalendarEventDto; nowMs: number; silenceUntil: number | null }) {
  const startMs = Date.parse(event.startUtc);
  const ongoing = startMs <= nowMs;
  const caption = t(ongoing ? "calendar.now" : "calendar.next");
  const countdown = ongoing
    ? t("calendar.endsIn", { rel: relativeMinutes((Date.parse(event.endUtc) - nowMs) / 60_000) })
    : relativeMinutes((startMs - nowMs) / 60_000);
  const hasFooter = !!event.meetingUrl || !!event.location || ongoing;
  return (
    <section dir="ltr" className="relative overflow-hidden rounded-[22px] bg-white/[0.08] pl-5 pr-4 py-3.5 flex flex-col gap-2" aria-label={caption}>
      <span className="absolute inset-y-0 left-0 w-[5px]" style={{ background: colorOf(event) }} aria-hidden="true" />
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.08em] text-white/45 truncate" dir="auto">
          {caption}
        </span>
        <span className="flex-shrink-0 text-[12.5px] font-medium tabular-nums text-white/60" dir="auto">
          {countdown}
        </span>
      </div>
      <div className="flex items-baseline justify-between gap-3">
        <span className="flex-shrink-0 text-[13px] font-medium tabular-nums text-white/75 whitespace-nowrap">{withDay(event, nowMs, timeRange(event))}</span>
        <h3
          className="min-w-0 flex-1 text-right text-[18px] font-semibold leading-snug text-white line-clamp-2"
          dir="auto"
          style={{ unicodeBidi: "plaintext", overflowWrap: "anywhere" }}
        >
          {subjectOf(event)}
        </h3>
      </div>
      {hasFooter && (
        <div className="flex items-center justify-between gap-3 min-h-[28px]">
          <span className="flex items-center gap-1.5 flex-shrink-0">
            {event.meetingUrl && <JoinButton url={event.meetingUrl} subject={subjectOf(event)} />}
            {ongoing && <SilenceToggle event={event} silenceUntil={silenceUntil} nowMs={nowMs} />}
          </span>
          {event.location && (
            <span className="min-w-0 text-right text-[12px] text-white/45 truncate" dir="auto" style={{ unicodeBidi: "plaintext" }}>
              {event.location}
            </span>
          )}
        </div>
      )}
    </section>
  );
}

/** A compact row: color bar, a fixed-width lead (time, "Now", "All day"), the subject, and join. */
function EventRow({ event, lead, leadIsLabel = false, joinable = false }: { event: CalendarEventDto; lead: string; leadIsLabel?: boolean; joinable?: boolean }) {
  return (
    <li dir="ltr" className="flex items-center gap-2.5 px-1 py-[3px] min-h-[28px]">
      <ColorBar event={event} />
      <span
        className={`w-[58px] flex-shrink-0 truncate whitespace-nowrap ${
          leadIsLabel ? "text-[10.5px] font-semibold uppercase tracking-[0.06em] text-white/40" : "text-[12.5px] font-medium tabular-nums text-white/55"
        }`}
        dir="auto"
      >
        {lead}
      </span>
      <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-white/85" dir="auto" style={{ unicodeBidi: "plaintext" }}>
        {subjectOf(event)}
      </span>
      {joinable && <JoinIcon event={event} />}
    </li>
  );
}

function GroupLabel({ children }: { children: string }) {
  return (
    <h4 className="px-1 pt-1 text-[10.5px] font-semibold uppercase tracking-[0.08em] text-white/40" dir="auto">
      {children}
    </h4>
  );
}

/** Later meetings grouped by day: "Later" for the rest of today, then "Tomorrow", then weekdays. */
function LaterList({ events, nowMs }: { events: CalendarEventDto[]; nowMs: number }) {
  const groups: Array<{ day: number; events: CalendarEventDto[] }> = [];
  for (const event of events) {
    const day = startOfDay(Date.parse(event.startUtc));
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.events.push(event);
    else groups.push({ day, events: [event] });
  }
  return (
    <div className="flex flex-col gap-2" aria-label={t("calendar.later")} role="group">
      {groups.map((group) => (
        <div key={group.day} className="flex flex-col gap-1">
          <GroupLabel>{dayLabel(new Date(group.day), nowMs) ?? t("calendar.later")}</GroupLabel>
          <ul className="flex flex-col gap-0.5">
            {group.events.map((event) => (
              <EventRow key={event.id} event={event} lead={formatTime(new Date(event.startUtc))} joinable />
            ))}
          </ul>
        </div>
      ))}
    </div>
  );
}

/** Invitations still waiting for an answer, answered right here through Outlook. */
function InvitesSection({ invites, nowMs }: { invites: readonly MeetingInviteDto[]; nowMs: number }) {
  if (invites.length === 0) return null;
  return (
    <section className="flex flex-col gap-1.5" aria-label={t("calendar.invites")}>
      <GroupLabel>{t("calendar.invites")}</GroupLabel>
      <ul className="flex flex-col gap-1.5">
        {invites.map((invite) => (
          <li key={invite.id} dir="ltr" className="rounded-[16px] bg-white/[0.06] px-3 py-2 flex flex-col gap-1.5">
            <div className="flex items-baseline justify-between gap-3">
              <span className="flex-shrink-0 text-[11.5px] tabular-nums text-white/50 truncate max-w-[55%]" dir="auto">
                {inviteBody(invite, nowMs)}
              </span>
              <span className="min-w-0 flex-1 text-right text-[13.5px] font-semibold text-white truncate" dir="auto" style={{ unicodeBidi: "plaintext" }}>
                {invite.subject.trim() || t("calendar.noSubject")}
              </span>
            </div>
            <InviteActions inviteId={invite.id} height={26} />
          </li>
        ))}
      </ul>
    </section>
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
  /** Invites answered from the island already (hidden before the next sync confirms it). */
  answeredInvites?: ReadonlySet<string>;
  /** End of the current "silent" (null = ringing), for the toggle on a meeting in progress. */
  silenceUntil?: number | null;
}

/** Pure rendering of today from a calendar snapshot — the tab wires it to live data. Every upcoming meeting is listed; the panel scrolls. */
export function CalendarView({ snapshot, nowMs, answeredInvites, silenceUntil = null }: CalendarViewProps) {
  const meetings = selectUpcoming(snapshot.events, nowMs, MAX_LISTED);
  const started = meetings.filter((m) => Date.parse(m.startUtc) <= nowMs);
  const future = meetings.filter((m) => Date.parse(m.startUtc) > nowMs);
  // The card is the next meeting that has not started; one that is already running only takes it
  // when nothing else is coming, and otherwise shows above it as a compact "Now" row.
  const [next, ...later] = future.length > 0 ? future : started;
  const inProgress = future.length > 0 ? started.slice(0, MAX_IN_PROGRESS) : [];
  const allDay = selectAllDay(snapshot.events, nowMs, MAX_ALL_DAY);
  const invites = snapshot.invites.filter((invite) => !answeredInvites?.has(invite.id));
  const degraded = snapshot.status !== "connected";
  const allDayList = allDay.length > 0 && (
    <ul className="flex flex-col gap-0.5" aria-label={t("calendar.allDay")}>
      {allDay.map((event) => (
        <EventRow key={event.id} event={event} lead={t("calendar.allDay")} leadIsLabel />
      ))}
    </ul>
  );
  const invitesSection = <InvitesSection invites={invites} nowMs={nowMs} />;

  if (!next) {
    const copy = degraded ? STATUS_COPY[snapshot.status as Exclude<CalendarStatus, "connected">] : null;
    return (
      <div dir="ltr" className="flex-1 flex flex-col gap-3">
        {invitesSection}
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
      {invitesSection}
      {allDayList}
      {inProgress.length > 0 && (
        <ul className="flex flex-col gap-0.5" aria-label={t("calendar.now")}>
          {inProgress.map((event) => (
            <EventRow key={event.id} event={event} lead={t("calendar.now")} leadIsLabel joinable />
          ))}
        </ul>
      )}
      <NextMeetingCard event={next} nowMs={nowMs} silenceUntil={silenceUntil} />
      {later.length > 0 && <LaterList events={later} nowMs={nowMs} />}
    </div>
  );
}

interface DayViewProps {
  nowMs: number;
  /** The day's events when known, and how it is being read. */
  day: DayState | undefined;
  status: CalendarStatus;
}

/** Any day but today: its meetings in order, all-day ones first. Pure. */
export function DayView({ nowMs, day, status }: DayViewProps) {
  const events = day?.events ?? null;
  if (!events) {
    const failed = day?.state === "error";
    return (
      <div dir="ltr" className="flex-1 flex flex-col items-center justify-center py-6">
        <EmptyState
          icon={<CalendarIcon size={22} />}
          title={failed ? t(status === "connected" ? "calendar.dayFailed" : STATUS_COPY[status as Exclude<CalendarStatus, "connected">].title) : t("calendar.loading")}
        />
      </div>
    );
  }
  const shown = events.filter((e) => e.allDay || isRealMeeting(e) || e.busyStatus === "free");
  const allDay = shown.filter((e) => e.allDay && e.responseStatus !== "declined");
  const timed = shown.filter((e) => !e.allDay);
  if (allDay.length + timed.length === 0) {
    return (
      <div dir="ltr" className="flex-1 flex flex-col items-center justify-center py-6">
        <EmptyState icon={<CalendarIcon size={22} />} title={t("calendar.dayEmpty")} />
      </div>
    );
  }
  return (
    <div dir="ltr" className="flex flex-col gap-2">
      {allDay.length > 0 && (
        <ul className="flex flex-col gap-0.5" aria-label={t("calendar.allDay")}>
          {allDay.map((event) => (
            <EventRow key={event.id} event={event} lead={t("calendar.allDay")} leadIsLabel />
          ))}
        </ul>
      )}
      <ul className="flex flex-col gap-0.5">
        {timed.map((event) => (
          <EventRow key={event.id} event={event} lead={formatTime(new Date(event.startUtc))} joinable={Date.parse(event.endUtc) > nowMs} />
        ))}
      </ul>
    </div>
  );
}

export function CalendarTab() {
  const snapshot = useCalendar();
  // Minute resolution is enough: it refreshes the relative time and drops finished meetings.
  const nowMs = useMinute().getTime();
  const today = useToday().getTime();
  const [picked, setPicked] = useState<number | null>(null);
  // A pick from yesterday's view does not outlive midnight: back to (the new) today.
  const selected = picked !== null && picked >= addDays(today, -14) ? picked : today;
  const isToday = selected === today;
  const answered = useAnsweredInviteIds();
  const silenceUntil = useSilenceUntil();

  // A day fully inside the regular sync comes from the snapshot; any other is read on demand.
  // Today is read too (for the timeline, which also shows what has already ended).
  const coveredBySync = selected > today && addDays(selected, 1) <= nowMs + SYNC_HORIZON_MS;
  const day = useCalendarDay(selected, snapshot.status === "connected" && !coveredBySync);
  const snapshotDay = useMemo(() => eventsOfDay(snapshot.events, selected), [snapshot.events, selected]);
  const dayEvents: readonly CalendarEventDto[] = coveredBySync ? snapshotDay : (day?.events ?? (isToday ? snapshotDay : []));
  const dayState: DayState | undefined = coveredBySync ? { state: "ready", events: snapshotDay, fetchedAt: nowMs } : day;

  const busyDays = useMemo(() => {
    const days = new Set<number>();
    for (const e of snapshot.events) if (!e.allDay && isRealMeeting(e)) days.add(startOfDay(Date.parse(e.startUtc)));
    for (const e of day?.events ?? []) if (!e.allDay && isRealMeeting(e)) days.add(startOfDay(Date.parse(e.startUtc)));
    return days;
  }, [snapshot.events, day]);

  return (
    <div dir="ltr" className="flex flex-col gap-2.5">
      <WeekStrip selected={selected} today={today} busyDays={busyDays} onSelect={(d) => setPicked(d === today ? null : d)} />
      <DayTimeline events={dayEvents.filter((e) => isRealMeeting(e) || e.busyStatus === "free")} dayStartMs={selected} nowMs={nowMs} colorOf={colorOf} />
      {isToday ? (
        <CalendarView snapshot={snapshot} nowMs={nowMs} answeredInvites={answered} silenceUntil={silenceUntil} />
      ) : (
        <DayView nowMs={nowMs} day={dayState} status={snapshot.status} />
      )}
    </div>
  );
}
