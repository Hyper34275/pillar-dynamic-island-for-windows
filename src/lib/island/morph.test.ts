import { describe, expect, it } from "vitest";
import { layerFade, partFade } from "../../components/Pill/animations";
import { layerFrame, transitionDirection, transitionProgress, type Fade, type LayerFrame } from "./morph";

const FRESH: LayerFrame = { opacity: 0, offset: 0 };
const SHOWN: LayerFrame = { opacity: 1, offset: 0 };
const steps = (n = 200) => Array.from({ length: n + 1 }, (_, i) => i / n);
const opacityAt = (fade: Fade, entering: boolean, start: LayerFrame, p: number) => layerFrame(fade, entering, start, 0, p).opacity;

describe("transitionProgress", () => {
  const open = { from: [135, 34], to: [404, 420] };

  it("goes from 0 where the driver starts to 1 where it arrives", () => {
    expect(transitionProgress(open, [135, 34])).toBe(0);
    expect(transitionProgress(open, [404, 420])).toBe(1);
    expect(transitionProgress(open, [269.5, 227])).toBeCloseTo(0.5);
  });

  it("stays within 0..1 for overshoot and for momentum carried away from a new target", () => {
    expect(transitionProgress(open, [404.6, 420.8])).toBeGreaterThan(0.99);
    expect(transitionProgress(open, [404.6, 420.8])).toBeLessThanOrEqual(1);
    expect(transitionProgress(open, [120, 20])).toBe(0);
  });

  it("is complete at once for a transition that goes nowhere (e.g. one alert replacing a same-sized one)", () => {
    expect(transitionProgress({ from: [380, 104], to: [380, 104] }, [380, 104])).toBe(1);
  });

  it("knows which way the driver travels", () => {
    expect(transitionDirection({ from: [0], to: [2] })).toBe(1);
    expect(transitionDirection({ from: [2], to: [1] })).toBe(-1);
    expect(transitionDirection({ from: [1], to: [1] })).toBe(0);
  });

  it("uses an explicit direction over the drivers' (a progress that only counts forwards)", () => {
    expect(transitionDirection({ from: [3], to: [4], direction: -1 })).toBe(-1);
    expect(transitionProgress({ from: [3], to: [4], direction: -1 }, [3.5])).toBeCloseTo(0.5);
  });
});

describe("layerFrame", () => {
  it("keeps a new layer invisible until its window opens, then makes it fully visible by the end of it", () => {
    const fade = partFade.body;
    expect(opacityAt(fade, true, FRESH, 0)).toBe(0);
    expect(opacityAt(fade, true, FRESH, fade.in[0])).toBe(0);
    expect(opacityAt(fade, true, FRESH, fade.in[1])).toBe(1);
    expect(opacityAt(fade, true, FRESH, 1)).toBe(1);
  });

  it("fades a leaving layer to exactly 0 by its window's end, so it can be removed", () => {
    for (const fade of [...Object.values(layerFade), ...Object.values(partFade)]) {
      expect(opacityAt(fade, false, SHOWN, 0)).toBe(1);
      expect(opacityAt(fade, false, SHOWN, fade.out)).toBe(0);
      expect(opacityAt(fade, false, SHOWN, 1)).toBe(0);
    }
  });

  it("is monotonic: entering only ever rises, leaving only ever falls", () => {
    for (const fade of [...Object.values(layerFade), ...Object.values(partFade)]) {
      let rising = 0;
      let falling = 1;
      for (const p of steps()) {
        const up = opacityAt(fade, true, FRESH, p);
        const down = opacityAt(fade, false, SHOWN, p);
        expect(up).toBeGreaterThanOrEqual(rising);
        expect(down).toBeLessThanOrEqual(falling);
        rising = up;
        falling = down;
      }
    }
  });

  it("continues an interrupted layer from what it showed, without a jump", () => {
    const fade = layerFade.tab;
    const midway: LayerFrame = { opacity: 0.4, offset: -5 };
    // caught while leaving, then chosen again: rises at once from 0.4
    expect(layerFrame(fade, true, midway, 0, 0)).toEqual(midway);
    expect(opacityAt(fade, true, midway, 0.05)).toBeGreaterThan(0.4);
    // caught while entering, then replaced: falls from 0.4 and slides out
    expect(layerFrame(fade, false, midway, 8, 0)).toEqual(midway);
    expect(layerFrame(fade, false, midway, 8, fade.out)).toEqual({ opacity: 0, offset: 8 });
  });

  it("slides entering content in from its offset and leaving content out to the exit offset", () => {
    expect(layerFrame(layerFade.tab, true, { opacity: 0, offset: 8 }, 0, 0).offset).toBe(8);
    expect(layerFrame(layerFade.tab, true, { opacity: 0, offset: 8 }, 0, 1).offset).toBe(0);
    expect(layerFrame(layerFade.tab, false, SHOWN, -8, 1).offset).toBe(-8);
  });
});

describe("island choreography", () => {
  // The most visible thing on the island at every point of the transition: the island is never
  // an empty shape, in either direction. The expanded island is its parts (header, body, dock).
  const parts = Object.values(partFade);
  const expandedAt = (entering: boolean, p: number) =>
    Math.max(...parts.map((fade) => opacityAt(fade, entering, entering ? FRESH : SHOWN, p)));
  const opening = (p: number) => Math.max(opacityAt(layerFade.compact, false, SHOWN, p), expandedAt(true, p));
  const closing = (p: number) => Math.max(expandedAt(false, p), opacityAt(layerFade.compact, true, FRESH, p));

  it("never empties the island while opening or closing", () => {
    expect(Math.min(...steps().map(opening))).toBeGreaterThan(0.25);
    expect(Math.min(...steps().map(closing))).toBeGreaterThan(0.25);
  });

  it("never leaves the tab content area empty while switching tabs", () => {
    const floor = Math.min(...steps().map((p) => Math.max(opacityAt(layerFade.tab, false, SHOWN, p), opacityAt(layerFade.tab, true, FRESH, p))));
    expect(floor).toBeGreaterThan(0.25);
  });

  it("removes the expanded layer only once its slowest part has gone", () => {
    expect(layerFade.expanded.out).toBe(Math.max(...parts.map((fade) => fade.out)));
  });

  it("opening: space first, then the header, the body, and the dock last", () => {
    expect(opacityAt(partFade.body, true, FRESH, 0.3)).toBe(0);
    expect(opacityAt(partFade.body, true, FRESH, 0.65)).toBeGreaterThan(0.6);
    expect(partFade.header.in[0]).toBeLessThan(partFade.body.in[0]);
    expect(partFade.dock.in[1]).toBeGreaterThanOrEqual(partFade.body.in[1]);
    // the compact content is gone before the shape is far from a pill
    expect(opacityAt(layerFade.compact, false, SHOWN, 0.3)).toBe(0);
  });

  it("closing: the dock goes first, the body holds through most of it, and the compact content waits for a small shape", () => {
    expect(partFade.dock.out).toBeLessThan(partFade.body.out);
    expect(opacityAt(partFade.body, false, SHOWN, 0.25)).toBeGreaterThan(0.8);
    // no compact date floating in a shape much larger than the pill
    expect(opacityAt(layerFade.compact, true, FRESH, 0.42)).toBe(0);
  });
});
