import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStickyStore, normalizeStickyNote, normalizeStickySnapshot, stickyHasSection, STICKY_POLL_MS, type StickySnapshot } from "./sticky";
import { stickyMarkColor } from "./stickyColour";

const wireNote = (over: Record<string, unknown> = {}) => ({
  id: "3f2a-guid",
  text: "Buy milk\nand bread",
  title: "Buy milk",
  colour: "blue",
  updatedAt: 1_790_000_000_000,
  createdAt: 1_789_000_000_000,
  ...over,
});

describe("normalizeStickySnapshot", () => {
  it("accepts the backend's shape", () => {
    const s = normalizeStickySnapshot({ availability: "ok", revision: 4, notes: [wireNote(), wireNote({ id: "b", colour: "pink" })] })!;
    expect(s.availability).toBe("ok");
    expect(s.revision).toBe(4);
    expect(s.notes.map((n) => [n.id, n.colour])).toEqual([
      ["3f2a-guid", "blue"],
      ["b", "pink"],
    ]);
    expect(s.notes[0]).toEqual({ id: "3f2a-guid", text: "Buy milk\nand bread", title: "Buy milk", colour: "blue", updatedAt: 1_790_000_000_000, createdAt: 1_789_000_000_000 });
  });

  it("is null for anything that is not an answer, and unavailable for an unknown state", () => {
    for (const bad of [null, undefined, 5, "ok", [], [wireNote()]]) expect(normalizeStickySnapshot(bad)).toBeNull();
    expect(normalizeStickySnapshot({ availability: "something-new", notes: [wireNote()] })!).toEqual({ availability: "unavailable", notes: [], revision: 0 });
    for (const a of ["noData", "notInstalled", "unsupported", "unavailable"]) {
      expect(normalizeStickySnapshot({ availability: a, notes: [wireNote()] })!.notes).toEqual([]); // only `ok` carries notes
    }
  });

  it("repairs notes: drops the unusable and duplicates, defaults the colour, derives the title, caps text and count", () => {
    const s = normalizeStickySnapshot({
      availability: "ok",
      notes: [
        wireNote({ id: "keep", colour: "magenta", title: "", text: "\n  First line  \nsecond" }),
        wireNote({ id: "keep" }),
        wireNote({ id: "" }),
        wireNote({ id: "blank", text: "  \n " }),
        wireNote({ id: "big", text: "א".repeat(12_000) }),
        wireNote({ id: "neg", updatedAt: -5, createdAt: "x" }),
        "junk",
        null,
      ],
    })!;
    expect(s.notes.map((n) => n.id)).toEqual(["keep", "big", "neg"]);
    expect(s.notes[0].colour).toBe("yellow");
    expect(s.notes[0].title).toBe("First line");
    expect([...s.notes[1].text]).toHaveLength(10_000);
    expect(s.notes[2].updatedAt).toBe(0);
    expect(s.notes[2].createdAt).toBe(0);

    const many = normalizeStickySnapshot({ availability: "ok", notes: Array.from({ length: 700 }, (_, i) => wireNote({ id: `n${i}` })) })!;
    expect(many.notes).toHaveLength(500);
  });

  it("normalizeStickyNote rejects what is not a note", () => {
    expect(normalizeStickyNote({})).toBeNull();
    expect(normalizeStickyNote(wireNote({ id: "x".repeat(201) }))).toBeNull();
    expect(normalizeStickyNote(wireNote({ text: 7 }))).toBeNull();
  });

  it("the Notes tab has a section for everything except not installed and not known", () => {
    expect(stickyHasSection(null)).toBe(false);
    expect(stickyHasSection({ availability: "notInstalled", notes: [], revision: 1 })).toBe(false);
    for (const a of ["ok", "noData", "unsupported", "unavailable"] as const) expect(stickyHasSection({ availability: a, notes: [], revision: 1 })).toBe(true);
  });
});

describe("stickyMarkColor", () => {
  it("has a distinct colour for each of the seven Sticky Notes colours", () => {
    const names = ["yellow", "green", "blue", "purple", "pink", "gray", "charcoal"] as const;
    const marks = names.map(stickyMarkColor);
    expect(new Set(marks).size).toBe(7);
    for (const m of marks) expect(m).toMatch(/^#[0-9A-F]{6}$/i);
  });
});

describe("createStickyStore", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const snap = (revision: number, availability: StickySnapshot["availability"] = "ok", n = 1): StickySnapshot => ({
    availability,
    revision,
    notes: Array.from({ length: n }, (_, i) => normalizeStickyNote(wireNote({ id: `n${i}` }))!),
  });

  it("makes no request and no timer until someone subscribes, and none again after the last one leaves", async () => {
    const load = vi.fn(async () => snap(1));
    const store = createStickyStore({ load });
    expect(vi.getTimerCount()).toBe(0);
    expect(load).not.toHaveBeenCalled();
    expect(store.getSnapshot()).toBeNull();

    const a = vi.fn();
    const off = store.subscribe(a);
    expect(load).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(store.getSnapshot()!.revision).toBe(1);
    expect(a).toHaveBeenCalledTimes(1);

    const b = vi.fn();
    const offB = store.subscribe(b); // a second subscriber shares the first one's request and timer
    expect(load).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(1);
    offB();
    expect(vi.getTimerCount()).toBe(1);
    off();
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("asks again every ten seconds, never faster, and only notifies when the answer changed", async () => {
    const answers = [snap(1), snap(1), snap(2, "ok", 2)];
    const load = vi.fn(async () => answers[Math.min(load.mock.calls.length - 1, answers.length - 1)]);
    const store = createStickyStore({ load }, { pollMs: 1000 }); // asking for faster is raised to the floor
    const listener = vi.fn();
    const off = store.subscribe(listener);
    await vi.advanceTimersByTimeAsync(0);
    expect(load).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS - 1);
    expect(load).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(load).toHaveBeenCalledTimes(2);
    expect(listener).toHaveBeenCalledTimes(1); // same revision: no change
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS);
    expect(load).toHaveBeenCalledTimes(3);
    expect(listener).toHaveBeenCalledTimes(2);
    expect(store.getSnapshot()!.notes).toHaveLength(2);
    off();
  });

  it("skips its turn while the page cannot be seen", async () => {
    let visible = false;
    const load = vi.fn(async () => snap(1));
    const store = createStickyStore({ load }, { visible: () => visible });
    const off = store.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(load).toHaveBeenCalledTimes(1); // the first ask is the open tab itself
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS * 3);
    expect(load).toHaveBeenCalledTimes(1);
    visible = true;
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS);
    expect(load).toHaveBeenCalledTimes(2);
    off();
  });

  it("never has two requests in flight", async () => {
    let release: (s: StickySnapshot | null) => void = () => {};
    const load = vi.fn(() => new Promise<StickySnapshot | null>((r) => (release = r)));
    const store = createStickyStore({ load });
    const off = store.subscribe(() => {});
    void store.refresh();
    void store.refresh();
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS * 2);
    expect(load).toHaveBeenCalledTimes(1);
    release(snap(1));
    await vi.advanceTimersByTimeAsync(0);
    expect(store.getSnapshot()!.revision).toBe(1);
    off();
  });

  it("stays unknown when the backend cannot answer, and keeps the last good answer through a failed ask", async () => {
    const results: Array<StickySnapshot | null | "throw"> = [null, snap(1), "throw", null, snap(2, "unsupported", 0)];
    let i = 0;
    const load = vi.fn(async () => {
      const r = results[i++];
      if (r === "throw") throw new Error("boom");
      return r;
    });
    const store = createStickyStore({ load });
    const off = store.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(store.getSnapshot()).toBeNull(); // unknown, not an error and not "unavailable"
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS);
    expect(store.getSnapshot()!.revision).toBe(1);
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS); // throws
    expect(store.getSnapshot()!.revision).toBe(1);
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS); // null
    expect(store.getSnapshot()!.revision).toBe(1);
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS);
    expect(store.getSnapshot()!.availability).toBe("unsupported");
    off();
  });

  it("a state change with the same revision still reaches the page", async () => {
    const answers = [snap(1, "ok"), snap(1, "unavailable", 0)];
    let i = 0;
    const store = createStickyStore({ load: async () => answers[Math.min(i++, 1)] });
    const off = store.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(STICKY_POLL_MS);
    expect(store.getSnapshot()!.availability).toBe("unavailable");
    off();
  });
});
