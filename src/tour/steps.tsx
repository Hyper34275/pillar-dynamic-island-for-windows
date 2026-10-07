import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useSpring } from "motion/react";
import { alertIslandSize } from "../components/Pill/alertLayout";
import { expandedSize, layerFade, notificationSize, ringerSize, type IslandSize } from "../components/Pill/animations";
import { CompactIsland } from "../components/Pill/CompactIsland";
import { IslandLayer } from "../components/Pill/IslandLayer";
import { MeetingAlert } from "../components/Pill/MeetingAlert";
import { NotificationToast } from "../components/Pill/NotificationToast";
import { AboutView } from "../components/Pill/panels/AboutTab";
import { CalendarView, DayView } from "../components/Pill/panels/CalendarTab";
import { DayTimeline } from "../components/Pill/panels/DayTimeline";
import { NotesView } from "../components/Pill/panels/NotesTab";
import { NotificationsView } from "../components/Pill/panels/NotificationsTab";
import { SettingsView } from "../components/Pill/panels/SettingsTab";
import { WeekStrip } from "../components/Pill/panels/WeekStrip";
import { RingerPill } from "../components/Pill/RingerPill";
import { TabDock } from "../components/Pill/TabDock";
import { TABS, type TabId } from "../components/Pill/tabs";
import { colorOf } from "../components/Pill/ui/eventColor";
import { APP_NAME } from "../lib/appInfo";
import { eventsOfDay } from "../lib/calendar/dayRange";
import { isRealMeeting } from "../lib/calendar/select";
import { startOfDay } from "../lib/dateFormat";
import { t, type MessageKey } from "../lib/i18n";
import { useAutoScroll } from "./autoScroll";
import { tourCompactContent } from "./compact";
import { CrossFade } from "./crossFade";
import {
  inviteNotification,
  NOW_MOMENT,
  NOW_STATUS,
  SOON_STATUS,
  TEAMS_NOTIFICATION,
  TOUR_ALERT,
  TOUR_DAYS,
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
} from "./mockData";

/** The calendar step flips to the next day this often (while the step is shown). */
export const DAY_SWITCH_MS = 1400;
/** The ring / silent step switches to silent this long after it appears. */
export const RINGER_SWITCH_MS = 1200;
/** About's clock moves on once a second, as the real one does. */
const CLOCK_TICK_MS = 1000;

/** The dock capsule's slide between tabs: critically damped, about 0.2 s (the island's own tab motion is its own session's business). */
const DOCK_SPRING = { type: "spring" as const, stiffness: 500, damping: 45, mass: 1 };

const noop = () => {};
const never = async () => false;

export interface TourStep {
  id: string;
  /** What the island shows: steps with the same layer keep it mounted, so only its inside changes. */
  layer: string;
  /** The island's size for this step, from the same functions the real island uses. */
  size: IslandSize;
  content: ReactNode;
  /** A page of the Island Center the explanation links to. */
  action?: { page: "notes" | "settings"; labelKey: MessageKey };
}

// -----------------------------------------------------------------------------
// Steps that move on their own (each has one timer, only while it is on screen)
// -----------------------------------------------------------------------------

function RingerStep() {
  const [silent, setSilent] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setSilent(true), RINGER_SWITCH_MS);
    return () => clearTimeout(timer);
  }, []);
  return <RingerPill ringer={tourRinger(silent)} />;
}

const BUSY_DAYS: ReadonlySet<number> = new Set(TOUR_EVENTS.filter(isRealMeeting).map((event) => startOfDay(Date.parse(event.startUtc))));

/** The calendar tab with the week strip, the day's timeline and its meetings; it flips through three days. */
function CalendarDemo() {
  const [index, setIndex] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => setIndex((i) => (i + 1) % TOUR_DAYS.length), DAY_SWITCH_MS);
    return () => clearInterval(timer);
  }, []);
  const selected = TOUR_DAYS[index];
  const dayEvents = useMemo(() => eventsOfDay(TOUR_EVENTS, selected), [selected]);
  return (
    <div dir="ltr" className="flex flex-col gap-2.5">
      <WeekStrip selected={selected} today={TOUR_TODAY} busyDays={BUSY_DAYS} onSelect={noop} />
      <DayTimeline events={dayEvents.filter(isRealMeeting)} dayStartMs={selected} nowMs={TOUR_NOW} colorOf={colorOf} />
      {selected === TOUR_TODAY ? (
        <CalendarView snapshot={TOUR_SNAPSHOT} nowMs={TOUR_NOW} />
      ) : (
        <DayView nowMs={TOUR_NOW} day={{ state: "ready", events: dayEvents, fetchedAt: TOUR_NOW }} status="connected" />
      )}
    </div>
  );
}

function AboutDemo() {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => setSeconds((s) => s + 1), CLOCK_TICK_MS);
    return () => clearInterval(timer);
  }, []);
  return (
    <AboutView computerName={TOUR_SYSTEM_INFO.computerName} localIpv4={TOUR_SYSTEM_INFO.localIpv4} now={new Date(TOUR_NOW + seconds * 1000)} onCopy={never} />
  );
}

const PANELS: Record<TabId, () => ReactNode> = {
  calendar: () => <CalendarDemo />,
  notifications: () => <NotificationsView entries={tourHistory()} nowMs={TOUR_NOW} notificationsEnabled onActivate={noop} onRemove={noop} onClear={noop} />,
  notes: () => <NotesView notes={TOUR_NOTES} nowMs={TOUR_NOW} onNew={noop} onOpen={noop} onTogglePin={noop} onCopy={noop} onRemove={noop} />,
  about: () => <AboutDemo />,
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

/**
 * The expanded island, built like ExpandedIsland: title, the tab's panel and the real dock. The
 * same frame stays mounted while the tour goes from tab to tab, so the dock's highlight slides on
 * its spring and the title and panel cross-fade, as they do when a person switches tabs.
 */
function ExpandedFrame({ tab }: { tab: TabId }) {
  const config = TABS.find((candidate) => candidate.id === tab) ?? TABS[0];
  const index = TABS.indexOf(config);
  const indicator = useSpring(index, DOCK_SPRING);
  // The stage is inert, so the panel is moved by code: Settings, Calendar and the lists show their lower parts too.
  const panelHost = useRef<HTMLDivElement>(null);
  useAutoScroll(panelHost, tab);
  useEffect(() => {
    indicator.set(index);
  }, [index, indicator]);
  return (
    <IslandLayer
      name="expanded"
      fade={layerFade.expanded}
      size={expandedSize()}
      dir="ltr"
      className="island-expanded flex flex-col pt-4 pb-2 px-4 cursor-default text-white"
      role="region"
      aria-label={t("island.expandedLabel", { app: APP_NAME })}
    >
      <div className="relative flex-shrink-0 mb-3 px-1 h-[22px]">
        <CrossFade id={tab} className="px-1">
          <h2 className="text-white text-[20px] font-bold tracking-tight leading-none" dir="auto">
            {t(config.labelKey)}
          </h2>
        </CrossFade>
      </div>
      <div ref={panelHost} className="flex-1 min-h-0 overflow-hidden w-full relative">
        <CrossFade id={tab} className={`flex flex-col px-1 ${tab === "about" ? "overflow-hidden" : "overflow-y-auto island-scroll pb-3"}`}>
          {PANELS[tab]()}
        </CrossFade>
      </div>
      <TabDock active={tab} indicator={indicator} onSelect={noop} />
    </IslandLayer>
  );
}

// -----------------------------------------------------------------------------
// The twelve steps, in the order of the tour
// -----------------------------------------------------------------------------

function expandedStep(tab: TabId, action?: TourStep["action"]): TourStep {
  return { id: `tab-${tab}`, layer: "expanded", size: expandedSize(), content: <ExpandedFrame tab={tab} />, action };
}

/**
 * Builds the steps. Sizes are measured here (canvas text measurement), so it runs once, in the
 * browser; the sizes come from the functions PillShell uses.
 */
export function buildSteps(): TourStep[] {
  const invite = inviteNotification();
  const collapsed = tourCompactContent({ nowMs: TOUR_NOW, unseen: 3 });
  const soon = tourCompactContent({ nowMs: TOUR_NOW, status: SOON_STATUS });
  const inMeeting = tourCompactContent({ nowMs: NOW_MOMENT, status: NOW_STATUS, silent: true });
  return [
    { id: "collapsed", layer: "collapsed", size: collapsed.size, content: <CompactIsland content={collapsed} /> },
    { id: "soon", layer: "soon", size: soon.size, content: <CompactIsland content={soon} /> },
    { id: "in-meeting", layer: "in-meeting", size: inMeeting.size, content: <CompactIsland content={inMeeting} /> },
    { id: "reminder", layer: "reminder", size: alertIslandSize(TOUR_ALERT), content: <MeetingAlert alert={TOUR_ALERT} /> },
    { id: "ringer", layer: "ringer", size: ringerSize(), content: <RingerStep /> },
    {
      id: "invitation",
      layer: "invitation",
      size: notificationSize(invite.body !== "", true),
      content: <NotificationToast notification={invite} onDismiss={noop} onActivate={noop} />,
    },
    {
      id: "windows-notification",
      layer: "windows-notification",
      size: notificationSize(TEAMS_NOTIFICATION.body !== "", false),
      content: <NotificationToast notification={TEAMS_NOTIFICATION} onDismiss={noop} onActivate={noop} />,
    },
    expandedStep("calendar"),
    expandedStep("notifications"),
    expandedStep("notes", { page: "notes", labelKey: "tour.openNotes" }),
    expandedStep("about"),
    expandedStep("settings", { page: "settings", labelKey: "tour.openSettings" }),
  ];
}

export const TOUR_STEP_COUNT = 12;
