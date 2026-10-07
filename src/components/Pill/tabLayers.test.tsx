// @vitest-environment jsdom
// The tab content's hand-off: at most two layers, one owner of the screen at a time, the latest
// target wins. Two harnesses: the hook alone with the progress spring stepped by hand (so a 60 Hz
// and a 32 Hz display, the user's remote session, are exact), and the real ExpandedIsland on the
// fake frame clock (the DOM: mounted panels, titles, actions, ARIA, the shift's direction).
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { motionValue, type MotionValue } from "motion/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFixedLocale } from "../../lib/i18n";
import { stepSpring, type SpringState } from "../../lib/island/spring";
import { ExpandedIsland } from "./ExpandedIsland";
import { tabCapsuleSpring, tabContentSprings, tabFade, TAB_SHIFT_PX_REDUCED, useTabLayers, useTabSteps, type TabLayer } from "./tabLayers";
import { TABS, type TabId } from "./tabs";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  setFixedLocale(null);
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

// ---- the hook alone, springs stepped by hand --------------------------------------------------

interface Snapshot {
  time: number;
  layers: { id: number; tab: TabId; leaving: boolean; opacity: number; offset: number }[];
}

function Harness({ tab, progress, reduced, sink }: { tab: TabId; progress: MotionValue<number>; reduced: boolean; sink: { current: readonly TabLayer[] } }) {
  const steps = useTabSteps(tab, TABS.findIndex((t) => t.id === tab));
  sink.current = useTabLayers(tab, progress, steps, reduced);
  return null;
}

/**
 * Plays `clicks` ({at: ms, tab}) on a display of `hz`, until `until` ms. The content progress is
 * the real spring (closed form, retargeted from its own position and velocity on each click,
 * like useSpringValue). Returns one snapshot per frame, taken after the frame's values are set.
 */
let playCount = 0;
function play(clicks: { at: number; tab: TabId }[], { hz, reduced = false, until = 1500, start = "calendar" as TabId }: { hz: number; reduced?: boolean; until?: number; start?: TabId }) {
  const progress = motionValue(0);
  const sink: { current: readonly TabLayer[] } = { current: [] };
  const params = reduced ? tabContentSprings.reduced : tabContentSprings.normal;
  const key = ++playCount;
  const render = (tab: TabId) => act(() => root.render(<Harness key={key} tab={tab} progress={progress} reduced={reduced} sink={sink} />));
  render(start);
  let goal = 0;
  let state: SpringState = { value: 0, velocity: 0 };
  const frames: Snapshot[] = [];
  const dt = 1000 / hz;
  const pending = [...clicks];
  const clickTimes: number[] = [];
  const look = () => sink.current.map((l) => ({ id: l.id, tab: l.tab, leaving: l.leaving, opacity: l.opacity.get(), offset: l.offset.get() }));
  // What is on screen just before and just after each click (before its first frame): the same pixels.
  const joins: { before: Snapshot["layers"]; after: Snapshot["layers"] }[] = [];
  for (let time = 0; time <= until; time += dt) {
    while (pending.length && pending[0].at <= time) {
      const click = pending.shift()!;
      const before = look();
      render(click.tab);
      joins.push({ before, after: look() });
      goal += 1;
      clickTimes.push(time);
    }
    state = stepSpring(state, goal, params, dt / 1000);
    if (Math.abs(state.value - goal) < 0.001 && Math.abs(state.velocity) < 0.06) state = { value: goal, velocity: 0 };
    act(() => progress.set(state.value));
    frames.push({
      time,
      layers: look(),
    });
  }
  return { frames, clickTimes, joins };
}

/** The numbers worth reading, plus the rules every playback must keep. */
function audit(frames: Snapshot[]) {
  let maxLayers = 0;
  let maxMin = 0;
  let overOwned = 0;
  let maxJump = 0;
  const prev = new Map<number, { leaving: boolean; opacity: number }>();
  for (const f of frames) {
    maxLayers = Math.max(maxLayers, f.layers.length);
    expect(f.layers.filter((l) => !l.leaving)).toHaveLength(1);
    expect(f.layers.filter((l) => l.leaving).length).toBeLessThanOrEqual(1);
    if (f.layers.length === 2) {
      const [a, b] = f.layers;
      maxMin = Math.max(maxMin, Math.min(a.opacity, b.opacity));
      if (a.opacity > 0.35 && b.opacity > 0.35) overOwned++;
    }
    for (const l of f.layers) {
      const before = prev.get(l.id);
      if (before) {
        maxJump = Math.max(maxJump, Math.abs(l.opacity - before.opacity));
        // a page that is leaving on two frames running never gets more visible
        if (before.leaving && l.leaving) expect(l.opacity).toBeLessThanOrEqual(before.opacity + 1e-9);
      }
    }
    prev.clear();
    for (const l of f.layers) prev.set(l.id, { leaving: l.leaving, opacity: l.opacity });
  }
  return { maxLayers, maxMin, overOwned, maxJump };
}

const BURST: { at: number; tab: TabId }[] = [
  { at: 0, tab: "notifications" },
  { at: 70, tab: "notes" },
  { at: 140, tab: "about" },
  { at: 210, tab: "settings" },
  { at: 280, tab: "calendar" },
  { at: 350, tab: "about" },
  { at: 420, tab: "notes" },
];

describe("tab layers: the hook", () => {
  it("never overlaps outgoing and incoming above 0.35 within one transition and reports the overlap", () => {
    for (const hz of [60, 32]) {
      const { frames } = play([{ at: 0, tab: "about" }], { hz });
      const a = audit(frames);
      console.log(`single transition @${hz} Hz: maxLayers=${a.maxLayers} maxMin(opacity)=${a.maxMin.toFixed(3)} framesBothOver0.35=${a.overOwned}`);
      expect(a.maxLayers).toBe(2);
      expect(a.overOwned).toBe(0);
      expect(a.maxMin).toBeLessThan(0.3);
      expect(frames[frames.length - 1].layers).toHaveLength(1);
    }
  });

  it("a burst of 7 clicks in 450 ms: at most two layers, one owner, ends on the last tab alone", () => {
    for (const hz of [1000 / 16.7, 1000 / 31.2]) {
      const { frames, clickTimes } = play(BURST, { hz });
      const a = audit(frames);
      const last = frames[frames.length - 1];
      expect(a.maxLayers).toBeLessThanOrEqual(2);
      expect(a.overOwned).toBe(0);
      expect(last.layers).toHaveLength(1);
      expect(last.layers[0].tab).toBe("notes");
      expect(last.layers[0].opacity).toBe(1);
      const lastClick = clickTimes[clickTimes.length - 1];
      const after = frames.filter((f) => f.time >= lastClick);
      const readable = after.find((f) => f.layers.some((l) => !l.leaving && l.opacity >= 0.9))!;
      const single = after.find((f) => f.layers.length === 1)!;
      console.log(
        `burst @${hz.toFixed(1)} Hz: maxLayers=${a.maxLayers} maxMin=${a.maxMin.toFixed(3)} framesBothOver0.35=${a.overOwned} maxJump=${a.maxJump.toFixed(3)} lastClick->incoming>=0.9: ${(readable.time - lastClick).toFixed(0)} ms, ->outgoing unmounted: ${(single.time - lastClick).toFixed(0)} ms`
      );
    }
  });

  it("A -> B -> A quickly: never three layers, A ends alone, opacity continuous", () => {
    for (const hz of [60, 32]) {
      for (const gap of [30, 60, 100, 160]) {
        const { frames, joins } = play(
          [
            { at: 0, tab: "notes" },
            { at: gap, tab: "calendar" },
          ],
          { hz }
        );
        const a = audit(frames);
        expect(a.maxLayers).toBeLessThanOrEqual(2);
        expect(a.maxMin).toBeLessThan(0.3);
        // A page that survives a click shows exactly what it showed: no restart, no jump. (Between
        // frames the outgoing page may drop a lot at once: its fade is short on purpose.)
        for (const join of joins) {
          for (const l of join.after) {
            const was = join.before.find((b) => b.id === l.id);
            if (was) {
              expect(l.opacity).toBeCloseTo(was.opacity, 9);
              expect(l.offset).toBeCloseTo(was.offset, 9);
            }
          }
        }
        const last = frames[frames.length - 1];
        expect(last.layers.map((l) => l.tab)).toEqual(["calendar"]);
        expect(last.layers[0].opacity).toBe(1);
      }
    }
  });

  it("drops the incoming page that never became visible, and the old outgoing when the incoming owned the screen", () => {
    // Click, click again before the first target is readable: the first target is discarded.
    const early = play(
      [
        { at: 0, tab: "notes" },
        { at: 20, tab: "about" },
      ],
      { hz: 60 }
    );
    const seen = new Set(early.frames.flatMap((f) => f.layers.map((l) => l.tab)));
    expect(seen.has("notes")).toBe(true); // it existed as a layer, but...
    expect(Math.max(...early.frames.flatMap((f) => f.layers.filter((l) => l.tab === "notes").map((l) => l.opacity)))).toBeLessThan(0.05);
    // Click once the first target owns the screen: it becomes the outgoing page, the old one is gone at once.
    const late = play(
      [
        { at: 0, tab: "notes" },
        { at: 140, tab: "about" },
      ],
      { hz: 60 }
    );
    const atClick = late.frames.find((f) => f.time >= 140)!;
    expect(atClick.layers.map((l) => l.tab)).toEqual(["notes", "about"]);
    expect(atClick.layers[0].leaving).toBe(true);
  });

  it("reduced motion: no offset ever, same layer rules", () => {
    const { frames } = play(BURST, { hz: 60, reduced: true });
    const a = audit(frames);
    expect(a.maxLayers).toBeLessThanOrEqual(2);
    expect(a.overOwned).toBe(0);
    for (const f of frames) for (const l of f.layers) expect(Math.abs(l.offset)).toBe(TAB_SHIFT_PX_REDUCED);
    expect(frames[frames.length - 1].layers).toHaveLength(1);
  });

  it("the fade windows hand over: the outgoing page is gone by 0.35 before the incoming one is half readable", () => {
    expect(tabFade.out).toBeLessThanOrEqual(0.35);
    expect(tabFade.in[0]).toBeGreaterThanOrEqual(0.3 - 1e-9);
    expect(tabFade.in[1]).toBeLessThanOrEqual(0.75 + 1e-9);
  });
});

// ---- the capsule -------------------------------------------------------------------------------

/** One capsule move of `slots`, frame by frame like useSpringValue (no overshoot, snap within 0.001). */
function capsuleMove(slots: number, hz: number, response: number) {
  const slotPx = 352 / 5;
  const dt = 1 / hz;
  let state: SpringState = { value: 0, velocity: 0 };
  let prevVelocity = 0;
  let maxStep = 0;
  let peakV = 0;
  let peakA = 0;
  let settle = 0;
  let firstFrame = 0;
  const cumulative: number[] = [];
  for (let n = 1; n <= 600; n++) {
    const before = state.value;
    let next = stepSpring(state, slots, { response, dampingFraction: 1 }, dt);
    if (next.value > slots) next = { value: slots, velocity: 0 };
    if (Math.abs(next.value - slots) <= 0.001 && Math.abs(next.velocity) <= 0.06) next = { value: slots, velocity: 0 };
    const stepPx = (next.value - before) * slotPx;
    if (n === 1) firstFrame = stepPx;
    maxStep = Math.max(maxStep, stepPx);
    const v = stepPx / dt;
    peakV = Math.max(peakV, v);
    peakA = Math.max(peakA, Math.abs(v - prevVelocity) / dt);
    prevVelocity = v;
    cumulative.push(next.value / slots);
    state = next;
    if (Math.abs(slots - next.value) * slotPx <= 0.5 && !settle) settle = n * dt * 1000;
    if (next.value === slots) break;
  }
  return { slotPx, maxStepPx: maxStep, maxStepSlot: maxStep / slotPx, firstFramePx: firstFrame, peakV, peakA, settleMs: settle, share2: cumulative[1] ?? 1 };
}

describe("tab capsule", () => {
  it("reads as continuous at 60 Hz and 32 Hz, reacts on the next frame, and leads the content", () => {
    for (const [label, response] of [
      ["before 0.22", 0.22],
      ["after  " + tabCapsuleSpring.response.toFixed(2), tabCapsuleSpring.response],
    ] as const) {
      for (const slots of [1, 4]) {
        for (const hz of [60, 32]) {
          const m = capsuleMove(slots, hz, response);
          console.log(
            `capsule ${label} ${slots}-slot @${hz} Hz: max ${m.maxStepPx.toFixed(1)} px/frame (${m.maxStepSlot.toFixed(2)} slot), first frame ${m.firstFramePx.toFixed(1)} px, peak v ${m.peakV.toFixed(0)} px/s, peak a ${m.peakA.toFixed(0)} px/s2, settle(0.5px) ${m.settleMs.toFixed(0)} ms, first 2 frames ${(m.share2 * 100).toFixed(0)}% of travel`
          );
        }
      }
    }
    expect(tabCapsuleSpring.dampingFraction).toBe(1);
    const oneSlot60 = capsuleMove(1, 60, tabCapsuleSpring.response);
    expect(oneSlot60.maxStepSlot).toBeLessThanOrEqual(0.35);
    expect(oneSlot60.firstFramePx).toBeGreaterThan(2); // visibly reacts on the very next frame
    const fourSlot32 = capsuleMove(4, 32, tabCapsuleSpring.response);
    expect(fourSlot32.share2).toBeLessThan(0.5); // not most of the travel in two frames
    expect(tabCapsuleSpring.response).toBeLessThan(tabContentSprings.normal.response); // the capsule leads the content
  });
});

// ---- the real island, DOM ----------------------------------------------------------------------

const noop = () => {};
const panels = () => [...container.querySelectorAll<HTMLElement>("[data-panel]")];
const titles = () => [...container.querySelectorAll<HTMLElement>("[data-tab-title]")];
const actions = () => [...container.querySelectorAll<HTMLElement>("[data-tab-action]")];
const opacityOf = (el: Element) => Number((el as HTMLElement).style.opacity || "1");

function Island({ tab, reduced, onSelect }: { tab: TabId; reduced: boolean; onSelect: (id: TabId) => void }) {
  return <ExpandedIsland activeTab={tab} reducedMotion={reduced} notificationStatus={null} onRequestNotificationAccess={noop} onSelectTab={onSelect} />;
}

async function frames(n: number, ms = 16) {
  for (let i = 0; i < n; i++) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
  }
}

describe("tab layers: the island", () => {
  async function mount(reduced: boolean) {
    vi.useFakeTimers();
    let setTab: (id: TabId) => void = noop;
    function Host() {
      const [tab, set] = useState<TabId>("calendar");
      setTab = set;
      return <Island tab={tab} reduced={reduced} onSelect={set} />;
    }
    await act(async () => {
      root.render(<Host />);
    });
    await frames(5);
    return (id: TabId) => act(async () => setTab(id));
  }

  it("seven clicks in ~450 ms: <= 2 panels, titles and actions every frame, one panel for the last tab at the end", async () => {
    const go = await mount(false);
    const order: TabId[] = ["notifications", "notes", "about", "settings", "calendar", "about", "notifications"];
    let maxPanels = 0;
    let maxTitles = 0;
    let maxActions = 0;
    let bothReadable = 0;
    const check = () => {
      maxPanels = Math.max(maxPanels, panels().length);
      maxTitles = Math.max(maxTitles, titles().length);
      maxActions = Math.max(maxActions, actions().length);
      expect(container.querySelectorAll('[role="tabpanel"]').length).toBeLessThanOrEqual(1);
      const t = titles();
      if (t.length === 2 && opacityOf(t[0]) > 0.35 && opacityOf(t[1]) > 0.35) bothReadable++;
    };
    for (const id of order) {
      await go(id);
      check();
      for (let i = 0; i < 4; i++) {
        await frames(1);
        check();
      }
    }
    for (let i = 0; i < 80; i++) {
      await frames(1);
      check();
    }
    console.log(`island burst: maxPanels=${maxPanels} maxTitles=${maxTitles} maxActions=${maxActions} framesTwoReadableTitles=${bothReadable}`);
    expect(maxPanels).toBeLessThanOrEqual(2);
    expect(maxTitles).toBeLessThanOrEqual(2);
    expect(maxActions).toBeLessThanOrEqual(2);
    expect(bothReadable).toBe(0);
    expect(panels().map((p) => p.dataset.panel)).toEqual(["notifications"]);
    expect(titles()).toHaveLength(1);
    expect(actions().map((a) => a.dataset.tabAction)).toEqual(["notifications"]);
    expect(container.querySelector('[role="tabpanel"]')!.id).toBe("panel-notifications");
    const capsule = container.querySelector<HTMLElement>("[data-tab-indicator]")!;
    expect(capsule.style.transform).toContain(`translateX(${TABS.findIndex((t) => t.id === "notifications") * 100}%)`);
  });

  it("the outgoing page and its header action are hidden and unclickable; only the incoming is the tabpanel", async () => {
    const go = await mount(false);
    await go("notifications");
    await frames(1);
    await go("about"); // notifications never became visible: dropped; calendar still owns the screen
    const leaving = panels().find((p) => p.dataset.panel === "calendar")!;
    expect(leaving.getAttribute("aria-hidden")).toBe("true");
    expect(leaving.style.pointerEvents).toBe("none");
    expect(leaving.getAttribute("role")).toBeNull();
    const incoming = panels().find((p) => p.dataset.panel === "about")!;
    expect(incoming.getAttribute("role")).toBe("tabpanel");
    expect(incoming.id).toBe("panel-about");
    expect(incoming.getAttribute("aria-labelledby")).toBe("tab-about");

    // a leaving header action cannot be clicked
    await frames(80);
    await go("notifications");
    await frames(60); // notifications owns the screen, with its action
    await go("calendar");
    const action = actions().find((a) => a.dataset.tabAction === "notifications");
    if (action) {
      expect(action.style.pointerEvents).toBe("none");
      expect(action.getAttribute("aria-hidden")).toBe("true");
    }
  });

  it("shifts towards where the tab is, left to right, in Hebrew too", async () => {
    setFixedLocale("he");
    const go = await mount(false);
    // Read straight after the click, before any frame: the shift does not depend on timing.
    await go("about"); // later tab: to the right
    const incoming = panels().find((p) => p.dataset.panel === "about")!;
    const outgoing = panels().find((p) => p.dataset.panel === "calendar")!;
    const x = (el: HTMLElement) => Number(/translateX\((-?[\d.]+)px\)/.exec(el.style.transform)?.[1] ?? 0);
    expect(x(incoming)).toBeGreaterThan(0);
    expect(x(outgoing)).toBeLessThanOrEqual(0);
    // Going back to an earlier tab comes from the left (hook alone: the exact frame clock, no timers).
    const { joins } = play(
      [
        { at: 0, tab: "about" },
        { at: 800, tab: "calendar" },
      ],
      { hz: 60 }
    );
    const fresh = (join: (typeof joins)[number]) => join.after.find((l) => !l.leaving)!;
    expect(fresh(joins[0]).offset).toBeGreaterThan(0);
    expect(fresh(joins[1]).offset).toBeLessThan(0);
  });

  it("reduced motion: no x offset ever, still at most two layers", async () => {
    const go = await mount(true);
    for (const id of ["notes", "about", "settings", "calendar"] as TabId[]) {
      await go(id);
      for (let i = 0; i < 4; i++) {
        await frames(1);
        expect(panels().length).toBeLessThanOrEqual(2);
        for (const p of panels()) expect(p.style.transform === "none" || p.style.transform === "" || /translateX\(0(px)?\)/.test(p.style.transform)).toBe(true);
      }
    }
    await frames(60);
    expect(panels().map((p) => p.dataset.panel)).toEqual(["calendar"]);
  });
});
