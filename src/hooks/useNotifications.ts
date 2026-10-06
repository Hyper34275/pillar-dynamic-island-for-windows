import { useCallback, useEffect, useRef, useState } from "react";
import { ipc, normalizeNotification, normalizeNotificationStatus, onEvent, type IslandNotification, type NotificationStatus } from "../lib/ipc";

/** How long a toast stays before it folds into the unseen-count badge. */
export const NOTIFICATION_TOAST_MS = 4000;
const SEEN_IDS_MAX = 50;

export interface UseNotificationsResult {
  status: NotificationStatus | null;
  /** The toast currently on screen, if any. */
  toast: IslandNotification | null;
  /** Notifications received since the island was last opened. */
  unseen: number;
  dismissToast: () => void;
  markSeen: () => void;
  activate: (notification: IslandNotification) => void;
  requestAccess: () => void;
}

/**
 * Windows toast mirroring. Event-driven only (no polling) and in-memory only: titles and
 * bodies are shown on screen but never persisted and never logged.
 */
export function useNotifications(enabled: boolean): UseNotificationsResult {
  const [status, setStatus] = useState<NotificationStatus | null>(null);
  const [toast, setToast] = useState<IslandNotification | null>(null);
  const [unseen, setUnseen] = useState(0);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const seenIds = useRef<number[]>([]);

  const clearToastTimer = useCallback(() => {
    if (toastTimer.current !== null) {
      clearTimeout(toastTimer.current);
      toastTimer.current = null;
    }
  }, []);

  const dismissToast = useCallback(() => {
    clearToastTimer();
    setToast(null);
  }, [clearToastTimer]);

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
      dismissToast();
      setUnseen(0);
      return;
    }
    const off = onEvent<unknown>("notification-received", (payload) => {
      const notification = normalizeNotification(payload);
      if (!notification || seenIds.current.includes(notification.id)) return;
      seenIds.current = [...seenIds.current, notification.id].slice(-SEEN_IDS_MAX);
      clearToastTimer();
      setToast(notification);
      setUnseen((n) => n + 1);
      toastTimer.current = setTimeout(() => {
        toastTimer.current = null;
        setToast(null);
      }, NOTIFICATION_TOAST_MS);
    });
    return () => {
      off();
      clearToastTimer();
    };
  }, [enabled, dismissToast, clearToastTimer]);

  const markSeen = useCallback(() => setUnseen(0), []);

  const activate = useCallback((notification: IslandNotification) => {
    void (notification.aumid ? ipc.activateAppByAumid(notification.aumid) : ipc.activateNotification(notification.id));
  }, []);

  const requestAccess = useCallback(() => {
    void ipc.notificationsRequestAccess().then((next) => {
      if (next) setStatus(next);
    });
  }, []);

  return { status, toast, unseen, dismissToast, markSeen, activate, requestAccess };
}
