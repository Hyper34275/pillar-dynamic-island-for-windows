import { APP_NAME, APP_VERSION } from "./appInfo";
import type { Diagnostics, NotificationStatus, OutlookMode, SystemInfo } from "./ipc";
import type { CalendarSnapshot } from "./calendar/types";

export type NotificationDiagnostic = NotificationStatus | "off";

const NA = "n/a";

/** Outlook mode from the backend when it reports one, otherwise inferred from the calendar status. */
export function outlookModeOf(snapshot: CalendarSnapshot, diagnostics: Diagnostics | null): OutlookMode {
  if (diagnostics?.outlookMode) return diagnostics.outlookMode;
  if (snapshot.status === "waiting") return "none";
  return snapshot.status === "newOutlookOnly" ? "new" : "classic";
}

export function outlookRunningOf(snapshot: CalendarSnapshot, diagnostics: Diagnostics | null): boolean {
  return diagnostics?.outlookRunning ?? snapshot.status !== "waiting";
}

export function formatOs(info: SystemInfo | null): string | null {
  if (!info) return null;
  const version = info.osDisplayVersion ? ` ${info.osDisplayVersion}` : "";
  return `${info.osName}${version} (build ${info.osBuild})`;
}

/** Outlook process / attach state in the words support staff expect. */
export function outlookConnectionOf(snapshot: CalendarSnapshot): string {
  switch (snapshot.status) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting";
    case "waiting":
      return "Waiting for Outlook";
    case "newOutlookOnly":
      return "New Outlook only (unsupported)";
    default:
      return "Connection Failed";
  }
}

export function calendarStateOf(snapshot: CalendarSnapshot): string {
  switch (snapshot.status) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting";
    case "waiting":
      return "Waiting for Outlook";
    default:
      return "Unavailable";
  }
}

/** The internal error code worth showing: none while Outlook is simply not running or all is well. */
export function internalErrorOf(snapshot: CalendarSnapshot): string | null {
  return snapshot.status === "waiting" || snapshot.status === "connected" ? null : snapshot.errorCode;
}

const NOTIFICATION_CODES: Partial<Record<NotificationStatus, string>> = {
  denied: "NOTIF-201",
  unspecified: "NOTIF-202",
  policy: "NOTIF-202",
  unsupported: "NOTIF-203",
  error: "NOTIF-204",
};

/** Mirrors the backend's status-to-code table (notifications.rs). */
export function notificationCodeOf(status: NotificationDiagnostic | null): string | null {
  return status === null || status === "off" ? null : (NOTIFICATION_CODES[status] ?? null);
}

const NOTIFICATION_WORDS: Record<NotificationDiagnostic, string> = {
  allowed: "Available",
  denied: "Denied in Windows settings",
  unspecified: "Not allowed yet",
  unsupported: "Unsupported",
  policy: "Restricted by policy",
  error: "Unavailable",
  off: "Off",
};

export function notificationWordsOf(status: NotificationDiagnostic | null): string {
  if (status === null) return NA;
  const code = notificationCodeOf(status);
  return code ? `${NOTIFICATION_WORDS[status]} (${code})` : NOTIFICATION_WORDS[status];
}

export interface DiagnosticsTextInput {
  info: SystemInfo | null;
  diagnostics: Diagnostics | null;
  snapshot: CalendarSnapshot;
  notifications: NotificationDiagnostic | null;
  generatedAt: Date;
}

function iso(unixMs: number | null): string {
  return unixMs === null ? NA : new Date(unixMs).toISOString();
}

/**
 * The block copied by "Copy diagnostics": a one-line summary first (what a ticket needs),
 * then every field. Support-facing, so it is English and unlocalised.
 * Privacy: system identifiers and counts only — never meeting subjects, locations,
 * organizers or notification content.
 */
export function buildDiagnosticsText({ info, diagnostics, snapshot, notifications, generatedAt }: DiagnosticsTextInput): string {
  const error = internalErrorOf(snapshot);
  const os = info ? `${info.osName}${info.osDisplayVersion ? ` ${info.osDisplayVersion}` : ""} Build ${info.osBuild}` : NA;
  const summary = [
    `${APP_NAME} ${info?.appVersion ?? APP_VERSION}`,
    os,
    `Computer: ${info?.computerName ?? NA}`,
    `User: ${info?.windowsUser ?? NA}`,
    `Outlook: ${outlookConnectionOf(snapshot)}${error ? ` (Internal Error: ${error})` : ""}`,
    `Calendar: ${calendarStateOf(snapshot)}`,
    `Cached events: ${snapshot.cachedCount}`,
    `Notifications: ${notificationWordsOf(notifications)}`,
  ].join(" / ");

  const lines = [
    summary,
    "",
    `${APP_NAME} diagnostics`,
    `Generated: ${generatedAt.toISOString()}`,
    `App version: ${info?.appVersion ?? APP_VERSION}`,
    `Windows user: ${info?.windowsUser ?? NA}`,
    `Computer: ${info?.computerName ?? NA}`,
    `Local IP: ${info?.localIpv4 ?? NA}${info?.ipAdapter ? ` (${info.ipAdapter})` : ""}`,
    `OS: ${formatOs(info) ?? NA}`,
    `WebView2: ${info?.webview2Version ?? NA}`,
    `Session ID: ${info?.sessionId ?? NA}`,
    `Outlook process: ${outlookRunningOf(snapshot, diagnostics) ? "running" : "not running"}`,
    `Outlook mode: ${outlookModeOf(snapshot, diagnostics)}`,
    `Calendar status: ${snapshot.status}${snapshot.errorCode ? ` (${snapshot.errorCode})` : ""}`,
    `Last calendar sync: ${iso(snapshot.lastSyncUnixMs)}`,
    `Notifications: ${notifications ?? NA}`,
    `Notification delivery: ${diagnostics?.notificationMode ?? NA}`,
  ];
  const codes = diagnostics?.recentErrorCodes ?? [];
  if (codes.length > 0) lines.push(`Recent error codes: ${codes.join(", ")}`);
  return lines.join("\r\n");
}
