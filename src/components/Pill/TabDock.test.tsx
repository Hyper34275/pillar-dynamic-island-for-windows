// @vitest-environment jsdom
// The tablist's keyboard (WAI-ARIA tabs pattern) and Escape, through the real PillShell.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFixedLocale } from "../../lib/i18n";
import { PillShell } from "./PillShell";

// The notes list is the backend's; here it is loaded and empty, so the composer's text box shows.
vi.mock("../../hooks/useNotes", () => ({
  useNotes: () => ({
    notes: [],
    loaded: true,
    loadFailed: false,
    retry: vi.fn(),
    add: vi.fn(async () => true),
    update: vi.fn(async () => true),
    remove: vi.fn(async () => true),
    togglePin: vi.fn(async () => true),
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 6, 15, 30));
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  setFixedLocale(null);
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-expanded]")!;
const tab = (id: string) => container.querySelector<HTMLElement>(`#tab-${id}`)!;
const selected = () => container.querySelector('[role="tab"][aria-selected="true"]')!.id;

async function mountExpanded() {
  await act(async () => {
    root.render(<PillShell />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1500);
  });
  await act(async () => {
    island().click();
  });
}

async function keydown(target: Element, key: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  await act(async () => {
    target.dispatchEvent(event);
  });
  return event;
}

describe("tablist keyboard", () => {
  it("moves selection AND DOM focus with ArrowRight/ArrowLeft/Home/End, wrapping, in dock order", async () => {
    await mountExpanded();
    tab("calendar").focus();
    expect(document.activeElement).toBe(tab("calendar"));

    await keydown(tab("calendar"), "ArrowRight");
    expect(selected()).toBe("tab-notifications");
    expect(document.activeElement).toBe(tab("notifications"));
    // roving tabindex follows
    expect(tab("notifications").tabIndex).toBe(0);
    expect(tab("calendar").tabIndex).toBe(-1);

    await keydown(document.activeElement!, "End");
    expect(selected()).toBe("tab-settings");
    expect(document.activeElement).toBe(tab("settings"));

    await keydown(tab("settings"), "ArrowRight"); // wraps forwards
    expect(selected()).toBe("tab-calendar");
    expect(document.activeElement).toBe(tab("calendar"));

    await keydown(tab("calendar"), "ArrowLeft"); // wraps backwards
    expect(selected()).toBe("tab-settings");
    expect(document.activeElement).toBe(tab("settings"));

    await keydown(tab("settings"), "Home");
    expect(selected()).toBe("tab-calendar");
    expect(document.activeElement).toBe(tab("calendar"));
  });

  it("is left to right in Hebrew too: ArrowRight is the next tab", async () => {
    setFixedLocale("he");
    await mountExpanded();
    tab("calendar").focus();
    await keydown(tab("calendar"), "ArrowRight");
    expect(selected()).toBe("tab-notifications");
    expect(document.activeElement).toBe(tab("notifications"));
  });

  it("leaves modified arrows and other keys alone", async () => {
    await mountExpanded();
    tab("calendar").focus();
    const withAlt = await keydown(tab("calendar"), "ArrowRight", { altKey: true });
    expect(withAlt.defaultPrevented).toBe(false);
    const letter = await keydown(tab("calendar"), "a");
    expect(letter.defaultPrevented).toBe(false);
    expect(selected()).toBe("tab-calendar");
  });

  it("does not switch tabs when arrows or Home/End are pressed inside the note box", async () => {
    await mountExpanded();
    await act(async () => {
      tab("notes").click();
    });
    expect(selected()).toBe("tab-notes");
    const box = container.querySelector<HTMLTextAreaElement>("textarea");
    expect(box).not.toBeNull();
    box!.focus();
    for (const key of ["ArrowRight", "ArrowLeft", "Home", "End"]) {
      const event = await keydown(box!, key);
      expect(selected()).toBe("tab-notes");
      expect(event.defaultPrevented).toBe(false); // the text box keeps its caret movement
    }
    expect(document.activeElement).toBe(box);
  });

  it("does not switch tabs when an arrow is pressed on the document or on a control in a panel", async () => {
    await mountExpanded();
    await keydown(document.body, "ArrowRight");
    await keydown(container.querySelector('[role="tabpanel"]')!, "ArrowRight");
    expect(selected()).toBe("tab-calendar");
  });
});

describe("Escape", () => {
  it("collapses the panel when nothing inside handled it", async () => {
    await mountExpanded();
    tab("calendar").focus();
    await keydown(tab("calendar"), "Escape");
    expect(island().getAttribute("data-expanded")).toBe("false");
  });

  it("does not collapse when the note box handles it (it leaves the box instead)", async () => {
    await mountExpanded();
    await act(async () => {
      tab("notes").click();
    });
    const box = container.querySelector<HTMLTextAreaElement>("textarea")!;
    box.focus();
    expect(document.activeElement).toBe(box);
    await keydown(box, "Escape");
    expect(island().getAttribute("data-expanded")).toBe("true");
    expect(document.activeElement).not.toBe(box);
  });

  it("is ignored when an inner control already prevented it", async () => {
    await mountExpanded();
    const inner = container.querySelector<HTMLElement>('[role="tabpanel"]')!;
    inner.addEventListener("keydown", (e) => e.preventDefault(), { once: true });
    const event = await keydown(inner, "Escape");
    expect(event.defaultPrevented).toBe(true);
    expect(island().getAttribute("data-expanded")).toBe("true");
    // the next, unhandled Escape closes it
    await keydown(inner, "Escape");
    expect(island().getAttribute("data-expanded")).toBe("false");
  });
});
