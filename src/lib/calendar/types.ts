// Calendar IPC types, exactly as in docs/ENTERPRISE_DESIGN.md section 1.

export type CalendarStatus =
  | "waiting" // Outlook not running (normal state, OUTLOOK-101 is informational)
  | "connecting"
  | "connected"
  | "newOutlookOnly" // olk.exe running, no classic COM
  | "elevationMismatch" // Outlook runs elevated, we don't (or vice versa)
  | "unresponsive" // watchdog / RPC_E_CALL_REJECTED persists
  | "failed";

export type CalendarBusyStatus = "free" | "tentative" | "busy" | "oof" | "workingElsewhere";
export type CalendarResponseStatus = "none" | "organized" | "tentative" | "accepted" | "declined" | "notResponded";

/** What a calendar is to the user (decided by the backend from Outlook's group type and store, never from names). */
export type CalendarSourceKind = "primary" | "personal" | "shared" | "other";
/** The Outlook Calendar navigation group a calendar sits in. */
export type CalendarSourceGroup = "my" | "shared" | "other" | "rooms" | "custom" | "unknown";
export type CalendarSourceState = "ok" | "notSelected" | "unavailable" | "pending";

/** One calendar of the user's Outlook Calendar module. Discovered = all of them; active = the ones that contribute events. */
export type CalendarSourceDto = {
  id: string; // the events' calendarId
  name: string; // display name, presentation only
  group: CalendarSourceGroup;
  kind: CalendarSourceKind;
  selected: boolean; // checked in Outlook (as last known)
  active: boolean; // contributes events (selected, or the primary calendar)
  pendingInOutlook: boolean; // switched in the island; Outlook's checkbox follows once Outlook shows its calendar
  color?: string; // "#RRGGBB" the calendar has in Outlook's pane (an active one always has one)
  state: CalendarSourceState;
  errorCode: string | null; // e.g. "CAL-SHARED-101"
  eventCount: number;
  lastReadUnixMs: number | null;
};

/** "outlook": read from Outlook's pane now; "remembered": Outlook is not on its calendar, last readout; "primaryOnly": nothing known yet. */
export type CalendarSelectionOrigin = "outlook" | "remembered" | "primaryOnly";

export type CalendarSourcesReport = {
  sources: CalendarSourceDto[]; // primary first, then Outlook's pane order
  selection: CalendarSelectionOrigin;
  groups: number;
  listener: boolean; // Outlook's navigation change notifications are connected
  discoveredUnixMs: number | null;
};

export type CalendarEventDto = {
  id: string; // sha256(EntryID + "|" + startUtc) truncated to 16 hex
  calendarId: string;
  /** The source calendar's display name (always set by `normalizeEvent`). */
  calendarName?: string;
  /** Always set by `normalizeEvent`; older payloads mean the primary calendar. */
  sourceKind?: CalendarSourceKind;
  subject: string;
  startUtc: string; // ISO-8601 UTC
  endUtc: string;
  allDay: boolean;
  location: string | null;
  organizer: string | null;
  isRecurring: boolean;
  meetingUrl: string | null;
  busyStatus: CalendarBusyStatus;
  responseStatus: CalendarResponseStatus;
  color: string | null; // "#RRGGBB" of the event's first colored Outlook category
  /** "#RRGGBB" of its calendar in Outlook's pane; only while several calendars are shown. */
  calendarColor?: string;
};

/** An unread Outlook meeting request in the Inbox. In memory only, like events. */
export type MeetingInviteDto = {
  id: string; // sha256(EntryID) truncated to 16 hex
  subject: string;
  organizer: string | null;
  startUtc: string | null; // the requested meeting, when Outlook could tell
  endUtc: string | null;
  location: string | null;
  receivedUtc: string;
};

export type CalendarSnapshot = {
  status: CalendarStatus;
  errorCode: string | null; // e.g. "OUTLOOK-102"
  lastSyncUnixMs: number | null;
  cachedCount: number;
  nextRetryUnixMs: number | null;
  events: CalendarEventDto[]; // sorted by start asc, now..+48h, max 50
  invites: MeetingInviteDto[]; // newest first, max 10
  /** The calendars of the latest discovery (kept while Outlook is away); null before the first. */
  sources?: CalendarSourcesReport | null;
};

export const WAITING_SNAPSHOT: CalendarSnapshot = {
  status: "waiting",
  errorCode: null,
  lastSyncUnixMs: null,
  cachedCount: 0,
  nextRetryUnixMs: null,
  events: [],
  invites: [],
  sources: null,
};
