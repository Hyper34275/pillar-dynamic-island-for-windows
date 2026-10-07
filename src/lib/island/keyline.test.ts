import { beforeEach, describe, expect, it } from "vitest";
import {
  KEYLINE_OFF_ABOVE,
  KEYLINE_ON_BELOW,
  getKeylineWanted,
  keylineShadow,
  lumaOf,
  nextKeyline,
  physicalPixel,
  resetKeyline,
  setBackdropLuminance,
} from "./keyline";

// Representative backdrops (sRGB 0..255) and whether a cold start (keyline off) draws one.
const BACKDROPS: Array<[string, [number, number, number], boolean]> = [
  ["white", [255, 255, 255], false],
  ["light gray", [200, 200, 200], false],
  ["mid gray", [128, 128, 128], false],
  ["dark gray #333", [51, 51, 51], false],
  ["dark gray #1c1c1c", [28, 28, 28], true],
  ["near black", [10, 10, 10], true],
  ["pure black", [0, 0, 0], true],
  ["navy", [0, 0, 128], true],
  ["purple", [128, 0, 128], false],
  ["dark purple", [60, 0, 60], true],
  ["colourful photo (mean)", [110, 130, 90], false],
  ["high-contrast photo (mean)", [90, 90, 90], false],
  ["dark title bar #202020", [32, 32, 32], false],
];

describe("keyline decision", () => {
  beforeEach(resetKeyline);

  it.each(BACKDROPS)("%s", (_name, [r, g, b], expected) => {
    expect(nextKeyline(lumaOf(r, g, b), false)).toBe(expected);
  });

  it("has hysteresis between the on and off thresholds", () => {
    const between = (KEYLINE_ON_BELOW + KEYLINE_OFF_ABOVE) / 2;
    expect(nextKeyline(between, false)).toBe(false);
    expect(nextKeyline(between, true)).toBe(true);
    expect(nextKeyline(KEYLINE_OFF_ABOVE + 0.01, true)).toBe(false);
    expect(nextKeyline(KEYLINE_ON_BELOW - 0.01, false)).toBe(true);
  });

  it("an unknown or invalid backdrop means no keyline", () => {
    expect(nextKeyline(null, true)).toBe(false);
    expect(nextKeyline(Number.NaN, true)).toBe(false);
  });

  it("the store follows readings with hysteresis and tells subscribers only about changes", () => {
    setBackdropLuminance(0.05);
    expect(getKeylineWanted()).toBe(true);
    setBackdropLuminance(0.15);
    expect(getKeylineWanted()).toBe(true);
    setBackdropLuminance(0.3);
    expect(getKeylineWanted()).toBe(false);
    setBackdropLuminance(0.15);
    expect(getKeylineWanted()).toBe(false);
    setBackdropLuminance(null);
    expect(getKeylineWanted()).toBe(false);
  });
});

describe("keyline geometry", () => {
  it("is one physical pixel at every Windows scale", () => {
    expect(physicalPixel(1)).toBe(1);
    expect(physicalPixel(1.25)).toBe(0.8);
    expect(physicalPixel(1.5)).toBe(0.667);
    expect(physicalPixel(1.75)).toBe(0.571);
    expect(physicalPixel(2)).toBe(0.5);
    expect(physicalPixel(0)).toBe(1);
  });

  it("is an inset zero-blur shadow so it follows the radius", () => {
    expect(keylineShadow(1.5, "rgba(255,255,255,0.14)")).toBe("inset 0 0 0 0.667px rgba(255,255,255,0.14)");
  });
});
