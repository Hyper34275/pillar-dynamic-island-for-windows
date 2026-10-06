// Typed wrappers around the Rust commands and events of docs/ENTERPRISE_DESIGN.md section 1.
//
// Nothing here throws into React: outside Tauri, or when a command fails, calls resolve to
// null / false / a fallback and the failure is logged by code only (never message text).

import { listen } from "@tauri-apps/api/event";
import { isTauriAvailable, tauriInvoke, type InvokeOptions } from "./tauri";
import { dlog } from "./debugLog";
import { describeError } from "./errors";

// -----------------------------------------------------------------------------
// Types (camelCase over IPC)
// -----------------------------------------------------------------------------

export type SystemInfo = {
  computerName: string;
  localIpv4: string | null;
  ipAdapter: string | null;
  windowsUser: string; // "DOMAIN\\USER"
  sessionId: number;
  osName: string;
  osDisplayVersion: string | null; // "21H2"
  osBuild: number; // 19044
  appVersion: string;
  webview2Version: string | null;
};

export type OutlookMode = "classic" | "new" | "none";

/** Privacy-safe runtime diagnostics. Every field is optional on the wire. */
export type Diagnostics = {
  outlookRunning: boolean | null;
  outlookMode: OutlookMode | null;
  recentErrorCodes: string[];
};

export type NotificationStatus = "allowed" | "denied" | "unspecified" | "unsupported" | "policy" | "error";

export const REMINDER_MINUTE_OPTIONS = [5, 10, 15, 30] as const;

export type Settings = {
  launchWithWindows: boolean;
  hideInFullscreen: boolean;
  meetingReminderEnabled: boolean;
  reminderMinutes: number;
  notificationsEnabled: boolean;
  /** Display the island lives on; null = primary. */
  monitorId: string | null;
};

export type SettingsPatch = Partial<Settings>;

export const SETTINGS_DEFAULTS: Settings = {
  launchWithWindows: true,
  hideInFullscreen: true,
  meetingReminderEnabled: true,
  reminderMinutes: 30,
  notificationsEnabled: true,
  monitorId: null,
};

export type MonitorInfo = {
  id: string;
  name: string;
  isPrimary: boolean;
};

/** Logical px; `radius` is the island's corner radius, used to clip the native window. */
export type IslandGeometry = {
  width: number;
  height: number;
  radius?: number;
};

/** A notification as delivered by the backend. Held in memory only, never persisted or logged. */
export type IslandNotification = {
  id: number;
  appName: string;
  title: string;
  body: string;
  timestamp: number;
  aumid: string | null;
};

// -----------------------------------------------------------------------------
// Normalisers: the UI never trusts the wire shape
// -----------------------------------------------------------------------------

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

export function normalizeSettings(raw: unknown): Settings {
  const r = isRecord(raw) ? raw : {};
  const minutes = typeof r.reminderMinutes === "number" && Number.isFinite(r.reminderMinutes) ? Math.round(r.reminderMinutes) : null;
  return {
    launchWithWindows: bool(r.launchWithWindows, SETTINGS_DEFAULTS.launchWithWindows),
    hideInFullscreen: bool(r.hideInFullscreen, SETTINGS_DEFAULTS.hideInFullscreen),
    meetingReminderEnabled: bool(r.meetingReminderEnabled, SETTINGS_DEFAULTS.meetingReminderEnabled),
    reminderMinutes: minutes !== null && minutes > 0 ? minutes : SETTINGS_DEFAULTS.reminderMinutes,
    notificationsEnabled: bool(r.notificationsEnabled, SETTINGS_DEFAULTS.notificationsEnabled),
    monitorId: typeof r.monitorId === "string" ? r.monitorId : typeof r.monitorId === "number" ? String(r.monitorId) : null,
  };
}

function normalizeDiagnostics(raw: unknown): Diagnostics {
  const r = isRecord(raw) ? raw : {};
  const mode = r.outlookMode;
  return {
    outlookRunning: typeof r.outlookRunning === "boolean" ? r.outlookRunning : null,
    outlookMode: mode === "classic" || mode === "new" || mode === "none" ? mode : null,
    recentErrorCodes: Array.isArray(r.recentErrorCodes) ? r.recentErrorCodes.filter((c): c is string => typeof c === "string") : [],
  };
}

function normalizeMonitors(raw: unknown): MonitorInfo[] {
  if (!Array.isArray(raw)) return [];
  const out: MonitorInfo[] = [];
  for (const item of raw) {
    if (!isRecord(item)) continue;
    const id = typeof item.id === "string" || typeof item.id === "number" ? String(item.id) : null;
    if (id === null) continue;
    out.push({
      id,
      name: typeof item.name === "string" ? item.name : "",
      isPrimary: item.isPrimary === true,
    });
  }
  return out;
}

/** Accepts the snake_case payload of the original backend as well as camelCase. */
export function normalizeNotification(raw: unknown): IslandNotification | null {
  if (!isRecord(raw) || typeof raw.id !== "number") return null;
  const str = (a: unknown, b: unknown): string => (typeof a === "string" ? a : typeof b === "string" ? b : "");
  const aumid = raw.aumid;
  return {
    id: raw.id,
    appName: str(raw.appName, raw.app_name),
    title: str(raw.title, null),
    body: str(raw.body, null),
    timestamp: typeof raw.timestamp === "number" ? raw.timestamp : Date.now(),
    aumid: typeof aumid === "string" && aumid ? aumid : null,
  };
}

export function normalizeReminderState(raw: unknown): Record<string, number> {
  const out: Record<string, number> = {};
  if (!isRecord(raw)) return out;
  for (const [key, value] of Object.entries(raw)) {
    if (typeof value === "number" && Number.isFinite(value)) out[key] = value;
  }
  return out;
}

const NOTIFICATION_STATUSES: readonly NotificationStatus[] = ["allowed", "denied", "unspecified", "unsupported", "policy", "error"];

export function normalizeNotificationStatus(raw: unknown): NotificationStatus | null {
  const value = isRecord(raw) ? raw.status : raw;
  return NOTIFICATION_STATUSES.includes(value as NotificationStatus) ? (value as NotificationStatus) : null;
}

// -----------------------------------------------------------------------------
// Call helpers
// -----------------------------------------------------------------------------

async function call<T>(cmd: string, args?: Record<string, unknown>, options?: InvokeOptions): Promise<T | null> {
  try {
    return await tauriInvoke<T>(cmd, args, { silent: true, ...options });
  } catch (error) {
    dlog("warn", "ipc", `${cmd} failed: ${describeError(error)}`);
    return null;
  }
}

/** Unit-returning command: true when it ran, false outside Tauri or on failure. */
async function callVoid(cmd: string, args?: Record<string, unknown>, options?: InvokeOptions): Promise<boolean> {
  if (!isTauriAvailable()) return false;
  try {
    await tauriInvoke(cmd, args, { silent: true, ...options });
    return true;
  } catch (error) {
    dlog("warn", "ipc", `${cmd} failed: ${describeError(error)}`);
    return false;
  }
}

export const ipc = {
  getSystemInfo: () => call<SystemInfo>("get_system_info"),

  async getDiagnostics(): Promise<Diagnostics | null> {
    const raw = await call<unknown>("get_diagnostics");
    return raw === null ? null : normalizeDiagnostics(raw);
  },

  copyTextToClipboard: (text: string) => callVoid("copy_text_to_clipboard", { text }),
  openLogDir: () => callVoid("open_log_dir"),

  async getSettings(): Promise<Settings | null> {
    const raw = await call<unknown>("get_settings");
    return raw === null ? null : normalizeSettings(raw);
  },

  async updateSettings(patch: SettingsPatch): Promise<Settings | null> {
    const raw = await call<unknown>("update_settings", { patch });
    return raw === null ? null : normalizeSettings(raw);
  },

  async notificationsGetStatus(): Promise<NotificationStatus | null> {
    return normalizeNotificationStatus(await call<unknown>("notifications_get_status"));
  },

  /** Only ever from an explicit user click. */
  async notificationsRequestAccess(): Promise<NotificationStatus | null> {
    return normalizeNotificationStatus(await call<unknown>("notifications_request_access"));
  },

  activateNotification: (id: number) => callVoid("activate_notification", { id }),
  activateAppByAumid: (aumid: string) => callVoid("activate_app_by_aumid", { aumid }),

  /** Arguments are passed flat: invoke("set_island_geometry", { width, height, radius }). */
  setIslandGeometry: (geometry: IslandGeometry) => callVoid("set_island_geometry", { ...geometry }, { timeoutMs: 3000 }),
  setClickThrough: (ignore: boolean) => callVoid("set_click_through", { ignore }),

  /** Raw snapshot: the calendar provider owns normalisation. */
  calendarGetSnapshot: () => call<unknown>("calendar_get_snapshot"),
  calendarRefresh: () => callVoid("calendar_refresh"),

  /** key -> firedAtUnixMs. Null when the backend is unavailable. */
  async reminderStateLoad(): Promise<Record<string, number> | null> {
    const raw = await call<unknown>("reminder_state_load");
    return raw === null ? null : normalizeReminderState(raw);
  },
  reminderStateSave: (map: Record<string, number>) => callVoid("reminder_state_save", { map }),

  async getMonitors(): Promise<MonitorInfo[] | null> {
    const raw = await call<unknown>("get_monitors");
    return raw === null ? null : normalizeMonitors(raw);
  },
};

// -----------------------------------------------------------------------------
// Events
// -----------------------------------------------------------------------------

/** Subscribes to a backend event; returns a disposer that is safe to call at any time. */
export function onEvent<T>(name: string, handler: (payload: T) => void): () => void {
  if (!isTauriAvailable()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | null = null;
  listen<T>(name, (event) => {
    if (!disposed) handler(event.payload);
  })
    .then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    })
    .catch(() => {
      // event system unavailable: nothing to subscribe to
    });
  return () => {
    disposed = true;
    unlisten?.();
    unlisten = null;
  };
}
