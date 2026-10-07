import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { usePillState } from "../../hooks/usePillState";
import { useSettings } from "../../hooks/useSettings";
import { useMinute, useToday } from "../../hooks/useClock";
import { useNotifications } from "../../hooks/useNotifications";
import { useIslandEvents } from "../../hooks/useIslandEvents";
import { useIslandState } from "../../hooks/useIslandState";
import { useCalendarEvents, useCalendarService } from "../../hooks/useCalendar";
import { useReminders } from "../../hooks/useReminders";
import { useMeetingSilence } from "../../hooks/useMeetingSilence";
import { useMissedReplay } from "../../hooks/useMissedReplay";
import { meetingStatus } from "../../lib/calendar/meetingStatus";
import { silence, useSilenceUntil } from "../../lib/island/silence";
import { useDoNotDisturb, useDoNotDisturbSync } from "../../lib/island/dnd";
import { useScreenReader, ScreenReaderLiveRegions } from "../../hooks/useScreenReader";
import { useDesktopGestures } from "../../hooks/useDesktopGestures";
import { APP_NAME } from "../../lib/appInfo";
import { color } from "../../design/tokens";
import { NO_LIMITS, setIslandLimits, useIslandLimits, type IslandLimits } from "../../lib/island/limits";
import { ipc, type IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { dlog } from "../../lib/debugLog";
import { bootAnimationDuration, expandedSize, ISLAND_TOP_INSET, islandSprings, limitSize, pillDimensions, ringerSize, springConfig, type IslandSize } from "./animations";
import { toastLayout } from "./toastLayout";
import { ShellContext, TransitionContext, useDrivenTransition } from "./drivenTransition";
import { useIslandMotion } from "./useIslandMotion";
import type { ReminderStore } from "../../lib/reminders/types";
import { alertIslandSize } from "./alertLayout";
import { CompactIsland } from "./CompactIsland";
import { ContextMenu } from "./ContextMenu";
import { ExpandedIsland } from "./ExpandedIsland";
import { MeetingAlert, meetingAlertAnnouncement, meetingAlertLabel, meetingAlertSubject } from "./MeetingAlert";
import { NotificationToast } from "./NotificationToast";
import { notificationAnnouncement } from "./ui/notification";
import { notificationHistory } from "../../lib/notifications/history";
import { RingerPill, ringerLabel } from "./RingerPill";
import { TABS, type TabId } from "./tabs";
import { useCompactContent, useCompactLabels } from "./useCompactLayout";
import { maxShapeSize, usePillGeometry } from "./usePillGeometry";

// Constant on purpose: an animated shadow would repaint every frame of every morph. Inset,
// because the window region is cut to the island's shape and would clip anything outside it.
const ISLAND_EDGE = `inset 0 0 0 0.5px ${color.islandEdge}`;

// The island's shape never leaves these: the launch dot below, the largest preferred shape above.
// The monitor's limits (lib/island/limits.ts) only ever lower every target, so a limited island
// stays inside these bounds and inside its (equally limited) stage window.
const SHAPE_BOUNDS = (() => {
  const max = maxShapeSize(NO_LIMITS);
  return { minWidth: pillDimensions.boot.width, minHeight: pillDimensions.boot.height, maxWidth: max.width, maxHeight: max.height };
})();

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
  // Do not disturb ended: "You missed N notifications", then each one it held.
  useMissedReplay(state, islandState.showNotification);
  const { activate: activateNotification } = notifications;
  const { pin } = islandState;
  const activateToast = useCallback(
    (notification: IslandNotification) => {
      // The missed summary leads to the list of what was missed (the Notifications tab).
      if (notification.missedSummary) pin("notifications");
      else activateNotification(notification);
    },
    [activateNotification, pin]
  );

  // The collapsed island: the date, or a meeting about to start / in progress, plus the unseen count.
  const events = useCalendarEvents();
  const minute = useMinute().getTime();
  const status = useMemo(() => meetingStatus(events, minute), [events, minute]);
  const silenceUntil = useSilenceUntil();
  // Windows "Do not disturb" (the bell in the open island): followed here so pop-ups respect it
  // from the start, and shown in the closed island like a silenced meeting.
  useDoNotDisturbSync();
  const doNotDisturb = useDoNotDisturb() === true;
  const compact = useCompactContent(
    labels,
    status,
    settings.notificationsEnabled ? notifications.unseen : 0,
    (silenceUntil !== null && silenceUntil > minute) || doNotDisturb
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
  // Geometry: one owner. The view decides the shape the island should have (`shownSize`); the
  // island motion engine (useIslandMotion) is the only thing that moves the island's width,
  // height and radius towards it, retargeting from what is on screen. The native window is a
  // fixed stage that never moves during a morph; only its region (the clickable shape) follows
  // `target` through the ordered queue (usePillGeometry). The island's size also drives every
  // island transition: each layer of content fades by how far the size has travelled
  // (drivenTransition.ts), so content never shows before the island has room for it, and never
  // vanishes while the island is still large.
  // ---------------------------------------------------------------------------
  // Every shape is held to the monitor's limits (the panel shrinks; an alert or toast only can on a
  // screen narrower than itself), so the island and its stage window never exceed the work area.
  const limits = useIslandLimits();
  const target = useMemo<IslandSize>(() => {
    switch (view.kind) {
      case "userExpanded":
        return expandedSize(limits);
      case "meetingAlert":
        return limitSize(alertIslandSize(view.alert), limits);
      case "ringer":
        return limitSize(ringerSize(), limits);
      case "notification":
        return limitSize(toastLayout(view.notification).size, limits);
      case "idle":
        return limitSize(compact.size, limits);
    }
  }, [view, compact.size, limits]);

  // The monitor's limits: asked for once at start; `display-changed` brings the new monitor's.
  // An answer that was asked for before a newer display-changed arrived is stale and dropped.
  const limitsSeqRef = useRef(0);
  useEffect(() => {
    let disposed = false;
    const seq = limitsSeqRef.current;
    void ipc.getIslandLimits().then((answer) => {
      if (!disposed && seq === limitsSeqRef.current) setIslandLimits(answer);
    });
    return () => {
      disposed = true;
    };
  }, []);

  // Boot: dot → morph into the compact pill → interactive. The dot is a state of its own (with
  // no content), held briefly so it is seen; from the morph on, the island follows the view
  // like any other transition, and booting ends when that morph has landed (not on a timer).
  const isDot = isBooting && bootPhase === "dot";
  useEffect(() => {
    if (!isBooting) return;
    if (reducedMotion) {
      setBootPhase("morph");
      completeBootAnimation();
      return;
    }
    const morphTimer = setTimeout(() => setBootPhase("morph"), bootAnimationDuration.dotAppear);
    // The latest booting may end: a hidden window (fullscreen app at launch) runs no frames,
    // and input must not wait for the morph to be seen.
    const deadline = setTimeout(completeBootAnimation, bootAnimationDuration.dotAppear + bootAnimationDuration.morphToPill);
    return () => {
      clearTimeout(morphTimer);
      clearTimeout(deadline);
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

  // Follow the shown size (boot, expand, alert, toast, collapse, new date text). The engine
  // continues from the current size and velocity, so a reversed or interrupted morph never
  // restarts, and the newest target always wins. Reduced motion keeps this (see REDUCED_MOTION
  // in animations.ts).
  const { width, height, radius, settled } = useIslandMotion(shownSize, {
    initial: pillDimensions.boot,
    bounds: SHAPE_BOUNDS,
    springs: islandSprings,
  });

  const { invalidate: invalidateGeometry } = usePillGeometry(target, { width, height });

  // Interactive as soon as the morph has landed (the boot effect above holds the deadline).
  useEffect(() => {
    if (!isBooting || isDot) return;
    if (settled.get()) {
      completeBootAnimation();
      return;
    }
    return settled.on("change", (atRest) => {
      if (atRest) completeBootAnimation();
    });
  }, [isBooting, isDot, settled, completeBootAnimation]);

  const islandDrivers = useMemo(() => [width, height], [width, height]);
  const islandTransition = useDrivenTransition(islandDrivers, [shownSize.width, shownSize.height], shownKey);
  const shell = useMemo(() => ({ transition: islandTransition, width, height }), [islandTransition, width, height]);

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
  const { markSeen, markViewed, unseen } = notifications;
  useEffect(() => {
    if (isExpanded) markSeen();
  }, [isExpanded, markSeen]);

  // Closing the island is what makes the Notifications tab's unread dots history.
  const wasExpandedRef = useRef(false);
  useEffect(() => {
    if (wasExpandedRef.current && !isExpanded) markViewed();
    wasExpandedRef.current = isExpanded;
  }, [isExpanded, markViewed]);

  // A notification shown as a toast is announced politely by name ("New notification from Teams:
  // <title>"); the toast itself never takes focus and is not an alert. Arrivals that show no toast
  // (the island is open, or toasts wait behind a meeting alert) are announced as a count.
  const shownNotification = view.kind === "notification" ? view.notification : null;
  useEffect(() => {
    if (shownNotification) announce(notificationAnnouncement(shownNotification));
  }, [shownNotification, announce]);
  const prevUnseenRef = useRef(0);
  useEffect(() => {
    if (unseen > prevUnseenRef.current && !shownNotification) announce(t("notif.announce", { n: unseen - prevUnseenRef.current }));
    prevUnseenRef.current = unseen;
  }, [unseen, announce, shownNotification]);

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
    // `show`: a request to show the island (the Center's "show in the island"), never to close it.
    (tab?: TabId, show = false) => {
      if (view.kind === "userExpanded") {
        if (tab && tab !== activeTab) islandState.expand(tab);
        else if (!show) closeAll("toggle");
        return;
      }
      if (view.kind === "meetingAlert") islandState.dismissAlert();
      else if (view.kind === "ringer") islandState.dismissRinger();
      else if (view.kind === "notification") islandState.dismissNotification();
      islandState.pin(tab);
    },
    [view.kind, activeTab, islandState, closeAll]
  );

  // A display or DPI change: take the new monitor's limits (from the event, or ask when an older
  // backend sent none), then send the stage again whatever the queue believes it already sent.
  const displayChanged = useCallback(
    (fresh: IslandLimits | null) => {
      const seq = ++limitsSeqRef.current;
      if (fresh) {
        setIslandLimits(fresh);
        invalidateGeometry();
        return;
      }
      invalidateGeometry();
      void ipc.getIslandLimits().then((answer) => {
        if (seq !== limitsSeqRef.current) return;
        setIslandLimits(answer);
        invalidateGeometry();
      });
    },
    [invalidateGeometry]
  );

  useIslandEvents({
    onToggle: toggleIsland,
    onFullscreenChanged: setFullscreen,
    onDisplayChanged: displayChanged,
    onForegroundChanged: foregroundChanged,
  });

  // ---------------------------------------------------------------------------
  // Gestures + context menu (the menu lives inside the island, so only the expanded one has room)
  // ---------------------------------------------------------------------------
  const { handlers: gestureHandlers, contextMenu, closeContextMenu } = useDesktopGestures({
    enabled: true,
    reducedMotion,
    // A swipe moves the strip like the arrows do: towards the side of the next tab (see above).
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
      // Escape closes whatever is presented (the panel, an alert, a toast, the ringer pill); it
      // never deletes anything. A control that already used the key (the note box leaving
      // itself, a confirmation cancelling) has called preventDefault or stopped the event: not ours.
      if (e.key === "Escape" && view.kind !== "idle") {
        if (e.defaultPrevented) return;
        closeAll("escape key");
        return;
      }

      // Ctrl+Shift+Space: toggle expand/collapse.
      if (e.key === " " && e.ctrlKey && e.shiftKey) {
        e.preventDefault();
        toggleIsland();
        return;
      }

      // The arrows, Home and End are NOT handled here: they belong to the tablist (TabDock) and act
      // only while a tab has focus (WAI-ARIA tabs pattern), so they never hijack a text box or a
      // control inside the panel. Tab / Shift+Tab is not handled either: normal DOM focus traversal.
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [view.kind, closeAll, toggleIsland]);

  const carriesControls = view.kind === "notification" || view.kind === "meetingAlert";
  const ariaLabel = isExpanded
    ? t("island.expandedLabel", { app: APP_NAME })
    : view.kind === "meetingAlert"
      ? `${meetingAlertLabel(view.alert)}. ${meetingAlertSubject(view.alert)}`
      : view.kind === "ringer"
        ? `${ringerLabel(view.ringer)}. ${view.ringer.phase === "start" ? t("ringer.hint") : ""}`
        : compact.ariaLabel;

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
      {/* The pointer target: the island plus the strip of ISLAND_TOP_INSET above it (the window
          region's "bridge" up to the screen's top edge). One element receives enter, leave and
          click, so moving from the strip into the island is not a leave (no hover flicker) and a
          throw against the top edge counts as a hit on the island. It is as wide as the island. */}
      <motion.div
        dir="ltr"
        className="relative cursor-pointer"
        data-island-hit=""
        style={{ width, paddingTop: ISLAND_TOP_INSET }}
        onPointerEnter={pointerEnter}
        onPointerLeave={pointerLeave}
        onPointerDown={gestureHandlers.onPointerDown}
        onPointerMove={gestureHandlers.onPointerMove}
        onPointerUp={gestureHandlers.onPointerUp}
        onContextMenu={(e) => {
          if (isExpanded) gestureHandlers.onContextMenu(e);
          else e.preventDefault();
        }}
        // Expanded content stops its own clicks, so this only sees the collapsed island, the strip and alerts.
        onClick={() => {
          if (isBooting) return;
          if (view.kind === "idle") islandState.pin();
          else if (view.kind === "meetingAlert") closeAll("meeting alert clicked");
          else if (view.kind === "ringer") toggleRinger(view.ringer);
        }}
      >
      <motion.div
        dir="ltr"
        className="relative"
        // A toast or a meeting alert carries its own buttons: the island is then a named group around
        // them, never a button that contains buttons (and a click on it does nothing a key could not).
        role={isExpanded ? "dialog" : carriesControls ? "group" : "button"}
        aria-label={view.kind === "notification" ? undefined : ariaLabel}
        aria-expanded={carriesControls ? undefined : isExpanded ? "true" : "false"}
        tabIndex={isExpanded ? -1 : carriesControls ? undefined : 0}
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
        onKeyDown={(e) => {
          if (view.kind === "idle" && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            islandState.pin();
          }
        }}
      >
        {/* One layer per kind of content. A replaced layer stays mounted (fading with the morph,
            unclickable) until it is gone, so closing never empties the island before it shrinks. */}
        <ShellContext.Provider value={shell}>
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
                onRemove={() => {
                  notificationHistory.remove(view.notification.id);
                  islandState.dismissNotification();
                }}
                onActivate={activateToast}
                onSwipeAway={() => closeAll("toast swiped away")}
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
        </ShellContext.Provider>

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
      </motion.div>
    </>
  );
}
