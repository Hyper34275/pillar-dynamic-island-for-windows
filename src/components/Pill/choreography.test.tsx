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
