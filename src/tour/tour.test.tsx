// @vitest-environment jsdom
// The tour page: all twelve steps with mock data, no IPC at all, an inert stage, the controls and
// autoplay, and the timers it owns. Fake timers; the Tauri bridge is replaced by spies that must
// never be called.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import tourHtml from "../../tour.html?raw";
import { pillDimensions } from "../components/Pill/animations";
import { smallExpanded } from "../design/tokens";
import { t, type MessageKey } from "../lib/i18n";
import { SCROLL_START_MS, SCROLL_STEP_MS } from "./autoScroll";
import { TOUR_SYSTEM_INFO } from "./mockData";
import { parseTourParams } from "./params";
import { DAY_SWITCH_MS, RINGER_SWITCH_MS, TOUR_STEP_COUNT } from "./steps";
import { AUTOPLAY_MS, TourApp } from "./TourApp";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  emit: vi.fn(),
  tauriInvoke: vi.fn(),
}));

// The page is "inside Tauri" as far as the guards can tell, so any call would reach a spy.
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke, isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen, emit: bridge.emit }));
vi.mock("../lib/tauri", () => ({
  tauriInvoke: bridge.tauriInvoke,
  isTauriAvailable: () => true,
  TauriTimeoutError: class extends Error {},
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
const hostMessages = vi.fn();

beforeEach(() => {
  vi.useFakeTimers();
  // No canvas in jsdom: text is measured by the estimate, without a "not implemented" error each time.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  Object.assign(window, { chrome: { webview: { postMessage: hostMessages } } });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  hostMessages.mockClear();
  Object.values(bridge).forEach((spy) => spy.mockClear());
  delete (window as { chrome?: unknown }).chrome;
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));

async function mount(props: { initialStep?: number; autoplay?: boolean } = {}) {
  await act(async () => {
    root.render(<TourApp initialStep={0} autoplay={false} {...props} />);
  });
}

const stage = () => container.querySelector<HTMLElement>('[data-testid="tour-stage"]')!;
const island = () => container.querySelector<HTMLElement>(".tour-island")!;
const heading = () => container.querySelector("h1")!.textContent;
const eyebrow = () => container.querySelector(".tour-eyebrow")!.textContent;
const titleOf = (n: number) => t(`tour.s${n}.title` as MessageKey);
const button = (label: string) => [...container.querySelectorAll("button")].find((b) => b.textContent?.includes(label)) as HTMLButtonElement;
const dots = () => [...container.querySelectorAll<HTMLButtonElement>(".tour-dot")];
const click = (el: Element) => act(async () => void el.dispatchEvent(new MouseEvent("click", { bubbles: true })));
const noIpc = () => {
  for (const [name, spy] of Object.entries(bridge)) expect(spy, name).not.toHaveBeenCalled();
};

describe("the twelve steps", () => {
  it.each(Array.from({ length: TOUR_STEP_COUNT }, (_, i) => i))("step %i renders its own title, a stage and no IPC", async (index) => {
    await mount({ initialStep: index });
    expect(heading()).toBe(titleOf(index + 1));
    expect(eyebrow()).toBe(t("tour.step", { n: index + 1, total: TOUR_STEP_COUNT }));
    expect(container.querySelector(".tour-text")!.textContent).toBeTruthy();
    expect(stage().querySelector("[data-layer-id]")).not.toBeNull();
    noIpc();
  });

  it("has an explanation for every step in both languages", () => {
    for (let n = 1; n <= TOUR_STEP_COUNT; n++) {
      for (const locale of ["en", "he"] as const) {
        for (const part of ["title", "text"]) expect(t(`tour.s${n}.${part}` as MessageKey, undefined, locale), `${locale} s${n} ${part}`).toBeTruthy();
      }
    }
    expect(t("tour.s1.title", undefined, "he")).toMatch(/[֐-׿]/);
  });

  it("sizes the island with the functions the real one uses", async () => {
    const sizeAt = async (index: number) => {
      root.unmount();
      root = createRoot(container);
      await mount({ initialStep: index });
      return { w: island().style.width, h: island().style.height, r: island().style.borderRadius };
    };
    const e = pillDimensions.expanded;
    for (const index of [7, 8, 9, 10, 11]) expect(await sizeAt(index)).toEqual({ w: `${e.width}px`, h: `${e.height}px`, r: `${e.radius}px` });
    expect(await sizeAt(4)).toEqual({
      w: `${pillDimensions.ringer.width}px`,
      h: `${pillDimensions.ringer.height}px`,
      r: `${pillDimensions.ringer.height / 2}px`,
    });
    expect((await sizeAt(0)).h).toBe(`${pillDimensions.compact.height}px`);
    expect((await sizeAt(5)).w).toBe(`${smallExpanded.width}px`);
    expect((await sizeAt(3)).w).toBe(`${smallExpanded.width}px`);
  });

  it("shows the collapsed island in the full display: date, clock, weekday and the unseen indicator", async () => {
    await mount({ initialStep: 0 });
    const compact = stage().querySelector('[data-layer="compact"]')!;
    expect(compact.textContent).toMatch(/\d{1,2}[./]\d{1,2}/);
    expect(compact.textContent).toMatch(/\d{1,2}:\d{2}/);
    expect(compact.textContent).toContain("3");
  });

  it("switches ring to silent after a moment (step 5)", async () => {
    await mount({ initialStep: 4 });
    expect(stage().textContent).toContain(t("ringer.ring"));
    await ms(RINGER_SWITCH_MS + 50);
    expect(stage().textContent).toContain(t("ringer.silent"));
    expect(stage().textContent).not.toContain(t("ringer.ring"));
    noIpc();
  });

  it("flips through three days in the calendar step (step 8)", async () => {
    await mount({ initialStep: 7 });
    const selected = () => stage().querySelector('button[aria-pressed="true"]')!.textContent;
    const first = selected();
    await ms(DAY_SWITCH_MS + 50);
    const second = selected();
    expect(second).not.toBe(first);
    await ms(DAY_SWITCH_MS);
    expect(selected()).not.toBe(second);
    await ms(DAY_SWITCH_MS);
    expect(selected()).toBe(first);
    noIpc();
  });
});

describe("the stage", () => {
  it.each([0, 3, 5, 7, 11])("is inert, hidden from assistive tech and deaf to the pointer (step %i)", async (index) => {
    await mount({ initialStep: index });
    expect(stage().hasAttribute("inert")).toBe(true);
    expect(stage().getAttribute("aria-hidden")).toBe("true");
    expect(stage().style.pointerEvents).toBe("none");
    expect(stage().getAttribute("dir")).toBe("ltr");
    expect(stage().querySelector("a[href]")).toBeNull();
  });

  it("calls nothing even when its buttons are clicked", async () => {
    for (const index of [3, 5, 6, 9, 11]) {
      root.unmount();
      root = createRoot(container);
      await mount({ initialStep: index });
      for (const el of stage().querySelectorAll("button, [role='button']")) await click(el);
    }
    noIpc();
    expect(hostMessages).not.toHaveBeenCalled();
  });
});

describe("the controls", () => {
  it("goes forward and back, and the first step has no back", async () => {
    await mount();
    expect(button(t("tour.prev")).disabled).toBe(true);
    await click(button(t("tour.next")));
    expect(heading()).toBe(titleOf(2));
    await click(button(t("tour.next")));
    expect(heading()).toBe(titleOf(3));
    await click(button(t("tour.prev")));
    expect(heading()).toBe(titleOf(2));
  });

  it("has one dot per step, the current one marked, and a dot jumps to its step", async () => {
    await mount();
    expect(dots()).toHaveLength(TOUR_STEP_COUNT);
    expect(dots()[0].getAttribute("aria-current")).toBe("step");
    expect(dots().filter((d) => d.hasAttribute("aria-current"))).toHaveLength(1);
    await click(dots()[6]);
    expect(heading()).toBe(titleOf(7));
    expect(dots()[6].getAttribute("aria-current")).toBe("step");
    expect(dots()[6].getAttribute("aria-label")).toBe(t("tour.dot", { n: 7, title: titleOf(7) }));
  });

  it("answers the arrow keys the way a right-to-left page reads (left is forward)", async () => {
    await mount({ initialStep: 3 });
    await act(async () => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft" })));
    expect(heading()).toBe(titleOf(5));
    await act(async () => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" })));
    expect(heading()).toBe(titleOf(4));
  });

  it("finishes on the last step and tells the host", async () => {
    await mount({ initialStep: TOUR_STEP_COUNT - 1 });
    expect(button(t("tour.next"))).toBeUndefined();
    await click(button(t("tour.finish")));
    expect(hostMessages).toHaveBeenCalledWith({ type: "done" });
  });

  it("offers the Center pages next to the notes and settings steps", async () => {
    await mount({ initialStep: 9 });
    await click(button(t("tour.openNotes")));
    expect(hostMessages).toHaveBeenLastCalledWith({ type: "navigate", page: "notes" });
    await click(button(t("tour.next")));
    await click(button(t("tour.next")));
    await click(button(t("tour.openSettings")));
    expect(hostMessages).toHaveBeenLastCalledWith({ type: "navigate", page: "settings" });
  });

  it("is a no-op outside the Center (no WebView2 host)", async () => {
    delete (window as { chrome?: unknown }).chrome;
    await mount({ initialStep: TOUR_STEP_COUNT - 1 });
    await expect(click(button(t("tour.finish")))).resolves.not.toThrow();
  });
});

describe("autoplay", () => {
  it("is on by default and moves on every five seconds", async () => {
    await mount({ autoplay: true });
    expect(button(t("tour.autoplay")).getAttribute("aria-pressed")).toBe("true");
    await ms(AUTOPLAY_MS - 100);
    expect(heading()).toBe(titleOf(1));
    await ms(150);
    expect(heading()).toBe(titleOf(2));
    await ms(AUTOPLAY_MS);
    expect(heading()).toBe(titleOf(3));
  });

  it("stops when the person navigates by hand", async () => {
    await mount({ autoplay: true });
    await ms(AUTOPLAY_MS + 50);
    await click(button(t("tour.next")));
    expect(heading()).toBe(titleOf(3));
    expect(button(t("tour.autoplay")).getAttribute("aria-pressed")).toBe("false");
    await ms(AUTOPLAY_MS * 3);
    expect(heading()).toBe(titleOf(3));
  });

  it("can be switched off and on again", async () => {
    await mount({ autoplay: true });
    await click(button(t("tour.autoplay")));
    await ms(AUTOPLAY_MS * 2);
    expect(heading()).toBe(titleOf(1));
    await click(button(t("tour.autoplay")));
    await ms(AUTOPLAY_MS + 50);
    expect(heading()).toBe(titleOf(2));
  });

  it("walks through every step, then stops at the last one, with no IPC on the way", async () => {
    await mount({ autoplay: true });
    for (let n = 2; n <= TOUR_STEP_COUNT; n++) {
      await ms(AUTOPLAY_MS + 50);
      expect(heading()).toBe(titleOf(n));
    }
    expect(button(t("tour.autoplay")).getAttribute("aria-pressed")).toBe("false");
    await ms(AUTOPLAY_MS * 3);
    expect(heading()).toBe(titleOf(TOUR_STEP_COUNT));
    noIpc();
    expect(hostMessages).not.toHaveBeenCalled();
  });

  it("starts over from the first step when switched on at the end", async () => {
    await mount({ initialStep: TOUR_STEP_COUNT - 1 });
    await click(button(t("tour.autoplay")));
    expect(heading()).toBe(titleOf(1));
    expect(button(t("tour.autoplay")).getAttribute("aria-pressed")).toBe("true");
  });
});

describe("timers", () => {
  it("leaves none behind when the page goes away", async () => {
    await mount({ autoplay: true });
    await ms(AUTOPLAY_MS + 50);
    await click(button(t("tour.autoplay")));
    await click(dots()[7]); // the calendar: it owns an interval of its own
    await ms(DAY_SWITCH_MS + 50);
    await click(dots()[10]); // about: its clock ticks
    await ms(2500);
    act(() => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
  });

  it("has only the autoplay timer while a plain step is shown", async () => {
    await mount({ initialStep: 1, autoplay: true });
    await ms(500);
    expect(vi.getTimerCount()).toBe(1);
  });

  it("has no timer at all on a plain step without autoplay", async () => {
    await mount({ initialStep: 1 });
    await ms(500);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("clears a replaced step's leftovers once its fade is over", async () => {
    await mount({ initialStep: 4 });
    await click(button(t("tour.next")));
    expect(stage().querySelectorAll("[data-layer-id]").length).toBeGreaterThan(1);
    await ms(500);
    expect(island().querySelectorAll(":scope > [data-layer-id]")).toHaveLength(1);
  });
});

describe("the address", () => {
  it("opens on the step it names, without autoplay, for screenshots", () => {
    expect(parseTourParams("?step=7")).toMatchObject({ initialStep: 6, autoplay: false });
    expect(parseTourParams("?step=12&autoplay=1")).toMatchObject({ initialStep: 11, autoplay: true });
  });

  it("keeps the step in range and autoplay on without one", () => {
    expect(parseTourParams("")).toMatchObject({ initialStep: 0, autoplay: true });
    expect(parseTourParams("?step=0").initialStep).toBe(0);
    expect(parseTourParams("?step=99").initialStep).toBe(TOUR_STEP_COUNT - 1);
    expect(parseTourParams("?step=abc").initialStep).toBe(0);
  });

  it("follows Israel's regional format unless told otherwise", () => {
    expect(parseTourParams("").format).toBe("he-IL");
    expect(parseTourParams("?format=en-US").format).toBe("en-US");
  });
});

describe("the page", () => {
  it("carries the contract's content security policy and loads only its own script", () => {
    const csp = tourHtml.match(/http-equiv="Content-Security-Policy"\s+content="([^"]+)"/)?.[1] ?? "";
    expect(csp).toBe(
      "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"
    );
    expect(tourHtml).toContain('src="/src/tour/main.tsx"');
    expect(tourHtml).toMatch(/<html[^>]*dir="rtl"/);
  });

  it("keeps the mock island's morph and fades under reduced motion", async () => {
    // The stylesheet as written (vitest hands CSS imports back empty), read through node without its types.
    const nodeModule = (name: string) => import(/* @vite-ignore */ ["node", name].join(":"));
    const { readFileSync } = (await nodeModule("fs")) as { readFileSync: (path: string, encoding: string) => string };
    const { fileURLToPath } = (await nodeModule("url")) as { fileURLToPath: (url: string) => string };
    const indexCss = readFileSync(fileURLToPath(import.meta.url.replace(/[^/]*$/, "../index.css")), "utf8");
    expect(indexCss.slice(indexCss.indexOf(".tour-island") - 400)).toMatch(/@media \(prefers-reduced-motion: reduce\)/);
    expect(indexCss).toMatch(/\.tour-island\s*\{[^}]*transition-duration:\s*420ms\s*!important/);
    expect(indexCss).toMatch(/\.tour-island \.tour-layer--in\s*\{[^}]*animation-duration/);
    expect(indexCss).toMatch(/\.tour-island \.tour-layer--out\s*\{[^}]*animation-duration/);
  });
});

describe("the panel of the expanded steps (inert, so code scrolls it)", () => {
  const scrollTo = vi.fn();
  const sizes = { scrollHeight: 900, clientHeight: 288 };
  const defined: string[] = [];
  const define = (name: string, descriptor: PropertyDescriptor) => {
    Object.defineProperty(HTMLElement.prototype, name, { configurable: true, ...descriptor });
    defined.push(name);
  };

  beforeEach(() => {
    scrollTo.mockClear();
    define("scrollTo", { value: scrollTo, writable: true });
    define("scrollHeight", { get: () => sizes.scrollHeight });
    define("clientHeight", { get: () => sizes.clientHeight });
  });
  afterEach(() => {
    for (const name of defined.splice(0)) delete (HTMLElement.prototype as unknown as Record<string, unknown>)[name];
  });

  it("walks the settings through the display choice, the Center buttons and the end", async () => {
    await mount({ initialStep: 11 });
    expect(scrollTo).not.toHaveBeenCalled();
    await ms(SCROLL_START_MS + 50);
    expect(scrollTo).toHaveBeenCalledTimes(1);
    await ms(SCROLL_STEP_MS);
    expect(scrollTo).toHaveBeenCalledTimes(2);
    await ms(SCROLL_STEP_MS);
    expect(scrollTo).toHaveBeenCalledTimes(3);
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 900 - 288, behavior: "smooth" });
    await ms(SCROLL_STEP_MS * 3);
    expect(scrollTo).toHaveBeenCalledTimes(3);
    expect(vi.getTimerCount()).toBe(0);
    noIpc();
  });

  it.each([7, 8, 9])("scrolls step %i to the end of its content", async (index) => {
    await mount({ initialStep: index });
    await ms(SCROLL_START_MS + 50);
    expect(scrollTo).toHaveBeenCalledWith({ top: 900 - 288, behavior: "smooth" });
  });

  it("does not scroll About or the compact steps", async () => {
    for (const index of [0, 10]) {
      root.unmount();
      root = createRoot(container);
      await mount({ initialStep: index });
      await ms(SCROLL_START_MS * 4);
    }
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("scrolls without animation under reduced motion, and still scrolls", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce"), media: query, addEventListener() {}, removeEventListener() {} }));
    try {
      await mount({ initialStep: 11 });
      await ms(SCROLL_START_MS + SCROLL_STEP_MS * 2 + 50);
      expect(scrollTo).toHaveBeenCalledTimes(3);
      expect(scrollTo).toHaveBeenLastCalledWith({ top: 900 - 288, behavior: "auto" });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("stops at once when the step is left, and leaves no timer when the page goes away mid-scroll", async () => {
    await mount({ initialStep: 11 });
    await ms(SCROLL_START_MS + 50);
    expect(scrollTo).toHaveBeenCalledTimes(1);
    await click(button(t("tour.prev"))); // About: nothing to scroll
    await ms(SCROLL_START_MS + SCROLL_STEP_MS * 3);
    expect(scrollTo).toHaveBeenCalledTimes(1);
    await click(button(t("tour.next")));
    await ms(SCROLL_START_MS + 50);
    act(() => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
    root = createRoot(container);
  });
});

describe("the double click on Next", () => {
  it("does not let a second click reach Finish on the last step", async () => {
    await mount({ initialStep: TOUR_STEP_COUNT - 2 });
    const next = button(t("tour.next"));
    await click(next);
    expect(heading()).toBe(titleOf(TOUR_STEP_COUNT));
    const finish = button(t("tour.finish"));
    expect(finish).not.toBe(next);
    expect(next.isConnected).toBe(false);
    await click(next);
    expect(hostMessages).not.toHaveBeenCalled();
    await click(finish);
    expect(hostMessages).toHaveBeenCalledWith({ type: "done" });
  });
});

describe("mock data", () => {
  it("keeps the backslash of the Windows user", () => {
    expect(TOUR_SYSTEM_INFO.windowsUser).toBe("CORP\\dana");
  });
});

describe("announcements", () => {
  it("keeps the explanation out of a live region and announces only the title, only by hand", async () => {
    await mount({ initialStep: 2, autoplay: true });
    expect(container.querySelector(".tour-copy")!.hasAttribute("aria-live")).toBe(false);
    const status = () => container.querySelector<HTMLElement>("[role=status]")!;
    expect(status().textContent).toBe("");
    await ms(AUTOPLAY_MS + 50);
    expect(status().textContent).toBe("");
    await click(button(t("tour.next")));
    expect(status().textContent).toContain(titleOf(5));
    expect(status().textContent).not.toContain(t(`tour.s5.text` as MessageKey));
  });
});

