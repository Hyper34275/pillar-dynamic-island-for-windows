import { useCallback, useEffect, useRef, useState } from "react";
import { ipc, normalizeNotification, normalizeNotificationStatus, onEvent, type IslandNotification, type NotificationStatus } from "../lib/ipc";

const SEEN_IDS_MAX = 50;

export interface UseNotificationsResult {
  status: NotificationStatus | null;
  /** Notifications received since the island was last opened. */
  unseen: number;
  markSeen: () => void;
  activate: (notification: IslandNotification) => void;
  requestAccess: () => void;
}

/**
 * Windows toast mirroring. Event-driven only (no polling) and in-memory only: titles and
 * bodies are shown on screen but never persisted and never logged. What to do with a new
 * notification (show it, queue it behind a meeting alert) is the island state's business:
 * it is handed to `onReceived`.
 */
export function useNotifications(enabled: boolean, onReceived: (notification: IslandNotification) => void): UseNotificationsResult {
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

  useEffect(() => {
    if (!enabled) {
      setUnseen(0);
      return;
    }
    const off = onEvent<unknown>("notification-received", (payload) => {
      const notification = normalizeNotification(payload);
      if (!notification || seenIds.current.includes(notification.id)) return;
      seenIds.current = [...seenIds.current, notification.id].slice(-SEEN_IDS_MAX);
      setUnseen((n) => n + 1);
      onReceivedRef.current(notification);
    });
    return off;
  }, [enabled]);

  const markSeen = useCallback(() => setUnseen(0), []);

  const activate = useCallback((notification: IslandNotification) => {
    void (notification.aumid ? ipc.activateAppByAumid(notification.aumid) : ipc.activateNotification(notification.id));
  }, []);

  const requestAccess = useCallback(() => {
    void ipc.notificationsRequestAccess().then((next) => {
      if (next) setStatus(next);
    });
  }, []);

  return { status, unseen, markSeen, activate, requestAccess };
}
