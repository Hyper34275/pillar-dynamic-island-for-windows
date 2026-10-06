import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion, useSpring } from "motion/react";
import { usePillState } from "../../hooks/usePillState";
import { useSettings } from "../../hooks/useSettings";
import { useToday } from "../../hooks/useClock";
import { useNotifications } from "../../hooks/useNotifications";
import { useIslandEvents } from "../../hooks/useIslandEvents";
import { useScreenReader, ScreenReaderLiveRegions } from "../../hooks/useScreenReader";
import { useDesktopGestures } from "../../hooks/useDesktopGestures";
import { useNativePointer, type HitRect, type OutsidePress } from "../../hooks/useNativePointer";
import { APP_NAME } from "../../lib/appInfo";
import { fullDate } from "../../lib/dateFormat";
import { t } from "../../lib/i18n";
import { dlog } from "../../lib/debugLog";
import { createFocusTrap } from "../../utils/focusTrap";
import { bootAnimationDuration, compactSize, expandedSize, pillDimensions, springConfig, windowMargin } from "./animations";
import { CompactIsland } from "./CompactIsland";
import { ContextMenu } from "./ContextMenu";
import { ExpandedIsland } from "./ExpandedIsland";
import { NotificationToast, TOAST_GAP } from "./NotificationToast";
import { TABS, type TabId } from "./tabs";
import { useCompactLabels } from "./useCompactLayout";
import { usePillGeometry } from "./usePillGeometry";

// Comfort margin around the island's hit region (CSS px).
const HIT_PADDING = 4;
// In the DOM fallback, a blur this soon after expanding is the expand itself
// (tray menu closing, focus settling) — not the user clicking away.
const BLUR_GRACE_MS = 300;

// Constant on purpose: an animated shadow would repaint every frame of every morph.
const ISLAND_SHADOW = "0 0 0 0.5px rgba(255,255,255,0.08), 0 6px 18px rgba(0,0,0,0.45)";

interface PillShellProps {
  /** The island is slid off-screen (fullscreen app): clear the hit region. */
  suspended?: boolean;
}

export function PillShell({ suspended = false }: PillShellProps) {
  const {
    isBooting,
    isHovering,
    isExpanded,
    pointerEnter,
    pointerLeave,
    expand,
    collapse,
    completeBootAnimation,
  } = usePillState();

  const reducedMotion = useReducedMotion() ?? false;
  const { settings } = useSettings();
  const today = useToday();
  const labels = useCompactLabels(today);
  const notifications = useNotifications(settings.notificationsEnabled);
  const { announce, politeAnnouncement, assertiveAnnouncement } = useScreenReader({
    defaultPriority: "polite",
    announcementDelay: 100,
    deduplicate: true,
    deduplicationWindow: 5000,
  });

  const containerRef = useRef<HTMLDivElement>(null);
  const expandedContentRef = useRef<HTMLDivElement>(null);
  const [activeTab, setActiveTab] = useState<TabId>("datetime");
  const [bootPhase, setBootPhase] = useState<"dot" | "morph">("dot");

  // ---------------------------------------------------------------------------
  // Geometry: the island's own size (animated by springs) and the native window
  // that must contain it (sent through the ordered resize queue).
  // ---------------------------------------------------------------------------
  const showToast = !isExpanded && notifications.toast !== null;
  const island = isExpanded ? expandedSize() : compactSize(labels.contentWidth, isHovering);

  const windowGeometry = useMemo(() => {
    if (isExpanded) {
      const e = pillDimensions.expanded;
      return { width: e.width + windowMargin.expandedX, height: e.height + windowMargin.expandedY };
    }
    // Sized for the HOVER pill: a window the size of the idle pill would clip it and
    // make hover flicker as the cursor "left" the window while still on the island.
    const hover = compactSize(labels.contentWidth, true);
    const width = hover.width + windowMargin.collapsedX;
    const height = hover.height + windowMargin.collapsedY;
    return showToast
      ? { width: Math.max(width, windowMargin.toastWidth), height: height + windowMargin.toastY }
      : { width, height };
  }, [isExpanded, labels.contentWidth, showToast]);

  const width = useSpring(pillDimensions.boot.width, reducedMotion ? springConfig.instant : springConfig.island);
  const height = useSpring(pillDimensions.boot.height, reducedMotion ? springConfig.instant : springConfig.island);
  const radius = useSpring(pillDimensions.boot.radius, reducedMotion ? springConfig.instant : springConfig.island);

  const { invalidate: invalidateGeometry } = usePillGeometry(windowGeometry, { width, height });

  // Boot: dot → morph into the compact pill → interactive.
  useEffect(() => {
    if (!isBooting) return;
    const compact = compactSize(labels.contentWidth, false);
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

  // Follow the target size once booted (hover, expand, collapse, new date text).
  useEffect(() => {
    if (isBooting) return;
    width.set(island.width);
    height.set(island.height);
    radius.set(island.radius);
  }, [isBooting, island.width, island.height, island.radius, width, height, radius]);

  const visualState = isExpanded ? "expanded" : isHovering ? "hover" : "idle";
  useEffect(() => {
    dlog("info", "pill", `visualState -> ${visualState}${isBooting ? " (booting)" : ""}`);
  }, [visualState, isBooting]);
  useEffect(() => {
    dlog("info", "pill", `activeTab -> ${activeTab}`);
  }, [activeTab]);

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
      setActiveTab(TABS[next].id);
    },
    [activeTabIndex]
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
  // Backend events
  // ---------------------------------------------------------------------------
  useIslandEvents({
    onToggle: (tab) => {
      if (!isExpanded) {
        if (tab) setActiveTab(tab);
        expand("island-toggle");
      } else if (tab && tab !== activeTab) {
        setActiveTab(tab);
      } else {
        collapse("island-toggle");
      }
    },
    onDisplayChanged: invalidateGeometry,
  });

  // ---------------------------------------------------------------------------
  // Gestures + context menu
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
      if (!isExpanded) expand("long press");
    },
  });

  // ---------------------------------------------------------------------------
  // Pointer tracking: native (global cursor + outside presses) or DOM fallback.
  // The window is small and transparent, so DOM enter/leave/click only see the cursor
  // over OUR window; the backend tracks the global cursor against the hit region
  // pushed here, and we fall back to DOM events + window blur when that's unavailable.
  // ---------------------------------------------------------------------------
  const [viewport, setViewport] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  useEffect(() => {
    const onResize = () =>
      setViewport((prev) =>
        prev.width === window.innerWidth && prev.height === window.innerHeight
          ? prev
          : { width: window.innerWidth, height: window.innerHeight }
      );
    onResize();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // Toast layout size (enter/exit transforms are ignored — approximate is fine).
  const [toastSize, setToastSize] = useState<{ w: number; h: number } | null>(null);
  const toastObserverRef = useRef<ResizeObserver | null>(null);
  const toastWrapperRef = useCallback((el: HTMLDivElement | null) => {
    toastObserverRef.current?.disconnect();
    toastObserverRef.current = null;
    if (!el) {
      setToastSize(null);
      return;
    }
    const measure = () => {
      const rect = el.getBoundingClientRect();
      const next = { w: Math.ceil(rect.width), h: Math.ceil(rect.height) };
      setToastSize((prev) => (prev && prev.w === next.w && prev.h === next.h ? prev : next));
    };
    measure();
    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver(measure);
      observer.observe(el);
      toastObserverRef.current = observer;
    }
  }, []);

  // Hit region from TARGET geometry, not the animated springs, so it reflects where the island is going.
  const hitRects = useMemo<HitRect[]>(() => {
    if (suspended) return [];
    // The context menu overlay covers the whole window — clicks on it aren't "outside".
    if (contextMenu.isOpen) return [{ x: 0, y: 0, w: viewport.width, h: viewport.height }];
    // The island sits at the top-center of the viewport; pad the sides and bottom (top is the screen edge).
    const rects: HitRect[] = [
      {
        x: (viewport.width - island.width) / 2 - HIT_PADDING,
        y: 0,
        w: island.width + HIT_PADDING * 2,
        h: island.height + HIT_PADDING,
      },
    ];
    if (showToast && toastSize) {
      rects.push({
        x: (viewport.width - toastSize.w) / 2 - HIT_PADDING,
        y: island.height + TOAST_GAP - HIT_PADDING,
        w: toastSize.w + HIT_PADDING * 2,
        h: toastSize.h + HIT_PADDING * 2,
      });
    }
    return rects;
  }, [suspended, contextMenu.isOpen, viewport.width, viewport.height, island.width, island.height, showToast, toastSize]);

  const handleNativePointerChange = useCallback(
    (inside: boolean) => {
      dlog("debug", "pointer", `native: pointer ${inside ? "entered" : "left"} island`);
      if (inside) pointerEnter();
      else pointerLeave();
    },
    [pointerEnter, pointerLeave]
  );

  const handleOutsidePress = useCallback(
    (press: OutsidePress) => {
      if (contextMenu.isOpen) closeContextMenu();
      if (!isExpanded) return;
      collapse(`outside press (${press.button} button at ${Math.round(press.x)},${Math.round(press.y)})`);
    },
    [contextMenu.isOpen, closeContextMenu, isExpanded, collapse]
  );

  const { nativeActive } = useNativePointer({
    rects: hitRects,
    viewportWidth: viewport.width,
    armed: isExpanded && !suspended,
    onPointerChange: handleNativePointerChange,
    onOutsidePress: handleOutsidePress,
  });

  // DOM fallback: clicks on other apps are invisible to the page, so also close on window blur.
  useEffect(() => {
    if (nativeActive || !isExpanded) return;
    const expandedAt = performance.now();
    const onBlur = () => {
      if (performance.now() - expandedAt < BLUR_GRACE_MS) {
        dlog("debug", "pill", "window blur ignored: island just expanded");
        return;
      }
      collapse("window lost focus (DOM fallback)");
    };
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  }, [nativeActive, isExpanded, collapse]);

  // Clicks inside OUR window's transparent margin. Kept alongside native presses:
  // collapse() is idempotent if both fire.
  useEffect(() => {
    if (!isExpanded) return;
    const handleGlobalClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        collapse(`outside click at ${Math.round(e.clientX)},${Math.round(e.clientY)}`);
      }
    };
    // Slight delay so the click that expanded the island doesn't immediately close it.
    const timeoutId = setTimeout(() => document.addEventListener("click", handleGlobalClick), 100);
    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener("click", handleGlobalClick);
    };
  }, [isExpanded, collapse]);

  // Focus trap while expanded.
  useEffect(() => {
    if (!isExpanded || !expandedContentRef.current) return;
    return createFocusTrap({
      container: expandedContentRef.current,
      restoreFocus: containerRef.current,
      initialFocus: true,
    });
  }, [isExpanded]);

  // ---------------------------------------------------------------------------
  // Keyboard
  // ---------------------------------------------------------------------------
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isExpanded) {
        collapse("escape key");
        return;
      }

      // Ctrl+Shift+Space: toggle expand/collapse (expand works from idle or hover).
      if (e.key === " " && e.ctrlKey && e.shiftKey) {
        e.preventDefault();
        if (isExpanded) collapse("ctrl+shift+space shortcut");
        else expand("ctrl+shift+space shortcut");
        return;
      }

      if (!isExpanded) return;

      // Arrow keys navigate the tab strip. The layout is physically LTR, so Left is always "previous".
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        goToTab(e.key === "ArrowLeft" ? -1 : 1);
      } else if (e.key === "Home" || e.key === "End") {
        e.preventDefault();
        setActiveTab(e.key === "Home" ? TABS[0].id : TABS[TABS.length - 1].id);
      }
      // Tab / Shift+Tab is intentionally NOT handled: it must do normal DOM focus
      // traversal so keyboard users can reach the controls inside the active panel.
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isExpanded, collapse, expand, goToTab]);

  const ariaLabel = isExpanded ? t("island.expandedLabel", { app: APP_NAME }) : `${fullDate(today)}. ${t("island.open")}`;

  return (
    <>
      {/* Live regions stay mounted whether collapsed or expanded. */}
      <ScreenReaderLiveRegions polite={politeAnnouncement} assertive={assertiveAnnouncement} />
      <motion.div
        ref={containerRef}
        dir="ltr"
        className="relative cursor-pointer"
        role={isExpanded ? "dialog" : "button"}
        aria-label={ariaLabel}
        aria-modal={isExpanded ? "true" : undefined}
        aria-expanded={isExpanded ? "true" : "false"}
        tabIndex={isExpanded ? -1 : 0}
        data-expanded={isExpanded ? "true" : "false"}
        style={{
          width,
          height,
          borderRadius: radius,
          boxShadow: ISLAND_SHADOW,
          overflow: "visible",
          background:
            isBooting && bootPhase === "dot"
              ? "radial-gradient(circle, rgba(255,255,255,0.85) 0%, rgba(200,200,200,0.6) 100%)"
              : "#000",
        }}
        initial={{ opacity: 0, scale: 0 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={reducedMotion ? { duration: 0.1, ease: "easeOut" } : springConfig.entrance}
        whileTap={!isExpanded && !reducedMotion ? { scale: 0.97 } : undefined}
        // DOM hover only when native tracking is unavailable — the backend's pill-pointer
        // events are the single source of truth otherwise.
        onMouseEnter={() => {
          if (!nativeActive) pointerEnter();
        }}
        onMouseLeave={() => {
          if (!nativeActive) pointerLeave();
        }}
        onPointerDown={gestureHandlers.onPointerDown}
        onPointerMove={gestureHandlers.onPointerMove}
        onPointerUp={gestureHandlers.onPointerUp}
        onContextMenu={gestureHandlers.onContextMenu}
        // Expands from idle too: hover is entered 100ms after the pointer arrives, so a
        // quick click must not depend on it.
        onClick={() => expand("click")}
        onKeyDown={(e) => {
          if (!isExpanded && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            expand(`keyboard (${e.key === " " ? "space" : "enter"})`);
          }
        }}
      >
        {settings.notificationsEnabled && (
          <NotificationToast
            notification={showToast ? notifications.toast : null}
            reducedMotion={reducedMotion}
            onDismiss={notifications.dismissToast}
            onActivate={notifications.activate}
            wrapperRef={toastWrapperRef}
          />
        )}

        <AnimatePresence>
          {!isBooting && !isExpanded && (
            <CompactIsland key="compact" labels={labels} unseen={settings.notificationsEnabled ? unseen : 0} reducedMotion={reducedMotion} />
          )}
        </AnimatePresence>

        {isExpanded && (
          <ExpandedIsland
            containerRef={expandedContentRef}
            activeTab={activeTab}
            direction={tabDirRef.current.dir}
            reducedMotion={reducedMotion}
            notificationStatus={notifications.status}
            onRequestNotificationAccess={notifications.requestAccess}
            onSelectTab={setActiveTab}
          />
        )}

        {contextMenu.isOpen && (
          <ContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            onClose={closeContextMenu}
            items={[
              {
                label: isExpanded ? t("ctx.collapse") : t("ctx.expand"),
                run: () => (isExpanded ? collapse("context menu") : expand("context menu")),
              },
              ...(isExpanded
                ? [
                    { label: t("ctx.prevTab"), run: () => goToTab(-1) },
                    { label: t("ctx.nextTab"), run: () => goToTab(1) },
                  ]
                : []),
            ]}
          />
        )}
      </motion.div>
    </>
  );
}
