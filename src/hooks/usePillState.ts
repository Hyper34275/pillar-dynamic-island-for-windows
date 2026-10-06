import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import type { 
  InteractionState, 
  ContentState, 
  ContentStateType,
  TimerRunningState,
  TimerAlertState,
  MediaState,
  NotificationState,
} from "../types/pill";
import { createContentState } from "../types/pill";
import { dlog } from "../lib/debugLog";

// =============================================================================
// Timing Constants
// =============================================================================

const HOVER_DELAY_MS = 100;
const EXIT_DELAY_MS = 120;
// Short grace before an expanded island closes on pointer leave. Leave is now
// reported by the backend's global cursor tracking (with pointer capture, so a
// slider drag slipping off the edge isn't a leave) — the old 1.8s made it feel
// like leaving wasn't detected at all.
const EXPANDED_EXIT_DELAY_MS = 400;
const NOTIFICATION_DURATION_MS = 5000;

// =============================================================================
// Types
// =============================================================================

export type { InteractionState, ContentState, ContentStateType };

// =============================================================================
// Hook
// =============================================================================

interface UsePillStateReturn {
  isBooting: boolean;
  isIdle: boolean;
  isHovering: boolean;
  isExpanded: boolean;
  /** Cursor entered the island (native tracking or DOM fallback). */
  pointerEnter: () => void;
  /** Cursor left the island (native tracking or DOM fallback). */
  pointerLeave: () => void;
  /**
   * Expand from idle/hover (ignored during boot). `source` is logged — click,
   * keyboard, tray, shortcut, workflow… If the pointer isn't over the island at
   * that moment, no leave timer runs: it stays open until an outside press,
   * Escape, the toggle, or the pointer enters and then leaves.
   */
  expand: (source: string) => void;
  /** Close to idle and cancel pending hover/leave timers. `reason` is logged. */
  collapse: (reason: string) => void;
  completeBootAnimation: () => void;
  
  // New content state API
  contentStates: ContentState[];
  activeContentState: ContentState | null;
  backgroundStates: ContentState[];
  addContentState: (state: ContentState) => void;
  removeContentState: (id: string) => void;
  updateContentState: (id: string, updates: Record<string, unknown>) => void;
  clearContentStates: (type?: ContentStateType) => void;
  setTimerState: (timer: TimerRunningState["data"] | null) => void;
  setTimerAlert: (alert: TimerAlertState["data"] | null) => void;
  setMediaState: (media: MediaState["data"] | null) => void;
  showNotification: (notification: Omit<NotificationState["data"], "expiresAt">) => void;
}

export function usePillState(): UsePillStateReturn {
  // Interaction state
  const [interactionState, setInteractionState] = useState<InteractionState>("boot");
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const exitTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const interactionStateRef = useRef<InteractionState>(interactionState);
  interactionStateRef.current = interactionState;
  // Whether the cursor is over the island right now (tracked even during boot).
  const pointerInsideRef = useRef(false);
  // Auto-dismiss timers for notification content states; cleaned up on unmount
  // so they don't fire setState after the component is gone.
  const notificationTimeoutsRef = useRef<Set<ReturnType<typeof setTimeout>>>(new Set());

  // Content states (multiple can be active simultaneously)
  const [contentStates, setContentStates] = useState<ContentState[]>([]);

  // ==========================================================================
  // Content State Management
  // ==========================================================================

  const addContentState = useCallback((state: ContentState) => {
    setContentStates(prev => {
      // Remove any existing state of the same type (except notifications which can stack)
      const filtered = state.type === "notification" 
        ? prev 
        : prev.filter(s => s.type !== state.type);
      return [...filtered, state].sort((a, b) => b.priority - a.priority);
    });
  }, []);

  const removeContentState = useCallback((id: string) => {
    setContentStates(prev => prev.filter(s => s.id !== id));
  }, []);

  const updateContentState = useCallback((
    id: string, 
    updates: Record<string, unknown>
  ) => {
    setContentStates(prev => prev.map(s => {
      if (s.id !== id) return s;
      if ("data" in s && s.data) {
        return { ...s, data: { ...(s.data as Record<string, unknown>), ...updates } } as ContentState;
      }
      return s;
    }));
  }, []);

  const clearContentStates = useCallback((type?: ContentStateType) => {
    if (type) {
      setContentStates(prev => prev.filter(s => s.type !== type));
    } else {
      setContentStates([]);
    }
  }, []);

  // ==========================================================================
  // Convenience Methods
  // ==========================================================================

  const setTimerState = useCallback((timer: TimerRunningState["data"] | null) => {
    if (timer === null) {
      setContentStates(prev => prev.filter(s => s.type !== "timer_running"));
    } else {
      const state = createContentState<TimerRunningState>("timer_running", timer);
      addContentState(state);
    }
  }, [addContentState]);

  const setTimerAlert = useCallback((alert: TimerAlertState["data"] | null) => {
    if (alert === null) {
      setContentStates(prev => prev.filter(s => s.type !== "timer_alert"));
    } else {
      const state = createContentState<TimerAlertState>("timer_alert", alert);
      addContentState(state);
    }
  }, [addContentState]);

  const setMediaState = useCallback((media: MediaState["data"] | null) => {
    if (media === null) {
      setContentStates(prev => prev.filter(s => s.type !== "media"));
    } else {
      const state = createContentState<MediaState>("media", media);
      addContentState(state);
    }
  }, [addContentState]);

  const showNotification = useCallback((
    notification: Omit<NotificationState["data"], "expiresAt">
  ) => {
    const state = createContentState<NotificationState>("notification", {
      ...notification,
      expiresAt: Date.now() + NOTIFICATION_DURATION_MS,
    });
    addContentState(state);

    // Auto-remove notification after expiry — track the handle so unmount cancels it.
    const handle = setTimeout(() => {
      notificationTimeoutsRef.current.delete(handle);
      removeContentState(state.id);
    }, NOTIFICATION_DURATION_MS);
    notificationTimeoutsRef.current.add(handle);
  }, [addContentState, removeContentState]);

  // ==========================================================================
  // Derived Content State
  // ==========================================================================

  const { activeContentState, backgroundStates } = useMemo(() => {
    if (contentStates.length === 0) {
      return { activeContentState: null, backgroundStates: [] };
    }
    // States are already sorted by priority (highest first)
    const [active, ...background] = contentStates;
    return { activeContentState: active, backgroundStates: background };
  }, [contentStates]);

  // ==========================================================================
  // Interaction State Management
  // ==========================================================================
  // boot → idle (completeBootAnimation)
  // idle → hover:      pointerEnter, after HOVER_DELAY_MS
  // hover → idle:      pointerLeave, after EXIT_DELAY_MS
  // idle/hover → expanded: expand(source)
  // expanded → idle:   collapse(reason) — directly, or pointerLeave after
  //                    EXPANDED_EXIT_DELAY_MS (cancelled by re-entering)
  // Timer callbacks re-check the live state via the ref, so a stale timer can
  // never move the pill somewhere it shouldn't be.

  const clearInteractionTimers = useCallback(() => {
    if (hoverTimeoutRef.current) {
      clearTimeout(hoverTimeoutRef.current);
      hoverTimeoutRef.current = null;
    }
    if (exitTimeoutRef.current) {
      clearTimeout(exitTimeoutRef.current);
      exitTimeoutRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => {
      clearInteractionTimers();
      notificationTimeoutsRef.current.forEach((handle) => clearTimeout(handle));
      notificationTimeoutsRef.current.clear();
    };
  }, [clearInteractionTimers]);

  // Update the ref eagerly (not just on the next render) so back-to-back calls in
  // the same tick — e.g. collapse() then a bubbling click's expand() — see it.
  const transitionTo = useCallback((next: InteractionState) => {
    interactionStateRef.current = next;
    setInteractionState(next);
  }, []);

  const completeBootAnimation = useCallback(() => {
    transitionTo("idle");
  }, [transitionTo]);

  const collapse = useCallback((reason: string) => {
    clearInteractionTimers();
    const s = interactionStateRef.current;
    if (s === "boot" || s === "idle") return; // idempotent: several close paths can race
    dlog("info", "pill", `collapse: ${reason}`);
    transitionTo("idle");
  }, [clearInteractionTimers, transitionTo]);

  const expand = useCallback((source: string) => {
    const s = interactionStateRef.current;
    // Skip while booting so we don't fight the boot animation.
    if (s === "boot") {
      dlog("debug", "pill", `expand ignored during boot: ${source}`);
      return;
    }
    if (s === "expanded") return;
    clearInteractionTimers();
    dlog(
      "info",
      "pill",
      `expand: ${source}${pointerInsideRef.current ? "" : " (pointer outside — stays open until outside press/escape/toggle)"}`
    );
    transitionTo("expanded");
  }, [clearInteractionTimers, transitionTo]);

  const pointerEnter = useCallback(() => {
    pointerInsideRef.current = true;
    const s = interactionStateRef.current;
    if (s === "boot") return;

    if (exitTimeoutRef.current) {
      clearTimeout(exitTimeoutRef.current);
      exitTimeoutRef.current = null;
      if (s === "expanded") dlog("debug", "pill", "pointer re-entered: expanded leave grace cancelled");
    }
    if (s !== "idle") return;

    if (hoverTimeoutRef.current) clearTimeout(hoverTimeoutRef.current);
    hoverTimeoutRef.current = setTimeout(() => {
      hoverTimeoutRef.current = null;
      if (interactionStateRef.current === "idle" && pointerInsideRef.current) {
        transitionTo("hover");
      }
    }, HOVER_DELAY_MS);
  }, [transitionTo]);

  const pointerLeave = useCallback(() => {
    const wasInside = pointerInsideRef.current;
    pointerInsideRef.current = false;
    if (hoverTimeoutRef.current) {
      clearTimeout(hoverTimeoutRef.current);
      hoverTimeoutRef.current = null;
    }

    const s = interactionStateRef.current;
    if (s !== "hover" && s !== "expanded") return;
    // A leave without a preceding enter (e.g. expanded from the tray while the
    // cursor was elsewhere) must not start the close timer.
    if (s === "expanded" && !wasInside) return;

    if (exitTimeoutRef.current) clearTimeout(exitTimeoutRef.current);
    if (s === "hover") {
      exitTimeoutRef.current = setTimeout(() => {
        exitTimeoutRef.current = null;
        if (interactionStateRef.current === "hover" && !pointerInsideRef.current) {
          transitionTo("idle");
        }
      }, EXIT_DELAY_MS);
      return;
    }

    exitTimeoutRef.current = setTimeout(() => {
      exitTimeoutRef.current = null;
      if (interactionStateRef.current === "expanded" && !pointerInsideRef.current) {
        collapse(`pointer left island (${EXPANDED_EXIT_DELAY_MS}ms grace)`);
      }
    }, EXPANDED_EXIT_DELAY_MS);
  }, [collapse, transitionTo]);

  // ==========================================================================
  // Derived Interaction State
  // ==========================================================================

  const derivedInteractionState = useMemo(() => ({
    isBooting: interactionState === "boot",
    isIdle: interactionState === "idle",
    isHovering: interactionState === "hover",
    isExpanded: interactionState === "expanded",
  }), [interactionState]);

  return {
    ...derivedInteractionState,
    pointerEnter,
    pointerLeave,
    expand,
    collapse,
    completeBootAnimation,
    
    // Content state API
    contentStates,
    activeContentState,
    backgroundStates,
    addContentState,
    removeContentState,
    updateContentState,
    clearContentStates,
    setTimerState,
    setTimerAlert,
    setMediaState,
    showNotification,
  };
}
