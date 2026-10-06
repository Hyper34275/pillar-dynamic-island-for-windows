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

  it("marks a meeting that already started as in progress", () => {
    const html = render(connected([event("now", -10, 20)]));
    expect(html).toContain("In progress");
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
