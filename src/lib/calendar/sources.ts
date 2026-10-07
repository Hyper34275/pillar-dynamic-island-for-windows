// Presentation helpers for calendar sources (Outlook's My / Shared / Other calendars). Pure.

import type { MessageKey } from "../i18n";
import type { CalendarEventDto, CalendarSourceDto, CalendarSourceKind, CalendarSourcesReport } from "./types";

const KIND_LABEL: Record<CalendarSourceKind, MessageKey> = {
  primary: "calendar.kindPrimary",
  personal: "calendar.kindPersonal",
  shared: "calendar.kindShared",
  other: "calendar.kindOther",
};

export function kindLabelKey(kind: CalendarSourceKind): MessageKey {
  return KIND_LABEL[kind];
}

/**
 * The small "which calendar" label of an event, or null when it would only add noise: events of
 * the user's own default calendar carry none. A calendar without a name falls back to its kind.
 */
export function eventSourceLabel(event: Pick<CalendarEventDto, "sourceKind" | "calendarName">, kindLabel: (key: MessageKey) => string): string | null {
  const kind = event.sourceKind ?? "primary";
  if (kind === "primary") return null;
  const name = event.calendarName?.trim();
  return name ? name : kindLabel(KIND_LABEL[kind]);
}

/** The secondary line of one source in the sources list: what it is, and why it gives nothing when it does not. */
export function sourceStatusKeys(source: CalendarSourceDto): MessageKey[] {
  const keys: MessageKey[] = [KIND_LABEL[source.kind]];
  if (source.kind === "primary") keys.push("calendar.sourceAlwaysOn");
  else if (!source.active) keys.push("calendar.sourceNotSelected");
  else if (source.state === "unavailable") keys.push("calendar.sourceUnavailable");
  else if (source.state === "pending") keys.push("calendar.sourcePending");
  return keys;
}

/** Discovered vs active counts. */
export function sourceCounts(report: CalendarSourcesReport | null | undefined): { total: number; active: number } {
  const sources = report?.sources ?? [];
  return { total: sources.length, active: sources.filter((s) => s.active).length };
}

/** The list is worth showing once there is more than the default calendar to talk about. */
export function hasSourcesToShow(report: CalendarSourcesReport | null | undefined): report is CalendarSourcesReport {
  return !!report && report.sources.length > 1;
}
