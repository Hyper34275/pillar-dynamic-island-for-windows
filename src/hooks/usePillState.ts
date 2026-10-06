import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import type { InteractionState } from "../types/pill";
import { dlog } from "../lib/debugLog";

// =============================================================================
// Timing Constants
// =============================================================================

const HOVER_DELAY_MS = 100;
const EXIT_DELAY_MS = 120;
// Short grace before an expanded island closes on pointer leave. Leave is reported
// by the backend's global cursor tracking, so the grace only absorbs edge jitter.
const EXPANDED_EXIT_DELAY_MS = 400;

export type { InteractionState };

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
   * keyboard, tray, shortcut… If the pointer isn't over the island at that moment,
   * no leave timer runs: it stays open until an outside press, Escape, the toggle,
   * or the pointer enters and then leaves.
   */
  expand: (source: string) => void;
  /** Close to idle and cancel pending hover/leave timers. `reason` is logged. */
  collapse: (reason: string) => void;
  completeBootAnimation: () => void;
}

export function usePillState(): UsePillStateReturn {
  const [interactionState, setInteractionState] = useState<InteractionState>("boot");
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const exitTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const interactionStateRef = useRef<InteractionState>(interactionState);
  interactionStateRef.current = interactionState;
  // Whether the cursor is over the island right now (tracked even during boot).
  const pointerInsideRef = useRef(false);

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

  useEffect(() => clearInteractionTimers, [clearInteractionTimers]);

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
  };
}
