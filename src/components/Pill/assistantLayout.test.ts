// The card's shape per phase, and the proof that it never grows the stage window.
import { afterEach, describe, expect, it } from "vitest";
import type { AssistantCard, AssistantItem, Choice } from "../../lib/assistant/types";
import { NO_LIMITS, setIslandLimits } from "../../lib/island/limits";
import { control, panel, smallExpanded, type as typeRoles } from "../../design/tokens";
import { expandedSize, limitSize, meetingAlertSize, pillDimensions } from "./animations";
import {
  ASSISTANT_MAX_CHOICES,
  ASSISTANT_MAX_ITEMS,
  assistantIslandSize,
  assistantLayout,
  assistantMaxSize,
  cardHeadline,
  choiceLabel,
  itemMeta,
  itemRowHeight,
  PROCESSING_HEIGHT,
  showsAllResults,
  showsExtend,
  showsRemember,
  textLines,
  visibleChoices,
} from "./assistantLayout";
import { maxShapeSize, stageSize } from "./usePillGeometry";
import { toastMaxSize } from "./toastLayout";

afterEach(() => setIslandLimits(NO_LIMITS));

const card = (extra: Partial<AssistantCard> = {}): AssistantCard => ({
  queryId: "q",
  query: "",
  phase: "answer",
  lang: "he",
  title: "מחר יש לאיציק 3 פגישות",
  summary: "",
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
const item = (id: string, extra: Partial<AssistantItem> = {}): AssistantItem => ({
  id,
  kind: "event",
  title: `Meeting ${id}`,
  subtitle: null,
  time: 1_700_000_000_000,
  endTime: null,
  accent: null,
  openable: false,
  unread: false,
  source: "Calendar",
  ...extra,
});
const choice = (id: string, extra: Partial<Choice> = {}): Choice => ({ id, label: `Mailbox ${id}`, kind: "mailbox", preferred: false, ...extra });
const LONG = "מצאתי הרבה מאוד תוצאות שונות ומגוונות בכל התיבות והיומנים שיש לך הרשאה אליהם ואי אפשר להציג את כולן כאן ולכן ".repeat(3);

describe("the card's shape", () => {
  it("working is one line: a capsule as tall as the line and narrower than a card", () => {
    const size = assistantIslandSize(card({ phase: "processing" }));
    expect(size.height).toBe(PROCESSING_HEIGHT);
    expect(size.radius).toBe(PROCESSING_HEIGHT / 2);
    expect(size.width).toBeLessThan(smallExpanded.width);
    expect(size.width).toBeGreaterThan(100);
  });

  it("an answer, a question and an error are the small-expanded width", () => {
    for (const phase of ["answer", "choices", "error"] as const) expect(assistantIslandSize(card({ phase })).width).toBe(smallExpanded.width);
  });

  it("an answer grows with its summary and its items, three rows at most", () => {
    const bare = assistantIslandSize(card()).height;
    const withSummary = assistantIslandSize(card({ summary: "09:00 · 11:30 · 14:00" })).height;
    expect(withSummary).toBe(bare + 4 + typeRoles.body.lineHeight);
    const one = assistantLayout(card({ items: [item("1")], total: 1 }));
    const many = assistantLayout(card({ items: ["1", "2", "3", "4", "5"].map((id) => item(id)), total: 5 }));
    expect(one.items).toHaveLength(1);
    expect(many.items).toHaveLength(ASSISTANT_MAX_ITEMS);
    expect(many.size.height).toBeGreaterThan(one.size.height);
    expect(many.size.height).toBe(bare + 12 + 3 * itemRowHeight(item("1")) + 2 * 4 + (many.showAll ? 16 + control.height : 0));
  });

  it("a row with a source is taller than a row without one", () => {
    expect(itemRowHeight(item("1"))).toBe(8 + typeRoles.body.lineHeight + typeRoles.meta.lineHeight);
    expect(itemRowHeight(item("1", { source: null }))).toBe(8 + typeRoles.body.lineHeight);
    expect(itemMeta(item("1", { subtitle: "Dana", source: "Inbox" }))).toBe("Dana · Inbox");
    expect(itemMeta(item("1", { source: null }))).toBe("");
  });

  it("a long title takes two lines, a very long one still two", () => {
    expect(assistantLayout(card({ title: "קצר" })).titleLines).toBe(1);
    expect(assistantLayout(card({ title: LONG })).titleLines).toBe(2);
    expect(textLines("", typeRoles.title, 100, 2)).toBe(0);
    expect(textLines(LONG, typeRoles.body, 300, 2)).toBe(2);
  });

  it("an error shows its code under the text, a card with no title uses the summary as the headline", () => {
    const plain = assistantIslandSize(card({ phase: "error", title: "לא הצלחתי" })).height;
    expect(assistantIslandSize(card({ phase: "error", title: "לא הצלחתי", errorCode: "MAIL-101" })).height).toBe(plain + 4 + typeRoles.meta.lineHeight);
    const headlineOnly = card({ title: "", summary: "רק סיכום" });
    expect(cardHeadline(headlineOnly)).toBe("רק סיכום");
    expect(assistantLayout(headlineOnly).summaryLines).toBe(0);
  });
});

describe("the actions", () => {
  it("show all results when there is more than the card lists, several sources, or an unfinished search", () => {
    expect(showsAllResults(card({ items: [item("1")], total: 1 }))).toBe(false);
    expect(showsAllResults(card({ items: [item("1")], total: 9 }))).toBe(true);
    expect(showsAllResults(card({ items: ["1", "2", "3", "4"].map((id) => item(id)), total: 4 }))).toBe(true);
    expect(showsAllResults(card({ items: [item("1")], total: 1, sources: ["A", "B"] }))).toBe(true);
    expect(showsAllResults(card({ items: [item("1")], total: 1, sources: ["A"] }))).toBe(false);
    expect(showsAllResults(card({ items: [], total: 0, partial: true }))).toBe(true);
    expect(showsAllResults(card({ phase: "choices", total: 9, partial: true }))).toBe(false);
  });

  it("extend only on an answer that can be extended; the row costs one button height", () => {
    expect(showsExtend(card({ canExtend: true }))).toBe(true);
    expect(showsExtend(card({ canExtend: false }))).toBe(false);
    expect(showsExtend(card({ phase: "error", canExtend: true }))).toBe(false);
    const without = assistantIslandSize(card()).height;
    expect(assistantIslandSize(card({ canExtend: true })).height).toBe(without + 16 + control.height);
    // Both buttons share ONE row.
    expect(assistantIslandSize(card({ canExtend: true, partial: true })).height).toBe(without + 16 + control.height);
  });

  it("the remember switch only for mailbox questions", () => {
    const mailbox = card({ phase: "choices", choices: [choice("a")] });
    expect(showsRemember(mailbox)).toBe(true);
    expect(showsRemember(card({ phase: "choices", choices: [choice("a", { kind: "option" })] }))).toBe(false);
    expect(assistantIslandSize(mailbox).height).toBeGreaterThan(assistantIslandSize(card({ phase: "choices", choices: [choice("a", { kind: "option" })] })).height);
  });
});

describe("the choices", () => {
  it("the preferred first, the way out last", () => {
    const list = [choice("all", { kind: "allMailboxes" }), choice("b"), choice("a", { preferred: true }), choice("c")];
    expect(visibleChoices(list).map((c) => c.id)).toEqual(["a", "b", "c", "all"]);
  });

  it("at most six, and the way out is never the one that is cut", () => {
    const many = [...Array.from({ length: 9 }, (_, i) => choice(`m${i}`)), choice("all", { kind: "allMailboxes" })];
    const shown = visibleChoices(many);
    expect(shown).toHaveLength(ASSISTANT_MAX_CHOICES);
    expect(shown[shown.length - 1]?.id).toBe("all");
    expect(shown.slice(0, 5).map((c) => c.id)).toEqual(["m0", "m1", "m2", "m3", "m4"]);
  });

  it("the long way-out label is cut at its dash for the button", () => {
    const out = choice("all", { kind: "allMailboxes", label: "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן." });
    expect(choiceLabel(out)).toBe("אני לא יודע…");
    expect(choiceLabel(choice("x", { label: "A — B" }))).toBe("A — B");
    expect(choiceLabel(choice("y", { kind: "allMailboxes", label: "הכול" }))).toBe("הכול");
  });
});

describe("never larger than the island's existing bound", () => {
  const worst: AssistantCard[] = [
    card({ title: LONG, summary: LONG, items: ["1", "2", "3"].map((id) => item(id, { subtitle: "x" })), total: 99, partial: true, canExtend: true, sources: ["A", "B"] }),
    card({ phase: "choices", question: LONG, choices: Array.from({ length: 12 }, (_, i) => choice(String(i), { kind: i === 11 ? "allMailboxes" : "mailbox" })) }),
    card({ phase: "error", title: LONG, summary: LONG, errorCode: "MAIL-109" }),
    card({ phase: "processing" }),
  ];

  it("every worst case fits assistantMaxSize, and that fits the 400x440 panel", () => {
    const max = assistantMaxSize();
    expect(max.width).toBeLessThanOrEqual(panel.width);
    expect(max.height).toBeLessThanOrEqual(panel.height);
    for (const c of worst) {
      const size = assistantIslandSize(c);
      expect(size.width).toBeLessThanOrEqual(max.width);
      expect(size.height).toBeLessThanOrEqual(max.height);
    }
  });

  it("the stage window is exactly what it was without the card", () => {
    const d = pillDimensions;
    const withoutAssistant = {
      width: Math.max(expandedSize(NO_LIMITS).width, meetingAlertSize(2, true, true).width, toastMaxSize().width, d.compactMeeting.maxWidth, d.compact.maxWidth, d.ringer.width),
      height: Math.max(expandedSize(NO_LIMITS).height, meetingAlertSize(2, true, true).height, toastMaxSize().height, d.compact.height, d.ringer.height),
    };
    expect(maxShapeSize(NO_LIMITS)).toEqual(withoutAssistant);
    expect(maxShapeSize(NO_LIMITS)).toEqual({ width: panel.width, height: panel.height });
    const stage = stageSize(NO_LIMITS);
    for (const c of worst) {
      const size = assistantIslandSize(c);
      expect(stage.width).toBeGreaterThanOrEqual(size.width);
      expect(stage.height).toBeGreaterThanOrEqual(size.height);
    }
  });

  it("on a small monitor the stage still holds the limited card", () => {
    const limits = { maxWidth: 300, maxHeight: 380, scale: 1 };
    const limited = limitSize(assistantMaxSize(), limits);
    const stage = stageSize(limits);
    expect(stage.width).toBeGreaterThanOrEqual(limited.width);
    expect(stage.height).toBeGreaterThanOrEqual(limited.height);
    expect(limited.width).toBeLessThanOrEqual(300);
    expect(limited.height).toBeLessThanOrEqual(380);
  });
});
