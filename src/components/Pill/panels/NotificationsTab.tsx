import { useMinute } from "../../../hooks/useClock";
import { activateNotification } from "../../../hooks/useNotifications";
import { useSettings } from "../../../hooks/useSettings";
import { relativeMinutes } from "../../../lib/dateFormat";
import { t } from "../../../lib/i18n";
import type { IslandNotification } from "../../../lib/ipc";
import { notificationHistory, useNotificationHistory, type HistoryEntry, type NotificationHistory } from "../../../lib/notifications/history";
import { BellIcon, BellSlashIcon, XIcon } from "../ui/icons";
import { InviteActions, InviteAvatar } from "../ui/meetingActions";
import { AppAvatar, cleanAppName, EmptyState, SYSTEM_COLORS } from "../ui/primitives";

/** "now", "5 min. ago", "2 hr. ago" in the UI language. */
function receivedLabel(receivedAt: number, nowMs: number): string {
  const minutes = Math.floor((nowMs - receivedAt) / 60_000);
  return minutes < 1 ? t("notif.now") : relativeMinutes(-minutes);
}

function NotificationRow({
  entry,
  nowMs,
  onActivate,
  onRemove,
}: {
  entry: HistoryEntry;
  nowMs: number;
  onActivate: (notification: IslandNotification) => void;
  onRemove: (id: number) => void;
}) {
  const { notification, receivedAt, silenced } = entry;
  const invite = notification.invite;
  const appLabel = invite ? t("invite.label") : cleanAppName(notification.appName) || notification.appName;
  return (
    <li
      dir="ltr"
      className="group relative flex gap-3 rounded-[18px] bg-white/[0.06] hover:bg-white/[0.09] pl-3 pr-2 py-2.5 cursor-pointer transition-colors"
      role="button"
      tabIndex={0}
      onClick={() => onActivate(notification)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onActivate(notification);
        } else if (e.key === "Delete") {
          e.preventDefault();
          onRemove(notification.id);
        }
      }}
    >
      {invite ? <InviteAvatar size={34} /> : <AppAvatar name={notification.appName} size={34} radius={10} />}
      <div className="flex-1 min-w-0 flex flex-col gap-0.5">
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-semibold truncate" style={{ color: invite ? SYSTEM_COLORS.blue : "rgba(255,255,255,0.45)" }} dir="auto">
            {appLabel}
          </span>
          {silenced && (
            <span className="flex-shrink-0" style={{ color: SYSTEM_COLORS.red }} title={t("notifs.silenced")} aria-label={t("notifs.silenced")}>
              <BellSlashIcon size={11} />
            </span>
          )}
          <span className="ml-auto flex-shrink-0 text-[11px] text-white/35 tabular-nums" dir="auto">
            {receivedLabel(receivedAt, nowMs)}
          </span>
        </div>
        <h4 className="text-[13.5px] font-semibold text-white truncate leading-snug" dir="auto">
          {notification.title || t("notif.default")}
        </h4>
        {notification.body && (
          <p className="text-[12.5px] text-white/60 line-clamp-2 leading-snug" dir="auto">
            {notification.body}
          </p>
        )}
        {invite && (
          <div className="mt-1" onClick={(e) => e.stopPropagation()}>
            <InviteActions inviteId={invite.id} height={26} />
          </div>
        )}
      </div>
      <button
        type="button"
        className="self-start w-6 h-6 rounded-full flex items-center justify-center text-white/30 hover:text-white hover:bg-white/10 flex-shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-opacity"
        aria-label={t("notifs.remove")}
        onClick={(e) => {
          e.stopPropagation();
          onRemove(notification.id);
        }}
      >
        <XIcon size={11} strokeWidth={2.6} />
      </button>
    </li>
  );
}

interface NotificationsViewProps {
  entries: readonly HistoryEntry[];
  nowMs: number;
  notificationsEnabled: boolean;
  onActivate: (notification: IslandNotification) => void;
  onRemove: (id: number) => void;
  onClear: () => void;
}

/** Pure rendering of the session's notifications, newest first. */
export function NotificationsView({ entries, nowMs, notificationsEnabled, onActivate, onRemove, onClear }: NotificationsViewProps) {
  if (entries.length === 0) {
    return (
      <div dir="ltr" className="flex-1 flex flex-col items-center justify-center">
        <EmptyState
          icon={<BellIcon size={22} />}
          title={t("notifs.empty")}
          subtitle={notificationsEnabled ? t("notifs.emptyHint") : t("notifs.off")}
        />
      </div>
    );
  }
  return (
    <div dir="ltr" className="flex flex-col gap-2">
      <div className="flex items-center justify-between px-1">
        {!notificationsEnabled ? (
          <span className="text-[11px] text-white/45 truncate" dir="auto">
            {t("notifs.off")}
          </span>
        ) : (
          <span />
        )}
        <button type="button" className="text-[12px] font-semibold hover:brightness-125 flex-shrink-0" style={{ color: SYSTEM_COLORS.blue }} onClick={onClear}>
          <span dir="auto">{t("notifs.clear")}</span>
        </button>
      </div>
      <ul className="flex flex-col gap-1.5">
        {entries.map((entry) => (
          <NotificationRow key={entry.notification.id} entry={entry} nowMs={nowMs} onActivate={onActivate} onRemove={onRemove} />
        ))}
      </ul>
    </div>
  );
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
      onActivate={activateNotification}
      onRemove={history.remove}
      onClear={history.clear}
    />
  );
}
