// @vitest-environment jsdom
// The collapsed island's clock and the notification arrival, end to end through the real shell:
// the three displays, the unseen indicator, the toast's timing (hover pause, restart, silence)
// and the swipe. Fake timers; the backend's events are fed by hand.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { silence } from "../../lib/island/silence";
import { NOTIFICATION_MS } from "../../lib/island/timing";
import { SETTINGS_DEFAULTS, type IslandDisplay } from "../../lib/ipc";
import { PillShell } from "./PillShell";

const backend = vi.hoisted(() => ({
  handlers: new Map<string, (payload: unknown) => void>(),
  activate: vi.fn(),
  display: "full" as string,
}));

vi.mock("../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/ipc")>();
  return {
    ...original,
    onEvent: (name: string, handler: (payload: unknown) => void) => {
      backend.handlers.set(name, handler);
      return () => backend.handlers.delete(name);
    },
    ipc: { ...original.ipc, activateNotification: backend.activate },
  };
});

// The display setting is the only thing the tests change, so the settings hook is replaced.
vi.mock("../../hooks/useSettings", async () => {
  const { SETTINGS_DEFAULTS: defaults } = await vi.importActual<typeof import("../../lib/ipc")>("../../lib/ipc");
  return {
    useSettings: () => ({ settings: { ...defaults, islandDisplay: backend.display }, loaded: true, update: async () => true }),
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
let nextId = 1;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 6, 15, 30));
  backend.activate.mockClear();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  silence.clear();
  backend.handlers.clear();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-view]")!;
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));

async function mount(display: IslandDisplay = "full") {
  backend.display = display;
  await act(async () => {
    root.render(<PillShell />);
  });
  await ms(1500); // boot: dot -> morph -> interactive
}

const notify = (title = "New message") =>
  act(async () => {
    backend.handlers.get("notification-received")!({ id: nextId++, appName: "Slack", title, body: "", timestamp: Date.now(), aumid: null });
  });

/** The unseen indicator in the collapsed island: the element filled with the island's blue (the accent tokens). */
const indicator = () =>
  [...container.querySelectorAll<HTMLElement>('[data-layer="compact"] span')].find((s) => /--ci-accent/.test(s.getAttribute("style") ?? ""));

const pointer = (el: Element, type: string, clientX: number) => el.dispatchEvent(new MouseEvent(type, { bubbles: true, clientX, clientY: 10 }));

describe("the collapsed island's clock", () => {
  it("shows the date, the time and the weekday in that order", async () => {
    await mount("full");
    const text = island().textContent!;
    expect(text.indexOf("10/6")).toBeGreaterThan(-1);
    expect(text.indexOf("10/6")).toBeLessThan(text.indexOf("3:30"));
    expect(text.indexOf("3:30")).toBeLessThan(text.indexOf("Tuesday"));
    expect(text).toContain("PM");
    expect(SETTINGS_DEFAULTS.islandDisplay).toBe("full");
  });

  it("shows only the time in the clock display and no time in the date display", async () => {
    await mount("clock");
    expect(island().textContent).toContain("3:30");
    expect(island().textContent).not.toContain("10/6");
    expect(island().textContent).not.toContain("Tuesday");
    act(() => root.unmount());
    root = createRoot(container);
    await mount("date");
    expect(island().textContent).not.toContain("3:30");
    expect(island().textContent).toContain("10/6");
  });

  it("changes the time at the minute without adding a timer or resizing the island", async () => {
    await mount("full");
    await ms(1_000);
    expect(vi.getTimerCount()).toBe(1);
    const width = island().style.width;
    await ms(59_000);
    expect(vi.getTimerCount()).toBe(1);
    expect(island().textContent).toContain("3:31");
    expect(island().style.width).toBe(width);
  });

  it("keeps a single timer in the clock display too", async () => {
    await mount("clock");
    await ms(1_000);
    expect(vi.getTimerCount()).toBe(1);
  });
});

describe("a notification arriving", () => {
  it("shows the toast for 4.5 s of un-hovered time, then collapses to the island with a dot", async () => {
    await mount();
    await notify();
    expect(island().dataset.view).toBe("notification");
    expect(indicator()).toBeUndefined();
    await ms(NOTIFICATION_MS - 100);
    expect(island().dataset.view).toBe("notification");
    await ms(200);
    expect(island().dataset.view).toBe("idle");
    expect(indicator()).toBeDefined();
    expect(indicator()!.textContent).toBe("");
  });

  it("pauses while the pointer is on it and resumes with the time that was left", async () => {
    await mount();
    await notify();
    await ms(3_000);
    act(() => void island().dispatchEvent(new Event("pointerover", { bubbles: true })));
    await ms(20_000);
    expect(island().dataset.view).toBe("notification");
    act(() => void island().dispatchEvent(new Event("pointerout", { bubbles: true })));
    await ms(1_000);
    expect(island().dataset.view).toBe("notification"); // 1.5 s were left, 1 s passed
    await ms(700);
    expect(island().dataset.view).toBe("idle");
  });

  it("restarts the full time for a second notification and counts both once collapsed", async () => {
    await mount();
    await notify("first");
    await ms(3_000);
    await notify("second");
    expect(island().textContent).toContain("second");
    await ms(3_000); // 6 s since the first, but only 3 since the second
    expect(island().dataset.view).toBe("notification");
    await ms(NOTIFICATION_MS - 3_000 + 100);
    expect(island().dataset.view).toBe("idle");
    expect(indicator()!.textContent).toBe("2");
  });

  it("never pops up while silenced, but the indicator shows in the island", async () => {
    await mount();
    act(() => silence.until(Date.now() + 3_600_000));
    await notify();
    await ms(200);
    expect(island().dataset.view).toBe("idle");
    expect(indicator()).toBeDefined();
  });

  it("clears the indicator when the island is opened", async () => {
    await mount();
    act(() => silence.until(Date.now() + 3_600_000));
    await notify();
    expect(indicator()).toBeDefined();
    await act(async () => island().click());
    expect(island().dataset.view).toBe("userExpanded");
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    await ms(500);
    expect(island().dataset.view).toBe("idle");
    expect(indicator()).toBeUndefined();
  });

  it("dismisses a toast with a 60 px horizontal drag without opening the app, and ignores a 30 px one", async () => {
    await mount();
    await notify();
    const toast = () => container.querySelector<HTMLElement>('[data-layer="notification"]')!;
    pointer(toast(), "pointerdown", 100);
    pointer(toast(), "pointerup", 130);
    pointer(toast(), "click", 130);
    expect(island().dataset.view).toBe("notification");
    expect(backend.activate).not.toHaveBeenCalled();
    await act(async () => {
      pointer(toast(), "pointerdown", 100);
      pointer(toast(), "pointerup", 160);
    });
    expect(island().dataset.view).toBe("idle");
    expect(backend.activate).not.toHaveBeenCalled();
  });
});

describe("arrival announcement", () => {
  it("announces a toast politely by app and title, and never moves focus to it", async () => {
    await mount();
    const before = document.activeElement;
    await notify("Build finished");
    await ms(300);
    const polite = container.querySelector('[aria-live="polite"]');
    expect(polite?.textContent).toContain("Slack");
    expect(polite?.textContent).toContain("Build finished");
    expect(container.querySelector('[aria-live="assertive"]')?.textContent ?? "").toBe("");
    expect(document.activeElement).toBe(before);
    expect(island().querySelector('[role="alert"]')).toBeNull();
  });
});

describe("island semantics while a toast shows", () => {
  it("is a group around the toast's own buttons, not a button containing buttons", async () => {
    await mount();
    expect(island().getAttribute("role")).toBe("button");
    await notify("Build finished");
    await ms(300);
    expect(island().getAttribute("role")).toBe("group");
    expect(island().hasAttribute("tabindex")).toBe(false);
    expect(island().hasAttribute("aria-expanded")).toBe(false);
    expect(island().querySelector("button")).not.toBeNull();
  });
});
