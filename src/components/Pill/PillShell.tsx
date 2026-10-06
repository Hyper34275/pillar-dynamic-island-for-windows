import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion, useSpring } from "motion/react";
import { usePillState } from "../../hooks/usePillState";
import { useSettings } from "../../hooks/useSettings";
import { useToday } from "../../hooks/useClock";
import { useNotifications } from "../../hooks/useNotifications";
import { useIslandEvents } from "../../hooks/useIslandEvents";
import { useIslandState } from "../../hooks/useIslandState";
import { useCalendarService } from "../../hooks/useCalendar";
import { useReminders } from "../../hooks/useReminders";
import { useScreenReader, ScreenReaderLiveRegions } from "../../hooks/useScreenReader";
import { useDesktopGestures } from "../../hooks/useDesktopGestures";
import { APP_NAME } from "../../lib/appInfo";
import { fullDate } from "../../lib/dateFormat";
import { t } from "../../lib/i18n";
import { dlog } from "../../lib/debugLog";
import { bootAnimationDuration, compactSize, expandedSize, notificationSize, pillDimensions, springConfig, type IslandSize } from "./animations";
import type { ReminderStore } from "../../lib/reminders/types";
import { alertIslandSize } from "./alertLayout";
import { CompactIsland } from "./CompactIsland";
import { ContextMenu } from "./ContextMenu";
import { ExpandedIsland } from "./ExpandedIsland";
import { MeetingAlert, meetingAlertAnnouncement, meetingAlertLabel, meetingAlertSubject } from "./MeetingAlert";
import { NotificationToast } from "./NotificationToast";
import { TABS, type TabId } from "./tabs";
import { useCompactLabels } from "./useCompactLayout";
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
  const { settings } = useSettings();
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
  useReminders(islandState.showAlert, reminderStore);

  const { isBooting, completeBootAnimation, pointerEnter, pointerLeave, holdCollapsed } = usePillState({
    expanded: state.expanded,
    pinned: state.pinned,
    temporary: view.kind === "meetingAlert" || view.kind === "notification",
    expand: islandState.expand,
    pin: islandState.pin,
    collapse: islandState.collapse,
    setHovering: islandState.setHovering,
  });

  const [bootPhase, setBootPhase] = useState<"dot" | "morph">("dot");

  // ---------------------------------------------------------------------------
  // Geometry: the island's own size (animated by springs) and the native window,
  // which is exactly the island's target size (sent through the ordered resize queue).
  // ---------------------------------------------------------------------------
  const target = useMemo<IslandSize>(() => {
    switch (view.kind) {
      case "userExpanded":
        return expandedSize();
      case "meetingAlert":
        return alertIslandSize(view.alert);
      case "notification":
        return notificationSize(view.notification.body !== "");
      case "idle":
        return compactSize(labels.contentWidth);
    }
  }, [view, labels.contentWidth]);

  const width = useSpring(pillDimensions.boot.width, reducedMotion ? springConfig.instant : springConfig.island);
  const height = useSpring(pillDimensions.boot.height, reducedMotion ? springConfig.instant : springConfig.island);
  const radius = useSpring(pillDimensions.boot.radius, reducedMotion ? springConfig.instant : springConfig.island);

  const { invalidate: invalidateGeometry } = usePillGeometry(target, { width, height });

  // Boot: dot → morph into the compact pill → interactive.
  useEffect(() => {
    if (!isBooting) return;
    const compact = compactSize(labels.contentWidth);
    const morph = () => {
      setBootPhase("morph");
      width.set(compact.width);
      height.set(compact.height);
      radius.set(compact.radius);
    };
    if (reducedMotion) {
      morph();
      completeBootAnimation();
      return;
    }
    const morphTimer = setTimeout(morph, bootAnimationDuration.dotAppear);
    const doneTimer = setTimeout(completeBootAnimation, bootAnimationDuration.dotAppear + bootAnimationDuration.morphToPill);
    return () => {
      clearTimeout(morphTimer);
      clearTimeout(doneTimer);
    };
  }, [isBooting, reducedMotion, labels.contentWidth, width, height, radius, completeBootAnimation]);

  // Follow the target size once booted (expand, alert, toast, collapse, new date text).
  useEffect(() => {
    if (isBooting) return;
    width.set(target.width);
    height.set(target.height);
    radius.set(target.radius);
  }, [isBooting, target.width, target.height, target.radius, width, height, radius]);

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

  // Opening the calendar side of things is when stale data is most visible: ask for a sync.
  useEffect(() => {
    if (isExpanded) void calendar.refresh();
  }, [isExpanded, calendar]);

  // ---------------------------------------------------------------------------
  // Tabs
  // ---------------------------------------------------------------------------
  const activeTabIndex = TABS.findIndex((tab) => tab.id === activeTab);
  // Computed only when the tab actually changes, so unrelated re-renders during the
  // transition can't flip the slide direction.
  const tabDirRef = useRef<{ index: number; dir: 1 | -1 }>({ index: activeTabIndex, dir: 1 });
  if (tabDirRef.current.index !== activeTabIndex) {
    tabDirRef.current = { index: activeTabIndex, dir: activeTabIndex > tabDirRef.current.index ? 1 : -1 };
  }

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
  const closeAll = useCallback(() => {
    holdCollapsed();
    if (view.kind === "meetingAlert") islandState.dismissAlert();
    else if (view.kind === "notification") islandState.dismissNotification();
    else islandState.collapse();
  }, [holdCollapsed, view.kind, islandState]);

  const toggleIsland = useCallback(
    (tab?: TabId) => {
      if (view.kind === "userExpanded") {
        if (tab && tab !== activeTab) islandState.expand(tab);
        else closeAll();
        return;
      }
      if (view.kind === "meetingAlert") islandState.dismissAlert();
      else if (view.kind === "notification") islandState.dismissNotification();
      islandState.pin(tab);
    },
    [view.kind, activeTab, islandState, closeAll]
  );

  useIslandEvents({
    onToggle: toggleIsland,
    onFullscreenChanged: setFullscreen,
    onDisplayChanged: invalidateGeometry,
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
        closeAll();
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
      : `${fullDate(today)}. ${t("island.open")}`;

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
          background:
            isBooting && bootPhase === "dot"
              ? "radial-gradient(circle, rgba(255,255,255,0.85) 0%, rgba(200,200,200,0.6) 100%)"
              : "#000",
        }}
        initial={{ opacity: 0, scale: 0 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={reducedMotion ? { duration: 0.1, ease: "easeOut" } : springConfig.entrance}
        whileTap={view.kind === "idle" && !reducedMotion ? { scale: 0.97 } : undefined}
        onPointerEnter={pointerEnter}
        onPointerLeave={pointerLeave}
        // A click anywhere inside an expanded island is the user engaging with it: keep it open.
        onPointerDownCapture={() => {
          if (isExpanded && !state.pinned) islandState.pin();
        }}
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
          else if (view.kind === "meetingAlert") closeAll();
        }}
        onKeyDown={(e) => {
          if (view.kind === "idle" && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            islandState.pin();
          }
        }}
      >
        <AnimatePresence>
          {!isBooting && view.kind === "idle" && (
            <CompactIsland key="compact" labels={labels} unseen={settings.notificationsEnabled ? unseen : 0} reducedMotion={reducedMotion} />
          )}
          {view.kind === "meetingAlert" && <MeetingAlert key={`alert-${view.alert.key}`} alert={view.alert} reducedMotion={reducedMotion} />}
          {view.kind === "notification" && (
            <NotificationToast
              key={`notification-${view.notification.id}`}
              notification={view.notification}
              reducedMotion={reducedMotion}
              onDismiss={islandState.dismissNotification}
              onActivate={notifications.activate}
            />
          )}
        </AnimatePresence>

        {isExpanded && (
          <ExpandedIsland
            activeTab={activeTab}
            direction={tabDirRef.current.dir}
            reducedMotion={reducedMotion}
            notificationStatus={notifications.status}
            onRequestNotificationAccess={notifications.requestAccess}
            onSelectTab={islandState.expand}
          />
        )}

        {contextMenu.isOpen && (
          <ContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            onClose={closeContextMenu}
            items={[
              { label: t("ctx.collapse"), run: closeAll },
              { label: t("ctx.prevTab"), run: () => goToTab(-1) },
              { label: t("ctx.nextTab"), run: () => goToTab(1) },
            ]}
          />
        )}
      </motion.div>
    </>
  );
}
