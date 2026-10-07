// The panel's preferred size, the monitor's limits, the stage window and the top inset.
import rustMonitors from "../../../src-tauri/src/monitors.rs?raw";
import { afterEach, describe, expect, it } from "vitest";
import { NO_LIMITS, getIslandLimits, parseIslandLimits, setIslandLimits, subscribeIslandLimits, type IslandLimits } from "../../lib/island/limits";
import { dock, panel } from "../../design/tokens";
import { compactSize, expandedSize, ISLAND_TOP_INSET, limitSize, meetingAlertSize, PANEL_MIN_HEIGHT, pillDimensions } from "./animations";
import { maxShapeSize, stageSize } from "./usePillGeometry";
import { toastMaxSize } from "./toastLayout";

const limits = (maxWidth: number, maxHeight: number): IslandLimits => ({ maxWidth, maxHeight, scale: 1 });

afterEach(() => setIslandLimits(NO_LIMITS));

describe("expandedSize", () => {
  it("is the preferred 400x440 when nothing limits it, with or without the store", () => {
    expect(expandedSize()).toEqual(pillDimensions.expanded);
    expect(expandedSize(NO_LIMITS)).toEqual(pillDimensions.expanded);
    expect(expandedSize(limits(1896, 1020))).toEqual(pillDimensions.expanded);
  });

  it("shrinks to the monitor: width = min(preferred, maxWidth), height = min(preferred, maxHeight)", () => {
    expect(expandedSize(limits(300, 440))).toMatchObject({ width: 300, height: 440 });
    // 1280x720 at 150%: 829.33 wide, 428 high. Fractional DIPs stay fractional (Rust snaps pixels).
    expect(expandedSize(limits(829.33, 428))).toMatchObject({ width: 400, height: 428 });
    expect(expandedSize(limits(300.5, 400.25))).toMatchObject({ width: 300.5, height: 400.25 });
    expect(expandedSize(limits(300, 400)).radius).toBe(panel.radius);
  });

  it("never goes below the least usable height: header + dock + one card", () => {
    expect(PANEL_MIN_HEIGHT).toBeGreaterThan(panel.headerHeight + dock.height);
    expect(PANEL_MIN_HEIGHT).toBeLessThan(panel.height);
    expect(expandedSize(limits(400, 50)).height).toBe(PANEL_MIN_HEIGHT);
    expect(expandedSize(limits(400, PANEL_MIN_HEIGHT + 10)).height).toBe(PANEL_MIN_HEIGHT + 10);
  });

  it("follows the shared store when called with no argument (PanelFrame)", () => {
    setIslandLimits(limits(350, 380));
    expect(expandedSize()).toMatchObject({ width: 350, height: 380 });
    setIslandLimits(NO_LIMITS);
    expect(expandedSize()).toEqual(pillDimensions.expanded);
  });
});

describe("stageSize", () => {
  it("holds every shape plus the island's gap below the screen's top edge", () => {
    const stage = stageSize(NO_LIMITS);
    const shape = maxShapeSize(NO_LIMITS);
    expect(stage.height).toBe(shape.height + ISLAND_TOP_INSET);
    expect(stage.width).toBe(shape.width);
    for (const size of [expandedSize(NO_LIMITS), meetingAlertSize(2, true, true), toastMaxSize(), compactSize(1000)]) {
      expect(stage.width).toBeGreaterThanOrEqual(size.width);
      expect(stage.height).toBeGreaterThanOrEqual(size.height + ISLAND_TOP_INSET);
    }
  });

  it("uses the same limited sizes: it never exceeds the work area the limits describe", () => {
    // 1280x720 at 150%: 829.33 x 428 DIP (the 8 DIP gap is inside the 440 the monitor offers).
    const l = limits(829.33, 428);
    const stage = stageSize(l);
    expect(stage.height).toBeLessThanOrEqual(l.maxHeight + ISLAND_TOP_INSET);
    expect(stage.width).toBeLessThanOrEqual(l.maxWidth);
    // a very small screen: even alerts and toasts are held to it
    const tiny = limits(320, 300);
    const small = stageSize(tiny);
    expect(small.width).toBeLessThanOrEqual(320);
    expect(small.height).toBeLessThanOrEqual(Math.max(PANEL_MIN_HEIGHT, 300) + ISLAND_TOP_INSET);
    expect(limitSize(meetingAlertSize(2, true, true), tiny).width).toBeLessThanOrEqual(320);
    expect(limitSize(toastMaxSize(), tiny).height).toBeLessThanOrEqual(300);
  });

  it("reads the shared store without an argument", () => {
    setIslandLimits(limits(320, 300));
    expect(stageSize()).toEqual(stageSize(limits(320, 300)));
  });
});

describe("limitSize", () => {
  it("returns the same object when it fits and a clamped one (radius kept a pill) when it does not", () => {
    const size = { width: 200, height: 40, radius: 20 };
    expect(limitSize(size, limits(500, 500))).toBe(size);
    expect(limitSize(size, limits(150, 30))).toEqual({ width: 150, height: 30, radius: 15 });
  });
});

describe("no even-DIP rounding", () => {
  it("keeps content widths as measured (fractional DIPs)", () => {
    const c = pillDimensions.compact;
    const content = 100.37; // between the minimum and the maximum once the padding is added
    expect(content + c.paddingX * 2).toBeGreaterThan(c.minWidth);
    expect(content + c.paddingX * 2).toBeLessThan(c.maxWidth);
    expect(compactSize(content).width).toBeCloseTo(content + c.paddingX * 2, 10);
  });
});

describe("top inset", () => {
  it("equals ISLAND_TOP_INSET in the backend (monitors.rs)", () => {
    const match = /pub const ISLAND_TOP_INSET: f64 = ([0-9.]+);/.exec(rustMonitors);
    expect(match).not.toBeNull();
    expect(Number(match![1])).toBe(ISLAND_TOP_INSET);
  });
});

describe("limits store", () => {
  it("parses only usable backend answers", () => {
    expect(parseIslandLimits({ maxWidth: 829.33, maxHeight: 428, scale: 1.5 })).toEqual({ maxWidth: 829.33, maxHeight: 428, scale: 1.5 });
    expect(parseIslandLimits({ maxWidth: 800, maxHeight: 400 })).toEqual({ maxWidth: 800, maxHeight: 400, scale: 1 });
    for (const bad of [null, undefined, 5, "x", {}, { maxWidth: 0, maxHeight: 400 }, { maxWidth: 800, maxHeight: -1 }, { maxWidth: NaN, maxHeight: 4 }, { maxWidth: "8", maxHeight: 4 }]) {
      expect(parseIslandLimits(bad)).toBeNull();
    }
  });

  it("notifies on change only, and the snapshot is stable", () => {
    let calls = 0;
    const off = subscribeIslandLimits(() => calls++);
    setIslandLimits(limits(500, 400));
    const first = getIslandLimits();
    setIslandLimits(limits(500, 400)); // same numbers: no notification, same object
    expect(calls).toBe(1);
    expect(getIslandLimits()).toBe(first);
    off();
    setIslandLimits(limits(600, 400));
    expect(calls).toBe(1);
  });
});
