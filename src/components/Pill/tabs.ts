import type { MessageKey } from "../../lib/i18n";
import { BellIcon, CalendarIcon, GearIcon, InfoIcon } from "./ui/icons";

export type TabId = "calendar" | "notifications" | "about" | "settings";

export interface TabConfig {
  id: TabId;
  labelKey: MessageKey;
  Icon: typeof CalendarIcon;
}

// Order is the dock order and the arrow-key order.
export const TABS: readonly TabConfig[] = [
  { id: "calendar", labelKey: "tab.calendar", Icon: CalendarIcon },
  { id: "notifications", labelKey: "tab.notifications", Icon: BellIcon },
  { id: "about", labelKey: "tab.about", Icon: InfoIcon },
  { id: "settings", labelKey: "tab.settings", Icon: GearIcon },
];

export function isTabId(value: unknown): value is TabId {
  return TABS.some((tab) => tab.id === value);
}
