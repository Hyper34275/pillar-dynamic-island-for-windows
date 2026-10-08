// @vitest-environment jsdom
// The smart-search card end to end through the real shell: the backend's "assistant-update" events
// show it, a meeting alert pre-empts it and it comes back, Escape dismisses it, its timing, and the
// toasts that wait behind it. Fake timers; the backend's events are fed by hand.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ASSISTANT_ANSWER_MS, ASSISTANT_CHOICES_MS } from "../../lib/island/timing";
import type { ReminderAlert } from "../../lib/reminders/types";
import { PillShell } from "./PillShell";

const backend = vi.hoisted(() => ({
  handlers: new Map<string, (payload: unknown) => void>(),
  dismiss: vi.fn(),
  center: vi.fn(),
  keyboard: vi.fn(),
  showAlert: null as null | ((alert: unknown) => void),
}));

vi.mock("../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/ipc")>();
  return {
    ...original,
    onEvent: (name: string, handler: (payload: unknown) => void) => {
      backend.handlers.set(name, handler);
      return () => backend.handlers.delete(name);
    },
    ipc: { ...original.ipc, assistantDismiss: backend.dismiss, assistantOpenCenter: backend.center, islandKeyboard: backend.keyboard },
  };
});

vi.mock("../../hooks/useSettings", async () => {
  const { SETTINGS_DEFAULTS: defaults } = await vi.importActual<typeof import("../../lib/ipc")>("../../lib/ipc");
  return { useSettings: () => ({ settings: { ...defaults }, loaded: true, update: async () => true }) };
});

// The reminder engine is replaced by a handle that raises an alert on demand.
vi.mock("../../hooks/useReminders", () => ({
  useReminders: (show: (alert: unknown) => void) => {
    backend.showAlert = show;
    return { snooze: vi.fn() };
  },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = new Date(2026, 9, 7, 10, 0).getTime();
let container: HTMLDivElement;
let root: Root;
let nextId = 1;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
  backend.dismiss.mockReset();
  backend.center.mockReset();
  backend.keyboard.mockReset();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  backend.handlers.clear();
  backend.showAlert = null;
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-view]")!;
const layers = () => [...container.querySelectorAll<HTMLElement>("[data-layer]")].filter((el) => el.getAttribute("aria-hidden") !== "true").map((el) => el.dataset.layer);
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));
async function steps(n: number) {
  for (let t = 0; t < n; t += 50) await ms(Math.min(50, n - t));
}
async function mount() {
  await act(async () => {
    root.render(<PillShell />);
  });
  await ms(1500); // boot
}
const wire = (queryId: string, extra: Record<string, unknown> = {}) => ({
  queryId,
  query: "",
  phase: "processing",
  lang: "he",
  title: "",
  summary: "",
  question: null,
  choices: [],
  items: [],
  total: 0,
  partial: false,
  canExtend: false,
  errorCode: null,
  sources: [],
  createdAt: NOW,
  followUp: false,
  ...extra,
});
const send = (payload: unknown) =>
  act(async () => {
    backend.handlers.get("assistant-update")!(payload);
  });
const ANSWER = (queryId = "q1") =>
  wire(queryId, {
    phase: "answer",
    title: "מחר יש לאיציק 3 פגישות",
    summary: "09:00 · 11:30 · 14:00",
    total: 3,
    items: [
      { id: "e1", kind: "event", title: "Standup", subtitle: null, time: NOW, endTime: null, accent: "#0A84FF", openable: false, unread: false, source: "היומן של איציק" },
    ],
  });
const CHOICES = (queryId = "q1") =>
  wire(queryId, {
    phase: "choices",
    question: "באיזו תיבת דואר לחפש?",
    choices: [
      { id: "a", label: "תיבת הדואר שלי", kind: "mailbox", preferred: true },
      { id: "all", label: "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן.", kind: "allMailboxes", preferred: false },
    ],
  });
const alertEvent = (): ReminderAlert => ({
  key: "alert-1",
  eventId: "e",
  subject: "Design review",
  startUtc: new Date(NOW + 30 * 60_000).toISOString(),
  endUtc: new Date(NOW + 75 * 60_000).toISOString(),
  location: null,
  minutesRemaining: 30,
  reminderType: { kind: "beforeStart", minutes: 30 },
});
const notify = () =>
  act(async () => {
    backend.handlers.get("notification-received")!({ id: nextId++, appName: "Slack", title: "New message", body: "", timestamp: Date.now(), aumid: null });
  });
const key = (init: KeyboardEventInit) => act(async () => void document.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init })));

describe("the smart-search card in the shell", () => {
  it("shows the working card, then the answer in place: one layer, one session, the same query", async () => {
    await mount();
    expect(island().dataset.view).toBe("idle");
    await send(wire("q1"));
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
    expect(island().textContent).toContain("Working…");
    expect(island().getAttribute("role")).toBe("group");
    expect(island().getAttribute("aria-label")).toBe("Smart search");
    const layer = container.querySelector('[data-layer="assistant"]');

    await send(ANSWER());
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
    expect(island().textContent).toContain("מחר יש לאיציק 3 פגישות");
    expect(island().textContent).toContain("Standup");
    // The layer was not replaced: no new session, no morph restart.
    expect(container.querySelector('[data-layer="assistant"]')).toBe(layer);
    expect(container.querySelectorAll('[data-layer="assistant"]')).toHaveLength(1);
  });

  it("ignores an unusable payload", async () => {
    await mount();
    await send({ queryId: "bad id!", phase: "answer" });
    await send(null);
    await send({ queryId: "ok", phase: "nonsense" });
    expect(island().dataset.view).toBe("idle");
  });

  it("never takes the keyboard", async () => {
    await mount();
    await send(CHOICES());
    await ms(150);
    expect(backend.keyboard).not.toHaveBeenCalled();
  });

  it("an answer goes after 12 s unattended and a question after 60 s", async () => {
    await mount();
    await send(ANSWER());
    await steps(ASSISTANT_ANSWER_MS - 100);
    expect(island().dataset.view).toBe("assistant");
    await steps(300);
    expect(island().dataset.view).toBe("idle");

    await send(CHOICES("q2"));
    await steps(ASSISTANT_ANSWER_MS + 1000);
    expect(island().dataset.view).toBe("assistant");
    await steps(ASSISTANT_CHOICES_MS - ASSISTANT_ANSWER_MS - 1000 + 300);
    expect(island().dataset.view).toBe("idle");
    // Timing out is not a dismiss: the question stays answerable from the search bar.
    expect(backend.dismiss).not.toHaveBeenCalled();
  });

  it("a working card keeps waiting however long (only a backstop ends it) and a final card restarts the time", async () => {
    await mount();
    await send(wire("q1"));
    await steps(30_000);
    expect(island().dataset.view).toBe("assistant");
    await send(ANSWER());
    await steps(ASSISTANT_ANSWER_MS - 200);
    expect(island().dataset.view).toBe("assistant");
  });

  it("the pointer on the island holds the time", async () => {
    await mount();
    await send(ANSWER());
    const hit = container.querySelector<HTMLElement>("[data-island-hit]")!;
    await act(async () => void hit.dispatchEvent(new MouseEvent("pointerover", { bubbles: true })));
    await steps(ASSISTANT_ANSWER_MS * 2);
    expect(island().dataset.view).toBe("assistant");
    await act(async () => void hit.dispatchEvent(new MouseEvent("pointerout", { bubbles: true })));
    await steps(ASSISTANT_ANSWER_MS + 300);
    expect(island().dataset.view).toBe("idle");
  });

  it("Escape dismisses it and tells the backend", async () => {
    await mount();
    await send(ANSWER("q9"));
    await ms(150);
    await key({ key: "Escape" });
    await ms(150);
    expect(island().dataset.view).toBe("idle");
    expect(backend.dismiss).toHaveBeenCalledWith("q9");
  });

  it("Escape that a control already used is not ours", async () => {
    await mount();
    await send(ANSWER());
    await act(async () => {
      const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
      event.preventDefault();
      document.dispatchEvent(event);
    });
    expect(island().dataset.view).toBe("assistant");
  });

  it("the close button and the toggle shortcut dismiss it too", async () => {
    await mount();
    await send(ANSWER("q1"));
    await ms(150);
    const close = [...container.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.getAttribute("aria-label") === "Close")!;
    await act(async () => void close.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(island().dataset.view).toBe("idle");
    expect(backend.dismiss).toHaveBeenCalledWith("q1");

    await send(ANSWER("q2"));
    await ms(150);
    await key({ key: " ", ctrlKey: true, shiftKey: true });
    await ms(150);
    // The panel the person asked for replaces the card.
    expect(island().dataset.view).toBe("userExpanded");
    expect(backend.dismiss).toHaveBeenCalledWith("q2");
  });

  it("show all results opens the Center and the card goes without a dismiss", async () => {
    await mount();
    await send(wire("q5", { ...ANSWER("q5"), total: 9 }));
    await ms(150);
    const all = [...container.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Show all results")!;
    await act(async () => void all.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(backend.center).toHaveBeenCalledWith("q5");
    expect(island().dataset.view).toBe("idle");
    expect(backend.dismiss).not.toHaveBeenCalled();
  });

  it("closes an open panel (it is what the person asked for just now)", async () => {
    await mount();
    await key({ key: " ", ctrlKey: true, shiftKey: true });
    await ms(150);
    expect(island().dataset.view).toBe("userExpanded");
    await send(ANSWER());
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
  });

  it("does not expand on hover while it shows", async () => {
    await mount();
    await send(ANSWER());
    const hit = container.querySelector<HTMLElement>("[data-island-hit]")!;
    await act(async () => void hit.dispatchEvent(new MouseEvent("pointerover", { bubbles: true })));
    await ms(500);
    expect(island().dataset.view).toBe("assistant");
  });
});

describe("a meeting alert and the card", () => {
  it("the alert pre-empts the card, and the card comes back after the alert", async () => {
    await mount();
    await send(CHOICES());
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
    await act(async () => backend.showAlert!(alertEvent()));
    await ms(150);
    expect(island().dataset.view).toBe("meetingAlert");
    expect(island().textContent).toContain("Design review");

    // The alert's own time (8 s) passes; the card was not running meanwhile.
    await steps(8_300);
    expect(island().dataset.view).toBe("assistant");
    expect(island().textContent).toContain("באיזו תיבת דואר לחפש?");
  });

  it("an update under the alert is kept, and Escape on the alert leaves the card alone", async () => {
    await mount();
    await send(wire("q1"));
    await act(async () => backend.showAlert!(alertEvent()));
    await ms(150);
    await send(ANSWER());
    await key({ key: "Escape" });
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
    expect(island().textContent).toContain("מחר יש לאיציק 3 פגישות");
    expect(backend.dismiss).not.toHaveBeenCalled();
  });

  it("the card's time does not run while the alert shows", async () => {
    await mount();
    await send(ANSWER());
    await steps(ASSISTANT_ANSWER_MS - 3000);
    await act(async () => backend.showAlert!(alertEvent()));
    await steps(8_300);
    expect(island().dataset.view).toBe("assistant");
    await steps(3_300);
    expect(island().dataset.view).toBe("idle");
  });
});

describe("a closed working card", () => {
  it("is not brought back by the answer that still arrives for it", async () => {
    await mount();
    await send(wire("q1"));
    await ms(150);
    await key({ key: "Escape" });
    await ms(150);
    expect(backend.dismiss).toHaveBeenCalledWith("q1");
    await send(ANSWER("q1"));
    await ms(150);
    expect(island().dataset.view).toBe("idle");
    // A new question is a new card.
    await send(wire("q2"));
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
  });
});

describe("toasts and the card", () => {
  it("a notification waits behind the card and shows when it ends", async () => {
    await mount();
    await send(ANSWER());
    await ms(150);
    await notify();
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
    await steps(ASSISTANT_ANSWER_MS);
    expect(island().dataset.view).toBe("notification");
  });

  it("a toast on screen goes back to the line when the card arrives, and returns after it", async () => {
    await mount();
    await notify();
    await ms(150);
    expect(island().dataset.view).toBe("notification");
    await send(ANSWER());
    await ms(150);
    expect(island().dataset.view).toBe("assistant");
    await key({ key: "Escape" });
    await ms(150);
    expect(island().dataset.view).toBe("notification");
  });

  it("the card is only ever one layer of the island at rest", async () => {
    await mount();
    await send(ANSWER());
    await ms(2000);
    expect(layers()).toEqual(["assistant"]);
  });
});
