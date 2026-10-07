import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { CalendarEventDto, CalendarSnapshot, MeetingInviteDto } from "../../../lib/calendar/types";
import type { HistoryEntry } from "../../../lib/notifications/history";
import { alertHasActions, canSnooze } from "../alertLayout";
import { control, toast } from "../../../design/tokens";
import { toastLayout } from "../toastLayout";
import { CalendarView, DayView } from "./CalendarTab";
import { layoutTimeline } from "./DayTimeline";
import { NotificationsView } from "./NotificationsTab";
import { addDays, clampDay, FUTURE_DAYS, PAST_DAYS, weekOf } from "./WeekStrip";

const DAY = new Date(2026, 9, 6).getTime(); // Tuesday 6 Oct 2026, local midnight
const at = (hour: number, minute = 0) => new Date(2026, 9, 6, hour, minute).toISOString();

function event(id: string, start: string, end: string, extra: Partial<CalendarEventDto> = {}): CalendarEventDto {
  return {
    id,
    calendarId: "c",
    subject: `Meeting ${id}`,
    startUtc: start,
    endUtc: end,
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

const snapshot = (events: CalendarEventDto[], invites: MeetingInviteDto[] = []): CalendarSnapshot => ({
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: DAY,
  cachedCount: events.length,
  nextRetryUnixMs: null,
  events,
  invites,
});

describe("layoutTimeline", () => {
  it("shows the working day, stretched by earlier or later meetings", () => {
    expect(layoutTimeline([event("a", at(10), at(11))], DAY)).toMatchObject({ fromHour: 8, toHour: 18, lanes: 1 });
    expect(layoutTimeline([event("a", at(6, 30), at(7)), event("b", at(19), at(20, 15))], DAY)).toMatchObject({ fromHour: 6, toHour: 21 });
  });

  it("places meetings by time and gives overlapping ones their own lane", () => {
    const { blocks, lanes } = layoutTimeline([event("a", at(9), at(10)), event("b", at(9, 30), at(11)), event("c", at(10), at(11))], DAY);
    expect(lanes).toBe(2);
    expect(blocks.map((b) => [b.event.id, b.lane])).toEqual([
      ["a", 0],
      ["b", 1],
      ["c", 0],
    ]);
    expect(blocks[0].left).toBeCloseTo(0.1); // 9:00 on 8..18
    expect(blocks[0].width).toBeCloseTo(0.1);
  });

  it("leaves all-day events out", () => {
    expect(layoutTimeline([event("h", at(0), new Date(2026, 9, 7).toISOString(), { allDay: true })], DAY).blocks).toEqual([]);
  });
});

describe("week strip days", () => {
  it("is Sunday to Saturday around the day, across a month end", () => {
    const week = weekOf(new Date(2026, 9, 30).getTime()); // Friday
    expect(week.map((d) => new Date(d).getDate())).toEqual([25, 26, 27, 28, 29, 30, 31]);
    expect(new Date(weekOf(DAY)[0]).getDay()).toBe(0);
    expect(new Date(addDays(new Date(2026, 9, 31).getTime(), 1)).getMonth()).toBe(10);
  });

  it("keeps browsing within two weeks back and two months ahead", () => {
    expect(clampDay(addDays(DAY, -30), DAY)).toBe(addDays(DAY, -PAST_DAYS));
    expect(clampDay(addDays(DAY, 400), DAY)).toBe(addDays(DAY, FUTURE_DAYS));
    expect(clampDay(addDays(DAY, 3), DAY)).toBe(addDays(DAY, 3));
  });
});

describe("Join button", () => {
  const now = new Date(2026, 9, 6, 10, 0).getTime();
  it("appears on the next meeting and on later meetings that have a join link", () => {
    const html = renderToStaticMarkup(
      <CalendarView
        snapshot={snapshot([
          event("next", at(10, 20), at(10, 50), { meetingUrl: "https://teams.microsoft.com/l/meetup-join/1" }),
          event("later", at(12), at(13), { meetingUrl: "https://zoom.us/j/1" }),
          event("plain", at(14), at(15)),
        ])}
        nowMs={now}
      />
    );
    expect(html).toContain(">Join<");
    expect(html.match(/aria-label="Join the meeting: /g)).toHaveLength(2);
    expect(html).not.toContain("Join the meeting: Meeting plain");
  });

  it("offers the silent bell only on a meeting in progress", () => {
    const running = renderToStaticMarkup(<CalendarView snapshot={snapshot([event("now", at(9, 30), at(10, 30))])} nowMs={now} />);
    expect(running).toContain("Silence notifications until the meeting ends");
    const silent = renderToStaticMarkup(<CalendarView snapshot={snapshot([event("now", at(9, 30), at(10, 30))])} nowMs={now} silenceUntil={now + 60_000} />);
    expect(silent).toContain("Turn notifications back on");
    const upcoming = renderToStaticMarkup(<CalendarView snapshot={snapshot([event("next", at(11), at(12))])} nowMs={now} />);
    expect(upcoming).not.toContain("Silence notifications");
  });
});

describe("pending invitations in the Calendar tab", () => {
  const invite: MeetingInviteDto = {
    id: "inv1",
    subject: "Design review",
    organizer: "Dana",
    startUtc: at(15),
    endUtc: at(16),
    location: null,
    receivedUtc: at(8),
  };
  it("lists them with accept / maybe / decline, and hides the ones already answered", () => {
    const html = renderToStaticMarkup(<CalendarView snapshot={snapshot([], [invite])} nowMs={DAY + 9 * 3_600_000} />);
    expect(html).toContain("Pending invitations");
    expect(html).toContain("Design review");
    expect(html).toContain("From Dana");
    for (const label of [">Accept<", ">Maybe<", ">Decline<"]) expect(html).toContain(label);
    const answered = renderToStaticMarkup(<CalendarView snapshot={snapshot([], [invite])} nowMs={DAY} answeredInvites={new Set(["inv1"])} />);
    expect(answered).not.toContain("Design review");
  });
});

describe("DayView (any day but today)", () => {
  const now = new Date(2026, 9, 6, 10, 0).getTime();
  it("says it is loading until the day is read", () => {
    expect(renderToStaticMarkup(<DayView nowMs={now} day={{ state: "loading", events: null }} status="connected" />)).toContain("Loading from Outlook");
  });

  it("lists the day's meetings with all-day ones first", () => {
    const events = [event("b", at(14), at(15)), event("holiday", at(0), new Date(2026, 9, 7).toISOString(), { allDay: true, subject: "Holiday" })];
    const html = renderToStaticMarkup(<DayView nowMs={now} day={{ state: "ready", events, fetchedAt: now }} status="connected" />);
    expect(html.indexOf("Holiday")).toBeLessThan(html.indexOf("Meeting b"));
  });

  it("says when a day has no meetings, and when it could not be read", () => {
    expect(renderToStaticMarkup(<DayView nowMs={now} day={{ state: "ready", events: [], fetchedAt: now }} status="connected" />)).toContain("No meetings this day");
    expect(renderToStaticMarkup(<DayView nowMs={now} day={{ state: "error", events: null }} status="connected" />)).toContain("Couldn&#x27;t load this day");
    expect(renderToStaticMarkup(<DayView nowMs={now} day={{ state: "error", events: null }} status="waiting" />)).toContain("Waiting for Outlook");
  });
});

describe("NotificationsView", () => {
  const noop = () => {};
  const entry = (id: number, extra: Partial<HistoryEntry> = {}, invite = false): HistoryEntry => ({
    notification: { id, appName: "Teams", title: `Title ${id}`, body: `Body ${id}`, timestamp: 0, aumid: null, ...(invite ? { invite: { id: "inv", startUtc: null } } : {}) },
    receivedAt: 0,
    silenced: false,
    ...extra,
  });

  it("is empty with a hint, or says notifications are off", () => {
    expect(renderToStaticMarkup(<NotificationsView entries={[]} nowMs={0} notificationsEnabled onActivate={noop} onRemove={noop} onClear={noop} />)).toContain("No notifications");
    expect(renderToStaticMarkup(<NotificationsView entries={[]} nowMs={0} notificationsEnabled={false} onActivate={noop} onRemove={noop} onClear={noop} />)).toContain("turned off");
  });

  it("lists each one with its app, title, body and age, newest first as given", () => {
    const html = renderToStaticMarkup(
      <NotificationsView entries={[entry(2, { receivedAt: 0 }), entry(1, { receivedAt: 0, silenced: true })]} nowMs={10 * 60_000} notificationsEnabled onActivate={noop} onRemove={noop} onClear={noop} />
    );
    expect(html.indexOf("Title 2")).toBeLessThan(html.indexOf("Title 1"));
    expect(html).toContain("Body 1");
    expect(html).toMatch(/10 min/);
    expect(html).toContain("Arrived during a silenced meeting");
  });

  it("shows answer buttons for a meeting invitation", () => {
    const html = renderToStaticMarkup(<NotificationsView entries={[entry(-1, {}, true)]} nowMs={0} notificationsEnabled onActivate={noop} onRemove={noop} onClear={noop} />);
    expect(html).toContain("Meeting invitation");
    expect(html).toContain(">Accept<");
  });
});

describe("meeting alert buttons and toast sizes", () => {
  const alert = { key: "k", eventId: "e", subject: "s", startUtc: at(10), endUtc: at(11), location: null, reminderType: { kind: "beforeStart" as const, minutes: 30 } };
  it("snooze only while more than five minutes are left; a button row with join or snooze", () => {
    expect(canSnooze({ ...alert, minutesRemaining: 30 })).toBe(true);
    expect(canSnooze({ ...alert, minutesRemaining: 5 })).toBe(false);
    expect(alertHasActions({ ...alert, minutesRemaining: 2 })).toBe(false);
    expect(alertHasActions({ ...alert, minutesRemaining: 2, meetingUrl: "https://zoom.us/j/1" })).toBe(true);
  });

  it("an invitation toast is taller by its row of buttons", () => {
    const plain = { id: 1, appName: "Outlook", title: "Standup", body: "Daily", timestamp: 0, aumid: null };
    const invite = { ...plain, invite: { id: "i1", startUtc: at(10) } };
    expect(toastLayout(invite).size.height - toastLayout(plain).size.height).toBe(toast.actionsGap + control.height);
  });
});
