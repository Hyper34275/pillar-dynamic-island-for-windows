// The toast's shape: as narrow as its content, never wider than the small-expanded family, and as
// tall as its lines. Measured once per notification (canvas text measurement), so PillShell's
// target, the window region and the toast layer always agree.

import { control, icon, space, toast, type as typeRoles, type TypeRole } from "../../design/tokens";
import type { IslandNotification } from "../../lib/ipc";
import { t } from "../../lib/i18n";
import { measureText } from "../../lib/textMeasure";
import type { IslandSize } from "./animations";
import { cleanAppName } from "./ui/primitives";
import { uiFontFamily } from "./useCompactLayout";

/**
 * Room the hover-only dismiss button keeps at the end of the source row, always reserved (no reflow
 * on hover): the 28px slot + the meta slot's 8px start padding + the 4px row gap (notification.tsx).
 */
export const DISMISS_SLOT = control.round + space[2] + space[1];
/** Wrapping wastes part of every line: a text filling a line this much is already one more line. */
const WRAP_SLACK = 0.95;
/** Canvas and DOM text widths differ by sub-pixel rounding; a single-line text gets this much air so it never ellipsizes. */
const MEASURE_AIR = 2;

function measure(text: string, role: TypeRole): number {
  return measureText(text, `${role.weight} ${role.size}px ${uiFontFamily()}`, role.size);
}

export interface ToastLayout {
  size: IslandSize;
  /** Body lines as laid out (0 without a body). */
  bodyLines: 0 | 1 | 2;
}

/** The label above the title: the app's name, or "Meeting invitation". */
export function toastSource(notification: IslandNotification): string {
  return notification.invite ? t("invite.label") : cleanAppName(notification.appName) || notification.appName;
}

export function toastTitle(notification: IslandNotification): string {
  return notification.title || t("notif.default");
}

export function toastLayout(notification: IslandNotification): ToastLayout {
  const chrome = toast.padding * 2 + icon.app + toast.iconGap;
  const maxText = toast.maxWidth - chrome;
  const sourceWidth = measure(toastSource(notification), typeRoles.meta) + DISMISS_SLOT;
  const titleWidth = measure(toastTitle(notification), typeRoles.headline);
  const bodyWidth = notification.body ? measure(notification.body, typeRoles.body) : 0;
  // A body that fits one line (with wrapping slack) stays one line and sizes the toast; a longer one
  // wraps to two lines at the full width.
  const bodyLines: 0 | 1 | 2 = !notification.body ? 0 : bodyWidth <= maxText * WRAP_SLACK ? 1 : 2;
  const bodyNeed = bodyLines === 0 ? 0 : bodyLines === 1 ? bodyWidth : maxText;
  // An invitation carries three actions: it always takes the full width so they have room.
  const wanted = notification.invite ? maxText : Math.max(sourceWidth, titleWidth, bodyNeed) + MEASURE_AIR;
  const textWidth = Math.min(maxText, Math.ceil(wanted));
  // Fractional DIPs are fine: Rust snaps the edges to pixels (textWidth is already ceil'd, so no glyph clips).
  const width = Math.max(toast.minWidth, chrome + textWidth);

  const stack =
    typeRoles.meta.lineHeight + toast.stackGap + typeRoles.headline.lineHeight + (bodyLines ? toast.stackGap + typeRoles.body.lineHeight * bodyLines : 0);
  let height = toast.padding * 2 + Math.max(icon.app, stack);
  if (notification.invite) height += toast.actionsGap + control.height;
  return { size: { width, height, radius: Math.min(toast.radius, height / 2) }, bodyLines };
}

/** The tallest and widest a toast can be (the native window's stage must hold it). */
export function toastMaxSize(): IslandSize {
  const stack = typeRoles.meta.lineHeight + typeRoles.headline.lineHeight + typeRoles.body.lineHeight * toast.bodyMaxLines + toast.stackGap * 2;
  const height = toast.padding * 2 + stack + toast.actionsGap + control.height;
  return { width: toast.maxWidth, height, radius: toast.radius };
}
