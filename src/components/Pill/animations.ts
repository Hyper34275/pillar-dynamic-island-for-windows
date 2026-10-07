// Motion + geometry tokens for the island: the one place its timing is decided.
//
// Three springs drive everything that moves, all solved in closed form every frame of motion's
// frame loop (lib/island/spring.ts, useIslandMotion.ts): the island's shape (open, close,
// alerts, toasts: islandSprings), the tab capsule and the tab content's progress (tabSprings).
// Each retargets from its current position and velocity, so any interruption bends the motion
// instead of restarting it. Content has no clock of its own: it fades by how far its driver has
// travelled, on the windows in `layerFade` / `partFade` (lib/island/morph.ts). Every spring
// settles on its own and stops its frame loop; nothing here animates forever, and no timer or
// delay takes part in any of it.
//
// REDUCED_MOTION (OS "show animations" off): like motion's own reducedMotion="user" policy,
// movement that only decorates (the tab content's sideways shift, the press scale, the launch
// pop) is dropped, while the island's change of shape and the cross-fades stay, since they are
// how the island shows what it turned into. Correctness never depends on any of it.

import type { Fade } from "../../lib/island/morph";
import { getIslandLimits, type IslandLimits } from "../../lib/island/limits";
import type { SpringParams } from "../../lib/island/spring";
import { alert, card, compact as compactTokens, control, dock, panel, ringer as ringerTokens, smallExpanded, type } from "../../design/tokens";

export const springConfig = {
  // The unseen indicator's pop and the launch dot's entrance (motion springs, not geometry).
  island: {
    type: "spring" as const,
    stiffness: 400,
    damping: 36,
    mass: 1,
  },
};

/**
 * The island's width and height (Apple's response/dampingFraction). Slightly underdamped for a
 * tight, organic landing, but the geometry engine stops each axis at its target instead of
 * crossing it, so the window shape never overshoots. Height has a little more "mass" than
 * width: the island widens a hair ahead of dropping open, and draws its sides in a hair ahead
 * of lifting closed. Within a pixel of the target in ~0.35 s.
 */
export const islandSprings: { width: SpringParams; height: SpringParams } = {
  width: { response: 0.3, dampingFraction: 0.92 },
  height: { response: 0.34, dampingFraction: 0.92 },
};

/**
 * Tab changes. The capsule (the one selection highlight) is the primary feedback: it starts on
 * the next frame and is ~80 % of the way within 0.1 s, never swinging into the next tab. The
 * content's cross-fade runs on its own, slightly slower progress spring that starts in the same
 * frame, so the new content is never ahead of the capsule, and its duration does not depend on
 * how many tabs the capsule travels.
 */
export const tabSprings: { capsule: SpringParams; content: SpringParams } = {
  capsule: { response: 0.22, dampingFraction: 1 },
  content: { response: 0.36, dampingFraction: 1 },
};

/**
 * When each layer of island content fades, as fractions of the island's progress (0: the
 * shape starts moving, 1: it has arrived). Every layer rides the vertical centre of the shape
 * (see IslandLayer), so content never floats in a corner of a large empty shape. What leaves
 * and what arrives overlap, so no frame is an empty black shape:
 *  - opening, the compact content is gone by 0.3 while the expanded header is already coming in;
 *  - closing, the expanded body holds (fading) until 0.9 and the compact content arrives from 0.72,
 *    once the shape is small enough to be read as the pill it is becoming. Measured in the real
 *    WebView2 (Oct 2026): with the earlier 0.42 the date showed at 50 % in a 260x217 shape; now it
 *    starts at ~220x140 and is at two thirds by ~200x115, and no frame drops below ~29 % content.
 */
export const layerFade = {
  compact: { in: [0.72, 0.97], out: 0.3 },
  /** The expanded layer only times its own removal; its parts fade on `partFade`. */
  expanded: { in: [0, 0], out: 0.9 },
  /**
   * Meeting alert, notification toast, ring/silent pill: the shape opens first, then the content.
   * A toast replacing a toast changes only the width, so the two overlap (in from 0.35, out by
   * 0.55): measured in the real WebView2, the earlier 0.45/0.45 left one near-empty frame.
   */
  temporary: { in: [0.35, 0.9], out: 0.55 },
  /** Tab title and panel, on the tab content's progress. */
  tab: { in: [0.2, 0.9], out: 0.5 },
} as const satisfies Record<string, Fade>;

/**
 * The expanded island's parts, each riding its own edge of the shape (see IslandPart): the
 * header with the top-left corner, the dock with the bottom edge, the body in the middle.
 * Opening, space comes first, then the header, the body, and the dock last; closing, the dock
 * goes first and the body last, so the shape always has content until the compact one arrives.
 */
export const partFade = {
  header: { in: [0.15, 0.6], out: 0.6 },
  body: { in: [0.35, 0.85], out: 0.9 },
  dock: { in: [0.4, 0.9], out: 0.5 },
} as const satisfies Record<string, Fade>;

/** How far (px) tab content shifts with the direction of the tab change: a hint, not a slide. */
export const TAB_SHIFT_PX = 6;

/** Tab label colour change: as long as the indicator's visible travel, same ease-out feel. */
export const TAB_LABEL_TRANSITION = "color 200ms cubic-bezier(0.22, 1, 0.36, 1)";

export const bootAnimationDuration = {
  dotAppear: 200,
  morphToPill: 500,
};

// Island geometry. Logical pixels (Tauri converts with the window's scale factor). Every number
// comes from the design tokens (src/design/tokens.ts); this file only composes them into the
// shapes the island morphs between.
export const pillDimensions = {
  boot: { width: 8, height: 8, radius: 4 },
  compact: {
    height: compactTokens.height,
    paddingX: compactTokens.paddingX,
    /** Space between separate labels (date, clock, weekday). */
    gap: compactTokens.gap,
    gapFull: compactTokens.gap,
    minWidth: compactTokens.minWidth,
    clockMinWidth: compactTokens.clockMinWidth,
    maxWidth: compactTokens.maxWidth,
  },
  expanded: { width: panel.width, height: panel.height, radius: panel.radius },
  /** The ring / silent pill at the start of a meeting: the compact island's height, so it is the same object. */
  ringer: { width: ringerTokens.width, height: ringerTokens.height },
  /** The collapsed island while a meeting is about to start or running. */
  compactMeeting: { maxWidth: compactTokens.maxWidth },
} as const;

// Pixel model. Every size and position in the frontend is a floating-point number of DIPs: no
// rounding to a "nice" or even width (an even DIP is not even a whole pixel at 125%: 2 DIP is
// 2.5 px). The only places a length is rounded up in JS are measured text widths (ceil), where
// rounding down would clip the last glyph. Pixels are snapped ONCE, in Rust (monitors.rs), by
// edges at the target monitor's real DPI, so the island's centre never drifts.

/**
 * Gap (DIPs) between the screen's top edge and the island. Must equal `ISLAND_TOP_INSET` in
 * src-tauri/src/monitors.rs (a test reads that file and compares). The stage window starts at the
 * screen's top; the island is drawn this far down inside the page, and the window region adds a
 * bridge over the gap so a pointer thrown against the top edge still lands on the island.
 */
export const ISLAND_TOP_INSET = 8;

/**
 * The least height (DIPs) the panel keeps when a small screen limits it: its header, the dock
 * and one card of the body (padding plus a headline and a body line), with the panel's own
 * padding and gaps. Below this the panel stops being usable, so this wins over the monitor's
 * limit (the backend still clamps the stage window to the monitor itself).
 */
export const PANEL_MIN_HEIGHT =
  panel.paddingTop +
  panel.headerHeight +
  panel.headerGap +
  (card.padding * 2 + type.headline.lineHeight + type.body.lineHeight) +
  panel.dockGap +
  dock.height +
  panel.paddingBottom;

export interface IslandSize {
  width: number;
  height: number;
  radius: number;
}

/** A shape no larger than the monitor allows (an alert or toast on a very small screen). */
export function limitSize(size: IslandSize, limits: IslandLimits = getIslandLimits()): IslandSize {
  const width = Math.min(size.width, limits.maxWidth);
  const height = Math.min(size.height, limits.maxHeight);
  return width === size.width && height === size.height ? size : { width, height, radius: Math.min(size.radius, height / 2) };
}

/** A capsule of the compact island's height around content of `contentWidth` (padding included here). */
export function compactSize(
  contentWidth: number,
  maxWidth: number = pillDimensions.compact.maxWidth,
  minWidth: number = pillDimensions.compact.minWidth
): IslandSize {
  const c = pillDimensions.compact;
  return { width: Math.min(maxWidth, Math.max(minWidth, contentWidth + c.paddingX * 2)), height: c.height, radius: c.height / 2 };
}

/**
 * The panel: 400x440 (tokens.panel) is the PREFERRED size. On a smaller screen it shrinks to the
 * monitor's limits: the width to what the monitor allows, the height likewise but never below
 * PANEL_MIN_HEIGHT. The header and the dock keep their sizes; the scrolling body absorbs the
 * change. Without an argument it uses the shared limits (lib/island/limits.ts), which is how
 * PanelFrame and PillShell agree.
 */
export function expandedSize(limits: IslandLimits = getIslandLimits()): IslandSize {
  const { width, height, radius } = pillDimensions.expanded;
  return { width: Math.min(width, limits.maxWidth), height: Math.min(height, Math.max(PANEL_MIN_HEIGHT, limits.maxHeight)), radius };
}

/**
 * The meeting alert grows with its content. Rhythm (tokens.alert): context label, 4, subject (1-2
 * lines), 4, time, 2, place (optional), 16, action row (optional); 16 padding all round.
 */
export function meetingAlertSize(subjectLines: 1 | 2, hasLocation: boolean, hasActions = false): IslandSize {
  const p = smallExpanded.padding;
  let height = p + type.meta.lineHeight + alert.labelGap + type.title.lineHeight * subjectLines + alert.labelGap + type.body.lineHeight;
  if (hasLocation) height += alert.detailGap + type.body.lineHeight;
  if (hasActions) height += alert.actionsGap + control.height;
  height += p;
  return { width: smallExpanded.width, height, radius: Math.min(smallExpanded.radius, height / 2) };
}

export function ringerSize(): IslandSize {
  const r = pillDimensions.ringer;
  return { width: r.width, height: r.height, radius: r.height / 2 };
}
