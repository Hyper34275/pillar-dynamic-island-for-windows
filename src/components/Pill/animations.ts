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
    hoverHeight: 38,
    paddingX: 15,
    /** Minimum space between the date and the weekday. */
    gap: 14,
    minWidth: 112,
    maxWidth: 220,
    hoverGrow: 12,
  },
  expanded: { width: 404, height: 420, radius: 40 },
} as const;

/** Window slack around the island, so edge antialiasing and the hover growth are never clipped. */
export const windowMargin = {
  collapsedX: 12,
  collapsedY: 4,
  expandedX: 16,
  expandedY: 10,
  /** Notification toast: gap + toast height + breathing room, and a width that contains the toast. */
  toastY: 160,
  toastWidth: 420,
} as const;

export interface IslandSize {
  width: number;
  height: number;
  radius: number;
}

export function compactSize(contentWidth: number, hover: boolean): IslandSize {
  const c = pillDimensions.compact;
  const base = Math.min(c.maxWidth, Math.max(c.minWidth, contentWidth + c.paddingX * 2));
  const height = hover ? c.hoverHeight : c.height;
  return { width: base + (hover ? c.hoverGrow : 0), height, radius: height / 2 };
}

export function expandedSize(): IslandSize {
  return { ...pillDimensions.expanded };
}
