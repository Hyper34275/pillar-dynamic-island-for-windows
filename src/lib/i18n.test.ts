import { describe, expect, it } from "vitest";
import { detectLocale, isRtl, t, type MessageKey } from "./i18n";

describe("detectLocale", () => {
  it("maps Hebrew tags (including the legacy iw code) to he", () => {
    expect(detectLocale("he")).toBe("he");
    expect(detectLocale("he-IL")).toBe("he");
    expect(detectLocale("iw")).toBe("he");
    expect(detectLocale("HE_il")).toBe("he");
  });

  it("falls back to English for everything else, including missing input", () => {
    expect(detectLocale("en-GB")).toBe("en");
    expect(detectLocale("de-DE")).toBe("en");
    expect(detectLocale("ja")).toBe("en");
    expect(detectLocale("")).toBe("en");
    expect(detectLocale(undefined)).toBe("en");
  });
});

describe("isRtl", () => {
  it("is true for right-to-left scripts", () => {
    for (const tag of ["he", "he-IL", "ar", "ar-EG", "fa-IR", "ur"]) expect(isRtl(tag)).toBe(true);
  });
  it("is false for left-to-right and missing tags", () => {
    for (const tag of ["en", "en-US", "de", "ja", "", null, undefined]) expect(isRtl(tag)).toBe(false);
  });
});

describe("t", () => {
  it("returns the string for the requested locale", () => {
    expect(t("tab.about", undefined, "en")).toBe("About");
    expect(t("tab.about", undefined, "he")).toBe("אודות");
    expect(t("tab.settings", undefined, "he")).toBe("הגדרות");
  });

  it("interpolates parameters and leaves unknown placeholders alone", () => {
    expect(t("calendar.code", { code: "OUTLOOK-102" }, "en")).toBe("Code OUTLOOK-102");
    expect(t("calendar.code", {}, "en")).toBe("Code {code}");
  });

  it("covers every key the UI relies on in both languages", () => {
    const required: MessageKey[] = [
      "tab.calendar", "tab.about", "tab.settings",
      "calendar.next", "calendar.waiting", "calendar.noEvents", "calendar.newOutlook",
      "calendar.elevation", "calendar.unresponsive", "calendar.failed",
      "reminder.inMinutes", "reminder.startingNow",
      "about.computer", "about.ip", "about.diagnostics", "about.copy", "about.copied", "about.openLogs",
      "about.windowsUser", "about.os", "about.version", "about.outlook", "about.calendar",
      "about.cachedEvents", "about.lastSync", "about.notifications",
      "settings.launchWithWindows", "settings.hideInFullscreen", "settings.meetingReminders",
      "settings.reminderMinutes", "settings.notifications", "settings.meetingInvites",
      "invite.label", "about.time",
      "notif.allow", "notif.dismiss",
    ];
    for (const key of required) {
      for (const locale of ["en", "he"] as const) {
        const value = t(key, { n: 5 }, locale);
        expect(value, `${locale}:${key}`).toBeTruthy();
        expect(value).not.toMatch(/\{\w+\}/);
      }
    }
  });
});

describe("plurals", () => {
  it("English uses singular for 1 and plural otherwise", () => {
    expect(t("reminder.inMinutes", { n: 1 }, "en")).toBe("Meeting in 1 minute");
    expect(t("reminder.inMinutes", { n: 5 }, "en")).toBe("Meeting in 5 minutes");
    expect(t("reminder.inMinutes", { n: 0 }, "en")).toBe("Meeting in 0 minutes");
  });

  it("Hebrew has singular, dual and plural forms", () => {
    expect(t("reminder.inMinutes", { n: 1 }, "he")).toBe("פגישה בעוד דקה");
    expect(t("reminder.inMinutes", { n: 2 }, "he")).toBe("פגישה בעוד שתי דקות");
    expect(t("reminder.inMinutes", { n: 5 }, "he")).toBe("פגישה בעוד 5 דקות");
    expect(t("reminder.inMinutes", { n: 15 }, "he")).toBe("פגישה בעוד 15 דקות");
    expect(t("reminder.inMinutes", { n: 30 }, "he")).toBe("פגישה בעוד 30 דקות");
  });

  it("agrees with Intl.PluralRules for the categories it selects", () => {
    const he = new Intl.PluralRules("he");
    expect([1, 2, 11, 20].map((n) => he.select(n))).toEqual(["one", "two", "other", "other"]);
    const en = new Intl.PluralRules("en");
    expect([1, 2].map((n) => en.select(n))).toEqual(["one", "other"]);
  });

  it("has a separate 'starting now' message", () => {
    expect(t("reminder.startingNow", undefined, "en")).toBe("Meeting starting now");
    expect(t("reminder.startingNow", undefined, "he")).toBe("הפגישה מתחילה עכשיו");
  });
});
