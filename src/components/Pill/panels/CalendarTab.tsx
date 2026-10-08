import { useMemo, useState, type ReactNode } from "react";
import { sourceDirection, textDirection } from "../../../design/direction";
import { alert as alertTokens, color, compact, control, icon } from "../../../design/tokens";
import { useCalendar } from "../../../hooks/useCalendar";
import { useMinute, useToday } from "../../../hooks/useClock";
import { dayLabel, formatTime, relativeMinutes, startOfDay } from "../../../lib/dateFormat";
import { t, type MessageKey } from "../../../lib/i18n";
import { useAnsweredInviteIds } from "../../../lib/calendar/inviteAnswers";
import { dayCache, eventsOfDay, useCalendarDay, type DayState } from "../../../lib/calendar/dayRange";
import { isRealMeeting, selectAllDay, selectUpcoming } from "../../../lib/calendar/select";
import type { CalendarEventDto, CalendarSnapshot, CalendarStatus, MeetingInviteDto } from "../../../lib/calendar/types";
import { silence, useSilenceUntil } from "../../../lib/island/silence";
import { minutesUntil } from "../../../lib/island/countdown";
import { eventSourceLabel, hasSourcesToShow } from "../../../lib/calendar/sources";
import { CalendarSources } from "./CalendarSources";
import { inviteBody } from "../../../hooks/useMeetingInvites";
import { GROUP_CLASS } from "../ui/primitives";
import { EmptyState, ErrorState, STATE_ICON } from "../ui/states";
import { ActionButton, ActionRow, RoundButton } from "../ui/controls";
import { BellFilledIcon, BellSlashIcon, CalendarIcon, VideoIcon } from "../ui/icons";
import { InviteActions, JoinButton } from "../ui/meetingActions";
import { ipc } from "../../../lib/ipc";
import { colorOf, stripesOf } from "../ui/eventColor";
import { DayTimeline } from "./DayTimeline";
import { addDays, WeekStrip } from "./WeekStrip";

/**
 * What the calendar says when it has no events to show because of its own state. "Waiting" and
 * "connecting" are normal (an empty state); the others are things that are wrong (an error state,
 * with the code for support). `retry`: a manual refresh helps (Outlook may simply have been busy).
 */
const STATUS_COPY: Record<Exclude<CalendarStatus, "connected">, { title: MessageKey; hint?: MessageKey; error: boolean; retry: boolean }> = {
  waiting: { title: "calendar.waiting", hint: "calendar.waitingHint", error: false, retry: false },
  connecting: { title: "calendar.connecting", error: false, retry: false },
  newOutlookOnly: { title: "calendar.newOutlook", hint: "calendar.newOutlookHint", error: true, retry: false },
  elevationMismatch: { title: "calendar.elevation", hint: "calendar.elevationHint", error: true, retry: false },
  unresponsive: { title: "calendar.unresponsive", hint: "calendar.unresponsiveHint", error: true, retry: true },
  failed: { title: "calendar.failed", hint: "calendar.failedHint", error: true, retry: true },
};

/** The state block for a calendar that cannot deliver events (any status but "connected"). */
function StatusState({ status, errorCode, onRetry }: { status: Exclude<CalendarStatus, "connected">; errorCode: string | null; onRetry?: () => void }) {
  const copy = STATUS_COPY[status];
  const props = {
    icon: <CalendarIcon size={STATE_ICON} />,
    title: t(copy.title),
    hint: copy.hint ? t(copy.hint) : undefined,
  };
  if (!copy.error) return <EmptyState {...props} />;
  return (
    <ErrorState
      {...props}
      code={errorCode ? t("calendar.code", { code: errorCode }) : undefined}
      action={copy.retry && onRetry ? { label: t("island.tryAgain"), onPress: onRetry } : undefined}
    />
  );
}

/** Meetings already in progress that are listed next to the next one (a long "Busy" block must not hide it). */
const MAX_IN_PROGRESS = 2;
const MAX_ALL_DAY = 3;
/** The backend sends at most 50 events (48 h); all of them can be listed. */
const MAX_LISTED = 50;
/** The regular sync reads now .. +48 h; a day inside that is never read again on demand. */
const SYNC_HORIZON_MS = 48 * 3_600_000;

/** Row geometry: 44 high (the hit minimum), 4-wide colour bars inset 8 top and bottom, a fixed lead column. */
const ROW_HEIGHT = control.hit;
const ROW_BAR_WIDTH = 4;
/** Between the calendar stripe and the category stripe when an event shows both. */
const ROW_BAR_GAP = 2;
const ROW_BAR_HEIGHT = ROW_HEIGHT - 16;
const LEAD_WIDTH = 48;

export { colorOf, DEFAULT_EVENT_COLOR } from "../ui/eventColor";

function timeRange(event: CalendarEventDto): string {
  return `${formatTime(new Date(event.startUtc))} – ${formatTime(new Date(event.endUtc))}`;
}

function subjectOf(event: CalendarEventDto): string {
  return event.subject.trim() || t("calendar.noSubject");
}

/** Which calendar an event is from, when that is not the user's own default one. */
function sourceOf(event: CalendarEventDto): string | null {
  return eventSourceLabel(event, (key) => t(key));
}

/** React key: event ids are per calendar copy, the calendar id keeps them apart for certain. */
function keyOf(event: CalendarEventDto): string {
  return `${event.calendarId}:${event.id}`;
}

/** The range is an LTR run (start first, as written) even inside a Hebrew line; the day label stays in the flow. */
function withDay(event: CalendarEventDto, nowMs: number, text: string): ReactNode {
  const day = dayLabel(new Date(event.startUtc), nowMs);
  const range = <bdi dir="ltr">{text}</bdi>;
  return day ? <>{day}, {range}</> : range;
}

/**
 * The event's colours as rounded bars (radius = half their width) at the leading edge: its Outlook
 * category colour, or, with several calendars shown, its calendar's colour and then the category's.
 * Without a height the bars stretch to the row.
 */
function ColorBar({ event, height }: { event: CalendarEventDto; height?: number }) {
  return (
    <span className={`flex flex-shrink-0 ${height === undefined ? "self-stretch" : ""}`} style={{ gap: ROW_BAR_GAP }} aria-hidden="true">
      {stripesOf(event).map((stripe, i) => (
        <span key={i} className="ci-mark rounded-full" style={{ width: ROW_BAR_WIDTH, height, background: stripe }} />
      ))}
    </span>
  );
}

/** A small round "join" button for rows. */
function JoinIcon({ event }: { event: CalendarEventDto }) {
  if (!event.meetingUrl) return null;
  const url = event.meetingUrl;
  return (
    <RoundButton
      tint={color.positive}
      fill="tint"
      ariaLabel={t("calendar.joinAria", { subject: subjectOf(event) })}
      onPress={() => void ipc.openMeetingUrl(url)}
    >
      <VideoIcon size={icon.small} strokeWidth={2.2} />
    </RoundButton>
  );
}

/**
 * Ring / silent for the meeting in progress: silent holds notifications until it ends. It is an
 * action-button column beside Join, so it carries the same states as ActionButton (the shared
 * ActionButton has no pressed state, which a toggle needs for its aria-pressed).
 */
function SilenceToggle({ event, silenceUntil, nowMs }: { event: CalendarEventDto; silenceUntil: number | null; nowMs: number }) {
  const silent = silenceUntil !== null && silenceUntil > nowMs;
  const label = t(silent ? "calendar.unsilence" : "calendar.silence");
  const Glyph = silent ? BellSlashIcon : BellFilledIcon;
  return (
    <ActionButton
      variant={silent ? "destructive" : "neutral"}
      icon={<Glyph size={control.iconSize} />}
      ariaLabel={label}
      pressed={silent}
      title={label}
      onPress={() => {
        if (silent) silence.clear();
        else silence.until(Date.parse(event.endUtc));
      }}
    >
      {label}
    </ActionButton>
  );
}

/**
 * The meeting that matters most, as a surface card (radius 18, padding 12). The category colour is
 * a 4px rounded bar at the leading edge of the text block, 12 from the card's edge on every side
 * (the card's own padding), so the bar is concentric with the card's curve; a dot would say less
 * about duration/weight and lose the "this is an event of that calendar" reading. Caption and
 * countdown share a row; then the subject (title, 2 lines), time, place; Join (and silence for a
 * meeting in progress) fill the card's width 12 below.
 */
function NextMeetingCard({ event, nowMs, silenceUntil }: { event: CalendarEventDto; nowMs: number; silenceUntil: number | null }) {
  const startMs = Date.parse(event.startUtc);
  const ongoing = startMs <= nowMs;
  const caption = t(ongoing ? "calendar.now" : "calendar.next");
  const countdown = ongoing
    ? t("calendar.endsIn", { rel: relativeMinutes((Date.parse(event.endUtc) - nowMs) / 60_000) })
    : relativeMinutes(minutesUntil(startMs, nowMs));
  const hasActions = !!event.meetingUrl || ongoing;
  const source = sourceOf(event);
  return (
    <section className="ci-surface rounded-surface p-card-pad flex flex-col gap-3" aria-label={caption}>
      <div className="flex gap-3">
        <ColorBar event={event} />
        <div className="min-w-0 flex-1 flex flex-col">
          <div className="flex items-baseline justify-between gap-2">
            <span className="text-meta text-fg-tertiary truncate" title={source ?? undefined}>
              {caption}
              {source && (
                <>
                  {" · "}
                  <bdi dir={sourceDirection(source)}>{source}</bdi>
                </>
              )}
            </span>
            <span className="flex-shrink-0 text-meta text-fg-secondary tabular-nums">{countdown}</span>
          </div>
          <h3 dir={textDirection(subjectOf(event))} className="bidi mt-1 text-title text-fg line-clamp-2 [overflow-wrap:anywhere]">{subjectOf(event)}</h3>
          <span className="mt-1 text-body text-fg-secondary tabular-nums">{withDay(event, nowMs, timeRange(event))}</span>
          {event.location && (
            <span dir={textDirection(event.location)} className="bidi text-body text-fg-tertiary truncate" style={{ marginTop: alertTokens.detailGap }}>
              {event.location}
            </span>
          )}
        </div>
      </div>
      {hasActions && (
        <ActionRow>
          {event.meetingUrl && <JoinButton url={event.meetingUrl} subject={subjectOf(event)} />}
          {ongoing && <SilenceToggle event={event} silenceUntil={silenceUntil} nowMs={nowMs} />}
        </ActionRow>
      )}
    </section>
  );
}

/** A compact row: colour bar, a fixed-width lead (time, "Now", "All day"), the subject, and join. */
function EventRow({ event, lead, leadIsLabel = false, joinable = false }: { event: CalendarEventDto; lead: string; leadIsLabel?: boolean; joinable?: boolean }) {
  const source = sourceOf(event);
  return (
    <li className="flex items-center gap-3 px-3" style={{ minHeight: ROW_HEIGHT }}>
      <ColorBar event={event} height={ROW_BAR_HEIGHT} />
      <span
        className={`flex-shrink-0 truncate whitespace-nowrap text-start ${leadIsLabel ? "text-micro text-fg-tertiary" : "text-meta text-fg-tertiary tabular-nums"}`}
        style={{ width: LEAD_WIDTH }}
      >
        {lead}
      </span>
      <span className="min-w-0 flex-1 flex flex-col">
        <span dir={textDirection(subjectOf(event))} className="bidi truncate text-body text-fg">{subjectOf(event)}</span>
        {source && (
          <span dir={sourceDirection(source)} className="bidi truncate text-micro text-fg-tertiary" title={source}>
            {source}
          </span>
        )}
      </span>
      {joinable && <JoinIcon event={event} />}
    </li>
  );
}

/** Rows of one list as one surface group (hairlines between them, inset like the rows). */
function RowGroup({ children, label }: { children: ReactNode; label?: string }) {
  return (
    <ul className={GROUP_CLASS} aria-label={label}>
      {children}
    </ul>
  );
}

function GroupLabel({ children }: { children: string }) {
  return <h4 className="px-3 mb-1 text-micro text-fg-tertiary">{children}</h4>;
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
    <div className="flex flex-col gap-3" aria-label={t("calendar.later")} role="group">
      {groups.map((group) => (
        <div key={group.day}>
          <GroupLabel>{dayLabel(new Date(group.day), nowMs) ?? t("calendar.later")}</GroupLabel>
          <RowGroup>
            {group.events.map((event) => (
              <EventRow key={keyOf(event)} event={event} lead={formatTime(new Date(event.startUtc))} joinable />
            ))}
          </RowGroup>
        </div>
      ))}
    </div>
  );
}

/** Invitations still waiting for an answer, answered right here through Outlook. */
function InvitesSection({ invites, nowMs }: { invites: readonly MeetingInviteDto[]; nowMs: number }) {
  if (invites.length === 0) return null;
  return (
    <section aria-label={t("calendar.invites")}>
      <GroupLabel>{t("calendar.invites")}</GroupLabel>
      <ul className="flex flex-col gap-2">
        {invites.map((invite) => (
          <li key={invite.id} className="ci-surface rounded-surface p-card-pad flex flex-col gap-3">
            <div className="flex flex-col">
              <span dir={textDirection(invite.subject.trim() || t("calendar.noSubject"))} className="bidi text-headline text-fg line-clamp-2 [overflow-wrap:anywhere]">{invite.subject.trim() || t("calendar.noSubject")}</span>
              <span className="bidi text-meta text-fg-secondary truncate" style={{ marginTop: alertTokens.detailGap }}>
                {inviteBody(invite, nowMs)}
              </span>
            </div>
            <InviteActions inviteId={invite.id} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Shown above cached meetings while Outlook is away: what is wrong, and how old the list is (it is not fresh). */
function StatusLine({ status, errorCode, lastSyncUnixMs }: { status: Exclude<CalendarStatus, "connected">; errorCode: string | null; lastSyncUnixMs: number | null }) {
  return (
    <div className="flex flex-col items-center">
      <div className="flex items-center justify-center gap-2 text-meta text-fg-secondary">
        <span className="ci-mark rounded-full flex-shrink-0" style={{ width: compact.statusDot, height: compact.statusDot, background: color.warning }} aria-hidden="true" />
        <span className="bidi !text-center truncate">{t(STATUS_COPY[status].title)}</span>
        {errorCode && status !== "waiting" && <span className="tabular-nums text-fg-tertiary">{errorCode}</span>}
      </div>
      {lastSyncUnixMs !== null && (
        <span className="bidi text-micro text-fg-tertiary tabular-nums">{t("calendar.lastSynced", { time: formatTime(new Date(lastSyncUnixMs)) })}</span>
      )}
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
  /** Asks the calendar to read Outlook again (the retry of an error state). */
  onRetry?: () => void;
}

/** Pure rendering of today from a calendar snapshot — the tab wires it to live data. Every upcoming meeting is listed; the panel scrolls. */
export function CalendarView({ snapshot, nowMs, answeredInvites, silenceUntil = null, onRetry }: CalendarViewProps) {
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
    <RowGroup label={t("calendar.allDay")}>
      {allDay.map((event) => (
        <EventRow key={keyOf(event)} event={event} lead={t("calendar.allDay")} leadIsLabel />
      ))}
    </RowGroup>
  );
  const invitesSection = <InvitesSection invites={invites} nowMs={nowMs} />;
  const sourcesSection = hasSourcesToShow(snapshot.sources) && <CalendarSources report={snapshot.sources} />;

  if (!next) {
    return (
      <div className="flex-1 flex flex-col gap-2">
        {invitesSection}
        {allDayList}
        {degraded ? (
          <StatusState status={snapshot.status as Exclude<CalendarStatus, "connected">} errorCode={snapshot.errorCode} onRetry={onRetry} />
        ) : (
          <EmptyState icon={<CalendarIcon size={STATE_ICON} />} title={t("calendar.noEvents")} hint={t("calendar.noEventsHint")} />
        )}
        {sourcesSection}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {snapshot.status !== "connected" && <StatusLine status={snapshot.status} errorCode={snapshot.errorCode} lastSyncUnixMs={snapshot.lastSyncUnixMs} />}
      {invitesSection}
      {allDayList}
      {inProgress.length > 0 && (
        <RowGroup label={t("calendar.now")}>
          {inProgress.map((event) => (
            <EventRow key={keyOf(event)} event={event} lead={t("calendar.now")} leadIsLabel joinable />
          ))}
        </RowGroup>
      )}
      <NextMeetingCard event={next} nowMs={nowMs} silenceUntil={silenceUntil} />
      {later.length > 0 && <LaterList events={later} nowMs={nowMs} />}
      {sourcesSection}
    </div>
  );
}

interface DayViewProps {
  nowMs: number;
  /** The day's events when known, and how it is being read. */
  day: DayState | undefined;
  status: CalendarStatus;
  /** Reads the day again (the retry of its error state). */
  onRetry?: () => void;
}

/** Any day but today: its meetings in order, all-day ones first. Pure. */
export function DayView({ nowMs, day, status, onRetry }: DayViewProps) {
  const events = day?.events ?? null;
  if (!events) {
    // Not read yet: calm, no error. Read and failed: the calendar's own state when Outlook is the
    // problem, else the failure of this one day, with a retry.
    if (day?.state !== "error") return <EmptyState icon={<CalendarIcon size={STATE_ICON} />} title={t("calendar.loading")} />;
    if (status !== "connected") return <StatusState status={status} errorCode={null} onRetry={onRetry} />;
    return (
      <ErrorState
        icon={<CalendarIcon size={STATE_ICON} />}
        title={t("calendar.dayFailed")}
        action={onRetry ? { label: t("island.tryAgain"), onPress: onRetry } : undefined}
      />
    );
  }
  const shown = events.filter((e) => e.allDay || isRealMeeting(e) || e.busyStatus === "free");
  const allDay = shown.filter((e) => e.allDay && e.responseStatus !== "declined");
  const timed = shown.filter((e) => !e.allDay);
  if (allDay.length + timed.length === 0) {
    return <EmptyState icon={<CalendarIcon size={STATE_ICON} />} title={t("calendar.dayEmpty")} />;
  }
  return (
    <div className="flex flex-col gap-2">
      {allDay.length > 0 && (
        <RowGroup label={t("calendar.allDay")}>
          {allDay.map((event) => (
            <EventRow key={keyOf(event)} event={event} lead={t("calendar.allDay")} leadIsLabel />
          ))}
        </RowGroup>
      )}
      {timed.length > 0 && (
        <RowGroup>
          {timed.map((event) => (
            <EventRow key={keyOf(event)} event={event} lead={formatTime(new Date(event.startUtc))} joinable={Date.parse(event.endUtc) > nowMs} />
          ))}
        </RowGroup>
      )}
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
    <div className="flex flex-col gap-2 flex-1">
      <WeekStrip selected={selected} today={today} busyDays={busyDays} onSelect={(d) => setPicked(d === today ? null : d)} />
      <DayTimeline events={dayEvents.filter((e) => isRealMeeting(e) || e.busyStatus === "free")} dayStartMs={selected} nowMs={nowMs} colorOf={colorOf} />
      {isToday ? (
        <CalendarView snapshot={snapshot} nowMs={nowMs} answeredInvites={answered} silenceUntil={silenceUntil} onRetry={() => void ipc.calendarRefresh()} />
      ) : (
        <DayView nowMs={nowMs} day={dayState} status={snapshot.status} onRetry={() => dayCache.load(selected)} />
      )}
    </div>
  );
}
