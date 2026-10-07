import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { cancelFrame, frame, frameData, useMotionValue, type MotionValue } from "motion/react";
import { createIslandMotion, type FrameScheduler, type IslandFrame, type IslandShape, type ShapeBounds } from "../../lib/island/islandMotion";
import { springAtRest, stepSpring, type SpringParams, type SpringState } from "../../lib/island/spring";
import { dlog } from "../../lib/debugLog";

const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

/**
 * Frames come from motion's frame loop (its "update" step), so every value computed here and
 * every style bound to it (the island's size, each layer's opacity and offset) is written in
 * the same frame's "render" step: geometry and content can never be a frame apart.
 */
const wrappers = new WeakMap<(timestamp: number) => void, () => void>();
const wrapperOf = (callback: (timestamp: number) => void) => {
  let wrapper = wrappers.get(callback);
  if (!wrapper) {
    wrapper = () => callback(frameData.timestamp);
    wrappers.set(callback, wrapper);
  }
  return wrapper;
};

export const motionFrameScheduler: FrameScheduler = {
  now: () => (frameData.isProcessing ? frameData.timestamp : performance.now()),
  // One wrapper per callback: requesting the same callback twice still runs it once a frame.
  request: (callback) => {
    frame.update(wrapperOf(callback));
  },
  cancel: (callback) => cancelFrame(wrapperOf(callback)),
};

/** Frames of the island's motion, kept for diagnostics when `window.__ISLAND_TRACE__` is an array. */
function trace(f: IslandFrame) {
  const sink = (globalThis as { __ISLAND_TRACE__?: unknown }).__ISLAND_TRACE__;
  if (!Array.isArray(sink)) return;
  sink.push([
    Math.round(f.time * 10) / 10,
    f.generation,
    Math.round(f.width * 100) / 100,
    Math.round(f.height * 100) / 100,
    Math.round(f.radius * 100) / 100,
    Math.round(f.widthVelocity),
    Math.round(f.heightVelocity),
    Math.round(f.progress * 1000) / 1000,
    f.settled ? 1 : 0,
    `${f.target.width}x${f.target.height}`,
  ]);
}

export interface IslandMotionValues {
  width: MotionValue<number>;
  height: MotionValue<number>;
  radius: MotionValue<number>;
  /** Whether the island is at rest at its target. */
  settled: MotionValue<boolean>;
  /** The one engine behind the values (setTarget/jump/current). */
  engine: ReturnType<typeof createIslandMotion>;
}

/**
 * The island's shape, owned by one engine (lib/island/islandMotion.ts). `target` is applied in
 * a layout effect, so the motion starts from the commit that changed what the island shows,
 * before that commit is painted; nothing else may set these values.
 */
export function useIslandMotion(
  target: IslandShape,
  options: { initial: IslandShape; bounds: ShapeBounds; springs: { width: SpringParams; height: SpringParams } }
): IslandMotionValues {
  const width = useMotionValue(options.initial.width);
  const height = useMotionValue(options.initial.height);
  const radius = useMotionValue(options.initial.radius);
  const settled = useMotionValue(true);

  const engineRef = useRef<ReturnType<typeof createIslandMotion> | null>(null);
  if (engineRef.current === null) {
    engineRef.current = createIslandMotion({
      initial: options.initial,
      bounds: options.bounds,
      springs: options.springs,
      scheduler: motionFrameScheduler,
      onFrame: (f) => {
        width.set(f.width);
        height.set(f.height);
        radius.set(f.radius);
        settled.set(f.settled);
        trace(f);
      },
      onViolation: (message, f) => {
        dlog("warn", "pill", `island geometry invariant broken: ${message} (target ${f.target.width}x${f.target.height})`);
        if (import.meta.env.DEV) console.error(`[island] geometry invariant broken: ${message}`, f);
      },
    });
  }
  const engine = engineRef.current;

  const { width: w, height: h, radius: r } = target;
  useIsomorphicLayoutEffect(() => {
    engine.setTarget({ width: w, height: h, radius: r });
    settled.set(engine.current().settled);
  }, [engine, w, h, r, settled]);
  // No cleanup that kills the engine: StrictMode re-runs effects on a live component, and a
  // loop left running after a real unmount only writes to values nobody reads until it settles.
  // Stopping the frame loop on unmount is enough.
  useEffect(() => () => engine.dispose(), [engine]);

  return useMemo(() => ({ width, height, radius, settled, engine }), [width, height, radius, settled, engine]);
}

interface SpringValueOptions {
  /** Stop at the target instead of crossing it (a capsule must not swing into the next tab). */
  noOvershoot?: boolean;
  epsilon?: number;
}

/**
 * A single number on a spring (the tab capsule's slot, the tab content's progress), driven by
 * the same closed-form spring and frame loop as the island. Retargeting keeps the current value
 * and velocity; the first frame is computed from the instant the target changed.
 */
export function useSpringValue(target: number, params: SpringParams, { noOvershoot = false, epsilon = 0.001 }: SpringValueOptions = {}): MotionValue<number> {
  const value = useMotionValue(target);
  const ref = useRef<{ state: SpringState; goal: number; side: number; last: number; running: boolean } | null>(null);
  if (ref.current === null) ref.current = { state: { value: target, velocity: 0 }, goal: target, side: 0, last: 0, running: false };
  const paramsRef = useRef(params);
  paramsRef.current = params;

  useIsomorphicLayoutEffect(() => {
    const s = ref.current!;
    if (s.goal === target) return;
    s.goal = target;
    s.side = Math.sign(s.state.value - target);
    if (s.running) return;
    s.running = true;
    s.last = motionFrameScheduler.now();
    const tick = (timestamp: number) => {
      const dt = Math.max(0, (timestamp - s.last) / 1000);
      s.last = Math.max(s.last, timestamp);
      let next = stepSpring(s.state, s.goal, paramsRef.current, dt);
      if (noOvershoot && s.side !== 0 && Math.sign(next.value - s.goal) === -s.side) next = { value: s.goal, velocity: 0 };
      if (springAtRest(next, s.goal, epsilon)) next = { value: s.goal, velocity: 0 };
      s.state = next;
      value.set(next.value);
      if (next.value === s.goal && next.velocity === 0) s.running = false;
      else motionFrameScheduler.request(tick);
    };
    motionFrameScheduler.request(tick);
  }, [target, value, noOvershoot, epsilon]);

  return value;
}
