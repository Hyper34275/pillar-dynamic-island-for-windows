// The tab content's layers: who is on screen while the tab changes.
//
// A tab change hands the screen from one page to the next. The old design kept every page that
// had ever been shown alive until it had faded, so a burst of clicks stacked layers and the
// outgoing and incoming pages were both readable for a third of the transition (a double
// exposure of two page structures). This manager keeps AT MOST TWO layers, one outgoing and one
// incoming, and gives the screen to one of them at a time:
//
//  - the outgoing page fades quickly (gone by 0.35 of the content's progress), the incoming one
//    starts invisible and becomes readable right after (0.3 .. 0.75), so the two are never both
//    readable;
//  - a new click decides, from the LIVE opacities, who owns the screen right now: if the
//    incoming page already shows more than the outgoing one it becomes the outgoing page
//    (continuing from its opacity, never restarting) and the previous outgoing page is dropped at
//    once; otherwise the outgoing page stays and the incoming page, which never became visible,
//    is dropped. The newest target is the new incoming page. Clicking the outgoing page's own tab
//    brings it back, rising from where it is. Nothing queues: the latest target wins.
//
// The title, the header action and the panel are three cells that all render this one list, so
// they can never disagree about who is leaving. Opacity and offset are MotionValues written from
// the content progress every frame (no React render per frame); React only re-renders when the
// list changes: a click, and the one commit that removes a page that has faded out. That removal
// names the layer by a unique id and only removes it while it is still leaving, so a stale one
// can never remove a page that came back or the current target.

import { useLayoutEffect, useState } from "react";
import { motionValue, type MotionValue } from "motion/react";
import { layerFrame, transitionDirection, transitionProgress, type Fade, type LayerFrame, type Transition } from "../../lib/island/morph";
import type { SpringParams } from "../../lib/island/spring";
import { TAB_SHIFT_PX } from "./animations";
import type { TabId } from "./tabs";

/**
 * When a tab's page fades, as fractions of the content's progress. The outgoing page is gone
 * by 0.38 (it loses the screen early, with an ease-in that keeps it readable only briefly); the
 * incoming one is invisible until 0.26 and fully in by 0.7. They cross at ~0.27 opacity each
 * (never both readable, and the panel area never dips below ~0.27: it was 0.12, a visible
 * blink); the incoming page is readable by ~0.4 of the content's progress.
 */
export const tabFade = { in: [0.26, 0.7], out: 0.38 } as const satisfies Fade;

/**
 * The content progress spring (critically damped: no overshoot). The reduced profile is tighter
 * and shorter: with no shift to carry the eye, the hand-off just has to be quick and calm.
 */
export const tabContentSprings: { normal: SpringParams; reduced: SpringParams } = {
  normal: { response: 0.32, dampingFraction: 1 },
  reduced: { response: 0.22, dampingFraction: 1 },
};

/**
 * The selection capsule (one element, a physical object that never swings past its tab). 0.28
 * (it was 0.22) so a one-tab move at 60 Hz travels at most ~0.17 slot a frame, and a four-tab
 * move on a 32 Hz remote session needs several frames to arrive instead of crossing in two; it
 * still reacts on the very next frame and is ahead of the content (0.32).
 */
export const tabCapsuleSpring: SpringParams = { response: 0.28, dampingFraction: 1 };

/** How far (px) a page shifts towards its tab: a hint. Reduced motion does not move it at all. */
export const TAB_SHIFT_PX_REDUCED = 0;

export interface TabSteps {
  /** The content progress spring's target: one step further for every tab change. */
  target: number;
  /** Which way the change went along the dock (later tabs are to the right, in every language). */
  direction: -1 | 1;
}

/**
 * Where the content progress is heading: one step further every time the tab changes, and which
 * way the change went. The progress only ever counts forwards: it is the hand-off's clock, not a
 * physical object, so a change caught mid-way never has to undo momentum first; what is on
 * screen continues from where it is regardless.
 */
export function useTabSteps(activeTab: TabId, activeIndex: number): TabSteps {
  const [steps, setSteps] = useState({ tab: activeTab, index: activeIndex, target: 0, direction: 1 as -1 | 1 });
  if (steps.tab === activeTab) return steps;
  const next = { tab: activeTab, index: activeIndex, target: steps.target + 1, direction: (activeIndex < steps.index ? -1 : 1) as -1 | 1 };
  setSteps(next);
  return next;
}

/** One mounted page of the tab content. */
export interface TabLayer {
  /** Unique for the layer's whole life: what a removal names, so a stale one cannot hit another layer. */
  readonly id: number;
  readonly tab: TabId;
  /** True for the outgoing page: faded out and unclickable, then removed. */
  readonly leaving: boolean;
  readonly opacity: MotionValue<number>;
  /** Sideways shift in px (always 0 under reduced motion). */
  readonly offset: MotionValue<number>;
  /** What the page showed when the current transition began: its fade continues from here. */
  readonly start: LayerFrame;
}

interface LayersState {
  tab: TabId;
  transition: Transition;
  /** The outgoing page (if any) first, then the incoming one: stacking order. */
  layers: readonly TabLayer[];
  nextId: number;
}

/**
 * The layers after a click on `tab`. Pure apart from reading the live values: decides who owns
 * the screen, drops what must go, snapshots what continues and adds the new target. Always
 * returns one incoming layer and at most one outgoing one.
 */
export function retargetLayers(layers: readonly TabLayer[], tab: TabId, shift: number, direction: -1 | 1, nextId: number): { layers: TabLayer[]; nextId: number } {
  const incoming = layers.find((layer) => !layer.leaving)!;
  const outgoing = layers.find((layer) => layer.leaving);
  const snapshot = (layer: TabLayer, leaving: boolean): TabLayer => ({ ...layer, leaving, start: { opacity: layer.opacity.get(), offset: layer.offset.get() } });

  // Who owns the screen right now: the incoming page once it shows more than the outgoing one
  // (an outgoing page that has nothing left to show owns nothing).
  const outgoingOpacity = outgoing ? outgoing.opacity.get() : 0;
  const incomingOwns = !outgoing || outgoingOpacity <= 0 || incoming.opacity.get() > outgoingOpacity;

  if (outgoing && outgoing.tab === tab) {
    // Back to the page that is leaving: it rises again from its current opacity.
    return { layers: incomingOwns ? [snapshot(incoming, true), snapshot(outgoing, false)] : [snapshot(outgoing, false)], nextId };
  }
  const leavingLayer = incomingOwns ? snapshot(incoming, true) : snapshot(outgoing!, true);
  const fresh: TabLayer = {
    id: nextId,
    tab,
    leaving: false,
    opacity: motionValue(0),
    offset: motionValue(direction * shift),
    start: { opacity: 0, offset: direction * shift },
  };
  return { layers: [leavingLayer, fresh], nextId: nextId + 1 };
}

/**
 * The tab content's layers (at most two) for `activeTab`, faded by `progress` (the content
 * spring, whose target is `steps.target`). `reduced` drops the sideways shift.
 */
export function useTabLayers(activeTab: TabId, progress: MotionValue<number>, steps: TabSteps, reduced: boolean): readonly TabLayer[] {
  const shift = reduced ? TAB_SHIFT_PX_REDUCED : TAB_SHIFT_PX;
  const [state, setState] = useState<LayersState>(() => ({
    tab: activeTab,
    transition: { from: [steps.target], to: [steps.target], direction: steps.direction },
    layers: [{ id: 0, tab: activeTab, leaving: false, opacity: motionValue(1), offset: motionValue(0), start: { opacity: 1, offset: 0 } }],
    nextId: 1,
  }));

  // Decided during render, like the steps: the commit that mounts the new target already knows
  // it starts invisible, and the progress has not moved yet (its spring retargets in a layout effect).
  let current = state;
  if (state.tab !== activeTab) {
    const next = retargetLayers(state.layers, activeTab, shift, steps.direction, state.nextId);
    current = {
      tab: activeTab,
      transition: { from: [progress.get()], to: [steps.target], direction: steps.direction },
      layers: next.layers,
      nextId: next.nextId,
    };
    setState(current);
  }

  useLayoutEffect(() => {
    const run = () => {
      const p = transitionProgress(current.transition, [progress.get()]);
      const exit = -transitionDirection(current.transition) * shift;
      let gone: number[] | null = null;
      for (const layer of current.layers) {
        const frame = layerFrame(tabFade, !layer.leaving, layer.start, exit, p);
        layer.opacity.set(frame.opacity);
        layer.offset.set(frame.offset);
        if (layer.leaving && frame.opacity <= 0) (gone ??= []).push(layer.id);
      }
      if (gone) {
        const ids = gone;
        // By id and only while still leaving: a stale removal never touches a page that is current.
        setState((prev) => (prev.layers.some((layer) => layer.leaving && ids.includes(layer.id)) ? { ...prev, layers: prev.layers.filter((layer) => !(layer.leaving && ids.includes(layer.id))) } : prev));
      }
    };
    run();
    return progress.on("change", run);
  }, [current, progress, shift]);

  return current.layers;
}
