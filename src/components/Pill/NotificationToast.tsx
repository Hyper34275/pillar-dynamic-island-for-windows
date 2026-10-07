import { useRef } from "react";
import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { AppAvatar, cleanAppName, SYSTEM_COLORS } from "./ui/primitives";
import { XIcon } from "./ui/icons";
import { InviteActions, InviteAvatar } from "./ui/meetingActions";
import { layerFade, notificationSize } from "./animations";
import { IslandLayer } from "./IslandLayer";

/** The toast's margin to its edges; the avatar's radius is the island's 30 minus it (concentric corners). */
const TOAST_MARGIN = 12;
const AVATAR_SIZE = 40;
const AVATAR_RADIUS = 18;
/** A press that moved this far is a drag, not a click (the shell's long press cancels at the same distance). */
const CLICK_SLOP_PX = 12;
/** A horizontal drag this far sweeps the toast away (the shell's swipe distance). */
const SWIPE_PX = 48;

interface NotificationToastProps {
  notification: IslandNotification;
  onDismiss: () => void;
  onActivate: (notification: IslandNotification) => void;
  /** A horizontal swipe on the toast; defaults to a plain dismiss. */
  onSwipeAway?: () => void;
}

/**
 * A mirrored Windows notification, shown inside the island (which grows to fit, see
 * notificationSize). Body click activates the source app; the X only dismisses (it shows while the
 * pointer is on the island or the X has keyboard focus, so it never clutters the toast); a
 * horizontal swipe dismisses without opening the app. Do not add an onClick (capture) on the
 * wrapper: it would swallow the X button's stopPropagation and activate the app on every
 * dismiss. A meeting invitation looks the same with a calendar tile and a row of answer buttons
 * (accept / maybe / decline, sent through Outlook); its body click opens the Outlook calendar on
 * the meeting's day.
 */
export function NotificationToast({ notification, onDismiss, onActivate, onSwipeAway = onDismiss }: NotificationToastProps) {
  const isInvite = !!notification.invite;
  const appLabel = isInvite ? t("invite.label") : cleanAppName(notification.appName) || notification.appName;
  const pressRef = useRef<{ x: number; y: number } | null>(null);
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
      className="group flex items-start gap-3 cursor-pointer select-none text-white"
      style={{ padding: TOAST_MARGIN }}
      role="button"
      tabIndex={0}
      title={isInvite ? t("invite.open") : undefined}
      onPointerDown={(e) => {
        pressRef.current = { x: e.clientX, y: e.clientY };
      }}
      onPointerUp={(e) => {
        const press = pressRef.current;
        if (!press) return;
        const dx = e.clientX - press.x;
        const dy = e.clientY - press.y;
        if (Math.abs(dx) >= SWIPE_PX && Math.abs(dx) > Math.abs(dy)) onSwipeAway();
      }}
      onClick={(e) => {
        e.stopPropagation();
        // The tail of a drag or swipe must not also open the app.
        const press = pressRef.current;
        pressRef.current = null;
        if (press && Math.hypot(e.clientX - press.x, e.clientY - press.y) > CLICK_SLOP_PX) return;
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
      {isInvite ? <InviteAvatar size={AVATAR_SIZE} radius={AVATAR_RADIUS} /> : <AppAvatar name={notification.appName} size={AVATAR_SIZE} radius={AVATAR_RADIUS} />}

      <div className="flex-1 min-w-0 min-h-[40px] flex flex-col justify-center">
        <div className="flex">
          <span className="text-[11px] font-semibold leading-[14px] truncate" style={{ color: isInvite ? SYSTEM_COLORS.blue : "rgba(255,255,255,0.5)" }} dir="auto">
            {appLabel}
          </span>
        </div>
        <h4 className="text-white text-[15px] font-semibold truncate leading-[20px]" dir="auto">
          {notification.title || t("notif.default")}
        </h4>
        {notification.body && (
          <p className="text-white/60 text-[13px] line-clamp-2 leading-[18px]" dir="auto">
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
        className="w-6 h-6 rounded-full flex items-center justify-center text-white/60 hover:text-white hover:bg-white/10 bg-white/[0.08] transition-opacity duration-[120ms] opacity-0 group-hover:opacity-100 focus-visible:opacity-100 flex-shrink-0"
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
        <XIcon size={12} strokeWidth={2.6} />
      </button>
    </IslandLayer>
  );
}
