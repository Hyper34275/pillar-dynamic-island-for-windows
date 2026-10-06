import type { MouseEvent, ReactNode } from "react";
import { inviteAnswers, useInviteAnswer, type InviteAnswers } from "../../../lib/calendar/inviteAnswers";
import { t, type MessageKey } from "../../../lib/i18n";
import { ipc, type InviteResponse } from "../../../lib/ipc";
import { CalendarIcon, CheckIcon, VideoIcon } from "./icons";
import { SYSTEM_COLORS } from "./primitives";

/** Buttons inside clickable surfaces (a toast, the island) must not trigger the surface too. */
function stop(e: MouseEvent) {
  e.stopPropagation();
}

function ActionButton({
  label,
  tint,
  onClick,
  children,
  ariaLabel,
  height = 28,
}: {
  label: string;
  tint: string;
  onClick: () => void;
  children?: ReactNode;
  ariaLabel?: string;
  height?: number;
}) {
  return (
    <button
      type="button"
      className="inline-flex items-center justify-center gap-1.5 rounded-full px-3 font-semibold transition-[filter] hover:brightness-125 active:brightness-90 flex-shrink-0"
      style={{ height, fontSize: height >= 28 ? 12.5 : 11.5, color: tint, background: `color-mix(in srgb, ${tint} 20%, transparent)` }}
      aria-label={ariaLabel ?? label}
      onPointerDown={stop}
      onClick={(e) => {
        stop(e);
        onClick();
      }}
    >
      {children}
      <span dir="auto">{label}</span>
    </button>
  );
}

/** "Join": opens the meeting's join link (the Teams app for Teams links). */
export function JoinButton({ url, subject, height, onJoined }: { url: string; subject: string; height?: number; onJoined?: () => void }) {
  return (
    <ActionButton
      label={t("calendar.join")}
      ariaLabel={t("calendar.joinAria", { subject })}
      tint={SYSTEM_COLORS.green}
      height={height}
      onClick={() => {
        void ipc.openMeetingUrl(url);
        onJoined?.();
      }}
    >
      <VideoIcon size={height && height < 28 ? 13 : 15} strokeWidth={2.2} />
    </ActionButton>
  );
}

const DONE_LABEL: Record<InviteResponse, MessageKey> = {
  accept: "invite.accepted",
  tentative: "invite.tentativeDone",
  decline: "invite.declined",
};

/**
 * Accept / maybe / decline for a meeting invitation, answered through the user's own Outlook
 * (which sends the reply to the organizer). Shows the answer once given.
 */
export function InviteActions({
  inviteId,
  height = 28,
  showTentative = true,
  store = inviteAnswers,
}: {
  inviteId: string;
  height?: number;
  showTentative?: boolean;
  store?: InviteAnswers;
}) {
  const answer = useInviteAnswer(inviteId, store);
  if (answer?.state === "done") {
    const declined = answer.response === "decline";
    return (
      <span
        className="inline-flex items-center gap-1 text-[12px] font-semibold"
        style={{ color: declined ? SYSTEM_COLORS.red : SYSTEM_COLORS.green }}
        role="status"
      >
        <CheckIcon size={13} strokeWidth={2.6} />
        <span dir="auto">{t(DONE_LABEL[answer.response])}</span>
      </span>
    );
  }
  if (answer?.state === "sending") {
    return (
      <span className="text-[12px] font-medium text-white/55" role="status" dir="auto">
        {t("invite.sending")}
      </span>
    );
  }
  const respond = (response: InviteResponse) => void store.respond(inviteId, response);
  return (
    <span className="inline-flex items-center gap-1.5 flex-wrap">
      <ActionButton label={t("invite.accept")} tint={SYSTEM_COLORS.green} height={height} onClick={() => respond("accept")} />
      {showTentative && <ActionButton label={t("invite.tentative")} tint="#c7c7cc" height={height} onClick={() => respond("tentative")} />}
      <ActionButton label={t("invite.decline")} tint={SYSTEM_COLORS.red} height={height} onClick={() => respond("decline")} />
      {answer?.state === "failed" && (
        <span className="text-[11px] font-medium" style={{ color: SYSTEM_COLORS.orange }} role="alert" dir="auto">
          {t("invite.failed")}
        </span>
      )}
    </span>
  );
}

/** The tile in front of a meeting invitation: a calendar, tinted like the app's meeting accents. */
export function InviteAvatar({ size = 40 }: { size?: number }) {
  return (
    <div
      className="flex items-center justify-center flex-shrink-0 text-white"
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.3),
        background: `linear-gradient(160deg, ${SYSTEM_COLORS.blue}, color-mix(in srgb, ${SYSTEM_COLORS.blue} 70%, black))`,
        boxShadow: "inset 0 0.5px 0 rgba(255,255,255,0.25)",
      }}
      aria-hidden="true"
    >
      <CalendarIcon size={Math.round(size / 2)} strokeWidth={2.2} />
    </div>
  );
}
