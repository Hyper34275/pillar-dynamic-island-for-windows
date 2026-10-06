import { useEffect, useRef } from "react";
import { dayLabel, formatTime } from "../lib/dateFormat";
import { t } from "../lib/i18n";
import type { IslandNotification } from "../lib/ipc";
import type { MeetingInviteDto } from "../lib/calendar/types";
import { useCalendarService } from "./useCalendar";

/** An invite received this long before the app started still pops up (sign-in, restart). Older ones were there before us. */
export const INVITE_STARTUP_GRACE_MS = 10 * 60_000;
/** Remembered invite ids; far above anything real, bounds a runaway producer. */
const SEEN_MAX = 200;

// Windows notification ids are positive; invites count down from -1 so the two never collide.
let nextInviteId = -1;

/** "Tomorrow, 21:30 – 22:00 · From Dana" — whatever of it Outlook could tell. */
export function inviteBody(invite: MeetingInviteDto, nowMs: number): string {
  const parts: string[] = [];
  if (invite.startUtc && invite.endUtc) {
    const start = new Date(invite.startUtc);
    const range = `${formatTime(start)} – ${formatTime(new Date(invite.endUtc))}`;
    const day = dayLabel(start, nowMs);
    parts.push(day ? `${day}, ${range}` : range);
  }
  if (invite.organizer) parts.push(t("invite.from", { name: invite.organizer }));
  else if (invite.location) parts.push(invite.location);
  return parts.join(" · ");
}

export function inviteNotification(invite: MeetingInviteDto, nowMs: number): IslandNotification {
  return {
    id: nextInviteId--,
    appName: "Outlook",
    title: invite.subject.trim() || t("calendar.noSubject"),
    body: inviteBody(invite, nowMs),
    timestamp: nowMs,
    aumid: null,
    invite: { id: invite.id, startUtc: invite.startUtc },
  };
}

/**
 * New Outlook meeting requests, handed to `onReceived` as island notifications. Each invite pops
 * up once per app session, and only if it arrived after the app started (minus a short grace):
 * the unread requests already sitting in the Inbox are not news. `enabled` is null until the
 * settings have loaded; nothing is decided before that. Invites that come in while disabled are
 * remembered as seen, so turning the setting on does not replay them.
 */
export function useMeetingInvites(enabled: boolean | null, onReceived: (notification: IslandNotification) => void): void {
  const service = useCalendarService();
  const startedAt = useRef(Date.now());
  const seen = useRef<string[]>([]);
  const onReceivedRef = useRef(onReceived);
  onReceivedRef.current = onReceived;

  useEffect(() => {
    if (enabled === null) return;
    const check = () => {
      const fresh: MeetingInviteDto[] = [];
      for (const invite of service.getSnapshot().invites) {
        if (seen.current.includes(invite.id)) continue;
        seen.current = [...seen.current, invite.id].slice(-SEEN_MAX);
        if (enabled && Date.parse(invite.receivedUtc) >= startedAt.current - INVITE_STARTUP_GRACE_MS) fresh.push(invite);
      }
      // Oldest first, so the newest is the one left on screen.
      fresh.sort((a, b) => Date.parse(a.receivedUtc) - Date.parse(b.receivedUtc));
      const now = Date.now();
      for (const invite of fresh) onReceivedRef.current(inviteNotification(invite, now));
    };
    check();
    return service.subscribe(check);
  }, [service, enabled]);
}
