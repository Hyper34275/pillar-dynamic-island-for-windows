import { uiDirection, textDirection } from "../../design/direction";
import { alert as alertTokens, color, smallExpanded } from "../../design/tokens";
import { formatTime } from "../../lib/dateFormat";
import { t } from "../../lib/i18n";
import type { ReminderAlert } from "../../lib/reminders/types";
import { layerFade } from "./animations";
import { alertHasActions, alertIslandSize, canSnooze } from "./alertLayout";
import { IslandLayer } from "./IslandLayer";
import { BidiText } from "./ui/BidiText";
import { ActionButton, ActionRow } from "./ui/controls";
import { JoinButton } from "./ui/meetingActions";

/** "Meeting in 30 minutes" / "Meeting starting now"; the plural form comes from Intl.PluralRules per locale. */
export function meetingAlertLabel(alert: ReminderAlert): string {
  return alert.minutesRemaining > 0 ? t("reminder.inMinutes", { n: alert.minutesRemaining }) : t("reminder.startingNow");
}

export function meetingAlertSubject(alert: ReminderAlert): string {
  return alert.subject.trim() || t("calendar.noSubject");
}

export function meetingAlertTimeRange(alert: ReminderAlert): string {
  return `${formatTime(new Date(alert.startUtc))} – ${formatTime(new Date(alert.endUtc))}`;
}

/** What a screen reader says when the alert appears. */
export function meetingAlertAnnouncement(alert: ReminderAlert): string {
  return [meetingAlertLabel(alert), meetingAlertSubject(alert), meetingAlertTimeRange(alert), alert.location].filter(Boolean).join(". ");
}

interface MeetingAlertProps {
  /** The user joined the meeting from the alert (it can go away). */
  onJoin?: () => void;
  /** "Remind me in 5 min": the alert goes away and comes back then. */
  onSnooze?: () => void;
  alert: ReminderAlert;
}

/**
 * Content of the island while a meeting reminder shows. It only fills the island: the island
 * itself (and the native window) grow around it, sized by meetingAlertSize. The layout follows the
 * UI language (Hebrew: everything aligns right); each text shapes itself (`bidi`). The layer never
 * takes pointer events or focus except through its action row, so it cannot get in the way; the
 * announcement for assistive tech is made by the shell's live region.
 */
export function MeetingAlert({ alert, onJoin, onSnooze }: MeetingAlertProps) {
  return (
    <IslandLayer
      name="meetingAlert"
      fade={layerFade.temporary}
      size={alertIslandSize(alert)}
      dir={uiDirection()}
      className="flex flex-col select-none pointer-events-none"
      style={{ padding: smallExpanded.padding, color: color.fg }}
    >
      {/* The context label is the meta role at weight 600 (the role itself is 500). */}
      <span className="bidi truncate text-meta" style={{ color: color.accent, fontWeight: 600 }}>
        {meetingAlertLabel(alert)}
      </span>
      <h3 dir={textDirection(meetingAlertSubject(alert))} className="bidi line-clamp-2 text-title" style={{ marginTop: alertTokens.labelGap, overflowWrap: "anywhere" }}>
        <BidiText text={meetingAlertSubject(alert)} />
      </h3>
      <span className="bidi truncate text-body tabular-nums" style={{ marginTop: alertTokens.labelGap, color: color.fgSecondary }}>
        {/* A time range is an isolated LTR token (BidiText): "11:00 – 12:00" keeps its order in Hebrew. */}
        <BidiText text={meetingAlertTimeRange(alert)} />
      </span>
      {alert.location && (
        <span dir={textDirection(alert.location)} className="bidi truncate text-body" style={{ marginTop: alertTokens.detailGap, color: color.fgTertiary }}>
          <BidiText text={alert.location} />
        </span>
      )}
      {alertHasActions(alert) && (
        <ActionRow className="pointer-events-auto" style={{ marginTop: alertTokens.actionsGap }}>
          {alert.meetingUrl && <JoinButton url={alert.meetingUrl} subject={meetingAlertSubject(alert)} onJoined={onJoin} />}
          {canSnooze(alert) && (
            <ActionButton variant="neutral" onPress={() => onSnooze?.()}>
              {t("reminder.snooze")}
            </ActionButton>
          )}
        </ActionRow>
      )}
    </IslandLayer>
  );
}
