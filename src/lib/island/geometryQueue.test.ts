import { describe, expect, it, vi } from "vitest";
import type { IslandGeometry } from "../ipc";
import { islandSprings } from "../../components/Pill/animations";
import { createGeometryQueue, FIT_SLACK_PX, islandFits } from "./geometryQueue";
import { createIslandMotion } from "./islandMotion";

const size = (width: number, height: number): IslandGeometry => ({ width, height });

function harness(options: { fits?: () => boolean } = {}) {
  let clock = 0;
  const timers: Array<{ at: number; fn: () => void }> = [];
  const resolvers: Array<(ok: boolean) => void> = [];
  const sent: IslandGeometry[] = [];

  const queue = createGeometryQueue({
    send: (geometry) => {
      sent.push(geometry);
      return new Promise<boolean>((resolve) => resolvers.push(resolve));
    },
    fits: options.fits ?? (() => true),
    now: () => clock,
    setTimer: (fn, ms) => {
      const handle = { at: clock + ms, fn };
      timers.push(handle);
      return handle as unknown as ReturnType<typeof setTimeout>;
    },
    clearTimer: (handle) => {
      const i = timers.indexOf(handle as unknown as { at: number; fn: () => void });
      if (i >= 0) timers.splice(i, 1);
    },
  });

  return {
    queue,
    sent,
    timers,
    /** Completes the oldest in-flight send. */
    async complete(ok = true) {
      resolvers.shift()?.(ok);
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    },
    advance(ms: number) {
      clock += ms;
      for (const timer of [...timers]) {
        if (timer.at <= clock) {
          timers.splice(timers.indexOf(timer), 1);
          timer.fn();
        }
      }
    },
  };
}

describe("geometry queue", () => {
  it("sends the first request immediately", () => {
    const h = harness();
    h.queue.request(size(130, 40));
    expect(h.sent).toEqual([size(130, 40)]);
  });

  it("never re-sends a size that has not changed", async () => {
    const h = harness();
    h.queue.request(size(130, 40));
    await h.complete();
    h.queue.request(size(130, 40));
    h.queue.request(size(130, 40));
    expect(h.sent).toHaveLength(1);
  });

  it("is latest-wins: requests made while one is in flight collapse into the newest", async () => {
    const h = harness();
    h.queue.request(size(130, 40));
    h.queue.request(size(300, 100)); // superseded
    h.queue.request(size(420, 430));
    expect(h.sent).toEqual([size(130, 40)]);
    await h.complete();
    expect(h.sent).toEqual([size(130, 40), size(420, 430)]);
  });

  it("keeps requests strictly ordered: one in flight at a time", async () => {
    const h = harness();
    h.queue.request(size(130, 40));
    h.queue.request(size(420, 430));
    expect(h.sent).toHaveLength(1);
    await h.complete();
    expect(h.sent).toHaveLength(2);
    h.queue.request(size(130, 40));
    expect(h.sent).toHaveLength(2); // second is still in flight
    await h.complete();
    expect(h.sent).toHaveLength(3);
  });

  it("grows immediately even while the island animation is mid-flight", async () => {
    const h = harness({ fits: () => false });
    h.queue.request(size(130, 40));
    await h.complete();
    h.queue.request(size(420, 430));
    expect(h.sent).toHaveLength(2);
  });

  it("holds a shrink until the island fits, then sends it on the next poke", async () => {
    let fits = false;
    const h = harness({ fits: () => fits });
    h.queue.request(size(420, 430));
    await h.complete();

    h.queue.request(size(130, 40));
    expect(h.sent).toHaveLength(1); // deferred
    h.queue.poke();
    expect(h.sent).toHaveLength(1); // spring still too big

    fits = true;
    h.queue.poke();
    expect(h.sent).toEqual([size(420, 430), size(130, 40)]);
  });

  it("gives up waiting for the spring after the maximum delay", async () => {
    const h = harness({ fits: () => false });
    h.queue.request(size(420, 430));
    await h.complete();

    h.queue.request(size(130, 40));
    expect(h.sent).toHaveLength(1);
    h.advance(699);
    expect(h.sent).toHaveLength(1);
    h.advance(2);
    expect(h.sent).toHaveLength(2);
  });

  it("cancels a pending shrink when the island grows again", async () => {
    const h = harness({ fits: () => false });
    h.queue.request(size(420, 430));
    await h.complete();
    h.queue.request(size(130, 40)); // deferred
    h.queue.request(size(420, 430)); // back to what was sent: nothing to do
    h.advance(1000);
    expect(h.sent).toHaveLength(1);
    expect(h.timers).toHaveLength(0);
  });

  it("retries a failed resize on the next request instead of treating it as sent", async () => {
    const h = harness();
    h.queue.request(size(130, 40));
    await h.complete(false);
    expect(h.sent).toHaveLength(1);
    h.queue.request(size(130, 40));
    expect(h.sent).toHaveLength(2);
  });

  it("invalidate forces the current target to be sent again", async () => {
    const h = harness();
    h.queue.request(size(130, 40));
    await h.complete();
    h.queue.invalidate();
    expect(h.sent).toHaveLength(2);
  });

  it("does nothing after dispose", () => {
    const send = vi.fn(() => Promise.resolve(true));
    const queue = createGeometryQueue({ send, fits: () => true });
    queue.dispose();
    queue.poke();
    expect(send).not.toHaveBeenCalled();
  });
});

describe("islandFits", () => {
  it("accepts an island resting at exactly the target size (slack is a tolerance, not a margin)", () => {
    expect(islandFits({ width: 200, height: 34 }, size(200, 34))).toBe(true);
    expect(islandFits({ width: 200 + FIT_SLACK_PX, height: 34 }, size(200, 34))).toBe(true);
  });

  it("rejects an island that still sticks out of the smaller window", () => {
    expect(islandFits({ width: 200 + FIT_SLACK_PX + 0.5, height: 34 }, size(200, 34))).toBe(false);
    expect(islandFits({ width: 200, height: 34 + FIT_SLACK_PX + 0.5 }, size(200, 34))).toBe(false);
    expect(islandFits({ width: 404, height: 420 }, size(200, 34))).toBe(false);
  });

  it("is satisfied well before the shrink deadline when the real island motion collapses", () => {
    // The island's own engine and springs (animations.ts islandSprings), at 60 Hz.
    const target = { width: 200, height: 34, radius: 17 };
    let now = 0;
    let queued: ((t: number) => void)[] = [];
    let fitsAtMs = -1;
    const motion = createIslandMotion({
      initial: { width: 404, height: 420, radius: 40 },
      bounds: { minWidth: 8, minHeight: 8, maxWidth: 404, maxHeight: 420 },
      springs: islandSprings,
      scheduler: { now: () => now, request: (cb) => queued.push(cb), cancel: () => {} },
      onFrame: (f) => {
        if (fitsAtMs < 0 && islandFits(f, size(200, 34))) fitsAtMs = f.time;
      },
    });
    motion.setTarget(target);
    for (let i = 0; i < 120 && queued.length > 0; i++) {
      now += 1000 / 60;
      const run = queued;
      queued = [];
      run.forEach((cb) => cb(now));
    }
    expect(fitsAtMs).toBeGreaterThan(0);
    expect(fitsAtMs).toBeLessThan(500); // the deadline (700 ms) is never what shrinks the region
  });
});
