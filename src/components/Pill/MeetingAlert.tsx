import { motion } from "motion/react";
import { formatTime } from "../../lib/dateFormat";
import { t } from "../../lib/i18n";
import type { ReminderAlert } from "../../lib/reminders/types";
import { pillDimensions } from "./animations";
import { ALERT_SUBJECT_FONT_SIZE } from "./alertLayout";
import { SYSTEM_COLORS } from "./ui/primitives";

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
  alert: ReminderAlert;
  reducedMotion: boolean;
}

// Physically left-to-right like the rest of the island; each text picks its own glyph direction
// (dir="auto"), so Hebrew subjects shape correctly without flipping the layout.
const TEXT_BASE = { textAlign: "left", unicodeBidi: "plaintext" } as const;

/**
 * Content of the island while a meeting reminder shows. It only fills the island: the island
 * itself (and the native window) grow around it, sized by meetingAlertSize. It never takes
 * pointer events or focus, so it cannot get in the way of what the user is doing; the
 * announcement for assistive tech is made by the shell's live region.
 */
export function MeetingAlert({ alert, reducedMotion }: MeetingAlertProps) {
  return (
    <motion.div
      dir="ltr"
      className="absolute inset-0 flex flex-col justify-center select-none pointer-events-none"
      style={{ paddingInline: a.paddingX, paddingBlock: a.paddingY, gap: a.gap, color: "#f5f5f7" }}
      initial={{ opacity: 0, y: reducedMotion ? 0 : -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      transition={{ duration: reducedMotion ? 0.05 : 0.22, delay: reducedMotion ? 0 : 0.08, ease: [0.2, 0.8, 0.2, 1] }}
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
    </motion.div>
  );
}
