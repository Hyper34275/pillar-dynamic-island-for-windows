import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CalendarView } from "./CalendarTab";
import { WAITING_SNAPSHOT, type CalendarEventDto, type CalendarSnapshot } from "../../../lib/calendar/types";

const NOW = Date.UTC(2026, 9, 6, 10, 0, 0);
const iso = (offsetMin: number) => new Date(NOW + offsetMin * 60_000).toISOString();

function event(id: string, startMin: number, endMin: number, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "cal",
    subject: `Meeting ${id}`,
    startUtc: iso(startMin),
    endUtc: iso(endMin),
    allDay: false,
    location: null,
    organizer: null,
    isRecurring: false,
    meetingUrl: null,
    busyStatus: "busy",
    responseStatus: "accepted",
    ...extra,
  };
}

const connected = (events: CalendarEventDto[]): CalendarSnapshot => ({
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: NOW,
  cachedCount: events.length,
  nextRetryUnixMs: null,
  events,
});

const render = (snapshot: CalendarSnapshot) => renderToStaticMarkup(<CalendarView snapshot={snapshot} nowMs={NOW} />);

describe("CalendarView", () => {
  it("shows the waiting state while Outlook is not running", () => {
    const html = render(WAITING_SNAPSHOT);
    expect(html).toContain("Waiting for Outlook");
    expect(html).not.toContain("OUTLOOK-");
  });

  it("explains each degraded status and shows its code", () => {
    const cases: Array<[CalendarSnapshot["status"], string, string | null]> = [
      ["newOutlookOnly", "New Outlook isn&#x27;t supported", "OUTLOOK-104"],
      ["elevationMismatch", "Outlook runs with different permissions", "OUTLOOK-103"],
      ["unresponsive", "Outlook isn&#x27;t responding", "OUTLOOK-109"],
      ["failed", "Couldn&#x27;t read the calendar", "OUTLOOK-108"],
    ];
    for (const [status, title, code] of cases) {
      const html = render({ ...WAITING_SNAPSHOT, status, errorCode: code });
      expect(html, status).toContain(title);
      expect(html, status).toContain(code!);
    }
  });

  it("shows 'no upcoming meetings' when connected with nothing to show", () => {
    expect(render(connected([]))).toContain("No upcoming meetings");
    expect(render(connected([event("all-day", -60, 600, { allDay: true })]))).toContain("No upcoming meetings");
  });

  it("renders the next meeting card with its time and location, then up to two more rows", () => {
    const html = render(
      connected([
        event("one", 25, 55, { location: "Room 12" }),
        event("two", 90, 120),
        event("three", 150, 180),
        event("four", 200, 230),
      ])
    );
    expect(html).toContain("Next meeting");
    expect(html).toContain("Meeting one");
    expect(html).toContain("Room 12");
    expect(html).toMatch(/in 25 min/);
    expect(html).toContain("Meeting two");
    expect(html).toContain("Meeting three");
    expect(html).not.toContain("Meeting four");
  });

  it("labels a meeting that already started 'Now' instead of a countdown", () => {
    const html = render(connected([event("now", -10, 20)]));
    expect(html).toContain(">Now<");
    expect(html).not.toMatch(/in d+ min/);
  });

  it("keeps a long meeting in progress from hiding the real next meeting", () => {
    const html = render(connected([event("block", -120, 240, { subject: "All-morning block" }), event("next", 25, 55, { subject: "Standup" })]));
    const card = html.slice(html.indexOf('aria-label="Next meeting"'));
    expect(card).toContain("Standup");
    expect(card).not.toContain("All-morning block");
    expect(html.slice(0, html.indexOf('aria-label="Next meeting"'))).toContain("All-morning block"); // as a Now row above
  });

  it("uses the card for a meeting in progress when nothing else is coming", () => {
    const html = render(connected([event("now", -10, 20, { subject: "Running" })]));
    expect(html).toContain('aria-label="Now"');
    expect(html).not.toContain("Next meeting");
  });

  it("lets the empty-state texts pick their own direction, so Hebrew around \"Outlook\" is shaped right-to-left", () => {
    const html = render(WAITING_SNAPSHOT);
    expect(html).toMatch(/<span dir="auto"[^>]*>Waiting for Outlook<\/span>/);
    expect(html).toMatch(/<span dir="auto"[^>]*>Your meetings appear/);
  });

  it("shows the connecting state", () => {
    expect(render({ ...WAITING_SNAPSHOT, status: "connecting" })).toContain("Connecting to Outlook");
  });

  it("keeps all-day events in their own compact rows, never as the next meeting", () => {
    const html = render(connected([event("holiday", -600, 600, { allDay: true, subject: "Public holiday" }), event("real", 25, 55)]));
    expect(html).toContain("Public holiday");
    expect(html).toContain("All day");
    const hero = html.slice(html.indexOf('aria-label="Next meeting"'));
    expect(hero).toContain("Meeting real");
    expect(hero).not.toContain("Public holiday");
  });

  it("lists all-day events even when there is no timed meeting", () => {
    const html = render(connected([event("holiday", -600, 600, { allDay: true, subject: "Public holiday" })]));
    expect(html).toContain("Public holiday");
    expect(html).toContain("No upcoming meetings");
  });

  it("does not show a declined all-day event", () => {
    expect(render(connected([event("x", -600, 600, { allDay: true, responseStatus: "declined", subject: "Declined thing" })]))).not.toContain("Declined thing");
  });

  it("shows no organizer", () => {
    expect(render(connected([event("a", 10, 40, { organizer: "Hidden Person" })]))).not.toContain("Hidden Person");
  });

  it("falls back to a placeholder for empty subjects", () => {
    expect(render(connected([event("blank", 10, 40, { subject: "   " })]))).toContain("(No subject)");
  });

  it("keeps showing cached meetings with a status line while Outlook is unavailable", () => {
    const html = render({ ...connected([event("cached", 20, 50)]), status: "unresponsive", errorCode: "OUTLOOK-109" });
    expect(html).toContain("Meeting cached");
    expect(html).toContain("Outlook isn&#x27;t responding");
    expect(html).toContain("OUTLOOK-109");
  });

  it("is physically left-to-right so RTL languages cannot flip the layout", () => {
    expect(render(connected([event("a", 10, 40)]))).toContain('dir="ltr"');
  });
});

describe("CalendarView day labels", () => {
  const localNow = new Date(2026, 9, 6, 10, 0).getTime();
  const at = (day: number, hour: number) => new Date(2026, 9, 6 + day, hour, 0).toISOString();
  const local = (id: string, startDay: number, startHour: number, extra: Partial<CalendarEventDto> = {}) =>
    event(id, 0, 0, { startUtc: at(startDay, startHour), endUtc: at(startDay, startHour + 1), ...extra });
  const renderLocal = (events: CalendarEventDto[]) => renderToStaticMarkup(<CalendarView snapshot={connected(events)} nowMs={localNow} />);

  it("says nothing for today, 'Tomorrow' for the next day and the weekday after that", () => {
    const today = renderLocal([local("t", 0, 14)]);
    expect(today).not.toContain("Tomorrow");

    const html = renderLocal([local("a", 1, 9), local("b", 1, 11), local("c", 2, 9)]);
    expect(html).toContain("Tomorrow, ");
    expect(html).toMatch(/>Tomorrow</);
    expect(html).toContain(">Thursday<");
  });
});
