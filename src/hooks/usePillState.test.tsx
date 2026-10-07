// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../lib/ipc";
import {
  ALERT_MS,
  FOREGROUND_GRACE_MS,
  HOVER_INTENT_MS,
  LEAVE_COLLAPSE_MS,
  NOTIFICATION_GRACE_MS,
  NOTIFICATION_MS,
  UNATTENDED_COLLAPSE_MS,
} from "../lib/island/timing";
import type { ReminderAlert } from "../lib/reminders/types";
import { useIslandState, type IslandController } from "./useIslandState";
import { usePillState } from "./usePillState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// The meeting is two hours away on the (fake) clock, so waiting out an alert never makes it stale.
const alert = (key = "a", startsInMs = 2 * 3_600_000): ReminderAlert => ({
  key,
  eventId: key,
  subject: "s",
  startUtc: new Date(Date.now() + startsInMs).toISOString(),
  endUtc: new Date(Date.now() + startsInMs + 1_800_000).toISOString(),
  location: null,
  minutesRemaining: 30,
  reminderType: { kind: "beforeStart", minutes: 30 },
});
const toast = (id = 1): IslandNotification => ({ id, appName: "Teams", title: "t", body: "b", timestamp: 0, aumid: null });

let root: Root;
let container: HTMLDivElement;
let api: { island: IslandController; pill: ReturnType<typeof usePillState> };

function Harness({ suppressed = false }: { suppressed?: boolean }) {
  const island = useIslandState({ suppressed });
  const pill = usePillState({
    expanded: island.state.expanded,
    temporary: island.view.kind === "meetingAlert" || island.view.kind === "notification",
    expand: island.expand,
    pin: island.pin,
    collapse: island.collapse,
    setHovering: island.setHovering,
  });
  api = { island, pill };
  return <div id="out" data-view={island.view.kind} data-tab={island.state.tab} />;
}

const view = () => container.querySelector("#out")!.getAttribute("data-view");
const tab = () => container.querySelector("#out")!.getAttribute("data-tab");
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));
const call = (fn: () => void) => act(async () => fn());

async function mountReady(props: { suppressed?: boolean } = {}) {
  await act(async () => root.render(<Harness {...props} />));
  await call(() => api.pill.completeBootAnimation());
}

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

describe("hover and leave", () => {
  it("expands after the hover intent delay, not before", async () => {
    await mountReady();
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS - 1);
    expect(view()).toBe("idle");
    await ms(1);
    expect(view()).toBe("userExpanded");
  });

  it("ignores a fly-by that leaves before the intent delay", async () => {
    await mountReady();
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS - 20);
    await call(() => api.pill.pointerLeave());
    await ms(1000);
    expect(view()).toBe("idle");
  });

  it("does not expand while booting, but does once the boot animation completes under the pointer", async () => {
    await act(async () => root.render(<Harness />));
    await call(() => api.pill.pointerEnter());
    await ms(1000);
    expect(view()).toBe("idle");
    await call(() => api.pill.completeBootAnimation());
    await ms(HOVER_INTENT_MS);
    expect(view()).toBe("userExpanded");
  });

  it("reopens on the last-used tab", async () => {
    await mountReady();
    await call(() => api.island.expand("about"));
    await call(() => api.island.collapse());
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS);
    expect(view()).toBe("userExpanded");
    expect(tab()).toBe("about");
  });

  it("collapses after the leave grace, and re-entering within it cancels the collapse", async () => {
    await mountReady();
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS);
    await call(() => api.pill.pointerLeave());
    await ms(LEAVE_COLLAPSE_MS - 1);
    expect(view()).toBe("userExpanded");
    await call(() => api.pill.pointerEnter());
    await ms(LEAVE_COLLAPSE_MS * 3);
    expect(view()).toBe("userExpanded");

    await call(() => api.pill.pointerLeave());
    await ms(LEAVE_COLLAPSE_MS);
    expect(view()).toBe("idle");
  });

  it("collapses a clicked (pinned) island just as quickly once the pointer leaves it", async () => {
    await mountReady();
    await call(() => api.island.pin());
    await call(() => api.pill.pointerEnter());
    await call(() => api.pill.pointerLeave());
    await ms(LEAVE_COLLAPSE_MS - 1);
    expect(view()).toBe("userExpanded");
    await ms(1);
    expect(view()).toBe("idle");
  });

  it("never leaves an island that was opened while the pointer was elsewhere open forever", async () => {
    await mountReady();
    await call(() => api.island.pin("calendar")); // tray click or second launch
    await ms(UNATTENDED_COLLAPSE_MS - 1);
    expect(view()).toBe("userExpanded");
    await ms(1);
    expect(view()).toBe("idle");
  });

  it("gives a remotely opened island the short grace once the pointer has reached it and left", async () => {
    await mountReady();
    await call(() => api.island.pin()); // tray click
    await ms(1000);
    await call(() => api.pill.pointerEnter());
    await ms(5000);
    expect(view()).toBe("userExpanded");
    await call(() => api.pill.pointerLeave());
    await ms(LEAVE_COLLAPSE_MS);
    expect(view()).toBe("idle");
  });

  it("does not re-expand under a pointer that is still on the island after holdCollapsed", async () => {
    await mountReady();
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS);
    await call(() => {
      api.pill.holdCollapsed();
      api.island.collapse();
    });
    await ms(2000);
    expect(view()).toBe("idle");

    await call(() => api.pill.pointerLeave());
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS);
    expect(view()).toBe("userExpanded");
  });

  it("does not block the next hover when the island was closed while the pointer was elsewhere", async () => {
    await mountReady();
    await call(() => api.island.pin()); // tray click
    await call(() => {
      api.pill.holdCollapsed(); // second tray click closes it; no pointer leave will follow
      api.island.collapse();
    });
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS);
    expect(view()).toBe("userExpanded");
  });
});

describe("click outside (another window became active)", () => {
  it("collapses at once when the pointer is away from the island", async () => {
    await mountReady();
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS); // expands (separate step so the expansion renders at this time)
    await ms(FOREGROUND_GRACE_MS);
    await call(() => api.pill.pointerLeave());
    await call(() => api.pill.foregroundChanged()); // clicked the desktop
    expect(view()).toBe("idle");
  });

  it("closes a remotely opened island without waiting for the unattended grace", async () => {
    await mountReady();
    await call(() => api.island.pin()); // tray click
    await ms(FOREGROUND_GRACE_MS);
    await call(() => api.pill.foregroundChanged());
    expect(view()).toBe("idle");
  });

  it("ignores the foreground shuffle right after opening (tray menu, second launch)", async () => {
    await mountReady();
    await call(() => api.island.pin());
    await ms(FOREGROUND_GRACE_MS - 1);
    await call(() => api.pill.foregroundChanged());
    expect(view()).toBe("userExpanded");
  });

  it("stays open while the pointer is on the island", async () => {
    await mountReady();
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS);
    await ms(FOREGROUND_GRACE_MS);
    await call(() => api.pill.foregroundChanged()); // e.g. an app popped up a window on its own
    expect(view()).toBe("userExpanded");
  });

  it("does not cut a meeting alert short", async () => {
    await mountReady();
    await call(() => api.island.showAlert(alert()));
    await ms(FOREGROUND_GRACE_MS);
    await call(() => api.pill.foregroundChanged());
    expect(view()).toBe("meetingAlert");
  });
});

describe("temporary states", () => {
  it("auto-dismisses a meeting alert after its duration", async () => {
    await mountReady();
    await call(() => api.island.showAlert(alert()));
    expect(view()).toBe("meetingAlert");
    await ms(ALERT_MS - 1);
    expect(view()).toBe("meetingAlert");
    await ms(1);
    expect(view()).toBe("idle");
  });

  it("auto-dismisses a notification toast", async () => {
    await mountReady();
    await call(() => api.island.showNotification(toast()));
    expect(view()).toBe("notification");
    // Its dwell, then the session's grace (a second timer, armed when the dwell ends).
    await ms(NOTIFICATION_MS - NOTIFICATION_GRACE_MS);
    expect(view()).toBe("notification");
    await ms(NOTIFICATION_GRACE_MS);
    expect(view()).toBe("idle");
  });

  it("pauses the alert while the pointer is on it and resumes with the remaining time", async () => {
    await mountReady();
    await call(() => api.island.showAlert(alert()));
    await ms(3000);
    await call(() => api.pill.pointerEnter());
    await ms(60_000);
    expect(view()).toBe("meetingAlert"); // held while hovered, and hovering does not expand over it
    await call(() => api.pill.pointerLeave());
    await ms(ALERT_MS - 3000 - 1);
    expect(view()).toBe("meetingAlert");
    await ms(1);
    expect(view()).toBe("idle");
  });

  it("waits while suppressed (hidden behind a fullscreen app) and then runs its full time", async () => {
    await mountReady({ suppressed: true });
    await call(() => api.island.showAlert(alert()));
    await ms(60_000);
    expect(view()).toBe("meetingAlert");
    await act(async () => root.render(<Harness suppressed={false} />));
    await ms(ALERT_MS - 1);
    expect(view()).toBe("meetingAlert");
    await ms(1);
    expect(view()).toBe("idle");
  });

  it("drops an alert whose meeting started while the window was hidden", async () => {
    await mountReady({ suppressed: true });
    await call(() => api.island.showAlert(alert("a", 30 * 60_000)));
    await ms(31 * 60_000);
    expect(view()).toBe("meetingAlert");
    await act(async () => root.render(<Harness suppressed={false} />));
    expect(view()).toBe("idle");
  });

  it("corrects the minutes of an alert that waited while hidden", async () => {
    await mountReady({ suppressed: true });
    await call(() => api.island.showAlert(alert("a", 30 * 60_000)));
    await ms(20 * 60_000);
    await act(async () => root.render(<Harness suppressed={false} />));
    expect(api.island.state.alert?.minutesRemaining).toBe(10);
  });

  it("shows queued alerts one after the other, each for its full time", async () => {
    await mountReady();
    await call(() => {
      api.island.showAlert(alert("a"));
      api.island.showAlert(alert("b"));
    });
    await ms(ALERT_MS);
    expect(view()).toBe("meetingAlert");
    await ms(ALERT_MS);
    expect(view()).toBe("idle");
  });

  it("restores the expanded tab after an alert and then closes it if the pointer is gone", async () => {
    await mountReady();
    await call(() => api.island.pin("calendar"));
    await call(() => api.pill.pointerEnter());
    await call(() => api.island.showAlert(alert()));
    expect(view()).toBe("meetingAlert");
    await call(() => api.pill.pointerLeave()); // pointer leaves while the alert shows: nothing collapses yet
    await ms(ALERT_MS - 1);
    expect(view()).toBe("meetingAlert");
    await ms(1);
    expect(view()).toBe("userExpanded");
    expect(tab()).toBe("calendar");
    await ms(LEAVE_COLLAPSE_MS);
    expect(view()).toBe("idle");
  });

  it("does not expand an idle island over a showing alert on hover", async () => {
    await mountReady();
    await call(() => api.island.showAlert(alert()));
    await call(() => api.pill.pointerEnter());
    await ms(HOVER_INTENT_MS * 5);
    expect(view()).toBe("meetingAlert");
  });
});
