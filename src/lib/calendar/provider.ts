// Calendar providers.
//
// The UI and the reminder engine only ever talk to a CalendarProvider through the
// CalendarService (service.ts); nothing outside this folder knows where meetings come from.
//
// Adding a source (e.g. a MicrosoftGraphCalendarProvider):
//   1. Implement CalendarProvider in its own file. Emit snapshots in the shape of
//      docs/ENTERPRISE_DESIGN.md section 1 (run foreign data through normalizeSnapshot).
//   2. Register it in registry.ts (registerCalendarProvider). Every registered provider is
//      started with the app; the service merges their events (selectEvents/mergeSnapshots).
// Nothing else changes. Event ids must be unique per calendar (events are keyed by
// calendarId + id + startUtc), and providers must never throw into their callers.

import type { CalendarSnapshot } from "./types";

export interface CalendarProvider {
  readonly id: string;
  /** Calls `listener` whenever the snapshot changes. Returns the unsubscribe function. */
  subscribe(listener: (snapshot: CalendarSnapshot) => void): () => void;
  getSnapshot(): CalendarSnapshot;
  /** Asks the source to sync now. Resolves when done; never rejects. */
  refresh(): Promise<void>;
  dispose(): void;
}

export type CalendarProviderFactory = () => CalendarProvider;
