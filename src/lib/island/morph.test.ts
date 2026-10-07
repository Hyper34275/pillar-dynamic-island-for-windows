import { describe, expect, it } from "vitest";
import { arrivalFade, layerFade, partFade } from "../../components/Pill/animations";
import { tabFade } from "../../components/Pill/tabLayers";
import { toastPayloadFade } from "../../components/Pill/toastHandoff";
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
    const fade = tabFade;
    const midway: LayerFrame = { opacity: 0.4, offset: -5 };
    // caught while leaving, then chosen again: rises at once from 0.4
    expect(layerFrame(fade, true, midway, 0, 0)).toEqual(midway);
    expect(opacityAt(fade, true, midway, 0.05)).toBeGreaterThan(0.4);
    // caught while entering, then replaced: falls from 0.4 and slides out
    expect(layerFrame(fade, false, midway, 8, 0)).toEqual(midway);
    expect(layerFrame(fade, false, midway, 8, fade.out)).toEqual({ opacity: 0, offset: 8 });
  });

  it("slides entering content in from its offset and leaving content out to the exit offset", () => {
    expect(layerFrame(tabFade, true, { opacity: 0, offset: 8 }, 0, 0).offset).toBe(8);
    expect(layerFrame(tabFade, true, { opacity: 0, offset: 8 }, 0, 1).offset).toBe(0);
    expect(layerFrame(tabFade, false, SHOWN, -8, 1).offset).toBe(-8);
  });
});

describe("island choreography: one owner at a time", () => {
  // Every hand-over (opening, closing, a toast arriving or leaving, a tab change, a toast replacing
  // a toast) has ONE visual owner: the leaving content is faint before the arriving content is
  // readable (never both above READABLE), and the island is never an empty shape for more than an
  // instant (the most visible of the two stays above FLOOR).
  const READABLE = 0.35;
  const FLOOR = 0.08;
  const parts = Object.values(partFade);
  const expandedAt = (entering: boolean, p: number) =>
    Math.max(...parts.map((fade) => opacityAt(fade, entering, entering ? FRESH : SHOWN, p)));
  const handover = (leaving: (p: number) => number, arriving: (p: number) => number) => {
    const frames = steps(1000).map((p) => [leaving(p), arriving(p)] as const);
    return {
      floor: Math.min(...frames.map(([a, b]) => Math.max(a, b))),
      bothReadable: frames.filter(([a, b]) => a > READABLE && b > READABLE).length,
      maxOverlap: Math.max(...frames.map(([a, b]) => Math.min(a, b))),
    };
  };
  const leave = (fade: Fade) => (p: number) => opacityAt(fade, false, SHOWN, p);
  const arrive = (fade: Fade) => (p: number) => opacityAt(fade, true, FRESH, p);

  it.each([
    ["opening the panel", leave(layerFade.compact), (p: number) => expandedAt(true, p)],
    ["closing the panel", (p: number) => expandedAt(false, p), arrive(arrivalFade("compact", "expanded"))],
    ["a toast opening from the pill", leave(layerFade.compact), arrive(arrivalFade("temporary", "compact"))],
    ["a toast closing to the pill", leave(layerFade.temporary), arrive(arrivalFade("compact", "temporary"))],
    ["a meeting alert over the panel", (p: number) => expandedAt(false, p), arrive(arrivalFade("temporary", "expanded"))],
    ["a tab change", leave(tabFade), arrive(tabFade)],
    ["a toast replacing a toast (payload)", leave(toastPayloadFade), arrive(toastPayloadFade)],
  ] as const)("%s: never two readable owners, never an empty shape", (_name, leaving, arriving) => {
    const result = handover(leaving, arriving);
    expect(result.bothReadable).toBe(0);
    expect(result.floor).toBeGreaterThan(FLOOR);
    expect(result.maxOverlap).toBeLessThan(READABLE);
  });

  it("removes the expanded layer only once its slowest part has gone", () => {
    expect(layerFade.expanded.out).toBeGreaterThanOrEqual(Math.max(...parts.map((fade) => fade.out)));
  });

  it("opening: space first, then the header, the body, and the dock last", () => {
    expect(opacityAt(partFade.body, true, FRESH, 0.3)).toBe(0);
    expect(opacityAt(partFade.body, true, FRESH, 0.7)).toBeGreaterThan(0.6);
    expect(partFade.header.in[0]).toBeLessThan(partFade.body.in[0]);
    expect(partFade.dock.in[0]).toBeGreaterThanOrEqual(partFade.body.in[0]);
    // the compact content is gone before the shape is far from a pill
    expect(opacityAt(layerFade.compact, false, SHOWN, 0.25)).toBe(0);
  });

  it("closing: the header leaves the top row first, the dock covers the body's cut, the body holds, and the compact content waits for a small shape", () => {
    expect(partFade.header.out).toBeLessThan(partFade.dock.out);
    expect(partFade.dock.out).toBeLessThan(partFade.body.out);
    // no compact date floating in a shape much larger than the pill (the last ~20 % of the morph only)
    expect(opacityAt(arrivalFade("compact", "expanded"), true, FRESH, 0.78)).toBe(0);
    expect(opacityAt(partFade.body, false, SHOWN, 0.6)).toBeGreaterThan(0.4);
  });
});
