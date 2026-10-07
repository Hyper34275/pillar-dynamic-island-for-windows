import { useContext, useRef } from "react";
import { motion, useMotionValue, useTransform, type MotionValue } from "motion/react";
import { contentDirection } from "../../design/direction";
import { color, control, toast, type as typeRoles } from "../../design/tokens";
import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { fitScale, layerFade } from "./animations";
import { ShellContext, useTransitionLayer } from "./drivenTransition";
import { useArrivalFade } from "./IslandLayer";
import { TOAST_DRIFT_PX, useToastPayloads, usePayloadFade, type DrivenHandoff } from "./toastHandoff";
import { toastLayout, toastSource, toastTitle } from "./toastLayout";
import { RoundButton } from "./ui/controls";
import { BellSlashIcon, XIcon } from "./ui/icons";
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
  /** The shape the island is heading for with this toast (PillShell's target); defaults to the toast's own layout size. */
  targetSize?: { width: number; height: number };
  /** No drift, a tighter handoff (the OS "show animations" is off). */
  reducedMotion?: boolean;
}

/**
 * One notification's content inside the toast: laid out once at its own final size, centred on the
 * island and riding the shape's vertical centre (like IslandLayer), with its opacity and drift
 * from the payload handoff (toastHandoff.ts). Only the arriving payload is interactive and exposed
 * to assistive tech; a leaving one is inert while it fades.
 */
function ToastPayload({
  notification,
  onDismiss,
  onActivate,
  onSwipeAway,
  onRemove,
  interactive,
  leaving,
  driven,
  drift,
  onGone,
}: Required<Pick<NotificationToastProps, "notification" | "onDismiss" | "onActivate" | "onSwipeAway" | "onRemove">> & {
  interactive: boolean;
  leaving: boolean;
  driven: DrivenHandoff;
  drift: number;
  onGone: () => void;
}) {
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

  const { opacity, offset } = usePayloadFade(driven, leaving, drift, onGone);
  const y = useRide(layout.size.height, offset);
  // While the shape is narrower than this payload (opening from the pill, growing to a wider
  // toast) it fits the shape's width (uniformly, at most 15 %): the icon at its edge is never cut.
  const shell = useContext(ShellContext);
  const still = useMotionValue(layout.size.width);
  const scale = useTransform(shell?.width ?? still, (w: number) => fitScale(w, layout.size.width));

  return (
    <motion.div
      data-layer="notification"
      data-payload={leaving ? "leaving" : "shown"}
      data-payload-id={notification.id}
      aria-hidden={interactive ? undefined : true}
      className="group select-none absolute top-0"
      style={{
        padding: toast.padding,
        color: color.fg,
        width: layout.size.width,
        height: layout.size.height,
        left: `calc(50% - ${layout.size.width / 2}px)`,
        y,
        scale,
        opacity,
        pointerEvents: interactive ? undefined : "none",
      }}
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
        icon={
          isInvite ? (
            <CalendarAppIcon />
          ) : notification.missedSummary ? (
            // The island's own "You missed N" toast: the quiet slashed bell of the muted state.
            <AppIcon glyph={<BellSlashIcon size={20} />} tint={color.muted} />
          ) : (
            <AppIcon name={cleanAppName(notification.appName) || notification.appName} />
          )
        }
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
    </motion.div>
  );
}

/**
 * How far (px) a payload of `height` sits below the island's top so that it rides the shape's
 * vertical centre (as IslandLayer does), plus its handoff drift.
 */
function useRide(height: number, drift: MotionValue<number>): MotionValue<number> {
  const shell = useContext(ShellContext);
  const still = useMotionValue(height);
  const shellHeight = shell?.height ?? still;
  return useTransform([shellHeight, drift], ([h, d]: number[]) => (shell ? (h - height) / 2 : 0) + d);
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
 *
 * This is the toast STAGE: one island layer for the whole notification session. It fades with
 * the island's own transition (opening from Compact, closing back to it) and stays mounted while
 * the session moves from one notification to the next; only the payload inside changes hands
 * (toastHandoff.ts: at most two payloads, a strict ownership handoff, no double exposure).
 */
export function NotificationToast({
  notification,
  onDismiss,
  onActivate,
  onSwipeAway = onDismiss,
  onRemove = onDismiss,
  targetSize,
  reducedMotion = false,
}: NotificationToastProps) {
  const { opacity, isPresent } = useTransitionLayer({ fade: useArrivalFade("toast", layerFade.temporary) });
  const shell = useContext(ShellContext);
  const size = targetSize ?? toastLayout(notification).size;
  const { layers, driven, remove } = useToastPayloads(notification, size, shell, reducedMotion);
  const drift = reducedMotion ? 0 : TOAST_DRIFT_PX;
  return (
    <motion.div
      data-layer="toast"
      aria-hidden={isPresent ? undefined : true}
      className="absolute inset-0"
      style={{ opacity, pointerEvents: isPresent ? undefined : "none" }}
    >
      {layers.map((layer) => (
        <ToastPayload
          key={layer.id}
          notification={layer.notification}
          onDismiss={onDismiss}
          onActivate={onActivate}
          onSwipeAway={onSwipeAway}
          onRemove={onRemove}
          interactive={isPresent && !layer.leaving}
          leaving={layer.leaving}
          driven={driven}
          drift={drift}
          onGone={() => remove(layer.id, layer.gen)}
        />
      ))}
    </motion.div>
  );
}
