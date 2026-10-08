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
  if (source.kind === "primary") return [...keys, "calendar.sourceAlwaysOn"];
  if (!source.active) keys.push(source.pendingInOutlook ? "calendar.sourceOff" : "calendar.sourceNotSelected");
  else if (source.state === "unavailable") keys.push("calendar.sourceUnavailable");
  else if (source.state === "pending") keys.push("calendar.sourcePending");
  if (source.pendingInOutlook) keys.push("calendar.sourcePendingOutlook");
  return keys;
}

/** Whether the island offers a switch for this calendar: every one but the user's default calendar, which is always on. */
export function canSwitchSource(source: CalendarSourceDto): boolean {
  return source.kind !== "primary";
}

/** A switch the user just turned, until a report from Outlook catches up with it (or 15 s go by). */
export type SwitchRequest = { on: boolean; atMs: number };

const REQUEST_HOLD_MS = 15_000;

/** The calendar as the list should show it: the user's latest request wins until the backend reports it. */
export function withRequest(source: CalendarSourceDto, request: SwitchRequest | undefined): CalendarSourceDto {
  if (!request || request.on === source.active) return source;
  return { ...source, active: request.on, selected: request.on, pendingInOutlook: true, state: request.on ? "pending" : "notSelected" };
}

/** Requests still worth holding against `report`: not yet reflected, and recent. */
export function openRequests(report: CalendarSourcesReport, requests: Record<string, SwitchRequest>, nowMs: number): Record<string, SwitchRequest> {
  const open: Record<string, SwitchRequest> = {};
  for (const [id, request] of Object.entries(requests)) {
    const source = report.sources.find((s) => s.id === id);
    if (source && source.active !== request.on && nowMs - request.atMs < REQUEST_HOLD_MS) open[id] = request;
  }
  return open;
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
