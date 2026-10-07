// Ordered, latest-wins queue for the window region (the island's clickable shape inside the
// fixed stage window).
//
//  - At most one update is in flight; whatever was requested while it ran collapses into
//    a single follow-up with the newest target (intermediate sizes are never sent).
//  - A size that matches what was last sent is never sent again.
//  - Growing is immediate. Shrinking waits until the island's own animated shape has landed
//    inside the smaller region (`fits`, to half a pixel), bounded by `maxShrinkDelayMs`, so
//    the region never clips the island mid-animation.

import type { IslandGeometry } from "../ipc";

export interface GeometryQueueOptions {
  send: (geometry: IslandGeometry) => Promise<boolean>;
  /** True once the visible island fits inside `target`. */
  fits: (target: IslandGeometry) => boolean;
  maxShrinkDelayMs?: number;
  now?: () => number;
  setTimer?: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>;
  clearTimer?: (handle: ReturnType<typeof setTimeout>) => void;
}

/**
 * How far (logical px) the island may still stick out of a smaller region before the shrink is
 * applied: half a pixel, so the region never visibly clips the last frames of a landing shape
 * (2 px was measured cutting the closing pill's bottom edge for ~60 ms).
 */
export const FIT_SLACK_PX = 0.5;

/**
 * True once the animated island is no larger than `target` (give or take the slack). The slack
 * is a tolerance, never a margin: an island resting at exactly the target size must fit.
 */
export function islandFits(current: { width: number; height: number }, target: IslandGeometry): boolean {
  return current.width <= target.width + FIT_SLACK_PX && current.height <= target.height + FIT_SLACK_PX;
}

function sameSize(a: IslandGeometry | null, b: IslandGeometry): boolean {
  // The stage counts too: a monitor with other limits changes it while the island's own size stays.
  return !!a && a.width === b.width && a.height === b.height && a.radius === b.radius && a.stageWidth === b.stageWidth && a.stageHeight === b.stageHeight;
}

export function createGeometryQueue(options: GeometryQueueOptions) {
  // The deadline is a safety net for a shape that never reports landing (a hidden window runs
  // no frames), not the normal path: the island is within FIT_SLACK_PX in ~0.35 s.
  const { send, fits, maxShrinkDelayMs = 700 } = options;
  const now = options.now ?? (() => performance.now());
  const setTimer = options.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
  const clearTimer = options.clearTimer ?? ((h) => clearTimeout(h));

  let desired: IslandGeometry | null = null;
  let sent: IslandGeometry | null = null;
  let inFlight = false;
  let shrinkSince: number | null = null;
  let shrinkTimer: ReturnType<typeof setTimeout> | null = null;

  function cancelShrinkTimer() {
    if (shrinkTimer !== null) {
      clearTimer(shrinkTimer);
      shrinkTimer = null;
    }
  }

  function flush() {
    if (!desired) return;
    if (sameSize(sent, desired)) {
      shrinkSince = null;
      cancelShrinkTimer();
      return;
    }
    if (inFlight) return;

    const target = desired;
    const shrinking = !!sent && (target.width < sent.width || target.height < sent.height);
    if (shrinking && !fits(target)) {
      shrinkSince ??= now();
      const remaining = maxShrinkDelayMs - (now() - shrinkSince);
      if (remaining > 0) {
        // Re-armed on every poke; the deadline itself is anchored to the first deferral.
        cancelShrinkTimer();
        shrinkTimer = setTimer(() => {
          shrinkTimer = null;
          flush();
        }, remaining);
        return;
      }
    }

    shrinkSince = null;
    cancelShrinkTimer();
    const previous = sent;
    sent = target;
    inFlight = true;
    let failed = false;
    const fail = () => {
      // A failed resize must be retried by the next request, not skipped as "unchanged".
      failed = true;
      if (sent === target) sent = previous;
    };
    void send(target)
      .then((ok) => {
        if (!ok) fail();
      })
      .catch(fail)
      .finally(() => {
        inFlight = false;
        // Newer target queued meanwhile: send it. Otherwise never loop on a failing command.
        if (!failed || desired !== target) flush();
      });
  }

  return {
    /** Sets the newest desired window geometry. */
    request(target: IslandGeometry) {
      desired = target;
      flush();
    },
    /** Re-evaluates a deferred shrink; call whenever the island animation moves. */
    poke() {
      if (shrinkSince !== null) flush();
    },
    /** Forgets what was sent so the current target is sent again (monitor change, DPI change). */
    invalidate() {
      sent = null;
      flush();
    },
    dispose() {
      cancelShrinkTimer();
      desired = null;
    },
  };
}
