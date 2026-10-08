// Pure state of the smart search bar: the eight glow states and what moves between them.
// No clock and no I/O: timers (the 600 ms Completed hold) live in the page and dispatch HOLD_DONE.
//
// Processing exists only between a submit and the final card for that query: it ends when the
// card arrives, never after a minimum time. Cards of a superseded query are ignored.

import type { AssistantCard } from "../lib/assistant/types";

export type GlowState = "idle" | "activated" | "typing" | "submitting" | "processing" | "completed" | "error" | "disabled";

/** States whose layers move. Everything else renders static layers or nothing. */
export const ANIMATED_STATES: readonly GlowState[] = ["activated", "typing", "submitting", "processing"];

export type SearchState = {
  glow: GlowState;
  /** Query the bar is waiting for or showing; null until the first card of a submit arrives. */
  queryId: string | null;
  /** Ids of queries that were replaced by a newer submit: their cards are ignored. */
  superseded: readonly string[];
  /** The last card asked a question: the bar shows the "pick or type" hint. */
  choices: boolean;
  /** The last card was an answer (keep the text selected for a follow-up). */
  answered: boolean;
  /** AI Mode is switched off in the settings (survives window resets). */
  disabled: boolean;
};

export type SearchEvent =
  | { type: "SHOW" } // the window became visible / focused
  | { type: "HIDE" } // the window was hidden: back to Idle, queries forgotten
  | { type: "INPUT"; empty: boolean }
  | { type: "SUBMIT" }
  | { type: "CARD"; card: Pick<AssistantCard, "queryId" | "phase"> }
  | { type: "SUBMIT_FAILED" } // the command threw or returned nothing usable
  | { type: "HOLD_DONE" }
  | { type: "DISABLED"; on: boolean };

export const INITIAL_STATE: SearchState = {
  glow: "idle",
  queryId: null,
  superseded: [],
  choices: false,
  answered: false,
  disabled: false,
};

const MAX_SUPERSEDED = 8;

const IDLE_FIELDS = { queryId: null, superseded: [], choices: false, answered: false } as const;

export function searchReducer(state: SearchState, event: SearchEvent): SearchState {
  if (event.type === "DISABLED") {
    return event.on
      ? { ...state, glow: "disabled", disabled: true }
      : { ...state, glow: state.glow === "disabled" ? "idle" : state.glow, disabled: false };
  }
  if (state.disabled) return state;

  switch (event.type) {
    case "SHOW":
      return state.glow === "idle" ? { ...state, glow: "activated" } : state;

    case "HIDE":
      return { ...state, ...IDLE_FIELDS, glow: "idle" };

    case "INPUT": {
      if (state.glow === "idle" || state.glow === "submitting" || state.glow === "processing") return state;
      // Error holds until the next input; any input (even clearing the field) ends it.
      const glow: GlowState = event.empty ? "activated" : "typing";
      return state.glow === glow ? state : { ...state, glow };
    }

    case "SUBMIT": {
      if (state.glow === "idle") return state;
      const superseded =
        state.queryId === null ? state.superseded : [...state.superseded.filter((id) => id !== state.queryId), state.queryId].slice(-MAX_SUPERSEDED);
      return { ...state, glow: "submitting", queryId: null, superseded, choices: false, answered: false };
    }

    case "CARD": {
      const { queryId, phase } = event.card;
      if (state.glow === "idle") return state;
      if (state.superseded.includes(queryId)) return state;
      const waiting = state.glow === "submitting" || state.glow === "processing";
      // Our own query (or the first card after a submit adopts its id). Anything else is stale
      // or somebody else's: only a bar that is waiting for an answer reacts.
      if (state.queryId !== null && state.queryId !== queryId) return state;
      if (state.queryId === null && !waiting) return state;

      switch (phase) {
        case "processing":
          // Also for our own finished query: "search 10 more seconds" works again on it.
          return waiting || state.queryId === queryId ? { ...state, glow: "processing", queryId } : state;
        case "answer":
          return { ...state, glow: "completed", queryId, choices: false, answered: true };
        case "choices":
          return { ...state, glow: "completed", queryId, choices: true, answered: false };
        case "error":
          return { ...state, glow: "error", queryId, choices: false, answered: false };
      }
      return state;
    }

    case "SUBMIT_FAILED":
      return state.glow === "submitting" || state.glow === "processing" ? { ...state, glow: "error", choices: false, answered: false } : state;

    case "HOLD_DONE":
      return state.glow === "completed" ? { ...state, glow: "activated" } : state;
  }
}

// -----------------------------------------------------------------------------
// Rendering mode
// -----------------------------------------------------------------------------

/** full: animated gradient; reduced: frozen gradient, opacity/colour only; plain: 2 px system ring. */
export type GlowMode = "full" | "reduced" | "plain";

export function glowMode(opts: { reducedMotion: boolean; forcedColors: boolean; highContrast: boolean }): GlowMode {
  if (opts.forcedColors || opts.highContrast) return "plain";
  return opts.reducedMotion ? "reduced" : "full";
}

/** Whether the glow layers exist at all. Idle and Disabled draw nothing. */
export function glowVisible(glow: GlowState): boolean {
  return glow !== "idle" && glow !== "disabled";
}

/** Whether the layers move right now: only the four live states, and never while hidden. */
export function glowAnimates(glow: GlowState, mode: GlowMode, documentHidden: boolean): boolean {
  return mode === "full" && !documentHidden && ANIMATED_STATES.includes(glow);
}

/** Seconds per turn of the ring in each live state (Completed freezes where it is). */
export function sweepPeriodSeconds(glow: GlowState): number | null {
  switch (glow) {
    case "activated":
      return 12;
    case "typing":
      return 18;
    case "submitting":
      return 6;
    case "processing":
      return 3.5;
    default:
      return null;
  }
}

// -----------------------------------------------------------------------------
// Keyboard rules of the input (pure, so they are testable without a DOM)
// -----------------------------------------------------------------------------

export type KeyAction = "submit" | "close" | "recall" | "recallDown" | "none";

export function keyAction(e: { key: string; isComposing: boolean; shiftKey?: boolean }, text: string): KeyAction {
  // IME: Enter confirms a composition, it is not a submit. keyCode 229 is the legacy marker.
  if (e.isComposing) return "none";
  if (e.key === "Enter") return text.trim().length > 0 ? "submit" : "none";
  if (e.key === "Escape") return "close";
  if (e.key === "ArrowUp") return "recall";
  if (e.key === "ArrowDown") return "recallDown";
  return "none";
}

/**
 * ArrowUp / ArrowDown through the questions asked in this session (memory only, newest last).
 * `index` is how far back we are: -1 = not recalling. Returns the new index and the text to show
 * (null = leave the text alone).
 */
export function recall(history: readonly string[], index: number, dir: "up" | "down"): { index: number; text: string | null } {
  if (history.length === 0) return { index, text: null };
  if (dir === "up") {
    const next = Math.min(index + 1, history.length - 1);
    return { index: next, text: history[history.length - 1 - next] };
  }
  if (index < 0) return { index, text: null };
  const next = index - 1;
  return { index: next, text: next < 0 ? "" : history[history.length - 1 - next] };
}

/** The bar's text is capped like the backend's question limit. */
export const MAX_QUERY_CHARS = 500;
