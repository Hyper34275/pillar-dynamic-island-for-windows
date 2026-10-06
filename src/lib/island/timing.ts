// How long the island's temporary states last and how the pointer drives the user layer.

/** Meeting alert: long enough to read, short enough to stay out of the way. Hover pauses it. */
export const ALERT_MS = 8_000;
/** Notification toast. Hover pauses it. */
export const NOTIFICATION_MS = 4_500;
/** The pointer must rest on the island this long before it expands (a fly-by does nothing). */
export const HOVER_INTENT_MS = 120;
/** Grace after the pointer leaves an expanded island that nobody clicked. */
export const LEAVE_COLLAPSE_MS = 500;
/** Grace after the pointer leaves (or never entered) an island opened by a click or toggle. */
export const PINNED_LEAVE_COLLAPSE_MS = 4_000;
