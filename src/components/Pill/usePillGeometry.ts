import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import type { MotionValue } from "motion/react";
import { createGeometryQueue, islandFits } from "../../lib/island/geometryQueue";
import { getIslandLimits, useIslandLimits, type IslandLimits } from "../../lib/island/limits";
import { ipc, type IslandGeometry } from "../../lib/ipc";
import { dlog } from "../../lib/debugLog";
import { expandedSize, ISLAND_TOP_INSET, limitSize, meetingAlertSize, pillDimensions } from "./animations";
import { assistantMaxSize } from "./assistantLayout";
import { toastMaxSize } from "./toastLayout";

const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

/** The region geometry plus the stage window around every island shape (logical px). */
export type StagedGeometry = IslandGeometry & { stageWidth: number; stageHeight: number };

/**
 * The largest island shape on a monitor with these limits: the panel, the alert, the biggest
 * toast, the compact and ringer shapes, each no larger than the monitor allows. Without limits
 * these are the preferred sizes.
 */
export function maxShapeSize(limits: IslandLimits = getIslandLimits()): { width: number; height: number } {
  const d = pillDimensions;
  const expanded = expandedSize(limits);
  const alert = limitSize(meetingAlertSize(2, true, true), limits);
  const notification = limitSize(toastMaxSize(), limits);
  // The smart-search card stays inside the panel's bound, so this never grows the stage.
  const assistant = limitSize(assistantMaxSize(), limits);
  const compactMax = Math.min(d.compact.maxWidth, limits.maxWidth);
  return {
    width: Math.max(expanded.width, alert.width, notification.width, assistant.width, Math.min(d.compactMeeting.maxWidth, limits.maxWidth), compactMax, Math.min(d.ringer.width, limits.maxWidth)),
    height: Math.max(expanded.height, alert.height, notification.height, assistant.height, d.compact.height, d.ringer.height),
  };
}

/**
 * The native window: a fixed stage as large as the largest island shape, plus the island's gap
 * below the screen's top edge (the window starts AT the edge so its top strip can bridge to the
 * island; see ISLAND_TOP_INSET). It never resizes or moves while the island animates (a resized
 * WebView2 window shows its previous frame at the new size and stalls its frame pipeline); the
 * island morphs inside it and only the window region (the clickable shape) follows. Built from
 * the same limited sizes as the island's targets, so it never exceeds the monitor's work area.
 */
export function stageSize(limits: IslandLimits = getIslandLimits()): { width: number; height: number } {
  const shape = maxShapeSize(limits);
  return { width: shape.width, height: shape.height + ISLAND_TOP_INSET };
}

interface Springs {
  width: MotionValue<number>;
  height: MotionValue<number>;
}

/**
 * Keeps the window region on the island. `target` is the shape the current state needs; the
 * queue sends it in order, latest-wins, never while unchanged. A larger region is sent at once
 * (in the layout effect of the commit that asked for it, so it is in place before the island's
 * first larger frame); a smaller one waits until the island's animated shape fits inside it,
 * so the region never cuts the island mid-morph.
 */
export function usePillGeometry(target: IslandGeometry, springs: Springs): { invalidate: () => void } {
  const springsRef = useRef(springs);
  springsRef.current = springs;

  const queueRef = useRef<ReturnType<typeof createGeometryQueue> | null>(null);
  if (queueRef.current === null) {
    queueRef.current = createGeometryQueue({
      send: async (geometry) => {
        const startedAt = performance.now();
        const ok = await ipc.setIslandGeometry(geometry);
        const ms = Math.round(performance.now() - startedAt);
        if (ok) dlog(ms > 300 ? "warn" : "debug", "pill", `region ${geometry.width}x${geometry.height} applied in ${ms}ms`);
        else dlog("warn", "pill", `region ${geometry.width}x${geometry.height} failed [WIN-501]`);
        return ok;
      },
      fits: (t) => islandFits({ width: springsRef.current.width.get(), height: springsRef.current.height.get() }, t),
    });
  }
  const queue = queueRef.current;

  useEffect(() => {
    const poke = () => queue.poke();
    const offWidth = springs.width.on("change", poke);
    const offHeight = springs.height.on("change", poke);
    return () => {
      offWidth();
      offHeight();
      queue.dispose();
    };
  }, [queue, springs.width, springs.height]);

  const { width, height, radius } = target;
  const limits = useIslandLimits();
  useIsomorphicLayoutEffect(() => {
    const stage = stageSize(limits);
    const geometry: StagedGeometry = { width, height, radius, stageWidth: stage.width, stageHeight: stage.height };
    queue.request(geometry);
  }, [queue, width, height, radius, limits]);

  const invalidate = useCallback(() => queue.invalidate(), [queue]);
  return { invalidate };
}
