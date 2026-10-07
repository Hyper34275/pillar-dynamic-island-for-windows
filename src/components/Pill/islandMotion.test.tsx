// @vitest-environment jsdom
// The island's transitions with its springs actually running, frame by frame. The frame clock
// is the fake one (requestAnimationFrame and performance.now are faked with the timers), so
// every test steps whole 16 ms frames and sees every intermediate frame, whatever the machine's
// load. Each test drains its frame loops before the real clock comes back: a loop left waiting
// on a fake frame would otherwise stay stuck for the rest of the file.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PillShell } from "./PillShell";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  act(() => root.unmount());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  container.remove();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-expanded]")!;
const layer = (name: string) => island().querySelector<HTMLElement>(`[data-layer="${name}"]`);
const opacityOf = (el: HTMLElement | null) => (el ? Number(el.style.opacity || "1") : 0);
/** A part of the expanded island (header, body, dock): the expanded layer itself stays opaque and its parts fade. */
const part = (anchor: "top-start" | "center" | "bottom") => layer("expanded")?.querySelector<HTMLElement>(`[data-part="${anchor}"]`) ?? null;

async function wait(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

/** Steps one frame at a time until `done` (checking `eachFrame` on every frame on the way). */
async function waitUntil(done: () => boolean, timeoutMs = 3000, eachFrame?: () => void) {
  for (let t = 0; !done() && t < timeoutMs; t += 16) {
    await wait(16);
    eachFrame?.();
  }
  expect(done()).toBe(true);
}

/** Booted and open, with the expanded content fully faded in. */
async function openIsland() {
  await act(async () => {
    root.render(<PillShell />);
  });
  await wait(1000); // boot: dot -> morph -> interactive
  await act(async () => {
    island().click();
  });
  await waitUntil(() => opacityOf(part("bottom")) === 1);
}

describe("island transitions", () => {
  it("shows the expanded content only once the island has grown, at its final size (never squeezed)", async () => {
    await act(async () => {
      root.render(<PillShell />);
    });
    await wait(1000);
    await act(async () => {
      island().click();
    });
    const expanded = layer("expanded")!;
    // Laid out at its final size from the first frame; the island clips it while it grows.
    expect(expanded.style.width).toBe("404px");
    expect(expanded.style.height).toBe("420px");
    // On every frame while the island is still small, the body and the dock are not shown at
    // all; the header (riding the top-left corner) is the first to come.
    let smallFrames = 0;
    const checkFrame = () => {
      if (parseFloat(island().style.height) >= 140) return;
      smallFrames++;
      expect(opacityOf(part("center"))).toBe(0);
      expect(opacityOf(part("bottom"))).toBe(0);
    };
    checkFrame();
    await waitUntil(() => opacityOf(part("bottom")) === 1, 3000, checkFrame);
    expect(smallFrames).toBeGreaterThan(1);
    expect(layer("compact")).toBeNull();
    // At rest every part is exactly in place: no leftover offset.
    await waitUntil(() => island().style.width === "404px" && island().style.height === "420px");
    for (const anchor of ["top-start", "center", "bottom"] as const) expect(part(anchor)!.style.transform).toBe("none");
  });

  it("never draws a shape outside the compact..expanded range while opening and closing", async () => {
    await openIsland();
    const sizes: [number, number][] = [];
    const record = () => sizes.push([parseFloat(island().style.width), parseFloat(island().style.height)]);
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    await waitUntil(() => layer("expanded") === null && island().style.height === "34px", 3000, record);
    expect(sizes.length).toBeGreaterThan(5);
    const compactWidth = sizes[sizes.length - 1][0];
    for (const [w, h] of sizes) {
      expect(w).toBeGreaterThanOrEqual(compactWidth);
      expect(w).toBeLessThanOrEqual(404);
      expect(h).toBeGreaterThanOrEqual(34);
      expect(h).toBeLessThanOrEqual(420);
    }
    // shrinking only, frame after frame
    for (let i = 1; i < sizes.length; i++) {
      expect(sizes[i][0]).toBeLessThanOrEqual(sizes[i - 1][0] + 1e-6);
      expect(sizes[i][1]).toBeLessThanOrEqual(sizes[i - 1][1] + 1e-6);
    }
  });

  it("keeps the expanded content through the collapse (unclickable, hidden from assistive tech) and removes it once faded", async () => {
    await openIsland();
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    // Still there as the collapse begins: the island is not emptied before it shrinks.
    const leaving = layer("expanded")!;
    expect(leaving).not.toBeNull();
    expect(opacityOf(part("center"))).toBeGreaterThan(0.5); // a frame or two may already have run
    expect(leaving.getAttribute("aria-hidden")).toBe("true");
    expect(leaving.style.pointerEvents).toBe("none");
    expect(layer("compact")).not.toBeNull();

    await waitUntil(() => layer("expanded") === null);
    await waitUntil(() => opacityOf(layer("compact")) === 1);
  });

  it("reopening mid-collapse continues from what is on screen instead of starting over", async () => {
    await openIsland();
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    await waitUntil(() => opacityOf(part("center")) < 0.9);
    const caught = opacityOf(part("center"));
    expect(caught).toBeGreaterThan(0);

    await act(async () => {
      island().click(); // the leaving layer doesn't take the click: the collapsed island does
    });
    expect(island().getAttribute("data-expanded")).toBe("true");
    // The same layer comes back from where it was (no remount at 0, no jump to 1).
    expect(opacityOf(part("center"))).toBeGreaterThanOrEqual(caught - 0.05);
    expect(opacityOf(part("center"))).toBeLessThan(1);
    await waitUntil(() => opacityOf(part("bottom")) === 1);
  });

  it("switches tabs as one transition: one sliding highlight, and the outgoing panel cross-fades out without being the tabpanel", async () => {
    await openIsland();
    expect(container.querySelectorAll("[data-tab-indicator]")).toHaveLength(1);

    await act(async () => {
      container.querySelector<HTMLElement>("#tab-about")!.click();
    });
    // The selection is acknowledged at once; both panels are on screen during the cross-fade,
    // but only the incoming one is the tabpanel and only it takes the pointer.
    expect(container.querySelector('[role="tab"][aria-selected="true"]')!.id).toBe("tab-about");
    expect(container.querySelectorAll('[role="tabpanel"]')).toHaveLength(1);
    expect(container.querySelector('[role="tabpanel"]')!.id).toBe("panel-about");
    const outgoing = container.querySelector<HTMLElement>('[data-panel="calendar"]')!;
    expect(opacityOf(outgoing)).toBeGreaterThan(0.5);
    expect(outgoing.getAttribute("aria-hidden")).toBe("true");
    expect(outgoing.style.pointerEvents).toBe("none");

    await waitUntil(() => container.querySelector('[data-panel="calendar"]') === null);
    expect(container.querySelectorAll("[data-panel]")).toHaveLength(1);
    await waitUntil(() => opacityOf(container.querySelector('[data-panel="about"]')) === 1);
  });

  it("rapid tab clicks end on the last tab: one capsule at its slot, one panel, no stale transition winning", async () => {
    await openIsland();
    for (const id of ["notifications", "settings", "about", "calendar", "settings", "about"]) {
      await act(async () => {
        container.querySelector<HTMLElement>(`#tab-${id}`)!.click();
      });
      await wait(20);
    }
    expect(container.querySelector('[role="tab"][aria-selected="true"]')!.id).toBe("tab-about");
    await waitUntil(() => container.querySelectorAll("[data-panel]").length === 1);
    expect(container.querySelector("[data-panel]")!.getAttribute("data-panel")).toBe("about");
    await waitUntil(() => opacityOf(container.querySelector('[data-panel="about"]')) === 1);
    const capsules = container.querySelectorAll<HTMLElement>("[data-tab-indicator]");
    expect(capsules).toHaveLength(1);
    const aboutIndex = [...container.querySelectorAll('[role="tab"]')].findIndex((tab) => tab.id === "tab-about");
    await waitUntil(() => capsules[0].style.transform.includes(`translateX(${aboutIndex * 100}%)`));
  });

  it("closing during a tab change and reopening ends open on the last tab, at full size", async () => {
    await openIsland();
    await act(async () => {
      container.querySelector<HTMLElement>("#tab-settings")!.click();
    });
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    await wait(60);
    await act(async () => {
      island().click();
    });
    await waitUntil(() => island().style.width === "404px" && island().style.height === "420px");
    expect(island().getAttribute("data-expanded")).toBe("true");
    expect(container.querySelector('[role="tab"][aria-selected="true"]')!.id).toBe("tab-settings");
  });
});
