import { useCallback, useEffect, useRef } from "react";
import type { MotionValue } from "motion/react";
import { createGeometryQueue } from "../../lib/island/geometryQueue";
import { ipc, type IslandGeometry } from "../../lib/ipc";
import { dlog } from "../../lib/debugLog";

/** Tolerance between the animated island and the window it must fit inside. */
const FIT_SLACK_X = 4;
const FIT_SLACK_Y = 2;

interface Springs {
  width: MotionValue<number>;
  height: MotionValue<number>;
}

/**
 * Keeps the native window sized for the island. `target` is the window geometry the
 * current state needs; the queue sends it in order, latest-wins, never while unchanged,
 * and holds back shrinks until the island's springs have settled inside the new size.
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
        if (ok) dlog(ms > 300 ? "warn" : "debug", "pill", `geometry ${geometry.width}x${geometry.height} applied in ${ms}ms`);
        else dlog("warn", "pill", `geometry ${geometry.width}x${geometry.height} failed [WIN-501]`);
        return ok;
      },
      fits: (t) => springsRef.current.width.get() <= t.width - FIT_SLACK_X && springsRef.current.height.get() <= t.height - FIT_SLACK_Y,
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

  const { width, height, radius, animate } = target;
  useEffect(() => {
    queue.request({ width, height, radius, animate });
  }, [queue, width, height, radius, animate]);

  const invalidate = useCallback(() => queue.invalidate(), [queue]);
  return { invalidate };
}
