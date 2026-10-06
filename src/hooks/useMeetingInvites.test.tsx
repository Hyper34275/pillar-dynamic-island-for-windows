// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../lib/ipc";
import type { CalendarService } from "../lib/calendar/service";
import { WAITING_SNAPSHOT, type CalendarSnapshot, type MeetingInviteDto } from "../lib/calendar/types";
import { CalendarServiceContext } from "./useCalendar";
import { INVITE_STARTUP_GRACE_MS, inviteBody, useMeetingInvites } from "./useMeetingInvites";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const START = new Date(2026, 9, 6, 9, 0).getTime();

function fakeService() {
  let snapshot: CalendarSnapshot = WAITING_SNAPSHOT;
  const listeners = new Set<() => void>();
  const value: CalendarService = {
    subscribe: (l) => (listeners.add(l), () => listeners.delete(l)),
    getSnapshot: () => snapshot,
    refresh: async () => {},
    dispose: () => {},
  };
  return {
    value,
    emit: (invites: MeetingInviteDto[]) => {
      snapshot = { ...WAITING_SNAPSHOT, status: "connected", invites };
      listeners.forEach((l) => l());
    },
  };
}

function invite(id: string, receivedMsAfterStart: number, extra: Partial<MeetingInviteDto> = {}): MeetingInviteDto {
  return {
    id,
    subject: `Invite ${id}`,
    organizer: "Dana",
    startUtc: new Date(2026, 9, 7, 9, 30).toISOString(),
    endUtc: new Date(2026, 9, 7, 10, 0).toISOString(),
    location: null,
    receivedUtc: new Date(START + receivedMsAfterStart).toISOString(),
    ...extra,
  };
}

function Harness({ enabled, onReceived }: { enabled: boolean | null; onReceived: (n: IslandNotification) => void }) {
  useMeetingInvites(enabled, onReceived);
  return null;
}

let container: HTMLDivElement;
let root: Root;
let service: ReturnType<typeof fakeService>;
let received: IslandNotification[];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(START);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  service = fakeService();
  received = [];
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

function render(enabled: boolean | null) {
  act(() => {
    root.render(
      <CalendarServiceContext.Provider value={service.value}>
        <Harness enabled={enabled} onReceived={(n) => received.push(n)} />
      </CalendarServiceContext.Provider>
    );
  });
}

describe("useMeetingInvites", () => {
  it("pops up a new invite once, as an Outlook invitation that opens the calendar on its day", () => {
    render(true);
    act(() => service.emit([invite("a", 60_000)]));
    act(() => service.emit([invite("a", 60_000)])); // the next sync still lists it
    expect(received).toHaveLength(1);
    expect(received[0]).toMatchObject({ title: "Invite a", appName: "Outlook", aumid: null, invite: { startUtc: invite("a", 0).startUtc } });
    expect(received[0].id).toBeLessThan(0); // never collides with a Windows notification id
    expect(received[0].body).toContain("Dana");
  });

  it("ignores the unread invites that were already waiting before the app started", () => {
    render(true);
    act(() => service.emit([invite("old", -INVITE_STARTUP_GRACE_MS - 1), invite("recent", -60_000)]));
    expect(received.map((n) => n.title)).toEqual(["Invite recent"]);
  });

  it("shows several new invites oldest first, so the newest stays on screen", () => {
    render(true);
    act(() => service.emit([invite("newer", 120_000), invite("older", 60_000)]));
    expect(received.map((n) => n.title)).toEqual(["Invite older", "Invite newer"]);
  });

  it("waits for the settings, and does not replay invites that came in while switched off", () => {
    render(null);
    act(() => service.emit([invite("a", 60_000)]));
    expect(received).toHaveLength(0);
    render(false);
    expect(received).toHaveLength(0);
    render(true);
    expect(received).toHaveLength(0);
    act(() => service.emit([invite("a", 60_000), invite("b", 120_000)]));
    expect(received.map((n) => n.title)).toEqual(["Invite b"]);
  });

  it("falls back to a placeholder subject", () => {
    render(true);
    act(() => service.emit([invite("blank", 60_000, { subject: "  " })]));
    expect(received[0].title).toBe("(No subject)");
  });
});

describe("inviteBody", () => {
  it("says when and from whom, whatever Outlook could tell", () => {
    const body = inviteBody(invite("a", 0), START);
    expect(body).toMatch(/^Tomorrow, \d{1,2}:\d{2}/);
    expect(body).toContain(" · From Dana");
    expect(inviteBody(invite("a", 0, { startUtc: null, endUtc: null, organizer: null, location: "Room 3" }), START)).toBe("Room 3");
  });
});
