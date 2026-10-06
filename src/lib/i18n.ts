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
  "tab.datetime": "Date & Time",
  "tab.calendar": "Calendar",
  "tab.about": "About",

  "island.tabs": "Island sections",
  "island.expandedLabel": "{app} expanded",
  "island.open": "Open",
  "island.unavailable": "Unavailable",
  "island.tryAgain": "Try again",

  "ctx.expand": "Expand",
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
  "calendar.inProgress": "In progress",
  "calendar.noSubject": "(No subject)",
  "calendar.code": "Code {code}",

  "reminder.inMinutes": {
    one: "Meeting in {n} minute",
    other: "Meeting in {n} minutes",
  },
  "reminder.startingNow": "Meeting starting now",

  "status.waiting": "Waiting for Outlook",
  "status.connecting": "Connecting",
  "status.connected": "Connected",
  "status.newOutlookOnly": "New Outlook (unsupported)",
  "status.elevationMismatch": "Permission mismatch",
  "status.unresponsive": "Not responding",
  "status.failed": "Failed",

  "about.computer": "Computer",
  "about.ip": "Local IP",
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
  "about.modeClassic": "Classic",
  "about.modeNew": "New Outlook",
  "about.modeNone": "None",
  "about.calendar": "Calendar",
  "about.cachedEvents": "Cached events",
  "about.lastSync": "Last calendar sync",
  "about.notifications": "Notifications",
  "about.settings": "Settings",
  "about.credit": "Based on PILLAR (MIT License)",

  "settings.launchWithWindows": "Launch with Windows",
  "settings.hideInFullscreen": "Hide in fullscreen apps",
  "settings.meetingReminders": "Meeting reminders",
  "settings.reminderMinutes": "Remind me before",
  "settings.minutes": "{n} min",
  "settings.notifications": "Show notifications",
  "settings.monitor": "Display",
  "settings.monitorPrimary": "Primary",
  "settings.monitorN": "Display {n}",
  "settings.saveFailed": "Couldn't save settings",

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
  "tab.datetime": "תאריך ושעה",
  "tab.calendar": "יומן",
  "tab.about": "אודות",

  "island.tabs": "מקטעי האי",
  "island.expandedLabel": "{app} מורחב",
  "island.open": "פתיחה",
  "island.unavailable": "לא זמין",
  "island.tryAgain": "נסה שוב",

  "ctx.expand": "הרחב",
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
  "calendar.inProgress": "מתקיימת עכשיו",
  "calendar.noSubject": "(ללא נושא)",
  "calendar.code": "קוד {code}",

  "reminder.inMinutes": {
    one: "פגישה בעוד דקה",
    two: "פגישה בעוד שתי דקות",
    other: "פגישה בעוד {n} דקות",
  },
  "reminder.startingNow": "הפגישה מתחילה עכשיו",

  "status.waiting": "ממתין ל-Outlook",
  "status.connecting": "מתחבר",
  "status.connected": "מחובר",
  "status.newOutlookOnly": "Outlook חדש (לא נתמך)",
  "status.elevationMismatch": "אי-התאמת הרשאות",
  "status.unresponsive": "לא מגיב",
  "status.failed": "נכשל",

  "about.computer": "מחשב",
  "about.ip": "כתובת IP מקומית",
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
  "about.modeClassic": "קלאסי",
  "about.modeNew": "Outlook חדש",
  "about.modeNone": "אין",
  "about.calendar": "יומן",
  "about.cachedEvents": "אירועים בזיכרון",
  "about.lastSync": "סנכרון יומן אחרון",
  "about.notifications": "התראות",
  "about.settings": "הגדרות",
  "about.credit": "מבוסס על PILLAR (רישיון MIT)",

  "settings.launchWithWindows": "הפעלה עם Windows",
  "settings.hideInFullscreen": "הסתר באפליקציות במסך מלא",
  "settings.meetingReminders": "תזכורות לפגישות",
  "settings.reminderMinutes": "הזכר לי לפני",
  "settings.minutes": "{n} דק׳",
  "settings.notifications": "הצג התראות",
  "settings.monitor": "תצוגה",
  "settings.monitorPrimary": "ראשית",
  "settings.monitorN": "תצוגה {n}",
  "settings.saveFailed": "לא ניתן לשמור את ההגדרות",

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

/** The Windows/WebView2 UI language, validated so Intl never throws on it. */
export function getLocaleTag(): string {
  if (cachedTag) return cachedTag;
  const raw = typeof navigator !== "undefined" ? navigator.language : "";
  let tag = "en-US";
  try {
    if (raw && Intl.DateTimeFormat.supportedLocalesOf([raw]).length > 0) tag = raw;
  } catch {
    // malformed tag: keep the fallback
  }
  cachedTag = tag;
  return tag;
}

export function getLocale(): Locale {
  return detectLocale(getLocaleTag());
}

/** Applies lang/dir to <html> so text renders with the right direction. Layout stays LTR. */
export function applyDocumentLocale(): void {
  if (typeof document === "undefined") return;
  const tag = getLocaleTag();
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
