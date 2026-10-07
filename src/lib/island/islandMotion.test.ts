import { describe, expect, it } from "vitest";
import { islandSprings } from "../../components/Pill/animations";
import { createIslandMotion, frameViolations, radiusAt, type IslandFrame, type IslandShape, type ShapeBounds } from "./islandMotion";

const nth = <T,>(list: readonly T[], fromEnd: number): T => list[list.length - fromEnd];

const COMPACT: IslandShape = { width: 142, height: 34, radius: 17 };
const EXPANDED: IslandShape = { width: 404, height: 420, radius: 40 };
const ALERT: IslandShape = { width: 380, height: 131, radius: 30 };
const BOUNDS: ShapeBounds = { minWidth: 8, minHeight: 8, maxWidth: 404, maxHeight: 420 };
const SPRINGS = islandSprings;

/** A frame clock the test advances by hand (any frame rate). */
function harness(initial: IslandShape = COMPACT) {
  let now = 0;
  let queued: ((t: number) => void)[] = [];
  const frames: IslandFrame[] = [];
  const violations: string[] = [];
  const motion = createIslandMotion({
    initial,
    bounds: BOUNDS,
    springs: SPRINGS,
    scheduler: {
      now: () => now,
      request: (cb) => queued.push(cb),
      cancel: (cb) => (queued = queued.filter((q) => q !== cb)),
    },
    onFrame: (f) => frames.push(f),
    onViolation: (m) => violations.push(m),
  });
  const step = (ms: number, count = 1) => {
    for (let i = 0; i < count; i++) {
      now += ms;
      const run = queued;
      queued = [];
      run.forEach((cb) => cb(now));
    }
  };
  const settle = (ms = 1000 / 60) => {
    for (let i = 0; i < 600 && queued.length > 0; i++) step(ms);
  };
  return { motion, frames, violations, step, settle, pending: () => queued.length };
}

describe("island motion", () => {
  it("collapses without ever going below the compact size (the 12 px sliver can't happen)", () => {
    for (const hz of [30, 32, 60, 120, 144]) {
      const h = harness(EXPANDED);
      h.motion.setTarget(COMPACT);
      h.settle(1000 / hz);
      expect(h.violations).toEqual([]);
      for (const f of h.frames) {
        expect(f.width).toBeGreaterThanOrEqual(COMPACT.width);
        expect(f.height).toBeGreaterThanOrEqual(COMPACT.height);
        expect(f.width).toBeLessThanOrEqual(EXPANDED.width);
      }
      // monotonic shrink from rest
      for (let i = 1; i < h.frames.length; i++) {
        expect(h.frames[i].width).toBeLessThanOrEqual(h.frames[i - 1].width + 1e-9);
        expect(h.frames[i].height).toBeLessThanOrEqual(h.frames[i - 1].height + 1e-9);
      }
      expect(nth(h.frames, 1)).toMatchObject({ ...COMPACT, settled: true });
    }
  });

  it("moves on the first frame after the request and is within a pixel of the target by ~0.45 s", () => {
    const h = harness();
    h.motion.setTarget(EXPANDED);
    h.step(1000 / 60);
    expect(h.frames[0].width).toBeGreaterThan(COMPACT.width + 1);
    h.settle();
    const within = h.frames.findIndex((f) => EXPANDED.width - f.width < 1 && EXPANDED.height - f.height < 1);
    expect((within + 1) / 60).toBeLessThan(0.45);
    // the rest is a sub-pixel tail, then the loop stops on its own
    expect(h.frames.length / 60).toBeLessThan(0.8);
  });

  it("lands without a visible snap: the last step is a fraction of a pixel, no larger than the motion before it", () => {
    for (const [from, to] of [[COMPACT, EXPANDED], [EXPANDED, COMPACT], [COMPACT, ALERT]]) {
      const h = harness(from);
      h.motion.setTarget(to);
      h.settle();
      const [a, b, c] = [nth(h.frames, 3), nth(h.frames, 2), nth(h.frames, 1)];
      const lastStep = Math.abs(c.width - b.width) + Math.abs(c.height - b.height);
      const stepBefore = Math.abs(b.width - a.width) + Math.abs(b.height - a.height);
      expect(lastStep).toBeLessThan(0.5);
      expect(lastStep).toBeLessThanOrEqual(stepBefore + 0.05);
    }
  });

  it("a reversed open bends back from where it is, keeping its velocity (no restart, no jump)", () => {
    const h = harness();
    h.motion.setTarget(EXPANDED);
    h.step(1000 / 60, 6);
    const caught = nth(h.frames, 1)!;
    h.motion.setTarget(COMPACT);
    h.step(1000 / 60);
    const next = nth(h.frames, 1)!;
    // continuous position: one frame's worth of travel at most
    expect(Math.abs(next.width - caught.width)).toBeLessThan(Math.abs(caught.widthVelocity) / 60 + 1);
    // momentum: it was growing, so it still grows a little before turning
    expect(next.width).toBeGreaterThanOrEqual(caught.width);
    expect(next.from.width).toBeCloseTo(caught.width, 6);
    h.settle();
    expect(h.violations).toEqual([]);
    expect(nth(h.frames, 1)).toMatchObject({ ...COMPACT, settled: true });
  });

  it("open-close-open spam always ends at the last request, never on a stale target", () => {
    const h = harness();
    const targets = [EXPANDED, COMPACT, EXPANDED, COMPACT, ALERT, EXPANDED, COMPACT, EXPANDED];
    for (const t of targets) {
      h.motion.setTarget(t);
      h.step(1000 / 32, 2);
    }
    h.settle(1000 / 32);
    expect(h.violations).toEqual([]);
    expect(nth(h.frames, 1)).toMatchObject({ ...EXPANDED, settled: true });
    for (const f of h.frames) expect(frameViolations(f, BOUNDS)).toEqual([]);
  });

  it("only ever runs one frame loop, however many targets arrive", () => {
    const h = harness();
    h.motion.setTarget(EXPANDED);
    h.motion.setTarget(COMPACT);
    h.motion.setTarget(EXPANDED);
    expect(h.pending()).toBe(1);
  });

  it("the radius follows the size: no snap, never more than half the shorter side", () => {
    const h = harness(EXPANDED);
    h.motion.setTarget(COMPACT);
    h.settle(1000 / 30);
    let previous = EXPANDED.radius;
    for (const f of h.frames) {
      expect(f.radius).toBeLessThanOrEqual(Math.min(f.width, f.height) / 2 + 1e-9);
      expect(Math.abs(f.radius - previous)).toBeLessThan(8);
      previous = f.radius;
    }
    expect(radiusAt(EXPANDED, COMPACT, EXPANDED.width, EXPANDED.height)).toBe(40);
    expect(radiusAt(EXPANDED, COMPACT, COMPACT.width, COMPACT.height)).toBe(17);
  });

  it("clamps targets to the stage and reports broken frames", () => {
    const h = harness();
    h.motion.setTarget({ width: 900, height: 900, radius: 40 });
    h.settle();
    expect(nth(h.frames, 1)).toMatchObject({ width: 404, height: 420 });
    expect(frameViolations({ ...nth(h.frames, 1)!, width: Number.NaN }, BOUNDS)).not.toEqual([]);
    expect(frameViolations({ ...nth(h.frames, 1)!, width: 12, height: 27 }, { ...BOUNDS, minWidth: 100 })).not.toEqual([]);
  });

  it("jump goes there at once and stops any running motion", () => {
    const h = harness();
    h.motion.setTarget(EXPANDED);
    h.step(16);
    h.motion.jump(COMPACT);
    expect(h.pending()).toBe(0);
    expect(nth(h.frames, 1)).toMatchObject({ ...COMPACT, settled: true });
  });
});
