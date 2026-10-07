// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Ringer } from "../../lib/island/state";
import { RingerPill } from "./RingerPill";

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

const show = (silent: boolean) => {
  act(() => root.render(<RingerPill ringer={{ silent, phase: "start" } as Ringer} />));
  return container.querySelector<HTMLElement>('[data-layer="ringer"]')!;
};

describe("RingerPill", () => {
  it("shows the silent state in a calm grey, never the destructive red (muted is not dangerous)", () => {
    const silent = show(true);
    expect(silent.style.color).toBe("var(--ci-fg-secondary)");
    expect(silent.style.color).not.toContain("destructive");
    expect(show(false).style.color).toBe("var(--ci-fg)");
  });
});
