// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_BAR, SPOTLIGHT_BAR } from "./bar";
import { SearchBar, type SearchApi } from "./SearchBar";
import { ss } from "./strings";
import type { AssistantCard } from "../lib/assistant/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let handlers: Record<string, (p: unknown) => void>;

const subscribe = (name: string, h: (p: unknown) => void) => {
  handlers[name] = h;
  return () => {
    delete handlers[name];
  };
};

function card(queryId: string, phase: AssistantCard["phase"]) {
  return { queryId, query: "", phase, lang: "he", title: "t", summary: "", question: null, choices: [], items: [], total: 0, partial: false, canExtend: false, errorCode: null, sources: [], createdAt: 0, followUp: false };
}

function mount(api: SearchApi, props: Partial<Parameters<typeof SearchBar>[0]> = {}) {
  act(() => {
    root.render(createElement(SearchBar, { bar: DEFAULT_BAR, api, subscribe, ...props }));
  });
}

function type(input: HTMLInputElement, value: string) {
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    set.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function press(input: HTMLInputElement, key: string, init: KeyboardEventInit = {}) {
  const ev = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  act(() => {
    input.dispatchEvent(ev);
  });
  return ev;
}

const glowState = () => host.querySelector(".ci-glow")?.getAttribute("data-state") ?? null;

beforeEach(() => {
  handlers = {};
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => setTimeout(() => cb(0), 0) as unknown as number);
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("SearchBar", () => {
  const makeApi = (result: AssistantCard | null = card("q1", "answer") as AssistantCard) => ({
    submit: vi.fn().mockResolvedValue(result),
    close: vi.fn(),
  });

  it("focuses the input on mount, with the locale placeholder, auto direction once typed", () => {
    mount(makeApi());
    const input = host.querySelector("input")!;
    expect(document.activeElement).toBe(input);
    expect(input.placeholder).toMatch(/יובל|Yuval/);
    type(input, "שלום");
    expect(input.getAttribute("dir")).toBe("auto");
  });

  it("glows (Activated) once shown", () => {
    mount(makeApi());
    expect(glowState()).toBe("activated");
  });

  it("submits on Enter with origin searchBar semantics, trimmed", async () => {
    const api = makeApi();
    mount(api);
    const input = host.querySelector("input")!;
    type(input, "  מה יש לאיציק ביומן מחר?  ");
    expect(glowState()).toBe("typing");
    await act(async () => {
      press(input, "Enter");
    });
    expect(api.submit).toHaveBeenCalledWith("מה יש לאיציק ביומן מחר?");
    expect(glowState()).toBe("completed");
  });

  it("does not submit on an empty field or while an IME composition is open", () => {
    const api = makeApi();
    mount(api);
    const input = host.querySelector("input")!;
    press(input, "Enter");
    type(input, "abc");
    press(input, "Enter", { isComposing: true });
    expect(api.submit).not.toHaveBeenCalled();
  });

  it("Escape and the AI button close the bar", () => {
    const api = makeApi();
    mount(api);
    const input = host.querySelector("input")!;
    expect(press(input, "Escape").defaultPrevented).toBe(true);
    expect(api.close).toHaveBeenCalledTimes(1);
    const btn = host.querySelector("button.sb-ai") as HTMLButtonElement;
    expect(btn.getAttribute("aria-label")).toMatch(/AI/);
    act(() => btn.click());
    expect(api.close).toHaveBeenCalledTimes(2);
  });

  it("shows Processing for the pushed card, ignores a stale one, then completes", async () => {
    let resolve!: (c: AssistantCard) => void;
    const api = { submit: vi.fn().mockReturnValue(new Promise<AssistantCard>((r) => (resolve = r))), close: vi.fn() };
    mount(api);
    const input = host.querySelector("input")!;
    type(input, "x");
    act(() => {
      press(input, "Enter");
    });
    expect(glowState()).toBe("submitting");
    act(() => handlers["assistant-update"](card("q9", "processing")));
    expect(glowState()).toBe("processing");
    act(() => handlers["assistant-update"](card("stale", "answer")));
    expect(glowState()).toBe("processing");
    await act(async () => {
      resolve(card("q9", "answer") as AssistantCard);
    });
    expect(glowState()).toBe("completed");
  });

  it("settles back to Activated after the 600 ms hold", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    mount(api);
    const input = host.querySelector("input")!;
    type(input, "x");
    await act(async () => {
      press(input, "Enter");
    });
    expect(glowState()).toBe("completed");
    act(() => {
      vi.advanceTimersByTime(599);
    });
    expect(glowState()).toBe("completed");
    act(() => {
      vi.advanceTimersByTime(2);
    });
    expect(glowState()).toBe("activated");
  });

  it("a failed submit turns the ring to Error; typing again clears it", async () => {
    const api = { submit: vi.fn().mockRejectedValue(new Error("x")), close: vi.fn() };
    mount(api);
    const input = host.querySelector("input")!;
    type(input, "x");
    await act(async () => {
      press(input, "Enter");
    });
    expect(glowState()).toBe("error");
    type(input, "xy");
    expect(glowState()).toBe("typing");
  });

  it("after a choices card the field empties and the hint is the placeholder", async () => {
    const api = makeApi(card("q1", "choices") as AssistantCard);
    mount(api);
    const input = host.querySelector("input")!;
    type(input, "תמצא את המייל");
    await act(async () => {
      press(input, "Enter");
    });
    expect(input.value).toBe("");
    expect(input.placeholder).toBe(ss("choicesHint"));
  });

  it("ArrowUp recalls the previous question", async () => {
    const api = makeApi();
    mount(api);
    const input = host.querySelector("input")!;
    type(input, "first");
    await act(async () => {
      press(input, "Enter");
    });
    type(input, "");
    press(input, "ArrowUp");
    expect(input.value).toBe("first");
  });

  it("draws no glow layers while Disabled, and blocks the field", () => {
    mount(makeApi(), { disabled: true });
    expect(host.querySelector(".ci-glow")).toBeNull();
    expect(host.querySelector("input")!.disabled).toBe(true);
  });

  it("high contrast uses the plain ring and no gradient layers", () => {
    mount(makeApi(), { bar: { ...DEFAULT_BAR, highContrast: true } });
    expect(host.querySelector(".ci-glow--plain")).not.toBeNull();
    expect(host.querySelector(".ci-glow__sweep")).toBeNull();
  });

  it("reduced motion is reflected in data-mode and starts no rotation", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduced-motion"), addEventListener() {}, removeEventListener() {} }));
    const animate = vi.fn();
    (HTMLElement.prototype as unknown as { animate: unknown }).animate = animate;
    mount(makeApi());
    const glow = host.querySelector(".ci-glow")!;
    expect(glow.getAttribute("data-mode")).toBe("reduced");
    expect(glow.getAttribute("data-animated")).toBe("false");
    expect(animate).not.toHaveBeenCalled();
    delete (HTMLElement.prototype as unknown as { animate?: unknown }).animate;
  });
});

describe("SearchBar spotlight variant", () => {
  const api = () => ({ submit: vi.fn().mockResolvedValue(card("q1", "answer") as AssistantCard), close: vi.fn() });

  it("renders the capsule, not the taskbar bar, and keeps the same flow", async () => {
    const a = api();
    mount(a, { bar: SPOTLIGHT_BAR });
    expect(host.querySelector(".sp-root")).not.toBeNull();
    expect(host.querySelector(".sb-bar")).toBeNull();
    const input = host.querySelector("input")!;
    expect(document.activeElement).toBe(input);
    expect(input.placeholder).toBe(ss("spotlightPlaceholder"));
    expect(host.querySelector(".sp-chip")?.textContent).toContain("Alt");
    type(input, "מה יש ביומן");
    expect(host.querySelector(".sp-chip")?.getAttribute("data-kind")).toBe("enter");
    press(input, "Enter");
    await act(async () => {});
    expect(a.submit).toHaveBeenCalledWith("מה יש ביומן");
  });

  it("Esc closes; Enter during IME composition does not submit", () => {
    const a = api();
    mount(a, { bar: SPOTLIGHT_BAR });
    const input = host.querySelector("input")!;
    type(input, "x");
    press(input, "Enter", { isComposing: true });
    expect(a.submit).not.toHaveBeenCalled();
    press(input, "Escape");
    expect(a.close).toHaveBeenCalled();
  });

  it("re-lays out when the variant switches", () => {
    mount(api(), { bar: SPOTLIGHT_BAR });
    expect(host.querySelector(".sp-root")).not.toBeNull();
    mount(api(), { bar: DEFAULT_BAR });
    expect(host.querySelector(".sp-root")).toBeNull();
    expect(host.querySelector(".sb-bar")).not.toBeNull();
  });
});
