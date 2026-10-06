// Motion + geometry tokens for the island: the one place its timing is decided.
//
// Two springs drive everything that moves: the island's size (open, close, alerts, toasts) and
// the tab dock indicator (tab changes). Content has no clock of its own; it fades by how far its
// driver has travelled, on the windows in `layerFade` (see lib/island/morph.ts). All springs
// settle on their own (motion stops the frame loop at rest), and nothing here animates forever.
//
// REDUCED_MOTION (OS "show animations" off): like motion's own reducedMotion="user" policy,
// movement that only decorates (the tab content slide, the press scale, the launch pop) is
// dropped, while the island's change of size and the cross-fades stay, since they are how the
// island shows what it turned into. Correctness never depends on any of it: jumping the drivers
// (MotionValue.jump) completes every transition in one frame.

import type { Fade } from "../../lib/island/morph";

export const springConfig = {
  // The island morph: width, height and corner radius together. Damping ratio 0.9, so it lands
  // in ~0.28 s with under a pixel of overshoot: precise, no visible bounce.
  island: {
    type: "spring" as const,
    stiffness: 400,
    damping: 36,
    mass: 1,
  },
  // The dock indicator, which also drives the tab content crossfade: critically damped, ~0.2 s.
  tab: {
    type: "spring" as const,
    stiffness: 500,
    damping: 45,
    mass: 1,
  },
};

/**
 * When each kind of content fades during a transition, as fractions of the transition's
 * progress (0: the driver starts moving, 1: it has arrived). What leaves and what arrives
 * overlap, so the island is never an empty shape, and content that needs room (the expanded
 * panel) only shows once the island has grown enough to hold it.
 */
export const layerFade = {
  compact: { in: [0.3, 0.85], out: 0.4 },
  expanded: { in: [0.2, 0.75], out: 0.7 },
  /** Meeting alert and notification toast. */
  temporary: { in: [0.45, 0.9], out: 0.45 },
  tab: { in: [0.15, 0.85], out: 0.45 },
} as const satisfies Record<string, Fade>;

/** How far (px) tab content slides with the direction of the tab change. */
export const TAB_SHIFT_PX = 8;

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
    /** Minimum space between the date and the weekday. */
    gap: 14,
    minWidth: 112,
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
  /** Windows notification mirrored into the island; an invitation adds a row of answer buttons. */
  notification: { width: 372, heightWithBody: 88, height: 64, actionsHeight: 34, maxRadius: 30 },
  /** The ring / silent pill at the start of a meeting. */
  ringer: { width: 200, height: 48 },
  /** Join / snooze buttons under a meeting alert. */
  alertActions: { height: 30, gap: 10 },
  /** The collapsed island while a meeting is about to start or running (it may be wider). */
  compactMeeting: { maxWidth: 300 },
  /** The unseen-notifications count inside the collapsed island. */
  badge: { size: 16, wide: 22, gap: 8 },
} as const;

export interface IslandSize {
  width: number;
  height: number;
  radius: number;
}

export function compactSize(contentWidth: number, maxWidth: number = pillDimensions.compact.maxWidth): IslandSize {
  const c = pillDimensions.compact;
  return { width: Math.min(maxWidth, Math.max(c.minWidth, contentWidth + c.paddingX * 2)), height: c.height, radius: c.height / 2 };
}

/** Room the unseen count takes inside the collapsed island (0 without one). */
export function badgeWidth(unseen: number): number {
  const b = pillDimensions.badge;
  return unseen > 0 ? b.gap + (unseen > 9 ? b.wide : b.size) : 0;
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
