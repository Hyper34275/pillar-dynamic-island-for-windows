// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MARK_Y_PATH } from "./paths";
import { YuvalMark } from "./YuvalMark";
import { WORDMARK_ASPECT, YuvalWordmark } from "./YuvalWordmark";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("YuvalMark and YuvalWordmark", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  it("two marks on one page do not share gradient ids", () => {
    act(() =>
      root.render(
        <>
          <YuvalMark size={24} />
          <YuvalMark size={96} />
        </>,
      ),
    );
    const ids = Array.from(host.querySelectorAll("[id]")).map((el) => el.id);
    expect(ids.length).toBeGreaterThan(0);
    expect(new Set(ids).size).toBe(ids.length);
    for (const el of host.querySelectorAll("[fill^='url(#']")) {
      const target = /url\(#([^)]+)\)/.exec(el.getAttribute("fill") ?? "")?.[1];
      expect(host.querySelector(`[id="${target}"]`)).not.toBeNull();
    }
  });

  it("is decorative unless it has a title, and picks the small drawing up to 48 px", () => {
    act(() => root.render(<YuvalMark size={32} />));
    const small = host.querySelector("svg")!;
    expect(small.getAttribute("aria-hidden")).toBe("true");
    expect(small.getAttribute("width")).toBe("32");
    // The small drawing strokes its Y; the large one fills the font outline.
    expect(host.querySelector(`path[d="${MARK_Y_PATH}"]`)).toBeNull();
    act(() => root.render(<YuvalMark size={96} title="Yuval" />));
    const large = host.querySelector("svg")!;
    expect(large.getAttribute("aria-label")).toBe("Yuval");
    expect(host.querySelector(`path[d="${MARK_Y_PATH}"]`)).not.toBeNull();
  });

  it("the wordmark keeps its aspect ratio and colours by tone", () => {
    act(() => root.render(<YuvalWordmark height={40} tone="dark" />));
    const svg = host.querySelector("svg")!;
    expect(svg.getAttribute("height")).toBe("40");
    expect(Number(svg.getAttribute("width"))).toBeCloseTo(40 * WORDMARK_ASPECT, 5);
    expect(host.querySelector("path")!.getAttribute("fill")).toBe("#FFFFFF");
    act(() => root.render(<YuvalWordmark tone="current" />));
    expect(host.querySelector("path")!.getAttribute("fill")).toBe("currentColor");
    act(() => root.render(<YuvalWordmark tone="aurora" />));
    expect(host.querySelector("path")!.getAttribute("fill")).toMatch(/^url\(#.+-aurora\)$/);
    expect(host.querySelector("linearGradient")).not.toBeNull();
  });
});
