import { describe, expect, it } from "vitest";
import { ipc, normalizeNotification, normalizeNotificationStatus, normalizeReminderState, normalizeSettings, SETTINGS_DEFAULTS } from "./ipc";

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
      monitorId: "2",
    });
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
