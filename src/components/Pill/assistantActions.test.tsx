// @vitest-environment jsdom
// Commands on the smart-search card ("תחפש בגוגל חתולים", "תפתח את ynet", "תנעל את המחשב"): the card
// shows what will happen and ONE openable item. Nothing runs from the text; the item is a native button,
// so a click, or Enter / Space on the focused button, is the only way to run it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { normalizeAssistantCard, type AssistantCard as Card, type AssistantItem } from "../../lib/assistant/types";
import { AssistantCard } from "./AssistantCard";
import { assistantLayout } from "./assistantLayout";

const api = vi.hoisted(() => ({ open: vi.fn(), center: vi.fn(), extend: vi.fn(), choose: vi.fn() }));

vi.mock("../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/ipc")>();
  return {
    ...original,
    ipc: { ...original.ipc, assistantOpenItem: api.open, assistantOpenCenter: api.center, assistantExtend: api.extend, assistantChoose: api.choose },
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const actionItem = (extra: Partial<AssistantItem> = {}): AssistantItem => ({
  id: "a1",
  kind: "action",
  title: "חפש בגוגל",
  subtitle: "www.google.com",
  time: null,
  endTime: null,
  accent: null,
  openable: true,
  unread: false,
  source: null,
  ...extra,
});

const card = (extra: Partial<Card> = {}): Card => ({
  queryId: "q1",
  query: "תחפש בגוגל חתולים",
  phase: "answer",
  lang: "he",
  title: "לחפש בגוגל: ״חתולים״",
  summary: "לחץ כדי לבצע",
  question: null,
  choices: [],
  items: [actionItem()],
  total: 1,
  partial: false,
  canExtend: false,
  errorCode: null,
  sources: ["action"],
  createdAt: 0,
  followUp: false,
  ...extra,
});

let container: HTMLDivElement;
let root: Root;
const callbacks = { onClose: vi.fn(), onHide: vi.fn(), onCard: vi.fn() };

function render(c: Card) {
  act(() => root.render(<AssistantCard card={c} {...callbacks} />));
}
const layer = () => container.querySelector<HTMLElement>('[data-layer="assistant"]')!;
const itemButton = () => layer().querySelector<HTMLButtonElement>("li button")!;

beforeEach(() => {
  Object.values(api).forEach((fn) => fn.mockReset());
  Object.values(callbacks).forEach((fn) => fn.mockReset());
  api.open.mockResolvedValue(true);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("the wire", () => {
  it("keeps an item of kind action with all its fields", () => {
    const raw = { ...card(), items: [actionItem()] };
    const parsed = normalizeAssistantCard(raw)!;
    expect(parsed.items).toHaveLength(1);
    expect(parsed.items[0]).toMatchObject({ id: "a1", kind: "action", title: "חפש בגוגל", subtitle: "www.google.com", openable: true });
  });

  it("still drops an item of a kind it does not know", () => {
    const parsed = normalizeAssistantCard({ ...card(), items: [{ ...actionItem(), kind: "rocket" }, actionItem({ id: "a2" })] })!;
    expect(parsed.items.map((i) => i.id)).toEqual(["a2"]);
  });
});

describe("a command card", () => {
  it("says what will happen and offers exactly one item", () => {
    render(card());
    const text = layer().textContent!;
    expect(text).toContain("לחפש בגוגל");
    expect(text).toContain("חתולים");
    expect(text).toContain("לחץ כדי לבצע");
    expect(text).toContain("www.google.com");
    expect(layer().querySelectorAll("li")).toHaveLength(1);
    // one source, one item: no "show all", no "search for 10 more seconds"
    expect(layer().textContent).not.toMatch(/הצג את כל|Show all/);
  });

  it("is sized to exactly what the layout counted", () => {
    const c = card();
    render(c);
    expect(layer().style.height).toBe(`${assistantLayout(c).size.height}px`);
    const long = card({ title: "לחפש בגוגל: ״" + "חתולים חמודים ".repeat(12) + "״" });
    render(long);
    expect(layer().style.height).toBe(`${assistantLayout(long).size.height}px`);
    expect(assistantLayout(long).size.height).toBeLessThanOrEqual(440);
  });

  it("runs nothing until the item is clicked, then asks for exactly that item once", async () => {
    render(card());
    expect(api.open).not.toHaveBeenCalled();
    await act(async () => {
      itemButton().dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(api.open).toHaveBeenCalledTimes(1);
    expect(api.open).toHaveBeenCalledWith("q1", "a1");
  });

  it("the item is a real, focusable button: Enter and Space on it work like a click", async () => {
    render(card());
    const b = itemButton();
    // A native <button> is activated by Enter (keydown) and Space (keyup) in the browser, and nothing on the
    // page may cancel those keys: this is what that needs.
    expect(b.tagName).toBe("BUTTON");
    expect(b.getAttribute("type")).toBe("button");
    expect(b.disabled).toBe(false);
    expect(b.tabIndex).toBeGreaterThanOrEqual(0);
    act(() => b.focus());
    expect(document.activeElement).toBe(b);
    for (const [type, key] of [
      ["keydown", "Enter"],
      ["keyup", "Enter"],
      ["keydown", " "],
      ["keyup", " "],
    ] as const) {
      const ev = new KeyboardEvent(type, { key, bubbles: true, cancelable: true });
      await act(async () => {
        b.dispatchEvent(ev);
      });
      expect(ev.defaultPrevented, `${type} ${JSON.stringify(key)} must reach the button's default action`).toBe(false);
    }
    // the browser turns the key into this click
    await act(async () => b.click());
    expect(api.open).toHaveBeenCalledWith("q1", "a1");
  });

  it("names the item together with what it will do, for a screen reader", () => {
    render(card());
    const label = itemButton().getAttribute("aria-label")!;
    expect(label).toContain("חפש בגוגל");
    expect(label).toContain("חתולים");
  });

  it("an English command card lays out the same way", () => {
    const c = card({
      lang: "en",
      query: "search google for cats",
      title: "Search Google for “cats”",
      summary: "Click to do it",
      items: [actionItem({ title: "Search Google" })],
    });
    render(c);
    expect(layer().textContent).toContain("Search Google for");
    expect(layer().style.height).toBe(`${assistantLayout(c).size.height}px`);
  });

  it("the offer after 'I don't know' is the same kind of item", async () => {
    const c = card({
      query: "מה מזג האוויר",
      title: "את מזג האוויר אפשר לבדוק בגוגל",
      summary: "",
      items: [actionItem({ id: "w1", title: "חפש בגוגל: ״מה מזג האוויר״" })],
    });
    render(c);
    expect(layer().textContent).toContain("את מזג האוויר אפשר לבדוק בגוגל");
    await act(async () => itemButton().click());
    expect(api.open).toHaveBeenCalledWith("q1", "w1");
  });
});
