// Typed wrappers around the Rust commands and events of docs/ENTERPRISE_DESIGN.md section 1.
//
// Nothing here throws into React: outside Tauri, or when a command fails, calls resolve to
// null / false / a fallback and the failure is logged by code only (never message text).

import { listen } from "@tauri-apps/api/event";
import { isTauriAvailable, tauriInvoke, type InvokeOptions } from "./tauri";
import { dlog } from "./debugLog";
import { describeError } from "./errors";
import { NO_LIMITS, parseIslandLimits, type IslandLimits } from "./island/limits";

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

/** How Windows toasts reach the island: change events, a read-only poll when events cannot be subscribed, or not at all. */
export type NotificationMode = "events" | "polling" | "none";

/** Privacy-safe runtime diagnostics. Every field is optional on the wire. */
export type Diagnostics = {
  outlookRunning: boolean | null;
  outlookMode: OutlookMode | null;
  notificationMode: NotificationMode | null;
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
  /** New Outlook meeting requests pop up in the island (and the Inbox is read for them). */
  meetingInvitesEnabled: boolean;
  /** When a meeting starts the island offers to silence notifications until it ends. */
  meetingSilencePrompt: boolean;
  /** Display the island lives on; null = primary. */
  monitorId: string | null;
  /** False until the Island Center's welcome page has been shown once (set by the backend on first run). */
  onboardingDone: boolean;
  /** What the collapsed island shows: date + clock + weekday, the time only, or date + weekday. */
  islandDisplay: IslandDisplay;
};

export type IslandDisplay = "full" | "clock" | "date";

const ISLAND_DISPLAYS: readonly IslandDisplay[] = ["full", "clock", "date"];

export type SettingsPatch = Partial<Settings>;

export const SETTINGS_DEFAULTS: Settings = {
  launchWithWindows: true,
  hideInFullscreen: true,
  meetingReminderEnabled: true,
  reminderMinutes: 30,
  notificationsEnabled: true,
  meetingInvitesEnabled: true,
  meetingSilencePrompt: true,
  monitorId: null,
  onboardingDone: false,
  islandDisplay: "full",
};

/** A note, kept only on this computer. Times are unix ms. */
export type Note = {
  id: string;
  text: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
};

/** Pages the Island Center can be opened on; `note:<id>` opens one note for editing. */
export type CenterPage = "welcome" | "tour" | "settings" | "notes" | "notes-new" | `note:${string}`;

/** How the user answers a meeting invitation from the island. */
export type InviteResponse = "accept" | "tentative" | "decline";

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
  /** The native stage window around every island shape (it includes the island's gap below the screen's top edge). */
  stageWidth?: number;
  stageHeight?: number;
};

/** A notification as delivered by the backend. Held in memory only, never persisted or logged. */
export type IslandNotification = {
  id: number;
  appName: string;
  title: string;
  body: string;
  timestamp: number;
  aumid: string | null;
  /**
   * Set when this is an Outlook meeting request rather than a mirrored Windows toast
   * (see useMeetingInvites). Activating it opens the Outlook calendar on the meeting's day;
   * `id` answers it (accept / decline).
   */
  invite?: { id: string; startUtc: string | null };
  /**
   * Set on the island's own "You missed N notifications" toast, shown when Do not disturb is
   * turned off (see lib/notifications/missed.ts). Never from the backend; activating it opens the
   * Notifications tab.
   */
  missedSummary?: { count: number };
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
    reminderMinutes: minutes !== null && minutes >= 0 ? minutes : SETTINGS_DEFAULTS.reminderMinutes,
    notificationsEnabled: bool(r.notificationsEnabled, SETTINGS_DEFAULTS.notificationsEnabled),
    meetingInvitesEnabled: bool(r.meetingInvitesEnabled, SETTINGS_DEFAULTS.meetingInvitesEnabled),
    meetingSilencePrompt: bool(r.meetingSilencePrompt, SETTINGS_DEFAULTS.meetingSilencePrompt),
    monitorId: typeof r.monitorId === "string" ? r.monitorId : typeof r.monitorId === "number" ? String(r.monitorId) : null,
    onboardingDone: bool(r.onboardingDone, SETTINGS_DEFAULTS.onboardingDone),
    islandDisplay: ISLAND_DISPLAYS.includes(r.islandDisplay as IslandDisplay) ? (r.islandDisplay as IslandDisplay) : SETTINGS_DEFAULTS.islandDisplay,
  };
}

/** Rust is authoritative; these are the same limits, so the UI never sends what the backend would drop. */
export const NOTE_MAX_CHARS = 10_000;
export const NOTES_MAX = 500;
const NOTE_ID = /^[A-Za-z0-9_-]{1,64}$/;

/** Truncates at a Unicode scalar boundary (never inside a surrogate pair). */
export function clipNoteText(text: string): string {
  if (text.length <= NOTE_MAX_CHARS) return text;
  const chars = Array.from(text);
  return chars.length <= NOTE_MAX_CHARS ? text : chars.slice(0, NOTE_MAX_CHARS).join("");
}

/** One wire note, or null when it is unusable (bad id, empty text). Timestamps are repaired, not trusted. */
export function normalizeNote(raw: unknown, nowMs: number = Date.now()): Note | null {
  if (!isRecord(raw)) return null;
  if (typeof raw.id !== "string" || !NOTE_ID.test(raw.id)) return null;
  if (typeof raw.text !== "string" || raw.text.trim() === "") return null;
  const time = (v: unknown): number => (typeof v === "number" && Number.isFinite(v) && v > 0 ? v : nowMs);
  const createdAt = time(raw.createdAt);
  return {
    id: raw.id,
    text: clipNoteText(raw.text),
    createdAt,
    updatedAt: Math.max(time(raw.updatedAt), createdAt),
    pinned: raw.pinned === true,
  };
}

/** Canonical order, as Rust returns it: pinned first, then newest update, then id. */
export function compareNotes(a: Note, b: Note): number {
  if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
  if (a.updatedAt !== b.updatedAt) return b.updatedAt - a.updatedAt;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** Valid notes only, duplicate ids resolved to the newest, at most 500, in canonical order. */
export function normalizeNotes(raw: unknown): Note[] {
  if (!Array.isArray(raw)) return [];
  const now = Date.now();
  const byId = new Map<string, Note>();
  for (const item of raw) {
    const note = normalizeNote(item, now);
    if (!note) continue;
    const existing = byId.get(note.id);
    if (!existing || note.updatedAt > existing.updatedAt) byId.set(note.id, note);
  }
  let notes = [...byId.values()];
  // Keep the 500 newest by update time, whatever their pin state, then present them canonically.
  if (notes.length > NOTES_MAX) notes = notes.sort((x, y) => y.updatedAt - x.updatedAt || (x.id < y.id ? -1 : 1)).slice(0, NOTES_MAX);
  return notes.sort(compareNotes);
}

function normalizeDiagnostics(raw: unknown): Diagnostics {
  const r = isRecord(raw) ? raw : {};
  const mode = r.outlookMode;
  return {
    outlookRunning: typeof r.outlookRunning === "boolean" ? r.outlookRunning : null,
    outlookMode: mode === "classic" || mode === "new" || mode === "none" ? mode : null,
    notificationMode: r.notificationMode === "events" || r.notificationMode === "polling" || r.notificationMode === "none" ? r.notificationMode : null,
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

  /** BCP-47 tag of the Windows regional format (dates, weekdays, times); null when unavailable. */
  getFormatLocale: () => call<string | null>("get_format_locale", undefined, { timeoutMs: 2000 }),

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

  /** Whether Windows "Do not disturb" is on (the user's choice); null when it can't be read. */
  dndGet: () => call<boolean>("dnd_get", undefined, { timeoutMs: 5000 }),

  /** Only ever from an explicit click on the bell. Returns the state after the change, null on failure. */
  dndSet: (on: boolean) => call<boolean>("dnd_set", { on }, { timeoutMs: 5000 }),

  activateNotification: (id: number) => callVoid("activate_notification", { id }),
  activateAppByAumid: (aumid: string) => callVoid("activate_app_by_aumid", { aumid }),

  /** Only ever from an explicit user click: brings classic Outlook forward on its calendar, on `startUtc`'s day when given. */
  outlookOpenCalendar: (startUtc: string | null) => callVoid("outlook_open_calendar", { startUtc }, { timeoutMs: 15_000 }),

  /** Only ever from an explicit click. Outlook answers and sends the reply; it may show its own security prompt first. */
  outlookRespondInvite: (id: string, response: InviteResponse) =>
    callVoid("outlook_respond_invite", { id, response }, { timeoutMs: 65_000 }),

  /** Only ever from an explicit click, with a link the backend itself reported (event.meetingUrl). */
  openMeetingUrl: (url: string) => callVoid("open_meeting_url", { url }),

  /** Raw events of another stretch of the calendar (at most 7 days); the calendar module normalises them. */
  calendarGetRange: (fromUtc: string, toUtc: string) => call<unknown>("calendar_get_range", { fromUtc, toUtc }, { timeoutMs: 30_000 }),

  /** Arguments are passed flat: invoke("set_island_geometry", { width, height, radius }). */
  setIslandGeometry: (geometry: IslandGeometry) => callVoid("set_island_geometry", { ...geometry }, { timeoutMs: 3000 }),

  /** Raw snapshot: the calendar provider owns normalisation. */
  calendarGetSnapshot: () => call<unknown>("calendar_get_snapshot"),
  calendarRefresh: () => callVoid("calendar_refresh"),

  /** key -> firedAtUnixMs. Null when the backend is unavailable. */
  async reminderStateLoad(): Promise<Record<string, number> | null> {
    const raw = await call<unknown>("reminder_state_load");
    return raw === null ? null : normalizeReminderState(raw);
  },
  reminderStateSave: (map: Record<string, number>) => callVoid("reminder_state_save", { map }),

  /** Whether a fullscreen app is in front right now (the event only reports changes). */
  async getFullscreenState(): Promise<boolean | null> {
    const value = await call<unknown>("get_fullscreen_state");
    return typeof value === "boolean" ? value : null;
  },

  /** Null when the backend is unavailable. */
  async notesLoad(): Promise<Note[] | null> {
    const raw = await call<unknown>("notes_load");
    return raw === null ? null : normalizeNotes(raw);
  },

  /** Replaces the whole list; resolves to the sanitised list Rust stored, null when it could not be saved. */
  async notesSave(notes: readonly Note[]): Promise<Note[] | null> {
    const raw = await call<unknown>("notes_save", { notes });
    return raw === null ? null : normalizeNotes(raw);
  },

  /**
   * Only ever from a click in the Notes tab's text box (`true`) and when that box loses focus
   * (`false`): while on, the island window may be active so typing reaches the page. When another
   * window takes the keyboard the backend ends it and emits `island-keyboard-ended`.
   */
  islandKeyboard: (on: boolean) => callVoid("island_keyboard", { on }, { timeoutMs: 3000 }),

  /** Only ever from an explicit user click: opens (or brings forward) the Island Center on a page. */
  openCenter: (page: CenterPage) => callVoid("open_center", { page }, { timeoutMs: 10_000 }),

  /**
   * How large the island may be on the monitor it is on (logical px). Never fails: outside Tauri,
   * on an error or on an unusable answer there is no limit (the preferred sizes apply).
   */
  async getIslandLimits(): Promise<IslandLimits> {
    return parseIslandLimits(await call<unknown>("get_island_limits", undefined, { timeoutMs: 2000 })) ?? NO_LIMITS;
  },

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
