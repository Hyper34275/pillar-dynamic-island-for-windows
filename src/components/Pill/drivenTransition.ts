import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useMotionValue, usePresence, type MotionValue } from "motion/react";
import { layerFrame, transitionDirection, transitionProgress, type Fade, type LayerFrame, type Transition } from "../../lib/island/morph";

/** A transition as its layers see it: where it started and where it goes, plus the live driver values. */
export interface DrivenTransition {
  transition: Transition;
  drivers: readonly MotionValue<number>[];
}

// Outside any provider (static renders, isolated tests) layers simply show, fully.
const SETTLED: DrivenTransition = { transition: { from: [], to: [] }, drivers: [] };

/**
 * The transition the layers below follow. Nested providers scope it: island layers follow the
 * island's size, tab panels inside the expanded island follow the dock indicator. Context, not
 * props, on purpose: a leaving layer is frozen by AnimatePresence with its last props, but it
 * must still follow the newest transition.
 */
export const TransitionContext = createContext<DrivenTransition>(SETTLED);

const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

/**
 * The transition `drivers` are making towards `target`. A new one begins, from wherever the
 * drivers are at that instant, whenever `key` (what is shown) or `target` changes. It is decided
 * during render, so the very commit that mounts a new layer already knows that layer starts
 * the transition invisible: no frame can show it ahead of the container.
 */
export function useDrivenTransition(drivers: readonly MotionValue<number>[], target: readonly number[], key: string): DrivenTransition {
  const id = `${key}|${target.join(",")}`;
  const [state, setState] = useState(() => ({ id, transition: { from: target, to: target } as Transition }));
  let current = state;
  if (state.id !== id) {
    current = { id, transition: { from: drivers.map((driver) => driver.get()), to: target } };
    setState(current);
  }
  const { transition } = current;
  return useMemo(() => ({ transition, drivers }), [transition, drivers]);
}

interface LayerOptions {
  fade: Fade;
  /** How far (px) the layer travels with the transition's direction: in from +offset, out to -offset. */
  offset?: number;
}

/**
 * Opacity and offset of one layer of the nearest transition, recomputed from the drivers every
 * frame they move (no React render per frame). Inside AnimatePresence a removed layer stays
 * mounted until it has faded out, then lets AnimatePresence remove it; if it is added back
 * meanwhile it rises again from where it was.
 */
export function useTransitionLayer({ fade, offset = 0 }: LayerOptions): {
  opacity: MotionValue<number>;
  offset: MotionValue<number>;
  isPresent: boolean;
} {
  const driven = useContext(TransitionContext);
  const [isPresent, safeToRemove] = usePresence();
  const opacity = useMotionValue(0);
  const shift = useMotionValue(0);

  // What this layer showed when the current transition (or its own presence) last changed:
  // the point its fade continues from. A newly mounted layer starts invisible.
  const startRef = useRef<{ transition: Transition; entering: boolean; frame: LayerFrame } | null>(null);
  const previous = startRef.current;
  if (!previous || previous.transition !== driven.transition || previous.entering !== isPresent) {
    startRef.current = {
      transition: driven.transition,
      entering: isPresent,
      frame: previous
        ? { opacity: opacity.get(), offset: shift.get() }
        : { opacity: 0, offset: transitionDirection(driven.transition) * offset },
    };
  }

  const update = () => {
    const { transition, entering, frame: start } = startRef.current!;
    const progress = transitionProgress(
      transition,
      driven.drivers.map((driver) => driver.get())
    );
    const frame = layerFrame(fade, entering, start, -transitionDirection(transition) * offset, progress);
    opacity.set(frame.opacity);
    shift.set(frame.offset);
  };
  // Also during render, so the first paint of a new transition is already right.
  update();
  const updateRef = useRef(update);
  updateRef.current = update;

  useIsomorphicLayoutEffect(() => {
    const run = () => updateRef.current();
    const unsubscribe = driven.drivers.map((driver) => driver.on("change", run));
    return () => unsubscribe.forEach((off) => off());
  }, [driven.drivers]);

  useEffect(() => {
    if (isPresent || !safeToRemove) return;
    if (opacity.get() <= 0) {
      safeToRemove();
      return;
    }
    return opacity.on("change", (value) => {
      if (value <= 0) safeToRemove();
    });
  }, [isPresent, safeToRemove, opacity]);

  return { opacity, offset: shift, isPresent };
}
