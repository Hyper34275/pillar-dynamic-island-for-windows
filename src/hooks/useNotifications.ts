import { useCallback, useEffect, useRef, useState } from "react";
import { ipc, normalizeNotification, normalizeNotificationStatus, onEvent, type IslandNotification, type NotificationStatus } from "../lib/ipc";
import { doNotDisturb, type DoNotDisturb } from "../lib/island/dnd";
import { silence, type Silence } from "../lib/island/silence";
import { notificationHistory, type NotificationHistory } from "../lib/notifications/history";
import { useMeetingInvites } from "./useMeetingInvites";
import { useSettings } from "./useSettings";

const SEEN_IDS_MAX = 50;

/**
 * Where an arriving notification goes: always to the history; it pops up (`show`) unless the
 * user silenced notifications for the meeting they are in or turned on Windows "Do not disturb"
 * (the bell). Returns whether it popped up.
 */
export function deliverNotification(
  notification: IslandNotification,
  now: number,
  history: NotificationHistory,
  silenceStore: Silence,
  show: (notification: IslandNotification) => void,
  dnd: Pick<DoNotDisturb, "isOn"> = doNotDisturb
): boolean {
  const muted = dnd.isOn();
  const silenced = muted || silenceStore.isSilent(now);
  history.add(notification, now, silenced);
  if (!silenced) show(notification);
  return !silenced;
}

/** What a click on a notification (toast or Notifications tab) does: open its app, or, for an invitation, the Outlook calendar on its day. */
export function activateNotification(notification: IslandNotification): void {
  if (notification.invite) {
    void ipc.outlookOpenCalendar(notification.invite.startUtc);
    return;
  }
  void (notification.aumid ? ipc.activateAppByAumid(notification.aumid) : ipc.activateNotification(notification.id));
}

export interface UseNotificationsResult {
  status: NotificationStatus | null;
  /** Notifications received since the island was last opened. */
  unseen: number;
  markSeen: () => void;
  /** The island closed: what the Notifications tab showed is no longer unread (history.markViewed). */
  markViewed: () => void;
  activate: (notification: IslandNotification) => void;
  requestAccess: () => void;
}

/**
 * Windows toast mirroring. Event-driven only (no polling) and in-memory only: titles and
 * bodies are shown on screen but never persisted and never logged. What to do with a new
 * notification (show it, queue it behind a meeting alert) is the island state's business:
 * it is handed to `onReceived`. New Outlook meeting requests (useMeetingInvites, their own
 * setting) arrive the same way and count as unseen too. Everything also goes to the in-memory
 * history the Notifications tab shows; while a meeting is silenced or Do not disturb is on nothing pops up.
 */
export function useNotifications(
  enabled: boolean,
  onReceived: (notification: IslandNotification) => void,
  historyStore: NotificationHistory = notificationHistory,
  silenceStore: Silence = silence
): UseNotificationsResult {
  const [status, setStatus] = useState<NotificationStatus | null>(null);
  const [unseen, setUnseen] = useState(0);
  const seenIds = useRef<number[]>([]);
  const onReceivedRef = useRef(onReceived);
  onReceivedRef.current = onReceived;

  useEffect(() => {
    let disposed = false;
    void ipc.notificationsGetStatus().then((next) => {
      if (!disposed && next) setStatus(next);
    });
    const offStatus = onEvent<unknown>("notification-status", (payload) => {
      const next = normalizeNotificationStatus(payload);
      if (next) setStatus(next);
    });
    return () => {
      disposed = true;
      offStatus();
    };
  }, []);

  // Every arrival goes to the Notifications tab and the unseen count; it only pops up in the
  // island when the user has not silenced notifications for the meeting they are in.
  const receive = useCallback(
    (notification: IslandNotification) => {
      setUnseen((n) => n + 1);
      deliverNotification(notification, Date.now(), historyStore, silenceStore, (n) => onReceivedRef.current(n));
    },
    [historyStore, silenceStore]
  );

  useEffect(() => {
    if (!enabled) {
      setUnseen(0);
      return;
    }
    const off = onEvent<unknown>("notification-received", (payload) => {
      const notification = normalizeNotification(payload);
      if (!notification || seenIds.current.includes(notification.id)) return;
      seenIds.current = [...seenIds.current, notification.id].slice(-SEEN_IDS_MAX);
      receive(notification);
    });
    return off;
  }, [enabled, receive]);

  const { settings, loaded } = useSettings();
  useMeetingInvites(loaded ? settings.meetingInvitesEnabled : null, receive);

  const markSeen = useCallback(() => setUnseen(0), []);

  const markViewed = useCallback(() => historyStore.markViewed(Date.now()), [historyStore]);

  const activate = useCallback(activateNotification, []);

  const requestAccess = useCallback(() => {
    void ipc.notificationsRequestAccess().then((next) => {
      if (next) setStatus(next);
    });
  }, []);

  return { status, unseen, markSeen, markViewed, activate, requestAccess };
}
