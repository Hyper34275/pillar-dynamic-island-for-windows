import { motion, useReducedMotion } from "motion/react";
import type { CSSProperties, ReactNode } from "react";
import { badge, color, icon as iconTokens, identityColors, type as typeRoles } from "../../../design/tokens";
import { springConfig } from "../animations";
import { CalendarIcon } from "./icons";

// =============================================================================
// Identity: the app/event tile every notification starts with, and the one count badge.
// =============================================================================

const tileColorCache = new Map<string, string>();

/** Stable identity colour derived from a name (identity only, never status). */
export function identityColor(name: string): string {
  const cached = tileColorCache.get(name);
  if (cached) return cached;
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
  const tint = identityColors[Math.abs(hash) % identityColors.length];
  if (tileColorCache.size > 200) tileColorCache.clear();
  tileColorCache.set(name, tint);
  return tint;
}

interface AppIconProps {
  /** The cleaned app name: its initial and its identity colour. */
  name?: string;
  /** A glyph instead of the initial (e.g. the calendar of an invitation). */
  glyph?: ReactNode;
  /** Overrides the identity colour (the calendar's blue). */
  tint?: string;
}

/**
 * The identity tile: always 40×40 with radius 12 (concentric inside the 28-radius toast at its
 * 16px padding). One size and one shape for every app, the calendar included.
 */
export function AppIcon({ name = "", glyph, tint }: AppIconProps) {
  const fill = tint ?? identityColor(name);
  const style: CSSProperties = {
    width: iconTokens.app,
    height: iconTokens.app,
    borderRadius: iconTokens.appRadius,
    background: `linear-gradient(160deg, ${fill}, color-mix(in srgb, ${fill} 72%, black))`,
    boxShadow: "inset 0 0.5px 0 rgba(255,255,255,0.25)",
    // The initial: 17px semibold (the title role's size), optically centred by the line box.
    fontSize: typeRoles.title.size,
    lineHeight: `${iconTokens.app}px`,
  };
  return (
    <div className="ci-tile flex items-center justify-center flex-shrink-0 font-semibold text-white uppercase" style={style} aria-hidden="true">
      {glyph ?? (name.trim().charAt(0) || "•")}
    </div>
  );
}

/** The invitation tile: the calendar glyph on the calendar's blue. */
export function CalendarAppIcon() {
  return <AppIcon glyph={<CalendarIcon size={20} strokeWidth={2.2} />} tint={color.accent} />;
}

/**
 * The unseen count. A calm 8px dot for one; a tinted capsule with the count from two ("9+" past
 * nine). Both live in the same 20px-high slot, so 1 → 2 never changes what is around it. The
 * digits are the micro role (11 semibold), tabular. It pops in once (opacity only under reduced
 * motion); nothing loops.
 */
export function CountBadge({ count, className = "", style }: { count: number; className?: string; style?: CSSProperties }) {
  const reducedMotion = useReducedMotion() ?? false;
  if (count <= 0) return null;
  const isDot = count === 1;
  const width = isDot ? badge.size : count > 9 ? badge.wide : badge.size;
  return (
    <span className={`ci-badge flex-shrink-0 flex items-center justify-center ${className}`} style={{ width, height: badge.size, ...style }} aria-hidden="true">
      <motion.span
        key={isDot ? "dot" : "count"}
        className="flex items-center justify-center rounded-full tabular-nums text-micro"
        style={
          isDot
            ? { width: badge.dot, height: badge.dot, background: color.accent }
            : { width, height: badge.size, background: color.accentSoft, color: color.accentOnSoft, lineHeight: `${badge.size}px` }
        }
        initial={reducedMotion ? { opacity: 0 } : { scale: 0.6, opacity: 0 }}
        animate={reducedMotion ? { opacity: 1 } : { scale: 1, opacity: 1 }}
        transition={reducedMotion ? { duration: 0.12, ease: "easeOut" } : springConfig.island}
      >
        {isDot ? null : count > 9 ? "9+" : count}
      </motion.span>
    </span>
  );
}

/** Width the CountBadge takes (0 without one): the compact island reserves it before measuring. */
export function countBadgeWidth(count: number): number {
  if (count <= 0) return 0;
  return count > 9 ? badge.wide : badge.size;
}
