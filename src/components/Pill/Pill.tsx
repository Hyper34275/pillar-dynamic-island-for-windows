import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion, useSpring, useTransform, AnimatePresence } from "motion/react";
import { usePillState } from "../../hooks/usePillState";
import { useTimer } from "../../hooks/useTimer";
import { useMediaSession } from "../../hooks/useMediaSession";
import { useVolume } from "../../hooks/useVolume";
import { useAutoStart } from "../../hooks/useAutoStart";
import { useBrightness } from "../../hooks/useBrightness";
import { useAudioDevices } from "../../hooks/useAudioDevices";
import { usePerAppMixer } from "../../hooks/usePerAppMixer";
import { useNotifications } from "../../hooks/useNotifications";
import { useBattery } from "../../hooks/useBattery";
import { usePrismAI } from "../../hooks/usePrismAI";
import { useProductivity } from "../../hooks/useProductivity";
import { useAppearance } from "../../hooks/useAppearance";
import { useSettings } from "../../hooks/useSettings";
import { useAdaptivePolling } from "../../hooks/useAdaptivePolling";
import { useScreenReader, useAnnounceListChange, ScreenReaderLiveRegions } from "../../hooks/useScreenReader";
import { useDesktopGestures } from "../../hooks/useDesktopGestures";
import { useWorkflowEvents } from "../../hooks/useWorkflowEvents";
import { useNativePointer, type HitRect, type OutsidePress } from "../../hooks/useNativePointer";
import { NotificationToast, NotificationsList, NotificationIndicator } from "./modules/NotificationModule";
import { BatteryIndicator } from "./modules/BatteryModule";
import { springConfig, pillDimensions, bootAnimationDuration, idleSlotAnimations, getPillTargetStyle, getIdleSlotWidths, type PillVisualState, PILL_DURATION_FAST } from "./animations";
import { TimerExpanded, TIMER_TINT } from "./modules/TimerModule";
import { MediaExpanded, MediaIndicator, AlbumArt } from "./modules/MediaModule";
import { useAlbumArt } from "../../hooks/useAlbumArt";
import { Segmented, SYSTEM_COLORS } from "./ui/primitives";
import { BellIcon, CheckCircleIcon, MusicIcon, SlidersIcon, SparklesIcon, TimerIcon } from "./ui/icons";
import { QuickSettings } from "./modules/VolumeModule";
import { PrismModule } from "./modules/PrismModule";
import { ProductivityModule } from "./modules/ProductivityModule";
import { SystemMonitor } from "./modules/SystemMonitor";
import { StateIndicators, TimerMiniProgress } from "./indicators/StateIndicators";
import { createFocusTrap } from "../../utils/focusTrap";
import { tauriInvoke } from "../../lib/tauri";
import { dlog } from "../../lib/debugLog";
import { fireAndForget } from "../../lib/fireAndForget";
import { platformApi } from "../../lib/platform";
import type { PrismAction } from "../../types/prism";
import type { WorkflowActionEnvelope } from "../../types/workflows";
import { createPillThemeTokens, resolveReducedMotion } from "./themeTokens";

const TIMER_NOTIFICATION_TITLE = "PILLAR Timer Complete";

// Tab type for expanded view
type ExpandedTab = "timer" | "media" | "notifications" | "settings" | "prism" | "productivity";
const EXPANDED_TAB_CONFIG: Array<{
  id: ExpandedTab;
  label: string;
  title: string;
  Icon: typeof TimerIcon;
  ariaLabel: string;
  hasBadge?: boolean;
}> = [
  { id: "timer", label: "Timer", title: "Timer", Icon: TimerIcon, ariaLabel: "Timer module" },
  { id: "media", label: "Music", title: "Now Playing", Icon: MusicIcon, ariaLabel: "Media controls" },
  { id: "notifications", label: "Alerts", title: "Notifications", Icon: BellIcon, ariaLabel: "Notifications", hasBadge: true },
  { id: "settings", label: "Controls", title: "Control Center", Icon: SlidersIcon, ariaLabel: "Settings" },
  { id: "productivity", label: "Focus", title: "Focus", Icon: CheckCircleIcon, ariaLabel: "Productivity module" },
  { id: "prism", label: "Prism", title: "Prism", Icon: SparklesIcon, ariaLabel: "Prism AI assistant" },
];

// Relative luminance check so text on an accent fill stays legible (white accent → black text).
function contrastOn(hex: string): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return "#ffffff";
  const n = parseInt(m[1], 16);
  const lum = (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255;
  return lum > 0.62 ? "#000000" : "#ffffff";
}

// Helper to get current time string
const getTimeString = () => {
  const now = new Date();
  return now.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false });
};

const getDateString = () => {
  return new Date().toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
};

const getSecondsString = () => {
  return new Date().getSeconds().toString().padStart(2, "0");
};

// Summarize backup/import conflicts into an actionable sentence (count + first
// human-readable reason) instead of a dead-end "needs attention" message.
function describeBackupConflicts(
  result: { conflicts?: Array<{ message?: string }> } | null | undefined
): string {
  const conflicts = result?.conflicts ?? [];
  if (conflicts.length === 0) return " Check your network or storage and try again.";
  const first = conflicts[0]?.message;
  return ` ${conflicts.length} conflict${conflicts.length === 1 ? "" : "s"}.${first ? ` ${first}` : ""} Review before applying.`;
}

// True when focus is on an editable field inside the panel that holds unsaved
// (non-empty) text. Used to avoid collapsing the panel mid-edit, which would
// unmount the module and silently discard the user's typed task/note/event.
function panelHasUnsavedDraft(container: HTMLElement | null): boolean {
  const el = document.activeElement as HTMLElement | null;
  if (!el || !container || !container.contains(el)) return false;
  const editable =
    el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
  if (!editable) return false;
  const value = (el as HTMLInputElement).value ?? el.textContent ?? "";
  return value.trim().length > 0;
}

// Comfort margin around the island's hit region (CSS px).
const HIT_PADDING = 4;
// Gap between the pill and the notification toast (matches the toast wrapper's top offset).
const TOAST_GAP = 8;
// Longest we keep the expanded-size window after collapsing, waiting for the
// island's shrink spring to land inside the collapsed window bounds.
const SHRINK_MAX_DELAY_MS = 450;
// In the DOM fallback, a blur this soon after expanding is the expand itself
// (tray menu closing, focus settling) — not the user clicking away.
const BLUR_GRACE_MS = 300;

interface PillProps {
  /** App slid the island off-screen (fullscreen app) — clear the hit region. */
  suspended?: boolean;
}

export function Pill({ suspended = false }: PillProps = {}) {
  const {
    isBooting,
    isIdle,
    isHovering,
    isExpanded,
    pointerEnter,
    pointerLeave,
    // Every expand/collapse goes through these with a source/reason so the debug
    // log always records WHY the island opened or closed.
    expand,
    collapse,
    completeBootAnimation,
    // Content state API
    backgroundStates,
    setTimerState,
    setTimerAlert,
  } = usePillState();

  // Adaptive polling for reduced CPU usage
  const { triggerActivity } = useAdaptivePolling({
    baseInterval: 5000,
    activeInterval: 1000,
    idleThreshold: 30000,
    deepSleepInterval: 15000,
    deepSleepThreshold: 300000,
  });

  // Screen reader support. The live regions are rendered by THIS component (see
  // the top of the return) so they reflect the same instance that announce() drives.
  // (Previously App.tsx rendered the regions against a separate, never-announced instance.)
  const { announce, politeAnnouncement, assertiveAnnouncement } = useScreenReader({
    defaultPriority: "polite",
    announcementDelay: 100,
    deduplicate: true,
    deduplicationWindow: 5000,
  });
  const announceListChange = useAnnounceListChange(announce);

  // Timer hook
  const {
    timer,
    stats: timerStats,
    categories: timerCategories,
    selectedCategory: selectedTimerCategory,
    setSelectedCategory: setSelectedTimerCategory,
    presets,
    startTimer,
    pauseTimer,
    resumeTimer,
    stopTimer,
    dismissAlert,
    formatTime,
    progress: timerProgress,
  } = useTimer(
    // On timer update - sync to content state
    (timerState) => {
      if (timerState.isActive) {
        setTimerState({
          label: timerState.label,
          totalSeconds: timerState.totalSeconds,
          remainingSeconds: timerState.remainingSeconds,
          isPaused: timerState.isPaused,
        });
      } else if (!timerState.isComplete) {
        setTimerState(null);
      }
    },
    // On timer complete - show alert
    (label) => {
      setTimerState(null);
      setTimerAlert({ label, completedAt: Date.now() });
      void sendTimerCompletionNotification(label);
    }
  );

  const ensureTimerNotificationPermission = useCallback(async (): Promise<NotificationPermission | "unsupported"> => {
    if (typeof window === "undefined" || typeof Notification === "undefined") {
      return "unsupported";
    }
    if (Notification.permission === "granted" || Notification.permission === "denied") {
      return Notification.permission;
    }
    try {
      return await Notification.requestPermission();
    } catch {
      return "denied";
    }
  }, []);

  const sendTimerCompletionNotification = useCallback(
    async (label: string) => {
      const permission = await ensureTimerNotificationPermission();
      if (permission !== "granted") return;

      const desktopNotification = new Notification(TIMER_NOTIFICATION_TITLE, {
        body: `${label} finished. Time's up.`,
        tag: `pillar-timer-${label.toLowerCase().replace(/\s+/g, "-")}`,
        requireInteraction: true,
      });

      desktopNotification.onclick = () => {
        window.focus();
        desktopNotification.close();
      };

      window.setTimeout(() => {
        desktopNotification.close();
      }, 15000);
    },
    [ensureTimerNotificationPermission]
  );

  const startTimerWithNotification = useCallback(
    (preset: Parameters<typeof startTimer>[0]) => {
      // Don't request OS notification permission here — popping the permission
      // dialog the instant a timer starts is intrusive. It's requested lazily on
      // first completion (sendTimerCompletionNotification). The in-app "Done!"
      // alert fires regardless, so denying notifications never loses the signal.
      startTimer(preset);
    },
    [startTimer]
  );

  // Dismiss timer alert
  const handleDismissAlert = () => {
    dismissAlert();
    setTimerAlert(null);
  };

  // Media session hook
  const {
    media,
    recentSources,
    timeline,
    playbackInfo,
    playPause,
    next: mediaNext,
    previous: mediaPrevious,
    toggleRepeat,
    toggleShuffle,
    seekTo,
    pauseOtherSessions,
  } = useMediaSession(1500); // Poll every 1.5s (reduced from 600ms to avoid saturating backend)

  // Volume hook
  const {
    volume,
    setVolume,
    toggleMute,
  } = useVolume(5000); // Poll every 5 seconds

  // Auto-start hook
  const {
    isEnabled: autoStartEnabled,
    setEnabled: setAutoStartEnabled,
  } = useAutoStart();

  // Brightness hook
  const {
    brightness,
    setBrightness,
  } = useBrightness(10000); // Poll every 10 seconds

  // Audio devices hook
  const {
    devices: audioDevices,
    defaultDevice: defaultAudioDevice,
  } = useAudioDevices(15000); // Poll every 15s (devices rarely change; reduced from 5s)

  // Per-app mixer hook
  const {
    sessions: audioSessions,
    setSessionVolume,
    setSessionMute,
  } = usePerAppMixer(8000); // Poll every 8s (heavy COM operation; reduced from 3s)

  // Notifications hook
  const {
    notifications,
    history: notificationHistory,
    hasAccess: hasNotificationAccess,
    latestNotification,
    notificationPhase,
    isNewNotification,
    dismissNotification,
    clearLatest: clearLatestNotification,
    clearHistory: clearNotificationHistory,
  } = useNotifications(); // Real-time via Windows NotificationChanged event; fallback poll every 30s

  // Battery hook
  const {
    battery,
    isLow: isBatteryLow,
    isCritical: isBatteryCritical,
  } = useBattery(60000); // Poll every 60s (battery changes slowly)

  const {
    state: productivity,
    addTask,
    toggleTask,
    removeTask,
    addNote,
    updateNote,
    removeNote,
    addAgendaEvent,
    clearCompletedTasks,
    exportBackup,
    importBackup,
  } = useProductivity();

  // Appearance settings (mode, opacity, accent color)
  const appearance = useAppearance();
  const isNotch = appearance.active.mode === "notch";

  // Centralized settings (motion, behavior, timer persistence)
  const { settings: appSettings, update: updateSettings } = useSettings();

  // Album artwork for the current track (also the source of the album accent color,
  // sampled from decoded pixels in the webview).
  const albumArt = useAlbumArt(media);

  // Effective accent color: album art override or user setting
  const effectiveAccentColor = (appearance.active.useAlbumAccent && media && albumArt.color) ? albumArt.color : appearance.active.accentColor;

  // Auto-pause other sessions when a new session starts playing
  const prevMediaTitleRef = useRef<string | null>(null);
  useEffect(() => {
    if (!appSettings.behavior.pause_other_sessions || !media?.isPlaying) return;
    const currentTitle = media.title;
    if (prevMediaTitleRef.current !== null && prevMediaTitleRef.current !== currentTitle) {
      platformApi.pauseOtherSessions().catch(() => {});
    }
    prevMediaTitleRef.current = currentTitle;
  }, [media?.title, media?.isPlaying, appSettings.behavior.pause_other_sessions]);

  // Active-app awareness for Prism — Pilly-style "knows what you're working on",
  // done privately: active window title + exe name only, opt-in, persisted locally.
  // Default on; resolved on demand at send time (no background polling).
  // Whether a Groq key is configured for Prism (null = unknown / not in Tauri).
  const [prismHasKey, setPrismHasKey] = useState<boolean | null>(null);
  useEffect(() => {
    tauriInvoke<boolean>("has_prism_api_key", undefined, { silent: true })
      .then((has) => setPrismHasKey(has))
      .catch(() => setPrismHasKey(null));
  }, []);

  const [activeAppContext, setActiveAppContext] = useState<boolean>(() => {
    try {
      return localStorage.getItem("pillar_prism_active_app") !== "off";
    } catch {
      return true;
    }
  });
  const toggleActiveAppContext = useCallback((enabled: boolean) => {
    setActiveAppContext(enabled);
    try {
      localStorage.setItem("pillar_prism_active_app", enabled ? "on" : "off");
    } catch {
      /* ignore storage errors */
    }
  }, []);

  const {
    messages: prismMessages,
    actions: prismActions,
    actionMode: prismActionMode,
    usage: prismUsage,
    isLoading: prismLoading,
    error: prismError,
    setActionMode: setPrismActionMode,
    setActions: setPrismActions,
    clearChat: clearPrismChat,
    sendMessage: sendPrismMessage,
  } = usePrismAI({
    timer,
    media,
    volume,
    brightness,
    notifications,
    audioSessions,
    autoStartEnabled,
    battery,
    productivitySummary: {
      taskCount: productivity.tasks.length,
      completedTaskCount: productivity.tasks.filter((task) => task.completed).length,
      noteCount: productivity.notes.length,
      upcomingEventCount: productivity.calendarEvents.filter((event) => event.startsAt >= Date.now()).length,
    },
    includeActiveApp: activeAppContext,
    activeApp: null,
  });

  const savePrismApiKey = useCallback(async (key: string) => {
    try {
      await tauriInvoke("set_prism_api_key", { key });
      setPrismHasKey(true);
      clearPrismChat(); // drop the "no key" error
      return true;
    } catch {
      return false;
    }
  }, [clearPrismChat]);

  // Whether to show notification badge in the pill
  const hasNotificationBadge = appSettings.layout.idle_indicators.notifications && notifications.length > 0 &&
    (notificationPhase === "showing" || notificationPhase === "idle");
  const themeTokens = createPillThemeTokens({ ...appearance.active, accentColor: effectiveAccentColor });
  const reducedMotion = resolveReducedMotion(appSettings.motion.reduced_motion_override);
  const visibleTabs = EXPANDED_TAB_CONFIG.filter((tab) => appSettings.layout.visible_tabs[tab.id]);
  const safeTabs = visibleTabs.length > 0 ? visibleTabs : EXPANDED_TAB_CONFIG;

  const containerRef = useRef<HTMLDivElement>(null);
  const expandedContentRef = useRef<HTMLDivElement>(null);
  const pillToggleRef = useRef<HTMLDivElement>(null);
  const [bootPhase, setBootPhase] = useState<"dot" | "morph" | "complete">("dot");
  const [activeTab, setActiveTab] = useState<ExpandedTab>("timer");
  const [notificationsView, setNotificationsView] = useState<"live" | "history">("live");
  const [time, setTime] = useState(getTimeString);
  const [dateStr, setDateStr] = useState(getDateString);
  const [seconds, setSeconds] = useState(getSecondsString);
  const panelTransitionDuration = reducedMotion ? 0.08 : PILL_DURATION_FAST;

  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--pillar-surface-primary", themeTokens.surfacePrimary);
    root.style.setProperty("--pillar-surface-secondary", themeTokens.surfaceSecondary);
    root.style.setProperty("--pillar-border", themeTokens.borderColor);
    root.style.setProperty("--pillar-text", themeTokens.textPrimary);
    root.style.setProperty("--pillar-text-muted", themeTokens.textMuted);
    root.style.setProperty("--pillar-accent", themeTokens.accent);
    root.style.setProperty("--pillar-accent-contrast", contrastOn(themeTokens.accent));
    root.style.setProperty("--pillar-shadow", themeTokens.shadow);
  }, [themeTokens]);

  // Clock ticks whenever pill is shown (after boot) so time is always correct
  // Pause only when document is hidden (window minimized) to save CPU
  const shouldTickClock = !isBooting;

  useEffect(() => {
    if (!shouldTickClock) return;

    const tick = () => {
      // setTime/setDateStr are no-ops via React's same-value bailout when the
      // minute/day hasn't changed, so collapsed ticks don't re-render. Seconds,
      // however, change every tick AND are only shown in the expanded header — so
      // only update them while expanded. This stops a full per-second re-render of
      // the whole Pill while it sits collapsed (the common state).
      setTime(getTimeString());
      setDateStr(getDateString());
      if (isExpanded) setSeconds(getSecondsString());
    };

    // Initial sync
    tick();

    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [shouldTickClock, isExpanded]);

  // When app becomes visible again, sync time immediately
  useEffect(() => {
    if (!shouldTickClock) return;
    const onVisibilityChange = () => {
      if (!document.hidden) {
        setTime(getTimeString());
        setDateStr(getDateString());
        setSeconds(getSecondsString());
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => document.removeEventListener("visibilitychange", onVisibilityChange);
  }, [shouldTickClock]);

  // Determine what to show based on active content state
  const hasTimerActive = timer.isActive || timer.isComplete;
  const hasTimerAlert = timer.isComplete;
  const hasMediaPlaying = media?.isPlaying ?? false;
  const showTimerInIdle = hasTimerActive && !isExpanded;
  const showMediaInIdle = hasMediaPlaying && !isExpanded && appSettings.layout.idle_indicators.media;

  // Whether to show battery indicator in the idle pill
  const showBatteryInIdle = battery.hasBattery && appSettings.layout.idle_indicators.battery;

  // One options object drives the idle pill width, the slot widths and the window size.
  const idleSlotOptions = useMemo(
    () => ({
      hasMedia: showMediaInIdle,
      hasBattery: showBatteryInIdle,
      hasNotifications: hasNotificationBadge,
      hasTimer: hasTimerActive,
    }),
    [showMediaInIdle, showBatteryInIdle, hasNotificationBadge, hasTimerActive]
  );
  const idleSlots = getIdleSlotWidths(idleSlotOptions, isHovering);

  // Accessible name for the COLLAPSED pill. The visual idle content (clock, timer
  // countdown, "Done!", battery, notification count) is otherwise invisible to AT,
  // which would only hear a static "PILLAR Dynamic Island, button". Composed on
  // focus via aria-label (not a live region) so the clock doesn't spam SR every second.
  const collapsedAriaLabel = useMemo(() => {
    const parts = [`PILLAR. ${time}.`];
    if (timer.isComplete) parts.push("Timer finished.");
    else if (timer.isActive) parts.push(`Timer ${formatTime(timer.remainingSeconds)} remaining.`);
    if (notifications.length > 0) parts.push(`${notifications.length} notification${notifications.length === 1 ? "" : "s"}.`);
    if (battery.hasBattery) parts.push(`Battery ${battery.percent}%${battery.isCharging ? ", charging" : ""}.`);
    return parts.join(" ");
  }, [time, timer.isComplete, timer.isActive, timer.remainingSeconds, formatTime, notifications.length, battery.hasBattery, battery.percent, battery.isCharging]);

  useEffect(() => {
    if (!safeTabs.some((t) => t.id === activeTab)) {
      setActiveTab(safeTabs[0].id);
    }
  }, [activeTab, safeTabs]);

  const goToTab = useCallback((direction: -1 | 1) => {
    const ids = safeTabs.map((tab) => tab.id);
    const currentIndex = ids.indexOf(activeTab);
    const startIndex = currentIndex >= 0 ? currentIndex : 0;
    const nextIndex = direction === 1
      ? (startIndex + 1) % ids.length
      : (startIndex - 1 + ids.length) % ids.length;
    setActiveTab(ids[nextIndex]);
  }, [activeTab, safeTabs]);

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
      if (!isExpanded && (isHovering || isIdle)) {
        expand("long press");
      }
    },
  });

  // Snappy spring so hover-in and unhover-out feel the same. When reduced motion is
  // requested, use a near-instant spring so the pill resize snaps without a bouncy morph.
  const pillSpring = reducedMotion ? { stiffness: 1000, damping: 100, mass: 0.1 } : springConfig.island;
  const width = useSpring(pillDimensions.boot.width, pillSpring);
  const height = useSpring(pillDimensions.boot.height, pillSpring);
  const borderRadiusTop = useSpring(pillDimensions.boot.borderRadius, pillSpring);
  const borderRadiusBottom = useSpring(pillDimensions.boot.borderRadius, pillSpring);
  const shadowOpacity = useSpring(0.2, pillSpring);

  // Combined borderRadius: top-left top-right bottom-right bottom-left
  const borderRadius = useTransform(
    [borderRadiusTop, borderRadiusBottom],
    ([t, b]: number[]) => `${t}px ${t}px ${b}px ${b}px`
  );

  // Soft ambient shadow plus a hairline ring so the black island still reads
  // against dark wallpapers — no glass, no gradients, like the real thing.
  const boxShadow = useTransform(
    shadowOpacity,
    (v) =>
      `0 0 0 0.5px rgba(255, 255, 255, ${0.05 + v * 0.08}), 0 6px 20px rgba(0, 0, 0, ${v * 0.6}), 0 18px 50px rgba(0, 0, 0, ${v * 0.55})`
  );

  // Boot animation sequence
  useEffect(() => {
    if (!isBooting) return;

    // Reduced motion: skip the ~1.15s dot→morph→zoom flourish that gates all
    // interaction at launch. Snap straight to idle so the pill is usable immediately.
    if (reducedMotion) {
      width.set(pillDimensions.idle.width);
      height.set(pillDimensions.idle.height);
      borderRadiusBottom.set(pillDimensions.idle.borderRadius);
      borderRadiusTop.set(isNotch ? 0 : pillDimensions.idle.borderRadius);
      setBootPhase("complete");
      completeBootAnimation();
      return;
    }

    const sequence = async () => {
      // Phase 1: Dot appears
      await new Promise((r) => setTimeout(r, bootAnimationDuration.dotAppear));

      // Phase 2: Morph to pill (idle size)
      setBootPhase("morph");
      width.set(pillDimensions.idle.width);
      height.set(pillDimensions.idle.height);
      borderRadiusBottom.set(pillDimensions.idle.borderRadius);
      borderRadiusTop.set(isNotch ? 0 : pillDimensions.idle.borderRadius);

      await new Promise((r) => setTimeout(r, bootAnimationDuration.morphToPill));

      // Phase 3: Zoom in like hover, then back out
      width.set(pillDimensions.hover.width);
      height.set(pillDimensions.hover.height);
      borderRadiusBottom.set(pillDimensions.hover.borderRadius);
      borderRadiusTop.set(isNotch ? 0 : pillDimensions.hover.borderRadius);
      shadowOpacity.set(0.25);

      await new Promise((r) => setTimeout(r, 350));

      // Complete → settles back to idle size
      setBootPhase("complete");
      completeBootAnimation();
    };

    sequence();
  }, [isBooting, width, height, borderRadiusTop, borderRadiusBottom, isNotch, shadowOpacity, completeBootAnimation, reducedMotion]);

  // Single place for hover/expanded/unhover: one visual state → one target style
  const visualState: PillVisualState = isExpanded ? "expanded" : isHovering ? "hover" : "idle";

  // ---- Debug log: state transitions (change-only, never per-frame) ----
  useEffect(() => {
    dlog("info", "pill", `visualState -> ${visualState}${isBooting ? " (booting)" : ""}`);
  }, [visualState, isBooting]);

  useEffect(() => {
    dlog("info", "pill", `activeTab -> ${activeTab}`);
  }, [activeTab]);

  const mediaPresent = !!media;
  const mediaPlaying = media?.isPlaying ?? false;
  const mediaTitleShort = media?.title ? media.title.slice(0, 40) : "";
  useEffect(() => {
    dlog(
      "info",
      "pill",
      mediaPresent
        ? `media ${mediaPlaying ? "playing" : "paused"}: "${mediaTitleShort}"`
        : "media: none"
    );
  }, [mediaPresent, mediaPlaying, mediaTitleShort]);

  useEffect(() => {
    dlog("info", "pill", `notificationPhase -> ${notificationPhase}`);
  }, [notificationPhase]);

  // Update dimensions from current visual state (keeps animations smooth, logic simple)
  useEffect(() => {
    if (isBooting) return;
    const target = getPillTargetStyle(visualState, idleSlotOptions);
    width.set(target.width);
    height.set(target.height);
    borderRadiusBottom.set(target.borderRadius);
    borderRadiusTop.set(isNotch ? 0 : target.borderRadius);
    shadowOpacity.set(target.shadow);
  }, [isBooting, visualState, isNotch, idleSlotOptions, width, height, borderRadiusTop, borderRadiusBottom, shadowOpacity]);

  // Keep window always receiving clicks so the pill is clickable.
  // (Enabling click-through when idle would block mouseenter, so we'd never get hover/click.)
  useEffect(() => {
    const setClickThrough = async (ignore: boolean) => {
      // Unit-returning commands resolve to null on success; failures throw.
      try {
        await tauriInvoke("set_click_through", { ignore });
      } catch (error) {
        dlog("error", "pill", `set_click_through(${ignore}) failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    };
    setClickThrough(false);
  }, []);

  // Resize and re-center window when expanded or when notification toast is showing (so toast isn't clipped)
  const showNotificationToast = !isExpanded && (notificationPhase === "incoming" || notificationPhase === "absorbing") && !!latestNotification;
  // Collapsed window width follows the HOVER pill — the widest collapsed state for
  // the current indicators (matches getPillTargetStyle logic).
  const collapsedPillWidth = getPillTargetStyle("hover", idleSlotOptions).width;
  // Whether the window is currently sized for the expanded island, and when the
  // pending expanded→collapsed shrink started (survives effect re-runs).
  const windowSizedForExpandedRef = useRef(false);
  const shrinkStartedAtRef = useRef<number | null>(null);
  useEffect(() => {
    const pillWidth = isExpanded ? pillDimensions.expanded.width : collapsedPillWidth;
    // Collapsed: size the window for the HOVER pill (+4px slack). Sizing it to the
    // 36px idle pill clipped the 40px hover pill, so the cursor "left" the window
    // while still on the island and hover flickered on and off.
    const pillHeight = isExpanded ? pillDimensions.expanded.height : pillDimensions.hover.height + 4;
    // Toast is 300-380px wide, so the window must be wide enough to contain it.
    // Otherwise keep only a thin margin around the island: wider margins were
    // invisible window area that swallowed clicks meant for apps underneath
    // (e.g. browser tabs at the top of the screen).
    const marginX = isExpanded ? 16 : 12;
    const w = showNotificationToast
      ? Math.max(pillWidth + marginX, 420)
      : pillWidth + marginX;
    // Toast needs: 10px gap + ~120px toast height + 20px breathing room = ~150px below pill
    const extraHeight = isExpanded ? 10 : (showNotificationToast ? 160 : 0);
    const h = pillHeight + extraHeight;

    const resizeAndCenter = async () => {
      windowSizedForExpandedRef.current = isExpanded;
      shrinkStartedAtRef.current = null;
      const startedAt = performance.now();
      try {
        // resize_and_center returns Result<(), _>, so success resolves to null.
        await tauriInvoke("resize_and_center", { width: w, height: h });
        const ms = Math.round(performance.now() - startedAt);
        dlog(ms > 300 ? "warn" : "info", "pill", `resize_and_center ${w}x${h} (expanded=${isExpanded} toast=${showNotificationToast}) ok in ${ms}ms`);
      } catch (error) {
        const ms = Math.round(performance.now() - startedAt);
        dlog(
          "error",
          "pill",
          `resize_and_center ${w}x${h} FAILED after ${ms}ms: ${error instanceof Error ? error.message : String(error)}`
        );
      }
    };

    // Growing (expand, toast appearing, indicator changes) is immediate. Only the
    // expanded→collapsed shrink waits — shrinking the window ~20ms after the state
    // change clipped the island's closing spring mid-animation.
    if (isExpanded || !windowSizedForExpandedRef.current || reducedMotion) {
      shrinkStartedAtRef.current = null;
      void resizeAndCenter();
      return;
    }

    // Keep the deadline from the first run if this effect re-runs mid-wait.
    if (shrinkStartedAtRef.current === null) shrinkStartedAtRef.current = performance.now();
    const shrinkStartedAt = shrinkStartedAtRef.current;
    let settled = false;
    const unsubscribers: Array<() => void> = [];
    let maxTimer: ReturnType<typeof setTimeout> | null = null;
    const stopWaiting = () => {
      settled = true;
      unsubscribers.forEach((unsub) => unsub());
      if (maxTimer) clearTimeout(maxTimer);
    };
    const shrinkNow = (why: string) => {
      if (settled) return;
      stopWaiting();
      dlog("debug", "pill", `window shrink deferred ${Math.round(performance.now() - shrinkStartedAt)}ms (${why})`);
      void resizeAndCenter();
    };
    // Safe to shrink once the (still animating) island fits inside the collapsed window.
    const checkFits = () => {
      if (width.get() <= w - 4 && height.get() <= h - 2) shrinkNow("island settled");
    };
    unsubscribers.push(width.on("change", checkFits), height.on("change", checkFits));
    maxTimer = setTimeout(
      () => shrinkNow("max delay"),
      Math.max(0, SHRINK_MAX_DELAY_MS - (performance.now() - shrinkStartedAt))
    );
    checkFits();

    // Re-expanding (or any target change) cancels the pending shrink; the next run decides again.
    return () => {
      if (!settled) stopWaiting();
    };
  }, [isExpanded, showNotificationToast, collapsedPillWidth, reducedMotion, width, height]);

  // ---- Pointer tracking: native (global cursor + outside presses) or DOM fallback ----
  // The window is small and transparent, so DOM enter/leave/click only see the cursor
  // over OUR window. The backend tracks the global cursor against the hit region
  // pushed here; when that's unavailable we fall back to DOM events + window blur.

  // Viewport in state so the hit rects follow window resizes (rects are viewport-relative).
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

  // Toast layout size (its enter/absorb transforms are ignored — approximate is fine).
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

  // Hit region from TARGET geometry, not the animated springs, so it reflects where
  // the island is going. (During boot visualState is "idle", so boot uses idle dims.)
  const hitTarget = getPillTargetStyle(visualState, idleSlotOptions);
  const hitRects = useMemo<HitRect[]>(() => {
    if (suspended) return [];
    // The context menu overlay covers the whole window — clicks on it aren't "outside".
    if (contextMenu.isOpen) return [{ x: 0, y: 0, w: viewport.width, h: viewport.height }];
    // Pill sits at the top-center of the viewport; pad the sides and bottom (top is the screen edge).
    const rects: HitRect[] = [
      {
        x: (viewport.width - hitTarget.width) / 2 - HIT_PADDING,
        y: 0,
        w: hitTarget.width + HIT_PADDING * 2,
        h: hitTarget.height + HIT_PADDING,
      },
    ];
    if (showNotificationToast && toastSize) {
      rects.push({
        x: (viewport.width - toastSize.w) / 2 - HIT_PADDING,
        y: hitTarget.height + TOAST_GAP - HIT_PADDING,
        w: toastSize.w + HIT_PADDING * 2,
        h: toastSize.h + HIT_PADDING * 2,
      });
    }
    return rects;
  }, [suspended, contextMenu.isOpen, viewport.width, viewport.height, hitTarget.width, hitTarget.height, showNotificationToast, toastSize]);

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
      const where = `${press.button} button at ${Math.round(press.x)},${Math.round(press.y)}`;
      // Don't collapse (and unmount the module, losing the draft) while the user
      // is mid-edit with unsaved text in a panel field.
      if (panelHasUnsavedDraft(containerRef.current)) {
        dlog("info", "pill", `outside press (${where}) ignored: panel has unsaved draft`);
        return;
      }
      collapse(`outside press (${where})`);
    },
    [contextMenu.isOpen, closeContextMenu, isExpanded, collapse]
  );

  const { nativeActive } = useNativePointer({
    rects: hitRects,
    armed: isExpanded && !suspended,
    onPointerChange: handleNativePointerChange,
    onOutsidePress: handleOutsidePress,
  });

  // DOM fallback: hover/leave come from the root's onMouseEnter/onMouseLeave (see
  // below; ignored while native tracking is active so the two sources can't fight).
  // Clicks on other apps are invisible to the page, so also close on window blur.
  useEffect(() => {
    if (nativeActive || !isExpanded) return;
    const expandedAt = performance.now();
    const onBlur = () => {
      if (performance.now() - expandedAt < BLUR_GRACE_MS) {
        dlog("debug", "pill", "window blur ignored: island just expanded");
        return;
      }
      if (panelHasUnsavedDraft(containerRef.current)) {
        dlog("info", "pill", "window blur ignored: panel has unsaved draft");
        return;
      }
      collapse("window lost focus (DOM fallback)");
    };
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  }, [nativeActive, isExpanded, collapse]);

  // Focus trap for expanded state
  useEffect(() => {
    if (!isExpanded || !expandedContentRef.current) return;

    const cleanup = createFocusTrap({
      container: expandedContentRef.current,
      restoreFocus: pillToggleRef.current,
      initialFocus: true,
    });

    return cleanup;
  }, [isExpanded]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Escape: close expanded view — but if the user is mid-edit with unsaved
      // text, first blur the field (which would lose nothing) instead of collapsing
      // and discarding their draft. A second Escape then closes.
      if (e.key === "Escape" && isExpanded) {
        if (panelHasUnsavedDraft(containerRef.current)) {
          (document.activeElement as HTMLElement | null)?.blur();
        } else {
          collapse("escape key");
        }
        return;
      }

      // Ctrl+Shift+Space: toggle expand/collapse (expand works from idle or hover).
      if (e.key === " " && e.ctrlKey && e.shiftKey) {
        e.preventDefault();
        if (isExpanded) {
          collapse("ctrl+shift+space shortcut");
        } else {
          expand("ctrl+shift+space shortcut");
        }
        triggerActivity(); // Mark user as active
        return;
      }

      // Tab-strip navigation keys must NOT hijack caret movement inside text fields
      // (Prism chat, productivity inputs). If focus is in an editable element, let
      // the browser handle Arrow/Home/End natively.
      const target = e.target as HTMLElement | null;
      const isEditingText = !!target?.closest("input, textarea, [contenteditable]");

      // Arrow keys: navigate the tab strip (only when expanded, not while editing text)
      if (isExpanded && !isEditingText && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        e.preventDefault();
        const tabs = safeTabs.map((tab) => tab.id);
        const currentIndex = tabs.indexOf(activeTab);
        const nextIndex =
          e.key === "ArrowLeft"
            ? (currentIndex > 0 ? currentIndex - 1 : tabs.length - 1)
            : (currentIndex < tabs.length - 1 ? currentIndex + 1 : 0);
        setActiveTab(tabs[nextIndex]);
        triggerActivity();
        return;
      }

      // Home/End: jump to first/last tab (only when expanded, not while editing text)
      if (isExpanded && !isEditingText && (e.key === "Home" || e.key === "End")) {
        e.preventDefault();
        const tabs = safeTabs.map((tab) => tab.id);
        setActiveTab(e.key === "Home" ? tabs[0] : tabs[tabs.length - 1]);
        triggerActivity();
        return;
      }

      // NOTE: Tab / Shift+Tab is intentionally NOT handled here. It must perform
      // normal DOM focus traversal so keyboard users can reach the controls inside
      // the active panel (Start/Pause/Stop, sliders, Prism input, productivity
      // fields). The tab STRIP is reachable via Arrow + Home/End above plus its
      // roving tabindex, per the WAI-ARIA tabs pattern. A previous Tab branch here
      // preventDefault'd every Tab and trapped focus on the tab strip.
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isExpanded, activeTab, collapse, expand, safeTabs, triggerActivity]);

  // Screen reader announcements for key state changes
  const prevTimerStateRef = useRef(timer);
  const prevMediaStateRef = useRef(media);
  const prevNotificationsRef = useRef(notifications);
  const prevActiveTabRef = useRef(activeTab);
  const prevVolumeRef = useRef(volume);

  // Announce timer state changes
  useEffect(() => {
    const prev = prevTimerStateRef.current;
    if (prev.isActive !== timer.isActive) {
      if (timer.isActive) {
        announce(`Timer started: ${timer.label}`);
      } else if (timer.isComplete) {
        announce(`Timer completed: ${timer.label}`, "assertive");
      } else if (prev.isActive && !timer.isActive && !timer.isPaused) {
        announce("Timer stopped");
      }
    }
    if (prev.isPaused !== timer.isPaused && timer.isPaused) {
      announce("Timer paused");
    }
    if (prev.isPaused !== timer.isPaused && !timer.isPaused && timer.isActive) {
      announce("Timer resumed");
    }
    prevTimerStateRef.current = timer;
  }, [timer, announce]);

  // Announce media state changes
  useEffect(() => {
    const prev = prevMediaStateRef.current;
    if (prev?.title !== media?.title && media?.title) {
      announce(`Now playing: ${media.title} by ${media.artist || "Unknown Artist"}`);
    }
    if (prev?.isPlaying !== media?.isPlaying) {
      if (media?.isPlaying) {
        announce("Media playing");
      } else {
        announce("Media paused");
      }
    }
    prevMediaStateRef.current = media;
  }, [media, announce]);

  // Announce notification changes
  useEffect(() => {
    const prev = prevNotificationsRef.current;
    announceListChange(notifications, prev, "notification", "notifications", "notifications");
    prevNotificationsRef.current = notifications;
  }, [notifications, announceListChange]);

  // Announce tab changes
  useEffect(() => {
    const prev = prevActiveTabRef.current;
    if (prev !== activeTab) {
      const tabNames: Record<ExpandedTab, string> = {
        timer: "Timer",
        media: "Media",
        notifications: "Notifications",
        settings: "Settings",
        productivity: "Productivity",
        prism: "Prism AI",
      };
      announce(`Switched to ${tabNames[activeTab]} tab`);
    }
    prevActiveTabRef.current = activeTab;
  }, [activeTab, announce]);

  // Announce volume changes
  useEffect(() => {
    const prev = prevVolumeRef.current;
    if (prev?.level !== volume?.level && volume?.level !== undefined) {
      announce(`Volume: ${Math.round(volume.level * 100)}%`);
    }
    if (prev?.isMuted !== volume?.isMuted) {
      announce(volume?.isMuted ? "Muted" : "Unmuted");
    }
    prevVolumeRef.current = volume;
  }, [volume, announce]);

  const runPrismAction = useCallback(
    async (action: PrismAction): Promise<string> => {
      const args = action.args ?? {};

      switch (action.type) {
        case "start_timer": {
          const minutesValue = Number(args.minutes);
          if (!Number.isFinite(minutesValue) || minutesValue <= 0) {
            throw new Error("Invalid minutes for start_timer.");
          }
          const minutes = Math.max(1, Math.min(720, Math.round(minutesValue)));
          const label =
            typeof args.label === "string" && args.label.trim()
              ? args.label.trim().slice(0, 40)
              : `Prism ${minutes}m`;
          startTimerWithNotification({ label, minutes });
          return `Timer started: ${label} (${minutes}m).`;
        }
        case "pause_timer":
          pauseTimer();
          return "Timer paused.";
        case "resume_timer":
          resumeTimer();
          return "Timer resumed.";
        case "stop_timer":
          stopTimer();
          return "Timer stopped.";
        case "set_volume": {
          const levelValue = Number(args.level);
          if (!Number.isFinite(levelValue)) {
            throw new Error("Invalid level for set_volume.");
          }
          const level = Math.max(0, Math.min(100, Math.round(levelValue)));
          await setVolume(level);
          return `Volume set to ${level}%.`;
        }
        case "toggle_mute":
          await toggleMute();
          return "Mute toggled.";
        case "set_brightness": {
          const levelValue = Number(args.level);
          if (!Number.isFinite(levelValue)) {
            throw new Error("Invalid level for set_brightness.");
          }
          const level = Math.max(0, Math.min(100, Math.round(levelValue)));
          await setBrightness(level);
          return `Brightness set to ${level}%.`;
        }
        case "media_play_pause":
          await playPause();
          return "Media play/pause sent.";
        case "media_next":
          await mediaNext();
          return "Media next sent.";
        case "media_previous":
          await mediaPrevious();
          return "Media previous sent.";
        case "add_task": {
          const title =
            typeof args.title === "string" && args.title.trim()
              ? args.title.trim().slice(0, 120)
              : "";
          if (!title) {
            throw new Error("Invalid title for add_task.");
          }
          addTask(title);
          return `Task added: ${title}`;
        }
        case "add_note": {
          const title =
            typeof args.title === "string" && args.title.trim()
              ? args.title.trim().slice(0, 120)
              : "Quick note";
          const content = typeof args.content === "string" ? args.content.slice(0, 2000) : "";
          addNote(title, content);
          return `Note added: ${title}`;
        }
        default:
          throw new Error(`Unsupported action type: ${action.type}`);
      }
    },
    [
      mediaNext,
      mediaPrevious,
      addNote,
      addTask,
      pauseTimer,
      playPause,
      resumeTimer,
      setBrightness,
      setVolume,
      startTimer,
      stopTimer,
      toggleMute,
    ]
  );

  // When Actions mode is on and Prism returns actions, run them directly (no extra buttons)
  useEffect(() => {
    if (!prismActionMode || prismActions.length === 0) return;
    const toRun = [...prismActions];
    setPrismActions([]);
    toRun.forEach((action) => {
      runPrismAction(action).catch(() => {});
    });
  }, [prismActionMode, prismActions, setPrismActions, runPrismAction]);

  const executeWorkflowAction = useCallback((action: WorkflowActionEnvelope) => {
    const tabActionMap: Record<string, ExpandedTab> = {
      open_timer_tab: "timer",
      open_media_tab: "media",
      open_notifications_tab: "notifications",
      open_settings_tab: "settings",
      open_prism_tab: "prism",
      open_productivity_tab: "productivity",
    };

    // Tray / global-shortcut / workflow callers don't pass through hover; expand()
    // works from idle too. If the cursor isn't over the island, it stays open until
    // an outside press / Escape / toggle (no leave timer).
    if (action.id === "toggle_expand") {
      if (isExpanded) {
        collapse(`workflow action toggle_expand (${action.source})`);
      } else {
        expand(`workflow action toggle_expand (${action.source})`);
      }
      return;
    }

    if (action.id === "quick_add_task") {
      const rawTitle = typeof action.args?.title === "string" ? action.args.title : "Quick task";
      const title = rawTitle.trim().slice(0, 120);
      if (title) addTask(title);
      expand(`workflow action quick_add_task (${action.source})`);
      setActiveTab("productivity");
      return;
    }

    const targetTab = tabActionMap[action.id];
    if (!targetTab) return;
    expand(`workflow action ${action.id} (${action.source})`);
    setActiveTab(targetTab);
  }, [addTask, expand, collapse, isExpanded]);

  useWorkflowEvents(executeWorkflowAction);

  const triggerWorkflowAction = useCallback(async (id: WorkflowActionEnvelope["id"], args?: Record<string, unknown>) => {
    const envelope: WorkflowActionEnvelope = {
      id,
      args,
      source: "ui",
      timestamp: Date.now(),
    };
    try {
      const dispatched = await platformApi.dispatchWorkflowAction(id, args);
      if (dispatched === null) {
        executeWorkflowAction(envelope);
      }
    } catch {
      executeWorkflowAction(envelope);
    }
  }, [executeWorkflowAction]);

  // Handle click outside — only sees clicks inside OUR window (its transparent margin).
  // Kept in both modes: native outside presses cover the rest of the screen, and
  // collapse() is idempotent if both fire.
  useEffect(() => {
    if (!isExpanded) return;

    const handleGlobalClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        // Don't collapse (and unmount the module, losing the draft) while the user
        // is mid-edit with unsaved text in a panel field.
        if (panelHasUnsavedDraft(containerRef.current)) {
          dlog("debug", "pill", "outside click ignored: panel has unsaved draft");
          return;
        }
        const t = e.target instanceof Element ? e.target : null;
        collapseIsland(
          `outside click at ${Math.round(e.clientX)},${Math.round(e.clientY)} on <${t?.tagName.toLowerCase() ?? "?"}${t?.id ? `#${t.id}` : ""}>`
        );
      }
    };

    // Add slight delay to prevent immediate close
    const timeoutId = setTimeout(() => {
      document.addEventListener("click", handleGlobalClick);
    }, 100);

    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener("click", handleGlobalClick);
    };
  }, [isExpanded, collapseIsland]);

  // Direction of the last tab change so panels slide the way the dock moved.
  const activeTabIndex = safeTabs.findIndex((t) => t.id === activeTab);
  // Computed only when the tab actually changes, so the clock/media re-renders that
  // happen during the exit animation can't flip the direction mid-transition.
  const tabDirRef = useRef({ index: activeTabIndex, dir: 1 });
  if (tabDirRef.current.index !== activeTabIndex) {
    tabDirRef.current = { index: activeTabIndex, dir: activeTabIndex > tabDirRef.current.index ? 1 : -1 };
  }
  const tabDirection = tabDirRef.current.dir;
  const activeTabConfig = safeTabs.find((t) => t.id === activeTab) ?? safeTabs[0];

  const panelVariants = {
    enter: (dir: number) => (reducedMotion ? { opacity: 0 } : { opacity: 0, x: dir * 18, filter: "blur(4px)" }),
    center: { opacity: 1, x: 0, filter: "blur(0px)" },
  };
  const panelTransition = reducedMotion
    ? { duration: 0.08 }
    : { x: { type: "spring" as const, stiffness: 520, damping: 40 }, opacity: { duration: panelTransitionDuration }, filter: { duration: panelTransitionDuration } };

  const renderPanel = (tab: ExpandedTab) => {
    switch (tab) {
      case "timer":
        return (
          <TimerExpanded
            timer={timer}
            stats={timerStats}
            categories={timerCategories}
            selectedCategory={selectedTimerCategory}
            onSelectCategory={setSelectedTimerCategory}
            presets={presets}
            formatTime={formatTime}
            progress={timerProgress}
            onStart={startTimerWithNotification}
            onPause={pauseTimer}
            onResume={resumeTimer}
            onStop={stopTimer}
            onDismiss={handleDismissAlert}
          />
        );
      case "media":
        return (
          <MediaExpanded
            media={media}
            artUrl={albumArt.url}
            recentSources={recentSources}
            timeline={timeline}
            playbackInfo={playbackInfo}
            accentColor={effectiveAccentColor}
            onPlayPause={playPause}
            onNext={mediaNext}
            onPrevious={mediaPrevious}
            onToggleRepeat={toggleRepeat}
            onToggleShuffle={toggleShuffle}
            onPauseOthers={pauseOtherSessions}
            onSeek={seekTo}
            volume={volume}
            onVolumeChange={setVolume}
            onMuteToggle={toggleMute}
          />
        );
      case "notifications": {
        const listCount = notificationsView === "live" ? notifications.length : notificationHistory.length;
        return (
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between gap-2">
              <Segmented
                options={[
                  { id: "live", label: "Live", ariaLabel: "Show live notifications" },
                  { id: "history", label: "History", ariaLabel: "Show notification history" },
                ] as const}
                value={notificationsView}
                onChange={setNotificationsView}
                ariaLabel="Notification view"
              />
              {listCount > 0 && (
                <button
                  type="button"
                  className="h-[26px] px-3 rounded-full text-[11px] font-semibold bg-white/[0.08] text-white/60 hover:text-white hover:bg-white/[0.14] transition-colors"
                  onClick={() => {
                    if (notificationsView === "live") {
                      notifications.slice(0, 30).forEach((n) => dismissNotification(n.id));
                    } else {
                      clearNotificationHistory();
                    }
                  }}
                >
                  {notificationsView === "live" ? "Clear all" : "Clear history"}
                </button>
              )}
            </div>
            <NotificationsList
              notifications={notificationsView === "live" ? notifications : notificationHistory}
              hasAccess={hasNotificationAccess}
              onDismiss={dismissNotification}
              onActivate={async (id) => {
                try {
                  const caps = await platformApi.getCapabilities();
                  if (!caps.notifications) return;
                  const live = notifications.find(n => n.id === id);
                  // History items are usually gone from Action Center already, so
                  // activate_notification(id) fails with "not found" — open the app instead.
                  const notif = live ?? notificationHistory.find(n => n.id === id);
                  if (notif?.aumid) {
                    await platformApi.activateAppByAumid(notif.aumid);
                  } else if (live) {
                    await platformApi.activateNotification(id);
                  } else {
                    dlog("info", "pill", `history notification ${id} has no app id; nothing to open`);
                    return;
                  }
                  if (live) dismissNotification(id);
                  collapseIsland("notification activated");
                } catch (_) { /* ignore */ }
              }}
            />
          </div>
        );
      }
      case "settings":
        return (
          <QuickSettings
            header={
              // Live CPU/RAM monitor — only polls while this tab is open.
              <SystemMonitor enabled={activeTab === "settings"} accentColor={effectiveAccentColor} />
            }
            volume={volume}
            onVolumeChange={setVolume}
            onMuteToggle={toggleMute}
            brightness={brightness}
            onBrightnessChange={setBrightness}
            audioDevices={audioDevices}
            defaultAudioDevice={defaultAudioDevice}
            audioSessions={audioSessions}
            onSessionVolumeChange={setSessionVolume}
            onSessionMuteToggle={setSessionMute}
            autoStartEnabled={autoStartEnabled}
            onAutoStartToggle={() => fireAndForget(setAutoStartEnabled(!autoStartEnabled), "autostart toggle")}
            layoutSettings={appSettings.layout}
            onLayoutChange={(layoutPatch) => fireAndForget(updateSettings({ layout: layoutPatch }), "layout change")}
            appearance={appearance}
            motionSettings={{
              animationSpeed: appSettings.motion.animation_speed,
              onAnimationSpeedChange: (speed) =>
                fireAndForget(updateSettings({ motion: { animation_speed: speed } }), "animation speed change"),
            }}
          />
        );
      case "prism":
        return (
          <PrismModule
            messages={prismMessages}
            actionMode={prismActionMode}
            usage={prismUsage}
            isLoading={prismLoading}
            error={prismError}
            onSendMessage={sendPrismMessage}
            onToggleActionMode={setPrismActionMode}
            onClearChat={clearPrismChat}
            activeAppContext={activeAppContext}
            onToggleActiveAppContext={toggleActiveAppContext}
            needsApiKey={prismHasKey === false}
            onSaveApiKey={savePrismApiKey}
          />
        );
      case "productivity":
        return (
          <ProductivityModule
            state={productivity}
            onAddTask={addTask}
            onToggleTask={toggleTask}
            onRemoveTask={removeTask}
            onClearCompleted={clearCompletedTasks}
            onAddNote={addNote}
            onUpdateNote={updateNote}
            onRemoveNote={removeNote}
            onAddEvent={addAgendaEvent}
            onExportBackup={async () => {
              const result = await exportBackup();
              announce(result?.valid ? "Productivity backup exported." : `Backup export failed.${describeBackupConflicts(result)}`);
            }}
            onPreviewImport={async () => {
              const result = await importBackup("preview");
              announce(result?.valid ? "Backup preview valid, ready to apply." : `Backup preview has conflicts.${describeBackupConflicts(result)}`);
            }}
            onApplyImport={async () => {
              const result = await importBackup("apply");
              announce(result?.valid ? "Backup applied." : `Backup not applied.${describeBackupConflicts(result)}`);
            }}
          />
        );
    }
  };

  // Prism and Media manage their own height; the rest scroll inside the panel.
  const panelScrolls = activeTab !== "prism" && activeTab !== "media" && !(activeTab === "timer" && (timer.isActive || timer.isComplete));

  return (
    <>
      {/* Live regions render here (Pill's own useScreenReader instance) so every
          announce() call below is actually spoken. Kept outside the pill body so
          they stay mounted whether collapsed or expanded. */}
      <ScreenReaderLiveRegions polite={politeAnnouncement} assertive={assertiveAnnouncement} />
    <motion.div
      ref={containerRef}
      className="relative cursor-pointer"
      role={isExpanded ? "dialog" : "button"}
      aria-label={isExpanded ? "PILLAR Dynamic Island - Expanded" : collapsedAriaLabel}
      aria-modal={isExpanded ? "true" : undefined}
      aria-expanded={isExpanded ? "true" : "false"}
      tabIndex={isExpanded ? -1 : 0}
      data-expanded={isExpanded ? "true" : "false"}
      style={{
        width,
        height,
        borderRadius,
        boxShadow,
        overflow: "visible",
        background: isBooting && bootPhase === "dot"
          ? "radial-gradient(circle, rgba(255,255,255,0.85) 0%, rgba(200,200,200,0.6) 100%)"
          : "var(--pillar-surface-primary)",
      }}
      initial={{ opacity: 0, scale: 0 }}
      animate={{
        opacity: 1,
        scale: 1,
      }}
      transition={reducedMotion ? { duration: 0.1, ease: "easeOut" } : springConfig.bouncy}
      whileTap={!isExpanded && !reducedMotion ? { scale: 0.97 } : undefined}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onPointerDown={gestureHandlers.onPointerDown}
      onPointerMove={gestureHandlers.onPointerMove}
      onPointerUp={gestureHandlers.onPointerUp}
      onContextMenu={gestureHandlers.onContextMenu}
      onClick={() => {
        handleClick();
        triggerActivity(); // Mark user as active
      }}
      onKeyDown={(e) => {
        if (!isExpanded && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          // Force-expand: handleClick only expands from the mouse-set "hover" state,
          // so a keyboard user who Tab-focuses the idle pill could not open it.
          expandPill();
          triggerActivity(); // Mark user as active
        }
      }}
    >
      {/* Timer progress ring around pill (when timer active in idle/hover) */}
      {showTimerInIdle && (
        <TimerMiniProgress
          progress={timerProgress}
          width={width}
          height={height}
        />
      )}

      {/* Background state indicators */}
      {backgroundStates.length > 0 && !isExpanded && (
        <StateIndicators states={backgroundStates} position="right" />
      )}

      {/* Notification toast (appears BELOW pill, then animates into badge).
          Body click → activate via NotificationToast's internal onActivate;
          X button → dismiss only. Do NOT add an onClick(Capture) on this wrapper:
          a capture-phase handler swallows the X button's stopPropagation and
          ends up activating the app every time the user tries to dismiss. */}
      {!isExpanded && (notificationPhase === "incoming" || notificationPhase === "absorbing") && latestNotification && (
        <div
          className="absolute left-1/2 -translate-x-1/2 z-[100]"
          style={{ top: "calc(100% + 8px)" }}
        >
          <NotificationToast
            notification={latestNotification}
            onDismiss={clearLatestNotification}
            onActivate={async (id) => {
              try {
                const caps = await platformApi.getCapabilities();
                if (!caps.notifications) return;
                const notif = notifications.find(n => n.id === id);
                if (notif?.aumid) {
                  await platformApi.activateAppByAumid(notif.aumid);
                } else {
                  await platformApi.activateNotification(id);
                }
              } catch {
                // Best-effort activation; ignore failures so the toast still dismisses cleanly.
              }
            }}
            phase={notificationPhase}
          />
        </div>
      )}

      {/* Idle / hover: the compact island */}
      <AnimatePresence>
        {!isBooting && !isExpanded && (
          <motion.div
            key="compact"
            ref={pillToggleRef}
            className="absolute inset-0 flex items-center pointer-events-none select-none px-[14px] text-white"
            style={{
              fontVariantNumeric: "tabular-nums",
              fontSize: "15px",
              fontWeight: 600,
              letterSpacing: "-0.01em",
            }}
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.9, filter: "blur(4px)" }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
            exit={{ opacity: 0, transition: { duration: 0.06 } }}
            transition={{ duration: 0.22, delay: 0.06 }}
          >
            {/* Leading: album art */}
            <motion.div
              className="flex items-center flex-shrink-0 min-w-0 overflow-hidden"
              animate={{ width: idleSlots.left }}
              initial={false}
              transition={idleSlotAnimations.transition}
            >
              <AnimatePresence mode="wait">
                {showMediaInIdle && (
                  <motion.div
                    key="media"
                    className="flex items-center flex-shrink-0"
                    initial={idleSlotAnimations.left.initial}
                    animate={idleSlotAnimations.left.animate}
                    exit={idleSlotAnimations.left.exit}
                    transition={idleSlotAnimations.transition}
                  >
                    <AlbumArt url={albumArt.url} size={22} radius={6} />
                  </motion.div>
                )}
              </AnimatePresence>
            </motion.div>

            {/* Center: clock or live timer */}
            <motion.div
              layout
              className="flex items-center justify-center flex-1 min-w-0 overflow-hidden"
              transition={idleSlotAnimations.transition}
            >
              <AnimatePresence mode="wait" initial={false}>
                {hasTimerActive && !hasTimerAlert && (
                  <motion.span
                    key="timer"
                    className="inline-flex items-center gap-1.5"
                    style={{ color: timer.isPaused ? "rgba(255,255,255,0.55)" : TIMER_TINT }}
                    initial={idleSlotAnimations.center.initial}
                    animate={idleSlotAnimations.center.animate}
                    exit={idleSlotAnimations.center.exit}
                    transition={idleSlotAnimations.transition}
                  >
                    <TimerIcon size={15} strokeWidth={2.4} />
                    {formatTime(timer.remainingSeconds)}
                  </motion.span>
                )}
                {hasTimerAlert && (
                  <motion.span
                    key="alert"
                    className="inline-flex items-center gap-1.5"
                    style={{ color: TIMER_TINT }}
                    initial={idleSlotAnimations.center.initial}
                    animate={{
                      ...idleSlotAnimations.center.animate,
                      // Steady opacity under reduced motion — no perpetual blink.
                      opacity: reducedMotion ? 1 : [1, 0.55, 1],
                    }}
                    exit={idleSlotAnimations.center.exit}
                    transition={{
                      ...idleSlotAnimations.transition,
                      ...(reducedMotion ? {} : { opacity: { duration: 1.2, repeat: Infinity } }),
                    }}
                  >
                    <BellIcon size={14} strokeWidth={2.4} />
                    Done
                  </motion.span>
                )}
                {!hasTimerActive && !hasTimerAlert && (
                  <motion.span
                    key="time"
                    initial={idleSlotAnimations.center.initial}
                    animate={idleSlotAnimations.center.animate}
                    exit={idleSlotAnimations.center.exit}
                    transition={idleSlotAnimations.transition}
                  >
                    {time}
                  </motion.span>
                )}
              </AnimatePresence>
            </motion.div>

            {/* Trailing: waveform, battery, notifications */}
            <motion.div
              className="flex items-center justify-end flex-shrink-0 self-stretch gap-1.5"
              // Clip only horizontally (for the width animation) so the notification
              // count bubble can overhang vertically without being cut off.
              style={{ overflowX: "clip", overflowY: "visible" }}
              animate={{ width: idleSlots.right }}
              initial={false}
              transition={idleSlotAnimations.transition}
            >
              <AnimatePresence>
                {showMediaInIdle && (
                  <motion.div
                    key="wave"
                    className="flex items-center flex-shrink-0"
                    initial={idleSlotAnimations.right.initial}
                    animate={idleSlotAnimations.right.animate}
                    exit={idleSlotAnimations.right.exit}
                    transition={idleSlotAnimations.transition}
                  >
                    <MediaIndicator isPlaying color={effectiveAccentColor} bars={4} height={13} />
                  </motion.div>
                )}
                {showBatteryInIdle && (
                  <BatteryIndicator
                    key="battery"
                    battery={battery}
                    isLow={isBatteryLow}
                    isCritical={isBatteryCritical}
                    showPercent={isHovering}
                  />
                )}
                {hasNotificationBadge && notifications.length > 0 && (
                  <NotificationIndicator
                    key="notification-badge"
                    count={notifications.length}
                    appName={notifications[0]?.appName || "App"}
                    isNew={isNewNotification}
                    layoutId={undefined}
                  />
                )}
              </AnimatePresence>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Expanded island */}
      {/* No AnimatePresence here on purpose: an exit animation that never settled
          left this layer mounted (invisible) on top of the collapsed pill, where its
          stopPropagation swallowed every click — the island could not be reopened. */}
        {isExpanded && (
          <motion.div
            ref={expandedContentRef}
            className="island-expanded absolute inset-0 flex flex-col pt-4 pb-2 px-4 cursor-default text-white"
            style={{ borderRadius: "inherit" }}
            role="region"
            aria-label="PILLAR expanded content"
            onClick={(e) => e.stopPropagation()}
            onPointerDown={(e) => e.stopPropagation()}
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.94, filter: "blur(8px)" }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
            transition={reducedMotion ? { duration: 0.08 } : { duration: 0.26, delay: 0.05, ease: [0.2, 0.8, 0.2, 1] }}
          >
            {/* Album-color glow, clipped to the island's own shape so it never shows a hard edge */}
            {activeTab === "media" && media && (
                <motion.div
                  key="media-glow"
                  className="absolute inset-0 pointer-events-none"
                  style={{
                    borderRadius: "inherit",
                    background: `radial-gradient(70% 55% at 18% 30%, color-mix(in srgb, ${effectiveAccentColor} 30%, transparent), transparent 75%)`,
                  }}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.4 }}
                />
              )}

            {/* Header */}
            <div className="relative flex items-end justify-between mb-3 px-1 flex-shrink-0">
              <motion.h2
                key={activeTabConfig.id}
                className="text-white text-[20px] font-bold tracking-tight leading-none"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ type: "spring", stiffness: 500, damping: 36 }}
              >
                {activeTabConfig.title}
              </motion.h2>
              <div className="flex items-baseline gap-1.5 tabular-nums leading-none" style={{ fontVariantNumeric: "tabular-nums" }}>
                <span className="text-white/40 text-[12px] font-medium">{dateStr}</span>
                <span className="text-white/85 text-[13px] font-semibold">
                  {time}
                  <span className="text-white/35">:{seconds}</span>
                </span>
              </div>
            </div>

            {/* Active panel */}
            <div className="flex-1 flex flex-col min-h-0 overflow-hidden w-full relative">
              {/* Enter-only transition. AnimatePresence mode="wait" gated the new panel on the
                  old one's exit, and an exit that never settled left tab clicks doing nothing. */}
              <motion.div
                  key={activeTab}
                  id={`panel-${activeTab}`}
                  role="tabpanel"
                  aria-labelledby={`tab-${activeTab}`}
                  className={`flex-1 min-h-0 w-full flex flex-col px-1 ${panelScrolls ? "overflow-y-auto island-scroll pb-3" : "overflow-hidden"}`}
                  custom={tabDirection}
                  variants={panelVariants}
                  initial="enter"
                  animate="center"
                  transition={panelTransition}
                >
                  {renderPanel(activeTab)}
                </motion.div>
            </div>

            {/* Dock */}
            <div
              className="relative flex items-stretch mt-2 rounded-[22px] bg-white/[0.06] flex-shrink-0 overflow-hidden"
              role="tablist"
              aria-label="PILLAR modules"
            >
              {safeTabs.map(tab => {
                const selected = activeTab === tab.id;
                const badge = tab.hasBadge && notifications.length > 0 ? notifications.length : 0;
                return (
                  <motion.button
                    key={tab.id}
                    id={`tab-${tab.id}`}
                    role="tab"
                    type="button"
                    aria-selected={selected}
                    aria-controls={`panel-${tab.id}`}
                    aria-label={tab.id === "notifications" && notifications.length > 0 ? `Notifications (${notifications.length} unread)` : tab.ariaLabel}
                    tabIndex={selected ? 0 : -1}
                    className={`relative flex-1 h-[50px] flex flex-col items-center justify-center gap-[3px] transition-colors ${
                      selected ? "text-white" : "text-white/40 hover:text-white/75"
                    }`}
                    onClick={() => setActiveTab(tab.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        setActiveTab(tab.id);
                      }
                    }}
                    whileTap={{ scale: 0.9 }}
                    transition={{ type: "spring", stiffness: 600, damping: 30 }}
                  >
                    {selected && (
                      <motion.span
                        layoutId="dock-active"
                        className="absolute inset-1 rounded-[18px] bg-white/[0.12]"
                        style={{ boxShadow: "inset 0 0.5px 0 rgba(255,255,255,0.1)" }}
                        transition={reducedMotion ? { duration: 0 } : { type: "spring", stiffness: 520, damping: 38 }}
                      />
                    )}
                    <span className="relative">
                      <tab.Icon size={18} strokeWidth={selected ? 2.3 : 2} />
                      {badge > 0 && (
                        <span
                          className="absolute -top-1.5 -right-2.5 min-w-[15px] h-[15px] px-1 rounded-full text-[9px] text-white font-bold flex items-center justify-center tabular-nums"
                          style={{ background: SYSTEM_COLORS.red, boxShadow: "0 0 0 2px #0b0b0b" }}
                          aria-hidden="true"
                        >
                          {badge > 9 ? "9+" : badge}
                        </span>
                      )}
                    </span>
                    <span className="relative text-[9.5px] font-semibold leading-none tracking-tight">{tab.label}</span>
                  </motion.button>
                );
              })}
            </div>
          </motion.div>
        )}

      {contextMenu.isOpen && (
        <div
          className="fixed inset-0 z-[120]"
          onClick={closeContextMenu}
          onContextMenu={(e) => {
            e.preventDefault();
            closeContextMenu();
          }}
        >
          <motion.div
            className="absolute min-w-[168px] rounded-[14px] bg-black/95 p-1"
            style={{ left: contextMenu.x, top: contextMenu.y, boxShadow: "0 0 0 0.5px rgba(255,255,255,0.12), 0 12px 32px rgba(0,0,0,0.5)", originX: 0, originY: 0 }}
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ type: "spring", stiffness: 600, damping: 34 }}
          >
            {[
              { label: isExpanded ? "Collapse" : "Expand", run: () => void triggerWorkflowAction("toggle_expand") },
              { label: "Open Focus", run: () => void triggerWorkflowAction("open_productivity_tab") },
              ...(isExpanded
                ? [
                    { label: "Previous tab", run: () => goToTab(-1) },
                    { label: "Next tab", run: () => goToTab(1) },
                  ]
                : []),
            ].map((item) => (
              <button
                key={item.label}
                type="button"
                className="w-full text-left text-[12px] font-medium text-white/85 px-2.5 h-8 rounded-[10px] hover:bg-white/[0.1] transition-colors"
                onClick={() => {
                  item.run();
                  closeContextMenu();
                }}
              >
                {item.label}
              </button>
            ))}
          </motion.div>
        </div>
      )}

    </motion.div>
    </>
  );
}
