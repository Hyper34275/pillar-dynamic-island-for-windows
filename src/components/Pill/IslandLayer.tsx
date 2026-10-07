import { motion, useIsPresent, useMotionValue, useTransform, type HTMLMotionProps, type MotionValue } from "motion/react";
import { createContext, useContext, useMemo, type CSSProperties, type ReactNode } from "react";
import type { Fade } from "../../lib/island/morph";
import { arrivalFade, BODY_ENTRY_SCALE, fitScale, type IslandOrigin } from "./animations";
import { LayerContext, ShellContext, useIslandPartFade, useTransitionLayer } from "./drivenTransition";

/**
 * What the island showed before the transition now running (PillShell provides it). An arriving
 * layer picks its fade-in window from it (animations.ts entryFade), so each hand-over is strict:
 * the compact content arrives late after the 400x440 panel, early after a toast. Leaving layers
 * keep their own `out`. Outside a provider (tests, the tour) the layer's own `fade` applies.
 */
export const IslandOriginContext = createContext<IslandOrigin | null>(null);

/** Layers whose arrival depends on the origin, by name. */
const ARRIVAL_KIND: Record<string, "compact" | "temporary"> = { compact: "compact", meetingAlert: "temporary", ringer: "temporary", toast: "temporary" };

/**
 * The fade a layer called `name` uses: its own while leaving (or outside a shell), the
 * origin's arrival window while it arrives.
 */
export function useArrivalFade(name: string, fade: Fade): Fade {
  // useIsPresent only reads the presence (usePresence would register a second removal guard).
  const isPresent = useIsPresent();
  const origin = useContext(IslandOriginContext);
  const kind = ARRIVAL_KIND[name];
  return origin && kind && isPresent ? arrivalFade(kind, origin) : fade;
}

interface IslandLayerProps extends Omit<HTMLMotionProps<"div">, "style"> {
  /** Names the layer in the DOM (`data-layer`), for tests and diagnostics. */
  name: string;
  fade: Fade;
  /** The layer's own size: the island's size while it shows this layer. */
  size: { width: number; height: number };
  /**
   * The layer's parts fade on their own (IslandPart) and the layer itself stays opaque; its
   * `fade.out` then only says when it may be removed (the slowest part's `out`).
   */
  parts?: boolean;
  style?: CSSProperties;
}

/** How far (px) a layer of `size` sits below the island's top so that it rides the shape's vertical centre. */
function useRide(size: number, shellSize: MotionValue<number> | undefined) {
  const still = useMotionValue(size);
  return useTransform(shellSize ?? still, (s: number) => (shellSize ? (s - size) / 2 : 0));
}

/**
 * One kind of island content: the compact date, the expanded panel, a meeting alert, a
 * notification. It is laid out once, at its final size, and never resized: the island morphs
 * around it and clips it, so nothing inside reflows, squashes or scales while the island
 * changes shape. It is centred on the island horizontally and rides the island's vertical
 * centre while the shape is a different height, so its content is always in the middle of the
 * shape (never floating at the top of a large empty one) and arrives exactly in place, offset
 * 0, when the shape is its size. Its opacity is read off the island's animated size (see
 * morph.ts). When it is replaced it stays mounted, unclickable and hidden from assistive tech,
 * until it has faded out.
 */
export function IslandLayer({ name, fade, size, parts = false, className = "", style, tabIndex, children, ...rest }: IslandLayerProps) {
  const { opacity, isPresent } = useTransitionLayer({ fade: useArrivalFade(name, fade) });
  const shell = useContext(ShellContext);
  const y = useRide(size.height, shell?.height);
  const layer = useMemo(() => ({ width: size.width, height: size.height, isPresent }), [size.width, size.height, isPresent]);
  return (
    <motion.div
      {...rest}
      data-layer={name}
      aria-hidden={isPresent ? rest["aria-hidden"] : true}
      tabIndex={isPresent ? tabIndex : undefined}
      className={`absolute top-0 ${className}`}
      style={{
        ...style,
        width: size.width,
        height: size.height,
        left: `calc(50% - ${size.width / 2}px)`,
        y,
        opacity: parts ? 1 : opacity,
        pointerEvents: isPresent ? style?.pointerEvents : "none",
      }}
    >
      <LayerContext.Provider value={layer}>{children as ReactNode}</LayerContext.Provider>
    </motion.div>
  );
}

/**
 * Which edge of the island's shape a part travels with: "top-start" the top and leading corner
 * (the header), "top" the top edge, centred horizontally (the body, right under the header),
 * "bottom" the bottom edge (the dock), "center" the middle.
 */
export type PartAnchor = "top-start" | "top" | "bottom" | "center";

interface IslandPartProps {
  fade: Fade;
  anchor: PartAnchor;
  /**
   * The layer runs right to left: "top-start" then rides the top-RIGHT corner (the title sits at
   * the leading edge, so it must come out of the shape's leading side, not be cropped by it).
   */
  rtl?: boolean;
  /**
   * A "top" part (the panel's body) is masked to the room above the row that rides the bottom edge
   * (the dock): while the shape is shorter than the layer the body never runs under the dock (no
   * double exposure of a card and the dock). Its top, the primary content right under the header,
   * is whole and in place from its first visible frame; its only cut is at the bottom, exactly on
   * the dock's line, which the dock (arriving with it) covers. It scales a touch from its top with
   * its fade (BODY_ENTRY_SCALE), so it establishes itself / recedes rather than being uncovered.
   */
  between?: boolean;
  /** No scale (reduced motion). */
  still?: boolean;
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}

/**
 * A part of an island layer that travels with one edge of the shape while it morphs: the
 * header with the top-left corner, the dock with the bottom edge, the body in the middle. So
 * the open island's parts come out of the shape's edges as it grows and go back into them as
 * it shrinks, instead of being uncovered or cropped away by a moving mask. At rest every
 * offset is 0, so the resting layout is exactly the static one.
 */
export function IslandPart({ fade, anchor, rtl = false, between = false, still: noScale = false, className = "", style, children }: IslandPartProps) {
  const opacity = useIslandPartFade(fade);
  const shell = useContext(ShellContext);
  const layer = useContext(LayerContext);
  const layerWidth = layer?.width ?? 0;
  const layerHeight = layer?.height ?? 0;
  const still = useMotionValue(0);
  // The layer is centred horizontally and rides the vertical centre (IslandLayer); these undo
  // that for the edge the part belongs to.
  const x = useTransform(shell?.width ?? still, (w: number) => (shell && anchor === "top-start" ? ((layerWidth - w) / 2) * (rtl ? -1 : 1) : 0));
  const y = useTransform(shell?.height ?? still, (h: number) =>
    !shell ? 0 : anchor === "top-start" || anchor === "top" ? (layerHeight - h) / 2 : anchor === "bottom" ? (h - layerHeight) / 2 : 0
  );
  // The body rides the top edge under the header; the dock rides the bottom edge, i.e. it is
  // d = layerHeight - h closer to the body than at rest. Masking d off the body's bottom keeps it
  // exactly above the dock (at rest d = 0: no mask).
  const clipPath = useTransform(shell?.height ?? still, (h: number) => {
    if (!between || !shell) return "none";
    const cut = Math.max(0, layerHeight - h);
    return cut > 0.01 ? `inset(0 0 ${cut}px 0)` : "none";
  });
  // The body establishes itself with a slight scale (not under reduced motion); the body and the
  // dock also fit the shape's width while it is narrower than the panel (animations.ts fitScale).
  const fits = anchor === "top" || anchor === "bottom";
  const scale = useTransform([opacity, shell?.width ?? still], ([o, w]: number[]) => {
    const entry = between && !noScale ? BODY_ENTRY_SCALE + (1 - BODY_ENTRY_SCALE) * o : 1;
    return Math.min(entry, shell && fits ? fitScale(w, layerWidth) : 1);
  });
  const origin = anchor === "top" ? "50% 0" : anchor === "bottom" ? "50% 100%" : undefined;
  return (
    <motion.div data-part={anchor} className={className} style={{ ...style, x, y, opacity, clipPath, scale, transformOrigin: origin }}>
      {children}
    </motion.div>
  );
}
