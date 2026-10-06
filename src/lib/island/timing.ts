// How long the island's temporary states last and how the pointer drives the user layer.

/** Meeting alert: long enough to read, short enough to stay out of the way. Hover pauses it. */
export const ALERT_MS = 8_000;
/** Notification toast. Hover pauses it. */
export const NOTIFICATION_MS = 4_500;
/** The pointer must rest on the island this long before it expands (a fly-by does nothing). */
export const HOVER_INTENT_MS = 120;
/**
 * Grace after the pointer leaves an expanded island it was on, clicked or not. Short so that
 * moving away visibly closes it; re-entering within it cancels the collapse.
 */
export const LEAVE_COLLAPSE_MS = 400;
/**
 * An island opened while the pointer was elsewhere (tray, second launch) and the pointer has not
 * reached it yet: time to get there before it closes on its own.
 */
export const UNATTENDED_COLLAPSE_MS = 4_000;
/**
 * Another window becoming active (a click on the desktop, the taskbar or another app) closes the
 * island at once, except this soon after it opened: opening from the tray or a second launch
 * shuffles the foreground window itself.
 */
export const FOREGROUND_GRACE_MS = 700;
