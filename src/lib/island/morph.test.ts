import { describe, expect, it } from "vitest";
import { layerFade } from "../../components/Pill/animations";
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
});

describe("layerFrame", () => {
  it("keeps a new layer invisible until its window opens, then makes it fully visible by the end of it", () => {
    const fade = layerFade.expanded;
    expect(opacityAt(fade, true, FRESH, 0)).toBe(0);
    expect(opacityAt(fade, true, FRESH, fade.in[0])).toBe(0);
    expect(opacityAt(fade, true, FRESH, fade.in[1])).toBe(1);
    expect(opacityAt(fade, true, FRESH, 1)).toBe(1);
  });

  it("fades a leaving layer to exactly 0 by its window's end, so it can be removed", () => {
    for (const fade of Object.values(layerFade)) {
      expect(opacityAt(fade, false, SHOWN, 0)).toBe(1);
      expect(opacityAt(fade, false, SHOWN, fade.out)).toBe(0);
      expect(opacityAt(fade, false, SHOWN, 1)).toBe(0);
    }
  });

  it("is monotonic: entering only ever rises, leaving only ever falls", () => {
    for (const fade of Object.values(layerFade)) {
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
  // The most visible of the two layers at every point of the transition: the island is never
  // an empty shape, in either direction.
  const floor = (leaving: Fade, entering: Fade) =>
    Math.min(...steps().map((p) => Math.max(opacityAt(leaving, false, SHOWN, p), opacityAt(entering, true, FRESH, p))));

  it("never empties the island while opening or closing", () => {
    expect(floor(layerFade.compact, layerFade.expanded)).toBeGreaterThan(0.25);
    expect(floor(layerFade.expanded, layerFade.compact)).toBeGreaterThan(0.25);
  });

  it("never leaves the tab content area empty while switching tabs", () => {
    expect(floor(layerFade.tab, layerFade.tab)).toBeGreaterThan(0.25);
  });

  it("shows the expanded panel only once the island has grown into it, and has it readable well before the end", () => {
    expect(opacityAt(layerFade.expanded, true, FRESH, 0.2)).toBe(0);
    expect(opacityAt(layerFade.expanded, true, FRESH, 0.5)).toBeGreaterThan(0.6);
  });

  it("keeps the expanded panel on screen through most of a close instead of dropping it at the start", () => {
    expect(opacityAt(layerFade.expanded, false, SHOWN, 0.3)).toBeGreaterThan(0.8);
  });
});
