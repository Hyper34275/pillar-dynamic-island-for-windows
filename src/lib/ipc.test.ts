import { describe, expect, it } from "vitest";
import {
  clipNoteText,
  ipc,
  normalizeNote,
  normalizeNotes,
  normalizeNotification,
  normalizeNotificationStatus,
  normalizePrefetchStatus,
  normalizeReminderState,
  normalizeSettings,
  NOTE_MAX_CHARS,
  NOTES_MAX,
  SETTINGS_DEFAULTS,
  type Note,
} from "./ipc";

describe("outside Tauri", () => {
  it("never throws and resolves to null / false", async () => {
    await expect(ipc.getSystemInfo()).resolves.toBeNull();
    await expect(ipc.getDiagnostics()).resolves.toBeNull();
    await expect(ipc.getSettings()).resolves.toBeNull();
    await expect(ipc.updateSettings({ launchWithWindows: false })).resolves.toBeNull();
    await expect(ipc.getMonitors()).resolves.toBeNull();
    await expect(ipc.notificationsGetStatus()).resolves.toBeNull();
    await expect(ipc.copyTextToClipboard("x")).resolves.toBe(false);
    await expect(ipc.openLogDir()).resolves.toBe(false);
    await expect(ipc.setIslandGeometry({ width: 1, height: 1 })).resolves.toBe(false);
    await expect(ipc.calendarGetSnapshot()).resolves.toBeNull();
    await expect(ipc.calendarRefresh()).resolves.toBe(false);
    await expect(ipc.reminderStateLoad()).resolves.toBeNull();
    await expect(ipc.reminderStateSave({ a: 1 })).resolves.toBe(false);
    await expect(ipc.calendarPrefetchStatus()).resolves.toBeNull();
    await expect(ipc.notesLoad()).resolves.toBeNull();
    await expect(ipc.notesSave([])).resolves.toBeNull();
    await expect(ipc.openCenter("settings")).resolves.toBe(false);
  });
});

describe("normalizeReminderState", () => {
  it("keeps numeric entries and drops everything else", () => {
    expect(normalizeReminderState({ a: 1, b: "2", c: null, d: Number.NaN, e: 0 })).toEqual({ a: 1, e: 0 });
  });
  it("returns an empty map for non-objects", () => {
    for (const raw of [null, undefined, 4, "x", []]) expect(normalizeReminderState(raw)).toEqual({});
  });
});

describe("normalizeSettings", () => {
  it("returns defaults for anything that is not an object", () => {
    for (const raw of [null, undefined, 3, "x", []]) expect(normalizeSettings(raw)).toEqual(SETTINGS_DEFAULTS);
  });

  it("keeps valid fields and replaces invalid ones with defaults", () => {
    expect(
      normalizeSettings({
        launchWithWindows: false,
        hideInFullscreen: "yes",
        meetingReminderEnabled: false,
        reminderMinutes: 15,
        notificationsEnabled: false,
        monitorId: 2,
      })
    ).toEqual({
      launchWithWindows: false,
      hideInFullscreen: true,
      meetingReminderEnabled: false,
      reminderMinutes: 15,
      notificationsEnabled: false,
      meetingInvitesEnabled: true,
      sharedCalendarReminders: true,
      meetingSilencePrompt: true,
      monitorId: "2",
      onboardingDone: false,
      islandDisplay: "full",
      aiSearchEnabled: true,
      aiSearchButton: true,
      aiSearchHotkey: true,
      calendarPrefetchDays: 7,
    });
  });

  it("defaults the schedule download to 7 days, keeps 0..31 rounded, clamps the rest and rejects non-numbers", () => {
    expect(SETTINGS_DEFAULTS.calendarPrefetchDays).toBe(7);
    expect(normalizeSettings({}).calendarPrefetchDays).toBe(7);
    expect(normalizeSettings({ calendarPrefetchDays: 0 }).calendarPrefetchDays).toBe(0);
    expect(normalizeSettings({ calendarPrefetchDays: 14 }).calendarPrefetchDays).toBe(14);
    expect(normalizeSettings({ calendarPrefetchDays: 13.6 }).calendarPrefetchDays).toBe(14);
    expect(normalizeSettings({ calendarPrefetchDays: 99 }).calendarPrefetchDays).toBe(31);
    expect(normalizeSettings({ calendarPrefetchDays: -4 }).calendarPrefetchDays).toBe(0);
    for (const bad of [Number.NaN, Infinity, "10", null, {}]) expect(normalizeSettings({ calendarPrefetchDays: bad }).calendarPrefetchDays).toBe(7);
  });

  it("normalises the prefetch status and answers null for nonsense", () => {
    expect(normalizePrefetchStatus(null)).toBeNull();
    expect(normalizePrefetchStatus("x")).toBeNull();
    expect(
      normalizePrefetchStatus({ days: 7, fromUnixMs: 1, toUnixMs: 2, fetchedUnixMs: "3", calendarsTotal: 4, calendarsRead: -1, failed: [["a", "CAL-1"], [1, 2], "z"] })
    ).toEqual({ days: 7, fromUnixMs: 1, toUnixMs: 2, fetchedUnixMs: null, calendarsTotal: 4, calendarsRead: 0, failed: [["a", "CAL-1"]] });
  });

  it("defaults the smart search switches to on and keeps explicit booleans", () => {
    expect(normalizeSettings({})).toMatchObject({ aiSearchEnabled: true, aiSearchButton: true, aiSearchHotkey: true });
    expect(normalizeSettings({ aiSearchEnabled: false, aiSearchButton: "no", aiSearchHotkey: false })).toMatchObject({
      aiSearchEnabled: false,
      aiSearchButton: true,
      aiSearchHotkey: false,
    });
  });

  it("defaults the 1.0.4 fields for settings files that predate them", () => {
    expect(SETTINGS_DEFAULTS.onboardingDone).toBe(false);
    expect(SETTINGS_DEFAULTS.islandDisplay).toBe("full");
    expect(normalizeSettings({ launchWithWindows: false })).toMatchObject({ onboardingDone: false, islandDisplay: "full" });
  });

  it("keeps onboardingDone and the three island displays, and maps anything else to full", () => {
    expect(normalizeSettings({ onboardingDone: true }).onboardingDone).toBe(true);
    expect(normalizeSettings({ onboardingDone: "yes" }).onboardingDone).toBe(false);
    for (const display of ["full", "clock", "date"] as const) expect(normalizeSettings({ islandDisplay: display }).islandDisplay).toBe(display);
    for (const bad of ["Clock", "", 3, null, {}]) expect(normalizeSettings({ islandDisplay: bad }).islandDisplay).toBe("full");
  });

  it("keeps 0 (remind at the start, as the backend allows) and rejects negative and non-finite reminder offsets", () => {
    expect(normalizeSettings({ reminderMinutes: 0 }).reminderMinutes).toBe(0);
    for (const bad of [-5, Number.NaN, Infinity, "10"]) {
      expect(normalizeSettings({ reminderMinutes: bad }).reminderMinutes).toBe(SETTINGS_DEFAULTS.reminderMinutes);
    }
  });
});

describe("normalizeNotification", () => {
  it("accepts camelCase and the original snake_case payload", () => {
    expect(normalizeNotification({ id: 1, appName: "Teams", title: "t", body: "b", timestamp: 5, aumid: "a!b" })).toEqual({
      id: 1,
      appName: "Teams",
      title: "t",
      body: "b",
      timestamp: 5,
      aumid: "a!b",
    });
    expect(normalizeNotification({ id: 2, app_name: "Mail", title: "t", body: "", timestamp: 6, aumid: null })).toMatchObject({
      id: 2,
      appName: "Mail",
      aumid: null,
    });
  });

  it("rejects payloads without a numeric id", () => {
    expect(normalizeNotification({ title: "x" })).toBeNull();
    expect(normalizeNotification(null)).toBeNull();
    expect(normalizeNotification("nope")).toBeNull();
  });
});

describe("normalizeNotificationStatus", () => {
  it("accepts a bare status or an object carrying one", () => {
    expect(normalizeNotificationStatus("allowed")).toBe("allowed");
    expect(normalizeNotificationStatus({ status: "policy" })).toBe("policy");
  });
  it("rejects unknown values", () => {
    expect(normalizeNotificationStatus("maybe")).toBeNull();
    expect(normalizeNotificationStatus(undefined)).toBeNull();
  });
});

const note = (over: Partial<Note> = {}): Note => ({ id: "a1", text: "hello", createdAt: 100, updatedAt: 200, pinned: false, ...over });

describe("normalizeNote", () => {
  it("keeps a valid note and the text exactly as typed", () => {
    expect(normalizeNote(note({ text: "  spaced \n text  ", pinned: true }))).toEqual(note({ text: "  spaced \n text  ", pinned: true }));
  });

  it("drops notes with a bad id, no text or a non-object shape", () => {
    for (const id of ["", "a b", "a/b", "x".repeat(65), 5, null]) expect(normalizeNote({ ...note(), id })).toBeNull();
    for (const text of ["", "   \n\t", 5, null]) expect(normalizeNote({ ...note(), text })).toBeNull();
    for (const raw of [null, undefined, "x", [], 3]) expect(normalizeNote(raw)).toBeNull();
    expect(normalizeNote({ ...note(), id: "x".repeat(64) })).not.toBeNull();
    expect(normalizeNote({ ...note(), id: "A_b-9" })).not.toBeNull();
  });

  it("repairs timestamps: non-positive means now, and updatedAt never precedes createdAt", () => {
    expect(normalizeNote({ ...note(), createdAt: 0, updatedAt: -1 }, 777)).toMatchObject({ createdAt: 777, updatedAt: 777 });
    expect(normalizeNote({ ...note(), createdAt: "x", updatedAt: Number.NaN }, 777)).toMatchObject({ createdAt: 777, updatedAt: 777 });
    expect(normalizeNote({ ...note(), createdAt: 500, updatedAt: 300 })).toMatchObject({ createdAt: 500, updatedAt: 500 });
  });

  it("treats a non-true pinned flag as false", () => {
    expect(normalizeNote({ ...note(), pinned: "true" })!.pinned).toBe(false);
    expect(normalizeNote({ ...note(), pinned: undefined })!.pinned).toBe(false);
  });

  it("truncates over-long text by Unicode characters, never inside a surrogate pair", () => {
    const long = "😀".repeat(NOTE_MAX_CHARS + 5);
    const clipped = normalizeNote({ ...note(), text: long })!.text;
    expect(Array.from(clipped)).toHaveLength(NOTE_MAX_CHARS);
    expect(clipped.endsWith("😀")).toBe(true);
    expect(clipNoteText("short")).toBe("short");
    // 10,000 UTF-16 units of BMP text is exactly at the limit and untouched.
    expect(clipNoteText("a".repeat(NOTE_MAX_CHARS))).toHaveLength(NOTE_MAX_CHARS);
  });
});

describe("normalizeNotes", () => {
  it("returns an empty list for anything that is not an array", () => {
    for (const raw of [null, undefined, {}, "x", 4]) expect(normalizeNotes(raw)).toEqual([]);
  });

  it("orders pinned first, then newest update, then id", () => {
    const result = normalizeNotes([
      note({ id: "b", updatedAt: 300 }),
      note({ id: "a", updatedAt: 300 }),
      note({ id: "old-pin", updatedAt: 100, pinned: true }),
      note({ id: "c", updatedAt: 400 }),
    ]);
    expect(result.map((n) => n.id)).toEqual(["old-pin", "c", "a", "b"]);
  });

  it("keeps the copy with the larger updatedAt for a duplicate id and skips invalid entries", () => {
    const result = normalizeNotes([note({ id: "d", text: "older", updatedAt: 200 }), { nope: true }, note({ id: "d", text: "newer", updatedAt: 900 })]);
    expect(result).toHaveLength(1);
    expect(result[0].text).toBe("newer");
  });

  it("keeps the 500 notes with the newest updatedAt", () => {
    const many = Array.from({ length: NOTES_MAX + 20 }, (_, i) => note({ id: `n${i}`, createdAt: 1, updatedAt: 1000 + i }));
    const result = normalizeNotes(many);
    expect(result).toHaveLength(NOTES_MAX);
    expect(result.some((n) => n.id === "n0")).toBe(false);
    expect(result[0].id).toBe(`n${NOTES_MAX + 19}`);
  });
});
