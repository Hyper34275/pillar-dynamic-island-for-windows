import { AnimatePresence, motion } from "motion/react";
import { NOTIFICATION_TOAST_MS } from "../../hooks/useNotifications";
import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { AppAvatar, cleanAppName } from "./ui/primitives";
import { XIcon } from "./ui/icons";

/** Distance between the island and the toast; the hit region must account for it too. */
export const TOAST_GAP = 8;

interface NotificationToastProps {
  notification: IslandNotification | null;
  reducedMotion: boolean;
  onDismiss: () => void;
  onActivate: (notification: IslandNotification) => void;
  /** Receives the toast's wrapper element while it is mounted (null when it leaves). */
  wrapperRef: (el: HTMLDivElement | null) => void;
}

/**
 * Drops below the island when a Windows notification arrives. Body click activates the
 * source app; the X only dismisses. Do not add an onClick (capture) on the wrapper: it
 * would swallow the X button's stopPropagation and activate the app on every dismiss.
 */
export function NotificationToast({ notification, reducedMotion, onDismiss, onActivate, wrapperRef }: NotificationToastProps) {
  return (
    <AnimatePresence>
      {notification && (
        <div
          key="toast-position"
          ref={wrapperRef}
          className="absolute left-1/2 -translate-x-1/2 z-[100]"
          style={{ top: `calc(100% + ${TOAST_GAP}px)` }}
        >
          <motion.div
            key={notification.id}
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: -14, scale: 0.94 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={reducedMotion ? { opacity: 0 } : { opacity: 0, y: -10, scale: 0.96 }}
            transition={reducedMotion ? { duration: 0.08 } : { type: "spring", stiffness: 420, damping: 32 }}
            style={{ originY: 0 }}
          >
            <ToastCard notification={notification} reducedMotion={reducedMotion} onDismiss={onDismiss} onActivate={onActivate} />
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}

function ToastCard({
  notification,
  reducedMotion,
  onDismiss,
  onActivate,
}: {
  notification: IslandNotification;
  reducedMotion: boolean;
  onDismiss: () => void;
  onActivate: (notification: IslandNotification) => void;
}) {
  const appLabel = cleanAppName(notification.appName) || notification.appName;
  const activate = () => {
    onActivate(notification);
    onDismiss();
  };

  return (
    <div
      dir="ltr"
      className="relative w-[352px] rounded-[26px] cursor-pointer overflow-hidden select-none bg-black text-white"
      style={{ boxShadow: "0 0 0 0.5px rgba(255,255,255,0.08), 0 12px 32px rgba(0,0,0,0.5)" }}
      onClick={(e) => {
        e.stopPropagation();
        activate();
      }}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          e.stopPropagation();
          activate();
        } else if (e.key === "Delete" || e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onDismiss();
        }
      }}
    >
      <div className="flex items-center gap-3 pl-3 pr-2 py-3">
        <AppAvatar name={notification.appName} size={40} radius={12} />

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-white/45 text-[11px] font-semibold truncate" dir="auto">
              {appLabel}
            </span>
            <span className="text-white/30 text-[11px] ml-auto flex-shrink-0">{t("notif.now")}</span>
          </div>
          <h4 className="text-white text-[14px] font-semibold truncate leading-snug" dir="auto">
            {notification.title || t("notif.default")}
          </h4>
          {notification.body && (
            <p className="text-white/60 text-[12.5px] line-clamp-2 leading-snug" dir="auto">
              {notification.body}
            </p>
          )}
        </div>

        <button
          type="button"
          className="self-start w-7 h-7 rounded-full flex items-center justify-center text-white/35 hover:text-white hover:bg-white/10 transition-colors flex-shrink-0"
          aria-label={t("notif.dismiss")}
          onClick={(e) => {
            e.stopPropagation();
            onDismiss();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              e.stopPropagation();
              onDismiss();
            }
          }}
        >
          <XIcon size={13} strokeWidth={2.6} />
        </button>
      </div>

      {/* Time-left hairline: a single finite transform animation. */}
      <motion.div
        className="absolute bottom-0 left-6 right-6 h-[2px] rounded-full bg-white/25"
        style={{ transformOrigin: "left" }}
        initial={{ scaleX: 1 }}
        animate={{ scaleX: reducedMotion ? 1 : 0 }}
        transition={reducedMotion ? { duration: 0 } : { duration: NOTIFICATION_TOAST_MS / 1000, ease: "linear" }}
      />
    </div>
  );
}
