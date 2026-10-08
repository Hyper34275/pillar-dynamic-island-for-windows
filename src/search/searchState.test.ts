import { describe, expect, it } from "vitest";
import {
  INITIAL_STATE,
  glowAnimates,
  glowMode,
  glowVisible,
  keyAction,
  recall,
  searchReducer,
  sweepPeriodSeconds,
  type GlowState,
  type SearchEvent,
  type SearchState,
} from "./searchState";
import { SEARCH_STRINGS } from "./strings";
import { sanitizeBar, DEFAULT_BAR } from "./bar";

const run = (events: SearchEvent[], from: SearchState = INITIAL_STATE) => events.reduce(searchReducer, from);
const card = (queryId: string, phase: "processing" | "answer" | "choices" | "error"): SearchEvent => ({ type: "CARD", card: { queryId, phase } });

describe("searchReducer", () => {
  it("starts Idle and activates on show", () => {
    expect(INITIAL_STATE.glow).toBe("idle");
    expect(run([{ type: "SHOW" }]).glow).toBe("activated");
  });

  it("ignores input and submit while Idle", () => {
    expect(run([{ type: "INPUT", empty: false }]).glow).toBe("idle");
    expect(run([{ type: "SUBMIT" }]).glow).toBe("idle");
  });

  it("types and returns to Activated when the field empties", () => {
    const s = run([{ type: "SHOW" }, { type: "INPUT", empty: false }]);
    expect(s.glow).toBe("typing");
    expect(searchReducer(s, { type: "INPUT", empty: true }).glow).toBe("activated");
  });

  it("follows submit -> processing -> completed, and holds Processing only until the card", () => {
    let s = run([{ type: "SHOW" }, { type: "INPUT", empty: false }, { type: "SUBMIT" }]);
    expect(s.glow).toBe("submitting");
    s = run([card("q1", "processing")], s);
    expect(s.glow).toBe("processing");
    expect(s.queryId).toBe("q1");
    s = run([card("q1", "answer")], s);
    expect(s.glow).toBe("completed");
    expect(s.answered).toBe(true);
    s = run([{ type: "HOLD_DONE" }], s);
    expect(s.glow).toBe("activated");
  });

  it("ends Submitting without a processing card when the answer arrives directly", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "answer")]);
    expect(s.glow).toBe("completed");
  });

  it("ignores typing while a request runs", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, { type: "INPUT", empty: false }]);
    expect(s.glow).toBe("submitting");
  });

  it("ignores results of a superseded query (stale queryId)", () => {
    let s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "processing"), { type: "SUBMIT" }]);
    expect(s.superseded).toEqual(["q1"]);
    s = run([card("q1", "answer")], s);
    expect(s.glow).toBe("submitting"); // late card of q1 must not complete q2
    s = run([card("q2", "processing"), card("q1", "error"), card("q2", "answer")], s);
    expect(s.glow).toBe("completed");
    expect(s.queryId).toBe("q2");
  });

  it("ignores a card of another query once ours is known", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q2", "processing"), card("other", "answer")]);
    expect(s.glow).toBe("processing");
  });

  it("ignores cards (island queries) when the bar is not waiting", () => {
    expect(run([{ type: "SHOW" }, card("x", "processing")]).glow).toBe("activated");
    expect(run([{ type: "SHOW" }, card("x", "answer")]).glow).toBe("activated");
  });

  it("a choices card completes and sets the hint flag; the next submit clears it", () => {
    let s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "choices")]);
    expect(s.glow).toBe("completed");
    expect(s.choices).toBe(true);
    s = run([{ type: "HOLD_DONE" }], s);
    expect(s.choices).toBe(true);
    s = run([{ type: "SUBMIT" }], s);
    expect(s.choices).toBe(false);
  });

  it("a typed reply to a question continues the same query and completes (review #12/#16)", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "choices"), { type: "SUBMIT" }, card("q1", "processing"), card("q1", "answer")]);
    expect(s.glow).toBe("completed");
    expect(s.queryId).toBe("q1");
  });

  it("a new question after a question card is still adopted", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "choices"), { type: "SUBMIT" }, card("q2", "answer")]);
    expect(s.glow).toBe("completed");
    expect(s.queryId).toBe("q2");
  });

  it("extending the same query shows Processing again", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "answer"), card("q1", "processing")]);
    expect(s.glow).toBe("processing");
  });

  it("Error holds until the next input", () => {
    let s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "error")]);
    expect(s.glow).toBe("error");
    expect(searchReducer(s, { type: "HOLD_DONE" }).glow).toBe("error");
    s = searchReducer(s, { type: "INPUT", empty: false });
    expect(s.glow).toBe("typing");
  });

  it("a failed command is an Error, but only while waiting", () => {
    expect(run([{ type: "SHOW" }, { type: "SUBMIT" }, { type: "SUBMIT_FAILED" }]).glow).toBe("error");
    expect(run([{ type: "SHOW" }, { type: "SUBMIT_FAILED" }]).glow).toBe("activated");
  });

  it("hide resets everything to Idle", () => {
    const s = run([{ type: "SHOW" }, { type: "SUBMIT" }, card("q1", "processing"), { type: "HIDE" }]);
    expect(s).toEqual(INITIAL_STATE);
  });

  it("Disabled blocks everything until re-enabled", () => {
    let s = run([{ type: "SHOW" }, { type: "DISABLED", on: true }]);
    expect(s.glow).toBe("disabled");
    s = run([{ type: "SHOW" }, { type: "INPUT", empty: false }, { type: "SUBMIT" }], s);
    expect(s.glow).toBe("disabled");
    s = run([{ type: "DISABLED", on: false }, { type: "SHOW" }], s);
    expect(s.glow).toBe("activated");
  });
});

describe("glow rendering rules", () => {
  it("draws nothing in Idle and Disabled", () => {
    const visible = (["idle", "activated", "typing", "submitting", "processing", "completed", "error", "disabled"] as GlowState[]).filter(glowVisible);
    expect(visible).toEqual(["activated", "typing", "submitting", "processing", "completed", "error"]);
  });

  it("animates only the four live states, only in full mode, never while hidden", () => {
    for (const g of ["activated", "typing", "submitting", "processing"] as GlowState[]) {
      expect(glowAnimates(g, "full", false)).toBe(true);
      expect(glowAnimates(g, "full", true)).toBe(false);
      expect(glowAnimates(g, "reduced", false)).toBe(false);
      expect(glowAnimates(g, "plain", false)).toBe(false);
      expect(sweepPeriodSeconds(g)).not.toBeNull();
    }
    for (const g of ["idle", "completed", "error", "disabled"] as GlowState[]) {
      expect(glowAnimates(g, "full", false)).toBe(false);
      expect(sweepPeriodSeconds(g)).toBeNull();
    }
    expect(sweepPeriodSeconds("typing")!).toBeGreaterThan(sweepPeriodSeconds("activated")!);
    expect(sweepPeriodSeconds("processing")!).toBeLessThan(sweepPeriodSeconds("submitting")!);
  });

  it("picks the mode: high contrast and forced colours beat reduced motion", () => {
    expect(glowMode({ reducedMotion: false, forcedColors: false, highContrast: false })).toBe("full");
    expect(glowMode({ reducedMotion: true, forcedColors: false, highContrast: false })).toBe("reduced");
    expect(glowMode({ reducedMotion: true, forcedColors: true, highContrast: false })).toBe("plain");
    expect(glowMode({ reducedMotion: false, forcedColors: false, highContrast: true })).toBe("plain");
  });
});

describe("keyAction", () => {
  const key = (k: string, isComposing = false) => ({ key: k, isComposing });
  it("submits on Enter with text, not when empty or blank", () => {
    expect(keyAction(key("Enter"), "מה יש")).toBe("submit");
    expect(keyAction(key("Enter"), "")).toBe("none");
    expect(keyAction(key("Enter"), "   ")).toBe("none");
  });
  it("ignores every key while an IME composition is open", () => {
    expect(keyAction(key("Enter", true), "text")).toBe("none");
    expect(keyAction(key("Escape", true), "text")).toBe("none");
  });
  it("maps Escape and the arrows", () => {
    expect(keyAction(key("Escape"), "")).toBe("close");
    expect(keyAction(key("ArrowUp"), "")).toBe("recall");
    expect(keyAction(key("ArrowDown"), "")).toBe("recallDown");
    expect(keyAction(key("a"), "")).toBe("none");
  });
});

describe("recall", () => {
  const h = ["a", "b", "c"];
  it("walks back and forth through the history", () => {
    let r = recall(h, -1, "up");
    expect(r).toEqual({ index: 0, text: "c" });
    r = recall(h, r.index, "up");
    expect(r.text).toBe("b");
    r = recall(h, 2, "up");
    expect(r).toEqual({ index: 2, text: "a" }); // clamped
    expect(recall(h, 1, "down")).toEqual({ index: 0, text: "c" });
    expect(recall(h, 0, "down")).toEqual({ index: -1, text: "" });
    expect(recall(h, -1, "down").text).toBeNull();
    expect(recall([], -1, "up").text).toBeNull();
  });
});

describe("strings", () => {
  it("he and en have the same keys, none empty", () => {
    expect(Object.keys(SEARCH_STRINGS.he).sort()).toEqual(Object.keys(SEARCH_STRINGS.en).sort());
    for (const loc of ["he", "en"] as const) for (const v of Object.values(SEARCH_STRINGS[loc])) expect(v.trim().length).toBeGreaterThan(0);
  });
  it("uses the contract wording", () => {
    expect(SEARCH_STRINGS.he.placeholder).toBe("שאל את יובל…");
    expect(SEARCH_STRINGS.he.aiToggle).toBe("יציאה ממצב AI");
    expect(SEARCH_STRINGS.he.choicesHint).toBe("אפשר לבחור באי או להקליד תשובה");
  });
});

describe("sanitizeBar", () => {
  it("falls back for missing or absurd geometry", () => {
    expect(sanitizeBar(null)).toEqual(DEFAULT_BAR);
    const s = sanitizeBar({ width: NaN, height: 3, radius: -1, scale: 0 });
    expect(s.width).toBe(DEFAULT_BAR.width);
    expect(s.height).toBe(DEFAULT_BAR.height);
    expect(s.radius).toBe(0);
    expect(s.scale).toBe(1);
  });
  it("keeps a good state", () => {
    const s = sanitizeBar({ anchored: true, width: 372, height: 64, radius: 4, scale: 1.25, highContrast: true, edge: "top" });
    expect(s).toMatchObject({ anchored: true, width: 372, height: 64, highContrast: true, edge: "top" });
  });
  it("derives the variant of an older backend from anchored", () => {
    expect(sanitizeBar({ anchored: true, width: 372, height: 64 }).variant).toBe("taskbar");
    expect(sanitizeBar({ width: 372, height: 64 }).variant).toBe("floating");
    expect(sanitizeBar({ variant: "bogus" as never, width: 372, height: 64 }).variant).toBe("floating");
  });
  it("keeps taskbar and floating as before", () => {
    expect(sanitizeBar({ variant: "taskbar", anchored: true, width: 372, height: 64, radius: 4 })).toMatchObject({ variant: "taskbar", anchored: true, radius: 4 });
    expect(sanitizeBar({ variant: "floating", width: 572, height: 60, radius: 4 })).toMatchObject({ variant: "floating", anchored: false });
  });
  it("spotlight: fixed capsule radius, never anchored, safe sizes", () => {
    const s = sanitizeBar({ variant: "spotlight", anchored: true, width: 736, height: 116, radius: 3 });
    expect(s).toMatchObject({ variant: "spotlight", anchored: false, width: 736, height: 116, radius: 30 });
    const bad = sanitizeBar({ variant: "spotlight", width: NaN, height: 10 });
    expect(bad.width).toBe(736);
    expect(bad.height).toBe(116);
  });
});
