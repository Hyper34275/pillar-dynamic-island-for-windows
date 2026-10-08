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
      "settings.reminderMinutes", "settings.notifications", "settings.meetingInvites", "settings.sharedCalendarReminders",
      "calendar.sources", "calendar.kindShared", "calendar.sourceNotSelected", "calendar.sourcesHint",
      "invite.label", "about.time",
      "notif.allow", "notif.dismiss",
      "tab.notes", "notes.new", "notes.empty", "notes.emptyHint", "notes.saveFailed", "notes.pinned", "notes.pin", "notes.unpin",
      "notes.copy", "notes.copied", "notes.delete", "notes.list",
      "settings.center", "settings.openCenter", "settings.tour",
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

describe("notes and Yuval Center strings", () => {
  it("are in Hebrew where the contract fixes the wording", () => {
    expect(t("tab.notes", undefined, "he")).toBe("פתקים");
    expect(t("notes.new", undefined, "he")).toBe("פתק חדש");
    expect(t("notes.copied", undefined, "he")).toBe("הועתק");
    expect(t("settings.openCenter", undefined, "he")).toBe("פתח את מרכז יובל");
    expect(t("settings.tour", undefined, "he")).toBe("סיור במערכת");
    expect(t("tab.notes", undefined, "en")).toBe("Notes");
  });

  it("call the product Yuval (יובל in Hebrew) and its second app the Yuval Center", () => {
    expect(t("settings.center", undefined, "en")).toBe("Yuval Center");
    expect(t("settings.openCenter", undefined, "en")).toBe("Open Yuval Center");
    expect(t("settings.center", undefined, "he")).toBe("מרכז יובל");
    expect(t("island.expandedLabel", { app: "Yuval" }, "en")).toBe("Yuval expanded");
    expect(t("island.expandedLabel", { app: "Yuval" }, "he")).toBe("יובל מורחב");
    expect(t("calendar.elevationHint", undefined, "en")).toContain("Outlook and Yuval");
    expect(t("calendar.elevationHint", undefined, "he")).toContain("ואת יובל");
    for (const locale of ["en", "he"] as const) {
      for (const key of ["settings.center", "settings.openCenter", "tour.openNotes", "tour.openSettings", "tour.s10.text", "tour.s12.text", "calendar.elevationHint"] as const) {
        const text = t(key, undefined, locale);
        expect(text, `${key} (${locale})`).not.toMatch(/CompanyIsland|Island Center|מרכז האי/);
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

describe("the collapsed island's display setting", () => {
  it("is labelled in both languages, in the order full, clock, date", () => {
    const keys = ["settings.display.full", "settings.display.clock", "settings.display.date"] as const;
    expect(keys.map((key) => t(key, undefined, "he"))).toEqual(["שעה, תאריך ויום", "שעה בלבד", "תאריך ויום"]);
    expect(keys.map((key) => t(key, undefined, "en"))).toEqual(["Time, date and day", "Time only", "Date and day"]);
    expect(t("settings.islandDisplay", undefined, "he")).toBe("האי המכווץ");
  });
});

describe("tour strings", () => {
  const keys: MessageKey[] = [
    "tour.pageTitle", "tour.step", "tour.prev", "tour.next", "tour.finish", "tour.dots", "tour.dot", "tour.autoplay", "tour.openNotes", "tour.openSettings",
    ...Array.from({ length: 12 }, (_, i) => [`tour.s${i + 1}.title`, `tour.s${i + 1}.text`] as MessageKey[]).flat(),
  ];

  it("exist in both languages with no placeholder left open", () => {
    for (const key of keys) {
      for (const locale of ["en", "he"] as const) {
        const value = t(key, { n: 3, total: 12, title: "x" }, locale);
        expect(value, `${locale}:${key}`).toBeTruthy();
        expect(value).not.toMatch(/\{\w+\}/);
      }
    }
  });

  it("are Hebrew in the Hebrew table, with the contract's wording for the title", () => {
    expect(t("tour.pageTitle", undefined, "he")).toBe("סיור במערכת");
    expect(t("tour.prev", undefined, "he")).toBe("הקודם");
    expect(t("tour.next", undefined, "he")).toBe("הבא");
    expect(t("tour.finish", undefined, "he")).toBe("סיום");
    for (let n = 1; n <= 12; n++) expect(t(`tour.s${n}.text` as MessageKey, undefined, "he")).toMatch(/[֐-׿]/);
  });
});
