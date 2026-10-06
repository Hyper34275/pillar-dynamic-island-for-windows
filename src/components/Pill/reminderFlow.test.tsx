// @vitest-environment jsdom
// End to end through the real hooks and components: a calendar provider emits a meeting,
// the reminder engine fires, the island shows the alert, folds back, and never repeats.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CalendarServiceContext } from "../../hooks/useCalendar";
import type { CalendarService } from "../../lib/calendar/service";
import { WAITING_SNAPSHOT, type CalendarEventDto, type CalendarSnapshot } from "../../lib/calendar/types";
import { ALERT_MS } from "../../lib/island/timing";
import type { ReminderStore } from "../../lib/reminders/types";
import { PillShell } from "./PillShell";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MIN = 60_000;
const NOW = new Date(2026, 9, 6, 10, 0).getTime();

function fakeService(): CalendarService & { emit: (snapshot: CalendarSnapshot) => void } {
  let snapshot = WAITING_SNAPSHOT;
  const listeners = new Set<() => void>();
  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => snapshot,
    refresh: async () => {},
    dispose: () => listeners.clear(),
    emit(next) {
      snapshot = next;
      listeners.forEach((l) => l());
    },
  };
}

function memoryStore(): ReminderStore & { data: Record<string, number> } {
  const store = {
    data: {} as Record<string, number>,
    async load() {
      return { ...store.data };
    },
    async save(fired: Record<string, number>) {
      store.data = { ...fired };
    },
  };
  return store;
}

const meeting = (startInMin: number, extra: Partial<CalendarEventDto> = {}): CalendarEventDto => ({
  id: "m1",
  calendarId: "cal",
  subject: "Design review",
  startUtc: new Date(NOW + startInMin * MIN).toISOString(),
  endUtc: new Date(NOW + (startInMin + 45) * MIN).toISOString(),
  allDay: false,
  location: "Room 7",
  organizer: null,
  isRecurring: false,
  meetingUrl: null,
  busyStatus: "busy",
  responseStatus: "accepted",
  ...extra,
});

const connected = (events: CalendarEventDto[]): CalendarSnapshot => ({
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: NOW,
  cachedCount: events.length,
  nextRetryUnixMs: null,
  events,
});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-view]")!;
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));

async function mount(service: ReturnType<typeof fakeService>, store: ReminderStore) {
  await act(async () => {
    root.render(
      <CalendarServiceContext.Provider value={service}>
        <PillShell reminderStore={store} />
      </CalendarServiceContext.Provider>
    );
  });
  await ms(1500); // boot, and the store's initial load
}

describe("meeting reminder flow", () => {
  it("shows the alert for a meeting 30 minutes ahead, folds back after 8 seconds and never repeats", async () => {
    const service = fakeService();
    const store = memoryStore();
    await mount(service, store);
    expect(island().dataset.view).toBe("idle");

    await act(async () => service.emit(connected([meeting(30)])));
    await ms(150); // past the live region's short announcement delay
    expect(island().dataset.view).toBe("meetingAlert");
    expect(island().textContent).toContain("Meeting in 30 minutes");
    expect(island().textContent).toContain("Design review");
    expect(island().textContent).toContain("Room 7");
    // announced to assistive tech, politely, without moving focus
    expect(container.querySelector('[aria-live="polite"]')!.textContent).toContain("Meeting in 30 minutes");
    expect(document.activeElement === document.body || !island().contains(document.activeElement)).toBe(true);

    await ms(ALERT_MS);
    expect(island().dataset.view).toBe("idle");
    expect(island().textContent).toContain("10/6"); // the date pill is back

    await act(async () => service.emit(connected([meeting(30)]))); // same meeting re-emitted
    await act(async () => service.emit(connected([meeting(30, { location: "Room 8" })]))); // details changed
    await ms(60_000);
    expect(island().dataset.view).toBe("idle");
    expect(Object.keys(store.data)).toHaveLength(1);
  });

  it("does not repeat after the app restarts with the persisted fired set", async () => {
    const service = fakeService();
    const store = memoryStore();
    await mount(service, store);
    await act(async () => service.emit(connected([meeting(30)])));
    await ms(50);
    expect(island().dataset.view).toBe("meetingAlert");
    act(() => root.unmount());

    root = createRoot(container);
    const restarted = fakeService();
    await mount(restarted, store);
    await act(async () => restarted.emit(connected([meeting(30)])));
    await ms(ALERT_MS * 2);
    expect(island().dataset.view).toBe("idle");
  });

  it("stays quiet for a meeting that is further away and alerts when it gets 30 minutes close", async () => {
    const service = fakeService();
    await mount(service, memoryStore());
    await act(async () => service.emit(connected([meeting(35)])));
    await ms(4 * MIN);
    expect(island().dataset.view).toBe("idle");
    await ms(1 * MIN + 1000);
    expect(island().dataset.view).toBe("meetingAlert");
    expect(island().textContent).toContain("Meeting in 30 minutes");
  });

  it("does not alert for a declined meeting", async () => {
    const service = fakeService();
    await mount(service, memoryStore());
    await act(async () => service.emit(connected([meeting(30, { responseStatus: "declined" })])));
    await ms(MIN);
    expect(island().dataset.view).toBe("idle");
  });
});
