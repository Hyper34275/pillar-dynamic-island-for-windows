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
import type { SpringParams } from "../../lib/island/spring";

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
 *  - closing, the expanded body holds until 0.6 and the compact content arrives from 0.42,
 *    once the shape is small enough to be read as the pill it is becoming.
 */
export const layerFade = {
  compact: { in: [0.42, 0.95], out: 0.3 },
  /** The expanded layer only times its own removal; its parts fade on `partFade`. */
  expanded: { in: [0, 0], out: 0.6 },
  /** Meeting alert, notification toast, ring/silent pill: the shape opens first, then the content. */
  temporary: { in: [0.45, 0.95], out: 0.45 },
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
  header: { in: [0.15, 0.6], out: 0.5 },
  body: { in: [0.35, 0.85], out: 0.6 },
  dock: { in: [0.4, 0.9], out: 0.4 },
} as const satisfies Record<string, Fade>;

/** How far (px) tab content shifts with the direction of the tab change: a hint, not a slide. */
export const TAB_SHIFT_PX = 6;

/** Tab label colour change: as long as the indicator's visible travel, same ease-out feel. */
export const TAB_LABEL_TRANSITION = "color 200ms cubic-bezier(0.22, 1, 0.36, 1)";

export const bootAnimationDuration = {
  dotAppear: 200,
  morphToPill: 500,
};

// Logical pixels: Tauri converts to physical using the window's scale factor.
export const pillDimensions = {
  boot: { width: 8, height: 8, radius: 4 },
  compact: {
    height: 34,
    paddingX: 15,
    /** Minimum space between the date and the weekday (the display "date"). */
    gap: 14,
    /** Space between the date, the clock and the weekday (the display "full"). */
    gapFull: 10,
    minWidth: 112,
    /** The display "clock" holds one short label, so it may be narrower. */
    clockMinWidth: 88,
    maxWidth: 220,
  },
  expanded: { width: 404, height: 420, radius: 40 },
  /** Meeting alert: the label, the subject (1-2 lines), the time range and an optional location. */
  alert: {
    width: 380,
    paddingX: 22,
    paddingY: 16,
    labelHeight: 14,
    subjectLineHeight: 22,
    detailHeight: 18,
    /** Space between stacked rows. */
    gap: 5,
    maxRadius: 30,
  },
  /**
   * Windows notification mirrored into the island; an invitation adds a row of answer buttons.
   * With a body, the height holds the toast's tallest text block inside its 12px margins: app
   * label 14 + title 20 + two body lines 36 = 70, plus 24 of margin = 94 (an invitation adds its
   * 8px gap and 26px buttons, which the 34 of actionsHeight covers).
   */
  notification: { width: 372, heightWithBody: 94, height: 64, actionsHeight: 34, maxRadius: 30 },
  /** The ring / silent pill at the start of a meeting. */
  ringer: { width: 200, height: 48 },
  /** Join / snooze buttons under a meeting alert. */
  alertActions: { height: 30, gap: 10 },
  /** The collapsed island while a meeting is about to start or running (it may be wider). */
  compactMeeting: { maxWidth: 300 },
  /**
   * The unseen-notifications indicator, last in the collapsed island's row: a dot for one, a
   * tinted capsule with the count from two (widths are fixed per state, never measured).
   */
  badge: { dot: 8, count: 16, countWide: 24, height: 16, gap: 8 },
} as const;

export interface IslandSize {
  width: number;
  height: number;
  radius: number;
}

export function compactSize(
  contentWidth: number,
  maxWidth: number = pillDimensions.compact.maxWidth,
  minWidth: number = pillDimensions.compact.minWidth
): IslandSize {
  const c = pillDimensions.compact;
  return { width: Math.min(maxWidth, Math.max(minWidth, contentWidth + c.paddingX * 2)), height: c.height, radius: c.height / 2 };
}

/** Room the unseen indicator takes inside the collapsed island, gap included (0 without one). */
export function badgeWidth(unseen: number): number {
  const b = pillDimensions.badge;
  if (unseen <= 0) return 0;
  return b.gap + (unseen === 1 ? b.dot : unseen <= 9 ? b.count : b.countWide);
}

export function expandedSize(): IslandSize {
  return { ...pillDimensions.expanded };
}

/** The meeting alert grows with its content: 1 or 2 subject lines, a location row if there is one, and a row of buttons (join, snooze) when there is one. */
export function meetingAlertSize(subjectLines: 1 | 2, hasLocation: boolean, hasActions = false): IslandSize {
  const a = pillDimensions.alert;
  const rows = 3 + (hasLocation ? 1 : 0); // label, subject, time (+ location)
  const actions = hasActions ? pillDimensions.alertActions.gap + pillDimensions.alertActions.height : 0;
  const height = a.paddingY * 2 + a.labelHeight + a.subjectLineHeight * subjectLines + a.detailHeight * (rows - 2) + a.gap * (rows - 1) + actions;
  return { width: a.width, height, radius: Math.min(a.maxRadius, height / 2) };
}

export function notificationSize(hasBody: boolean, hasActions = false): IslandSize {
  const n = pillDimensions.notification;
  const height = (hasBody ? n.heightWithBody : n.height) + (hasActions ? n.actionsHeight : 0);
  return { width: n.width, height, radius: Math.min(n.maxRadius, height / 2) };
}

export function ringerSize(): IslandSize {
  const r = pillDimensions.ringer;
  return { width: r.width, height: r.height, radius: r.height / 2 };
}
