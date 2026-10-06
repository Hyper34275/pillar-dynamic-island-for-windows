import { motion, type HTMLMotionProps } from "motion/react";
import type { CSSProperties } from "react";
import type { Fade } from "../../lib/island/morph";
import { useTransitionLayer } from "./drivenTransition";

interface IslandLayerProps extends Omit<HTMLMotionProps<"div">, "style"> {
  /** Names the layer in the DOM (`data-layer`), for tests and diagnostics. */
  name: string;
  fade: Fade;
  /** The layer's own size: the island's size while it shows this layer. */
  size: { width: number; height: number };
  style?: CSSProperties;
}

/**
 * One kind of island content: the compact date, the expanded panel, a meeting alert, a
 * notification. It is laid out once, at its final size, pinned to the island's top centre, and
 * never resized: the island morphs around it and clips it, so nothing inside reflows, squashes
 * or scales while the island changes shape. Its opacity is read off the island's animated size
 * (see morph.ts). When it is replaced it stays mounted, unclickable and hidden from assistive
 * tech, until it has faded out.
 */
export function IslandLayer({ name, fade, size, className = "", style, tabIndex, children, ...rest }: IslandLayerProps) {
  const { opacity, isPresent } = useTransitionLayer({ fade });
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
        opacity,
        pointerEvents: isPresent ? style?.pointerEvents : "none",
      }}
    >
      {children}
    </motion.div>
  );
}
