import { useCallback, useEffect, useRef, useState } from "react";
import type { TabId } from "../components/Pill/tabs";
import { HOVER_INTENT_MS, LEAVE_COLLAPSE_MS, PINNED_LEAVE_COLLAPSE_MS } from "../lib/island/timing";

interface PillIsland {
  /** The user layer is expanded (it may be hidden behind an alert or a toast). */
  expanded: boolean;
  pinned: boolean;
  /** A meeting alert or a notification is on screen. */
  temporary: boolean;
  expand: (tab?: TabId) => void;
  pin: (tab?: TabId) => void;
  collapse: () => void;
  setHovering: (hovering: boolean) => void;
}

interface UsePillStateReturn {
  isBooting: boolean;
  completeBootAnimation: () => void;
  /** DOM pointer events: the window is exactly the island, so enter/leave are the island's own. */
  pointerEnter: () => void;
  pointerLeave: () => void;
  /**
   * Stops the pointer that is on the island right now from re-expanding it (after Escape, a
   * dismissed alert, a toggle). Lifts as soon as the pointer leaves.
   */
  holdCollapsed: () => void;
}

/**
 * How the pointer drives the user layer of the island (the state itself lives in the island
 * reducer). Nothing here is global: there is no mouse hook and no outside-click detection,
 * the window is non-activating and island-sized.
 *
 *  - Rest on the island for HOVER_INTENT_MS: it expands to the last-used tab.
 *  - Leave: it collapses after LEAVE_COLLAPSE_MS, or PINNED_LEAVE_COLLAPSE_MS when a click or
 *    toggle pinned it. The same timer covers an island that was opened while the pointer was
 *    elsewhere (tray, second launch), so it can never stay open unattended.
 *  - While a meeting alert or toast is showing nothing is armed; when it ends the rules above
 *    apply again to whatever the user had open.
 * Both are effects of (pointer, island) state, so a stale timer cannot outlive its condition.
 */
export function usePillState(island: PillIsland): UsePillStateReturn {
  const { expanded, pinned, temporary, expand, collapse, setHovering } = island;
  const [isBooting, setBooting] = useState(true);
  const [inside, setInside] = useState(false);
  const heldRef = useRef(false);
  const insideRef = useRef(false);

  const completeBootAnimation = useCallback(() => setBooting(false), []);

  const pointerEnter = useCallback(() => {
    insideRef.current = true;
    setInside(true);
    setHovering(true);
  }, [setHovering]);

  const pointerLeave = useCallback(() => {
    insideRef.current = false;
    heldRef.current = false;
    setInside(false);
    setHovering(false);
  }, [setHovering]);

  // Only a pointer that is on the island can be held back; one elsewhere (a tray or shortcut
  // toggle) has no leave event coming to lift the hold, which would block its next hover.
  const holdCollapsed = useCallback(() => {
    heldRef.current = insideRef.current;
  }, []);

  useEffect(() => {
    if (!inside || isBooting || expanded || temporary || heldRef.current) return;
    const handle = setTimeout(() => expand(), HOVER_INTENT_MS);
    return () => clearTimeout(handle);
  }, [inside, isBooting, expanded, temporary, expand]);

  useEffect(() => {
    if (!expanded || inside || temporary) return;
    const handle = setTimeout(collapse, pinned ? PINNED_LEAVE_COLLAPSE_MS : LEAVE_COLLAPSE_MS);
    return () => clearTimeout(handle);
  }, [expanded, pinned, inside, temporary, collapse]);

  return { isBooting, completeBootAnimation, pointerEnter, pointerLeave, holdCollapsed };
}
