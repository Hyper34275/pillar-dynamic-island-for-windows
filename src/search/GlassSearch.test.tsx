// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FUNNY_STATUS_MS, funnyStatusLines } from "../lib/assistant/funnyStatus";
import { ASSISTANT_UPDATE_EVENT, type AssistantCard, type AssistantItem, type Choice } from "../lib/assistant/types";
import { getLocale } from "../lib/i18n";
import { SPOTLIGHT_BAR } from "./bar";
import { BACKDROP_EVENT, GLASS_METRICS, HEIGHT_MS, estimateSheetHeight } from "./glassModel";
import { glassClasses } from "./GlassSearch";
import { SearchBar, type SearchApi } from "./SearchBar";
import { ss } from "./strings";

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

const at = (h: number, m: number) => new Date(2026, 9, 10, h, m).getTime();

function item(id: string, over: Partial<AssistantItem> = {}): AssistantItem {
  return { id, kind: "event", title: `פגישה ${id}`, subtitle: null, time: at(9, 30), endTime: at(10, 0), accent: "#0A84FF", openable: true, unread: false, source: "יומן עבודה", ...over };
}

function card(over: Partial<AssistantCard> = {}): AssistantCard {
  return {
    queryId: "q1",
    query: "מה יש לי מחר?",
    phase: "answer",
    lang: "he",
    title: "מחר יש לך 3 פגישות",
    summary: "",
    question: null,
    choices: [],
    items: [item("e1"), item("e2", { time: at(13, 0), endTime: at(14, 0) }), item("e3", { time: at(16, 30), endTime: at(17, 30) })],
    total: 3,
    partial: false,
    canExtend: false,
    errorCode: null,
    sources: ["calendar"],
    createdAt: 1,
    followUp: false,
    ...over,
  };
}

const CHOICES: Choice[] = [
  { id: "m1", label: "דואר עבודה", kind: "mailbox", preferred: true },
  { id: "m2", label: "דואר אישי", kind: "mailbox", preferred: false },
  { id: "all", label: "לא יודע — חפש בכולן", kind: "allMailboxes", preferred: false },
];

type TestApi = { [K in keyof SearchApi]-?: ReturnType<typeof vi.fn> };

function makeApi(result: AssistantCard | null = card()): TestApi & SearchApi {
  return {
    submit: vi.fn().mockResolvedValue(result),
    close: vi.fn(),
    openItem: vi.fn(),
    openCenter: vi.fn(),
    choose: vi.fn().mockResolvedValue(null),
    extend: vi.fn().mockResolvedValue(null),
    region: vi.fn().mockResolvedValue(undefined),
    backdrop: vi.fn().mockResolvedValue(null),
  } as TestApi & SearchApi;
}

function mount(api: SearchApi, props: Partial<Parameters<typeof SearchBar>[0]> = {}) {
  act(() => {
    root.render(createElement(SearchBar, { bar: SPOTLIGHT_BAR, api, subscribe, ...props }));
  });
}

const input = () => host.querySelector("input") as HTMLInputElement;

function type(value: string) {
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    set.call(input(), value);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function press(key: string, init: KeyboardEventInit = {}) {
  const ev = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  act(() => {
    input().dispatchEvent(ev);
  });
  return ev;
}

async function ask(text = "מה יש לי מחר?") {
  type(text);
  press("Enter");
  await act(async () => {});
}

const rows = () => Array.from(host.querySelectorAll(".gl-row"));
const selectedRow = () => rows().findIndex((r) => r.hasAttribute("data-sel"));
const sheet = () => host.querySelector(".gl-sheet") as HTMLElement;
const regionCalls = (api: TestApi) => api.region.mock.calls.map((c) => c[0] as number);
const last = (list: number[]) => list[list.length - 1];

function matchMedia(flags: Record<string, boolean>) {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: Object.entries(flags).some(([key, on]) => on && q.includes(key)),
    addEventListener() {},
    removeEventListener() {},
  }));
}

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
  delete (HTMLElement.prototype as unknown as { offsetHeight?: unknown }).offsetHeight;
});

describe("the glass sheet: the field", () => {
  it("is the centre bar's page: a sheet with the field, not the taskbar bar", () => {
    mount(makeApi());
    expect(host.querySelector(".gl-root")).not.toBeNull();
    expect(host.querySelector(".gl-sheet .gl-field")).not.toBeNull();
    expect(host.querySelector(".sb-bar")).toBeNull();
    expect(host.querySelector(".sp-root")).toBeNull();
    expect(document.activeElement).toBe(input());
    expect(input().placeholder).toBe(ss("spotlightPlaceholder"));
  });

  it("shows the shortcut hint when empty and the Enter hint once there is text", () => {
    mount(makeApi());
    expect(host.querySelector(".gl-hint")?.getAttribute("data-kind")).toBe("keys");
    expect(host.querySelector(".gl-hint")?.textContent).toContain("Alt");
    type("שלום");
    expect(host.querySelector(".gl-hint")?.getAttribute("data-kind")).toBe("enter");
    expect(host.querySelector(".gl-hint")?.textContent).toContain("Enter");
    type("");
    expect(host.querySelector(".gl-hint")?.getAttribute("data-kind")).toBe("keys");
  });

  it("lays out right to left in Hebrew (and the field follows the text once typed)", () => {
    mount(makeApi());
    const rtl = getLocale() === "he";
    expect(host.querySelector(".gl-root")?.getAttribute("dir")).toBe(rtl ? "rtl" : "ltr");
    expect(input().getAttribute("dir")).toBe(rtl ? "rtl" : "ltr");
    type("hello");
    expect(input().getAttribute("dir")).toBe("auto");
  });

  it("Esc closes; Enter during an IME composition does not submit", () => {
    const api = makeApi();
    mount(api);
    type("x");
    press("Enter", { isComposing: true });
    expect(api.submit).not.toHaveBeenCalled();
    press("Escape");
    expect(api.close).toHaveBeenCalled();
  });

  it("is a combobox over its list, for a screen reader", () => {
    mount(makeApi());
    expect(input().getAttribute("role")).toBe("combobox");
    expect(input().getAttribute("aria-expanded")).toBe("false");
  });
});

describe("the glass sheet: working", () => {
  it("shows a playful line at once, and a screen reader hears the plain word", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    api.submit.mockReturnValue(new Promise(() => {}));
    mount(api);
    await ask();
    const line = host.querySelector("[data-funny-status]") as HTMLElement;
    expect(line).not.toBeNull();
    expect(funnyStatusLines()).toContain(line.textContent);
    // the joke is decoration: hidden from assistive technology, which hears "searching"
    expect(line.closest(".gl-status")?.getAttribute("aria-hidden")).toBe("true");
    expect(host.querySelector('[role="status"]')?.textContent).toBe(ss("processing"));
    expect(host.querySelector(".gl-shimmer")).not.toBeNull();
    expect(host.querySelector(".gl-hint")).toBeNull();
  });

  it("rotates the line every 2.2 s without a pause, and keeps the same order for the whole search", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    api.submit.mockReturnValue(new Promise(() => {}));
    mount(api);
    await ask();
    const lineNow = () => host.querySelector("[data-funny-status]")!.textContent;
    const first = lineNow();
    act(() => {
      vi.advanceTimersByTime(FUNNY_STATUS_MS + 20);
    });
    const second = lineNow();
    expect(second).not.toBe(first);
    // the processing card arriving (same query) does not reshuffle: the next line follows the same order
    act(() => handlers[ASSISTANT_UPDATE_EVENT]?.(card({ phase: "processing", title: "", items: [] })));
    expect(lineNow()).toBe(second);
    act(() => {
      vi.advanceTimersByTime(FUNNY_STATUS_MS + 20);
    });
    expect(lineNow()).not.toBe(second);
  });

  it("stops rotating when the window is hidden", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    api.submit.mockReturnValue(new Promise(() => {}));
    mount(api);
    await ask();
    const hide = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    // hidden: the search is forgotten, the sheet is the empty field again
    expect(host.querySelector("[data-funny-status]")).toBeNull();
    hide.mockRestore();
  });
});

describe("the glass sheet: the answer lives next to the question", () => {
  it("shows the headline, the summary and the rows with time, title, source and calendar colour", async () => {
    mount(makeApi(card({ summary: "שתיים מהן בזום" })));
    await ask();
    expect(host.querySelector(".gl-ans h3")?.textContent).toBe("מחר יש לך 3 פגישות");
    expect(host.querySelector(".gl-sub")?.textContent).toBe("שתיים מהן בזום");
    expect(rows()).toHaveLength(3);
    const first = rows()[0];
    expect(first.querySelector(".gl-t")?.textContent).toBe("פגישה e1");
    expect(first.querySelector(".gl-s")?.textContent).toBe("יומן עבודה");
    expect(first.querySelector(".gl-time")?.textContent).toMatch(/9:30|09:30/);
    expect(first.querySelector(".gl-time")?.textContent).toContain("–");
    expect(first.querySelector(".gl-time")?.getAttribute("dir")).toBe("ltr");
    expect((first.querySelector(".gl-tile") as HTMLElement).style.getPropertyValue("--c")).toBe("#0A84FF");
    // the question stays in the field, and the sheet offers the Center and Esc
    expect(input().value).toBe("מה יש לי מחר?");
    expect(host.querySelector(".gl-go")?.textContent).toBe(ss("showAll"));
    expect(host.querySelector(".gl-esc")?.textContent).toBe("Esc");
  });

  it("lists at most five rows", async () => {
    const many = Array.from({ length: 9 }, (_, i) => item(`e${i}`));
    mount(makeApi(card({ items: many, total: 9 })));
    await ask();
    expect(rows()).toHaveLength(5);
  });

  it("an answer that arrives as an event is shown too (and a card of another query is not)", async () => {
    const api = makeApi();
    api.submit.mockReturnValue(new Promise(() => {}));
    mount(api);
    await ask();
    // the first card after a submit is the question's own (the search state adopts its id) ...
    act(() => handlers[ASSISTANT_UPDATE_EVENT]?.(card({ phase: "processing", title: "", items: [] })));
    // ... after that another query's card is not shown
    act(() => handlers[ASSISTANT_UPDATE_EVENT]?.(card({ queryId: "other", title: "של מישהו אחר" })));
    expect(host.querySelector(".gl-ans")).toBeNull();
    expect(host.querySelector("[data-funny-status]")).not.toBeNull();
    act(() => handlers[ASSISTANT_UPDATE_EVENT]?.(card()));
    expect(host.querySelector(".gl-ans h3")?.textContent).toBe("מחר יש לך 3 פגישות");
    expect(rows()).toHaveLength(3);
  });

  it("an answer without rows is just its headline (no footer to open)", async () => {
    mount(makeApi(card({ title: "לא מצאתי כלום", items: [], total: 0 })));
    await ask();
    expect(host.querySelector(".gl-ans h3")?.textContent).toBe("לא מצאתי כלום");
    expect(rows()).toHaveLength(0);
    expect(host.querySelector(".gl-foot")).toBeNull();
  });

  it("an answer that can go on offers 'search 10 s more'", async () => {
    const api = makeApi(card({ canExtend: true, items: [], title: "חלקי", partial: true }));
    mount(api);
    await ask();
    const more = Array.from(host.querySelectorAll(".gl-go")).find((b) => b.textContent === ss("extend")) as HTMLButtonElement;
    expect(more).toBeDefined();
    act(() => more.click());
    expect(api.extend).toHaveBeenCalledWith("q1");
  });

  it("shows the error text of an error card, with its code", async () => {
    mount(makeApi(card({ phase: "error", title: "לא הצלחתי לקרוא את היומן", summary: "Outlook לא ענה", errorCode: "OUTLOOK-101", items: [] })));
    await ask();
    expect(host.querySelector(".gl-ans[data-error] h3")?.textContent).toBe("לא הצלחתי לקרוא את היומן");
    expect(host.querySelector(".gl-sub")?.textContent).toBe("Outlook לא ענה");
    expect(host.querySelector(".gl-code")?.textContent).toBe("OUTLOOK-101");
    expect(host.querySelector('[role="status"]')?.textContent).toContain("לא הצלחתי לקרוא את היומן");
  });

  it("a command that fails shows the plain failure text", async () => {
    const api = makeApi();
    api.submit.mockRejectedValue(new Error("boom"));
    mount(api);
    await ask();
    expect(host.querySelector(".gl-ans[data-error] h3")?.textContent).toBe(ss("failed"));
  });

  it("typing a new question replaces the answer; Enter then asks it", async () => {
    const api = makeApi();
    mount(api);
    await ask();
    expect(rows()).toHaveLength(3);
    type("ומחרתיים?");
    expect(rows()).toHaveLength(0);
    expect(host.querySelector(".gl-ans")).toBeNull();
    expect(host.querySelector(".gl-hint")?.getAttribute("data-kind")).toBe("enter");
    press("Enter");
    await act(async () => {});
    expect(api.submit).toHaveBeenLastCalledWith("ומחרתיים?");
    expect(api.openItem).not.toHaveBeenCalled();
  });

  it("closing the window forgets the answer", async () => {
    mount(makeApi());
    await ask();
    expect(rows()).toHaveLength(3);
    const hide = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(rows()).toHaveLength(0);
    expect(input().value).toBe("");
    hide.mockRestore();
  });
});

describe("the glass sheet: rows and keys", () => {
  it("the first row is selected; ArrowDown / ArrowUp move it and wrap; Enter opens the selected row", async () => {
    const api = makeApi();
    mount(api);
    await ask();
    expect(selectedRow()).toBe(0);
    expect(input().getAttribute("aria-activedescendant")).toBe("gl-opt-0");
    expect(input().getAttribute("aria-expanded")).toBe("true");
    expect(press("ArrowDown").defaultPrevented).toBe(true);
    expect(selectedRow()).toBe(1);
    press("ArrowDown");
    press("ArrowDown");
    expect(selectedRow()).toBe(0); // wrapped
    press("ArrowUp");
    expect(selectedRow()).toBe(2);
    press("Enter");
    expect(api.openItem).toHaveBeenCalledTimes(1);
    expect(api.openItem).toHaveBeenCalledWith("q1", "e3");
    expect(api.submit).toHaveBeenCalledTimes(1); // Enter did not ask again
  });

  it("a click on a row opens it", async () => {
    const api = makeApi();
    mount(api);
    await ask();
    act(() => (rows()[1] as HTMLElement).click());
    expect(api.openItem).toHaveBeenCalledWith("q1", "e2");
  });

  it("rows that cannot be opened are not selectable and not clickable", async () => {
    const api = makeApi(card({ items: [item("i1", { kind: "info", openable: false }), item("e2"), item("i3", { kind: "calc", openable: false })] }));
    mount(api);
    await ask();
    expect(selectedRow()).toBe(1); // the first openable row
    press("ArrowDown");
    expect(selectedRow()).toBe(1); // the only one: nowhere else to go
    act(() => (rows()[0] as HTMLElement).click());
    expect(api.openItem).not.toHaveBeenCalled();
    expect(rows()[0].getAttribute("aria-disabled")).toBe("true");
    press("Enter");
    expect(api.openItem).toHaveBeenCalledWith("q1", "e2");
  });

  describe("an item that acts (a command: lock the PC, a web search, a new mail)", () => {
    const command = () => makeApi(card({ title: "נעל את המחשב", items: [item("a1", { kind: "action", title: "נעל את המחשב", time: null, endTime: null, accent: null })] }));
    const moveOver = (el: Element, movement: number) => {
      const ev = new MouseEvent("mousemove", { bubbles: true });
      Object.defineProperty(ev, "movementX", { value: movement });
      act(() => {
        el.dispatchEvent(ev);
      });
    };

    it("never runs by arriving, nor by the Enter that was meant for the question (a second press, a held key)", async () => {
      const api = command();
      mount(api);
      await ask();
      expect(api.openItem).not.toHaveBeenCalled();
      expect(selectedRow()).toBe(0); // it is the default selection, shown as in the mock ...
      const second = press("Enter");
      expect(api.openItem).not.toHaveBeenCalled(); // ... but a second Enter is not a choice
      expect(second.defaultPrevented).toBe(true);
      expect(api.submit).toHaveBeenCalledTimes(1); // and it does not ask the question again either
      press("Enter", { repeat: true });
      expect(api.openItem).not.toHaveBeenCalled();
    });

    it("runs by a click on its row", async () => {
      const api = command();
      mount(api);
      await ask();
      act(() => (rows()[0] as HTMLElement).click());
      expect(api.openItem).toHaveBeenCalledWith("q1", "a1");
    });

    it("runs by Enter once the person has chosen it with an arrow key", async () => {
      const api = command();
      mount(api);
      await ask();
      press("ArrowDown"); // the only row: the selection stays, but it is now a choice
      press("Enter");
      expect(api.openItem).toHaveBeenCalledTimes(1);
      expect(api.openItem).toHaveBeenCalledWith("q1", "a1");
    });

    it("runs by Enter after the pointer moved over it, not after a layout-driven mousemove with no movement", async () => {
      const api = command();
      mount(api);
      await ask();
      moveOver(rows()[0], 0); // the row appeared under a pointer at rest
      press("Enter");
      expect(api.openItem).not.toHaveBeenCalled();
      moveOver(rows()[0], 4);
      press("Enter");
      expect(api.openItem).toHaveBeenCalledWith("q1", "a1");
    });

    it("a held Enter acts once at most, even on a chosen row", async () => {
      const api = command();
      mount(api);
      await ask();
      press("ArrowDown");
      press("Enter");
      press("Enter", { repeat: true });
      press("Enter", { repeat: true });
      expect(api.openItem).toHaveBeenCalledTimes(1);
    });

    it("a row that only opens something still takes the quick Enter", async () => {
      const api = makeApi();
      mount(api);
      await ask();
      press("Enter");
      expect(api.openItem).toHaveBeenCalledWith("q1", "e1");
    });
  });

  it("on a window too short for the rows the list scrolls to keep the selected row in view", async () => {
    const api = makeApi(card({ items: Array.from({ length: 5 }, (_, i) => item(`e${i}`)) }));
    mount(api, { bar: { ...SPOTLIGHT_BAR, height: 24 + 300 + 64 } });
    await ask();
    const list = host.querySelector("#gl-list") as HTMLElement;
    let scrollTop = 0;
    Object.defineProperty(list, "scrollTop", { configurable: true, get: () => scrollTop, set: (v: number) => (scrollTop = v) });
    Object.defineProperty(list, "scrollHeight", { configurable: true, value: 330 });
    Object.defineProperty(list, "clientHeight", { configurable: true, value: 200 });
    // the list shows y 100..300; rows are 62 px high from y 100 (minus what is scrolled away)
    list.getBoundingClientRect = () => ({ top: 100, bottom: 300 }) as DOMRect;
    rows().forEach((row, i) => {
      (row as HTMLElement).getBoundingClientRect = () => ({ top: 100 + i * 64 - scrollTop, bottom: 162 + i * 64 - scrollTop }) as DOMRect;
    });
    press("ArrowDown"); // row 1 (y 164..226): in view
    expect(scrollTop).toBe(0);
    press("ArrowDown");
    press("ArrowDown"); // row 3 (y 292..354): its bottom is below the list
    expect(scrollTop).toBe(54);
    press("ArrowUp");
    press("ArrowUp");
    press("ArrowUp"); // back to row 0: scrolled up to it
    expect(scrollTop).toBe(0);
  });

  it("ArrowUp recalls the previous question when there is no list under the field", async () => {
    const api = makeApi();
    mount(api);
    await ask("שאלה ראשונה");
    type("");
    expect(host.querySelector(".gl-rows")).toBeNull();
    press("ArrowUp");
    expect(input().value).toBe("שאלה ראשונה");
  });

  it("'show all results' opens the Center for the query and closes the sheet", async () => {
    const api = makeApi();
    mount(api);
    await ask();
    act(() => (host.querySelector(".gl-go") as HTMLButtonElement).click());
    expect(api.openCenter).toHaveBeenCalledWith("q1");
    expect(api.close).toHaveBeenCalled();
  });

  it("a click on the transparent margin around the sheet is a click outside: it closes, never leaves the field unfocused", async () => {
    const api = makeApi();
    mount(api);
    await ask();
    const ev = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    act(() => {
      host.querySelector(".gl-root")!.dispatchEvent(ev);
    });
    expect(ev.defaultPrevented).toBe(true); // no focus change: the field keeps the keyboard
    expect(api.close).toHaveBeenCalledTimes(1);
    // a click on the sheet itself does not close it
    act(() => {
      host.querySelector(".gl-ans")!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
    });
    expect(api.close).toHaveBeenCalledTimes(1);
  });

  it("a click on the sheet outside the field keeps the focus in the field", async () => {
    mount(makeApi());
    await ask();
    input().blur();
    const ev = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    act(() => {
      host.querySelector(".gl-ans")!.dispatchEvent(ev);
    });
    expect(ev.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(input());
  });
});

describe("the glass sheet: a question with buttons", () => {
  const ask1 = () => card({ phase: "choices", title: "באיזו תיבת דואר לחפש?", question: "באיזו תיבת דואר לחפש?", choices: CHOICES, items: [], total: 0 });
  const buttons = () => Array.from(host.querySelectorAll(".gl-choice")) as HTMLButtonElement[];

  it("shows the question, the preferred button first and the way out last, and empties the field", async () => {
    mount(makeApi(ask1()));
    await ask("מייל מדנה");
    expect(host.querySelector(".gl-ans h3")?.textContent).toBe("באיזו תיבת דואר לחפש?");
    expect(buttons().map((b) => b.textContent)).toEqual(["דואר עבודה", "דואר אישי", "לא יודע…"]);
    expect(buttons()[0].hasAttribute("data-preferred")).toBe(true);
    expect(buttons()[2].title).toBe("לא יודע — חפש בכולן");
    expect(input().value).toBe("");
    expect(input().placeholder).toBe(ss("glassChoicesHint"));
  });

  it("a click calls assistant_choose with the query and the option; it does not remember by default", async () => {
    const api = makeApi(ask1());
    mount(api);
    await ask("מייל מדנה");
    act(() => buttons()[1].click());
    expect(api.choose).toHaveBeenCalledWith("q1", "m2", false);
    expect(buttons().every((b) => b.disabled)).toBe(true); // waiting for the answer
    await act(async () => {});
    expect(buttons().every((b) => !b.disabled)).toBe(true); // it came back empty: live again
  });

  it("'remember my choice' is passed for a mailbox, never for a plain option", async () => {
    const api = makeApi(ask1());
    mount(api);
    await ask("מייל מדנה");
    act(() => (host.querySelector(".gl-remember") as HTMLButtonElement).click());
    expect(host.querySelector(".gl-remember")?.getAttribute("aria-checked")).toBe("true");
    act(() => buttons()[0].click());
    expect(api.choose).toHaveBeenCalledWith("q1", "m1", true);
  });

  it("an answer that comes back from the choice replaces the question", async () => {
    const api = makeApi(ask1());
    api.choose.mockResolvedValue(card({ title: "נמצאו 2 מיילים", items: [item("m1", { kind: "mail", time: at(8, 0), endTime: null })] }));
    mount(api);
    await ask("מייל מדנה");
    act(() => buttons()[0].click());
    await act(async () => {});
    expect(host.querySelector(".gl-ans h3")?.textContent).toBe("נמצאו 2 מיילים");
    expect(host.querySelector(".gl-choice")).toBeNull();
    expect(rows()).toHaveLength(1);
  });

  it("the arrows walk the buttons and Enter picks one while the field is empty; typed text is a reply", async () => {
    const api = makeApi(ask1());
    mount(api);
    await ask("מייל מדנה");
    expect(buttons()[0].hasAttribute("data-sel")).toBe(true);
    press("ArrowDown");
    expect(buttons()[1].hasAttribute("data-sel")).toBe(true);
    press("Enter");
    expect(api.choose).toHaveBeenCalledWith("q1", "m2", false);
    expect(api.submit).toHaveBeenCalledTimes(1);

    // a reply typed instead of a click goes to submit
    api.choose.mockClear();
    api.submit.mockClear();
    const api2 = makeApi(ask1());
    act(() => root.unmount());
    root = createRoot(host);
    mount(api2);
    await ask("מייל מדנה");
    type("דואר עבודה");
    expect(host.querySelector(".gl-choice")).not.toBeNull(); // the buttons stay while a reply is typed
    press("Enter");
    await act(async () => {});
    expect(api2.submit).toHaveBeenLastCalledWith("דואר עבודה");
    expect(api2.choose).not.toHaveBeenCalled();
  });
});

describe("the glass sheet: height and the click-through region", () => {
  it("reports the field's height first, then each growth of the sheet before it grows", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    mount(api);
    await act(async () => {});
    expect(regionCalls(api)).toEqual([74]);
    expect(sheet().style.height).toBe("74px");

    await ask();
    // 2 + 72 + 1 + 48 + 204 + 48
    const expected = estimateSheetHeight({ kind: "answer", card: card() });
    expect(expected).toBe(375);
    // ready -> working (2 + 72 + 1 + 60) -> answer: each growth is announced to the window first
    expect(regionCalls(api)).toEqual([74, 135, expected]);
    await act(async () => {});
    expect(sheet().style.height).toBe(`${expected}px`);
  });

  it("the region is told before the sheet grows (the sheet waits for the call)", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    let release: () => void = () => {};
    mount(api);
    await act(async () => {});
    api.region.mockImplementation(() => new Promise<void>((resolve) => (release = resolve)));
    await ask();
    expect(last(regionCalls(api))).toBe(375);
    expect(sheet().style.height).toBe("74px"); // still waiting for the window
    await act(async () => {
      release();
    });
    expect(sheet().style.height).toBe("375px");
  });

  it("a shrink lets the sheet go first and tells the window when it has finished", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    mount(api);
    await ask();
    await act(async () => {});
    api.region.mockClear();
    type("שאלה חדשה"); // the answer gives way to the field
    expect(sheet().style.height).toBe("74px"); // the sheet shrinks at once ...
    expect(regionCalls(api)).toEqual([]); // ... the window is not told yet
    act(() => {
      vi.advanceTimersByTime(HEIGHT_MS + GLASS_METRICS.frame * 20);
    });
    await act(async () => {});
    expect(regionCalls(api)).toEqual([74]);
  });

  it("a growth cancelled before the window answered does not leave its taller region behind", async () => {
    vi.useFakeTimers();
    const api = makeApi();
    mount(api);
    await act(async () => {});
    api.region.mockImplementation(() => new Promise<void>(() => {})); // the window has not answered yet
    await ask(); // the answer wants 375: the window was told, the sheet waits at 74
    expect(last(regionCalls(api))).toBe(375);
    expect(sheet().style.height).toBe("74px");
    type("שאלה חדשה"); // the answer gives way to the field before the sheet ever grew
    expect(sheet().style.height).toBe("74px");
    act(() => {
      vi.advanceTimersByTime(HEIGHT_MS + 100);
    });
    await act(async () => {});
    expect(last(regionCalls(api))).toBe(74); // the window is told the sheet is still just the field
  });

  it("measures the real layout where it can (a measured height beats the estimate)", async () => {
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get() {
        return (this as HTMLElement).classList.contains("gl-content") ? 120 : 0;
      },
    });
    const api = makeApi();
    mount(api);
    await act(async () => {});
    // 120 of content + the 1 px frame on each side
    expect(sheet().style.height).toBe("122px");
    expect(last(regionCalls(api))).toBe(122);
  });

  it("never asks for a sheet taller than the window holds", async () => {
    const api = makeApi(card({ items: Array.from({ length: 5 }, (_, i) => item(`e${i}`)) }));
    // a short window: 24 above, 64 below, 300 for the sheet
    mount(api, { bar: { ...SPOTLIGHT_BAR, height: 24 + 300 + 64 } });
    await ask();
    await act(async () => {});
    expect(Math.max(...regionCalls(api))).toBe(300);
    expect(sheet().style.height).toBe("300px");
  });

  it("says where it is again whenever the window is shown (the backend starts from the field alone)", async () => {
    const api = makeApi();
    mount(api);
    await act(async () => {});
    const before = regionCalls(api).length;
    const hide = vi.spyOn(document, "hidden", "get");
    hide.mockReturnValue(true);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    hide.mockReturnValue(false);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => {});
    expect(regionCalls(api).length).toBeGreaterThan(before);
    expect(last(regionCalls(api))).toBe(74);
    hide.mockRestore();
  });

  it("without a backend the page still works (no region command)", async () => {
    const api = makeApi();
    delete (api as Partial<SearchApi>).region;
    mount(api);
    await ask();
    await act(async () => {});
    expect(sheet().style.height).toBe("375px");
  });
});

describe("the glass sheet: the backdrop and the fallbacks", () => {
  const BMP = "data:image/bmp;base64,Qk0AAAA=";
  const backdrop = (over: Record<string, unknown> = {}) => ({ id: 1, image: BMP, luminance: 0.3, dark: true, transparency: true, ...over });
  const root$ = () => host.querySelector(".gl-root") as HTMLElement;

  it("paints the pushed picture behind the sheet and fades it in once it has loaded", async () => {
    mount(makeApi());
    expect(host.querySelector(".gl-bg")).toBeNull();
    expect(root$().classList.contains("gl-opaque")).toBe(true); // nothing to show through yet
    act(() => handlers[BACKDROP_EVENT]?.(backdrop()));
    const bg = host.querySelector(".gl-bg") as HTMLElement;
    expect(bg).not.toBeNull();
    expect(bg.querySelector("img")?.getAttribute("src")).toBe(BMP);
    expect(bg.getAttribute("data-ready")).toBe("off");
    expect(root$().classList.contains("gl-opaque")).toBe(false);
    act(() => {
      bg.querySelector("img")!.dispatchEvent(new Event("load"));
    });
    expect(bg.getAttribute("data-ready")).toBe("on");
  });

  it("pulls the picture when the page missed the event, and keeps the newest", async () => {
    const api = makeApi();
    api.backdrop.mockResolvedValue(backdrop({ id: 5 }));
    mount(api);
    await act(async () => {});
    expect(host.querySelector(".gl-bg img")).not.toBeNull();
    // an older picture arriving late does not replace the newer
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ id: 3, image: "data:image/bmp;base64,QUFB" })));
    expect(host.querySelector(".gl-bg img")?.getAttribute("src")).toBe(BMP);
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ id: 6, image: "data:image/bmp;base64,QkJC" })));
    expect(host.querySelector(".gl-bg img")?.getAttribute("src")).toBe("data:image/bmp;base64,QkJC");
  });

  it("drops the picture with the window (memory only)", async () => {
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop()));
    expect(host.querySelector(".gl-bg")).not.toBeNull();
    const hide = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(host.querySelector(".gl-bg")).toBeNull();
    hide.mockRestore();
  });

  it("draws only a data URL image, whatever the backend sends", () => {
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ image: "https://evil.example/x.png" })));
    expect(host.querySelector(".gl-bg")).toBeNull();
  });

  it("follows the Windows theme of the backdrop; the system scheme until there is one", () => {
    matchMedia({ "prefers-color-scheme: dark": true });
    mount(makeApi());
    expect(root$().classList.contains("gl-dark")).toBe(true);
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ dark: false })));
    expect(root$().classList.contains("gl-light")).toBe(true);
    expect(root$().classList.contains("gl-dark")).toBe(false);
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ id: 2, dark: true })));
    expect(root$().classList.contains("gl-dark")).toBe(true);
  });

  it("the tint follows the theme and the brightness of what is behind", () => {
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ dark: true, luminance: 0.1 })));
    const dark = Number(root$().style.getPropertyValue("--gl-a"));
    expect(dark).toBeCloseTo(0.46, 2);
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ id: 2, dark: true, luminance: 0.95 })));
    expect(Number(root$().style.getPropertyValue("--gl-a"))).toBeGreaterThan(dark + 0.1);
  });

  it("transparency effects off: a near-opaque tint and no picture, even if one was captured", () => {
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ transparency: false })));
    expect(host.querySelector(".gl-bg")).toBeNull();
    expect(root$().classList.contains("gl-opaque")).toBe(true);
    expect(Number(root$().style.getPropertyValue("--gl-a"))).toBeGreaterThanOrEqual(0.95);
  });

  it("a capture that failed (no image) is the same near-opaque tint", () => {
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop({ image: null, luminance: null })));
    expect(host.querySelector(".gl-bg")).toBeNull();
    expect(root$().classList.contains("gl-opaque")).toBe(true);
  });

  it("forced colours: the plain flag, system colours, no picture", () => {
    matchMedia({ "forced-colors": true });
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop()));
    expect(root$().classList.contains("gl-plain")).toBe(true);
    expect(host.querySelector(".gl-bg")).toBeNull();
    expect(root$().classList.contains("gl-opaque")).toBe(false); // plain is not the translucent fallback
  });

  it("high contrast (the backend's flag) is the plain mode too", () => {
    mount(makeApi(), { bar: { ...SPOTLIGHT_BAR, highContrast: true } });
    act(() => handlers[BACKDROP_EVENT]?.(backdrop()));
    expect(root$().classList.contains("gl-plain")).toBe(true);
    expect(host.querySelector(".gl-bg")).toBeNull();
  });

  it("reduced motion: the reduced flag (no scale-in, no growth, no turning parts)", () => {
    matchMedia({ "prefers-reduced-motion": true });
    mount(makeApi());
    expect(root$().classList.contains("gl-reduced")).toBe(true);
    expect(root$().classList.contains("gl-plain")).toBe(false);
  });

  it("full motion carries neither fallback flag", () => {
    mount(makeApi());
    expect(root$().classList.contains("gl-reduced")).toBe(false);
    expect(root$().classList.contains("gl-plain")).toBe(false);
  });

  it("reduced motion reports the shrunk height at once (nothing to wait for)", async () => {
    vi.useFakeTimers();
    matchMedia({ "prefers-reduced-motion": true });
    const api = makeApi();
    mount(api);
    await ask();
    await act(async () => {});
    api.region.mockClear();
    type("x");
    act(() => {
      vi.advanceTimersByTime(1);
    });
    await act(async () => {});
    expect(regionCalls(api)).toEqual([74]);
  });

  it("the class flags are what glass.css switches on", () => {
    expect(glassClasses({ theme: "dark", plain: false, reducedMotion: false, opaque: false })).toBe("gl-root gl-dark");
    expect(glassClasses({ theme: "light", plain: true, reducedMotion: true, opaque: true })).toBe("gl-root gl-light gl-plain gl-reduced gl-opaque");
  });

  it("geometry: the sheet sits in the fixed window, the picture is drawn at window coordinates", () => {
    mount(makeApi());
    act(() => handlers[BACKDROP_EVENT]?.(backdrop()));
    const style = root$().style;
    expect(style.getPropertyValue("--gl-w")).toBe("680px");
    expect(style.getPropertyValue("--gl-x")).toBe("40px");
    expect(style.getPropertyValue("--gl-top")).toBe("24px");
    expect(style.getPropertyValue("--gl-rw")).toBe("760px");
    expect(style.getPropertyValue("--gl-rh")).toBe("640px");
    expect(style.getPropertyValue("--gl-radius")).toBe("16px");
  });

  it("a narrower window narrows the sheet and keeps it centred", () => {
    mount(makeApi(), { bar: { ...SPOTLIGHT_BAR, width: 500 } });
    const style = root$().style;
    expect(style.getPropertyValue("--gl-w")).toBe("420px");
    expect(style.getPropertyValue("--gl-x")).toBe("40px");
  });
});
