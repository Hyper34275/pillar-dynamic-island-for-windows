// Driven transitions: one thing moves, everything else is read off it.
//
// Every visible change of the island is one transition with one driver: the island's animated
// size when it opens, closes or shows an alert, the dock indicator's position when the tab
// changes. Which content is visible, and how much, is a pure function of how far that driver
// has travelled. Content therefore cannot run ahead of or lag behind the container (both are
// read from the same values in the same frame), there is no timer or delay to race, and an
// interrupted transition simply starts a new one from wherever things are on screen.

export interface Transition {
  /** Driver values when the transition began (the visual state at that moment). */
  readonly from: readonly number[];
  /** Driver values it is heading to. */
  readonly to: readonly number[];
  /**
   * Which way content travels, when it is not the way the drivers go (the tab content's
   * progress only ever counts forwards; the direction is the tab change's).
   */
  readonly direction?: -1 | 0 | 1;
  /**
   * How the drivers' progress combines. "sum" (default): the distance left over all drivers
   * together (the island's width and height are one morph). "min": the least advanced driver that
   * moves at all, each normalised on its own (a toast's payload handoff, whose own clock must
   * wait for the shell: new content never reads ahead of the shape that has to hold it).
   */
  readonly combine?: "sum" | "min";
}

const clamp01 = (n: number) => (n < 0 ? 0 : n > 1 ? 1 : n);
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const span = (p: number, start: number, end: number) => (end <= start ? (p >= end ? 1 : 0) : clamp01((p - start) / (end - start)));
// Entering content becomes readable early; leaving content stays readable, then goes quickly.
// Either way it spends little time as a faint ghost.
const easeOut = (t: number) => 1 - (1 - t) * (1 - t);
const easeIn = (t: number) => t * t;

/**
 * 0 when the transition begins, 1 once its driver has arrived (never outside that range: a
 * driver still carrying momentum away from a new target reads as 0). A transition that goes
 * nowhere is complete at once.
 */
export function transitionProgress(transition: Transition, current: readonly number[]): number {
  if (transition.combine === "min") {
    let least = 1;
    for (let i = 0; i < transition.to.length; i++) {
      const distance = Math.abs(transition.to[i] - transition.from[i]);
      if (distance < 1e-6) continue;
      least = Math.min(least, clamp01(1 - Math.abs(transition.to[i] - (current[i] ?? transition.to[i])) / distance));
    }
    return least;
  }
  let total = 0;
  let left = 0;
  for (let i = 0; i < transition.to.length; i++) {
    total += Math.abs(transition.to[i] - transition.from[i]);
    left += Math.abs(transition.to[i] - (current[i] ?? transition.to[i]));
  }
  return total < 1e-6 ? 1 : clamp01(1 - left / total);
}

/** Which way the driver travels: +1, -1, or 0 for a transition that goes nowhere. */
export function transitionDirection(transition: Transition): -1 | 0 | 1 {
  if (transition.direction !== undefined) return transition.direction;
  let sum = 0;
  for (let i = 0; i < transition.to.length; i++) sum += transition.to[i] - transition.from[i];
  return sum > 0 ? 1 : sum < 0 ? -1 : 0;
}

/** Where, in a transition's progress, a layer fades in (`in`: from, to) and by when it is gone (`out`). */
export interface Fade {
  readonly in: readonly [number, number];
  readonly out: number;
}

/** What a layer shows: its opacity and its offset (px, along the direction of travel). */
export interface LayerFrame {
  opacity: number;
  offset: number;
}

/**
 * A layer's frame at progress `p`. `start` is what it showed when the transition began, so a
 * layer caught mid-fade by a new transition continues from there instead of jumping. Entering
 * layers settle at fully visible and offset 0; leaving ones fade to nothing while moving to
 * `exitOffset`. A layer that is already partly visible (it was leaving and came back) starts
 * rising at once rather than waiting for its fade-in window.
 */
export function layerFrame(fade: Fade, entering: boolean, start: LayerFrame, exitOffset: number, p: number): LayerFrame {
  if (entering) {
    const t = easeOut(span(p, start.opacity > 0 ? 0 : fade.in[0], fade.in[1]));
    return { opacity: lerp(start.opacity, 1, t), offset: lerp(start.offset, 0, t) };
  }
  const t = span(p, 0, fade.out);
  return { opacity: start.opacity * (1 - easeIn(t)), offset: lerp(start.offset, exitOffset, easeOut(t)) };
}
