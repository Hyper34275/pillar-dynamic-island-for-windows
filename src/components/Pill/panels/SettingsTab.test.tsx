// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CalendarServiceContext } from "../../../hooks/useCalendar";
import type { CalendarService } from "../../../lib/calendar/service";
import type { CalendarSnapshot } from "../../../lib/calendar/types";
import { formatDateTime } from "../../../lib/dateFormat";
import { SETTINGS_DEFAULTS, type NotificationStatus } from "../../../lib/ipc";
import { SettingsTab, SettingsView, type SettingsViewProps } from "./SettingsTab";

const openCenter = vi.hoisted(() => vi.fn(async (_page: string) => true));

// The backend's view of the world: polling delivery and two recent codes. (useSystemInfo caches it.)
vi.mock("../../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../lib/ipc")>();
  return {
    ...original,
    ipc: {
      ...original.ipc,
      openCenter,
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
    [...container.querySelectorAll("section[data-section=diagnostics] .min-h-hit")]
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

const buttonNamed = (name: string) => [...container.querySelectorAll("button")].find((b) => b.textContent === name)!;

describe("SettingsTab Yuval Center section", () => {
  it("opens the Center on its settings page and the tour from two buttons", async () => {
    openCenter.mockClear();
    await mount(service(snap({})));
    expect(container.querySelector("section[data-section=center]")).not.toBeNull();
    await act(async () => buttonNamed("Open Yuval Center").click());
    expect(openCenter).toHaveBeenLastCalledWith("settings");
    await act(async () => buttonNamed("System tour").click());
    expect(openCenter).toHaveBeenLastCalledWith("tour");
    expect(openCenter).toHaveBeenCalledTimes(2);
  });

  it("keeps every existing setting", async () => {
    await mount(service(snap({})));
    const switches = [...container.querySelectorAll('button[role="switch"]')].map((b) => b.getAttribute("aria-label"));
    expect(switches).toEqual([
      "Launch with Windows",
      "Hide in fullscreen apps",
      "Meeting reminders",
      "Meeting invitations",
      "Reminders from shared calendars",
      "Offer silence when a meeting starts",
      "Show notifications",
    ]);
    expect(container.textContent).toContain("Remind me before");
  });
});

describe("SettingsView", () => {
  function viewProps(over: Partial<SettingsViewProps> = {}): SettingsViewProps {
    return {
      settings: SETTINGS_DEFAULTS,
      monitors: [],
      info: null,
      diagnostics: null,
      snapshot: snap({}),
      notificationStatus: "allowed",
      flash: null,
      onChange: vi.fn(),
      onRequestNotificationAccess: vi.fn(),
      onCopyDiagnostics: vi.fn(),
      onOpenLogs: vi.fn(),
      onOpenCenter: vi.fn(),
      onOpenTour: vi.fn(),
      ...over,
    };
  }
  const render = (props: SettingsViewProps) => act(() => root.render(<SettingsView {...props} />));

  it("renders from props alone, with no providers and no backend", () => {
    render(viewProps());
    expect(container.querySelector("section[data-section=settings]")).not.toBeNull();
    expect(rows()["Outlook"]).toBe("Running");
    expect(rows()["Notifications"]).toBe("Allowed");
  });

  it("reports changes as patches and button presses as callbacks", () => {
    const props = viewProps();
    render(props);
    act(() => container.querySelector<HTMLElement>('button[role="switch"][aria-label="Launch with Windows"]')!.click());
    expect(props.onChange).toHaveBeenCalledWith({ launchWithWindows: !SETTINGS_DEFAULTS.launchWithWindows });
    act(() => buttonNamed("Open Yuval Center").click());
    act(() => buttonNamed("System tour").click());
    act(() => buttonNamed("Copy diagnostics").click());
    act(() => buttonNamed("Open logs").click());
    expect(props.onOpenCenter).toHaveBeenCalledTimes(1);
    expect(props.onOpenTour).toHaveBeenCalledTimes(1);
    expect(props.onCopyDiagnostics).toHaveBeenCalledTimes(1);
    expect(props.onOpenLogs).toHaveBeenCalledTimes(1);
  });

  it("shows the flash on the copy button and the save error in the header", () => {
    render(viewProps({ flash: "copied" }));
    expect(buttonNamed("Copied")).toBeDefined();
    render(viewProps({ flash: "failed" }));
    expect(buttonNamed("Copy failed")).toBeDefined();
    render(viewProps({ flash: "saveFailed" }));
    expect(container.querySelector('[role="alert"]')!.textContent).toBe("Couldn't save settings");
  });

  it("reports notifications as off when the user turned them off", () => {
    render(viewProps({ settings: { ...SETTINGS_DEFAULTS, notificationsEnabled: false } }));
    expect(rows()["Notifications"]).toBe("Off");
  });

  it("offers the collapsed island's display in three options, marks the current one and writes the choice", () => {
    const props = viewProps();
    render(props);
    const group = container.querySelector<HTMLElement>('[role="group"][aria-label="Collapsed island"]')!;
    const options = [...group.querySelectorAll("button")];
    expect(options.map((o) => o.textContent)).toEqual(["Time, date and day", "Time only", "Date and day"]);
    expect(options.map((o) => o.getAttribute("aria-pressed"))).toEqual(["true", "false", "false"]); // the default is "full"
    act(() => options[1].click());
    expect(props.onChange).toHaveBeenCalledWith({ islandDisplay: "clock" });
    act(() => options[2].click());
    expect(props.onChange).toHaveBeenCalledWith({ islandDisplay: "date" });

    render(viewProps({ settings: { ...SETTINGS_DEFAULTS, islandDisplay: "date" } }));
    const pressed = [...container.querySelectorAll('[aria-label="Collapsed island"] button')].map((o) => o.getAttribute("aria-pressed"));
    expect(pressed).toEqual(["false", "false", "true"]);
  });

  it("offers the schedule download in five options, marks 7 days and writes the choice", () => {
    const props = viewProps();
    render(props);
    const group = container.querySelector<HTMLElement>('[role="group"][aria-label="Download schedule ahead"]')!;
    const options = [...group.querySelectorAll("button")];
    expect(options.map((o) => o.textContent)).toEqual(["Off", "3 days", "7 days", "14 days", "30 days"]);
    expect(options.map((o) => o.getAttribute("aria-pressed"))).toEqual(["false", "false", "true", "false", "false"]);
    expect(container.textContent).toContain("Kept in memory only.");
    act(() => options[0].click());
    expect(props.onChange).toHaveBeenCalledWith({ calendarPrefetchDays: 0 });
    act(() => options[4].click());
    expect(props.onChange).toHaveBeenCalledWith({ calendarPrefetchDays: 30 });
  });

  it("shows an unlisted stored prefetch value as its own number", () => {
    render(viewProps({ settings: { ...SETTINGS_DEFAULTS, calendarPrefetchDays: 10 } }));
    const group = container.querySelector<HTMLElement>('[role="group"][aria-label="Download schedule ahead"]')!;
    expect([...group.querySelectorAll("button")].map((o) => o.textContent)).toEqual(["Off", "3 days", "7 days", "10 days", "14 days", "30 days"]);
  });

  it("offers the display choice only with more than one display", () => {
    render(viewProps());
    expect(container.textContent).not.toContain("Display");
    render(
      viewProps({
        monitors: [
          { id: "1", name: "Main", isPrimary: true },
          { id: "2", name: "Side", isPrimary: false },
        ],
      })
    );
    expect(container.textContent).toContain("Display");
    expect(container.textContent).toContain("Side");
  });
});
