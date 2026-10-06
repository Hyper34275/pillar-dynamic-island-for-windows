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
  subject: string;
  startUtc: string;
  endUtc: string;
  location: string | null;
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
}
