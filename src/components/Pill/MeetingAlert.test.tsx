import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { formatTime } from "../../lib/dateFormat";
import type { ReminderAlert } from "../../lib/reminders/types";
import { alertSubjectLines } from "./alertLayout";
import { meetingAlertSize, notificationSize } from "./animations";
import { MeetingAlert, meetingAlertAnnouncement } from "./MeetingAlert";

const START = Date.UTC(2026, 9, 6, 10, 30);

function alertOf(extra: Partial<ReminderAlert> = {}): ReminderAlert {
  return {
    key: "k",
    eventId: "e",
    subject: "Quarterly planning",
    startUtc: new Date(START).toISOString(),
    endUtc: new Date(START + 45 * 60_000).toISOString(),
    location: "Room 12",
    minutesRemaining: 30,
    reminderType: { kind: "beforeStart", minutes: 30 },
    ...extra,
  };
}

const render = (alert: ReminderAlert) => renderToStaticMarkup(<MeetingAlert alert={alert} reducedMotion={false} />);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("MeetingAlert", () => {
  it("shows the label, the subject, the time range and the location", () => {
    const html = render(alertOf());
    expect(html).toContain("Meeting in 30 minutes");
    expect(html).toContain("Quarterly planning");
    expect(html).toContain(`${formatTime(new Date(START))} – ${formatTime(new Date(START + 45 * 60_000))}`);
    expect(html).toContain("Room 12");
  });

  it("uses singular, plural and 'starting now' wording from the real remaining time", () => {
    expect(render(alertOf({ minutesRemaining: 1 }))).toContain("Meeting in 1 minute<");
    expect(render(alertOf({ minutesRemaining: 12 }))).toContain("Meeting in 12 minutes");
    expect(render(alertOf({ minutesRemaining: 0 }))).toContain("Meeting starting now");
  });

  it("omits the location row when there is none", () => {
    expect(render(alertOf({ location: null }))).not.toContain("Room 12");
  });

  it("falls back to a placeholder for a blank subject", () => {
    expect(render(alertOf({ subject: "   " }))).toContain("(No subject)");
  });

  it("clamps a very long subject to two lines instead of growing", () => {
    const subject = "Cross-functional quarterly planning and roadmap alignment review ".repeat(6).trim();
    const html = render(alertOf({ subject }));
    expect(html).toContain("line-clamp-2");
    expect(html).toContain(subject);
  });

  it("is physically left-to-right with direction-neutral text, and never takes pointer events", () => {
    const html = render(alertOf({ subject: "פגישת צוות" }));
    expect(html).toContain('dir="ltr"');
    expect(html).toContain("pointer-events-none");
    expect(html).toContain('dir="auto"');
    expect(html).toContain("text-align:left");
  });

  it("announces label, subject, time and location in one sentence list", () => {
    expect(meetingAlertAnnouncement(alertOf())).toBe(
      `Meeting in 30 minutes. Quarterly planning. ${formatTime(new Date(START))} – ${formatTime(new Date(START + 45 * 60_000))}. Room 12`
    );
    expect(meetingAlertAnnouncement(alertOf({ location: null, minutesRemaining: 0 }))).toMatch(/^Meeting starting now\. Quarterly planning\./);
  });

  it("renders Hebrew copy with Hebrew plural forms when the UI language is Hebrew", async () => {
    vi.resetModules();
    vi.stubGlobal("navigator", { language: "he-IL" });
    const [{ renderToStaticMarkup: renderHe }, { MeetingAlert: AlertHe }] = await Promise.all([import("react-dom/server"), import("./MeetingAlert")]);
    const html = (minutesRemaining: number) =>
      renderHe(<AlertHe alert={alertOf({ minutesRemaining, subject: "ישיבת צוות" })} reducedMotion={false} />);
    expect(html(30)).toContain("פגישה בעוד 30 דקות");
    expect(html(2)).toContain("פגישה בעוד שתי דקות");
    expect(html(1)).toContain("פגישה בעוד דקה");
    expect(html(0)).toContain("הפגישה מתחילה עכשיו");
    expect(html(30)).toContain("ישיבת צוות");
    expect(html(30)).toContain('dir="ltr"'); // layout stays physically LTR
  });
});

describe("alert geometry", () => {
  it("counts one line for a short subject and two for a long one", () => {
    expect(alertSubjectLines("Standup")).toBe(1);
    expect(alertSubjectLines("A subject that is certainly long enough to need a second line to be read in full")).toBe(2);
  });

  it("grows with a second subject line and with a location, with a radius that never exceeds half the height", () => {
    const base = meetingAlertSize(1, false);
    expect(meetingAlertSize(2, false).height).toBeGreaterThan(base.height);
    expect(meetingAlertSize(1, true).height).toBeGreaterThan(base.height);
    for (const size of [base, meetingAlertSize(2, true)]) {
      expect(size.radius).toBeLessThanOrEqual(size.height / 2);
      expect(size.width).toBeGreaterThan(300);
    }
  });

  it("sizes the notification island by whether it has a body", () => {
    expect(notificationSize(true).height).toBeGreaterThan(notificationSize(false).height);
  });
});
