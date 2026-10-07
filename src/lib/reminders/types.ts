import type { CalendarEventDto, CalendarSourceKind } from "../calendar/types";

/** Fire a reminder `minutes` before the event starts (0 = at the start). Other kinds can join later. */
export type ReminderType = { kind: "beforeStart"; minutes: number };

/** Stable id used inside persisted keys: "minutes-30", "start". */
export function reminderTypeId(type: ReminderType): string {
  return type.minutes === 0 ? "start" : `minutes-${type.minutes}`;
}

/** The persisted key of one reminder: "<eventId>|<startUtc>|<type>". Holds no meeting text. */
export function reminderKey(eventId: string, startUtc: string, type: ReminderType): string {
  return `${eventId}|${startUtc}|${reminderTypeId(type)}`;
}

export interface ReminderAlert {
  key: string;
  eventId: string;
  /** The calendar the event came from (presentation and policy; not part of the key). */
  calendarId?: string;
  calendarName?: string;
  sourceKind?: CalendarSourceKind;
  subject: string;
  startUtc: string;
  endUtc: string;
  location: string | null;
  /** The join link, when the meeting has one (a Join button on the alert). */
  meetingUrl?: string | null;
  /** Whole minutes left when the alert fires; a late alert reports what is actually left. */
  minutesRemaining: number;
  reminderType: ReminderType;
}

/** Fired-set persistence: key -> firedAtUnixMs. */
export interface ReminderStore {
  load(): Promise<Record<string, number>>;
  save(fired: Record<string, number>): Promise<void>;
}

export interface ReminderSettings {
  enabled: boolean;
  /** Minutes before the start, one reminder per entry (V1 passes [reminderMinutes]). */
  offsetsMinutes: readonly number[];
  /** Which calendars remind; absent = every active calendar. */
  sources?: ReminderSourcePolicy;
}

/**
 * Which calendars' events remind. Kept apart from discovery: the backend decides which calendars
 * are active (what is shown), this decides which of those remind.
 * - "all": every active calendar (the user's own and the checked shared ones)
 * - "own": the user's own calendars only (primary + personal); shared events are shown, not reminded
 * - "calendars": exactly these calendar ids (for a later per-calendar setting)
 */
export type ReminderSourcePolicy = { kind: "all" } | { kind: "own" } | { kind: "calendars"; calendarIds: readonly string[] };

export function remindsFor(policy: ReminderSourcePolicy | undefined, event: Pick<CalendarEventDto, "calendarId" | "sourceKind">): boolean {
  if (!policy || policy.kind === "all") return true;
  if (policy.kind === "own") return isOwnCalendar(event.sourceKind);
  return policy.calendarIds.includes(event.calendarId);
}

/** The user's own calendars come first when several reminders are due together. */
export function isOwnCalendar(kind: CalendarSourceKind | undefined): boolean {
  return kind === undefined || kind === "primary" || kind === "personal";
}
