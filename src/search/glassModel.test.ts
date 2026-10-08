import { describe, expect, it } from "vitest";
import type { AssistantCard, AssistantItem, Choice } from "../lib/assistant/types";
import {
  GLASS_MAX_CHOICES,
  GLASS_MAX_ROWS,
  GLASS_METRICS,
  OPAQUE_ALPHA,
  answerRows,
  choiceText,
  defaultSelection,
  enterActsOnSelection,
  estimateSheetHeight,
  glassEntries,
  glassHeadline,
  glassOptionId,
  glassSummary,
  glassView,
  heightPlan,
  maxSheetHeight,
  moveSelection,
  normalizeBackdrop,
  orderedChoices,
  rowMeta,
  rowTime,
  showsAllResults,
  showsRemember,
  tintAlpha,
  type GlassView,
} from "./glassModel";

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
    items: [],
    total: 0,
    partial: false,
    canExtend: false,
    errorCode: null,
    sources: [],
    createdAt: 1,
    followUp: false,
    ...over,
  };
}

function item(id: string, over: Partial<AssistantItem> = {}): AssistantItem {
  return { id, kind: "event", title: `פגישה ${id}`, subtitle: null, time: null, endTime: null, accent: "#0A84FF", openable: true, unread: false, source: "יומן עבודה", ...over };
}

const items = (n: number) => Array.from({ length: n }, (_, i) => item(`e${i + 1}`));

describe("normalizeBackdrop", () => {
  const url = "data:image/bmp;base64,Qk0AAAA=";

  it("keeps a base64 BMP or PNG data URL and the Windows flags", () => {
    expect(normalizeBackdrop({ id: 4, image: url, luminance: 0.4, dark: true, transparency: true })).toEqual({ id: 4, image: url, luminance: 0.4, dark: true, transparency: true });
    expect(normalizeBackdrop({ id: 1, image: "data:image/png;base64,iVBORw0KGgo=", luminance: 0.1, dark: false, transparency: true })?.image).toMatch(/^data:image\/png/);
  });

  it("never turns anything but a base64 image data URL into an image (no remote or script URLs)", () => {
    for (const bad of ["https://evil.example/x.png", "javascript:alert(1)", "data:text/html;base64,PHNjcmlwdD4=", "data:image/svg+xml;base64,PHN2Zz4=", "file:///C:/x.png", "data:image/bmp;base64,<script>", ""]) {
      expect(normalizeBackdrop({ id: 1, image: bad, luminance: 0.5, dark: false, transparency: true })?.image, bad).toBeNull();
    }
    expect(normalizeBackdrop({ id: 1, image: "data:image/bmp;base64," + "A".repeat(4_000_001), luminance: 0.5, dark: false, transparency: true })?.image).toBeNull();
  });

  it("clamps the luminance, drops it without a picture, and defaults transparency on", () => {
    expect(normalizeBackdrop({ id: 1, image: url, luminance: 7 })?.luminance).toBe(1);
    expect(normalizeBackdrop({ id: 1, image: url, luminance: -1 })?.luminance).toBe(0);
    expect(normalizeBackdrop({ id: 1, image: null, luminance: 0.5 })?.luminance).toBeNull();
    expect(normalizeBackdrop({ id: 1, image: url, luminance: "x" })?.luminance).toBeNull();
    expect(normalizeBackdrop({ id: 1 })).toMatchObject({ image: null, dark: false, transparency: true });
    expect(normalizeBackdrop({ id: 1, transparency: false })?.transparency).toBe(false);
  });

  it("is null for what is not an object, and a missing id is 0", () => {
    for (const bad of [null, undefined, "x", 3, [], true]) expect(normalizeBackdrop(bad)).toBeNull();
    expect(normalizeBackdrop({})?.id).toBe(0);
    expect(normalizeBackdrop({ id: NaN })?.id).toBe(0);
  });
});

describe("tintAlpha", () => {
  it("is the mock's tint over a backdrop of the same family", () => {
    expect(tintAlpha("dark", 0.2, false)).toBeCloseTo(0.46, 5);
    expect(tintAlpha("light", 0.8, false)).toBeCloseTo(0.66, 5);
    expect(tintAlpha("dark", null, false)).toBeCloseTo(0.46, 5);
    expect(tintAlpha("light", null, false)).toBeCloseTo(0.66, 5);
  });

  it("rises over the opposite backdrop, bounded, so the text keeps its contrast", () => {
    expect(tintAlpha("dark", 0.9, false)).toBeGreaterThan(0.6);
    expect(tintAlpha("dark", 1, false)).toBeLessThanOrEqual(0.72);
    expect(tintAlpha("light", 0.05, false)).toBeGreaterThan(0.8);
    expect(tintAlpha("light", 0, false)).toBeLessThanOrEqual(0.88);
    // monotonic with the brightness (dark theme) and the darkness (light theme)
    expect(tintAlpha("dark", 0.8, false)).toBeGreaterThan(tintAlpha("dark", 0.5, false));
    expect(tintAlpha("light", 0.1, false)).toBeGreaterThan(tintAlpha("light", 0.4, false));
  });

  it("is near opaque without a picture, and a bad luminance is clamped", () => {
    expect(tintAlpha("dark", 0.2, true)).toBe(OPAQUE_ALPHA);
    expect(tintAlpha("light", null, true)).toBe(OPAQUE_ALPHA);
    expect(OPAQUE_ALPHA).toBeGreaterThanOrEqual(0.95);
    expect(tintAlpha("dark", 50, false)).toBe(tintAlpha("dark", 1, false));
    expect(tintAlpha("light", -3, false)).toBe(tintAlpha("light", 0, false));
  });
});

describe("glassView", () => {
  const base = { glow: "activated" as const, queryId: null, card: null, hasText: false, stale: false };

  it("is the field alone until a question was asked: ready when empty, typing with text", () => {
    expect(glassView(base)).toEqual({ kind: "ready" });
    expect(glassView({ ...base, glow: "typing", hasText: true })).toEqual({ kind: "typing" });
    expect(glassView({ ...base, glow: "idle" })).toEqual({ kind: "ready" });
    expect(glassView({ ...base, glow: "disabled", hasText: true })).toEqual({ kind: "ready" });
  });

  it("works from the moment of the submit, before any card exists", () => {
    expect(glassView({ ...base, glow: "submitting", hasText: true })).toEqual({ kind: "processing", card: null });
    const working = card({ phase: "processing", title: "" });
    expect(glassView({ ...base, glow: "processing", queryId: "q1", card: working, hasText: true })).toEqual({ kind: "processing", card: working });
    // somebody else's card is not ours
    expect(glassView({ ...base, glow: "processing", queryId: "q1", card: card({ queryId: "other", phase: "processing" }) })).toEqual({ kind: "processing", card: null });
  });

  it("shows the card of its own query: answer, question, error", () => {
    const answer = card({ items: items(2) });
    expect(glassView({ ...base, glow: "completed", queryId: "q1", card: answer, hasText: true })).toEqual({ kind: "answer", card: answer });
    const ask = card({ phase: "choices" });
    expect(glassView({ ...base, glow: "completed", queryId: "q1", card: ask })).toEqual({ kind: "choices", card: ask });
    const bad = card({ phase: "error", errorCode: "OUTLOOK-101" });
    expect(glassView({ ...base, glow: "error", queryId: "q1", card: bad })).toEqual({ kind: "error", card: bad });
  });

  it("keeps the answer after the glow's 600 ms hold (the sheet is not the ring)", () => {
    const answer = card();
    for (const glow of ["completed", "activated"] as const) expect(glassView({ ...base, glow, queryId: "q1", card: answer, hasText: true }).kind).toBe("answer");
  });

  it("shows nothing of a card that is not the current query's", () => {
    expect(glassView({ ...base, glow: "completed", queryId: "q2", card: card() })).toEqual({ kind: "ready" });
    expect(glassView({ ...base, glow: "completed", queryId: null, card: card(), hasText: true })).toEqual({ kind: "typing" });
  });

  it("typing after a result gives way to the field", () => {
    expect(glassView({ ...base, glow: "typing", queryId: "q1", card: card(), hasText: true, stale: true })).toEqual({ kind: "typing" });
    expect(glassView({ ...base, glow: "activated", queryId: "q1", card: card(), hasText: false, stale: true })).toEqual({ kind: "ready" });
    // but a new search always shows its own state
    expect(glassView({ ...base, glow: "submitting", queryId: "q1", card: card(), hasText: true, stale: true }).kind).toBe("processing");
  });

  it("a failed command is an error without a card", () => {
    expect(glassView({ ...base, glow: "error", hasText: true })).toEqual({ kind: "error", card: null });
  });
});

describe("card text", () => {
  it("the headline is the title, or the question for a card that only asks", () => {
    expect(glassHeadline(card({ title: "מחר יש לך 3 פגישות" }))).toBe("מחר יש לך 3 פגישות");
    expect(glassHeadline(card({ title: "", summary: "לא נמצא כלום" }))).toBe("לא נמצא כלום");
    expect(glassHeadline(card({ phase: "choices", title: "t", question: "באיזו תיבה?" }))).toBe("באיזו תיבה?");
    expect(glassHeadline(card({ phase: "choices", title: "t", question: null }))).toBe("t");
  });

  it("the summary is under the headline, unless it already served as the headline", () => {
    expect(glassSummary(card({ title: "כותרת", summary: "פירוט" }))).toBe("פירוט");
    expect(glassSummary(card({ title: "", summary: "פירוט" }))).toBe("");
  });

  it("a row's second line is its subtitle and source", () => {
    expect(rowMeta(item("a", { subtitle: "דנה", source: "יומן אישי" }))).toBe("דנה · יומן אישי");
    expect(rowMeta(item("a", { subtitle: null, source: "יומן אישי" }))).toBe("יומן אישי");
    expect(rowMeta(item("a", { subtitle: null, source: null }))).toBe("");
  });

  it("at most five rows, only under an answer; the Center link needs at least one", () => {
    expect(GLASS_MAX_ROWS).toBe(5);
    expect(answerRows(card({ items: items(9) }))).toHaveLength(5);
    expect(answerRows(card({ phase: "choices", items: items(3) }))).toEqual([]);
    expect(showsAllResults(card({ items: items(1) }))).toBe(true);
    expect(showsAllResults(card({ items: [] }))).toBe(false);
    expect(showsAllResults(card({ phase: "error", items: items(2) }))).toBe(false);
  });
});

describe("rowTime", () => {
  const at = (h: number, m: number, dayOffset = 0) => new Date(2026, 9, 9 + dayOffset, h, m).getTime();
  const now = new Date(2026, 9, 9, 8, 0);

  it("an event is its range, in digits", () => {
    const text = rowTime(item("a", { time: at(9, 30), endTime: at(10, 0) }), now);
    expect(text).toMatch(/9:30|09:30/);
    expect(text).toContain("–");
    expect(text).toMatch(/10:00/);
  });

  it("an event without an end, or with the same end, is its start only", () => {
    expect(rowTime(item("a", { time: at(9, 30), endTime: null }), now)).not.toContain("–");
    expect(rowTime(item("a", { time: at(9, 30), endTime: at(9, 30) }), now)).not.toContain("–");
  });

  it("anything without a time has no time column", () => {
    expect(rowTime(item("a", { kind: "mail", time: null }), now)).toBe("");
  });

  it("a mail of today shows its hour, an older one its date (never a bare hour that could be any day)", () => {
    const today = rowTime(item("a", { kind: "mail", time: at(7, 5) }), now);
    expect(today).toMatch(/7:05|07:05/);
    const old = rowTime(item("a", { kind: "mail", time: at(7, 5, -6) }), now);
    expect(old).not.toMatch(/7:05|07:05/);
    expect(old.length).toBeGreaterThan(0);
  });
});

describe("a question's buttons", () => {
  const c = (id: string, kind: Choice["kind"], preferred = false, label = id): Choice => ({ id, label, kind, preferred });

  it("preferred first, the way out last, at most six, the way out never cut", () => {
    const list = [c("a", "mailbox"), c("b", "mailbox", true), c("all", "allMailboxes"), c("d", "mailbox"), c("e", "mailbox"), c("f", "mailbox"), c("g", "mailbox"), c("h", "mailbox")];
    const ordered = orderedChoices(list);
    expect(ordered.map((x) => x.id)).toEqual(["b", "a", "d", "e", "f", "all"]);
    expect(ordered).toHaveLength(GLASS_MAX_CHOICES);
  });

  it("the long way out is cut at its dash for the button (the label stays its tooltip)", () => {
    expect(choiceText(c("all", "allMailboxes", false, "לא יודע — חפש בכל התיבות"))).toBe("לא יודע…");
    expect(choiceText(c("m", "mailbox", false, "עבודה — דואר"))).toBe("עבודה — דואר");
  });

  it("only a mailbox question offers to remember the choice", () => {
    expect(showsRemember(card({ phase: "choices", choices: [c("m", "mailbox")] }))).toBe(true);
    expect(showsRemember(card({ phase: "choices", choices: [c("o", "option")] }))).toBe(false);
    expect(showsRemember(card({ phase: "answer", choices: [c("m", "mailbox")] }))).toBe(false);
  });
});

describe("selection", () => {
  const answer = (...openable: boolean[]): GlassView => ({ kind: "answer", card: card({ items: openable.map((o, i) => item(`e${i}`, { openable: o })) }) });

  it("the arrows land on openable rows only, and the start is the first of them", () => {
    const entries = glassEntries(answer(false, true, false, true));
    expect(entries).toEqual({ kind: "rows", ids: ["e0", "e1", "e2", "e3"], selectable: [1, 3] });
    expect(defaultSelection(entries)).toBe(1);
    expect(defaultSelection(glassEntries(answer(false, false)))).toBe(-1);
  });

  it("a question's buttons are all selectable, the preferred one first", () => {
    const view: GlassView = {
      kind: "choices",
      card: card({ phase: "choices", choices: [{ id: "a", label: "a", kind: "mailbox", preferred: false }, { id: "b", label: "b", kind: "mailbox", preferred: true }] }),
    };
    const entries = glassEntries(view);
    expect(entries).toEqual({ kind: "choices", ids: ["b", "a"], selectable: [0, 1] });
    expect(defaultSelection(entries)).toBe(0);
  });

  it("the field and the working line have nothing to select", () => {
    for (const view of [{ kind: "ready" }, { kind: "typing" }, { kind: "processing", card: null }, { kind: "error", card: null }] as GlassView[]) {
      expect(glassEntries(view)).toEqual({ kind: "none", ids: [], selectable: [] });
    }
  });

  it("ArrowDown / ArrowUp walk the selectable entries and wrap at both ends", () => {
    const sel = [1, 3, 4];
    expect(moveSelection(sel, 1, 1)).toBe(3);
    expect(moveSelection(sel, 3, 1)).toBe(4);
    expect(moveSelection(sel, 4, 1)).toBe(1);
    expect(moveSelection(sel, 1, -1)).toBe(4);
    expect(moveSelection(sel, 4, -1)).toBe(3);
  });

  it("from no selection ArrowDown starts at the first, ArrowUp at the last; nothing selectable stays -1", () => {
    expect(moveSelection([2, 5], -1, 1)).toBe(2);
    expect(moveSelection([2, 5], -1, -1)).toBe(5);
    expect(moveSelection([2, 5], 0, 1)).toBe(2); // a stale index counts as none
    expect(moveSelection([], 3, 1)).toBe(-1);
  });

  it("Enter opens the selected row of an answer; it answers a question only while the field is empty", () => {
    const rows = answer(true);
    expect(enterActsOnSelection(rows, 0, "מה יש לי מחר?")).toBe(true);
    expect(enterActsOnSelection(rows, -1, "x")).toBe(false);
    const ask: GlassView = { kind: "choices", card: card({ phase: "choices" }) };
    expect(enterActsOnSelection(ask, 0, "")).toBe(true);
    expect(enterActsOnSelection(ask, 0, "   ")).toBe(true);
    expect(enterActsOnSelection(ask, 0, "דואר עבודה")).toBe(false); // typed text is a reply: Enter submits it
    expect(enterActsOnSelection({ kind: "typing" }, 0, "x")).toBe(false);
    expect(enterActsOnSelection({ kind: "ready" }, 0, "")).toBe(false);
  });

  it("option ids are stable for aria-activedescendant", () => {
    expect(glassOptionId(0)).toBe("gl-opt-0");
    expect(glassOptionId(4)).toBe("gl-opt-4");
  });
});

describe("height", () => {
  const M = GLASS_METRICS;

  it("the field alone is the mock's 72 px plus the 1 px frame on each side", () => {
    expect(estimateSheetHeight({ kind: "ready" })).toBe(74);
    expect(estimateSheetHeight({ kind: "typing" })).toBe(74);
  });

  it("working adds the separator and the 60 px status row", () => {
    expect(estimateSheetHeight({ kind: "processing", card: null })).toBe(74 + 1 + 60);
  });

  it("an answer counts exactly what glass.css draws: headline, rows, footer", () => {
    // 2 + 72 + 1 + 48 (headline) + (4 + 3 x 62 + 2 x 2 + 10) rows + 48 footer = 375, as measured in the browser
    expect(estimateSheetHeight({ kind: "answer", card: card({ items: items(3) }) })).toBe(375);
    expect(estimateSheetHeight({ kind: "answer", card: card({ items: items(1) }) })).toBe(2 + 72 + 1 + 48 + (14 + 62) + 48);
    const five = estimateSheetHeight({ kind: "answer", card: card({ items: items(5) }) });
    expect(five).toBe(2 + 72 + 1 + 48 + (14 + 5 * 62 + 4 * 2) + 48);
    expect(five).toBeLessThanOrEqual(552);
  });

  it("no rows and nothing to open: only the headline", () => {
    expect(estimateSheetHeight({ kind: "answer", card: card({ items: [] }) })).toBe(2 + 72 + 1 + 48);
    // a search that can go on still offers it
    expect(estimateSheetHeight({ kind: "answer", card: card({ items: [], canExtend: true }) })).toBe(2 + 72 + 1 + 48 + M.footer);
  });

  it("a summary adds its lines; a long one two", () => {
    const short = estimateSheetHeight({ kind: "answer", card: card({ summary: "קצר" }) });
    const long = estimateSheetHeight({ kind: "answer", card: card({ summary: "ארוך ".repeat(30) }) });
    expect(short).toBe(2 + 72 + 1 + 48 + M.summaryGap + M.summaryLine);
    expect(long).toBe(short + M.summaryLine);
  });

  it("a question counts its buttons and the remember row", () => {
    const choices: Choice[] = [
      { id: "a", label: "a", kind: "mailbox", preferred: true },
      { id: "b", label: "b", kind: "mailbox", preferred: false },
    ];
    const withRemember = estimateSheetHeight({ kind: "choices", card: card({ phase: "choices", choices }) });
    const without = estimateSheetHeight({ kind: "choices", card: card({ phase: "choices", choices: choices.map((x) => ({ ...x, kind: "option" as const })) }) });
    expect(withRemember - without).toBe(M.remember);
    expect(without).toBe(2 + 72 + 1 + 48 + M.choicesPadding + 2 * M.choice + M.choiceGap);
  });

  it("never taller than the window allows", () => {
    expect(estimateSheetHeight({ kind: "answer", card: card({ items: items(5) }) }, 300)).toBe(300);
    expect(maxSheetHeight(640)).toBe(552);
    expect(maxSheetHeight(2000)).toBe(552);
    expect(maxSheetHeight(400)).toBe(400 - 24 - 64);
    expect(maxSheetHeight(10)).toBe(74);
  });

  it("growing tells the window first, shrinking tells it last", () => {
    expect(heightPlan(74, 375)).toBe("region-then-sheet");
    expect(heightPlan(375, 74)).toBe("sheet-then-region");
    expect(heightPlan(375, 375)).toBe("none");
  });
});
