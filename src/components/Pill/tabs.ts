import type { ComponentType } from "react";
import type { MessageKey } from "../../lib/i18n";
import { NotificationsClearAction } from "./panels/NotificationsTab";
import { BellIcon, CalendarIcon, GearIcon, InfoIcon, NoteIcon } from "./ui/icons";

export type TabId = "calendar" | "notifications" | "notes" | "about" | "settings";

export interface TabConfig {
  id: TabId;
  labelKey: MessageKey;
  Icon: typeof CalendarIcon;
  /** An action at the trailing edge of the panel header, in the title's row (e.g. "Clear all"). */
  HeaderAction?: ComponentType;
}

/**
 * The dock always runs left to right, in every UI language: the first tab (Calendar) is the
 * leftmost and Settings the rightmost. The arrow keys, swipes and the panel slide follow it.
 */
export const DOCK_DIRECTION = "ltr" as const;

// Order is the dock order and the arrow-key order, left to right.
export const TABS: readonly TabConfig[] = [
  { id: "calendar", labelKey: "tab.calendar", Icon: CalendarIcon },
  { id: "notifications", labelKey: "tab.notifications", Icon: BellIcon, HeaderAction: NotificationsClearAction },
  { id: "notes", labelKey: "tab.notes", Icon: NoteIcon },
  { id: "about", labelKey: "tab.about", Icon: InfoIcon },
  { id: "settings", labelKey: "tab.settings", Icon: GearIcon },
];

export function isTabId(value: unknown): value is TabId {
  return TABS.some((tab) => tab.id === value);
}
