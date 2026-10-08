// @vitest-environment jsdom
// The smart-search card: every phase renders what the layout counted, the buttons call the backend
// commands, text shapes itself (RTL / bidi), and a render error stays inside the card.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AssistantCard as Card, AssistantItem, Choice } from "../../lib/assistant/types";
import { AssistantCard, assistantAnnouncement } from "./AssistantCard";
import { assistantLayout } from "./assistantLayout";

const api = vi.hoisted(() => ({
  open: vi.fn(),
  center: vi.fn(),
  extend: vi.fn(),
  choose: vi.fn(),
}));

vi.mock("../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/ipc")>();
  return {
    ...original,
    ipc: { ...original.ipc, assistantOpenItem: api.open, assistantOpenCenter: api.center, assistantExtend: api.extend, assistantChoose: api.choose },
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const card = (extra: Partial<Card> = {}): Card => ({
  queryId: "q1",
  query: "",
  phase: "answer",
  lang: "he",
  title: "מחר יש לאיציק 3 פגישות",
  summary: "09:00 · 11:30 · 14:00",
  question: null,
  choices: [],
  items: [],
  total: 0,
  partial: false,
  canExtend: false,
  errorCode: null,
  sources: [],
  createdAt: 0,
  followUp: false,
  ...extra,
});
const at = (h: number, m: number) => new Date(2026, 9, 7, h, m).getTime();
const item = (id: string, extra: Partial<AssistantItem> = {}): AssistantItem => ({
  id,
  kind: "event",
  title: `Meeting ${id}`,
  subtitle: null,
  time: at(9, 0),
  endTime: null,
  accent: "#0A84FF",
  openable: false,
  unread: false,
  source: "היומן של איציק",
  ...extra,
});
const choice = (id: string, extra: Partial<Choice> = {}): Choice => ({ id, label: `Mailbox ${id}`, kind: "mailbox", preferred: false, ...extra });

let container: HTMLDivElement;
let root: Root;
const callbacks = { onClose: vi.fn(), onHide: vi.fn(), onCard: vi.fn() };

function render(c: Card, reducedMotion = false) {
  act(() => root.render(<AssistantCard card={c} reducedMotion={reducedMotion} {...callbacks} />));
}
const layer = () => container.querySelector<HTMLElement>('[data-layer="assistant"]')!;
const button = (label: RegExp | string) =>
  [...container.querySelectorAll<HTMLButtonElement>("button")].find((b) => (typeof label === "string" ? b.textContent === label || b.getAttribute("aria-label") === label : label.test(b.textContent ?? "") || label.test(b.getAttribute("aria-label") ?? "")))!;
const click = async (el: HTMLElement) => {
  await act(async () => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
};

beforeEach(() => {
  Object.values(api).forEach((fn) => fn.mockReset());
  Object.values(callbacks).forEach((fn) => fn.mockReset());
  api.open.mockResolvedValue(true);
  api.center.mockResolvedValue(true);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("working", () => {
  it("is one line with a moving sparkle and a close button", () => {
    render(card({ phase: "processing", title: "" }));
    expect(layer().textContent).toContain("Working…");
    expect(layer().querySelector('[data-sparkle="moving"]')).not.toBeNull();
    expect(layer().querySelectorAll("button")).toHaveLength(1);
    expect(layer().style.height).toBe("44px");
  });

  it("keeps the sparkle running under reduced motion (a motion value, not CSS), only gentler", () => {
    render(card({ phase: "processing" }), true);
    expect(layer().querySelector('[data-sparkle="moving"]')).not.toBeNull();
    expect(layer().querySelector("[style*='animation']")).toBeNull();
  });

  it("never offers results or choices", () => {
    render(card({ phase: "processing", items: [item("1")], choices: [choice("a")] }));
    expect(layer().querySelectorAll("button")).toHaveLength(1);
    expect(layer().textContent).not.toContain("Meeting 1");
  });
});

describe("an answer", () => {
  it("shows the headline, the summary and the items with their times", () => {
    render(card({ items: [item("1", { time: at(9, 0) }), item("2", { time: at(11, 30), title: "סקירת תקציב" }), item("3", { time: at(14, 0) })], total: 3 }));
    const text = layer().textContent!;
    expect(text).toContain("מחר יש לאיציק 3 פגישות");
    expect(text).toContain("09:00 · 11:30 · 14:00");
    expect(layer().querySelectorAll("li")).toHaveLength(3);
    expect(text).toMatch(/9:00/);
    expect(text).toContain("סקירת תקציב");
    expect(text).toContain("היומן של איציק");
    // 3 of 3 from one source: nothing more to show.
    expect(button(/הצג את כל|Show all/)).toBeUndefined();
  });

  it("lists three items at most and sizes the layer to what the layout counted", () => {
    const c = card({ items: ["1", "2", "3", "4", "5"].map((id) => item(id)), total: 5 });
    render(c);
    expect(layer().querySelectorAll("li")).toHaveLength(3);
    expect(layer().style.height).toBe(`${assistantLayout(c).size.height}px`);
  });

  it("an item that can be opened is a button that opens exactly that item", async () => {
    render(card({ items: [item("m1", { kind: "mail", title: "תקציב 2027", openable: true }), item("m2", { title: "not openable" })], total: 2 }));
    const open = button(/תקציב 2027/);
    expect(open).toBeDefined();
    await click(open);
    expect(api.open).toHaveBeenCalledWith("q1", "m1");
    expect(layer().querySelectorAll("li button")).toHaveLength(1);
  });

  it("show all results opens the Center on this query and hides the card, without dismissing the query", async () => {
    render(card({ items: [item("1")], total: 14, sources: ["A", "B"] }));
    await click(button(/הצג את כל התוצאות|Show all results/));
    expect(api.center).toHaveBeenCalledWith("q1");
    expect(callbacks.onHide).toHaveBeenCalledTimes(1);
    expect(callbacks.onClose).not.toHaveBeenCalled();
  });

  it("search 10 s more extends, hands the new card back, and cannot be pressed twice", async () => {
    const next = card({ title: "next", partial: false });
    api.extend.mockResolvedValue(next);
    render(card({ items: [item("1")], total: 14, partial: true, canExtend: true }));
    const extend = button(/חפש עוד|Search 10 s/);
    await click(extend);
    expect(api.extend).toHaveBeenCalledWith("q1");
    expect(callbacks.onCard).toHaveBeenCalledWith(next);
    await click(extend);
    expect(api.extend).toHaveBeenCalledTimes(1);
  });

  it("a failed extend lets the person try again", async () => {
    api.extend.mockResolvedValue(null);
    render(card({ partial: true, canExtend: true }));
    await click(button(/חפש עוד|Search 10 s/));
    await click(button(/חפש עוד|Search 10 s/));
    expect(api.extend).toHaveBeenCalledTimes(2);
  });

  it("the close button closes", async () => {
    render(card());
    await click(button(/סגור|Close/));
    expect(callbacks.onClose).toHaveBeenCalledTimes(1);
  });

  it("shows no time column when no item has a time", () => {
    render(card({ items: [item("1", { time: null, kind: "file" })], total: 1 }));
    expect(layer().querySelector("li .tabular-nums")).toBeNull();
  });
});

describe("a question", () => {
  const question = () =>
    card({
      phase: "choices",
      title: "",
      question: "באיזו תיבת דואר לחפש?",
      choices: [choice("a", { label: "תיבת הדואר שלי" }), choice("b", { preferred: true }), choice("all", { kind: "allMailboxes", label: "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן." })],
    });

  it("asks the question with one button per choice, the preferred first and the way out last", () => {
    render(question());
    expect(layer().textContent).toContain("באיזו תיבת דואר לחפש?");
    const labels = [...layer().querySelectorAll("button")].map((b) => b.getAttribute("aria-label") ?? b.textContent);
    const mailboxes = labels.filter((l) => l && /Mailbox|תיבת הדואר|אני לא יודע/.test(l));
    expect(mailboxes[0]).toBe("Mailbox b");
    expect(mailboxes[mailboxes.length - 1]).toMatch(/אני לא יודע/);
    expect(layer().textContent).toContain("אני לא יודע…");
  });

  it("a click answers with that choice and the remember flag off by default", async () => {
    api.choose.mockResolvedValue(card({ phase: "processing" }));
    render(question());
    await click(button("תיבת הדואר שלי"));
    expect(api.choose).toHaveBeenCalledWith("q1", "a", false);
    expect(callbacks.onCard).toHaveBeenCalled();
  });

  it("remember my choice sends the flag, for mailboxes; every button waits once one was pressed", async () => {
    api.choose.mockResolvedValue(null);
    render(question());
    const remember = button(/זכור את הבחירה|Remember/);
    expect(remember.getAttribute("aria-pressed")).toBe("false");
    await click(remember);
    expect(remember.getAttribute("aria-pressed")).toBe("true");
    await click(button("תיבת הדואר שלי"));
    expect(api.choose).toHaveBeenCalledWith("q1", "a", true);
  });

  it("the choose that fails frees the buttons again", async () => {
    api.choose.mockResolvedValue(null);
    render(question());
    await click(button("תיבת הדואר שלי"));
    await click(button("תיבת הדואר שלי"));
    expect(api.choose).toHaveBeenCalledTimes(2);
  });

  it("a plain option never asks to be remembered", async () => {
    api.choose.mockResolvedValue(null);
    render(card({ phase: "choices", question: "Which?", choices: [choice("x", { kind: "option", label: "Yes" }), choice("y", { kind: "mailbox" })] }));
    await click(button(/זכור את הבחירה|Remember/));
    await click(button("Yes"));
    expect(api.choose).toHaveBeenCalledWith("q1", "x", false);
  });
});

describe("an error", () => {
  it("shows the headline, the text and the code", () => {
    render(card({ phase: "error", title: "לא הצלחתי לקרוא את היומן", summary: "פתח את היומן ב-Outlook", errorCode: "MAIL-101" }));
    expect(layer().textContent).toContain("לא הצלחתי לקרוא את היומן");
    expect(layer().textContent).toContain("MAIL-101");
    expect(layer().querySelectorAll("button")).toHaveLength(1);
  });
});

describe("text direction", () => {
  it("the layer follows the UI language and each text shapes itself", () => {
    render(card({ title: "Meeting with Dana", summary: "מחר בבוקר", items: [item("1", { title: "השרת 10.20.30.41 זמין", subtitle: null, source: "יומן" })], total: 1 }));
    expect(layer().getAttribute("dir")).toBe("ltr"); // the tests' locale is English; Hebrew builds get rtl (uiDirection)
    expect(layer().querySelector("h3")!.getAttribute("dir")).toBe("ltr"); // Latin text is left to right
    expect(layer().querySelector("p")!.getAttribute("dir")).toBe("rtl");
    // A technical token inside a title keeps its own order.
    expect([...layer().querySelectorAll("li bdi[dir='ltr']")].map((el) => el.textContent)).toContain("10.20.30.41");
  });
});

describe("a render error", () => {
  it("stays in the card: a fallback line with a close button, and the next state gets a fresh try", async () => {
    const bad = card({ items: [item("1")], total: 1 });
    // A time that makes Intl throw inside the item row.
    (bad.items[0] as { time: number }).time = Number.MAX_VALUE;
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(bad);
    expect(layer().textContent).toMatch(/לא ניתן להציג|can't be shown/);
    await click(button(/סגור|Close/));
    expect(callbacks.onClose).toHaveBeenCalled();
    render(card({ title: "fine", queryId: "q1", phase: "error", summary: "" }));
    expect(layer().textContent).toContain("fine");
    spy.mockRestore();
  });
});

describe("announcements", () => {
  it("say the headline, the summary and the first items", () => {
    const text = assistantAnnouncement(card({ items: [item("1", { title: "Standup", time: at(9, 0) })], total: 1 }));
    expect(text).toContain("מחר יש לאיציק 3 פגישות");
    expect(text).toContain("09:00 · 11:30 · 14:00");
    expect(text).toContain("Standup");
  });

  it("a question announces the question, working announces that it works, an error its text", () => {
    expect(assistantAnnouncement(card({ phase: "choices", question: "Which mailbox?" }))).toBe("Which mailbox?");
    expect(assistantAnnouncement(card({ phase: "processing" }))).toBe("Working…");
    expect(assistantAnnouncement(card({ phase: "error", title: "Failed", summary: "Open Outlook" }))).toBe("Failed. Open Outlook");
  });
});
