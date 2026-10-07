import { startOfDay } from "../dateFormat";
import { compareEvents } from "./normalize";
import { WAITING_SNAPSHOT, type CalendarEventDto, type CalendarSnapshot } from "./types";

/** Timed, busy, not declined: what the Calendar tab lists and what reminders fire for. */
export function isRealMeeting(event: CalendarEventDto): boolean {
  return !event.allDay && event.busyStatus !== "free" && event.responseStatus !== "declined";
}

/**
 * Meetings worth showing: real meetings that are not yet over.
 * Input is assumed sorted by start; output keeps that order.
 */
export function selectUpcoming(events: readonly CalendarEventDto[], nowMs: number, limit: number): CalendarEventDto[] {
  const out: CalendarEventDto[] = [];
  for (const event of events) {
    if (out.length >= limit) break;
    if (!isRealMeeting(event) || Date.parse(event.endUtc) <= nowMs) continue;
    out.push(event);
  }
  return out;
}

/** All-day events that cover today and were not declined. They never become the "next meeting". */
export function selectAllDay(events: readonly CalendarEventDto[], nowMs: number, limit: number): CalendarEventDto[] {
  // +36 h from midnight lands inside tomorrow whatever the DST shift of today.
  const endOfToday = startOfDay(startOfDay(nowMs) + 36 * 3_600_000);
  const out: CalendarEventDto[] = [];
  for (const event of events) {
    if (out.length >= limit) break;
    if (!event.allDay || event.responseStatus === "declined") continue;
    if (Date.parse(event.endUtc) > nowMs && Date.parse(event.startUtc) < endOfToday) out.push(event);
  }
  return out;
}

/** Events of several calendars as one list: de-duplicated by calendar + id + start, sorted by start. */
export function selectEvents(snapshots: readonly CalendarSnapshot[]): CalendarEventDto[] {
  const seen = new Set<string>();
  const merged: CalendarEventDto[] = [];
  for (const snapshot of snapshots) {
    for (const event of snapshot.events) {
      const key = `${event.calendarId}|${event.id}|${event.startUtc}`;
      if (seen.has(key)) continue;
      seen.add(key);
      merged.push(event);
    }
  }
  return merged.sort(compareEvents);
}

/**
 * One snapshot for the UI out of every provider's. A single provider passes through untouched.
 * With several, the best status wins (connected, then the first that has something to say),
 * counts add up and the latest sync is reported.
 */
export function mergeSnapshots(snapshots: readonly CalendarSnapshot[]): CalendarSnapshot {
  if (snapshots.length === 0) return WAITING_SNAPSHOT;
  if (snapshots.length === 1) return snapshots[0];
  const lead = snapshots.find((s) => s.status === "connected") ?? snapshots[0];
  const syncs = snapshots.map((s) => s.lastSyncUnixMs).filter((n): n is number => n !== null);
  const retries = snapshots.map((s) => s.nextRetryUnixMs).filter((n): n is number => n !== null);
  return {
    status: lead.status,
    errorCode: lead.errorCode,
    lastSyncUnixMs: syncs.length > 0 ? Math.max(...syncs) : null,
    cachedCount: snapshots.reduce((sum, s) => sum + s.cachedCount, 0),
    nextRetryUnixMs: retries.length > 0 ? Math.min(...retries) : null,
    events: selectEvents(snapshots),
    invites: snapshots.flatMap((s) => s.invites).filter((invite, index, all) => all.findIndex((other) => other.id === invite.id) === index),
    sources: lead.sources ?? null,
  };
}
