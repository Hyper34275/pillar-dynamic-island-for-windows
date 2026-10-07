import type { MouseEvent, ReactNode } from "react";
import { sourceDirection, textDirection, type Dir } from "../../../design/direction";
import { card, color, icon, space, toast } from "../../../design/tokens";
import { getLocale, t } from "../../../lib/i18n";
import type { IslandNotification } from "../../../lib/ipc";
import { BidiText } from "./BidiText";
import { cleanAppName } from "./primitives";

// =============================================================================
// The notification grammar, shared by the toast in the island and the cards of the Notification
// Center. Only content and identity differ between apps; the structure never does:
//
//   [icon]  SOURCE · adornment ................ meta (time / unread / dismiss)
//           Title
//           Body (clamped)
//   [ action ][ action ][ action ]        ← full content width, under icon and text
//
// The icon's top aligns with the top of the text stack (the source line), so it anchors the
// stack instead of floating at the card's mathematical centre.
//
// DIRECTION (design/direction.ts): `dir` is the SHELL's, given by the caller. The Notification
// Center always passes the UI direction (the icon is on the same side in every card); the
// standalone toast passes its content's. Inside the shell each field has its own rule: the source
// name is isolated with its own direction ("Teams" stays LTR in a Hebrew card), title and body are
// paragraphs (Hebrew anywhere -> RTL) whose technical tokens (paths, URLs, times...) are isolated
// LTR runs. Alignment always follows the shell (the `bidi` class inherits it), so a row of cards
// shares one edge whatever language each card is in.
//
// SEMANTICS. A notification surface has one focus stop for its own action (NotificationPrimary:
// a real <button> stretched over the surface, named by the whole notification) and separate
// buttons for dismiss and actions; nothing interactive is ever inside something interactive. The
// visible text is aria-hidden because the primary button's name already says it, once.
// =============================================================================

export interface NotificationContentProps {
  /** The SHELL direction: which side the icon is on. The text fields choose their own (see above). */
  dir: Dir;
  icon: ReactNode;
  source: string;
  /** The invitation's label is the calendar's blue; app names are tertiary. */
  sourceTone?: "default" | "accent";
  /** Right after the source (e.g. the silenced bell). */
  sourceAdornment?: ReactNode;
  /** The end of the source row: time, unread dot, dismiss. Its position never depends on the text. */
  meta?: ReactNode;
  title: string;
  titleLines?: 1 | 2;
  body?: string;
  bodyLines?: 1 | 2 | 3;
  /** An ActionRow; spans the full content width under the icon and the text. */
  actions?: ReactNode;
  /** Gap between icon and text (toast and card share 12). */
  iconGap?: number;
  /** The text is read through a NotificationPrimary's label: hide it from the accessibility tree. */
  textHidden?: boolean;
}

function clamp(lines: number) {
  return lines === 1 ? "truncate" : lines === 2 ? "line-clamp-2" : "line-clamp-3";
}

export function NotificationContent({
  dir,
  icon: tile,
  source,
  sourceTone = "default",
  sourceAdornment,
  meta,
  title,
  titleLines = toast.titleMaxLines,
  body,
  bodyLines = toast.bodyMaxLines,
  actions,
  iconGap = card.iconGap,
  textHidden = false,
}: NotificationContentProps) {
  const hidden = textHidden ? true : undefined;
  return (
    // pointer-events none: a press on the text reaches the primary button underneath; the parts
    // that are interactive (meta, actions) switch it back on.
    <div dir={dir} className="relative grid min-w-0 pointer-events-none" style={{ gridTemplateColumns: `${icon.app}px minmax(0, 1fr)`, columnGap: iconGap }}>
      <div className="self-start">{tile}</div>
      <div className="min-w-0 flex flex-col" style={{ gap: toast.stackGap }}>
        <div className="flex items-center min-w-0 text-meta" style={{ gap: space[1] }}>
          <span
            dir={sourceDirection(source)}
            aria-hidden={hidden}
            className="bidi min-w-0 truncate"
            style={{ color: sourceTone === "accent" ? color.accent : color.fgTertiary }}
          >
            {source}
          </span>
          {sourceAdornment}
          {meta && (
            <span className="ms-auto ps-2 flex-shrink-0 flex items-center pointer-events-auto" style={{ gap: space[1] }}>
              {meta}
            </span>
          )}
        </div>
        <span dir={textDirection(title)} aria-hidden={hidden} className={`bidi text-headline ${clamp(titleLines)}`} style={{ color: color.fg, overflowWrap: "anywhere" }}>
          <BidiText text={title} />
        </span>
        {body && (
          <span dir={textDirection(body)} aria-hidden={hidden} className={`bidi text-body ${clamp(bodyLines)}`} style={{ color: color.fgSecondary, overflowWrap: "anywhere" }}>
            <BidiText text={body} />
          </span>
        )}
      </div>
      {actions && (
        <div className="pointer-events-auto" style={{ gridColumn: "1 / -1", marginTop: toast.actionsGap }} onClick={(e) => e.stopPropagation()}>
          {actions}
        </div>
      )}
    </div>
  );
}

interface NotificationPrimaryProps {
  /** The whole notification as one sentence (notificationAccessibleLabel). */
  label: string;
  onPress: () => void;
  title?: string;
  /** Pointer press, for a toast that tells a click from a swipe. */
  onClick?: (event: MouseEvent<HTMLButtonElement>) => void;
}

/**
 * The surface's own action (open the source app): a transparent <button> stretched over the
 * whole positioned parent, painted under the content (it comes first in the DOM, the content is
 * `relative` after it). Its accessible name is the whole notification, so a screen reader hears
 * it once, when focus lands on it; dismiss and the action buttons are its siblings, never its
 * children. The focus ring is drawn inside the surface (negative offset) so a clipping island
 * never cuts it.
 */
export function NotificationPrimary({ label, onPress, title, onClick }: NotificationPrimaryProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={title}
      data-notification-primary=""
      className="absolute inset-0 w-full h-full cursor-pointer"
      style={{ borderRadius: "inherit", background: "transparent", outlineOffset: -2 }}
      onClick={(e) => (onClick ? onClick(e) : onPress())}
    />
  );
}

// -----------------------------------------------------------------------------
// What a screen reader hears
// -----------------------------------------------------------------------------

const STRINGS = {
  en: {
    from: (source: string) => `${source} notification`,
    announce: (source: string, title: string) => `New notification from ${source}: ${title}`,
    announceInvite: (title: string) => `New meeting invitation: ${title}`,
  },
  he: {
    from: (source: string) => `התראה מ-${source}`,
    announce: (source: string, title: string) => `התראה חדשה מ-${source}: ${title}`,
    announceInvite: (title: string) => `זימון חדש לפגישה: ${title}`,
  },
} as const;

const strings = () => STRINGS[getLocale()];

/** Joins sentences with ". " unless the previous one already ends in punctuation. */
function sentences(parts: Array<string | undefined | false>): string {
  let out = "";
  for (const part of parts) {
    if (!part) continue;
    out = out ? `${out}${/[.!?…:]$/.test(out) ? "" : "."} ${part}` : part;
  }
  return out;
}

/**
 * One coherent sentence for a notification, as the primary button's name:
 * "Teams notification. Dana Cohen. Can we review the presentation before the meeting? 12 minutes ago."
 * An invitation's source already says what it is ("Meeting invitation").
 */
export function notificationAccessibleLabel(parts: {
  source: string;
  title: string;
  body?: string;
  received?: string;
  /** An invitation: the source is its own kind label. */
  invite?: boolean;
  silenced?: boolean;
}): string {
  const kind = parts.invite ? parts.source : strings().from(parts.source);
  const out = sentences([kind, parts.title, parts.body, parts.silenced && t("notifs.silenced"), parts.received]);
  return /[.!?…]$/.test(out) ? out : `${out}.`;
}

/**
 * The polite live-region text for a NEW notification (never role=alert, never moves focus):
 * "התראה חדשה מ-Teams: <title>". The body is not read: it can be long or private, and the person
 * can open the island to read it.
 */
export function notificationAnnouncement(notification: IslandNotification): string {
  const title = notification.title || t("notif.default");
  if (notification.invite) return strings().announceInvite(title);
  const source = cleanAppName(notification.appName) || notification.appName;
  return strings().announce(source, title);
}
