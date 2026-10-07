import { motion, useMotionValue, useTransform, type HTMLMotionProps, type MotionValue } from "motion/react";
import { useContext, useMemo, type CSSProperties, type ReactNode } from "react";
import type { Fade } from "../../lib/island/morph";
import { LayerContext, ShellContext, useIslandPartFade, useTransitionLayer } from "./drivenTransition";

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
  const { opacity, isPresent } = useTransitionLayer({ fade });
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

/** Which edge of the island's shape a part travels with. */
export type PartAnchor = "top-start" | "bottom" | "center";

interface IslandPartProps {
  fade: Fade;
  anchor: PartAnchor;
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
export function IslandPart({ fade, anchor, className = "", style, children }: IslandPartProps) {
  const opacity = useIslandPartFade(fade);
  const shell = useContext(ShellContext);
  const layer = useContext(LayerContext);
  const layerWidth = layer?.width ?? 0;
  const layerHeight = layer?.height ?? 0;
  const still = useMotionValue(0);
  // The layer is centred horizontally and rides the vertical centre (IslandLayer); these undo
  // that for the edge the part belongs to.
  const x = useTransform(shell?.width ?? still, (w: number) => (shell && anchor === "top-start" ? (layerWidth - w) / 2 : 0));
  const y = useTransform(shell?.height ?? still, (h: number) =>
    !shell ? 0 : anchor === "top-start" ? (layerHeight - h) / 2 : anchor === "bottom" ? (h - layerHeight) / 2 : 0
  );
  return (
    <motion.div data-part={anchor} className={className} style={{ ...style, x, y, opacity }}>
      {children}
    </motion.div>
  );
}
