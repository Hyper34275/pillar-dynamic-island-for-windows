import { useRef } from "react";
import { contentDirection } from "../../design/direction";
import { color, control, toast, type as typeRoles } from "../../design/tokens";
import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { layerFade } from "./animations";
import { IslandLayer } from "./IslandLayer";
import { toastLayout, toastSource, toastTitle } from "./toastLayout";
import { RoundButton } from "./ui/controls";
import { XIcon } from "./ui/icons";
import { AppIcon, CalendarAppIcon } from "./ui/identity";
import { InviteActions } from "./ui/meetingActions";
import { NotificationContent, NotificationPrimary, notificationAccessibleLabel } from "./ui/notification";
import { cleanAppName } from "./ui/primitives";

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
  /** The Delete key: removes the notification from the Notification Center too; defaults to a plain dismiss. */
  onRemove?: () => void;
}

/**
 * A mirrored Windows notification, shown inside the island (which grows to fit, see toastLayout).
 * A click on the toast (its one primary button, see NotificationPrimary) activates the source app; the X only dismisses (it shows while the pointer is on the
 * island or the X has keyboard focus, so it never clutters the toast; its slot is always reserved,
 * so nothing moves); a horizontal swipe dismisses without opening the app. Do not add an onClick
 * (capture) on the wrapper: it would swallow the X button's stopPropagation and activate the app on
 * every dismiss. A meeting invitation looks the same with a calendar tile and a row of answer
 * buttons (accept / maybe / decline, sent through Outlook); its body click opens the Outlook
 * calendar on the meeting's day. The content direction is the notification's own (an English
 * toast is laid out left to right, a Hebrew one right to left).
 */
export function NotificationToast({ notification, onDismiss, onActivate, onSwipeAway = onDismiss, onRemove = onDismiss }: NotificationToastProps) {
  const isInvite = !!notification.invite;
  const source = toastSource(notification);
  const title = toastTitle(notification);
  const layout = toastLayout(notification);
  const pressRef = useRef<{ x: number; y: number } | null>(null);
  const activate = () => {
    onActivate(notification);
    onDismiss();
  };
  const label = notificationAccessibleLabel({ source, title, body: notification.body || undefined, invite: isInvite });

  return (
    <IslandLayer
      name="notification"
      fade={layerFade.temporary}
      size={layout.size}
      className="group select-none"
      style={{ padding: toast.padding, color: color.fg }}
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
      onKeyDown={(e) => {
        // Enter and Space press the primary button natively. Escape only closes the toast (cancel:
        // the Notification Center keeps the entry); Delete removes the notification (onRemove).
        if (e.key === "Delete" || e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          if (e.key === "Delete") onRemove();
          else onDismiss();
        }
      }}
    >
      {/* The toast's own action: one focus stop named by the whole notification. It never takes
          focus by itself (no autofocus, no focus() call): a new notification must not steal it. */}
      <NotificationPrimary
        label={label}
        title={isInvite ? t("invite.open") : undefined}
        onPress={activate}
        onClick={(e) => {
          e.stopPropagation();
          // The tail of a drag or swipe must not also open the app.
          const press = pressRef.current;
          pressRef.current = null;
          if (press && Math.hypot(e.clientX - press.x, e.clientY - press.y) > CLICK_SLOP_PX) return;
          activate();
        }}
      />
      <NotificationContent
        dir={contentDirection(title, notification.body)}
        textHidden
        icon={isInvite ? <CalendarAppIcon /> : <AppIcon name={cleanAppName(notification.appName) || notification.appName} />}
        source={source}
        sourceTone={isInvite ? "accent" : "default"}
        title={title}
        body={notification.body || undefined}
        bodyLines={layout.bodyLines || undefined}
        meta={
          // The slot is as high as the source line, so the 28px button overhangs it by 6px above and
          // below (into the padding and the title's leading) and never changes the row's height.
          <span className="flex items-center justify-center" style={{ width: control.round, height: typeRoles.meta.lineHeight }}>
            <RoundButton
              onPress={onDismiss}
              ariaLabel={t("notif.dismiss")}
              className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
            >
              <XIcon size={control.iconSize - 4} strokeWidth={2.6} />
            </RoundButton>
          </span>
        }
        actions={isInvite ? <InviteActions inviteId={notification.invite!.id} /> : undefined}
      />
    </IslandLayer>
  );
}
