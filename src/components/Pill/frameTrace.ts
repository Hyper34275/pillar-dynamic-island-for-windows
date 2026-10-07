// Per-frame instrumentation of the island's content ownership, for the motion harness and dev.
//
// Off by default and free when off: no frame callback is even registered until something calls
// `window.__islandTrace.start()` (the runtime harness does, over CDP; a developer can from the
// console), so the idle island keeps running no frame loop at all. While on, once per animation
// frame it reads what is ON SCREEN (the DOM's own opacities and transforms, i.e. what
// the frame actually shows, not what the code meant) and
//  - appends one record to __ISLAND_FRAMES__ (the fields of the brief's debug list), and
//  - checks the ownership invariants, reporting a violation with console.error (never throws:
//    instrumentation must not take the island down):
//      at most 2 tab content layers, at most 2 toast payloads, one readable tab title,
//      one readable toast title, compact and expanded never both readable.

import { frame, cancelFrame } from "motion/react";
import { useEffect, useRef } from "react";

/** The session / tab state the DOM does not carry; read fresh every frame. */
export interface TraceState {
  view: string;
  shownKey: string;
  origin: string;
  progress: number;
  activeTab: string;
  queueLength: number;
  sessionPhase: string;
  sessionId: number;
  currentToastId: number | null;
}

export interface IslandFrameRecord {
  t: number;
  targetPresentation: string;
  shownKey: string;
  origin: string;
  presentationProgress: number;
  contentOwner: string;
  outgoingContentId: string | null;
  incomingContentId: string | null;
  mountedTransitionLayers: number;
  tabLayers: number;
  toastPayloads: number;
  activeTab: string;
  targetTab: string;
  tabIndicatorX: number;
  tabIndicatorVelocity: number;
  currentToastId: number | null;
  targetToastId: number | null;
  notificationQueueLength: number;
  notificationSessionState: string;
  sessionId: number;
  compactOpacity: number;
  expandedOpacity: number;
  headerOpacity: number;
  bodyOpacity: number;
  dockOpacity: number;
  toastOpacity: number;
  shellWidth: number;
  shellHeight: number;
  violations: string[];
}

/** A layer is "readable" above this opacity (the same bar as the tests). */
export const READABLE = 0.35;

type Sink = {
  __ISLAND_FRAMES__?: unknown;
  /** start(): record into __ISLAND_FRAMES__ (created if absent) and check; stop(): unregister. */
  __islandTrace?: { start: (options?: { record?: boolean }) => void; stop: () => void };
};

const num = (value: string | null | undefined, fallback: number) => {
  const n = value === undefined || value === null || value === "" ? NaN : Number(value);
  return Number.isFinite(n) ? n : fallback;
};
/** The opacity an element is drawn with, including its ancestors up to (not including) `stop`. */
function drawnOpacity(el: HTMLElement | null, stop: HTMLElement): number {
  let o = 1;
  for (let node = el; node && node !== stop; node = node.parentElement) o *= num(node.style.opacity, 1);
  return el ? o : 0;
}
function translateX(el: HTMLElement | null): number {
  if (!el) return 0;
  const m = /translateX\((-?[\d.]+)(px|%)\)/.exec(el.style.transform);
  if (!m) return 0;
  return m[2] === "%" ? (Number(m[1]) / 100) * el.offsetWidth : Number(m[1]);
}

/** Reads one frame off the island element. Pure apart from the DOM read; exported for tests. */
export function readFrame(island: HTMLElement, state: TraceState, t: number, previousX: number | null, dt: number): IslandFrameRecord {
  const layer = (name: string) => island.querySelector<HTMLElement>(`[data-layer="${name}"]`);
  const part = (anchor: string) => layer("expanded")?.querySelector<HTMLElement>(`[data-part="${anchor}"]`) ?? null;
  const compactOpacity = drawnOpacity(layer("compact"), island);
  const headerOpacity = drawnOpacity(part("top-start"), island);
  const bodyOpacity = drawnOpacity(part("top"), island);
  const dockOpacity = drawnOpacity(part("bottom"), island);
  const expandedOpacity = Math.max(headerOpacity, bodyOpacity, dockOpacity);
  const toast = layer("toast");
  const toastOpacity = drawnOpacity(toast, island);

  const panels = [...island.querySelectorAll<HTMLElement>("[data-panel]")];
  const titles = [...island.querySelectorAll<HTMLElement>("[data-tab-title]")];
  const payloads = [...island.querySelectorAll<HTMLElement>('[data-layer="notification"]')];
  const readableTitles = titles.filter((el) => drawnOpacity(el, island) > READABLE).length;
  const readablePayloads = payloads.filter((el) => drawnOpacity(el, island) > READABLE).length;

  const indicator = island.querySelector<HTMLElement>("[data-tab-indicator]");
  const x = translateX(indicator);
  const velocity = previousX === null || dt <= 0 ? 0 : ((x - previousX) / dt) * 1000;

  // Who owns the screen: the most visible of the island's layers.
  const owners: [string, number][] = [
    ["compact", compactOpacity],
    ["expanded", expandedOpacity],
    ["toast", toastOpacity],
    ["meetingAlert", drawnOpacity(layer("meetingAlert"), island)],
    ["ringer", drawnOpacity(layer("ringer"), island)],
  ];
  owners.sort((a, b) => b[1] - a[1]);
  const present = (els: HTMLElement[], attr: string, leaving: (el: HTMLElement) => boolean) => ({
    outgoing: els.find(leaving)?.getAttribute(attr) ?? null,
    incoming: els.find((el) => !leaving(el))?.getAttribute(attr) ?? null,
  });
  const tabIds = present(panels, "data-panel", (el) => el.getAttribute("aria-hidden") === "true");
  const payloadIds = present(payloads, "data-payload-id", (el) => el.getAttribute("data-payload") === "leaving");

  const violations: string[] = [];
  if (panels.length > 2) violations.push(`tabTransitionLayers=${panels.length} > 2`);
  if (payloads.length > 2) violations.push(`toastTransitionLayers=${payloads.length} > 2`);
  if (readableTitles > 1) violations.push(`${readableTitles} readable tab titles`);
  if (readablePayloads > 1) violations.push(`${readablePayloads} readable toast payloads`);
  if (compactOpacity > READABLE && bodyOpacity > READABLE) violations.push("compact and expanded body both readable");
  if (compactOpacity > READABLE && toastOpacity > READABLE) violations.push("compact and toast both readable");

  return {
    t,
    targetPresentation: state.view,
    shownKey: state.shownKey,
    origin: state.origin,
    presentationProgress: state.progress,
    contentOwner: owners[0][1] > 0.01 ? owners[0][0] : "none",
    outgoingContentId: tabIds.outgoing ?? payloadIds.outgoing,
    incomingContentId: tabIds.incoming ?? payloadIds.incoming,
    mountedTransitionLayers: island.querySelectorAll("[data-layer]").length,
    tabLayers: panels.length,
    toastPayloads: payloads.length,
    activeTab: tabIds.incoming ?? state.activeTab,
    targetTab: state.activeTab,
    tabIndicatorX: x,
    tabIndicatorVelocity: velocity,
    currentToastId: payloadIds.incoming === null ? null : Number(payloadIds.incoming),
    targetToastId: state.currentToastId,
    notificationQueueLength: state.queueLength,
    notificationSessionState: state.sessionPhase,
    sessionId: state.sessionId,
    compactOpacity,
    expandedOpacity,
    headerOpacity,
    bodyOpacity,
    dockOpacity,
    toastOpacity,
    shellWidth: num(island.style.width.replace("px", ""), island.offsetWidth),
    shellHeight: num(island.style.height.replace("px", ""), island.offsetHeight),
    violations,
  };
}

/**
 * Samples the island once per animation frame while the harness (or a developer) asks for it.
 * `getState` is read fresh each frame (keep it in a ref, no re-subscription per render).
 */
export function useIslandFrameTrace(getState: () => TraceState): void {
  const stateRef = useRef(getState);
  stateRef.current = getState;
  useEffect(() => {
    if (typeof window === "undefined") return;
    const sink = window as unknown as Sink;
    let previousX: number | null = null;
    let last = 0;
    let record = true;
    const tick = ({ timestamp }: { timestamp: number }) => {
      const island = document.querySelector<HTMLElement>("[data-expanded]");
      if (!island) return;
      const f = readFrame(island, stateRef.current(), timestamp, previousX, timestamp - last);
      previousX = f.tabIndicatorX;
      last = timestamp;
      if (record && Array.isArray(sink.__ISLAND_FRAMES__)) (sink.__ISLAND_FRAMES__ as IslandFrameRecord[]).push(f);
      if (f.violations.length) console.error("[island] ownership violation", f.violations.join("; "), f);
    };
    let running = false;
    const stop = () => {
      if (running) cancelFrame(tick);
      running = false;
    };
    sink.__islandTrace = {
      start: (options) => {
        record = options?.record ?? true;
        if (record && !Array.isArray(sink.__ISLAND_FRAMES__)) sink.__ISLAND_FRAMES__ = [];
        if (running) return;
        running = true;
        // keepAlive: every frame while tracing (motion's loop otherwise sleeps when nothing moves).
        frame.update(tick, true);
      },
      stop,
    };
    return () => {
      stop();
      delete sink.__islandTrace;
    };
  }, []);
}
