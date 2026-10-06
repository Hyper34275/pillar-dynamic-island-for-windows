// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { CalendarServiceContext } from "../../../hooks/useCalendar";
import type { CalendarService } from "../../../lib/calendar/service";
import type { CalendarSnapshot } from "../../../lib/calendar/types";
import { formatDateTime } from "../../../lib/dateFormat";
import { AboutTab } from "./AboutTab";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SYNC = new Date(2026, 9, 6, 9, 41).getTime();

function service(initial: CalendarSnapshot) {
  let snapshot = initial;
  const listeners = new Set<() => void>();
  const value: CalendarService = {
    subscribe: (l) => (listeners.add(l), () => listeners.delete(l)),
    getSnapshot: () => snapshot,
    refresh: async () => {},
    dispose: () => {},
  };
  return { value, emit: (next: CalendarSnapshot) => ((snapshot = next), listeners.forEach((l) => l())) };
}

const snap = (extra: Partial<CalendarSnapshot>): CalendarSnapshot => ({
  status: "connected",
  errorCode: null,
  lastSyncUnixMs: SYNC,
  cachedCount: 7,
  nextRetryUnixMs: null,
  events: [],
  ...extra,
});

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const rows = () =>
  Object.fromEntries(
    [...container.querySelectorAll("section:first-of-type [dir=ltr].flex")]
      .map((row) => [...row.children].map((c) => c.textContent ?? ""))
      .filter((cells) => cells.length === 2)
      .map(([label, value]) => [label, value])
  );

async function mount(svc: ReturnType<typeof service>) {
  await act(async () => {
    root.render(
      <CalendarServiceContext.Provider value={svc.value}>
        <AboutTab notificationStatus="allowed" onRequestNotificationAccess={() => {}} />
      </CalendarServiceContext.Provider>
    );
  });
}

describe("AboutTab diagnostics follow the live calendar", () => {
  it("shows Outlook status, mode, connection, cached events and the localized last sync", async () => {
    await mount(service(snap({})));
    const r = rows();
    expect(r["Outlook"]).toBe("Running");
    expect(r["Outlook mode"]).toBe("Classic Outlook");
    expect(r["Calendar"]).toBe("Connected");
    expect(r["Cached events"]).toBe("7");
    expect(r["Last calendar sync"]).toBe(formatDateTime(new Date(SYNC)));
  });

  it("shows the waiting state without a code and updates when the snapshot changes", async () => {
    const svc = service(snap({ status: "waiting", lastSyncUnixMs: null, cachedCount: 0 }));
    await mount(svc);
    expect(rows()["Outlook"]).toBe("Not running");
    expect(rows()["Outlook mode"]).toBe("None");
    expect(rows()["Calendar"]).toBe("Waiting for Outlook");
    expect(rows()["Last calendar sync"]).toBe("—");

    await act(async () => svc.emit(snap({ status: "failed", errorCode: "OUTLOOK-108" })));
    expect(rows()["Calendar"]).toBe("Failed · OUTLOOK-108");
    expect(rows()["Outlook mode"]).toBe("Classic Outlook");
  });

  it("reports New Outlook when only that is running", async () => {
    await mount(service(snap({ status: "newOutlookOnly", errorCode: "OUTLOOK-104" })));
    expect(rows()["Outlook mode"]).toBe("New Outlook");
    expect(rows()["Calendar"]).toBe("New Outlook (unsupported) · OUTLOOK-104");
  });

  it("never prints meeting content", async () => {
    const event = {
      id: "abc",
      calendarId: "c",
      subject: "Secret subject",
      startUtc: new Date(SYNC + 3_600_000).toISOString(),
      endUtc: new Date(SYNC + 7_200_000).toISOString(),
      allDay: false,
      location: "Secret place",
      organizer: "Secret person",
      isRecurring: false,
      meetingUrl: null,
      busyStatus: "busy" as const,
      responseStatus: "accepted" as const,
    };
    await mount(service(snap({ events: [event] })));
    expect(container.textContent).not.toMatch(/Secret/);
  });
});
