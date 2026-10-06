// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CalendarServiceContext } from "../../../hooks/useCalendar";
import type { CalendarService } from "../../../lib/calendar/service";
import type { CalendarSnapshot } from "../../../lib/calendar/types";
import { formatDateTime } from "../../../lib/dateFormat";
import type { NotificationStatus } from "../../../lib/ipc";
import { SettingsTab } from "./SettingsTab";

// The backend's view of the world: polling delivery and two recent codes. (useSystemInfo caches it.)
vi.mock("../../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../lib/ipc")>();
  return {
    ...original,
    ipc: {
      ...original.ipc,
      getSystemInfo: async () => null,
      getDiagnostics: async () => ({
        outlookRunning: null,
        outlookMode: null,
        notificationMode: "polling" as const,
        recentErrorCodes: ["NOTIF-204", "OUTLOOK-101"],
      }),
    },
  };
});

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
  invites: [],
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
    [...container.querySelectorAll("section[data-section=diagnostics] [dir=ltr].flex")]
      .map((row) => [...row.children].map((c) => c.textContent ?? ""))
      .filter((cells) => cells.length === 2)
      .map(([label, value]) => [label, value])
  );

async function mount(svc: ReturnType<typeof service>, notificationStatus: NotificationStatus = "allowed") {
  await act(async () => {
    root.render(
      <CalendarServiceContext.Provider value={svc.value}>
        <SettingsTab notificationStatus={notificationStatus} onRequestNotificationAccess={() => {}} />
      </CalendarServiceContext.Provider>
    );
  });
}

describe("SettingsTab diagnostics follow the live calendar", () => {
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
    expect(rows()["Calendar"]).toBe("Failed");
    expect(rows()["Internal Error"]).toBe("OUTLOOK-108");
    expect(rows()["Outlook mode"]).toBe("Classic Outlook");
  });

  it("reports New Outlook when only that is running", async () => {
    await mount(service(snap({ status: "newOutlookOnly", errorCode: "OUTLOOK-104" })));
    expect(rows()["Outlook mode"]).toBe("New Outlook");
    expect(rows()["Calendar"]).toBe("New Outlook (unsupported)");
    expect(rows()["Internal Error"]).toBe("OUTLOOK-104");
  });

  it("shows no internal error while connected or waiting", async () => {
    await mount(service(snap({})));
    expect(rows()["Internal Error"]).toBeUndefined();
    act(() => root.unmount());
    root = createRoot(container);
    await mount(service(snap({ status: "waiting", errorCode: "OUTLOOK-101" })));
    expect(rows()["Internal Error"]).toBeUndefined();
  });

  it("shows how notifications are delivered and the recent internal error codes", async () => {
    await mount(service(snap({})));
    expect(rows()["Notification delivery"]).toBe("Polling");
    expect(rows()["Recent error codes"]).toBe("NOTIF-204, OUTLOOK-101");
  });

  it("appends the NOTIF code to a notification status that is not allowed", async () => {
    await mount(service(snap({})), "policy");
    expect(rows()["Notifications"]).toBe("Disabled by policy · NOTIF-202");
  });

  it("shows a plain status when notifications are allowed", async () => {
    await mount(service(snap({})));
    expect(rows()["Notifications"]).toBe("Allowed");
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
      color: null,
    };
    await mount(service(snap({ events: [event] })));
    expect(container.textContent).not.toMatch(/Secret/);
  });
});
