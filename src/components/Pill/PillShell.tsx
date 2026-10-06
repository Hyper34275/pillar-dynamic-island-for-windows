import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion, useSpring } from "motion/react";
import { usePillState } from "../../hooks/usePillState";
import { useSettings } from "../../hooks/useSettings";
import { useMinute, useToday } from "../../hooks/useClock";
import { useNotifications } from "../../hooks/useNotifications";
import { useIslandEvents } from "../../hooks/useIslandEvents";
import { useIslandState } from "../../hooks/useIslandState";
import { useCalendarEvents, useCalendarService } from "../../hooks/useCalendar";
import { useReminders } from "../../hooks/useReminders";
import { useMeetingSilence } from "../../hooks/useMeetingSilence";
import { meetingStatus } from "../../lib/calendar/meetingStatus";
import { silence, useSilenceUntil } from "../../lib/island/silence";
import { useScreenReader, ScreenReaderLiveRegions } from "../../hooks/useScreenReader";
import { useDesktopGestures } from "../../hooks/useDesktopGestures";
import { APP_NAME } from "../../lib/appInfo";
import { fullDate } from "../../lib/dateFormat";
import { t } from "../../lib/i18n";
import { dlog } from "../../lib/debugLog";
import { bootAnimationDuration, expandedSize, notificationSize, pillDimensions, ringerSize, springConfig, type IslandSize } from "./animations";
import { TransitionContext, useDrivenTransition } from "./drivenTransition";
import type { ReminderStore } from "../../lib/reminders/types";
import { alertIslandSize } from "./alertLayout";
import { CompactIsland } from "./CompactIsland";
import { ContextMenu } from "./ContextMenu";
import { ExpandedIsland } from "./ExpandedIsland";
import { MeetingAlert, meetingAlertAnnouncement, meetingAlertLabel, meetingAlertSubject } from "./MeetingAlert";
import { NotificationToast } from "./NotificationToast";
import { RingerPill, ringerLabel } from "./RingerPill";
import { TABS, type TabId } from "./tabs";
import { useCompactContent, useCompactLabels } from "./useCompactLayout";
import { usePillGeometry } from "./usePillGeometry";

// Constant on purpose: an animated shadow would repaint every frame of every morph. Inset,
// because the native window is exactly the island and would clip anything outside it.
const ISLAND_EDGE = "inset 0 0 0 0.5px rgba(255,255,255,0.1)";

interface PillShellProps {
  /** Where fired reminders are remembered; the per-user state file unless a test supplies one. */
  reminderStore?: ReminderStore;
}

export function PillShell({ reminderStore }: PillShellProps = {}) {
  const reducedMotion = useReducedMotion() ?? false;
  const { settings, loaded: settingsLoaded } = useSettings();
  const today = useToday();
  const labels = useCompactLabels(today);
  const calendar = useCalendarService();
  const { announce, politeAnnouncement, assertiveAnnouncement } = useScreenReader({
    defaultPriority: "polite",
    announcementDelay: 100,
    deduplicate: true,
    deduplicationWindow: 5000,
  });

  // A fullscreen app in front hides the native window (if the user wants that); alerts and
  // toasts then wait instead of running out unseen.
  const [fullscreen, setFullscreen] = useState(false);
  const islandState = useIslandState({ suppressed: fullscreen && settings.hideInFullscreen });
  const { state, view } = islandState;
  const isExpanded = view.kind === "userExpanded";
  const activeTab = state.tab;

  const notifications = useNotifications(settings.notificationsEnabled, islandState.showNotification);
  const { snooze } = useReminders(islandState.showAlert, reminderStore);
  useMeetingSilence(settingsLoaded ? settings.meetingSilencePrompt : null, islandState.showRinger);

  // The collapsed island: the date, or a meeting about to start / in progress, plus the unseen count.
  const events = useCalendarEvents();
  const minute = useMinute().getTime();
  const status = useMemo(() => meetingStatus(events, minute), [events, minute]);
  const silenceUntil = useSilenceUntil();
  const compact = useCompactContent(
    labels,
    status,
    settings.notificationsEnabled ? notifications.unseen : 0,
    silenceUntil !== null && silenceUntil > minute
  );

  const { isBooting, completeBootAnimation, pointerEnter, pointerLeave, holdCollapsed, foregroundChanged } = usePillState({
    expanded: state.expanded,
    temporary: view.kind === "meetingAlert" || view.kind === "notification" || view.kind === "ringer",
    expand: islandState.expand,
    pin: islandState.pin,
    collapse: islandState.collapse,
    setHovering: islandState.setHovering,
  });

  const [bootPhase, setBootPhase] = useState<"dot" | "morph">("dot");

  // ---------------------------------------------------------------------------
  // Geometry: the island's own size (animated by springs) and the native window,
  // which is exactly the island's target size (sent through the ordered resize queue).
  // The island's size also drives every island transition: each layer of content fades by
  // how far the size has travelled (drivenTransition.ts), so content never shows before the
  // island has room for it, and never vanishes while the island is still large.
  // ---------------------------------------------------------------------------
  const target = useMemo<IslandSize>(() => {
    switch (view.kind) {
      case "userExpanded":
        return expandedSize();
      case "meetingAlert":
        return alertIslandSize(view.alert);
      case "ringer":
        return ringerSize();
      case "notification":
        return notificationSize(view.notification.body !== "", !!view.notification.invite);
      case "idle":
        return compact.size;
    }
  }, [view, compact.size]);

  const width = useSpring(pillDimensions.boot.width, springConfig.island);
  const height = useSpring(pillDimensions.boot.height, springConfig.island);
  const radius = useSpring(pillDimensions.boot.radius, springConfig.island);

  const { invalidate: invalidateGeometry } = usePillGeometry(target, { width, height });

  // Boot: dot → morph into the compact pill → interactive. The dot is a state of its own (with
  // no content); from the morph on, the island follows the view like any other transition.
  const isDot = isBooting && bootPhase === "dot";
  useEffect(() => {
    if (!isBooting) return;
    if (reducedMotion) {
      setBootPhase("morph");
      completeBootAnimation();
      return;
    }
    const morphTimer = setTimeout(() => setBootPhase("morph"), bootAnimationDuration.dotAppear);
    const doneTimer = setTimeout(completeBootAnimation, bootAnimationDuration.dotAppear + bootAnimationDuration.morphToPill);
    return () => {
      clearTimeout(morphTimer);
      clearTimeout(doneTimer);
    };
  }, [isBooting, reducedMotion, completeBootAnimation]);

  // What the island shows (also the AnimatePresence key of that layer) and the size it morphs to.
  const shownKey = isDot
    ? "boot"
    : view.kind === "meetingAlert"
      ? `alert-${view.alert.key}`
      : view.kind === "ringer"
        ? `ringer-${view.ringer.key}`
        : view.kind === "notification"
        ? `notification-${view.notification.id}`
        : view.kind === "userExpanded"
          ? "expanded"
          : "compact";
  const shownSize: IslandSize = isDot ? pillDimensions.boot : target;

  // Follow the shown size (boot, expand, alert, toast, collapse, new date text). The springs
  // continue from their current value and velocity, so a reversed or interrupted morph never
  // restarts. Reduced motion keeps this (see REDUCED_MOTION in animations.ts).
  useEffect(() => {
    width.set(shownSize.width);
    height.set(shownSize.height);
    radius.set(shownSize.radius);
  }, [shownSize.width, shownSize.height, shownSize.radius, width, height, radius]);

  const islandDrivers = useMemo(() => [width, height], [width, height]);
  const islandTransition = useDrivenTransition(islandDrivers, [shownSize.width, shownSize.height], shownKey);

  useEffect(() => {
    dlog("info", "pill", `view -> ${view.kind}${isBooting ? " (booting)" : ""}`);
  }, [view.kind, isBooting]);
  useEffect(() => {
    dlog("info", "pill", `activeTab -> ${activeTab}`);
  }, [activeTab]);

  // A meeting alert is announced once, politely, by the shell's live region.
  const shownAlert = view.kind === "meetingAlert" ? view.alert : null;
  useEffect(() => {
    if (shownAlert) announce(meetingAlertAnnouncement(shownAlert));
  }, [shownAlert, announce]);

  // The ring / silent pill says what it switched to.
  const shownRinger = view.kind === "ringer" ? view.ringer : null;
  useEffect(() => {
    if (shownRinger) announce(ringerLabel(shownRinger));
  }, [shownRinger, announce]);

  // Opening the calendar side of things is when stale data is most visible: ask for a sync.
  useEffect(() => {
    if (isExpanded) void calendar.refresh();
  }, [isExpanded, calendar]);

  // ---------------------------------------------------------------------------
  // Tabs
  // ---------------------------------------------------------------------------
  const activeTabIndex = TABS.findIndex((tab) => tab.id === activeTab);

  const goToTab = useCallback(
    (direction: -1 | 1) => {
      const next = (activeTabIndex + direction + TABS.length) % TABS.length;
      islandState.expand(TABS[next].id);
    },
    [activeTabIndex, islandState]
  );

  const announcedTabRef = useRef(activeTabIndex);
  useEffect(() => {
    if (announcedTabRef.current === activeTabIndex) return;
    announcedTabRef.current = activeTabIndex;
    announce(t(TABS[activeTabIndex].labelKey));
  }, [activeTabIndex, announce]);

  // ---------------------------------------------------------------------------
  // Notifications: opening the island marks everything as seen; new arrivals are announced.
  // ---------------------------------------------------------------------------
  const { markSeen, unseen } = notifications;
  useEffect(() => {
    if (isExpanded) markSeen();
  }, [isExpanded, markSeen]);

  const prevUnseenRef = useRef(0);
  useEffect(() => {
    if (unseen > prevUnseenRef.current) announce(t("notif.announce", { n: unseen - prevUnseenRef.current }));
    prevUnseenRef.current = unseen;
  }, [unseen, announce]);

  // ---------------------------------------------------------------------------
  // Closing and toggling. Whatever closes the island while the pointer is on it must also stop
  // that same pointer from expanding it again (hover intent) until it has left.
  // ---------------------------------------------------------------------------
  const closeAll = useCallback(
    (reason: string) => {
      dlog("info", "pill", `collapse: ${reason}`);
      holdCollapsed();
      if (view.kind === "meetingAlert") islandState.dismissAlert();
      else if (view.kind === "ringer") islandState.dismissRinger();
      else if (view.kind === "notification") islandState.dismissNotification();
      else islandState.collapse();
    },
    [holdCollapsed, view.kind, islandState]
  );

  const toggleIsland = useCallback(
    (tab?: TabId) => {
      if (view.kind === "userExpanded") {
        if (tab && tab !== activeTab) islandState.expand(tab);
        else closeAll("toggle");
        return;
      }
      if (view.kind === "meetingAlert") islandState.dismissAlert();
      else if (view.kind === "ringer") islandState.dismissRinger();
      else if (view.kind === "notification") islandState.dismissNotification();
      islandState.pin(tab);
    },
    [view.kind, activeTab, islandState, closeAll]
  );

  useIslandEvents({
    onToggle: toggleIsland,
    onFullscreenChanged: setFullscreen,
    onDisplayChanged: invalidateGeometry,
    onForegroundChanged: foregroundChanged,
  });

  // ---------------------------------------------------------------------------
  // Gestures + context menu (the menu lives inside the island, so only the expanded one has room)
  // ---------------------------------------------------------------------------
  const { handlers: gestureHandlers, contextMenu, closeContextMenu } = useDesktopGestures({
    enabled: true,
    reducedMotion,
    onSwipeLeft: () => {
      if (isExpanded) goToTab(1);
    },
    onSwipeRight: () => {
      if (isExpanded) goToTab(-1);
    },
    onLongPress: () => {
      if (view.kind === "idle") islandState.pin();
    },
  });

  // ---------------------------------------------------------------------------
  // Keyboard (only reaches the page if it ever has DOM focus: the window never takes it)
  // ---------------------------------------------------------------------------
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && view.kind !== "idle") {
        closeAll("escape key");
        return;
      }

      // Ctrl+Shift+Space: toggle expand/collapse.
      if (e.key === " " && e.ctrlKey && e.shiftKey) {
        e.preventDefault();
        toggleIsland();
        return;
      }

      if (!isExpanded) return;

      // Arrow keys navigate the tab strip. The layout is physically LTR, so Left is always "previous".
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        goToTab(e.key === "ArrowLeft" ? -1 : 1);
      } else if (e.key === "Home" || e.key === "End") {
        e.preventDefault();
        islandState.expand(e.key === "Home" ? TABS[0].id : TABS[TABS.length - 1].id);
      }
      // Tab / Shift+Tab is intentionally NOT handled: it must do normal DOM focus
      // traversal so keyboard users can reach the controls inside the active panel.
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [view.kind, isExpanded, closeAll, toggleIsland, goToTab, islandState]);

  const ariaLabel = isExpanded
    ? t("island.expandedLabel", { app: APP_NAME })
    : view.kind === "meetingAlert"
      ? `${meetingAlertLabel(view.alert)}. ${meetingAlertSubject(view.alert)}`
      : view.kind === "ringer"
        ? `${ringerLabel(view.ringer)}. ${view.ringer.phase === "start" ? t("ringer.hint") : ""}`
        : compact.statusText
          ? `${compact.statusText}. ${t("island.open")}`
          : `${fullDate(today)}. ${t("island.open")}`;

  // A tap on the ring / silent pill switches it; silent holds notifications until the meeting ends.
  const toggleRinger = (ringer: NonNullable<typeof shownRinger>) => {
    if (ringer.phase === "end") {
      islandState.dismissRinger();
      return;
    }
    if (ringer.silent) silence.clear();
    else silence.until(ringer.untilMs);
    dlog("info", "pill", `ringer -> ${ringer.silent ? "ring" : "silent"}`);
    islandState.toggleRinger();
  };

  return (
    <>
      {/* Live regions stay mounted whatever the island shows. */}
      <ScreenReaderLiveRegions polite={politeAnnouncement} assertive={assertiveAnnouncement} />
      <motion.div
        dir="ltr"
        className="relative cursor-pointer"
        role={isExpanded ? "dialog" : "button"}
        aria-label={ariaLabel}
        aria-expanded={isExpanded ? "true" : "false"}
        tabIndex={isExpanded ? -1 : 0}
        data-expanded={isExpanded ? "true" : "false"}
        data-view={view.kind}
        style={{
          width,
          height,
          borderRadius: radius,
          boxShadow: ISLAND_EDGE,
          overflow: "hidden",
          background: isDot
              ? "radial-gradient(circle, rgba(255,255,255,0.85) 0%, rgba(200,200,200,0.6) 100%)"
              : "#000",
        }}
        initial={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0 }}
        animate={{ opacity: 1, scale: 1 }}
        // The launch entrance of the dot, and the release of the press feedback: no bounce.
        transition={reducedMotion ? { duration: 0.1, ease: "easeOut" } : springConfig.island}
        whileTap={view.kind === "idle" && !reducedMotion ? { scale: 0.97 } : undefined}
        onPointerEnter={pointerEnter}
        onPointerLeave={pointerLeave}
        onPointerDown={gestureHandlers.onPointerDown}
        onPointerMove={gestureHandlers.onPointerMove}
        onPointerUp={gestureHandlers.onPointerUp}
        onContextMenu={(e) => {
          if (isExpanded) gestureHandlers.onContextMenu(e);
          else e.preventDefault();
        }}
        // Expanded content stops its own clicks, so this only sees the collapsed island and alerts.
        onClick={() => {
          if (isBooting) return;
          if (view.kind === "idle") islandState.pin();
          else if (view.kind === "meetingAlert") closeAll("meeting alert clicked");
          else if (view.kind === "ringer") toggleRinger(view.ringer);
        }}
        onKeyDown={(e) => {
          if (view.kind === "idle" && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            islandState.pin();
          }
        }}
      >
        {/* One layer per kind of content. A replaced layer stays mounted (fading with the morph,
            unclickable) until it is gone, so closing never empties the island before it shrinks. */}
        <TransitionContext.Provider value={islandTransition}>
          <AnimatePresence>
            {shownKey === "compact" && <CompactIsland key="compact" content={compact} />}
            {view.kind === "meetingAlert" && !isDot && (
              <MeetingAlert
                key={shownKey}
                alert={view.alert}
                onJoin={() => closeAll("joined from the alert")}
                onSnooze={() => {
                  snooze(view.alert);
                  closeAll("alert snoozed");
                }}
              />
            )}
            {view.kind === "ringer" && !isDot && <RingerPill key={shownKey} ringer={view.ringer} />}
            {view.kind === "notification" && !isDot && (
              <NotificationToast
                key={shownKey}
                notification={view.notification}
                onDismiss={islandState.dismissNotification}
                onActivate={notifications.activate}
              />
            )}
            {isExpanded && !isDot && (
              <ExpandedIsland
                key="expanded"
                activeTab={activeTab}
                reducedMotion={reducedMotion}
                notificationStatus={notifications.status}
                onRequestNotificationAccess={notifications.requestAccess}
                onSelectTab={islandState.expand}
              />
            )}
          </AnimatePresence>
        </TransitionContext.Provider>

        {contextMenu.isOpen && (
          <ContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            onClose={closeContextMenu}
            items={[
              { label: t("ctx.collapse"), run: () => closeAll("context menu") },
              { label: t("ctx.prevTab"), run: () => goToTab(-1) },
              { label: t("ctx.nextTab"), run: () => goToTab(1) },
            ]}
          />
        )}
      </motion.div>
    </>
  );
}
