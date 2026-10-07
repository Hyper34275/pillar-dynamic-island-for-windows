// @vitest-environment jsdom
// The closed island follows Windows' Do not disturb both ways: the slashed bell comes and goes.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const windows = vi.hoisted(() => ({ on: false }));
vi.mock("../../lib/island/dnd", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/island/dnd")>();
  const store = actual.createDoNotDisturb({ get: async () => windows.on, set: async (on) => (windows.on = on) });
  return {
    ...actual,
    doNotDisturb: store,
    useDoNotDisturb: (s = store) => actual.useDoNotDisturb(s),
    useDoNotDisturbSync: (s = store) => actual.useDoNotDisturbSync(s),
  };
});

import { doNotDisturb } from "../../lib/island/dnd";
import { PillShell } from "./PillShell";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 6, 15, 30));
  windows.on = false;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const slashed = () => !!container.querySelector('[data-layer="compact"] path[d="M3 3l18 18"]');

async function windowsSays(on: boolean) {
  windows.on = on;
  await act(async () => {
    await doNotDisturb.refresh();
    await vi.advanceTimersByTimeAsync(600);
  });
}

describe("the closed island and Do not disturb", () => {
  it("shows the slashed bell while it is on and drops it when it is turned off, every time", async () => {
    await act(async () => {
      root.render(<PillShell />);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    expect(slashed()).toBe(false);
    for (const on of [true, false, true, false]) {
      await windowsSays(on);
      expect(slashed()).toBe(on);
    }
  });
});
