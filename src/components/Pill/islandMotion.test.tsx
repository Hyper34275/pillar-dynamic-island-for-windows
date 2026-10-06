// @vitest-environment jsdom
// The island's transitions with its springs actually running. Real time throughout: motion's
// frame loop is driven by requestAnimationFrame, which fake timers don't advance (and a loop
// left waiting on a fake frame stays stuck for the rest of the file, hence a file of its own).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PillShell } from "./PillShell";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const island = () => container.querySelector<HTMLElement>("[data-expanded]")!;
const layer = (name: string) => island().querySelector<HTMLElement>(`[data-layer="${name}"]`);
const opacityOf = (el: HTMLElement | null) => (el ? Number(el.style.opacity || "1") : 0);

async function wait(ms: number) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

async function waitUntil(done: () => boolean, timeoutMs = 3000) {
  const started = Date.now();
  while (!done() && Date.now() - started < timeoutMs) await wait(16);
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
  await waitUntil(() => opacityOf(layer("expanded")) === 1);
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
    // Nothing of it shows while the island is still the small pill.
    expect(opacityOf(expanded)).toBe(0);
    await waitUntil(() => opacityOf(layer("expanded")) === 1);
    expect(layer("compact")).toBeNull();
  });

  it("keeps the expanded content through the collapse (unclickable, hidden from assistive tech) and removes it once faded", async () => {
    await openIsland();
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    // Still there as the collapse begins: the island is not emptied before it shrinks.
    const leaving = layer("expanded")!;
    expect(leaving).not.toBeNull();
    expect(opacityOf(leaving)).toBeGreaterThan(0.5); // a frame or two may already have run
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
    await waitUntil(() => opacityOf(layer("expanded")) < 0.9);
    const caught = opacityOf(layer("expanded"));
    expect(caught).toBeGreaterThan(0);

    await act(async () => {
      island().click(); // the leaving layer doesn't take the click: the collapsed island does
    });
    expect(island().getAttribute("data-expanded")).toBe("true");
    // The same layer comes back from where it was (no remount at 0, no jump to 1).
    expect(opacityOf(layer("expanded"))).toBeGreaterThanOrEqual(caught - 0.05);
    expect(opacityOf(layer("expanded"))).toBeLessThan(1);
    await waitUntil(() => opacityOf(layer("expanded")) === 1);
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
});
