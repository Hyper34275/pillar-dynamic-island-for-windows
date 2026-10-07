import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { TabId } from "../components/Pill/tabs";
import { dlog } from "../lib/debugLog";
import { FOREGROUND_GRACE_MS, HOVER_INTENT_MS, LEAVE_COLLAPSE_MS, UNATTENDED_COLLAPSE_MS } from "../lib/island/timing";
import { islandTyping } from "../lib/notes/typing";

interface PillIsland {
  /** The user layer is expanded (it may be hidden behind an alert or a toast). */
  expanded: boolean;
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
  /** Another app's window became active (backend `foreground-changed`). */
  foregroundChanged: () => void;
}

/**
 * How the pointer drives the user layer of the island (the state itself lives in the island
 * reducer). Nothing here is global: there is no mouse hook, the window is non-activating and
 * island-sized.
 *
 *  - Rest on the island for HOVER_INTENT_MS: it expands to the last-used tab.
 *  - Leave: once the pointer has been on the expanded island, it collapses LEAVE_COLLAPSE_MS
 *    after leaving, whether or not it was clicked. An island opened while the pointer was
 *    elsewhere (tray, second launch) gets UNATTENDED_COLLAPSE_MS to be reached, so it can never
 *    stay open unattended.
 *  - A click outside it (desktop, taskbar, another app) activates that window; the backend
 *    reports the foreground change and the island collapses at once, unless the pointer is on
 *    it or it opened less than FOREGROUND_GRACE_MS ago.
 *  - While a meeting alert or toast is showing nothing is armed; when it ends the rules above
 *    apply again to whatever the user had open.
 *  - While a note is being typed in the island (lib/notes/typing) leaving does not collapse it:
 *    the leave timer arms when the typing ends. A click outside still closes it.
 * The timers are effects of (pointer, island) state, so a stale timer cannot outlive its condition.
 */
export function usePillState(island: PillIsland): UsePillStateReturn {
  const { expanded, temporary, expand, collapse, setHovering } = island;
  const [isBooting, setBooting] = useState(true);
  const [inside, setInside] = useState(false);
  // The pointer has been on the island since it last expanded (decides which leave grace applies).
  const [visited, setVisited] = useState(false);
  const heldRef = useRef(false);
  const insideRef = useRef(false);
  const expandedAtRef = useRef(0);

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
    if (expanded) {
      expandedAtRef.current = Date.now();
      setVisited(insideRef.current);
    } else {
      setVisited(false);
    }
  }, [expanded]);

  useEffect(() => {
    if (expanded && inside) setVisited(true);
  }, [expanded, inside]);

  useEffect(() => {
    if (!inside || isBooting || expanded || temporary || heldRef.current) return;
    const handle = setTimeout(() => expand(), HOVER_INTENT_MS);
    return () => clearTimeout(handle);
  }, [inside, isBooting, expanded, temporary, expand]);

  const typing = useSyncExternalStore(islandTyping.subscribe, islandTyping.get);

  useEffect(() => {
    if (!expanded || inside || temporary || typing) return;
    const delay = visited ? LEAVE_COLLAPSE_MS : UNATTENDED_COLLAPSE_MS;
    const handle = setTimeout(() => {
      dlog("info", "pill", visited ? `collapse: pointer left (${delay} ms)` : `collapse: opened elsewhere, pointer never arrived (${delay} ms)`);
      collapse();
    }, delay);
    return () => clearTimeout(handle);
  }, [expanded, inside, temporary, typing, visited, collapse]);

  const foregroundChanged = useCallback(() => {
    if (!expanded || temporary || insideRef.current) return;
    if (Date.now() - expandedAtRef.current < FOREGROUND_GRACE_MS) return;
    dlog("info", "pill", "collapse: another window became active (click outside the island)");
    collapse();
  }, [expanded, temporary, collapse]);

  return { isBooting, completeBootAnimation, pointerEnter, pointerLeave, holdCollapsed, foregroundChanged };
}
