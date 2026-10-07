import type { ReactNode } from "react";
import { color, control } from "../../../design/tokens";
import { inviteAnswers, useInviteAnswer, type InviteAnswers } from "../../../lib/calendar/inviteAnswers";
import { t, type MessageKey } from "../../../lib/i18n";
import { ipc, type InviteResponse } from "../../../lib/ipc";
import { ActionButton, ActionRow } from "./controls";
import { CheckIcon, VideoIcon } from "./icons";

/** "Join": opens the meeting's join link (the Teams app for Teams links). Sits in an ActionRow. */
export function JoinButton({ url, subject, onJoined }: { url: string; subject: string; onJoined?: () => void }) {
  return (
    <ActionButton
      variant="primary"
      ariaLabel={t("calendar.joinAria", { subject })}
      icon={<VideoIcon size={control.iconSize} strokeWidth={2.2} />}
      onPress={() => {
        void ipc.openMeetingUrl(url);
        onJoined?.();
      }}
    >
      {t("calendar.join")}
    </ActionButton>
  );
}

const DONE_LABEL: Record<InviteResponse, MessageKey> = {
  accept: "invite.accepted",
  tentative: "invite.tentativeDone",
  decline: "invite.declined",
};

/** A one-line state (sending, answered) with the same height as the buttons it replaces, so the card never jumps. */
function StatusLine({ tone, children }: { tone: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-center text-label" role="status" style={{ height: control.height, gap: control.gap, color: tone }}>
      {children}
    </div>
  );
}

/**
 * Accept / maybe / decline for a meeting invitation, answered through the user's own Outlook
 * (which sends the reply to the organizer). Three equal buttons across the full width; once
 * answered (or while sending) a status line of the same height replaces them. If Outlook could
 * not take the answer, the line says so and pressing it sends the same answer again.
 */
export function InviteActions({
  inviteId,
  showTentative = true,
  store = inviteAnswers,
}: {
  inviteId: string;
  showTentative?: boolean;
  store?: InviteAnswers;
}) {
  const answer = useInviteAnswer(inviteId, store);
  const respond = (response: InviteResponse) => void store.respond(inviteId, response);
  if (answer?.state === "done") {
    return (
      <StatusLine tone={answer.response === "decline" ? color.destructiveText : color.positive}>
        <CheckIcon size={control.iconSize} strokeWidth={2.6} />
        <span className="bidi">{t(DONE_LABEL[answer.response])}</span>
      </StatusLine>
    );
  }
  if (answer?.state === "sending") {
    return (
      <StatusLine tone={color.fgTertiary}>
        <span className="bidi">{t("invite.sending")}</span>
      </StatusLine>
    );
  }
  if (answer?.state === "failed") {
    const retry = answer.response;
    return (
      <ActionRow>
        <ActionButton variant="neutral" onPress={() => respond(retry)}>
          <span className="bidi" role="alert" style={{ color: color.warning }}>
            {t("invite.failed")}
          </span>
        </ActionButton>
      </ActionRow>
    );
  }
  return (
    <ActionRow>
      <ActionButton variant="primary" onPress={() => respond("accept")}>
        {t("invite.accept")}
      </ActionButton>
      {showTentative && (
        <ActionButton variant="neutral" onPress={() => respond("tentative")}>
          {t("invite.tentative")}
        </ActionButton>
      )}
      <ActionButton variant="destructive" onPress={() => respond("decline")}>
        {t("invite.decline")}
      </ActionButton>
    </ActionRow>
  );
}

