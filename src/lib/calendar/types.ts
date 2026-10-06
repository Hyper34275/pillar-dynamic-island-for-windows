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
};

export type CalendarSnapshot = {
  status: CalendarStatus;
  errorCode: string | null; // e.g. "OUTLOOK-102"
  lastSyncUnixMs: number | null;
  cachedCount: number;
  nextRetryUnixMs: number | null;
  events: CalendarEventDto[]; // sorted by start asc, now..+48h, max 50
};

export const WAITING_SNAPSHOT: CalendarSnapshot = {
  status: "waiting",
  errorCode: null,
  lastSyncUnixMs: null,
  cachedCount: 0,
  nextRetryUnixMs: null,
  events: [],
};
