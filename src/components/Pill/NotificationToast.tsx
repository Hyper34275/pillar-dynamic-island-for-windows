import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { AppAvatar, cleanAppName, SYSTEM_COLORS } from "./ui/primitives";
import { XIcon } from "./ui/icons";
import { InviteActions, InviteAvatar } from "./ui/meetingActions";
import { layerFade, notificationSize } from "./animations";
import { IslandLayer } from "./IslandLayer";

interface NotificationToastProps {
  notification: IslandNotification;
  onDismiss: () => void;
  onActivate: (notification: IslandNotification) => void;
}

/**
 * A mirrored Windows notification, shown inside the island (which grows to fit, see
 * notificationSize). Body click activates the source app; the X only dismisses. Do not add an
 * onClick (capture) on the wrapper: it would swallow the X button's stopPropagation and
 * activate the app on every dismiss. A meeting invitation looks the same with a calendar tile and
 * a row of answer buttons (accept / maybe / decline, sent through Outlook); its body click opens
 * the Outlook calendar on the meeting's day.
 */
export function NotificationToast({ notification, onDismiss, onActivate }: NotificationToastProps) {
  const isInvite = !!notification.invite;
  const appLabel = isInvite ? t("invite.label") : cleanAppName(notification.appName) || notification.appName;
  const activate = () => {
    onActivate(notification);
    onDismiss();
  };

  return (
    <IslandLayer
      name="notification"
      fade={layerFade.temporary}
      size={notificationSize(notification.body !== "", isInvite)}
      dir="ltr"
      className="flex items-center gap-3 pl-4 pr-3 cursor-pointer select-none text-white"
      role="button"
      tabIndex={0}
      title={isInvite ? t("invite.open") : undefined}
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
      {isInvite ? <InviteAvatar /> : <AppAvatar name={notification.appName} size={40} radius={12} />}

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-semibold truncate" style={{ color: isInvite ? SYSTEM_COLORS.blue : "rgba(255,255,255,0.45)" }} dir="auto">
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
        {notification.invite && (
          <div className="mt-2" onClick={(e) => e.stopPropagation()}>
            <InviteActions inviteId={notification.invite.id} height={26} />
          </div>
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
    </IslandLayer>
  );
}
