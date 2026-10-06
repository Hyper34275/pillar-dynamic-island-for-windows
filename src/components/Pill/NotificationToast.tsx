import { motion } from "motion/react";
import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { AppAvatar, cleanAppName } from "./ui/primitives";
import { XIcon } from "./ui/icons";

interface NotificationToastProps {
  notification: IslandNotification;
  reducedMotion: boolean;
  onDismiss: () => void;
  onActivate: (notification: IslandNotification) => void;
}

/**
 * A mirrored Windows notification, shown inside the island (which grows to fit, see
 * notificationSize). Body click activates the source app; the X only dismisses. Do not add an
 * onClick (capture) on the wrapper: it would swallow the X button's stopPropagation and
 * activate the app on every dismiss.
 */
export function NotificationToast({ notification, reducedMotion, onDismiss, onActivate }: NotificationToastProps) {
  const appLabel = cleanAppName(notification.appName) || notification.appName;
  const activate = () => {
    onActivate(notification);
    onDismiss();
  };

  return (
    <motion.div
      dir="ltr"
      className="absolute inset-0 flex items-center gap-3 pl-4 pr-3 cursor-pointer select-none text-white"
      role="button"
      tabIndex={0}
      initial={{ opacity: 0, y: reducedMotion ? 0 : -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      transition={{ duration: reducedMotion ? 0.05 : 0.22, delay: reducedMotion ? 0 : 0.08, ease: [0.2, 0.8, 0.2, 1] }}
      onClick={(e) => {
        e.stopPropagation();
        activate();
      }}
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
        className="self-start mt-2.5 w-7 h-7 rounded-full flex items-center justify-center text-white/35 hover:text-white hover:bg-white/10 transition-colors flex-shrink-0"
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
    </motion.div>
  );
}
