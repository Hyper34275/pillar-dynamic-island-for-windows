// The smart-search card's shape: as tall as its phase needs, always inside the existing 400x440
// bound (so the stage window and SHAPE_BOUNDS never change). Pure and measured once per card
// (canvas text measurement, like toastLayout.ts), so PillShell's target, the window region and the
// card layer always agree: the component renders exactly what this function counted.

import { alert as alertTokens, control, smallExpanded, type as typeRoles, type TypeRole } from "../../design/tokens";
import type { AssistantCard, AssistantItem, Choice } from "../../lib/assistant/types";
import { t } from "../../lib/i18n";
import { measureText } from "../../lib/textMeasure";
import type { IslandSize } from "./animations";
import { uiFontFamily } from "./useCompactLayout";

/** Items listed under an answer; the rest is behind "show all results". */
export const ASSISTANT_MAX_ITEMS = 3;
/** Buttons of a question card (the 440 px bound holds six with the remember row). */
export const ASSISTANT_MAX_CHOICES = 6;
export const ASSISTANT_SUMMARY_MAX_LINES = 2;
export const ASSISTANT_TITLE_MAX_LINES = 2;

const PAD = smallExpanded.padding;
const CONTENT_WIDTH = smallExpanded.width - PAD * 2;
/** The identity sparkle before the title, and its gap. */
export const SPARKLE_SIZE = 14;
export const SPARKLE_GAP = 8;
/** The close button at the end of the title row, and its gap to the title. */
export const CLOSE_SLOT = control.round + 8;
/** Title -> summary, and the gap above the item list. */
export const TEXT_GAP = alertTokens.labelGap;
export const BLOCK_GAP = 12;
/** Item rows: padded surface, the accent stripe and the time column inside. */
export const ITEM_PAD_Y = 4;
export const ITEM_GAP = 4;
export const ITEM_TIME_WIDTH = 40;
/** The processing pill: sparkle, label and close, in one line. */
export const PROCESSING_HEIGHT = 44;
export const PROCESSING_SPARKLE = 18;
export const PROCESSING_PAD = 12;
const PROCESSING_MIN_WIDTH = 120;
/** Wrapping wastes part of every line: a text filling a line this much is already one more line. */
const WRAP_SLACK = 0.95;
/** Canvas and DOM text widths differ by sub-pixel rounding; a single-line text gets this much air. */
const MEASURE_AIR = 2;
/** The remember-my-choice row under a question's buttons. */
export const REMEMBER_HEIGHT = 28;
const REMEMBER_GAP = 8;

function width(text: string, role: TypeRole): number {
  // The DOM applies the role's letter spacing; the canvas measure does not.
  const measured = measureText(text, `${role.weight} ${role.size}px ${uiFontFamily()}`, role.size);
  return Math.ceil(measured + role.tracking * role.size * text.length);
}

/** Lines `text` takes in `available` px (0 for no text), capped at `max` (CSS clamps the rest). */
export function textLines(text: string, role: TypeRole, available: number, max: number): number {
  if (!text) return 0;
  const needed = width(text, role); // the measure is already rounded up to a whole pixel
  // One line when it fits whole; a wrapped text wastes part of every line, so it needs more room.
  const lines = needed <= available ? 1 : Math.max(2, Math.ceil(needed / (available * WRAP_SLACK)));
  return Math.min(max, lines);
}

/** The second line of an item: who / where it is from. */
export function itemMeta(item: AssistantItem): string {
  return [item.subtitle, item.source].filter((part): part is string => !!part).join(" · ");
}

export function itemRowHeight(item: AssistantItem): number {
  return ITEM_PAD_Y * 2 + typeRoles.body.lineHeight + (itemMeta(item) ? typeRoles.meta.lineHeight : 0);
}

/** What a choice button says: the long "I don't know — search everywhere" way out is cut at its dash (the full text is its tooltip and accessible name). */
export function choiceLabel(choice: Choice): string {
  const dash = choice.label.indexOf(" — ");
  return choice.kind === "allMailboxes" && dash > 0 ? `${choice.label.slice(0, dash)}…` : choice.label;
}

/** Preferred choice first, the "all mailboxes" way out last, the rest in the order given; at most MAX. */
export function visibleChoices(choices: readonly Choice[]): Choice[] {
  const all = choices.filter((choice) => choice.kind === "allMailboxes");
  const rest = choices.filter((choice) => choice.kind !== "allMailboxes");
  const preferred = rest.filter((choice) => choice.preferred);
  const others = rest.filter((choice) => !choice.preferred);
  const ordered = [...preferred, ...others];
  // The way out is never the one that is cut.
  return [...ordered.slice(0, ASSISTANT_MAX_CHOICES - Math.min(all.length, 1)), ...all.slice(0, 1)];
}

/** "Show all results": more than the card lists, results from several sources, or an unfinished search. */
export function showsAllResults(card: AssistantCard): boolean {
  if (card.phase !== "answer") return false;
  const shown = Math.min(card.items.length, ASSISTANT_MAX_ITEMS);
  return card.total > shown || card.items.length > shown || card.sources.length > 1 || card.partial;
}

export function showsExtend(card: AssistantCard): boolean {
  return card.phase === "answer" && card.canExtend;
}

/** Mailbox questions get the "remember my choice" switch. */
export function showsRemember(card: AssistantCard): boolean {
  return card.phase === "choices" && card.choices.some((choice) => choice.kind === "mailbox" || choice.kind === "allMailboxes");
}

/** The headline of a card: its title, or (a card with only a question) the question. */
export function cardHeadline(card: AssistantCard): string {
  if (card.phase === "choices") return card.question || card.title;
  return card.title || card.summary;
}

/** The body text under the headline: the summary, unless it already served as the headline. */
export function cardSummary(card: AssistantCard): string {
  return card.title ? card.summary : "";
}

export interface AssistantLayout {
  size: IslandSize;
  phase: AssistantCard["phase"];
  titleLines: number;
  summaryLines: number;
  items: AssistantItem[];
  choices: Choice[];
  showAll: boolean;
  showExtend: boolean;
  showRemember: boolean;
  /** Any listed item has a time: the column exists for every row (one alignment). */
  showTime: boolean;
}

const sizeOf = (w: number, h: number): IslandSize => ({ width: w, height: h, radius: Math.min(smallExpanded.radius, h / 2) });

function titleRowHeight(lines: number): number {
  return Math.max(lines * typeRoles.title.lineHeight, control.round);
}

function compute(card: AssistantCard): AssistantLayout {
  const base = { phase: card.phase, titleLines: 0, summaryLines: 0, items: [] as AssistantItem[], choices: [] as Choice[], showAll: false, showExtend: false, showRemember: false, showTime: false };

  if (card.phase === "processing") {
    const label = width(t("ai.processing"), typeRoles.headline);
    const w = Math.ceil(PROCESSING_PAD * 2 + PROCESSING_SPARKLE + SPARKLE_GAP + label + MEASURE_AIR + SPARKLE_GAP + control.round);
    return { ...base, size: sizeOf(Math.min(smallExpanded.width, Math.max(PROCESSING_MIN_WIDTH, w)), PROCESSING_HEIGHT) };
  }

  const titleAvail = CONTENT_WIDTH - SPARKLE_SIZE - SPARKLE_GAP - CLOSE_SLOT;
  const headline = cardHeadline(card);
  const titleLines = textLines(headline, typeRoles.title, titleAvail, ASSISTANT_TITLE_MAX_LINES);
  let height = PAD + titleRowHeight(titleLines);

  if (card.phase === "choices") {
    const choices = visibleChoices(card.choices);
    const showRemember = showsRemember(card);
    if (choices.length) height += BLOCK_GAP + choices.length * control.height + (choices.length - 1) * control.gap;
    if (showRemember) height += REMEMBER_GAP + REMEMBER_HEIGHT;
    return { ...base, titleLines, choices, showRemember, size: sizeOf(smallExpanded.width, height + PAD) };
  }

  // The summary says what the title does not; an error also shows its code.
  const summary = cardSummary(card);
  const summaryLines = textLines(summary, typeRoles.body, CONTENT_WIDTH, ASSISTANT_SUMMARY_MAX_LINES);
  if (summaryLines) height += TEXT_GAP + summaryLines * typeRoles.body.lineHeight;

  if (card.phase === "error") {
    if (card.errorCode) height += TEXT_GAP + typeRoles.meta.lineHeight;
    return { ...base, titleLines, summaryLines, size: sizeOf(smallExpanded.width, height + PAD) };
  }

  const items = card.items.slice(0, ASSISTANT_MAX_ITEMS);
  if (items.length) height += BLOCK_GAP + items.reduce((sum, item) => sum + itemRowHeight(item), 0) + (items.length - 1) * ITEM_GAP;
  const showAll = showsAllResults(card);
  const showExtend = showsExtend(card);
  if (showAll || showExtend) height += alertTokens.actionsGap + control.height;
  return {
    ...base,
    titleLines,
    summaryLines,
    items,
    showAll,
    showExtend,
    showTime: items.some((item) => item.time !== null),
    size: sizeOf(smallExpanded.width, height + PAD),
  };
}

/** A card that could not be measured still gets a usable shape: the layout never throws into the shell. */
const FALLBACK_SIZE = sizeOf(smallExpanded.width, PAD * 2 + control.round);

export function assistantLayout(card: AssistantCard): AssistantLayout {
  try {
    return compute(card);
  } catch {
    return { phase: card.phase, titleLines: 1, summaryLines: 0, items: [], choices: [], showAll: false, showExtend: false, showRemember: false, showTime: false, size: FALLBACK_SIZE };
  }
}

/** The island size for `card` (what PillShell targets). */
export function assistantIslandSize(card: AssistantCard): IslandSize {
  return assistantLayout(card).size;
}

/**
 * The tallest and widest a card can be (the native window's stage must hold it): the larger of
 * a full answer (two-line title, two summary lines, three two-line items, both actions) and a
 * full question (two-line question, six buttons, the remember row). Within the panel's bound.
 */
export function assistantMaxSize(): IslandSize {
  const title = PAD + titleRowHeight(ASSISTANT_TITLE_MAX_LINES);
  const summary = TEXT_GAP + ASSISTANT_SUMMARY_MAX_LINES * typeRoles.body.lineHeight;
  const itemRow = ITEM_PAD_Y * 2 + typeRoles.body.lineHeight + typeRoles.meta.lineHeight;
  const items = BLOCK_GAP + ASSISTANT_MAX_ITEMS * itemRow + (ASSISTANT_MAX_ITEMS - 1) * ITEM_GAP;
  const answer = title + summary + items + alertTokens.actionsGap + control.height + PAD;
  const question = title + BLOCK_GAP + ASSISTANT_MAX_CHOICES * control.height + (ASSISTANT_MAX_CHOICES - 1) * control.gap + REMEMBER_GAP + REMEMBER_HEIGHT + PAD;
  return sizeOf(smallExpanded.width, Math.max(answer, question));
}
