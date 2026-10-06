import { describe, expect, it } from "vitest";
import { buildDiagnosticsText, formatOs, outlookModeOf, outlookRunningOf } from "./diagnostics";
import type { SystemInfo } from "./ipc";
import { WAITING_SNAPSHOT, type CalendarSnapshot } from "./calendar/types";

const INFO: SystemInfo = {
  computerName: "PC-0042",
  localIpv4: "10.20.30.40",
  ipAdapter: "Ethernet",
  windowsUser: "CORP\\jdoe",
  sessionId: 2,
  osName: "Windows 10",
  osDisplayVersion: "21H2",
  osBuild: 19044,
  appVersion: "1.0.0",
  webview2Version: "120.0.2210.91",
};

const CONNECTED: CalendarSnapshot = {
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: Date.UTC(2026, 9, 6, 9, 58, 0),
  cachedCount: 7,
  nextRetryUnixMs: null,
  events: [],
};

describe("buildDiagnosticsText", () => {
  const generatedAt = new Date(Date.UTC(2026, 9, 6, 10, 0, 0));

  it("renders every field from the contract's diagnostics list", () => {
    const text = buildDiagnosticsText({ info: INFO, diagnostics: null, snapshot: CONNECTED, notifications: "allowed", generatedAt });
    const lines = text.split("\r\n");
    expect(lines[0]).toBe("CompanyIsland diagnostics");
    expect(lines).toContain("Generated: 2026-10-06T10:00:00.000Z");
    expect(lines).toContain("App version: 1.0.0");
    expect(lines).toContain("Windows user: CORP\\jdoe");
    expect(lines).toContain("Computer: PC-0042");
    expect(lines).toContain("Local IP: 10.20.30.40 (Ethernet)");
    expect(lines).toContain("OS: Windows 10 21H2 (build 19044)");
    expect(lines).toContain("Outlook: running");
    expect(lines).toContain("Outlook mode: classic");
    expect(lines).toContain("Calendar status: connected");
    expect(lines).toContain("Cached events: 7");
    expect(lines).toContain("Last calendar sync: 2026-10-06T09:58:00.000Z");
    expect(lines).toContain("Notifications: allowed");
  });

  it("degrades to n/a instead of failing when data is unavailable", () => {
    const text = buildDiagnosticsText({ info: null, diagnostics: null, snapshot: WAITING_SNAPSHOT, notifications: null, generatedAt });
    expect(text).toContain("Computer: n/a");
    expect(text).toContain("Local IP: n/a");
    expect(text).toContain("OS: n/a");
    expect(text).toContain("Last calendar sync: n/a");
    expect(text).toContain("Outlook: not running");
    expect(text).toContain("Outlook mode: none");
    expect(text).toContain("Notifications: n/a");
  });

  it("includes the error code and recent codes, and never meeting content", () => {
    const snapshot: CalendarSnapshot = {
      ...CONNECTED,
      status: "failed",
      errorCode: "OUTLOOK-108",
      events: [
        {
          id: "abcdef0123456789",
          calendarId: "cal",
          subject: "Confidential: layoffs",
          startUtc: "2026-10-06T11:00:00Z",
          endUtc: "2026-10-06T12:00:00Z",
          allDay: false,
          location: "Room 5",
          organizer: "Boss",
          isRecurring: false,
          meetingUrl: null,
          busyStatus: "busy",
          responseStatus: "accepted",
        },
      ],
    };
    const text = buildDiagnosticsText({
      info: INFO,
      diagnostics: { outlookRunning: true, outlookMode: "classic", recentErrorCodes: ["OUTLOOK-105", "OUTLOOK-108"] },
      snapshot,
      notifications: "off",
      generatedAt,
    });
    expect(text).toContain("Calendar status: failed (OUTLOOK-108)");
    expect(text).toContain("Recent error codes: OUTLOOK-105, OUTLOOK-108");
    expect(text).toContain("Notifications: off");
    expect(text).not.toMatch(/layoffs|Room 5|Boss/);
  });
});

describe("outlook state helpers", () => {
  it("prefers the backend's report and otherwise infers from the calendar status", () => {
    expect(outlookModeOf({ ...CONNECTED, status: "newOutlookOnly" }, null)).toBe("new");
    expect(outlookModeOf(WAITING_SNAPSHOT, null)).toBe("none");
    expect(outlookModeOf(WAITING_SNAPSHOT, { outlookRunning: null, outlookMode: "classic", recentErrorCodes: [] })).toBe("classic");
    expect(outlookRunningOf(CONNECTED, null)).toBe(true);
    expect(outlookRunningOf(WAITING_SNAPSHOT, null)).toBe(false);
    expect(outlookRunningOf(WAITING_SNAPSHOT, { outlookRunning: true, outlookMode: null, recentErrorCodes: [] })).toBe(true);
  });
});

describe("formatOs", () => {
  it("omits the display version when the OS does not report one", () => {
    expect(formatOs({ ...INFO, osDisplayVersion: null })).toBe("Windows 10 (build 19044)");
    expect(formatOs(null)).toBeNull();
  });
});
