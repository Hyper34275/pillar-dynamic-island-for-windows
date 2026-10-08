// Pure model of the centre "Spotlight glass" sheet: what it shows for a state of the search, which row
// is selected, how tall it should be, how strong its tint is. No React, no DOM, no clock.
//
// The sheet shows ONE of: the field alone (ready / typing), the working line, an answer (headline,
// summary, up to five rows), a question with its buttons, or an error. The card is the backend's
// AssistantCard (memory only: nothing here logs or stores it).

import { spotlight } from "../design/tokens";
import { shortDate, timeParts } from "../lib/dateFormat";
import type { AssistantCard, AssistantItem, Choice } from "../lib/assistant/types";
import type { GlowState } from "./searchState";

/** Rows listed under an answer; the rest is behind "show all results". */
export const GLASS_MAX_ROWS = 5;
/** Buttons of a question (like the island: the "I don't know" way out is never the one cut). */
export const GLASS_MAX_CHOICES = 6;

// =============================================================================
// The backdrop picture (from search_bar/snapshot.rs)
// =============================================================================

/** Wire name of the event carrying a GlassBackdrop (search_bar/glass.rs). */
export const BACKDROP_EVENT = "search-bar-backdrop";

export type GlassBackdrop = {
  /** Counts up per capture; the page ignores a picture it already holds. */
  id: number;
  /** A data URL of the screen under the window (blurred by the page), or null. */
  image: string | null;
  /** Mean brightness of the picture, 0..1, or null. */
  luminance: number | null;
  /** The Windows app theme is dark. */
  dark: boolean;
  /** "Transparency effects" are on in Windows. */
  transparency: boolean;
};

const IMAGE_PREFIX = /^data:image\/(bmp|png);base64,[A-Za-z0-9+/]+={0,2}$/;
/** A picture larger than this is not shown (the real one is about 100 KB). */
const MAX_IMAGE_CHARS = 4_000_000;

/** The backend's payload, made safe: only a base64 BMP/PNG data URL is ever used as an image. */
export function normalizeBackdrop(raw: unknown): GlassBackdrop | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const r = raw as Record<string, unknown>;
  const id = typeof r.id === "number" && Number.isFinite(r.id) ? r.id : 0;
  const image = typeof r.image === "string" && r.image.length <= MAX_IMAGE_CHARS && IMAGE_PREFIX.test(r.image) ? r.image : null;
  const luminance = typeof r.luminance === "number" && Number.isFinite(r.luminance) ? Math.min(1, Math.max(0, r.luminance)) : null;
  return {
    id,
    image,
    luminance: image ? luminance : null,
    dark: r.dark === true,
    transparency: r.transparency !== false,
  };
}

// =============================================================================
// Tint
// =============================================================================

export type GlassTheme = "dark" | "light";

/** Alpha of the tint of a sheet that has no picture behind it (transparency off, capture failed). */
export const OPAQUE_ALPHA = 0.96;

/**
 * Alpha of the tint over the blurred picture. The mock's values (dark .46, light .66) hold over a
 * wallpaper of the same family; over the opposite (a dark sheet over a white window, a light sheet
 * over a black one) the tint is raised so the text keeps its contrast.
 */
export function tintAlpha(theme: GlassTheme, luminance: number | null, opaque: boolean): number {
  if (opaque) return OPAQUE_ALPHA;
  const lum = luminance === null ? null : Math.min(1, Math.max(0, luminance));
  if (theme === "dark") {
    const base = 0.46;
    return lum === null ? base : Math.min(0.72, base + Math.max(0, lum - 0.35) * 0.55);
  }
  const base = 0.66;
  return lum === null ? base : Math.min(0.88, base + Math.max(0, 0.55 - lum) * 0.5);
}

// =============================================================================
// What the sheet shows
// =============================================================================

export type GlassView =
  /** The field, empty: placeholder and the shortcut hint. */
  | { kind: "ready" }
  /** The field with text, nothing under it yet. */
  | { kind: "typing" }
  /** Working: the playful line. `card` is the processing card once it arrived. */
  | { kind: "processing"; card: AssistantCard | null }
  | { kind: "answer"; card: AssistantCard }
  | { kind: "choices"; card: AssistantCard }
  /** `card` is null when the command itself failed (no card to read). */
  | { kind: "error"; card: AssistantCard | null };

export type GlassViewInput = {
  glow: GlowState;
  /** The query the search state is waiting for or showing. */
  queryId: string | null;
  /** The card of that query, when one arrived. */
  card: AssistantCard | null;
  hasText: boolean;
  /** The person typed after the result: the result gives way to the field. */
  stale: boolean;
};

export function glassView({ glow, queryId, card, hasText, stale }: GlassViewInput): GlassView {
  if (glow === "disabled") return { kind: "ready" };
  const mine = card && queryId !== null && card.queryId === queryId ? card : null;
  if (glow === "submitting") return { kind: "processing", card: null };
  if (glow === "processing") return { kind: "processing", card: mine };
  if (stale) return hasText ? { kind: "typing" } : { kind: "ready" };
  if (mine) {
    switch (mine.phase) {
      case "processing":
        return { kind: "processing", card: mine };
      case "answer":
        return { kind: "answer", card: mine };
      case "choices":
        return { kind: "choices", card: mine };
      case "error":
        return { kind: "error", card: mine };
    }
  }
  if (glow === "error") return { kind: "error", card: null };
  return hasText ? { kind: "typing" } : { kind: "ready" };
}

/** The headline of a card: its title, or (a card with only a question) the question. */
export function glassHeadline(card: AssistantCard): string {
  if (card.phase === "choices") return card.question || card.title;
  return card.title || card.summary;
}

/** The text under the headline: the summary, unless it already served as the headline. */
export function glassSummary(card: AssistantCard): string {
  return card.title ? card.summary : "";
}

/** The second line of a row: who / where it is from. */
export function rowMeta(item: AssistantItem): string {
  return [item.subtitle, item.source].filter((part): part is string => !!part).join(" · ");
}

const sameDay = (a: Date, b: Date) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/**
 * The time column of a row: an event's "09:30 – 10:00"; anything else with a time shows its hour
 * today and its date otherwise (a mail from last week is not "09:30"); nothing without a time.
 */
export function rowTime(item: AssistantItem, now: Date): string {
  if (item.time === null) return "";
  const start = new Date(item.time);
  const from = timeParts(start).digits;
  if (item.kind === "event") {
    if (item.endTime === null || item.endTime === item.time) return from;
    return `${from} – ${timeParts(new Date(item.endTime)).digits}`;
  }
  return sameDay(start, now) ? from : shortDate(start);
}

/** What a choice button says: the long "I don't know - search everywhere" way out is cut at its dash. */
export function choiceText(choice: Choice): string {
  const dash = choice.label.indexOf(" — ");
  return choice.kind === "allMailboxes" && dash > 0 ? `${choice.label.slice(0, dash)}…` : choice.label;
}

/** Preferred choice first, the "all mailboxes" way out last, the rest as given; at most GLASS_MAX_CHOICES. */
export function orderedChoices(choices: readonly Choice[]): Choice[] {
  const all = choices.filter((choice) => choice.kind === "allMailboxes");
  const rest = choices.filter((choice) => choice.kind !== "allMailboxes");
  const ordered = [...rest.filter((choice) => choice.preferred), ...rest.filter((choice) => !choice.preferred)];
  return [...ordered.slice(0, GLASS_MAX_CHOICES - Math.min(all.length, 1)), ...all.slice(0, 1)];
}

/** Mailbox questions get the "remember my choice" switch. */
export function showsRemember(card: AssistantCard): boolean {
  return card.phase === "choices" && card.choices.some((choice) => choice.kind === "mailbox" || choice.kind === "allMailboxes");
}

/** The rows under an answer. */
export function answerRows(card: AssistantCard): AssistantItem[] {
  return card.phase === "answer" ? card.items.slice(0, GLASS_MAX_ROWS) : [];
}

/** "Show all results": the answer has rows to open in the Center. */
export function showsAllResults(card: AssistantCard): boolean {
  return card.phase === "answer" && card.items.length > 0;
}

// =============================================================================
// Selection (ArrowUp / ArrowDown, then Enter)
// =============================================================================

/** DOM ids of the list and its options: the input points at the selected one (aria-activedescendant). */
export const GLASS_LIST_ID = "gl-list";
export const glassOptionId = (index: number): string => `gl-opt-${index}`;

export type GlassEntries = {
  kind: "rows" | "choices" | "none";
  ids: string[];
  /** Indexes the arrows may land on: every openable row, every choice. */
  selectable: number[];
};

const NO_ENTRIES: GlassEntries = { kind: "none", ids: [], selectable: [] };

export function glassEntries(view: GlassView): GlassEntries {
  if (view.kind === "answer") {
    const rows = answerRows(view.card);
    return { kind: "rows", ids: rows.map((r) => r.id), selectable: rows.flatMap((r, i) => (r.openable ? [i] : [])) };
  }
  if (view.kind === "choices") {
    const choices = orderedChoices(view.card.choices);
    return { kind: "choices", ids: choices.map((c) => c.id), selectable: choices.map((_, i) => i) };
  }
  return NO_ENTRIES;
}

/** Where the selection starts when a result arrives: the first openable row, the preferred choice. */
export function defaultSelection(entries: GlassEntries): number {
  return entries.selectable[0] ?? -1;
}

/** One step with an arrow key through the selectable entries, wrapping at both ends. */
export function moveSelection(selectable: readonly number[], current: number, step: 1 | -1): number {
  if (selectable.length === 0) return -1;
  const at = selectable.indexOf(current);
  if (at < 0) return step > 0 ? selectable[0] : selectable[selectable.length - 1];
  return selectable[(at + step + selectable.length) % selectable.length];
}

/**
 * Whether Enter acts on the selected entry instead of submitting the field: an answer's selected row
 * opens; a question's selected button answers it, but only while the field is empty (typed text is a reply).
 */
export function enterActsOnSelection(view: GlassView, selected: number, text: string): boolean {
  if (selected < 0) return false;
  if (view.kind === "answer") return true;
  if (view.kind === "choices") return text.trim().length === 0;
  return false;
}

// =============================================================================
// Height
// =============================================================================

/** Metrics of the sheet (px), from the approved mock: they add up to what glass.css draws. */
export const GLASS_METRICS = {
  /** The 1 px stroke on each side: the content sits inside it, so the sheet is the content plus this. */
  frame: 2,
  field: 72,
  separator: 1,
  status: 60,
  headline: 48,
  /** One line of summary under the headline. */
  summaryLine: 20,
  summaryGap: 6,
  rowsPadding: 14,
  row: 62,
  rowGap: 2,
  footer: 48,
  choice: 46,
  choiceGap: 4,
  remember: 40,
  choicesPadding: 18,
  errorCode: 22,
} as const;

/**
 * The sheet's height for a view, counted from the metrics above. The page measures the real DOM;
 * this is what it falls back on where nothing can be measured (and what the tests pin down).
 */
export function estimateSheetHeight(view: GlassView, maxHeight: number = spotlight.sheetMax): number {
  const m = GLASS_METRICS;
  let height: number = m.frame + m.field;
  switch (view.kind) {
    case "ready":
    case "typing":
      break;
    case "processing":
      height += m.separator + m.status;
      break;
    case "answer": {
      const rows = answerRows(view.card).length;
      height += m.separator + m.headline;
      const summary = glassSummary(view.card);
      if (summary) height += m.summaryGap + m.summaryLine * (summary.length > 70 ? 2 : 1);
      if (rows > 0) height += m.rowsPadding + rows * m.row + (rows - 1) * m.rowGap;
      if (showsAllResults(view.card) || view.card.canExtend) height += m.footer;
      break;
    }
    case "choices": {
      const n = orderedChoices(view.card.choices).length;
      height += m.separator + m.headline + m.choicesPadding + n * m.choice + Math.max(0, n - 1) * m.choiceGap;
      if (showsRemember(view.card)) height += m.remember;
      break;
    }
    case "error": {
      height += m.separator + m.headline;
      if (view.card && glassSummary(view.card)) height += m.summaryGap + m.summaryLine;
      if (view.card?.errorCode) height += m.errorCode;
      height += 12;
      break;
    }
  }
  return Math.min(maxHeight, Math.round(height));
}

/** The tallest sheet the window can hold, from the window's height. */
export function maxSheetHeight(windowHeight: number): number {
  return Math.max(spotlight.field, Math.min(spotlight.sheetMax, windowHeight - spotlight.marginTop - spotlight.marginBottom));
}

/**
 * How a change of the sheet's height is sequenced with the window's click-through region.
 * Growing: the window learns the new height first (so the new area is clickable and its shadow is
 * not clipped), then the sheet grows. Shrinking: the sheet shrinks first, the window follows once
 * it has finished. `none` when nothing changed.
 */
export type HeightPlan = "none" | "region-then-sheet" | "sheet-then-region";

export function heightPlan(applied: number, target: number): HeightPlan {
  if (target === applied) return "none";
  return target > applied ? "region-then-sheet" : "sheet-then-region";
}

/** Duration of the sheet's growth (ms); none under reduced motion. */
export const HEIGHT_MS = 220;
/** Duration of the backdrop's fade-in (ms): it never delays the panel, it only arrives over it. */
export const BACKDROP_FADE_MS = 120;
