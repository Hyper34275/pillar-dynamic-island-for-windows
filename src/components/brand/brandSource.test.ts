// @ts-nocheck
// Build-tooling test (runs in Node): the repo has no @types/node, which `tsc` would need for the node: imports.
// The brand geometry in paths.ts / YuvalMark.tsx is copied from the SVG sources; this fails when they drift apart.
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { MARK_Y_PATH, WORDMARK_B_PATH, WORDMARK_VIEWBOX } from "./paths";

const root = resolve(__dirname, "..", "..", "..");
const brand = (name: string) => readFileSync(join(root, "assets", "brand", name), "utf8");

describe("brand geometry follows the SVG sources in assets/brand", () => {
  it("the mark's Y is the Y of icon-2.svg", () => {
    expect(brand("icon-2.svg")).toContain(`<path d="${MARK_Y_PATH}" fill="url(#yg)"`);
  });

  it("the wordmark is wordmark-b.svg, in all three colourways", () => {
    for (const file of ["wordmark-b.svg", "wordmark-b-dark.svg", "wordmark-b-light.svg"]) {
      const svg = brand(file);
      expect(svg).toContain(`<path d="${WORDMARK_B_PATH}"`);
      expect(svg).toContain(`viewBox="${WORDMARK_VIEWBOX}"`);
    }
  });

  it("the small mark's Y skeleton is the one in icon-2-small.svg", () => {
    const svg = brand("icon-2-small.svg");
    expect(svg).toContain("M668 268C640 392 568 528 488 640C444 702 392 756 322 764");
    expect(svg).toContain("M352 286C360 398 424 476 524 508");
  });
});
