// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PillShell } from "./PillShell";
import { TabBoundary } from "./TabBoundary";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 6, 15, 30));
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-expanded]")!;

async function mountBooted() {
  await act(async () => {
    root.render(<PillShell />);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1500); // boot: dot -> morph -> interactive
  });
}

describe("PillShell", () => {
  it("shows the date and weekday once booted, as a collapsed button", async () => {
    await mountBooted();
    expect(island().getAttribute("role")).toBe("button");
    expect(island().getAttribute("data-expanded")).toBe("false");
    expect(island().textContent).toContain("10/6");
    expect(island().textContent).toContain("Tuesday");
  });

  it("runs a single timer while collapsed and idle: the clock's", async () => {
    await mountBooted();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000); // let the debug-log flush and boot leftovers settle
    });
    // Measured: exactly the clock's day timer (<= 60 s). The reminder engine adds one more only
    // while a reminder is pending. A per-second or per-frame timer would raise this count or
    // re-arm inside the minute below.
    expect(vi.getTimerCount()).toBe(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(59_000);
    });
    expect(vi.getTimerCount()).toBe(1);
  });

  it("expands on click into exactly five tabs and switches between them", async () => {
    await mountBooted();
    await act(async () => {
      island().click();
    });
    expect(island().getAttribute("role")).toBe("dialog");

    const tabs = [...container.querySelectorAll<HTMLElement>('[role="tab"]')];
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Calendar", "Notifications", "Notes", "About", "Settings"]);
    expect(tabs.map((tab) => tab.tabIndex)).toEqual([0, -1, -1, -1, -1]); // roving tabindex

    // Calendar first.
    expect(container.querySelector('[role="tabpanel"]')!.textContent).toContain("Waiting for Outlook");
    expect(container.querySelector('[role="tab"][aria-selected="true"]')!.id).toBe("tab-calendar");

    // About: the clock (with the full date) between the computer name and the IP.
    await act(async () => {
      tabs[3].click();
    });
    expect(container.querySelector('[role="tab"][aria-selected="true"]')!.id).toBe("tab-about");
    expect(container.querySelector('[role="tabpanel"]')!.textContent).toContain("3:30");
    expect(container.querySelector('[role="tabpanel"]')!.textContent).toContain("Tuesday, October 6, 2026");
    expect(container.querySelector('[role="tabpanel"]')!.textContent).not.toContain("Diagnostics");

    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    });
    expect(container.querySelector('[role="tab"][aria-selected="true"]')!.id).toBe("tab-settings");
    expect(container.querySelector('[role="tabpanel"]')!.textContent).toContain("Diagnostics");
  });

  it("expands when the pointer rests on it and collapses shortly after the pointer leaves", async () => {
    await mountBooted();
    await act(async () => {
      island().dispatchEvent(new Event("pointerover", { bubbles: true }));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(130);
    });
    expect(island().getAttribute("data-view")).toBe("userExpanded");

    await act(async () => {
      island().dispatchEvent(new Event("pointerout", { bubbles: true }));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(520);
    });
    expect(island().getAttribute("data-view")).toBe("idle");
  });

  it("does not collapse on its own while pinned by a click and the pointer is still on it", async () => {
    await mountBooted();
    await act(async () => {
      island().dispatchEvent(new Event("pointerover", { bubbles: true }));
      island().click();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(island().getAttribute("data-view")).toBe("userExpanded");
  });

  it("collapses on Escape", async () => {
    await mountBooted();
    await act(async () => {
      island().click();
    });
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(island().getAttribute("data-expanded")).toBe("false");
  });

  it("rolls the collapsed date over at midnight without a restart", async () => {
    vi.setSystemTime(new Date(2026, 9, 6, 23, 59, 30));
    await mountBooted(); // boot takes ~0.7s of the 30s left
    expect(island().textContent).toContain("10/6");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(31_000);
    });
    expect(island().textContent).toContain("10/7");
    expect(island().textContent).toContain("Wednesday");
  });
});

describe("TabBoundary", () => {
  function Boom(): never {
    throw new TypeError("secret meeting subject");
  }

  it("contains a render error to its tab and shows Unavailable with APP-001", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    await act(async () => {
      root.render(
        <div>
          <p id="sibling">still here</p>
          <TabBoundary tab="calendar">
            <Boom />
          </TabBoundary>
        </div>
      );
    });
    consoleError.mockRestore();
    expect(container.querySelector("#sibling")!.textContent).toBe("still here");
    const alert = container.querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain("Unavailable");
    expect(alert.textContent).toContain("APP-001");
    expect(container.textContent).not.toContain("secret meeting subject");
  });
});
