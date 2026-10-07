import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useMinute } from "../../../hooks/useClock";
import { activateNotification } from "../../../hooks/useNotifications";
import { useSettings } from "../../../hooks/useSettings";
import { uiDirection } from "../../../design/direction";
import { card, color, control, icon, space, type as typeRoles } from "../../../design/tokens";
import { relativeMinutes } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import type { IslandNotification } from "../../../lib/ipc";
import { isUnread, notificationHistory, useNotificationHistory, type HistoryEntry, type NotificationHistory } from "../../../lib/notifications/history";
import { HeaderActionButton } from "../HeaderAction";
import { RoundButton } from "../ui/controls";
import { BellIcon, BellSlashIcon, XIcon } from "../ui/icons";
import { AppIcon, CalendarAppIcon } from "../ui/identity";
import { EmptyState } from "../ui/states";
import { InviteActions } from "../ui/meetingActions";
import { NotificationContent, NotificationPrimary, notificationAccessibleLabel } from "../ui/notification";
import { cleanAppName } from "../ui/primitives";

/** "now", "5 min. ago", "2 hr. ago" in the UI language. */
function receivedLabel(receivedAt: number, nowMs: number): string {
  const minutes = Math.floor((nowMs - receivedAt) / 60_000);
  return minutes < 1 ? t("notif.now") : relativeMinutes(-minutes);
}

/** Opacity swap between the time and the dismiss button, which share one slot (no reflow). */
const SWAP = "transition-opacity duration-150 ease-out";

/** A card arriving or leaving: opacity plus height, ease-out, ~200 ms. Reduced motion: opacity only. */
const ENTER_S = 0.2;
const EXIT_S = 0.18;

function NotificationCard({
  entry,
  nowMs,
  unread,
  onActivate,
  onRemove,
}: {
  entry: HistoryEntry;
  nowMs: number;
  unread: boolean;
  onActivate: (notification: IslandNotification) => void;
  onRemove: (id: number) => void;
}) {
  const reducedMotion = useReducedMotion() ?? false;
  const { notification, receivedAt, silenced } = entry;
  const invite = notification.invite;
  const appName = cleanAppName(notification.appName) || notification.appName;
  const source = invite ? t("invite.label") : appName;
  const title = notification.title || t("notif.default");
  const received = receivedLabel(receivedAt, nowMs);
  // The shell is the UI's for every card: the icon is on the same side in all of them. Each text
  // field inside takes its own direction (NotificationContent).
  const dir = uiDirection();

  const label = notificationAccessibleLabel({
    source,
    invite: !!invite,
    title,
    body: notification.body || undefined,
    silenced,
    received,
  });

  const hidden = reducedMotion ? { opacity: 0 } : { opacity: 0, height: 0, overflow: "hidden" };
  const shown = reducedMotion ? { opacity: 1 } : { opacity: 1, height: "auto", transitionEnd: { overflow: "visible" } };

  return (
    // The wrapper animates (height, opacity) and carries the list gap as its bottom padding, so a
    // collapsing card takes its gap with it and the 8px rhythm never breaks.
    <motion.li
      className="list-none"
      style={{ paddingBottom: card.listGap }}
      initial={hidden}
      animate={{ ...shown, transition: { duration: ENTER_S, ease: "easeOut" } }}
      exit={{ ...hidden, transition: { duration: EXIT_S, ease: "easeOut" } }}
    >
      {/*
        Structure: the card is a plain surface, not a focus stop. Clicking a card opens the source app
        (activateNotification), so it has ONE primary button stretched over it (named by the whole
        notification, read once), and the dismiss button is its sibling: Tab goes primary, dismiss,
        (invitation actions), next card; nothing interactive sits inside anything interactive. Delete
        removes the notification whichever of them has focus; Escape does nothing here (it closes the
        island, handled by the shell, and never deletes).
      */}
      <div
        data-notification-card=""
        className="group relative ci-surface rounded-surface p-card-pad transition-colors duration-150 hover:bg-surface-hover"
        onKeyDown={(e) => {
          if (e.key === "Delete") {
            e.preventDefault();
            onRemove(notification.id);
          }
        }}
      >
        <NotificationPrimary label={label} onPress={() => onActivate(notification)} />
        <NotificationContent
          dir={dir}
          textHidden
          icon={invite ? <CalendarAppIcon /> : <AppIcon name={appName} />}
          source={source}
          sourceTone={invite ? "accent" : "default"}
          sourceAdornment={
            silenced && (
              // Muted is not dangerous: a quiet grey bell (its meaning is in the card's accessible name).
              <span className="flex-shrink-0 flex" style={{ color: color.muted }} title={t("notifs.silenced")} aria-hidden="true">
                <BellSlashIcon size={icon.small} />
              </span>
            )
          }
          meta={
            // One slot at the end of the source row, as tall as the meta line (so the row never
            // grows). The time, and the unread dot after it, are there at rest; on hover or
            // keyboard focus the 28px dismiss button takes its place, centred on the line.
            <span className="relative flex items-center justify-end" style={{ height: typeRoles.meta.lineHeight }}>
              <span className={`flex items-center ${SWAP} group-hover:opacity-0 group-focus-within:opacity-0`} style={{ gap: space[1] }}>
                <span aria-hidden="true" className="tabular-nums whitespace-nowrap" style={{ color: color.fgTertiary }}>
                  {received}
                </span>
                {unread && <span aria-hidden="true" data-unread="" className="ci-mark rounded-full flex-shrink-0" style={{ width: card.unreadDot, height: card.unreadDot, background: color.accent }} />}
              </span>
              <span className={`absolute end-0 top-1/2 flex opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 ${SWAP}`} style={{ marginTop: -control.round / 2 }}>
                <RoundButton ariaLabel={t("notifs.remove")} onPress={() => onRemove(notification.id)}>
                  <XIcon size={icon.small} strokeWidth={2.4} />
                </RoundButton>
              </span>
            </span>
          }
          title={title}
          titleLines={2}
          body={notification.body || undefined}
          bodyLines={3}
          actions={invite ? <InviteActions inviteId={invite.id} /> : undefined}
        />
      </div>
    </motion.li>
  );
}

/** No notifications: the shared empty state (ui/states.tsx), centred in the content area. */
function EmptyNotifications({ notificationsEnabled }: { notificationsEnabled: boolean }) {
  return <EmptyState icon={<BellIcon size={icon.state} />} title={t("notifs.empty")} hint={notificationsEnabled ? t("notifs.emptyHint") : t("notifs.off")} />;
}

interface NotificationsViewProps {
  entries: readonly HistoryEntry[];
  nowMs: number;
  notificationsEnabled: boolean;
  onActivate: (notification: IslandNotification) => void;
  onRemove: (id: number) => void;
  /** Entries received after this moment get the unread dot (default: none). */
  lastViewedAt?: number;
  /** Clearing lives in the panel header now (NotificationsClearAction); accepted so older callers compile. */
  onClear?: () => void;
}

/** Pure rendering of the session's notifications, newest first. */
export function NotificationsView({ entries, nowMs, notificationsEnabled, onActivate, onRemove, lastViewedAt = Infinity }: NotificationsViewProps) {
  const empty = entries.length === 0;
  return (
    <div className="relative flex-1 flex flex-col">
      {!empty && !notificationsEnabled && (
        <p className="text-meta flex-shrink-0 truncate" style={{ color: color.fgTertiary, paddingBottom: card.listGap }}>
          {t("notifs.off")}
        </p>
      )}
      {/* Opening the panel never animates the cards that are already there (initial={false}). */}
      <ul className="flex flex-col">
        <AnimatePresence initial={false}>
          {entries.map((entry) => (
            <NotificationCard
              key={entry.notification.id}
              entry={entry}
              nowMs={nowMs}
              unread={isUnread(entry, lastViewedAt)}
              onActivate={onActivate}
              onRemove={onRemove}
            />
          ))}
        </AnimatePresence>
      </ul>
      {empty && <EmptyNotifications notificationsEnabled={notificationsEnabled} />}
    </div>
  );
}

/** "Clear all" in the panel header (tabs.ts); there is nothing to clear when the list is empty. */
export function NotificationsClearAction({ history = notificationHistory }: { history?: NotificationHistory } = {}) {
  const entries = useNotificationHistory(history);
  if (entries.length === 0) return null;
  return <HeaderActionButton onPress={history.clear}>{t("notifs.clear")}</HeaderActionButton>;
}

export function NotificationsTab({ history = notificationHistory }: { history?: NotificationHistory }) {
  const entries = useNotificationHistory(history);
  const nowMs = useMinute().getTime();
  const { settings } = useSettings();
  return (
    <NotificationsView
      entries={entries}
      nowMs={nowMs}
      notificationsEnabled={settings.notificationsEnabled}
      lastViewedAt={history.getLastViewedAt()}
      onActivate={activateNotification}
      onRemove={history.remove}
    />
  );
}
