// How long the island's temporary states last and how the pointer drives the user layer.

import type { IslandNotification } from "../ipc";

/** Meeting alert: long enough to read, short enough to stay out of the way. Hover pauses it. */
export const ALERT_MS = 8_000;
/** A lone toast stays this long (hover pauses it); with the grace after it, the same total as before sessions. */
export const NOTIFICATION_DWELL_MS = 3_500;
/** Dwell with up to 3 notifications pending behind it. */
export const NOTIFICATION_DWELL_QUEUED_MS = 2_800;
/** Dwell with 4 to 6 pending. */
export const NOTIFICATION_DWELL_BUSY_MS = 2_200;
/** Dwell with more than 6 pending: the readability floor, never shorter. */
export const NOTIFICATION_DWELL_BACKLOG_MS = 1_800;
/** A meeting invitation has buttons to press: long alone, shorter (but still readable) with others waiting. */
export const INVITE_TOAST_MS = 9_000;
export const INVITE_TOAST_QUEUED_MS = 5_000;
/** The island's own "You missed N notifications" summary: a headline, read at a glance. */
export const MISSED_SUMMARY_MS = 2_500;
/**
 * After the last toast's dwell with nothing pending the island stays open this long, still showing
 * it: a notification arriving in it is shown at once, with no close and reopen in between.
 */
export const NOTIFICATION_GRACE_MS = 1_000;
/** A lone toast's whole time on screen: its dwell plus the grace (what a toast always took). */
export const NOTIFICATION_MS = NOTIFICATION_DWELL_MS + NOTIFICATION_GRACE_MS;

/** How long `notification` is read before the next one (or the grace) follows, given how many wait behind it. */
export function notificationDwellMs(notification: IslandNotification, pendingCount: number): number {
  if (notification.missedSummary) return MISSED_SUMMARY_MS;
  if (notification.invite) return pendingCount > 0 ? INVITE_TOAST_QUEUED_MS : INVITE_TOAST_MS;
  if (pendingCount <= 0) return NOTIFICATION_DWELL_MS;
  if (pendingCount <= 3) return NOTIFICATION_DWELL_QUEUED_MS;
  if (pendingCount <= 6) return NOTIFICATION_DWELL_BUSY_MS;
  return NOTIFICATION_DWELL_BACKLOG_MS;
}
/** A smart-search answer (or error) stays this long unattended; hover pauses it. */
export const ASSISTANT_ANSWER_MS = 12_000;
/** A question card ("which mailbox?") waits this long for a click, then goes (typing in the search bar still answers it). */
export const ASSISTANT_CHOICES_MS = 60_000;
/**
 * A "working" card has no timer of its own: the backend always ends it with an answer or an
 * error. This is only the backstop for a backend that never does (search budget 10 s + 10 s
 * extension, IPC timeout 45 s).
 */
export const ASSISTANT_PROCESSING_MAX_MS = 90_000;

/** How long a smart-search card of `phase` stays on screen when nobody touches it. */
export function assistantDwellMs(phase: "processing" | "answer" | "choices" | "error"): number {
  if (phase === "choices") return ASSISTANT_CHOICES_MS;
  if (phase === "processing") return ASSISTANT_PROCESSING_MAX_MS;
  return ASSISTANT_ANSWER_MS;
}
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
