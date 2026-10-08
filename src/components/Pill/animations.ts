// Motion + geometry tokens for the island: the one place its timing is decided.
//
// Three springs drive everything that moves, all solved in closed form every frame of motion's
// frame loop (lib/island/spring.ts, useIslandMotion.ts): the island's shape (open, close,
// alerts, toasts: islandSprings), the tab capsule and the tab content's progress (tabLayers.ts),
// and a toast's payload handoff (toastHandoff.ts).
// Each retargets from its current position and velocity, so any interruption bends the motion
// instead of restarting it. Content has no clock of its own: it fades by how far its driver has
// travelled, on the windows in `layerFade` / `partFade` (lib/island/morph.ts). Every spring
// settles on its own and stops its frame loop; nothing here animates forever, and no timer or
// delay takes part in any of it.
//
// REDUCED_MOTION (OS "show animations" off): a profile of its own, not "the same with a few
// decorations off": the shape morphs on tighter critically damped springs (islandSpringsReduced),
// the tab content and toast payloads hand over on faster clocks with no sideways / vertical drift,
// the panel body does not scale, and the press scale and the launch pop are dropped. What stays:
// the change of shape and the hand-overs, with the same ownership rules (one owner at a time, at
// most two layers, the latest target wins). Correctness never depends on any of it.

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
 * The shape under reduced motion: still a continuous change of shape (it is how the island says
 * what it turned into), but critically damped, tighter and with no width-before-height lag: less
 * travel time, no settle at all. Content keeps the same ownership rules (it reads off the same
 * progress), without drift or scale.
 */
export const islandSpringsReduced: { width: SpringParams; height: SpringParams } = {
  width: { response: 0.24, dampingFraction: 1 },
  height: { response: 0.24, dampingFraction: 1 },
};


/**
 * When each layer of island content fades, as fractions of the island's progress (0: the shape
 * starts moving, 1: it has arrived). Read off ONE driver (the island's animated size), so the
 * shell and every layer agree in every frame, an interrupted morph simply continues from what is
 * on screen, and there is no timer to race. The rules (the "ownership" of the island's content):
 *  - one owner at a time: what leaves is faint before what arrives becomes readable, so the
 *    compact content and the open panel (or a toast) are never both readable;
 *  - no near-empty shape: the gap between the last owner fading and the next arriving is a few
 *    hundredths of progress (one frame at most);
 *  - no tiny compact content inside a large shape: closing, the compact content only arrives in
 *    the last ~25 % of the morph (the shape is then within ~100 px of the pill), while the
 *    panel's body holds until then (masked between the header and dock rows, see IslandPart); the
 *    body leaves a little after the compact content starts (0.9 vs 0.74), so the hand-over never
 *    dips below ~0.25 of the island's content (lib/island/morph.test.ts keeps that floor);
 *  - opening, the compact content is gone by a quarter of the way, the header takes the top row
 *    (from the leading edge), the body establishes itself once there is room, the dock settles last.
 * Which window a layer ARRIVES on depends on what it replaces (entryFade): the compact content
 * coming back from a toast can arrive much earlier than from the 400x440 panel, because the
 * toast's shape is already almost a pill.
 */
export const layerFade = {
  /** Compact content: after the open panel (the default), arriving late; leaving early when anything opens. */
  compact: { in: [0.74, 0.97], out: 0.25 },
  /** The expanded layer only times its own removal (the slowest part's `out`); its parts fade on `partFade`. */
  expanded: { in: [0, 0], out: 0.9 },
  /**
   * Meeting alert, notification toast, ring/silent pill: the shape opens first, then the content;
   * they leave by half way, as the compact content arrives (entryFade.compact.fromTemporary).
   */
  temporary: { in: [0.35, 0.9], out: 0.5 },
} as const satisfies Record<string, Fade>;

/**
 * The window a layer arrives on, by what the island showed before. Leaving always uses the
 * layer's own `out` (layerFade): this only shifts arrivals so that the hand-over is strict for
 * every pair (panel ↔ compact, toast ↔ compact, panel → alert).
 */
export const entryFade = {
  compact: {
    fromExpanded: layerFade.compact,
    fromTemporary: { in: [0.36, 0.9], out: layerFade.compact.out },
  },
  temporary: {
    /** Toast / alert / ring pill opening from the pill: the compact content is gone by 0.25, the toast takes over from 0.15. */
    fromCompact: { in: [0.15, 0.8], out: layerFade.temporary.out },
    /** A meeting alert over the open panel: the panel's body holds until 0.84, so the alert waits for it. */
    fromExpanded: { in: [0.7, 0.98], out: layerFade.temporary.out },
  },
} as const satisfies Record<string, Record<string, Fade>>;

/** What the island showed before the transition now running: picks entryFade. */
export type IslandOrigin = "boot" | "compact" | "expanded" | "temporary";

/** The arrival window of a layer `kind` after `origin`. */
export function arrivalFade(kind: "compact" | "temporary", origin: IslandOrigin): Fade {
  if (kind === "compact") return origin === "temporary" ? entryFade.compact.fromTemporary : entryFade.compact.fromExpanded;
  return origin === "expanded" ? entryFade.temporary.fromExpanded : entryFade.temporary.fromCompact;
}

/**
 * The expanded island's parts, each riding its own edge of the shape (see IslandPart): the
 * header with the top / leading corner, the body right under it (masked above the dock, with a
 * slight scale from its top that supports its fade), the dock with the bottom edge. Opening: the
 * header takes the top row first (0.16); once there is room the body and the dock arrive together
 * (the dock covers the body's only cut line, at its bottom). Closing: the header leaves the top
 * row first, the dock holds a little longer (covering the body's cut while the body is still
 * readable), and the body goes last, handing over to the compact content at ~0.8.
 */
export const partFade = {
  header: { in: [0.16, 0.56], out: 0.5 },
  body: { in: [0.4, 0.85], out: 0.9 },
  dock: { in: [0.4, 0.85], out: 0.6 },
} as const satisfies Record<string, Fade>;

/** The body's scale at the start of its fade in / end of its fade out (supports the fade; 1 at rest). */
export const BODY_ENTRY_SCALE = 0.965;

/**
 * Content is laid out once at its final size and never reflows; while the shape is narrower than
 * it (opening, or a toast growing to a wider one) it is scaled uniformly to the shape's width,
 * never below this, so its own padding absorbs the difference: an app icon at a toast's edge, the
 * outer dock tabs, the panel's cards are never cut by the shape's sides (no crop reveal). Uniform
 * and at most 15 %: never a squash, and 1 at rest.
 */
export const FIT_MIN_SCALE = 0.85;

/** The fit scale for content `contentWidth` wide in a shape `shapeWidth` wide. */
export function fitScale(shapeWidth: number, contentWidth: number): number {
  if (!(contentWidth > 0)) return 1;
  return Math.min(1, Math.max(FIT_MIN_SCALE, shapeWidth / contentWidth));
}

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
