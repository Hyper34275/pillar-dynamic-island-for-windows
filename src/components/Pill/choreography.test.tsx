// @vitest-environment jsdom
// The island's content ownership through the real shell, frame by frame on the fake frame clock,
// read the way the runtime harness reads it (frameTrace.ts: the DOM's own opacities): opening,
// closing, reversing mid-way and tab spam never show two owners, never an empty shape for more
// than an instant, never the compact content inside a large shape.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { compact as compactTokens, panel } from "../../design/tokens";
import { PillShell } from "./PillShell";
import type { IslandFrameRecord } from "./frameTrace";

vi.mock("../../hooks/useSettings", async () => {
  const { SETTINGS_DEFAULTS: defaults } = await vi.importActual<typeof import("../../lib/ipc")>("../../lib/ipc");
  return { useSettings: () => ({ settings: defaults, loaded: true, update: async () => true }) };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type TraceWindow = Window & { __ISLAND_FRAMES__?: IslandFrameRecord[]; __islandTrace?: { start: () => void; stop: () => void } };
const w = window as TraceWindow;

let container: HTMLDivElement;
let root: Root;
let errors: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  errors = vi.spyOn(console, "error");
});

afterEach(async () => {
  w.__islandTrace?.stop();
  delete w.__ISLAND_FRAMES__;
  act(() => root.unmount());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  container.remove();
  errors.mockRestore();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-view]")!;
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));
const click = (el: Element) => act(() => void el.dispatchEvent(new MouseEvent("click", { bubbles: true })));

async function mount() {
  await act(async () => {
    root.render(<PillShell />);
  });
  // Boot (dot → pill) in small steps: one long act() would hold React's updates until its end.
  for (let t = 0; t < 1500; t += 50) await ms(50);
  expect(Math.round(parseFloat(island().style.height))).toBe(36);
  w.__ISLAND_FRAMES__ = [];
  w.__islandTrace!.start();
}

async function frames(n: number, step = 16.7) {
  for (let i = 0; i < n; i++) await ms(step);
}
const trace = () => w.__ISLAND_FRAMES__!;
const ownershipViolations = () => errors.mock.calls.filter((args: unknown[]) => String(args[0]).includes("ownership violation"));

const COMPACT_H = compactTokens.height;
/** "Large shape": more than a quarter of the way from the pill to the panel. */
const LARGE_H = COMPACT_H + 0.25 * (panel.height - COMPACT_H);

function check(records: IslandFrameRecord[]) {
  const moving = records.filter((f) => f.presentationProgress < 0.999);
  return {
    violations: records.flatMap((f) => f.violations),
    // the compact content readable inside a large shape
    compactInLargeShell: records.filter((f) => f.compactOpacity > 0.35 && f.shellHeight > LARGE_H).length,
    // the most visible content while the shape moves (no near-empty black shape)
    floor: Math.min(...moving.map((f) => Math.max(f.compactOpacity, f.expandedOpacity, f.toastOpacity)), 1),
    emptyFrames: moving.filter((f) => Math.max(f.compactOpacity, f.expandedOpacity, f.toastOpacity) < 0.08).length,
  };
}

describe("compact ↔ expanded ownership", () => {
  it.each([16.7, 31.2])("opens and closes with one owner per frame (%s ms frames)", async (step) => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(Math.ceil(700 / step), step);
    expect(island().dataset.view).toBe("userExpanded");
    const opening = trace().splice(0);
    act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await frames(Math.ceil(700 / step), step);
    expect(island().dataset.view).toBe("idle");
    const closing = trace().splice(0);
    for (const records of [opening, closing]) {
      const result = check(records);
      expect(result.violations).toEqual([]);
      expect(result.compactInLargeShell).toBe(0);

      expect(result.emptyFrames).toBeLessThanOrEqual(1);
    }
    // Ends with the compact content alone, fully shown.
    const last = closing[closing.length - 1];
    expect(last.compactOpacity).toBe(1);
    expect(last.expandedOpacity).toBe(0);
    expect(ownershipViolations()).toEqual([]);
  });

  it.each([60, 100, 160])("reverses an opening %s ms in from where it is: no restart, no double exposure", async (at) => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(Math.round(at / 16.7));
    const before = trace()[trace().length - 1];
    act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await frames(1);
    const after = trace()[trace().length - 1];
    // The compact content continues from its current opacity (it never jumps back to 0 or 1).
    expect(Math.abs(after.compactOpacity - before.compactOpacity)).toBeLessThan(0.35);
    await frames(45);
    const result = check(trace());
    expect(result.violations).toEqual([]);
    expect(result.compactInLargeShell).toBe(0);
    expect(island().dataset.view).toBe("idle");
    expect(trace()[trace().length - 1].compactOpacity).toBe(1);
  });
});

/** The longest run of consecutive frames in which a shape taller than `above` px shows almost nothing. */
function longestEmptyRun(records: IslandFrameRecord[], above = 60, below = 0.3) {
  let run = 0;
  let longest = 0;
  for (const f of records) {
    if (Math.max(f.compactOpacity, f.expandedOpacity, f.toastOpacity) < below && f.shellHeight > above && f.presentationProgress < 0.999) longest = Math.max(longest, ++run);
    else run = 0;
  }
  return longest;
}

/** The first frame whose transition progress has reached `p`. */
const atProgress = (records: IslandFrameRecord[], p: number) => records.find((f) => f.presentationProgress >= p - 1e-9) ?? records[records.length - 1];

describe("content follows the shape's progress (0 / 20 / 40 / 60 / 80 / 100 %)", () => {
  it("opening: the pill's content leaves, the header takes the top row, then the body and the dock; ends with the panel alone", async () => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(60);
    const records = trace().splice(0);
    const at = (p: number) => atProgress(records, p);
    expect(at(0).compactOpacity).toBe(1);
    expect(at(0).expandedOpacity).toBe(0);
    // 20 %: the pill's content is nearly gone, the header has begun, the body has not
    expect(at(0.2).compactOpacity).toBeLessThan(0.5);
    expect(at(0.2).bodyOpacity).toBe(0);
    // 40 %: the pill's content is gone, the header is readable, the body is just starting
    expect(at(0.4).compactOpacity).toBe(0);
    expect(at(0.4).headerOpacity).toBeGreaterThan(0.6);
    // 60 %: the body is established
    expect(at(0.6).bodyOpacity).toBeGreaterThan(0.2);
    // 80 %, 100 %: everything of the panel, nothing of the pill
    expect(at(0.8).bodyOpacity).toBeGreaterThan(0.8);
    expect(at(1).headerOpacity).toBe(1);
    expect(at(1).bodyOpacity).toBe(1);
    expect(at(1).dockOpacity).toBe(1);
    expect(at(1).compactOpacity).toBe(0);
    expect(check(records).violations).toEqual([]);
  });

  it("closing: the header leaves first, the body holds, the pill's content arrives in the last quarter; ends with the pill alone", async () => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(60);
    trace().splice(0);
    act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await frames(60);
    const records = trace().splice(0);
    const at = (p: number) => atProgress(records, p);
    expect(at(0.2).bodyOpacity).toBeGreaterThan(0.8);
    expect(at(0.2).compactOpacity).toBe(0);
    // 60 %: the header is gone from the top row, the body still holds
    expect(at(0.6).headerOpacity).toBe(0);
    expect(at(0.6).bodyOpacity).toBeGreaterThan(0.3);
    // 80 %: the hand-over: the body is faint, the pill's content is coming in, never both readable
    expect(at(0.8).bodyOpacity).toBeLessThan(0.35);
    expect(at(0.8).compactOpacity).toBeGreaterThan(0);
    expect(at(0.8).compactOpacity).toBeLessThan(0.6);
    expect(at(1).compactOpacity).toBe(1);
    expect(at(1).expandedOpacity).toBe(0);
    expect(check(records).violations).toEqual([]);
    // the content never drops below ~0.25 while the shape is large (no near-empty black shell)
    expect(check(records).floor).toBeGreaterThan(0.2);
  });
});

describe("a transition reversed part-way keeps one owner and never shows an empty shape", () => {
  it.each([40, 60, 100, 160, 220])("opening reversed %s ms in: the hand-over is measured along the shape's whole path", async (at) => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(Math.round(at / 16.7));
    act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await frames(50);
    const records = trace().splice(0);
    const result = check(records);
    expect(result.violations).toEqual([]);
    // Before the fix a reversal at 60 ms left a shell of 70-200 px with nothing in it for ~5 frames.
    expect(longestEmptyRun(records)).toBeLessThanOrEqual(1);
    expect(result.floor).toBeGreaterThan(0.2);
    expect(island().dataset.view).toBe("idle");
    expect(records[records.length - 1].compactOpacity).toBe(1);
    expect(records[records.length - 1].expandedOpacity).toBe(0);
  });

  it.each([60, 120, 200])("closing reversed %s ms in: the panel comes back, never two readable owners, never empty", async (at) => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(60);
    trace().splice(0);
    act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await frames(Math.round(at / 16.7));
    await click(island().closest("[data-island-hit]")!);
    await frames(60);
    const records = trace().splice(0);
    const result = check(records);
    expect(result.violations).toEqual([]);
    expect(longestEmptyRun(records)).toBeLessThanOrEqual(1);
    expect(result.floor).toBeGreaterThan(0.2);
    expect(island().dataset.view).toBe("userExpanded");
    const last = records[records.length - 1];
    expect([last.headerOpacity, last.bodyOpacity, last.dockOpacity, last.compactOpacity]).toEqual([1, 1, 1, 0]);
  });

  it("open → close → open within 200 ms ends open, the last request wins, and the shape stays inside its bounds", async () => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(3);
    act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    await frames(4);
    await click(island().closest("[data-island-hit]")!);
    await frames(60);
    const records = trace().splice(0);
    expect(island().dataset.view).toBe("userExpanded");
    expect(check(records).violations).toEqual([]);
    expect(Math.min(...records.map((f) => f.shellHeight))).toBeGreaterThanOrEqual(compactTokens.height - 0.5);
    expect(Math.max(...records.map((f) => f.shellHeight))).toBeLessThanOrEqual(panel.height + 0.5);
    expect(longestEmptyRun(records)).toBeLessThanOrEqual(1);
  });
});

describe("tab spam inside the open island", () => {
  it.each([16.7, 31.2])("7 clicks in ~450 ms: ≤ 2 tab layers, one readable title, the last target wins (%s ms frames)", async (step) => {
    await mount();
    await click(island().closest("[data-island-hit]")!);
    await frames(Math.ceil(700 / step), step);
    trace().splice(0);
    const tabs = [...container.querySelectorAll<HTMLElement>('[role="tab"]')];
    const order = [1, 3, 2, 4, 0, 3, 2];
    for (const index of order) {
      await click(tabs[index]);
      await frames(Math.max(1, Math.round(65 / step)), step);
    }
    await frames(Math.ceil(600 / step), step);
    const records = trace();
    expect(Math.max(...records.map((f) => f.tabLayers))).toBeLessThanOrEqual(2);
    expect(records.flatMap((f) => f.violations)).toEqual([]);
    const last = tabs[order[order.length - 1]];
    expect(last.getAttribute("aria-selected")).toBe("true");
    const panels = container.querySelectorAll("[data-panel]");
    expect(panels).toHaveLength(1);
    expect(panels[0].getAttribute("data-panel")).toBe(last.id.replace("tab-", ""));
  });
});
