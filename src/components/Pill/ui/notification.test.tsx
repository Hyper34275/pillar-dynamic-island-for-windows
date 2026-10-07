import { afterEach, describe, expect, it } from "vitest";
import { setFixedLocale } from "../../../lib/i18n";
import type { IslandNotification } from "../../../lib/ipc";
import { notificationAccessibleLabel, notificationAnnouncement } from "./notification";

afterEach(() => setFixedLocale(null));

const note = (extra: Partial<IslandNotification> = {}): IslandNotification => ({
  id: 1,
  appName: "Microsoft Teams",
  title: "דנה כהן",
  body: "אפשר לעבור על המצגת לפני הפגישה?",
  timestamp: 0,
  aumid: null,
  ...extra,
});

describe("notificationAccessibleLabel", () => {
  it("reads as one sentence: kind, title, body, time", () => {
    setFixedLocale("en");
    expect(
      notificationAccessibleLabel({ source: "Teams", title: "Dana Cohen", body: "Can we review the presentation before the meeting?", received: "12 minutes ago" })
    ).toBe("Teams notification. Dana Cohen. Can we review the presentation before the meeting? 12 minutes ago.");
  });

  it("does not double punctuation and always ends with a full stop", () => {
    setFixedLocale("en");
    expect(notificationAccessibleLabel({ source: "Teams", title: "Done!", received: "now" })).toBe("Teams notification. Done! now.");
    expect(notificationAccessibleLabel({ source: "Teams", title: "Hi" })).toBe("Teams notification. Hi.");
  });

  it("opens an invitation with its own kind, and a Hebrew label with 'התראה מ-Teams'", () => {
    setFixedLocale("en");
    expect(notificationAccessibleLabel({ source: "Meeting invitation", title: "Standup", invite: true })).toBe("Meeting invitation. Standup.");
    setFixedLocale("he");
    expect(notificationAccessibleLabel({ source: "Teams", title: "דנה כהן" })).toBe("התראה מ-Teams. דנה כהן.");
  });
});

describe("notificationAnnouncement (the polite live-region text for a NEW notification)", () => {
  it("says who and what, politely, without the body", () => {
    setFixedLocale("he");
    expect(notificationAnnouncement(note())).toBe("התראה חדשה מ-Teams: דנה כהן");
    setFixedLocale("en");
    expect(notificationAnnouncement(note({ title: "Dana Cohen" }))).toBe("New notification from Teams: Dana Cohen");
    expect(notificationAnnouncement(note({ title: "Dana Cohen" }))).not.toContain("מצגת");
  });

  it("names an invitation as such and falls back to the default title", () => {
    setFixedLocale("en");
    expect(notificationAnnouncement(note({ title: "Standup", invite: { id: "i", startUtc: null } }))).toBe("New meeting invitation: Standup");
    expect(notificationAnnouncement(note({ title: "" }))).toBe("New notification from Teams: Notification");
  });
});
