// Tolerant normalisation of the backend's calendar payload: the UI never trusts the wire shape.

import type {
  CalendarBusyStatus,
  CalendarEventDto,
  CalendarResponseStatus,
  CalendarSelectionOrigin,
  CalendarSnapshot,
  CalendarSourceDto,
  CalendarSourceGroup,
  CalendarSourceKind,
  CalendarSourceState,
  CalendarSourcesReport,
  CalendarStatus,
  MeetingInviteDto,
} from "./types";

const STATUSES: readonly CalendarStatus[] = ["waiting", "connecting", "connected", "newOutlookOnly", "elevationMismatch", "unresponsive", "failed"];
const BUSY: readonly CalendarBusyStatus[] = ["free", "tentative", "busy", "oof", "workingElsewhere"];
const RESPONSE: readonly CalendarResponseStatus[] = ["none", "organized", "tentative", "accepted", "declined", "notResponded"];
const KINDS: readonly CalendarSourceKind[] = ["primary", "personal", "shared", "other"];
const GROUPS: readonly CalendarSourceGroup[] = ["my", "shared", "other", "rooms", "custom", "unknown"];
const STATES: readonly CalendarSourceState[] = ["ok", "notSelected", "unavailable", "pending"];
const ORIGINS: readonly CalendarSelectionOrigin[] = ["outlook", "remembered", "primaryOnly"];
/** Far above any real Outlook pane (the backend caps it at 40). */
const MAX_SOURCES = 60;

/** Reported when the backend sends a status this build does not know ("reading items failed"). */
const UNKNOWN_STATUS_CODE = "OUTLOOK-108";

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function oneOf<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

function nonEmptyString(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

function finiteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Only a plain "#RRGGBB" ever reaches a style attribute. */
function hexColor(value: unknown): string | null {
  return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value : null;
}

function instant(value: unknown): string | null {
  const text = nonEmptyString(value);
  return text !== null && !Number.isNaN(Date.parse(text)) ? text : null;
}

/** At most this many invites are kept from one snapshot (the backend sends up to 10). */
const MAX_INVITES = 10;

/** Null for anything that cannot be scheduled or displayed (no id, unparseable instants, end before start). */
export function normalizeEvent(raw: unknown): CalendarEventDto | null {
  if (!isRecord(raw)) return null;
  const id = nonEmptyString(raw.id);
  const startUtc = nonEmptyString(raw.startUtc);
  const endUtc = nonEmptyString(raw.endUtc);
  if (!id || !startUtc || !endUtc) return null;
  const start = Date.parse(startUtc);
  const end = Date.parse(endUtc);
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return null;
  return {
    id,
    calendarId: nonEmptyString(raw.calendarId) ?? "default",
    calendarName: typeof raw.calendarName === "string" ? raw.calendarName.trim() : "",
    sourceKind: oneOf(raw.sourceKind, KINDS, "primary"),
    subject: typeof raw.subject === "string" ? raw.subject : "",
    startUtc,
    endUtc,
    allDay: raw.allDay === true,
    location: nonEmptyString(raw.location),
    organizer: nonEmptyString(raw.organizer),
    isRecurring: raw.isRecurring === true,
    meetingUrl: nonEmptyString(raw.meetingUrl),
    // Unknown availability counts as busy: better one extra reminder than a missed meeting.
    busyStatus: oneOf(raw.busyStatus, BUSY, "busy"),
    responseStatus: oneOf(raw.responseStatus, RESPONSE, "none"),
    color: hexColor(raw.color),
    calendarColor: hexColor(raw.calendarColor) ?? undefined,
  };
}

/** Null without an id. Unknown enums fall back to the safest reading (a shared calendar that is not active). */
export function normalizeSource(raw: unknown): CalendarSourceDto | null {
  if (!isRecord(raw)) return null;
  const id = nonEmptyString(raw.id);
  if (!id) return null;
  const kind = oneOf(raw.kind, KINDS, "shared");
  return {
    id,
    name: typeof raw.name === "string" ? raw.name.trim() : "",
    group: oneOf(raw.group, GROUPS, "unknown"),
    kind,
    selected: raw.selected === true,
    active: raw.active === true,
    pendingInOutlook: raw.pendingInOutlook === true,
    color: hexColor(raw.color) ?? undefined,
    state: oneOf(raw.state, STATES, "unavailable"),
    errorCode: nonEmptyString(raw.errorCode),
    eventCount: Math.max(0, Math.round(finiteNumber(raw.eventCount) ?? 0)),
    lastReadUnixMs: finiteNumber(raw.lastReadUnixMs),
  };
}

export function normalizeSources(raw: unknown): CalendarSourcesReport | null {
  if (!isRecord(raw)) return null;
  const sources = (Array.isArray(raw.sources) ? raw.sources : [])
    .map(normalizeSource)
    .filter((s): s is CalendarSourceDto => s !== null)
    .slice(0, MAX_SOURCES);
  return {
    sources,
    selection: oneOf(raw.selection, ORIGINS, "primaryOnly"),
    groups: Math.max(0, Math.round(finiteNumber(raw.groups) ?? 0)),
    listener: raw.listener === true,
    discoveredUnixMs: finiteNumber(raw.discoveredUnixMs),
  };
}

/** Null without an id or a receive time. A missing or broken meeting time only drops the time. */
export function normalizeInvite(raw: unknown): MeetingInviteDto | null {
  if (!isRecord(raw)) return null;
  const id = nonEmptyString(raw.id);
  const receivedUtc = instant(raw.receivedUtc);
  if (!id || !receivedUtc) return null;
  const startUtc = instant(raw.startUtc);
  const endUtc = instant(raw.endUtc);
  const timed = startUtc !== null && endUtc !== null && Date.parse(endUtc) >= Date.parse(startUtc);
  return {
    id,
    subject: typeof raw.subject === "string" ? raw.subject : "",
    organizer: nonEmptyString(raw.organizer),
    startUtc: timed ? startUtc : null,
    endUtc: timed ? endUtc : null,
    location: nonEmptyString(raw.location),
    receivedUtc,
  };
}

export function compareEvents(a: CalendarEventDto, b: CalendarEventDto): number {
  return Date.parse(a.startUtc) - Date.parse(b.startUtc) || Date.parse(a.endUtc) - Date.parse(b.endUtc) || a.id.localeCompare(b.id);
}

/** Null when the payload is not an object at all (the caller keeps its previous snapshot). */
export function normalizeSnapshot(raw: unknown): CalendarSnapshot | null {
  if (!isRecord(raw)) return null;
  const known = STATUSES.includes(raw.status as CalendarStatus);
  const events = (Array.isArray(raw.events) ? raw.events : [])
    .map(normalizeEvent)
    .filter((event): event is CalendarEventDto => event !== null)
    .sort(compareEvents);
  const status = known ? (raw.status as CalendarStatus) : "failed";
  return {
    status,
    errorCode: nonEmptyString(raw.errorCode) ?? (known ? null : UNKNOWN_STATUS_CODE),
    lastSyncUnixMs: finiteNumber(raw.lastSyncUnixMs),
    cachedCount: Math.max(0, Math.round(finiteNumber(raw.cachedCount) ?? events.length)),
    nextRetryUnixMs: finiteNumber(raw.nextRetryUnixMs),
    events,
    invites: (Array.isArray(raw.invites) ? raw.invites : [])
      .map(normalizeInvite)
      .filter((invite): invite is MeetingInviteDto => invite !== null)
      .slice(0, MAX_INVITES),
    sources: normalizeSources(raw.sources),
  };
}
