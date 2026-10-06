import { formatTime } from "../../lib/dateFormat";
import { t } from "../../lib/i18n";
import type { ReminderAlert } from "../../lib/reminders/types";
import { layerFade, pillDimensions } from "./animations";
import { ALERT_SUBJECT_FONT_SIZE, alertHasActions, alertIslandSize, canSnooze } from "./alertLayout";
import { IslandLayer } from "./IslandLayer";
import { SYSTEM_COLORS } from "./ui/primitives";
import { JoinButton } from "./ui/meetingActions";

const a = pillDimensions.alert;

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

// Physically left-to-right like the rest of the island; each text picks its own glyph direction
// (dir="auto"), so Hebrew subjects shape correctly without flipping the layout.
const TEXT_BASE = { textAlign: "left", unicodeBidi: "plaintext" } as const;

/**
 * Content of the island while a meeting reminder shows. It only fills the island: the island
 * itself (and the native window) grow around it, sized by meetingAlertSize. It never takes
 * pointer events or focus (only its Join / snooze buttons do), so it cannot get in the way; the
 * announcement for assistive tech is made by the shell's live region.
 */
export function MeetingAlert({ alert, onJoin, onSnooze }: MeetingAlertProps) {
  return (
    <IslandLayer
      name="meetingAlert"
      fade={layerFade.temporary}
      size={alertIslandSize(alert)}
      dir="ltr"
      className="flex flex-col justify-center select-none pointer-events-none"
      style={{ paddingInline: a.paddingX, paddingBlock: a.paddingY, gap: a.gap, color: "#f5f5f7" }}
    >
      <span
        dir="auto"
        className="truncate text-[12px] font-semibold"
        style={{ ...TEXT_BASE, height: a.labelHeight, lineHeight: `${a.labelHeight}px`, color: SYSTEM_COLORS.blue }}
      >
        {meetingAlertLabel(alert)}
      </span>
      <h3
        dir="auto"
        className="line-clamp-2 font-semibold"
        style={{ ...TEXT_BASE, fontSize: ALERT_SUBJECT_FONT_SIZE, lineHeight: `${a.subjectLineHeight}px`, overflowWrap: "anywhere" }}
      >
        {meetingAlertSubject(alert)}
      </h3>
      <span
        dir="auto"
        className="truncate text-[13px] tabular-nums text-white/70"
        style={{ ...TEXT_BASE, height: a.detailHeight, lineHeight: `${a.detailHeight}px` }}
      >
        {meetingAlertTimeRange(alert)}
      </span>
      {alert.location && (
        <span
          dir="auto"
          className="truncate text-[12.5px] text-white/50"
          style={{ ...TEXT_BASE, height: a.detailHeight, lineHeight: `${a.detailHeight}px` }}
        >
          {alert.location}
        </span>
      )}
      {alertHasActions(alert) && (
        <div className="flex items-center gap-2 pointer-events-auto" style={{ height: pillDimensions.alertActions.height, marginTop: pillDimensions.alertActions.gap - a.gap }}>
          {alert.meetingUrl && <JoinButton url={alert.meetingUrl} subject={meetingAlertSubject(alert)} height={28} onJoined={onJoin} />}
          {canSnooze(alert) && (
            <button
              type="button"
              className="inline-flex items-center h-[28px] rounded-full px-3 text-[12.5px] font-semibold text-white/80 bg-white/[0.1] hover:bg-white/[0.16] flex-shrink-0"
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                onSnooze?.();
              }}
            >
              <span dir="auto">{t("reminder.snooze")}</span>
            </button>
          )}
        </div>
      )}
    </IslandLayer>
  );
}
