import { useMotionValue } from "motion/react";
import type { ReactNode } from "react";
import { alertIslandSize } from "../components/Pill/alertLayout";
import { assistantIslandSize } from "../components/Pill/assistantLayout";
import { AssistantCard as AssistantCardView } from "../components/Pill/AssistantCard";
import type { AssistantCard, AssistantItem } from "../lib/assistant/types";
import { expandedSize, ringerSize, type IslandSize } from "../components/Pill/animations";
import { toastLayout } from "../components/Pill/toastLayout";
import type { HistoryEntry } from "../lib/notifications/history";
import { CompactIsland } from "../components/Pill/CompactIsland";
import { HeaderActionButton } from "../components/Pill/HeaderAction";
import { PANEL_TITLE_CLASS, PanelFrame, panelBodyClass } from "../components/Pill/PanelFrame";
import { MeetingAlert } from "../components/Pill/MeetingAlert";
import { NotificationToast } from "../components/Pill/NotificationToast";
import { AboutView } from "../components/Pill/panels/AboutTab";
import { CalendarView } from "../components/Pill/panels/CalendarTab";
import { DayTimeline } from "../components/Pill/panels/DayTimeline";
import { NoteComposer, NotesView } from "../components/Pill/panels/NotesTab";
import { NotificationsView } from "../components/Pill/panels/NotificationsTab";
import { SettingsView } from "../components/Pill/panels/SettingsTab";
import { WeekStrip } from "../components/Pill/panels/WeekStrip";
import { RingerPill } from "../components/Pill/RingerPill";
import { TabDock } from "../components/Pill/TabDock";
import { TABS, type TabId } from "../components/Pill/tabs";
import { colorOf } from "../components/Pill/ui/eventColor";
import { eventsOfDay } from "../lib/calendar/dayRange";
import { isRealMeeting } from "../lib/calendar/select";
import { startOfDay } from "../lib/dateFormat";
import { t } from "../lib/i18n";
import type { IslandNotification } from "../lib/ipc";
import type { StickyColour, StickySnapshot } from "../lib/notes/sticky";
import type { ReminderAlert } from "../lib/reminders/types";
import { tourCompactContent } from "../tour/compact";
import {
  inviteNotification,
  NOW_MOMENT,
  NOW_STATUS,
  SOON_STATUS,
  TEAMS_NOTIFICATION,
  TOUR_ALERT,
  TOUR_DIAGNOSTICS,
  TOUR_EVENTS,
  TOUR_NOTES,
  TOUR_NOW,
  TOUR_SETTINGS,
  TOUR_SNAPSHOT,
  TOUR_SYSTEM_INFO,
  TOUR_TODAY,
  tourHistory,
  tourRinger,
} from "../tour/mockData";

const noop = () => {};
const never = async () => false;

/** Made-up Windows Sticky Notes for the notes exhibits (read only in the island; five, so the section collapses to three). */
const STICKY_SAMPLE: StickySnapshot = {
  availability: "ok",
  revision: 1,
  notes: [
    ["שלח את הדוח לרוני עד יום ה'", "yellow", 40],
    ["Call the dentist\nask about Tuesday", "blue", 5 * 60],
    ["רשימת קניות:\nחלב, לחם, גבינה, ביצים", "green", 26 * 60],
    ["WiFi: guest-1127", "pink", 3 * 24 * 60],
    ["ללמוד ל-TypeScript", "charcoal", 9 * 24 * 60],
  ].map(([text, colour, minutesAgo], i) => ({
    id: `sticky-${i}`,
    text: text as string,
    title: (text as string).split("\n")[0],
    colour: colour as StickyColour,
    updatedAt: TOUR_NOW - (minutesAgo as number) * 60_000,
    createdAt: TOUR_NOW - (minutesAgo as number) * 60_000 - 3_600_000,
  })),
};

const LONG_ALERT: ReminderAlert = {
  ...TOUR_ALERT,
  key: "long",
  subject: "סקירה רבעונית של תקציב מחלקת מערכות המידע והתשתיות לשנת 2027 כולל תוכנית רכש",
  location: "חדר ישיבות גדול, קומה 14, בניין המטה הראשי (אגף מזרחי)",
};
const SOON_ALERT: ReminderAlert = { ...TOUR_ALERT, key: "soon", minutesRemaining: 0, location: null };

const SNIPPING: IslandNotification = {
  id: 10,
  appName: "Snipping Tool",
  title: "Screenshot copied to clipboard",
  body: "Select here to mark up and share the image.",
  timestamp: TOUR_NOW,
  aumid: null,
};
const SNIPPING_SHORT: IslandNotification = { ...SNIPPING, id: 11, body: "", title: "Snip saved" };
const TEAMS_LONG: IslandNotification = {
  id: 12,
  appName: "Microsoft Teams",
  title: "דנה כהן (Product Team)",
  body: "שלחתי לך את ה-deck המעודכן ב-SharePoint, תעבור על השקפים 4 עד 9 לפני הישיבה עם ה-VP מחר בבוקר בבקשה",
  timestamp: TOUR_NOW,
  aumid: null,
};
const OUTLOOK_EN: IslandNotification = {
  id: 13,
  appName: "Outlook",
  title: "Quarterly infrastructure budget review — final numbers attached",
  body: "Hi all, please find attached the final numbers for the Q3 review. Let me know before Thursday.",
  timestamp: TOUR_NOW,
  aumid: null,
};

const LONG_APP: IslandNotification = {
  id: 14,
  appName: "Contoso Enterprise Resource Planning Approvals",
  title: "Project Alpha_v2.pptx מוכן לבדיקה",
  body: "Meeting at 11:00 בחדר 3 — Teams — דניאל כהן",
  timestamp: TOUR_NOW,
  aumid: null,
};
const MIXED: IslandNotification = {
  id: 15,
  appName: "Microsoft Teams",
  title: "Teams — דניאל כהן",
  body: "Project Alpha_v2.pptx מוכן לבדיקה, השרת 10.20.30.41 זמין.",
  timestamp: TOUR_NOW,
  aumid: null,
};
const LONG_HEBREW_TITLE: IslandNotification = {
  id: 16,
  appName: "Outlook",
  title: "תזכורת: הגשת דוח ההוצאות החודשי של מחלקת הכספים והרכש עד סוף יום העבודה",
  body: "נא לצרף את כל הקבלות הסרוקות ולוודא שהסכומים תואמים לכרטיס האשראי הארגוני לפני השליחה.",
  timestamp: TOUR_NOW,
  aumid: null,
};

const BIDI_TEXTS: Array<[string, string]> = [
  ["Teams — דניאל כהן", "Project Alpha_v2.pptx מוכן לבדיקה"],
  ["Project Alpha_v2.pptx מוכן לבדיקה", "השרת 10.20.30.41 זמין"],
  ["Meeting at 11:00 בחדר 3", "10.20.30.41"],
  ["C:\\Users\\Daniel\\Report.pdf", "user@example.com"],
  ["https://example.com/path", "קישור לאתר החדש: https://example.com/path"],
  ["הכל מוכן 🎉 (גרסה 2.0) בחדר (3)", "Done 🎉 (v2.0) — הכל תקין (100%)"],
];
const BIDI_ENTRIES: HistoryEntry[] = BIDI_TEXTS.map(([title, body], i) => ({
  notification: { id: 2000 + i, appName: i % 2 ? "Outlook" : "Microsoft Teams", title, body, timestamp: TOUR_NOW, aumid: null },
  receivedAt: TOUR_NOW - (i + 1) * 4 * 60_000,
  silenced: false,
}));

function manyEntries(n: number): HistoryEntry[] {
  const base = tourHistory();
  return Array.from({ length: n }, (_, i) => {
    const entry = base[i % base.length];
    return { ...entry, notification: { ...entry.notification, id: 1000 + i }, receivedAt: entry.receivedAt - i * 7 * 60_000 };
  });
}

const BUSY_DAYS: ReadonlySet<number> = new Set(TOUR_EVENTS.filter(isRealMeeting).map((event) => startOfDay(Date.parse(event.startUtc))));

function CalendarPanel() {
  const dayEvents = eventsOfDay(TOUR_EVENTS, TOUR_TODAY);
  return (
    <div className="flex flex-col gap-2">
      <WeekStrip selected={TOUR_TODAY} today={TOUR_TODAY} busyDays={BUSY_DAYS} onSelect={noop} />
      <DayTimeline events={dayEvents.filter(isRealMeeting)} dayStartMs={TOUR_TODAY} nowMs={TOUR_NOW} colorOf={colorOf} />
      <CalendarView snapshot={TOUR_SNAPSHOT} nowMs={TOUR_NOW} />
    </div>
  );
}

const PANELS: Record<TabId, () => ReactNode> = {
  calendar: () => <CalendarPanel />,
  notifications: () => <NotificationsView entries={tourHistory()} nowMs={TOUR_NOW} notificationsEnabled lastViewedAt={TOUR_NOW - 5 * 60_000} onActivate={noop} onRemove={noop} />,
  notes: () => <NotesView notes={TOUR_NOTES} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} />,
  about: () => <AboutView computerName={TOUR_SYSTEM_INFO.computerName} localIpv4={TOUR_SYSTEM_INFO.localIpv4} now={new Date(TOUR_NOW)} onCopy={never} />,
  settings: () => (
    <SettingsView
      settings={TOUR_SETTINGS}
      monitors={[]}
      info={TOUR_SYSTEM_INFO}
      diagnostics={TOUR_DIAGNOSTICS}
      snapshot={TOUR_SNAPSHOT}
      notificationStatus="allowed"
      flash={null}
      onChange={noop}
      onRequestNotificationAccess={noop}
      onCopyDiagnostics={noop}
      onOpenLogs={noop}
      onOpenCenter={noop}
      onOpenTour={noop}
    />
  ),
};

/** The expanded island as ExpandedIsland lays it out (the same PanelFrame), static. */
function ExpandedPreview({ tab, panel }: { tab: TabId; panel?: ReactNode }) {
  const config = TABS.find((candidate) => candidate.id === tab) ?? TABS[0];
  const indicator = useMotionValue(TABS.indexOf(config));
  const hasEntries = !panel || tab !== "notifications" || (panel as { props?: { entries?: unknown[] } }).props?.entries?.length !== 0;
  return (
    <PanelFrame
      title={<h2 className={PANEL_TITLE_CLASS}>{t(config.labelKey)}</h2>}
      action={config.HeaderAction && hasEntries ? <HeaderActionButton onPress={noop}>{t("notifs.clear")}</HeaderActionButton> : undefined}
      dock={<TabDock active={tab} indicator={indicator} onSelect={noop} />}
    >
      <div className={`absolute inset-0 ${panelBodyClass(tab)}`}>{panel ?? PANELS[tab]()}</div>
    </PanelFrame>
  );
}

// --- Smart search: the island's assistant card (made-up data, like the rest of the gallery). ---
const tomorrowAt = (hour: number, minute: number): number => {
  const d = new Date(TOUR_NOW);
  d.setDate(d.getDate() + 1);
  d.setHours(hour, minute, 0, 0);
  return d.getTime();
};
const CARD_BASE: AssistantCard = {
  queryId: "gallery",
  query: "",
  phase: "answer",
  lang: "he",
  title: "",
  summary: "",
  question: null,
  choices: [],
  items: [],
  total: 0,
  partial: false,
  canExtend: false,
  errorCode: null,
  sources: [],
  createdAt: TOUR_NOW,
  followUp: false,
};
const meetingItem = (id: string, hour: number, minute: number, title: string, accent: string | null, source: string | null): AssistantItem => ({
  id,
  kind: "event",
  title,
  subtitle: null,
  time: tomorrowAt(hour, minute),
  endTime: tomorrowAt(hour + 1, minute),
  accent,
  openable: false,
  unread: false,
  source,
});
const mailItem = (id: string, title: string, sender: string, mailbox: string, daysAgo: number, unread = false): AssistantItem => ({
  id,
  kind: "mail",
  title,
  subtitle: sender,
  time: TOUR_NOW - daysAgo * 86_400_000,
  endTime: null,
  accent: null,
  openable: true,
  unread,
  source: mailbox,
});
const ITZIK_CARD: AssistantCard = {
  ...CARD_BASE,
  queryId: "gallery-itzik",
  title: "מחר יש לאיציק 3 פגישות",
  summary: "09:00 · 11:30 · 14:00",
  items: [
    meetingItem("e1", 9, 0, "סטנד־אפ צוות תשתיות", "#0A84FF", "היומן של איציק"),
    meetingItem("e2", 11, 30, "סקירת תקציב רבעונית", "#BF5AF2", "היומן של איציק"),
    meetingItem("e3", 14, 0, "Vendor sync — Contoso", "#30D158", "היומן של איציק"),
  ],
  total: 3,
  sources: ["היומן של איציק"],
};
const PROCESSING_CARD: AssistantCard = { ...CARD_BASE, queryId: "gallery-working", phase: "processing" };
const MAIL_PARTIAL_CARD: AssistantCard = {
  ...CARD_BASE,
  queryId: "gallery-mail",
  title: "מצאתי 14 מיילים עם המילה תקציב",
  summary: "החיפוש עדיין לא הושלם בכל התיבות.",
  items: [
    mailItem("m1", "תקציב 2027 — גרסה סופית לאישור", "דנה כהן", "תיבת הדואר שלי", 1, true),
    mailItem("m2", "RE: Budget forecast Q4", "Michael Levi", "Finance (shared)", 2),
    mailItem("m3", "אישור חריגת תקציב מחלקת IT", "רונית אברהם", "תיבת הדואר שלי", 4),
  ],
  total: 14,
  partial: true,
  canExtend: true,
  sources: ["תיבת הדואר שלי", "Finance (shared)"],
};
const MAILBOX_CHOICES_CARD: AssistantCard = {
  ...CARD_BASE,
  queryId: "gallery-choices",
  phase: "choices",
  question: "באיזו תיבת דואר לחפש?",
  choices: [
    { id: "mb1", label: "תיבת הדואר שלי", kind: "mailbox", preferred: true },
    { id: "mb2", label: "Finance (shared)", kind: "mailbox", preferred: false },
    { id: "all", label: "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן.", kind: "allMailboxes", preferred: false },
  ],
};
const ERROR_CARD: AssistantCard = {
  ...CARD_BASE,
  queryId: "gallery-error",
  phase: "error",
  title: "לא הצלחתי לקרוא את היומן של איציק",
  summary: "פתח את היומן שלו ב-Outlook כדי לאפשר גישה, ואז נסה שוב.",
  errorCode: "MAIL-101",
};
const assistantExhibit = (id: string, label: string, card: AssistantCard): Exhibit => ({ id, label, size: assistantIslandSize(card), node: <AssistantCardView card={card} /> });

interface Exhibit {
  id: string;
  label: string;
  size: IslandSize;
  node: ReactNode;
}

function exhibits(): Exhibit[] {
  const collapsed = tourCompactContent({ nowMs: TOUR_NOW, unseen: 3 });
  const collapsedOne = tourCompactContent({ nowMs: TOUR_NOW, unseen: 1 });
  const collapsedNone = tourCompactContent({ nowMs: TOUR_NOW });
  const soon = tourCompactContent({ nowMs: TOUR_NOW, status: SOON_STATUS });
  const inMeeting = tourCompactContent({ nowMs: NOW_MOMENT, status: NOW_STATUS, silent: true });
  const inMeetingBadge = tourCompactContent({ nowMs: NOW_MOMENT, status: NOW_STATUS, unseen: 2 });
  // Muting belongs to a meeting (the ring/silent pill at its start), so the muted state is a meeting one.
  const mutedCompact = tourCompactContent({ nowMs: NOW_MOMENT, status: NOW_STATUS, unseen: 2, silent: true });
  const invite = inviteNotification();
  const toast = (n: IslandNotification): Exhibit => ({ id: "", label: "", size: toastLayout(n).size, node: <NotificationToast notification={n} onDismiss={noop} onActivate={noop} /> });
  const t2 = (id: string, label: string, n: IslandNotification): Exhibit => ({ ...toast(n), id, label });
  const center = (entries: HistoryEntry[], enabled = true) => <NotificationsView entries={entries} nowMs={TOUR_NOW} notificationsEnabled={enabled} lastViewedAt={TOUR_NOW - 5 * 60_000} onActivate={noop} onRemove={noop} />;
  return [
    { id: "compact-date", label: "Compact · date/time/day + count", size: collapsed.size, node: <CompactIsland content={collapsed} /> },
    { id: "compact-dot", label: "Compact · one unseen", size: collapsedOne.size, node: <CompactIsland content={collapsedOne} /> },
    { id: "compact-plain", label: "Compact · nothing unseen", size: collapsedNone.size, node: <CompactIsland content={collapsedNone} /> },
    { id: "compact-soon", label: "Compact · countdown", size: soon.size, node: <CompactIsland content={soon} /> },
    { id: "compact-muted", label: "Compact · meeting muted + count", size: mutedCompact.size, node: <CompactIsland content={mutedCompact} /> },
    { id: "compact-now", label: "Compact · meeting + progress + silent", size: inMeeting.size, node: <CompactIsland content={inMeeting} /> },
    { id: "compact-now-badge", label: "Compact · meeting + count", size: inMeetingBadge.size, node: <CompactIsland content={inMeetingBadge} /> },
    { id: "ringer", label: "Ring / silent", size: ringerSize(), node: <RingerPill ringer={tourRinger(true)} /> },
    { id: "alert", label: "Meeting alert · join + snooze", size: alertIslandSize(TOUR_ALERT), node: <MeetingAlert alert={TOUR_ALERT} /> },
    { id: "alert-now", label: "Meeting alert · starting now", size: alertIslandSize(SOON_ALERT), node: <MeetingAlert alert={SOON_ALERT} /> },
    { id: "alert-long", label: "Meeting alert · long Hebrew", size: alertIslandSize(LONG_ALERT), node: <MeetingAlert alert={LONG_ALERT} /> },
    assistantExhibit("assistant-processing", "Assistant · working", PROCESSING_CARD),
    assistantExhibit("assistant-answer", "Assistant · answer, 3 meetings", ITZIK_CARD),
    assistantExhibit("assistant-choices", "Assistant · which mailbox?", MAILBOX_CHOICES_CARD),
    assistantExhibit("assistant-partial", "Assistant · partial mail answer, extend", MAIL_PARTIAL_CARD),
    assistantExhibit("assistant-error", "Assistant · error", ERROR_CARD),
    t2("invite", "Invitation · three actions", invite),
    t2("teams", "Teams", TEAMS_NOTIFICATION),
    t2("teams-long", "Teams · long mixed", TEAMS_LONG),
    t2("snipping", "Snipping Tool", SNIPPING),
    t2("snipping-short", "Snipping Tool · title only", SNIPPING_SHORT),
    t2("outlook-en", "Outlook · long English", OUTLOOK_EN),
    t2("long-app", "Long app name · mixed", LONG_APP),
    t2("mixed", "Mixed bidi", MIXED),
    t2("long-hebrew", "Long Hebrew title", LONG_HEBREW_TITLE),
    { id: "panel-notifications", label: "Panel · notifications", size: expandedSize(), node: <ExpandedPreview tab="notifications" /> },
    { id: "panel-notifications-empty", label: "Panel · notifications empty", size: expandedSize(), node: <ExpandedPreview tab="notifications" panel={center([])} /> },
    { id: "panel-notifications-1", label: "Panel · 1 notification", size: expandedSize(), node: <ExpandedPreview tab="notifications" panel={center(manyEntries(1).slice(0, 1).map((e) => ({ ...e, notification: TEAMS_LONG })))} /> },
    { id: "panel-notifications-30", label: "Panel · 30 notifications", size: expandedSize(), node: <ExpandedPreview tab="notifications" panel={center(manyEntries(30))} /> },
    { id: "panel-notifications-mixed", label: "Panel · mixed / long", size: expandedSize(), node: <ExpandedPreview tab="notifications" panel={center([LONG_APP, MIXED, LONG_HEBREW_TITLE, SNIPPING].map((n, i) => ({ notification: n, receivedAt: TOUR_NOW - (i + 1) * 9 * 60_000, silenced: false })), false)} /> },
    { id: "panel-notifications-bidi", label: "Panel · bidi strings", size: expandedSize(), node: <ExpandedPreview tab="notifications" panel={center(BIDI_ENTRIES)} /> },
    { id: "panel-calendar", label: "Panel · calendar", size: expandedSize(), node: <ExpandedPreview tab="calendar" /> },
    { id: "panel-notes", label: "Panel · notes", size: expandedSize(), node: <ExpandedPreview tab="notes" /> },
    { id: "panel-notes-empty", label: "Panel · notes empty", size: expandedSize(), node: <ExpandedPreview tab="notes" panel={<NotesView notes={[]} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} />} /> },
    { id: "panel-notes-composer", label: "Panel · notes empty, writing in the island", size: expandedSize(), node: <ExpandedPreview tab="notes" panel={<NotesView notes={[]} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} onOpenApp={noop} composer={<NoteComposer value="" onChange={noop} onSave={noop} />} />} /> },
    { id: "panel-notes-sticky", label: "Panel · notes + Windows Sticky Notes", size: expandedSize(), node: <ExpandedPreview tab="notes" panel={<NotesView notes={TOUR_NOTES.slice(0, 2)} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} onOpenApp={noop} composer={<NoteComposer value="" onChange={noop} onSave={noop} />} sticky={{ snapshot: STICKY_SAMPLE, onOpen: noop }} />} /> },
    { id: "panel-notes-sticky-none", label: "Panel · notes + no Sticky Notes yet", size: expandedSize(), node: <ExpandedPreview tab="notes" panel={<NotesView notes={TOUR_NOTES.slice(0, 1)} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} onOpenApp={noop} sticky={{ snapshot: { availability: "noData", notes: [], revision: 1 }, onOpen: noop }} />} /> },
    { id: "panel-notes-sticky-unavailable", label: "Panel · notes + Sticky Notes not available", size: expandedSize(), node: <ExpandedPreview tab="notes" panel={<NotesView notes={TOUR_NOTES.slice(0, 1)} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} onOpenApp={noop} sticky={{ snapshot: { availability: "unsupported", notes: [], revision: 1 }, onOpen: noop }} />} /> },
    { id: "panel-settings", label: "Panel · settings", size: expandedSize(), node: <ExpandedPreview tab="settings" /> },
    { id: "panel-about", label: "Panel · about", size: expandedSize(), node: <ExpandedPreview tab="about" /> },
  ];
}

const ISLAND_EDGE = "inset 0 0 0 0.5px rgba(255,255,255,0.1)";

function Island({ size, children }: { size: IslandSize; children: ReactNode }) {
  return (
    <div dir="ltr" data-testid="island" className="relative overflow-hidden" style={{ width: size.width, height: size.height, borderRadius: size.radius, background: "#000", boxShadow: ISLAND_EDGE }}>
      {children}
    </div>
  );
}

/** One exhibit alone at its exact size on a flat stage (visual tests): /gallery.html?exhibit=<id>. */
function Stage({ exhibit }: { exhibit: Exhibit }) {
  return (
    <div dir="ltr" data-stage={exhibit.id} style={{ position: "fixed", inset: 0, background: "#2a2a2e" }}>
      <div style={{ position: "absolute", left: 16, top: 16 }}>
        <Island size={exhibit.size}>{exhibit.node}</Island>
      </div>
    </div>
  );
}

export function Gallery({ only, exhibit = null }: { only: string | null; exhibit?: string | null }) {
  const found = exhibit ? exhibits().find((candidate) => candidate.id === exhibit) : undefined;
  if (found) return <Stage exhibit={found} />;
  const list = exhibits().filter((exhibit) => !only || only.split(",").includes(exhibit.id));
  return (
    <div dir="ltr" style={{ minHeight: "100vh", background: "linear-gradient(160deg,#5b6b82,#8d97a8)", padding: 24, display: "flex", flexWrap: "wrap", gap: 28, alignItems: "flex-start" }}>
      {list.map((exhibit) => (
        <figure key={exhibit.id} data-exhibit={exhibit.id} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 8 }}>
          <Island size={exhibit.size}>{exhibit.node}</Island>
          <figcaption style={{ font: "500 11px Segoe UI", color: "rgba(255,255,255,0.85)" }}>
            {exhibit.label} · {exhibit.size.width}×{exhibit.size.height} r{Math.round(exhibit.size.radius)}
          </figcaption>
        </figure>
      ))}
    </div>
  );
}
