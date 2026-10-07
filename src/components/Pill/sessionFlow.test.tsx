// @vitest-environment jsdom
// Notification sessions end to end through the real shell, frame by frame on the fake frame
// clock: a burst opens the island ONCE, shows every notification one after another with a clean
// payload handoff, and closes ONCE; a notification arriving while the island closes reverses the
// close (no compact resting frame); an open panel is never stolen by an ordinary notification.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { compact as compactTokens } from "../../design/tokens";
import { silence } from "../../lib/island/silence";
import { PillShell } from "./PillShell";

const backend = vi.hoisted(() => ({
  handlers: new Map<string, (payload: unknown) => void>(),
}));

vi.mock("../../lib/ipc", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/ipc")>();
  return {
    ...original,
    onEvent: (name: string, handler: (payload: unknown) => void) => {
      backend.handlers.set(name, handler);
      return () => backend.handlers.delete(name);
    },
    ipc: { ...original.ipc, activateNotification: vi.fn() },
  };
});

vi.mock("../../hooks/useSettings", async () => {
  const { SETTINGS_DEFAULTS: defaults } = await vi.importActual<typeof import("../../lib/ipc")>("../../lib/ipc");
  return { useSettings: () => ({ settings: defaults, loaded: true, update: async () => true }) };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;
let nextId = 1000;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 7, 10, 30));
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
  silence.clear();
  backend.handlers.clear();
  vi.useRealTimers();
});

const island = () => container.querySelector<HTMLElement>("[data-view]")!;
const ms = (n: number) => act(async () => void (await vi.advanceTimersByTimeAsync(n)));
const notify = (appName: string, title: string, id = nextId++) =>
  act(async () => {
    backend.handlers.get("notification-received")!({ id, appName, title, body: "", timestamp: Date.now(), aumid: null });
  });

async function mount() {
  await act(async () => {
    root.render(<PillShell />);
  });
  // Boot (dot → pill) in small steps: one long act() would hold React's updates until its end.
  for (let t = 0; t < 1500; t += 50) await ms(50);
  expect(Math.round(parseFloat(island().style.height))).toBe(36);
}

interface Frame {
  t: number;
  view: string;
  height: number;
  payloads: { title: string; opacity: number }[];
}

const opacityOf = (el: HTMLElement) => Number(el.style.opacity === "" ? "1" : el.style.opacity);

function frame(t: number): Frame {
  const el = island();
  return {
    t,
    view: el.dataset.view!,
    height: parseFloat(el.style.height),
    payloads: [...el.querySelectorAll<HTMLElement>('[data-layer="notification"]')].map((p) => ({
      title: p.querySelector(".text-headline")?.textContent ?? "",
      // The payload's own opacity times the toast stage's.
      opacity: opacityOf(p) * opacityOf(p.closest<HTMLElement>('[data-layer="toast"]')!),
    })),
  };
}

/** Steps `total` ms in frames of `step` ms, recording every frame; `at` injects things on the way. */
async function record(total: number, step: number, at: Record<number, () => Promise<void>> = {}) {
  // The frame before anything is injected: the baseline every open / close is counted from.
  const frames: Frame[] = [frame(-step)];
  const due = Object.keys(at)
    .map(Number)
    .sort((a, b) => a - b);
  for (let t = 0; t <= total; t += step) {
    while (due.length && due[0] <= t) await at[due.shift()!]();
    await ms(step);
    frames.push(frame(t));
  }
  return frames;
}

const COMPACT_HEIGHT = compactTokens.height;
/** The shape's opens and closes: leaving the compact height, and coming back to it. */
function shellCycles(frames: Frame[]) {
  let opens = 0;
  let closes = 0;
  let open = false;
  for (const f of frames) {
    const atCompact = Math.abs(f.height - COMPACT_HEIGHT) < 0.5;
    if (!open && !atCompact) {
      opens++;
      open = true;
    } else if (open && atCompact) {
      closes++;
      open = false;
    }
  }
  return { opens, closes };
}
function viewCycles(frames: Frame[]) {
  let opens = 0;
  let closes = 0;
  for (let i = 1; i < frames.length; i++) {
    if (frames[i].view === "notification" && frames[i - 1].view !== "notification") opens++;
    if (frames[i].view !== "notification" && frames[i - 1].view === "notification") closes++;
  }
  return { opens, closes };
}
const READABLE = 0.35;
function payloadChecks(frames: Frame[]) {
  return {
    maxPayloads: Math.max(...frames.map((f) => f.payloads.length)),
    bothReadableFrames: frames.filter((f) => f.payloads.filter((p) => p.opacity > READABLE).length > 1).length,
    shownInOrder: frames
      .map((f) => f.payloads.find((p) => p.opacity > 0.9)?.title)
      .filter((title): title is string => !!title)
      .filter((title, i, all) => i === 0 || all[i - 1] !== title),
  };
}

describe("a burst of notifications is one session", () => {
  it.each([16.7, 31.2])("Slack → WhatsApp → Teams → Jira → Outlook, 100 ms apart: one open, all five in order, one close (%s ms frames)", async (step) => {
    await mount();
    const apps = ["Slack", "WhatsApp", "Teams", "Jira", "Outlook"];
    const at = Object.fromEntries(apps.map((app, i) => [i * 100, () => notify(app, `${app} message`)]));
    const frames = await record(20_000, step, at);
    expect(viewCycles(frames)).toEqual({ opens: 1, closes: 1 });
    expect(shellCycles(frames)).toEqual({ opens: 1, closes: 1 });
    const checks = payloadChecks(frames);
    expect(checks.maxPayloads).toBeLessThanOrEqual(2);
    expect(checks.bothReadableFrames).toBe(0);
    expect(checks.shownInOrder).toEqual(apps.map((app) => `${app} message`));
    expect(frames[frames.length - 1].view).toBe("idle");
  });

  it("10 notifications in under a second: one open, one close, order kept, the session ends", async () => {
    await mount();
    const at = Object.fromEntries(Array.from({ length: 10 }, (_, i) => [i * 90, () => notify("Teams", `msg ${i + 1}`)]));
    const frames = await record(40_000, 31.2, at);
    expect(viewCycles(frames)).toEqual({ opens: 1, closes: 1 });
    expect(shellCycles(frames)).toEqual({ opens: 1, closes: 1 });
    const { shownInOrder, maxPayloads, bothReadableFrames } = payloadChecks(frames);
    expect(maxPayloads).toBeLessThanOrEqual(2);
    expect(bothReadableFrames).toBe(0);
    // In arrival order (the queue keeps at most 8 waiting: the oldest waiting one may be dropped,
    // it is in the Notification Center), ending with the newest.
    const numbers = shownInOrder.map((title) => Number(title.split(" ")[1]));
    expect([...numbers].sort((a, b) => a - b)).toEqual(numbers);
    expect(numbers[0]).toBe(1);
    expect(numbers[numbers.length - 1]).toBe(10);
    expect(numbers.length).toBeGreaterThanOrEqual(9);
    expect(frames[frames.length - 1].view).toBe("idle");
  });
});

describe("a notification arriving while the island closes", () => {
  it.each([60, 100])("reverses the close %s ms into it: no compact resting frame, no second open", async (delay) => {
    await mount();
    await notify("Slack", "first");
    // Run until the session ends and the shape starts closing.
    let frames: Frame[] = [];
    for (let t = 0; t < 10_000 && island().dataset.view !== "idle"; t += 16) {
      await ms(16);
    }
    expect(island().dataset.view).toBe("idle");
    const closingFrom = parseFloat(island().style.height);
    expect(closingFrom).toBeGreaterThan(COMPACT_HEIGHT + 10);
    frames = await record(1_500, 16.7, { [delay]: () => notify("WhatsApp", "second") });
    // The shape never got back to the compact height before reopening.
    const lowest = Math.min(...frames.map((f) => f.height));
    expect(lowest).toBeGreaterThan(COMPACT_HEIGHT + 2);
    expect(shellCycles(frames).closes).toBe(0);
    expect(frames[frames.length - 1].view).toBe("notification");
    expect(frames[frames.length - 1].payloads.map((p) => p.title)).toEqual(["second"]);
  });
});

describe("an open panel is not stolen", () => {
  it("keeps the panel when an ordinary notification arrives; it goes to the Notification Center", async () => {
    await mount();
    act(() => void island().closest<HTMLElement>("[data-island-hit]")!.click());
    await ms(800);
    expect(island().dataset.view).toBe("userExpanded");
    const frames = await record(3_000, 31.2, { 0: () => notify("Teams", "while the panel is open") });
    expect(frames.every((f) => f.view === "userExpanded")).toBe(true);
    expect(container.querySelector('[data-layer="toast"]')).toBeNull();
  });
});
