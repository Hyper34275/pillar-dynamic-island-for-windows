import { APP_NAME, APP_VERSION } from "./appInfo";
import type { Diagnostics, NotificationStatus, OutlookMode, SystemInfo } from "./ipc";
import type { CalendarSnapshot } from "./calendar/types";

export type NotificationDiagnostic = NotificationStatus | "off";

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

export interface DiagnosticsTextInput {
  info: SystemInfo | null;
  diagnostics: Diagnostics | null;
  snapshot: CalendarSnapshot;
  notifications: NotificationDiagnostic | null;
  generatedAt: Date;
}

const NA = "n/a";

function iso(unixMs: number | null): string {
  return unixMs === null ? NA : new Date(unixMs).toISOString();
}

/**
 * The block copied by "Copy diagnostics". Support-facing, so it is English and unlocalised.
 * Privacy: system identifiers and counts only — never meeting subjects, locations,
 * organizers or notification content.
 */
export function buildDiagnosticsText({ info, diagnostics, snapshot, notifications, generatedAt }: DiagnosticsTextInput): string {
  const lines = [
    `${APP_NAME} diagnostics`,
    `Generated: ${generatedAt.toISOString()}`,
    `App version: ${info?.appVersion ?? APP_VERSION}`,
    `Windows user: ${info?.windowsUser ?? NA}`,
    `Computer: ${info?.computerName ?? NA}`,
    `Local IP: ${info?.localIpv4 ?? NA}${info?.ipAdapter ? ` (${info.ipAdapter})` : ""}`,
    `OS: ${formatOs(info) ?? NA}`,
    `WebView2: ${info?.webview2Version ?? NA}`,
    `Session ID: ${info?.sessionId ?? NA}`,
    `Outlook: ${outlookRunningOf(snapshot, diagnostics) ? "running" : "not running"}`,
    `Outlook mode: ${outlookModeOf(snapshot, diagnostics)}`,
    `Calendar status: ${snapshot.status}${snapshot.errorCode ? ` (${snapshot.errorCode})` : ""}`,
    `Cached events: ${snapshot.cachedCount}`,
    `Last calendar sync: ${iso(snapshot.lastSyncUnixMs)}`,
    `Notifications: ${notifications ?? NA}`,
  ];
  const codes = diagnostics?.recentErrorCodes ?? [];
  if (codes.length > 0) lines.push(`Recent error codes: ${codes.join(", ")}`);
  return lines.join("\r\n");
}
