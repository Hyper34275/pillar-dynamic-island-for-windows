// Toast → toast: the island stays open and only its payload changes hands.
//
// Within a notification session (one open, several notifications, one close) the black shape is
// the persistent object: the toast layer (NotificationToast) stays mounted from the first
// notification to the last, so the island's own transition never cross-fades two toasts. Inside
// it the PAYLOAD (icon, source, title, body, buttons) is replaced with a strict ownership handoff:
//
//  - at most two payloads are mounted: the one leaving and the one arriving;
//  - the leaving one fades out quickly with a tiny upward drift and is gone by HANDOFF;
//  - the arriving one only starts at HANDOFF (so two app icons, two titles, two mirrored RTL / LTR
//    layouts are never readable together: at the crossing both are faint);
//  - the handoff runs on its own critically damped clock, combined with the shell's morph by
//    "least advanced" (morph.ts combine "min"): when the next toast needs a different width the
//    new payload, laid out at its final width, waits for the shape that has to hold it instead of
//    showing clipped inside the previous toast's width;
//  - a new notification mid-handoff retargets: whoever owns the screen at that instant (the
//    arriving one once past HANDOFF, else the leaving one) becomes the leaving payload and
//    continues from its current opacity; a payload that never owned the screen is discarded at
//    once. The latest notification always wins; nothing queues here (the session queue in
//    lib/island/state.ts decides WHAT is current, this only decides how it is drawn).
//
// No timers, animationend or promises: opacities are read off the drivers every frame, and a
// removal carries the generation it was scheduled for, so a stale removal can never take away
// the current payload.

import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useMotionValue, type MotionValue } from "motion/react";
import type { IslandNotification } from "../../lib/ipc";
import { layerFrame, transitionProgress, type Fade, type LayerFrame, type Transition } from "../../lib/island/morph";
import type { SpringParams } from "../../lib/island/spring";
import { useSpringValue } from "./useIslandMotion";

/** Where on the handoff's progress the screen changes owner. */
export const TOAST_HANDOFF = 0.32;

/** The leaving payload is gone by 0.34; the arriving one starts just before (0.27: no empty instant, both faint at the crossing) and is fully in by 0.8. */
export const toastPayloadFade: Fade = { in: [0.27, 0.8], out: 0.34 };

/**
 * The handoff's clock: critically damped (no bounce), about as quick as the shell's own width
 * spring so neither waits long for the other. Reduced motion: tighter, no drift.
 */
export const toastHandoffSprings: { normal: SpringParams; reduced: SpringParams } = {
  normal: { response: 0.3, dampingFraction: 1 },
  reduced: { response: 0.2, dampingFraction: 1 },
};

/** The drift (px, vertical) of a payload: the arriving one rises this far into place, the leaving one this far out. */
export const TOAST_DRIFT_PX = 4;

export interface PayloadLayer {
  id: number;
  /** What it shows. The arriving payload follows the newest props (an inline update of the same id); a leaving one is frozen. */
  notification: IslandNotification;
  leaving: boolean;
  /** Bumped whenever the layer changes role, so a removal scheduled for an older role is ignored. */
  gen: number;
}

interface HandoffState {
  shownId: number;
  /** The handoff clock's target: one step further per change of payload (it only ever counts forwards). */
  target: number;
  gen: number;
  layers: PayloadLayer[];
}

export interface DrivenHandoff {
  transition: Transition;
  drivers: readonly MotionValue<number>[];
}

interface ShellDrivers {
  width: MotionValue<number>;
  height: MotionValue<number>;
}

/**
 * The payloads to draw for `notification` (at most two) and the transition they fade on.
 * `targetSize` is the shape the island is heading for with this notification (PillShell's
 * target, so a monitor limit is respected); `shell` its live size (ShellContext).
 */
export function useToastPayloads(
  notification: IslandNotification,
  targetSize: { width: number; height: number },
  shell: ShellDrivers | null,
  reducedMotion: boolean
): { layers: PayloadLayer[]; driven: DrivenHandoff; remove: (id: number, gen: number) => void } {
  const [state, setState] = useState<HandoffState>(() => ({
    shownId: notification.id,
    target: 0,
    gen: 0,
    layers: [{ id: notification.id, notification, leaving: false, gen: 0 }],
  }));
  const progress = useSpringValue(state.target, reducedMotion ? toastHandoffSprings.reduced : toastHandoffSprings.normal);
  const drivers = useMemo(() => (shell ? [shell.width, shell.height, progress] : [progress]), [shell, progress]);

  // The transition the payloads follow now (decided during render, like useDrivenTransition, so
  // the commit that mounts the arriving payload already knows it starts invisible).
  const transitionRef = useRef<{ id: string; transition: Transition } | null>(null);

  let current = state;
  if (state.shownId !== notification.id) {
    const previous = transitionRef.current;
    const p = previous ? transitionProgress(previous.transition, drivers.map((driver) => driver.get())) : 1;
    const present = state.layers.find((layer) => !layer.leaving);
    const leaving = state.layers.find((layer) => layer.leaving);
    // Who owns the screen right now: the arriving payload once past the handoff, else the leaving one.
    const owner = !leaving || p >= TOAST_HANDOFF ? present : leaving;
    const gen = state.gen + 1;
    const back = state.layers.find((layer) => layer.id === notification.id);
    let layers: PayloadLayer[];
    if (back) {
      // The notification on screen a moment ago comes back: it rises again from where it is.
      const other = state.layers.find((layer) => layer !== back);
      layers = [
        ...(other && other === owner ? [{ ...other, leaving: true, gen }] : []),
        { ...back, notification, leaving: false, gen },
      ];
    } else {
      layers = [...(owner ? [{ ...owner, leaving: true, gen }] : []), { id: notification.id, notification, leaving: false, gen }];
    }
    current = { shownId: notification.id, target: state.target + 1, gen, layers };
    setState(current);
  } else {
    // Same notification, updated inline (the session replaced its payload): no handoff.
    const present = state.layers.find((layer) => !layer.leaving);
    if (present && present.notification !== notification) {
      current = { ...state, layers: state.layers.map((layer) => (layer === present ? { ...layer, notification } : layer)) };
      setState(current);
    }
  }

  const goal = shell ? [targetSize.width, targetSize.height, current.target] : [current.target];
  const transitionId = goal.join(",");
  if (transitionRef.current?.id !== transitionId) {
    transitionRef.current = {
      id: transitionId,
      transition: {
        // The first payload of a toast has nowhere to come from: it is simply there (the island's
        // own transition fades the whole toast in).
        from: transitionRef.current ? drivers.map((driver) => driver.get()) : goal,
        to: goal,
        direction: 1,
        combine: "min",
      },
    };
  }
  const { transition } = transitionRef.current;
  const driven = useMemo(() => ({ transition, drivers }), [transition, drivers]);

  const removeRef = useRef((id: number, gen: number) => {
    setState((s) => {
      const layer = s.layers.find((l) => l.id === id);
      if (!layer || !layer.leaving || layer.gen !== gen) return s;
      return { ...s, layers: s.layers.filter((l) => l !== layer) };
    });
  });

  return { layers: current.layers, driven, remove: removeRef.current };
}

const useIsomorphicLayoutEffect = typeof window === "undefined" ? () => {} : useLayoutEffect;

/**
 * One payload's opacity and drift on the handoff, recomputed from the drivers every frame they
 * move. Continues from what it showed when the transition (or its role) last changed, so an
 * interrupted handoff never jumps; a newly mounted payload starts invisible. Calls `onGone`
 * once a leaving payload has faded out.
 */
export function usePayloadFade(
  driven: DrivenHandoff,
  leaving: boolean,
  drift: number,
  onGone: () => void
): { opacity: MotionValue<number>; offset: MotionValue<number> } {
  const opacity = useMotionValue(0);
  const offset = useMotionValue(0);
  const startRef = useRef<{ transition: Transition; leaving: boolean; frame: LayerFrame } | null>(null);
  const previous = startRef.current;
  if (!previous || previous.transition !== driven.transition || previous.leaving !== leaving) {
    startRef.current = {
      transition: driven.transition,
      leaving,
      frame: previous ? { opacity: opacity.get(), offset: offset.get() } : { opacity: 0, offset: drift },
    };
  }
  const update = () => {
    const { transition, leaving: going, frame } = startRef.current!;
    const p = transitionProgress(
      transition,
      driven.drivers.map((driver) => driver.get())
    );
    const next = layerFrame(toastPayloadFade, !going, frame, -drift, p);
    opacity.set(next.opacity);
    offset.set(next.offset);
  };
  update();
  const updateRef = useRef(update);
  updateRef.current = update;
  const goneRef = useRef(onGone);
  goneRef.current = onGone;

  useIsomorphicLayoutEffect(() => {
    const run = () => updateRef.current();
    const off = driven.drivers.map((driver) => driver.on("change", run));
    return () => off.forEach((stop) => stop());
  }, [driven.drivers]);

  useIsomorphicLayoutEffect(() => {
    if (!leaving) return;
    if (opacity.get() <= 0) {
      goneRef.current();
      return;
    }
    return opacity.on("change", (value) => {
      if (value <= 0) goneRef.current();
    });
  }, [leaving, opacity]);

  return { opacity, offset };
}
