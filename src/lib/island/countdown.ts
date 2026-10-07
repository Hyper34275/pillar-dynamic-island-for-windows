// The one countdown to a meeting's start, for every surface that says "in N min" about it: the
// closed island's status, the meeting reminder, the screen-reader text. Before this the reminder
// rounded the minutes once when it fired while the closed island rounded up every minute, so the
// two could say 3 and 4 about the same meeting. Pure; callers pass the same clock (useMinute).

export type MeetingCountdown = { kind: "upcoming"; minutes: number } | { kind: "started" };

/**
 * Whole minutes left, rounded up: 4:00 → 4, 3:59 → 4, 3:01 → 4, 3:00 → 3, 0:01 → 1. From the
 * start on (0:00 and later) the meeting has started. A missing / invalid start counts as started.
 */
export function meetingCountdown(startMs: number, nowMs: number): MeetingCountdown {
  const left = startMs - nowMs;
  if (!(left > 0)) return { kind: "started" };
  return { kind: "upcoming", minutes: Math.ceil(left / 60_000) };
}

/** The countdown in minutes (0 once the meeting has started). */
export function minutesUntil(startMs: number, nowMs: number): number {
  const countdown = meetingCountdown(startMs, nowMs);
  return countdown.kind === "upcoming" ? countdown.minutes : 0;
}
