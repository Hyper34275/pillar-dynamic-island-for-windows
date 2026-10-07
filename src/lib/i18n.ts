// UI strings (English + Hebrew) and locale detection.
//
// Only strings are translated; dates and times always come from Intl (see dateFormat.ts),
// so every Windows language gets correctly formatted dates even without a string table.
// Text direction is applied to <html> for the sake of the strings only: the island
// layout itself is physically LTR and its containers set dir="ltr" explicitly.

export type Locale = "en" | "he";

type PluralForms = { other: string } & Partial<Record<Intl.LDMLPluralRule, string>>;
type Message = string | PluralForms;

const en = {
  "tab.calendar": "Calendar",
  "tab.about": "About",
  "tab.settings": "Settings",
  "tab.notifications": "Notifications",
  "tab.notes": "Notes",

  "island.tabs": "Island sections",
  "island.expandedLabel": "{app} expanded",
  "island.open": "Open",
  "island.unavailable": "Unavailable",
  "island.tryAgain": "Try again",

  "ctx.collapse": "Collapse",
  "ctx.prevTab": "Previous tab",
  "ctx.nextTab": "Next tab",

  "calendar.next": "Next meeting",
  "calendar.later": "Later",
  "calendar.waiting": "Waiting for Outlook",
  "calendar.waitingHint": "Your meetings appear once classic Outlook is running.",
  "calendar.connecting": "Connecting to Outlook…",
  "calendar.noEvents": "No upcoming meetings",
  "calendar.noEventsHint": "You're all clear.",
  "calendar.newOutlook": "New Outlook isn't supported",
  "calendar.newOutlookHint": "Switch to classic Outlook to see your meetings.",
  "calendar.elevation": "Outlook runs with different permissions",
  "calendar.elevationHint": "Start Outlook and CompanyIsland the same way, both without \"Run as administrator\".",
  "calendar.unresponsive": "Outlook isn't responding",
  "calendar.unresponsiveHint": "Waiting for Outlook to become available again.",
  "calendar.failed": "Couldn't read the calendar",
  "calendar.failedHint": "Trying again automatically.",
  "calendar.now": "Now",
  "calendar.allDay": "All day",
  "calendar.noSubject": "(No subject)",
  "calendar.code": "Code {code}",
  "calendar.endsIn": "Ends {rel}",
  "calendar.join": "Join",
  "calendar.joinAria": "Join the meeting: {subject}",
  "calendar.today": "Today",
  "calendar.prevWeek": "Previous week",
  "calendar.nextWeek": "Next week",
  "calendar.dayEmpty": "No meetings this day",
  "calendar.loading": "Loading from Outlook…",
  "calendar.dayFailed": "Couldn't load this day",
  "calendar.timeline": "Meetings across the day",
  "calendar.invites": "Pending invitations",
  "calendar.silence": "Silence notifications until the meeting ends",
  "calendar.unsilence": "Turn notifications back on",

  "reminder.inMinutes": {
    one: "Meeting in {n} minute",
    other: "Meeting in {n} minutes",
  },
  "reminder.startingNow": "Meeting starting now",
  "reminder.snooze": "Remind me in 5 min",
  "status.inMeetingUntil": "In a meeting until {time}",
  "ringer.ring": "Ring",
  "ringer.silent": "Silent",
  "ringer.hint": "Tap to silence notifications until the meeting ends",

  "status.waiting": "Waiting for Outlook",
  "status.connecting": "Connecting",
  "status.connected": "Connected",
  "status.newOutlookOnly": "New Outlook (unsupported)",
  "status.elevationMismatch": "Permission mismatch",
  "status.unresponsive": "Not responding",
  "status.failed": "Failed",

  "about.computer": "Computer",
  "about.ip": "Local IP",
  "about.time": "Current time",
  "about.copyHint": "Click to copy",
  "about.diagnostics": "Diagnostics",
  "about.copy": "Copy diagnostics",
  "about.copied": "Copied",
  "about.copyFailed": "Copy failed",
  "about.openLogs": "Open logs",
  "about.windowsUser": "Windows user",
  "about.os": "Operating system",
  "about.version": "App version",
  "about.outlook": "Outlook",
  "about.outlookRunning": "Running",
  "about.outlookNotRunning": "Not running",
  "about.outlookMode": "Outlook mode",
  "about.modeClassic": "Classic Outlook",
  "about.modeNew": "New Outlook",
  "about.modeNone": "None",
  "about.calendar": "Calendar",
  "about.cachedEvents": "Cached events",
  "about.lastSync": "Last calendar sync",
  "about.notifications": "Notifications",
  "about.notificationDelivery": "Notification delivery",
  "about.deliveryEvents": "Live events",
  "about.deliveryPolling": "Polling",
  "about.deliveryNone": "None",
  "about.internalError": "Internal Error",
  "about.recentErrors": "Recent error codes",
  "about.settings": "Settings",
  "about.credit": "Based on PILLAR (MIT License)",

  "settings.launchWithWindows": "Launch with Windows",
  "settings.hideInFullscreen": "Hide in fullscreen apps",
  "settings.meetingReminders": "Meeting reminders",
  "settings.reminderMinutes": "Remind me before",
  "settings.minutes": "{n} min",
  "settings.notifications": "Show notifications",
  "settings.meetingInvites": "Meeting invitations",
  "settings.meetingSilence": "Offer silence when a meeting starts",
  "settings.monitor": "Display",
  "settings.monitorPrimary": "Primary",
  "settings.monitorN": "Display {n}",
  "settings.saveFailed": "Couldn't save settings",
  "settings.islandDisplay": "Collapsed island",
  "settings.display.full": "Time, date and day",
  "settings.display.clock": "Time only",
  "settings.display.date": "Date and day",
  "settings.center": "Island Center",
  "settings.openCenter": "Open Island Center",
  "settings.tour": "System tour",

  "notif.status.allowed": "Allowed",
  "notif.status.denied": "Blocked in Windows settings",
  "notif.status.unspecified": "Not allowed yet",
  "notif.status.unsupported": "Not supported",
  "notif.status.policy": "Disabled by policy",
  "notif.status.error": "Unavailable",
  "notif.status.off": "Off",
  "notif.allow": "Allow access",
  "notif.dismiss": "Dismiss notification",
  "notif.default": "Notification",
  "notif.now": "now",
  "invite.label": "Meeting invitation",
  "invite.from": "From {name}",
  "invite.open": "Open the invitation day in the Outlook calendar",
  "invite.accept": "Accept",
  "invite.tentative": "Maybe",
  "invite.decline": "Decline",
  "invite.accepted": "Accepted",
  "invite.tentativeDone": "Answered maybe",
  "invite.declined": "Declined",
  "invite.sending": "Sending…",
  "invite.failed": "Couldn't answer",
  "notifs.empty": "No notifications",
  "notifs.emptyHint": "Notifications you receive appear here until the app restarts.",
  "notifs.off": "Showing notifications is turned off in Settings.",
  "notifs.clear": "Clear all",
  "notifs.remove": "Remove notification",
  "notifs.silenced": "Arrived during a silenced meeting",
  "notes.new": "New note",
  "notes.list": "Notes",
  "notes.empty": "No notes yet",
  "notes.emptyHint": "Notes are saved only on this computer.",
  "notes.pinned": "Pinned",
  "notes.pin": "Pin note",
  "notes.unpin": "Unpin note",
  "notes.copy": "Copy note",
  "notes.copied": "Copied",
  "notes.delete": "Delete note",
  "notes.saveFailed": "Couldn't save the note",
  "notes.open": "Open note",
  "notes.confirmDelete": "Delete?",
  "notes.confirmDeleteLabel": "Confirm deleting the note",
  "notes.loadFailed": "Couldn't load the notes",
  "notes.retry": "Try again",
  "tour.pageTitle": "System tour",
  "tour.step": "Step {n} of {total}",
  "tour.prev": "Previous",
  "tour.next": "Next",
  "tour.finish": "Finish",
  "tour.dots": "Tour steps",
  "tour.dot": "Step {n}: {title}",
  "tour.autoplay": "Autoplay",
  "tour.openNotes": "Open notes in the Island Center",
  "tour.openSettings": "Open settings in the Island Center",
  "tour.s1.title": "The collapsed island",
  "tour.s1.text": "The date, the time and the day are always in view. A blue dot or a number appears when there are notifications you haven't seen yet.",
  "tour.s2.title": "A meeting is about to start",
  "tour.s2.text": "Five minutes before a meeting the island shows how long is left and what the meeting is about.",
  "tour.s3.title": "In a meeting",
  "tour.s3.text": "While a meeting runs the island shows when it ends, with a thin progress bar. A red bell means notifications are silenced.",
  "tour.s4.title": "A reminder before the meeting",
  "tour.s4.text": "The island opens up with the meeting's details. Join straight from here, or ask to be reminded again in 5 minutes.",
  "tour.s5.title": "Ring or silent",
  "tour.s5.text": "When a meeting starts the island offers to silence notifications. One tap switches between ring and silent, and silent holds them until the meeting ends.",
  "tour.s6.title": "A meeting invitation",
  "tour.s6.text": "Invitations from Outlook appear in the island. Accept, answer maybe or decline with one tap.",
  "tour.s7.title": "Windows notifications",
  "tour.s7.text": "Notifications from your computer appear in the island. Tap one to open its app, or swipe it aside to dismiss it.",
  "tour.s8.title": "Calendar",
  "tour.s8.text": "Hover over the island to open it. In the Calendar tab you move between days and see the day's meetings on a timeline.",
  "tour.s9.title": "Notifications",
  "tour.s9.text": "Every notification you received is kept here until the app restarts.",
  "tour.s10.title": "Notes",
  "tour.s10.text": "Short notes for yourself, saved only on this computer. Pin, copy or delete them here; writing and editing happen in the Island Center.",
  "tour.s11.title": "About",
  "tour.s11.text": "The computer's name and IP address in one place, ready to read out to IT. A tap copies the value.",
  "tour.s12.title": "Settings",
  "tour.s12.text": "Choose what the island shows, when to remind you of meetings and more. You can also open the Island Center and this tour again from here.",
  "notif.announce": {
    one: "{n} new notification",
    other: "{n} new notifications",
  },
  "notif.unread": {
    one: "{n} unread notification",
    other: "{n} unread notifications",
  },
} satisfies Record<string, Message>;

export type MessageKey = keyof typeof en;

const he: Record<MessageKey, Message> = {
  "tab.calendar": "יומן",
  "tab.about": "אודות",
  "tab.settings": "הגדרות",
  "tab.notifications": "התראות",
  "tab.notes": "פתקים",

  "island.tabs": "מקטעי האי",
  "island.expandedLabel": "{app} מורחב",
  "island.open": "פתיחה",
  "island.unavailable": "לא זמין",
  "island.tryAgain": "נסה שוב",

  "ctx.collapse": "כווץ",
  "ctx.prevTab": "הכרטיסייה הקודמת",
  "ctx.nextTab": "הכרטיסייה הבאה",

  "calendar.next": "הפגישה הבאה",
  "calendar.later": "בהמשך",
  "calendar.waiting": "ממתין ל-Outlook",
  "calendar.waitingHint": "הפגישות שלך יופיעו כש-Outlook הקלאסי פועל.",
  "calendar.connecting": "מתחבר ל-Outlook…",
  "calendar.noEvents": "אין פגישות קרובות",
  "calendar.noEventsHint": "היומן שלך פנוי.",
  "calendar.newOutlook": "Outlook החדש אינו נתמך",
  "calendar.newOutlookHint": "עבור ל-Outlook הקלאסי כדי לראות את הפגישות.",
  "calendar.elevation": "ל-Outlook יש הרשאות שונות",
  "calendar.elevationHint": "הפעל את Outlook ואת CompanyIsland באותו אופן, שניהם בלי \"הפעל כמנהל\".",
  "calendar.unresponsive": "Outlook לא מגיב",
  "calendar.unresponsiveHint": "ממתין ש-Outlook יהיה זמין שוב.",
  "calendar.failed": "לא ניתן לקרוא את היומן",
  "calendar.failedHint": "מנסה שוב אוטומטית.",
  "calendar.now": "עכשיו",
  "calendar.allDay": "כל היום",
  "calendar.noSubject": "(ללא נושא)",
  "calendar.code": "קוד {code}",
  "calendar.endsIn": "מסתיימת {rel}",
  "calendar.join": "הצטרף",
  "calendar.joinAria": "הצטרף לפגישה: {subject}",
  "calendar.today": "היום",
  "calendar.prevWeek": "השבוע הקודם",
  "calendar.nextWeek": "השבוע הבא",
  "calendar.dayEmpty": "אין פגישות ביום הזה",
  "calendar.loading": "טוען מ-Outlook…",
  "calendar.dayFailed": "לא ניתן לטעון את היום הזה",
  "calendar.timeline": "הפגישות לאורך היום",
  "calendar.invites": "זימונים ממתינים",
  "calendar.silence": "השתק התראות עד סוף הפגישה",
  "calendar.unsilence": "החזר את ההתראות",

  "reminder.inMinutes": {
    one: "פגישה בעוד דקה",
    two: "פגישה בעוד שתי דקות",
    other: "פגישה בעוד {n} דקות",
  },
  "reminder.startingNow": "הפגישה מתחילה עכשיו",
  "reminder.snooze": "הזכר בעוד 5 דק׳",
  "status.inMeetingUntil": "בפגישה עד {time}",
  "ringer.ring": "צלצול",
  "ringer.silent": "שקט",
  "ringer.hint": "לחיצה משתיקה את ההתראות עד סוף הפגישה",

  "status.waiting": "ממתין ל-Outlook",
  "status.connecting": "מתחבר",
  "status.connected": "מחובר",
  "status.newOutlookOnly": "Outlook חדש (לא נתמך)",
  "status.elevationMismatch": "אי-התאמת הרשאות",
  "status.unresponsive": "לא מגיב",
  "status.failed": "נכשל",

  "about.computer": "מחשב",
  "about.ip": "כתובת IP מקומית",
  "about.time": "השעה הנוכחית",
  "about.copyHint": "לחיצה להעתקה",
  "about.diagnostics": "אבחון",
  "about.copy": "העתק אבחון",
  "about.copied": "הועתק",
  "about.copyFailed": "ההעתקה נכשלה",
  "about.openLogs": "פתח יומנים",
  "about.windowsUser": "משתמש Windows",
  "about.os": "מערכת הפעלה",
  "about.version": "גרסת האפליקציה",
  "about.outlook": "Outlook",
  "about.outlookRunning": "פועל",
  "about.outlookNotRunning": "לא פועל",
  "about.outlookMode": "מצב Outlook",
  "about.modeClassic": "Outlook קלאסי",
  "about.modeNew": "Outlook חדש",
  "about.modeNone": "אין",
  "about.calendar": "יומן",
  "about.cachedEvents": "אירועים בזיכרון",
  "about.lastSync": "סנכרון יומן אחרון",
  "about.notifications": "התראות",
  "about.notificationDelivery": "אספקת התראות",
  "about.deliveryEvents": "אירועים חיים",
  "about.deliveryPolling": "בדיקה מחזורית",
  "about.deliveryNone": "אין",
  "about.internalError": "שגיאה פנימית",
  "about.recentErrors": "קודי שגיאה אחרונים",
  "about.settings": "הגדרות",
  "about.credit": "מבוסס על PILLAR (רישיון MIT)",

  "settings.launchWithWindows": "הפעלה עם Windows",
  "settings.hideInFullscreen": "הסתר באפליקציות במסך מלא",
  "settings.meetingReminders": "תזכורות לפגישות",
  "settings.reminderMinutes": "הזכר לי לפני",
  "settings.minutes": "{n} דק׳",
  "settings.notifications": "הצג התראות",
  "settings.meetingInvites": "זימונים לפגישות",
  "settings.meetingSilence": "הצע שקט בתחילת פגישה",
  "settings.monitor": "תצוגה",
  "settings.monitorPrimary": "ראשית",
  "settings.monitorN": "תצוגה {n}",
  "settings.saveFailed": "לא ניתן לשמור את ההגדרות",
  "settings.islandDisplay": "האי המכווץ",
  "settings.display.full": "שעה, תאריך ויום",
  "settings.display.clock": "שעה בלבד",
  "settings.display.date": "תאריך ויום",
  "settings.center": "מרכז האי",
  "settings.openCenter": "פתח את מרכז האי",
  "settings.tour": "סיור במערכת",

  "notif.status.allowed": "מותר",
  "notif.status.denied": "חסום בהגדרות Windows",
  "notif.status.unspecified": "טרם אושר",
  "notif.status.unsupported": "לא נתמך",
  "notif.status.policy": "מושבת על ידי מדיניות",
  "notif.status.error": "לא זמין",
  "notif.status.off": "כבוי",
  "notif.allow": "אשר גישה",
  "notif.dismiss": "סגור התראה",
  "notif.default": "התראה",
  "notif.now": "עכשיו",
  "invite.label": "זימון לפגישה",
  "invite.from": "מאת {name}",
  "invite.open": "פתח את יום הזימון ביומן Outlook",
  "invite.accept": "אשר",
  "invite.tentative": "אולי",
  "invite.decline": "דחה",
  "invite.accepted": "אושר",
  "invite.tentativeDone": "נענה אולי",
  "invite.declined": "נדחה",
  "invite.sending": "שולח…",
  "invite.failed": "לא ניתן להשיב",
  "notifs.empty": "אין התראות",
  "notifs.emptyHint": "התראות שיתקבלו יופיעו כאן עד הפעלה מחדש של האפליקציה.",
  "notifs.off": "הצגת ההתראות כבויה בהגדרות.",
  "notifs.clear": "נקה הכול",
  "notifs.remove": "הסר התראה",
  "notifs.silenced": "הגיעה בזמן פגישה מושתקת",
  "notes.new": "פתק חדש",
  "notes.list": "פתקים",
  "notes.empty": "אין פתקים עדיין",
  "notes.emptyHint": "הפתקים נשמרים רק במחשב הזה.",
  "notes.pinned": "מוצמד",
  "notes.pin": "הצמד פתק",
  "notes.unpin": "בטל הצמדה",
  "notes.copy": "העתק פתק",
  "notes.copied": "הועתק",
  "notes.delete": "מחק פתק",
  "notes.saveFailed": "לא ניתן לשמור את הפתק",
  "notes.open": "פתח פתק",
  "notes.confirmDelete": "למחוק?",
  "notes.confirmDeleteLabel": "אשר מחיקת הפתק",
  "notes.loadFailed": "לא ניתן לטעון את הפתקים",
  "notes.retry": "נסה שוב",
  "tour.pageTitle": "סיור במערכת",
  "tour.step": "שלב {n} מתוך {total}",
  "tour.prev": "הקודם",
  "tour.next": "הבא",
  "tour.finish": "סיום",
  "tour.dots": "שלבי הסיור",
  "tour.dot": "שלב {n}: {title}",
  "tour.autoplay": "ניגון אוטומטי",
  "tour.openNotes": "פתח את הפתקים במרכז האי",
  "tour.openSettings": "פתח את ההגדרות במרכז האי",
  "tour.s1.title": "האי המכווץ",
  "tour.s1.text": "התאריך, השעה והיום תמיד מול העיניים. נקודה כחולה או מספר מופיעים כשיש התראות שעוד לא ראית.",
  "tour.s2.title": "פגישה עומדת להתחיל",
  "tour.s2.text": "חמש דקות לפני שפגישה מתחילה האי מראה כמה זמן נשאר ומה נושא הפגישה.",
  "tour.s3.title": "באמצע פגישה",
  "tour.s3.text": "במהלך פגישה האי מראה מתי היא מסתיימת, עם פס התקדמות דק. פעמון אדום מסמן שההתראות מושתקות.",
  "tour.s4.title": "תזכורת לפני הפגישה",
  "tour.s4.text": "האי נפתח ומציג את פרטי הפגישה. אפשר להצטרף ישר משם, או לבקש תזכורת נוספת בעוד 5 דקות.",
  "tour.s5.title": "צלצול או שקט",
  "tour.s5.text": "כשפגישה מתחילה האי מציע להשתיק התראות. נגיעה אחת מחליפה בין צלצול לשקט, והשקט מחזיק את ההתראות עד סוף הפגישה.",
  "tour.s6.title": "הזמנה לפגישה",
  "tour.s6.text": "הזמנות מ-Outlook מופיעות באי. אפשר לאשר, לענות ״אולי״ או לדחות בלחיצה אחת.",
  "tour.s7.title": "התראות Windows",
  "tour.s7.text": "התראות מהמחשב מופיעות באי. לחיצה פותחת את האפליקציה שלהן, והחלקה הצידה מסירה אותן.",
  "tour.s8.title": "יומן",
  "tour.s8.text": "מעבירים את העכבר מעל האי והוא נפתח. בלשונית היומן עוברים בין ימים ורואים את הפגישות של היום על ציר זמן.",
  "tour.s9.title": "התראות",
  "tour.s9.text": "כל ההתראות שקיבלת נשמרות כאן עד שהאפליקציה נסגרת.",
  "tour.s10.title": "פתקים",
  "tour.s10.text": "פתקים קצרים לעצמך, שנשמרים רק במחשב הזה. כאן אפשר להצמיד, להעתיק ולמחוק; כתיבה ועריכה נעשות במרכז האי.",
  "tour.s11.title": "אודות",
  "tour.s11.text": "שם המחשב וכתובת ה-IP במקום אחד, מוכנים להקראה לתמיכה. לחיצה מעתיקה את הערך.",
  "tour.s12.title": "הגדרות",
  "tour.s12.text": "קובעים מה האי מציג, מתי להזכיר על פגישות ועוד. מכאן אפשר גם לפתוח שוב את מרכז האי ואת הסיור הזה.",
  "notif.announce": {
    one: "התראה חדשה אחת",
    two: "שתי התראות חדשות",
    other: "{n} התראות חדשות",
  },
  "notif.unread": {
    one: "התראה אחת שלא נקראה",
    two: "שתי התראות שלא נקראו",
    other: "{n} התראות שלא נקראו",
  },
};

const tables: Record<Locale, Record<MessageKey, Message>> = { en, he };

const RTL_LANGUAGES = new Set(["ar", "he", "iw", "fa", "ur", "ps", "sd", "yi", "dv", "ug", "ckb"]);

function primaryLanguage(tag: string): string {
  return tag.toLowerCase().split(/[-_]/)[0];
}

/** String-table locale for a BCP-47 tag. Anything without a table falls back to English. */
export function detectLocale(tag: string | null | undefined): Locale {
  const lang = tag ? primaryLanguage(tag) : "";
  return lang === "he" || lang === "iw" ? "he" : "en";
}

export function isRtl(tag: string | null | undefined): boolean {
  return !!tag && RTL_LANGUAGES.has(primaryLanguage(tag));
}

let cachedTag: string | null = null;

/** `raw` when Intl accepts it as a locale, otherwise null (never throws). */
function supportedTag(raw: string | null | undefined): string | null {
  if (!raw) return null;
  try {
    return Intl.DateTimeFormat.supportedLocalesOf([raw]).length > 0 ? raw : null;
  } catch {
    return null;
  }
}

/** The Windows/WebView2 UI language, validated so Intl never throws on it. */
export function getLocaleTag(): string {
  if (cachedTag) return cachedTag;
  cachedTag = supportedTag(typeof navigator !== "undefined" ? navigator.language : "") ?? "en-US";
  return cachedTag;
}

let formatTag: string | null = null;

/**
 * Locale for dates, weekdays and times: the user's Windows regional format, which can differ
 * from the display language (English UI + Israel region = 6/10, not 10/6). Strings and text
 * direction keep following the UI language. Pass null to fall back to the UI language.
 */
export function setFormatLocale(raw: string | null | undefined): void {
  // Windows sort-order suffixes ("de-DE_phoneb") are not BCP-47.
  formatTag = supportedTag(raw?.split("_")[0]);
}

export function getFormatTag(): string {
  return formatTag ?? getLocaleTag();
}

const LOCALE_TAGS: Record<Locale, string> = { en: "en-US", he: "he-IL" };

let fixedLocale: Locale | null = null;

/**
 * Pins the UI language instead of following Windows. The app pins Hebrew at startup (main.tsx):
 * the island is always Hebrew, on an English Windows too. Null follows Windows again (tests).
 */
export function setFixedLocale(locale: Locale | null): void {
  fixedLocale = locale;
}

export function getLocale(): Locale {
  return fixedLocale ?? detectLocale(getLocaleTag());
}

/**
 * Locale for the *words* in dates (weekday and month names, "tomorrow", "in 5 min", AM/PM):
 * the pinned UI language when there is one, otherwise the regional format. Numbers, their
 * order and the 12/24-hour choice always follow the regional format (getFormatTag).
 */
export function getWordTag(): string {
  return fixedLocale ? LOCALE_TAGS[fixedLocale] : getFormatTag();
}

/** Applies lang/dir to <html> so text renders with the right direction. Layout stays LTR. */
export function applyDocumentLocale(): void {
  if (typeof document === "undefined") return;
  const tag = fixedLocale ? LOCALE_TAGS[fixedLocale] : getLocaleTag();
  document.documentElement.lang = tag;
  document.documentElement.dir = isRtl(tag) ? "rtl" : "ltr";
}

const pluralRules = new Map<string, Intl.PluralRules>();

function selectPlural(n: number, locale: Locale): Intl.LDMLPluralRule {
  let rules = pluralRules.get(locale);
  if (!rules) {
    rules = new Intl.PluralRules(locale);
    pluralRules.set(locale, rules);
  }
  return rules.select(n);
}

export type MessageParams = Record<string, string | number>;

export function t(key: MessageKey, params?: MessageParams, locale: Locale = getLocale()): string {
  const entry = tables[locale][key] ?? tables.en[key];
  let template: string;
  if (typeof entry === "string") {
    template = entry;
  } else {
    const n = typeof params?.n === "number" ? params.n : Number(params?.n ?? 0);
    template = entry[selectPlural(n, locale)] ?? entry.other;
  }
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}
