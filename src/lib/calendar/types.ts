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

export type CalendarEventDto = {
  id: string; // sha256(EntryID + "|" + startUtc) truncated to 16 hex
  calendarId: string;
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
};

export const WAITING_SNAPSHOT: CalendarSnapshot = {
  status: "waiting",
  errorCode: null,
  lastSyncUnixMs: null,
  cachedCount: 0,
  nextRetryUnixMs: null,
  events: [],
  invites: [],
};
