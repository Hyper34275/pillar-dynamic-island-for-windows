// Everything the tour shows is made up here: a fixed moment, a few meetings, notifications and
// notes. Nothing is read from the user's computer. The sample texts are Hebrew on purpose (the
// tour is Hebrew, like the island); only the strings of the page itself live in i18n.ts.

import { inviteBody } from "../hooks/useMeetingInvites";
import { APP_VERSION } from "../lib/appInfo";
import type { CalendarEventDto, CalendarSnapshot, MeetingInviteDto } from "../lib/calendar/types";
import type { MeetingStatus } from "../lib/calendar/meetingStatus";
import { startOfDay } from "../lib/dateFormat";
import { SETTINGS_DEFAULTS, type Diagnostics, type IslandNotification, type Note, type Settings, type SystemInfo } from "../lib/ipc";
import type { HistoryEntry } from "../lib/notifications/history";
import type { ReminderAlert } from "../lib/reminders/types";
import type { Ringer } from "../lib/island/state";

const MINUTE = 60_000;

/** Tuesday morning: a fixed "now", so the tour looks the same whenever it is opened. */
export const TOUR_NOW = new Date(2026, 9, 6, 10, 25).getTime();
export const TOUR_TODAY = startOfDay(TOUR_NOW);

/** Local time `hour:minute` on the day `dayOffset` days from the tour's today (ms). */
function at(dayOffset: number, hour: number, minute = 0): number {
  const today = new Date(TOUR_TODAY);
  return new Date(today.getFullYear(), today.getMonth(), today.getDate() + dayOffset, hour, minute).getTime();
}

const iso = (ms: number) => new Date(ms).toISOString();

function meeting(
  id: string,
  subject: string,
  day: number,
  from: [number, number],
  to: [number, number],
  extra: Partial<CalendarEventDto> = {}
): CalendarEventDto {
  return {
    id,
    calendarId: "tour",
    subject,
    startUtc: iso(at(day, from[0], from[1])),
    endUtc: iso(at(day, to[0], to[1])),
    allDay: false,
    location: null,
    organizer: null,
    isRecurring: false,
    meetingUrl: null,
    busyStatus: "busy",
    responseStatus: "accepted",
    color: null,
    ...extra,
  };
}

const TEAMS_URL = "https://teams.example.invalid/meet";

export const TOUR_EVENTS: CalendarEventDto[] = [
  meeting("t1", "סטנדאפ צוות", 0, [9, 0], [9, 30], { color: "#30D158" }),
  meeting("t2", "סנכרון מוצר", 0, [10, 30], [11, 0], { color: "#0A84FF", meetingUrl: TEAMS_URL, location: "חדר ישיבות 3" }),
  meeting("t3", "ארוחת צהריים עם הצוות", 0, [13, 0], [14, 0], { color: "#FF9F0A" }),
  meeting("t4", "סקירת תקציב רבעונית", 0, [15, 30], [16, 0], { color: "#BF5AF2", meetingUrl: TEAMS_URL }),
  meeting("t5", "פגישת לקוח", 1, [9, 30], [10, 30], { color: "#0A84FF", meetingUrl: TEAMS_URL }),
  meeting("t6", "הדרכת עובדים חדשים", 1, [12, 0], [12, 45], { color: "#30D158" }),
  meeting("t7", "תכנון רבעון", 1, [14, 0], [15, 0], { color: "#FF375F" }),
  meeting("t8", "ראיון מועמד", 2, [11, 0], [12, 0], { color: "#40C8E0" }),
  meeting("t9", "סיכום שבוע", 2, [16, 0], [17, 0], { color: "#FF9F0A" }),
];

export const TOUR_SNAPSHOT: CalendarSnapshot = {
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: TOUR_NOW - 30_000,
  cachedCount: TOUR_EVENTS.length,
  nextRetryUnixMs: null,
  events: TOUR_EVENTS,
  invites: [],
};

/** The days the calendar step flips through (today, tomorrow, the day after). */
export const TOUR_DAYS = [at(0, 0), at(1, 0), at(2, 0)] as const;

// -----------------------------------------------------------------------------
// The collapsed island: a meeting about to start / in progress
// -----------------------------------------------------------------------------

export const SOON_STATUS: MeetingStatus = { kind: "soon", event: TOUR_EVENTS[1], minutes: 5 };

/** Its own moment: 12 minutes into the 10:30 meeting. */
export const NOW_MOMENT = at(0, 10, 42);
export const NOW_STATUS: MeetingStatus = {
  kind: "now",
  event: TOUR_EVENTS[1],
  endMs: Date.parse(TOUR_EVENTS[1].endUtc),
  progress: (NOW_MOMENT - Date.parse(TOUR_EVENTS[1].startUtc)) / (30 * MINUTE),
};

// -----------------------------------------------------------------------------
// Alerts, ringer, toasts
// -----------------------------------------------------------------------------

export const TOUR_ALERT: ReminderAlert = {
  key: "tour-alert",
  eventId: "t4",
  subject: TOUR_EVENTS[3].subject,
  startUtc: TOUR_EVENTS[3].startUtc,
  endUtc: TOUR_EVENTS[3].endUtc,
  location: "חדר ישיבות 3",
  meetingUrl: TEAMS_URL,
  minutesRemaining: 15,
  reminderType: { kind: "beforeStart", minutes: 15 },
};

export function tourRinger(silent: boolean): Ringer {
  return { key: "tour-ringer", phase: "start", silent, untilMs: Date.parse(TOUR_EVENTS[1].endUtc), toggles: silent ? 1 : 0 };
}

const TOUR_INVITE: MeetingInviteDto = {
  id: "tour-invite",
  subject: "סקירת תקציב רבעונית",
  organizer: "דנה כהן",
  startUtc: iso(at(1, 10, 0)),
  endUtc: iso(at(1, 11, 0)),
  location: null,
  receivedUtc: iso(TOUR_NOW - 2 * MINUTE),
};

/** Built on demand: its body is text in the page's language, which is chosen after the modules load. */
export function inviteNotification(): IslandNotification {
  return {
    id: 1,
    appName: "Outlook",
    title: TOUR_INVITE.subject,
    body: inviteBody(TOUR_INVITE, TOUR_NOW),
    timestamp: TOUR_NOW,
    aumid: null,
    invite: { id: TOUR_INVITE.id, startUtc: TOUR_INVITE.startUtc },
  };
}

export const TEAMS_NOTIFICATION: IslandNotification = {
  id: 2,
  appName: "Microsoft Teams",
  title: "דנה כהן",
  body: "אפשר לעבור על המצגת לפני הישיבה?",
  timestamp: TOUR_NOW,
  aumid: null,
};

export function tourHistory(): HistoryEntry[] {
  return [
    { notification: inviteNotification(), receivedAt: TOUR_NOW - 2 * MINUTE, silenced: false },
    { notification: TEAMS_NOTIFICATION, receivedAt: TOUR_NOW - 12 * MINUTE, silenced: false },
    {
      notification: { id: 3, appName: "Outlook", title: "עדכון מערכת מתוכנן", body: "מחר בערב תתבצע תחזוקה בשרת הקבצים.", timestamp: TOUR_NOW, aumid: null },
      receivedAt: TOUR_NOW - 41 * MINUTE,
      silenced: true,
    },
    {
      notification: { id: 4, appName: "WhatsApp", title: "אבא", body: "מה שלומך? מתי תגיע?", timestamp: TOUR_NOW, aumid: null },
      receivedAt: TOUR_NOW - 3 * 60 * MINUTE,
      silenced: false,
    },
  ];
}

// -----------------------------------------------------------------------------
// Notes, About, Settings
// -----------------------------------------------------------------------------

function note(id: string, text: string, updatedMinutesAgo: number, pinned = false): Note {
  const updatedAt = TOUR_NOW - updatedMinutesAgo * MINUTE;
  return { id, text, createdAt: updatedAt - 60 * MINUTE, updatedAt, pinned };
}

/** In the canonical order: pinned first, then newest. */
export const TOUR_NOTES: Note[] = [
  note("tournote0000001", "להתקשר לתמיכה בעניין המדפסת בקומה 2, שלוחה 214", 3 * 24 * 60, true),
  note("tournote0000002", "סדר יום לישיבת הצוות:\nסטטוס פרויקט, חופשות קיץ, תקציב", 3 * 60),
  note("tournote0000003", "להזמין עוגה ליום ההולדת של נועה", 26 * 60),
  note("tournote0000004", "לשלוח למיכל את המצגת המעודכנת", 4 * 24 * 60),
];

export const TOUR_SYSTEM_INFO: SystemInfo = {
  computerName: "OFFICE-PC-042",
  localIpv4: "10.20.30.41",
  ipAdapter: "Ethernet",
  windowsUser: "CORP\\dana",
  sessionId: 1,
  osName: "Windows 11 Enterprise",
  osDisplayVersion: "23H2",
  osBuild: 22631,
  appVersion: APP_VERSION,
  webview2Version: null,
};

export const TOUR_DIAGNOSTICS: Diagnostics = {
  outlookRunning: true,
  outlookMode: "classic",
  notificationMode: "events",
  recentErrorCodes: [],
};

export const TOUR_SETTINGS: Settings = { ...SETTINGS_DEFAULTS, onboardingDone: true };
