import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CalendarView } from "./CalendarTab";
import { CalendarSources } from "./CalendarSources";
import { MeetingAlert, meetingAlertAnnouncement } from "../MeetingAlert";
import { eventSourceLabel, sourceCounts, sourceStatusKeys } from "../../../lib/calendar/sources";
import type { CalendarEventDto, CalendarSnapshot, CalendarSourceDto, CalendarSourcesReport } from "../../../lib/calendar/types";
import type { ReminderAlert } from "../../../lib/reminders/types";

const NOW = Date.UTC(2026, 9, 6, 10, 0, 0);
const iso = (offsetMin: number) => new Date(NOW + offsetMin * 60_000).toISOString();

function event(id: string, startMin: number, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "mine",
    calendarName: "Calendar",
    sourceKind: "primary",
    subject: `Meeting ${id}`,
    startUtc: iso(startMin),
    endUtc: iso(startMin + 30),
    allDay: false,
    location: null,
    organizer: null,
    isRecurring: false,
    meetingUrl: null,
    busyStatus: "busy",
    responseStatus: "accepted",
    color: null,
    ...extra,
  };
}

function source(id: string, extra: Partial<CalendarSourceDto> = {}): CalendarSourceDto {
  return { id, name: id, group: "shared", kind: "shared", selected: true, active: true, state: "ok", errorCode: null, eventCount: 1, lastReadUnixMs: NOW, ...extra };
}

const report = (sources: CalendarSourceDto[], selection: CalendarSourcesReport["selection"] = "outlook"): CalendarSourcesReport => ({
  sources,
  selection,
  groups: 2,
  listener: true,
  discoveredUnixMs: NOW,
});

const snapshot = (events: CalendarEventDto[], extra: Partial<CalendarSnapshot> = {}): CalendarSnapshot => ({
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: NOW,
  cachedCount: events.length,
  nextRetryUnixMs: null,
  invites: [],
  events,
  sources: null,
  ...extra,
});

const SUPPORT = "לוח משמרות תמיכה טכנית — מוקד ארצי צפון ומרכז, כולל כוננויות לילה וסופי שבוע";

describe("event source labels", () => {
  it("names the calendar of shared events, never of the user's own default one", () => {
    const k = (key: string) => key;
    expect(eventSourceLabel({ sourceKind: "primary", calendarName: "Calendar" }, k)).toBeNull();
    expect(eventSourceLabel({ calendarName: "Calendar" }, k)).toBeNull();
    expect(eventSourceLabel({ sourceKind: "shared", calendarName: " Support " }, k)).toBe("Support");
    expect(eventSourceLabel({ sourceKind: "shared", calendarName: "" }, k)).toBe("calendar.kindShared");
  });

  it("puts the source under shared events in the list and next to the caption of the next meeting", () => {
    const html = renderToStaticMarkup(
      <CalendarView
        nowMs={NOW}
        snapshot={snapshot([
          event("handover", 30, { calendarId: "support", calendarName: SUPPORT, sourceKind: "shared" }),
          event("review", 60),
          event("ops", 120, { calendarId: "team", calendarName: "Team Schedule", sourceKind: "shared" }),
        ])}
      />
    );
    // The next meeting is the shared one: its (Hebrew, very long) calendar name is isolated and carries the full name as a tooltip.
    expect(html).toContain(`<bdi dir="rtl">${SUPPORT}</bdi>`);
    expect(html).toContain(`title="${SUPPORT}"`);
    // Later rows: the shared one has its source line (LTR name isolated as LTR), the own one has none.
    expect(html).toMatch(/Meeting ops<\/span><span dir="ltr" class="bidi truncate text-micro text-fg-tertiary" title="Team Schedule">Team Schedule<\/span>/);
    expect(html).toMatch(/Meeting review<\/span><\/span>/);
  });
});

describe("calendar sources list", () => {
  const sources = [
    source("mine", { name: "Calendar", kind: "primary", group: "my" }),
    source("support", { name: SUPPORT }),
    source("team", { name: "Team Schedule", active: false, selected: false, state: "notSelected", eventCount: 0 }),
    source("old", { name: "Old Department", state: "unavailable", errorCode: "CAL-SHARED-101", eventCount: 0 }),
  ];

  it("counts discovered vs active and explains each state", () => {
    expect(sourceCounts(report(sources))).toEqual({ total: 4, active: 3 });
    expect(sourceStatusKeys(sources[0])).toEqual(["calendar.kindPrimary", "calendar.sourceAlwaysOn"]);
    expect(sourceStatusKeys(sources[2])).toEqual(["calendar.kindShared", "calendar.sourceNotSelected"]);
    expect(sourceStatusKeys(sources[3])).toEqual(["calendar.kindShared", "calendar.sourceUnavailable"]);
  });

  it("is a collapsed summary by default and a list when opened, with codes but no raw errors", () => {
    expect(renderToStaticMarkup(<CalendarSources report={report(sources)} />)).toContain("3 of 4 in use");
    const open = renderToStaticMarkup(<CalendarSources report={report(sources)} defaultOpen />);
    expect(open).toContain("Not selected in Outlook");
    expect(open).toContain("Unavailable · CAL-SHARED-101");
    expect(open).toContain("Always included");
    expect(open).toContain("Choose calendars in Outlook");
    expect(renderToStaticMarkup(<CalendarSources report={report(sources, "remembered")} defaultOpen />)).toContain("the last selection is used");
  });

  it("appears only when there is more than the default calendar", () => {
    const one = renderToStaticMarkup(<CalendarView nowMs={NOW} snapshot={snapshot([event("a", 30)], { sources: report([sources[0]]) })} />);
    expect(one).not.toContain("Calendar sources");
    const many = renderToStaticMarkup(<CalendarView nowMs={NOW} snapshot={snapshot([event("a", 30)], { sources: report(sources) })} />);
    expect(many).toContain("Calendar sources");
  });

  it("says how old cached meetings are while Outlook is away", () => {
    const html = renderToStaticMarkup(
      <CalendarView nowMs={NOW} snapshot={snapshot([event("a", 30)], { status: "waiting", errorCode: "OUTLOOK-101", lastSyncUnixMs: NOW - 5 * 60_000 })} />
    );
    expect(html).toContain("Waiting for Outlook");
    expect(html).toContain("Last updated");
    expect(html).toContain("Meeting a");
  });
});

describe("meeting reminder from a shared calendar", () => {
  const alert: ReminderAlert = {
    key: "k",
    eventId: "e",
    calendarId: "team",
    calendarName: "Team Schedule",
    sourceKind: "shared",
    subject: "Daily Operations Meeting",
    startUtc: iso(30),
    endUtc: iso(60),
    location: null,
    minutesRemaining: 30,
    reminderType: { kind: "beforeStart", minutes: 30 },
  };

  it("names the calendar after the countdown, and says it to screen readers", () => {
    const html = renderToStaticMarkup(<MeetingAlert alert={alert} nowMs={NOW} />);
    expect(html).toMatch(/Meeting in 30 minutes · <bdi dir="ltr"[^>]*>Team Schedule<\/bdi>/);
    expect(meetingAlertAnnouncement(alert, NOW)).toContain("Team Schedule");
  });

  it("adds nothing for the user's own calendar", () => {
    const own = { ...alert, sourceKind: "primary" as const, calendarName: "Calendar" };
    expect(renderToStaticMarkup(<MeetingAlert alert={own} nowMs={NOW} />)).not.toContain(" · ");
  });
});
