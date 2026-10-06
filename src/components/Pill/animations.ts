// Motion + geometry tokens for the island. All springs settle on their own (motion stops
// the frame loop at rest), and nothing here animates forever.

export const springConfig = {
  // The island morph: fast, with a small, confident overshoot.
  island: {
    type: "spring" as const,
    stiffness: 420,
    damping: 32,
    mass: 0.95,
  },
  // Reduced motion: effectively instant, no overshoot.
  instant: {
    stiffness: 1000,
    damping: 100,
    mass: 0.1,
  },
  // Entrance of the pill itself at launch.
  entrance: {
    type: "spring" as const,
    stiffness: 260,
    damping: 22,
    mass: 1,
  },
};

export const PILL_DURATION_FAST = 0.15;

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
  /** Windows notification mirrored into the island. */
  notification: { width: 372, heightWithBody: 88, height: 64, maxRadius: 30 },
} as const;

export interface IslandSize {
  width: number;
  height: number;
  radius: number;
}

export function compactSize(contentWidth: number): IslandSize {
  const c = pillDimensions.compact;
  return { width: Math.min(c.maxWidth, Math.max(c.minWidth, contentWidth + c.paddingX * 2)), height: c.height, radius: c.height / 2 };
}

export function expandedSize(): IslandSize {
  return { ...pillDimensions.expanded };
}

/** The meeting alert grows with its content: 1 or 2 subject lines, and a location row if there is one. */
export function meetingAlertSize(subjectLines: 1 | 2, hasLocation: boolean): IslandSize {
  const a = pillDimensions.alert;
  const rows = 3 + (hasLocation ? 1 : 0); // label, subject, time (+ location)
  const height = a.paddingY * 2 + a.labelHeight + a.subjectLineHeight * subjectLines + a.detailHeight * (rows - 2) + a.gap * (rows - 1);
  return { width: a.width, height, radius: Math.min(a.maxRadius, height / 2) };
}

export function notificationSize(hasBody: boolean): IslandSize {
  const n = pillDimensions.notification;
  const height = hasBody ? n.heightWithBody : n.height;
  return { width: n.width, height, radius: Math.min(n.maxRadius, height / 2) };
}
