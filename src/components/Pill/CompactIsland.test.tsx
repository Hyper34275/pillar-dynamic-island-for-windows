import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { CalendarEventDto } from "../../lib/calendar/types";
import type { MeetingStatus } from "../../lib/calendar/meetingStatus";
import { pillDimensions } from "./animations";
import { CompactIsland } from "./CompactIsland";
import { compactContentSize, type CompactContent } from "./useCompactLayout";

const labels = { date: "6.10", weekday: "Tuesday", contentWidth: 90 };

function content(extra: Partial<CompactContent> = {}): CompactContent {
  const merged = { labels, status: null, statusText: null, unseen: 0, silent: false, ...extra };
  return { ...merged, size: compactContentSize(merged.labels, merged.statusText ? 150 : null, merged.unseen, merged.silent) };
}

const render = (weekday: string, unseen = 0) => renderToStaticMarkup(<CompactIsland content={content({ labels: { ...labels, weekday }, unseen })} />);

const meeting: CalendarEventDto = {
  id: "m",
  calendarId: "c",
  subject: "Standup",
  startUtc: "2026-10-06T10:00:00Z",
  endUtc: "2026-10-06T10:30:00Z",
  allDay: false,
  location: null,
  organizer: null,
  isRecurring: false,
  meetingUrl: null,
  busyStatus: "busy",
  responseStatus: "accepted",
  color: "#A34E78",
};

describe("CompactIsland", () => {
  it("puts the date on the left and the weekday on the right, whatever the language", () => {
    for (const weekday of ["Tuesday", "יום שלישי", "الثلاثاء"]) {
      const html = render(weekday);
      expect(html.indexOf("6.10")).toBeLessThan(html.indexOf(weekday));
      expect(html).toContain('dir="ltr"');
    }
  });

  it("isolates each label as LTR and lets the text inside choose its own direction", () => {
    const html = render("יום שלישי");
    expect(html.match(/direction:ltr;unicode-bidi:isolate/g)).toHaveLength(2);
    expect(html.match(/<span dir="auto">/g)).toHaveLength(2);
  });

  it("shows the unseen count inside the island, after the weekday, never hanging over its edge", () => {
    expect(render("Tuesday")).not.toContain(">3<");
    const html = render("Tuesday", 3);
    expect(html).toContain(">3<");
    expect(html.indexOf("Tuesday")).toBeLessThan(html.indexOf(">3<"));
    expect(html).not.toMatch(/right-\[-\d+px\]|position:absolute[^>]*>3</);
    expect(render("Tuesday", 12)).toContain(">9+<");
  });

  it("makes room for the count instead of squeezing the weekday", () => {
    const without = compactContentSize(labels, null, 0, false).width;
    expect(compactContentSize(labels, null, 3, false).width - without).toBe(pillDimensions.badge.gap + pillDimensions.badge.size);
    expect(compactContentSize(labels, null, 12, false).width - without).toBe(pillDimensions.badge.gap + pillDimensions.badge.wide);
  });

  it("shows a meeting in progress with its color, the end time, a progress bar and the silent bell", () => {
    const status: MeetingStatus = { kind: "now", event: meeting, endMs: Date.parse(meeting.endUtc), progress: 0.5 };
    const html = renderToStaticMarkup(<CompactIsland content={content({ status, statusText: "In a meeting until 13:30", silent: true })} />);
    expect(html).toContain("In a meeting until 13:30");
    expect(html).toContain("background:#A34E78");
    expect(html).toContain("width:50%");
    expect(html).not.toContain("6.10");
  });

  it("is wider for a meeting status, up to its own limit", () => {
    expect(compactContentSize(labels, 2000, 0, false).width).toBe(pillDimensions.compactMeeting.maxWidth);
  });
});
