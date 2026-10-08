// @vitest-environment jsdom
// The same content-ownership rules under the OS "show animations: off" profile (reduced motion):
// the shape still morphs (it is how the island says what it turned into), the hand-overs keep one
// owner and no empty shape, a reversal bends the motion, and the final state is exact. A file of
// its own because motion reads the media query once per module load.
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
const realMatchMedia = window.matchMedia;

beforeEach(() => {
  window.matchMedia = ((query: string) => ({
    matches: /prefers-reduced-motion/.test(query),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  w.__islandTrace?.stop();
  delete w.__ISLAND_FRAMES__;
  act(() => root.unmount());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  container.remove();
  vi.useRealTimers();
  window.matchMedia = realMatchMedia;
});

const island = () => container.querySelector<HTMLElement>("[data-view]")!;
const hit = () => island().closest("[data-island-hit]")!;
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));
const click = (el: Element) => act(() => void el.dispatchEvent(new MouseEvent("click", { bubbles: true })));
const escape = () => act(() => void document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
const frames = async (n: number, step = 16.7) => {
  for (let i = 0; i < n; i++) await ms(step);
};
const trace = () => w.__ISLAND_FRAMES__!;

async function mount() {
  await act(async () => {
    root.render(<PillShell />);
  });
  for (let t = 0; t < 1500; t += 50) await ms(50);
  expect(Math.round(parseFloat(island().style.height))).toBe(36);
  w.__ISLAND_FRAMES__ = [];
  w.__islandTrace!.start();
}

const cover = (f: IslandFrameRecord) => Math.max(f.compactOpacity, f.expandedOpacity, f.toastOpacity);
function longestEmptyRun(records: IslandFrameRecord[]) {
  let run = 0;
  let longest = 0;
  for (const f of records) {
    if (cover(f) < 0.3 && f.shellHeight > 60 && f.presentationProgress < 0.999) longest = Math.max(longest, ++run);
    else run = 0;
  }
  return longest;
}

describe("reduced motion: the same ownership rules, a tighter profile", () => {
  it("opens and closes with one owner per frame, ending exact", async () => {
    await mount();
    await click(hit());
    await frames(45);
    expect(island().dataset.view).toBe("userExpanded");
    const opening = trace().splice(0);
    // the tighter profile is really the one running (the full profile needs 14 frames to 95 %)
    expect(opening.findIndex((f) => f.presentationProgress >= 0.95)).toBeLessThan(14);
    escape();
    await frames(45);
    expect(island().dataset.view).toBe("idle");
    const closing = trace().splice(0);
    for (const records of [opening, closing]) {
      expect(records.flatMap((f) => f.violations)).toEqual([]);
      expect(longestEmptyRun(records)).toBeLessThanOrEqual(1);
    }
    const last = closing[closing.length - 1];
    expect([last.shellHeight, last.compactOpacity, last.expandedOpacity]).toEqual([compactTokens.height, 1, 0]);
    expect(Math.max(...opening.map((f) => f.shellHeight))).toBeLessThanOrEqual(panel.height + 0.5);
  });

  it.each([60, 120, 200])("a close reversed %s ms in: one owner per frame, never empty, ends open", async (at) => {
    await mount();
    await click(hit());
    await frames(45);
    trace().splice(0);
    escape();
    await frames(Math.round(at / 16.7));
    await click(hit());
    await frames(45);
    const records = trace().splice(0);
    expect(records.flatMap((f) => f.violations)).toEqual([]);
    expect(longestEmptyRun(records)).toBeLessThanOrEqual(1);
    expect(island().dataset.view).toBe("userExpanded");
    const last = records[records.length - 1];
    expect([last.headerOpacity, last.bodyOpacity, last.dockOpacity, last.compactOpacity]).toEqual([1, 1, 1, 0]);
  });

  it.each([60, 120, 200])("an opening reversed %s ms in: one owner per frame, never empty, ends closed", async (at) => {
    await mount();
    await click(hit());
    await frames(Math.round(at / 16.7));
    escape();
    await frames(45);
    const records = trace().splice(0);
    expect(records.flatMap((f) => f.violations)).toEqual([]);
    expect(longestEmptyRun(records)).toBeLessThanOrEqual(1);
    expect(island().dataset.view).toBe("idle");
    expect(records[records.length - 1].compactOpacity).toBe(1);
  });
});
