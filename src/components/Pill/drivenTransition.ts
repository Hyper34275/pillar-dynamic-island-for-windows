import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useMotionValue, usePresence, type MotionValue } from "motion/react";
import { layerFrame, transitionDirection, transitionProgress, type Fade, type LayerStart, type Transition } from "../../lib/island/morph";

/** A transition as its layers see it: where it started and where it goes, plus the live driver values. */
export interface DrivenTransition {
  transition: Transition;
  drivers: readonly MotionValue<number>[];
}

// Outside any provider (static renders, isolated tests) layers simply show, fully.
const SETTLED: DrivenTransition = { transition: { from: [], to: [] }, drivers: [] };

/**
 * The transition the layers below follow. Nested providers scope it: island layers follow the
 * island's size, tab panels inside the expanded island follow the tab content's progress.
 * Context, not props, on purpose: a leaving layer is frozen by AnimatePresence with its last
 * props, but it must still follow the newest transition.
 */
export const TransitionContext = createContext<DrivenTransition>(SETTLED);

/**
 * The island's shape as content inside it sees it, whatever transition is nearest: the live
 * width and height (for riding its edges) and the island's own transition (for the expanded
 * island's parts, which sit inside the tab transition's provider).
 */
export interface Shell {
  transition: DrivenTransition;
  width: MotionValue<number>;
  height: MotionValue<number>;
}
export const ShellContext = createContext<Shell | null>(null);

/** The island layer a part belongs to: its laid-out size and whether it is arriving or leaving. */
export interface LayerInfo {
  width: number;
  height: number;
  isPresent: boolean;
}
export const LayerContext = createContext<LayerInfo | null>(null);

const useIsomorphicLayoutEffect = typeof window === "undefined" ? useEffect : useLayoutEffect;

/**
 * The transition `drivers` are making towards `target`. A new one begins whenever `key` (what is
 * shown) or `target` changes. It is decided during render, so the very commit that mounts a new
 * layer already knows that layer starts the transition invisible: no frame can show it ahead of
 * the container.
 *
 * Its path is measured from where the PREVIOUS transition was heading (that shape is what the
 * layers on screen belong to), not from wherever the drivers happen to be. At rest the two are the
 * same. Caught half way (the island reversed 60 ms into opening), the new transition's progress
 * already stands part of the way along the path back, and the layers' fades begin there
 * (morph.ts LayerStart.p): the content leaves and arrives against the shape's actual size, so an
 * interruption cannot leave a large shape between two owners with nothing in it.
 */
export function useDrivenTransition(
  drivers: readonly MotionValue<number>[],
  target: readonly number[],
  key: string,
  direction?: -1 | 0 | 1
): DrivenTransition {
  const id = `${key}|${target.join(",")}`;
  const [state, setState] = useState(() => ({ id, transition: { from: target, to: target, direction } as Transition }));
  let current = state;
  if (state.id !== id) {
    current = { id, transition: { from: state.transition.to, to: target, direction } };
    setState(current);
  }
  const { transition } = current;
  return useMemo(() => ({ transition, drivers }), [transition, drivers]);
}

/**
 * Opacity and offset of something that fades with `driven`, recomputed from its drivers every
 * frame they move (no React render per frame). `entering` says which way it goes. What it
 * showed when the transition (or its direction) last changed is where its fade continues
 * from, so an interrupted fade never jumps; a newly mounted one starts invisible.
 */
function useDrivenFade(driven: DrivenTransition, entering: boolean, fade: Fade, offset: number) {
  const opacity = useMotionValue(0);
  const shift = useMotionValue(0);

  const startRef = useRef<{ transition: Transition; entering: boolean; frame: LayerStart } | null>(null);
  const previous = startRef.current;
  if (!previous || previous.transition !== driven.transition || previous.entering !== entering) {
    startRef.current = {
      transition: driven.transition,
      entering,
      frame: {
        ...(previous ? { opacity: opacity.get(), offset: shift.get() } : { opacity: 0, offset: transitionDirection(driven.transition) * offset }),
        p: transitionProgress(driven.transition, driven.drivers.map((driver) => driver.get())),
      },
    };
  }

  const update = () => {
    const { transition, entering: arriving, frame: start } = startRef.current!;
    const progress = transitionProgress(
      transition,
      driven.drivers.map((driver) => driver.get())
    );
    const frame = layerFrame(fade, arriving, start, -transitionDirection(transition) * offset, progress);
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

  return { opacity, offset: shift };
}

interface LayerOptions {
  fade: Fade;
  /** How far (px) the layer travels with the transition's direction: in from +offset, out to -offset. */
  offset?: number;
}

/**
 * Opacity and offset of one layer of the nearest transition. Inside AnimatePresence a removed
 * layer stays mounted until it has faded out, then lets AnimatePresence remove it; if it is
 * added back meanwhile it rises again from where it was.
 */
export function useTransitionLayer({ fade, offset = 0 }: LayerOptions): {
  opacity: MotionValue<number>;
  offset: MotionValue<number>;
  isPresent: boolean;
} {
  const driven = useContext(TransitionContext);
  const [isPresent, safeToRemove] = usePresence();
  const { opacity, offset: shift } = useDrivenFade(driven, isPresent, fade, offset);

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

/**
 * Opacity of one part of an island layer (the expanded island's header, body, dock), on the
 * island's own transition and the layer's own arrival or departure. The layer's removal waits
 * for the slowest part (its own fade's `out`).
 */
export function useIslandPartFade(fade: Fade): MotionValue<number> {
  const shell = useContext(ShellContext);
  const layer = useContext(LayerContext);
  return useDrivenFade(shell?.transition ?? SETTLED, layer?.isPresent ?? true, fade, 0).opacity;
}
