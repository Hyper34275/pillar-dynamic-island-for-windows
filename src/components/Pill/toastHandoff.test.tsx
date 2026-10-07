// @vitest-environment jsdom
// Toast → toast payload handoff, frame by frame on the fake frame clock: at most two payloads,
// never two readable at once, the latest notification wins, and a new payload waits for the
// shell that has to hold it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { motionValue, type MotionValue } from "motion/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IslandNotification } from "../../lib/ipc";
import { ShellContext, type Shell } from "./drivenTransition";
import { NotificationToast } from "./NotificationToast";
import { toastLayout } from "./toastLayout";

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

const note = (id: number, appName: string, title: string, body = ""): IslandNotification => ({ id, appName, title, body, timestamp: 0, aumid: null });
const teamsHe = note(1, "Microsoft Teams", "דנה כהן", "שלחה לך הודעה חדשה");
const snipping = note(2, "Snipping Tool", "Screenshot saved", "Saved to your Pictures folder");
const outlook = note(3, "Outlook", "Quarterly review", "Room 4 · 15:00");
const slack = note(4, "Slack", "Build finished", "");

const noop = () => {};
let shell: Shell | null = null;

function render(notification: IslandNotification, reducedMotion = false) {
  act(() => {
    root.render(
      <ShellContext.Provider value={shell}>
        <NotificationToast
          notification={notification}
          onDismiss={noop}
          onActivate={noop}
          targetSize={shell ? toastLayout(notification).size : undefined}
          reducedMotion={reducedMotion}
        />
      </ShellContext.Provider>
    );
  });
}

const payloads = () => [...container.querySelectorAll<HTMLElement>('[data-layer="notification"]')];
const opacity = (el: HTMLElement) => Number(el.style.opacity === "" ? "1" : el.style.opacity);
const titleOf = (el: HTMLElement) => el.querySelector(".text-headline")?.textContent ?? "";

interface Sample {
  count: number;
  byTitle: Record<string, number>;
}
const sample = (): Sample => {
  const list = payloads();
  return { count: list.length, byTitle: Object.fromEntries(list.map((el) => [titleOf(el), opacity(el)])) };
};

async function frames(n: number, ms: number, onFrame: (s: Sample) => void) {
  for (let i = 0; i < n; i++) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
    onFrame(sample());
  }
}

/** Two payloads are both "readable" when each is above this opacity. */
const READABLE = 0.35;
const bothReadable = (s: Sample) => Object.values(s.byTitle).filter((o) => o > READABLE).length > 1;

describe("toast payload handoff", () => {
  beforeEach(() => {
    shell = null;
  });

  it.each([16.7, 31.2])("replaces Teams (Hebrew) with Snipping Tool (English) with no double exposure (%s ms frames)", async (ms) => {
    render(teamsHe);
    await frames(10, ms, () => {});
    expect(payloads()).toHaveLength(1);
    render(snipping);
    let maxLayers = 0;
    let readableOverlap = 0;
    let lastOld = 1;
    await frames(40, ms, (s) => {
      maxLayers = Math.max(maxLayers, s.count);
      if (bothReadable(s)) readableOverlap++;
      const old = s.byTitle[teamsHe.title] ?? 0;
      // The old payload never comes back once the new one was chosen.
      expect(old).toBeLessThanOrEqual(lastOld + 1e-9);
      lastOld = old;
    });
    expect(maxLayers).toBeLessThanOrEqual(2);
    expect(readableOverlap).toBe(0);
    const end = payloads();
    expect(end).toHaveLength(1);
    expect(titleOf(end[0])).toBe(snipping.title);
    expect(opacity(end[0])).toBe(1);
    expect(end[0].getAttribute("aria-hidden")).toBeNull();
  });

  it("keeps at most two payloads and lets the latest win when four arrive 30 ms apart", async () => {
    render(teamsHe);
    await frames(10, 16.7, () => {});
    let maxLayers = 0;
    let readableOverlap = 0;
    for (const next of [snipping, outlook, slack]) {
      render(next);
      maxLayers = Math.max(maxLayers, payloads().length);
      await frames(2, 15, (s) => {
        maxLayers = Math.max(maxLayers, s.count);
        if (bothReadable(s)) readableOverlap++;
      });
    }
    await frames(40, 16.7, (s) => {
      maxLayers = Math.max(maxLayers, s.count);
      if (bothReadable(s)) readableOverlap++;
    });
    expect(maxLayers).toBeLessThanOrEqual(2);
    expect(readableOverlap).toBe(0);
    expect(payloads().map(titleOf)).toEqual([slack.title]);
  });

  it("makes only the arriving payload interactive and exposed", async () => {
    render(teamsHe);
    await frames(10, 16.7, () => {});
    render(snipping);
    await frames(2, 16.7, () => {});
    const [leaving, arriving] = payloads();
    expect(leaving.getAttribute("data-payload")).toBe("leaving");
    expect(leaving.getAttribute("aria-hidden")).toBe("true");
    expect(leaving.style.pointerEvents).toBe("none");
    expect(arriving.getAttribute("data-payload")).toBe("shown");
    expect(arriving.getAttribute("aria-hidden")).toBeNull();
  });

  it("updates the same notification inline, with no handoff", async () => {
    render(teamsHe);
    await frames(10, 16.7, () => {});
    render({ ...teamsHe, body: "עדכון" });
    expect(payloads()).toHaveLength(1);
    expect(opacity(payloads()[0])).toBe(1);
    expect(payloads()[0].textContent).toContain("עדכון");
  });

  it("does not drift in reduced motion", async () => {
    render(teamsHe, true);
    await frames(10, 16.7, () => {});
    render(snipping, true);
    await frames(30, 16.7, () => {
      for (const el of payloads()) expect(el.style.transform === "" || el.style.transform === "none").toBe(true);
    });
    expect(payloads().map(titleOf)).toEqual([snipping.title]);
  });

  it("waits for the shell: a wider toast's payload stays hidden while the shell still has the old width", async () => {
    const a = toastLayout(slack).size;
    const b = toastLayout(snipping).size;
    expect(b.width).toBeGreaterThan(a.width + 20);
    const width: MotionValue<number> = motionValue(a.width);
    const height: MotionValue<number> = motionValue(a.height);
    shell = { transition: { transition: { from: [], to: [] }, drivers: [] }, width, height };
    render(slack);
    await frames(10, 16.7, () => {});
    render(snipping);
    // The handoff clock runs, but the shell has not moved: the old one leaves, the new one waits.
    await frames(20, 16.7, () => {});
    const waiting = sample();
    expect(waiting.byTitle[snipping.title] ?? 0).toBeLessThan(0.05);
    // The shell gets there: the new payload takes over.
    act(() => {
      width.set(b.width);
      height.set(b.height);
    });
    await frames(5, 16.7, () => {});
    expect(payloads().map(titleOf)).toEqual([snipping.title]);
    expect(opacity(payloads()[0])).toBe(1);
  });
});
