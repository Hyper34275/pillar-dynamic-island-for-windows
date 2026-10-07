// The island's shape: the one owner of its width, height and corner radius.
//
// One target (the shape the island should have now), one frame loop, two springs (width and
// height) and a radius derived from them. Nothing else writes the island's geometry: the view
// decides the target, this converges to it, the page and the window region read from it.
//
//  - Retargeting is last-writer-wins and keeps position and velocity: open -> close -> open
//    bends the same motion around instead of queueing three animations or restarting one.
//  - The springs are solved in closed form per frame (spring.ts), so the shape is exact at any
//    frame rate and a late frame lands where it should be at that instant.
//  - The geometry never overshoots its target: each axis remembers which side of the target
//    it started on and stops at the target rather than crossing it (an over-shrinking window
//    is broken geometry, not elasticity). It also never leaves the hard bounds (the boot dot
//    and the stage window), and every frame is checked against these invariants.
//  - The corner radius is not animated on its own: it moves from where it was to the target's
//    by how far the size has travelled, and never exceeds half the shorter side, so the
//    silhouette always matches the size it is drawn at.

import { springAtRest, stepSpring, type SpringParams, type SpringState } from "./spring";

export interface IslandShape {
  width: number;
  height: number;
  radius: number;
}

export interface ShapeBounds {
  minWidth: number;
  minHeight: number;
  maxWidth: number;
  maxHeight: number;
}

export interface IslandFrame extends IslandShape {
  /** ms, the frame's timestamp. */
  time: number;
  /** px per second. */
  widthVelocity: number;
  heightVelocity: number;
  /** Where the current transition began (what was on screen) and where it goes. */
  from: IslandShape;
  target: IslandShape;
  /** 0 when the transition began, 1 once the size has arrived. */
  progress: number;
  settled: boolean;
  /** Increments with every new target: tells transitions apart in traces. */
  generation: number;
}

export interface FrameScheduler {
  /** ms */
  now(): number;
  /** Calls `callback` with the frame's timestamp (ms) on the next frame. */
  request(callback: (timestamp: number) => void): void;
  cancel(callback: (timestamp: number) => void): void;
}

export interface IslandMotionOptions {
  initial: IslandShape;
  bounds: ShapeBounds;
  springs: { width: SpringParams; height: SpringParams };
  scheduler: FrameScheduler;
  onFrame: (frame: IslandFrame) => void;
  /** An invariant broke (a bug): reported once per transition, never thrown at the user. */
  onViolation?: (message: string, frame: IslandFrame) => void;
}

/** Below this (px, and px per 60 Hz frame) the island is at rest; the final snap is invisible. */
export const REST_EPSILON_PX = 0.05;
/** Rounding slack for the invariant checks (px). */
const TOLERANCE_PX = 0.5;

const clamp = (n: number, lo: number, hi: number) => (n < lo ? lo : n > hi ? hi : n);
const clamp01 = (n: number) => clamp(n, 0, 1);
const sign = (n: number) => (n > 0 ? 1 : n < 0 ? -1 : 0);

interface Axis {
  state: SpringState;
  /** Side of the target the axis started this transition on: it may reach the target, never cross it. */
  side: -1 | 0 | 1;
}

/** Geometric progress from `from` to `target` at size (w, h): 0 at the start, 1 on arrival. */
export function shapeProgress(from: IslandShape, target: IslandShape, width: number, height: number): number {
  const total = Math.abs(target.width - from.width) + Math.abs(target.height - from.height);
  if (total < 1e-6) return 1;
  const left = Math.abs(target.width - width) + Math.abs(target.height - height);
  return clamp01(1 - left / total);
}

/** The corner radius that belongs to size (w, h) on the way from `from` to `target`. */
export function radiusAt(from: IslandShape, target: IslandShape, width: number, height: number): number {
  const p = shapeProgress(from, target, width, height);
  const radius = from.radius + (target.radius - from.radius) * p;
  return clamp(radius, 0, Math.min(width, height) / 2);
}

/** What is wrong with a frame, if anything (an empty list for a valid one). */
export function frameViolations(frame: IslandFrame, bounds: ShapeBounds): string[] {
  const problems: string[] = [];
  const { width, height, radius } = frame;
  if (![width, height, radius, frame.widthVelocity, frame.heightVelocity].every(Number.isFinite)) problems.push("non-finite value");
  if (!(width > 0 && height > 0)) problems.push(`empty size ${width}x${height}`);
  if (width < bounds.minWidth - TOLERANCE_PX || height < bounds.minHeight - TOLERANCE_PX) problems.push(`below the minimum: ${width}x${height}`);
  if (width > bounds.maxWidth + TOLERANCE_PX || height > bounds.maxHeight + TOLERANCE_PX) problems.push(`beyond the stage: ${width}x${height}`);
  if (radius < 0 || radius > Math.min(width, height) / 2 + TOLERANCE_PX) problems.push(`radius ${radius} for ${width}x${height}`);
  return problems;
}

export function createIslandMotion(options: IslandMotionOptions) {
  const { bounds, springs, scheduler, onFrame, onViolation } = options;
  const start = { ...options.initial };
  const width: Axis = { state: { value: start.width, velocity: 0 }, side: 0 };
  const height: Axis = { state: { value: start.height, velocity: 0 }, side: 0 };
  let target: IslandShape = start;
  let from: IslandShape = start;
  let radius = start.radius;
  let last = scheduler.now();
  let running = false;
  let generation = 0;
  let reported = -1;

  const frame = (time: number, settled: boolean): IslandFrame => ({
    time,
    width: width.state.value,
    height: height.state.value,
    radius,
    widthVelocity: width.state.velocity,
    heightVelocity: height.state.velocity,
    from,
    target,
    progress: shapeProgress(from, target, width.state.value, height.state.value),
    settled,
    generation,
  });

  function emit(f: IslandFrame) {
    const problems = frameViolations(f, bounds);
    for (const [axis, value, goal] of [[width, f.width, target.width], [height, f.height, target.height]] as const) {
      if (axis.side !== 0 && sign(value - goal) === -axis.side) problems.push(`overshoot past ${goal}: ${value}`);
    }
    if (problems.length > 0 && reported !== generation) {
      reported = generation;
      onViolation?.(problems.join("; "), f);
    }
    onFrame(f);
  }

  function advance(axis: Axis, goal: number, params: SpringParams, dt: number, lo: number, hi: number) {
    let next = stepSpring(axis.state, goal, params, dt);
    // Reaching the target ends the approach: never cross it.
    if (axis.side !== 0 && sign(next.value - goal) === -axis.side) next = { value: goal, velocity: 0 };
    // Never leave the hard bounds; hitting one stops the motion along it.
    if (next.value < lo || next.value > hi) next = { value: clamp(next.value, lo, hi), velocity: 0 };
    axis.state = next;
    if (next.value === goal) axis.side = 0;
  }

  function tick(timestamp: number) {
    running = false;
    const dt = Math.max(0, (timestamp - last) / 1000);
    last = Math.max(last, timestamp);
    advance(width, target.width, springs.width, dt, bounds.minWidth, bounds.maxWidth);
    advance(height, target.height, springs.height, dt, bounds.minHeight, bounds.maxHeight);
    const settled =
      springAtRest(width.state, target.width, REST_EPSILON_PX) && springAtRest(height.state, target.height, REST_EPSILON_PX);
    if (settled) {
      width.state = { value: target.width, velocity: 0 };
      height.state = { value: target.height, velocity: 0 };
      width.side = height.side = 0;
      radius = target.radius;
    } else {
      radius = radiusAt(from, target, width.state.value, height.state.value);
      running = true;
      scheduler.request(tick);
    }
    emit(frame(timestamp, settled));
  }

  function clampShape(shape: IslandShape): IslandShape {
    const w = clamp(shape.width, bounds.minWidth, bounds.maxWidth);
    const h = clamp(shape.height, bounds.minHeight, bounds.maxHeight);
    return { width: w, height: h, radius: clamp(shape.radius, 0, Math.min(w, h) / 2) };
  }

  return {
    /**
     * The island should now have `shape`. The motion continues from what is on screen (size,
     * velocity, radius) towards it; the first frame is computed from this instant, so the
     * response starts on the very next frame.
     */
    setTarget(shape: IslandShape) {
      const next = clampShape(shape);
      if (next.width === target.width && next.height === target.height && next.radius === target.radius) {
        // Same target: keep going, or pick up again if the loop was stopped (dispose) mid-way.
        if (!running && (width.state.value !== target.width || height.state.value !== target.height)) {
          last = scheduler.now();
          running = true;
          scheduler.request(tick);
        }
        return;
      }
      generation++;
      from = { width: width.state.value, height: height.state.value, radius };
      target = next;
      for (const [axis, goal] of [[width, next.width], [height, next.height]] as const) {
        axis.side = sign(axis.state.value - goal);
        // Already there: any leftover velocity could only carry it past the target.
        if (axis.side === 0) axis.state = { value: goal, velocity: 0 };
      }
      if (!running) {
        last = scheduler.now();
        running = true;
        scheduler.request(tick);
      }
    },
    /** Goes to `shape` at once (no motion): boot, display change, tests. */
    jump(shape: IslandShape) {
      if (running) scheduler.cancel(tick);
      running = false;
      generation++;
      target = from = clampShape(shape);
      width.state = { value: target.width, velocity: 0 };
      height.state = { value: target.height, velocity: 0 };
      width.side = height.side = 0;
      radius = target.radius;
      last = scheduler.now();
      emit(frame(last, true));
    },
    current(): IslandFrame {
      return frame(last, !running);
    },
    dispose() {
      if (running) scheduler.cancel(tick);
      running = false;
    },
  };
}

export type IslandMotion = ReturnType<typeof createIslandMotion>;
