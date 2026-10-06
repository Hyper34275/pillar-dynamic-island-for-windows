import type { MessageKey } from "../../lib/i18n";
import { CalendarIcon, ClockIcon, InfoIcon } from "./ui/icons";

export type TabId = "datetime" | "calendar" | "about";

export interface TabConfig {
  id: TabId;
  labelKey: MessageKey;
  Icon: typeof ClockIcon;
}

// Order is the dock order and the arrow-key order.
export const TABS: readonly TabConfig[] = [
  { id: "datetime", labelKey: "tab.datetime", Icon: ClockIcon },
  { id: "calendar", labelKey: "tab.calendar", Icon: CalendarIcon },
  { id: "about", labelKey: "tab.about", Icon: InfoIcon },
];

export function isTabId(value: unknown): value is TabId {
  return TABS.some((tab) => tab.id === value);
}
