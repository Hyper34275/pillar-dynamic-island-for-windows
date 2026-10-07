import { afterEach, describe, expect, it, vi } from "vitest";
import { NOTE_MAX_CHARS, NOTES_MAX, type Note } from "../ipc";
import { createNotesStore, newNoteId, sortNotes, type NotesBackend } from "./store";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

const note = (over: Partial<Note> = {}): Note => ({ id: "n1", text: "hello", createdAt: 1000, updatedAt: 1000, pinned: false, ...over });

/** A backend that behaves like Rust: stores what it is given (sorted) and returns it. */
function backendWith(initial: Note[] = [], overrides: Partial<NotesBackend> = {}) {
  let stored = initial;
  let handler: (raw: unknown) => void = () => {};
  const saves: Note[][] = [];
  const backend: NotesBackend = {
    load: () => Promise.resolve(stored),
    save: (notes) => {
      saves.push(notes);
      stored = notes;
      return Promise.resolve(notes);
    },
    subscribe: (h) => {
      handler = h;
      return () => {};
    },
    ...overrides,
  };
  return { backend, saves, emit: (raw: unknown) => handler(raw) };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("sortNotes and newNoteId", () => {
  it("orders pinned first, then updatedAt descending, then id ascending, without touching the input", () => {
    const input = [note({ id: "b", updatedAt: 5 }), note({ id: "a", updatedAt: 5 }), note({ id: "z", updatedAt: 1, pinned: true }), note({ id: "c", updatedAt: 9 })];
    const copy = [...input];
    expect(sortNotes(input).map((n) => n.id)).toEqual(["z", "c", "a", "b"]);
    expect(input).toEqual(copy);
  });

  it("makes 16 lowercase hex characters, different each time", () => {
    const ids = new Set(Array.from({ length: 50 }, newNoteId));
    expect(ids.size).toBe(50);
    for (const id of ids) expect(id).toMatch(/^[0-9a-f]{16}$/);
  });
});

describe("notes store: loading", () => {
  it("loads on the first subscribe, in canonical order, and reports loaded", async () => {
    const { backend } = backendWith([note({ id: "a", updatedAt: 1 }), note({ id: "b", updatedAt: 2 })]);
    const store = createNotesStore(backend);
    expect(store.isLoaded()).toBe(false);
    const listener = vi.fn();
    store.subscribe(listener);
    await flush();
    expect(store.isLoaded()).toBe(true);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["b", "a"]);
    expect(listener).toHaveBeenCalled();
  });

  it("is loaded but empty, and flagged as failed, when the backend cannot be reached", async () => {
    const store = createNotesStore(backendWith([], { load: () => Promise.resolve(null) }).backend);
    store.subscribe(() => {});
    await flush();
    expect(store.isLoaded()).toBe(true);
    expect(store.isLoadFailed()).toBe(true);
    expect(store.getSnapshot()).toEqual([]);
  });

  it("treats a rejected load like a failed one", async () => {
    const store = createNotesStore(backendWith([], { load: () => Promise.reject(new Error("boom")) }).backend);
    store.subscribe(() => {});
    await flush();
    expect(store.isLoaded()).toBe(true);
    expect(store.isLoadFailed()).toBe(true);
  });

  it("ignores a load answer that lands after a newer notes-changed event", async () => {
    const gate = deferred<Note[] | null>();
    const { backend, emit } = backendWith([], { load: () => gate.promise });
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    emit([note({ id: "fresh", text: "from the center" })]);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["fresh"]);
    gate.resolve([note({ id: "stale" })]);
    await flush();
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["fresh"]);
    expect(store.isLoaded()).toBe(true);
    expect(store.isLoadFailed()).toBe(false);
  });

  it("a failed load answer after an event is not a failure: the event already gave the list", async () => {
    const gate = deferred<Note[] | null>();
    const { backend, emit } = backendWith([], { load: () => gate.promise });
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    emit([note({ id: "fresh" })]);
    gate.resolve(null);
    await flush();
    expect(store.isLoadFailed()).toBe(false);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["fresh"]);
  });

  it("never reports loaded with the previous empty list: the list is published before the flag", async () => {
    const { backend } = backendWith([note({ id: "a" })]);
    const store = createNotesStore(backend);
    const seen: Array<{ loaded: boolean; count: number }> = [];
    store.subscribe(() => seen.push({ loaded: store.isLoaded(), count: store.getSnapshot().length }));
    await flush();
    expect(seen.filter((s) => s.loaded && s.count === 0)).toEqual([]);
    expect(seen[seen.length - 1]).toEqual({ loaded: true, count: 1 });
  });

  it("retries a failed load on the next first subscriber, and recovers", async () => {
    let calls = 0;
    const { backend } = backendWith([note({ id: "a" })], {
      load: () => (++calls === 1 ? Promise.resolve(null) : Promise.resolve([note({ id: "a" })])),
    });
    const store = createNotesStore(backend);
    const off = store.subscribe(() => {});
    await flush();
    expect(store.isLoadFailed()).toBe(true);
    off();

    store.subscribe(() => {});
    expect(store.isLoaded()).toBe(false); // loading again, not "no notes"
    await flush();
    expect(calls).toBe(2);
    expect(store.isLoadFailed()).toBe(false);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["a"]);
  });

  it("retry() reloads after a failure and does nothing otherwise", async () => {
    let calls = 0;
    const { backend } = backendWith([], { load: () => (++calls === 1 ? Promise.resolve(null) : Promise.resolve([note({ id: "a" })])) });
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    store.retry();
    await flush();
    expect(calls).toBe(2);
    expect(store.getSnapshot()).toHaveLength(1);
    store.retry();
    await flush();
    expect(calls).toBe(2);
  });

  it("does not retry a load that is still in flight or that worked", async () => {
    let calls = 0;
    const { backend } = backendWith([note({ id: "a" })], { load: () => (++calls, Promise.resolve([note({ id: "a" })])) });
    const store = createNotesStore(backend);
    const off = store.subscribe(() => {});
    off();
    store.subscribe(() => {});
    await flush();
    expect(calls).toBe(1);
  });

  it("refuses to add a note while the stored list is unknown, so a save cannot replace it", async () => {
    const { backend, saves } = backendWith([], { load: () => Promise.resolve(null) });
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    expect(await store.add("new note")).toBe(false);
    expect(saves).toEqual([]);
  });

  it("replaces the confirmed list on notes-changed, normalising it", async () => {
    const { backend, emit } = backendWith([note({ id: "a" })]);
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    emit([note({ id: "x", text: "from the center" }), { id: "bad id", text: "dropped" }, note({ id: "y", updatedAt: 3000 })]);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["y", "x"]);
  });

  it("keeps the snapshot reference when nothing changed", async () => {
    const { backend, emit } = backendWith([note({ id: "a" })]);
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    const before = store.getSnapshot();
    emit([note({ id: "a" })]);
    expect(store.getSnapshot()).toBe(before);
  });
});

describe("notes store: add", () => {
  it("shows the note at once, saves the whole list, and confirms with the server's answer", async () => {
    const gate = deferred<Note[] | null>();
    const saves: Note[][] = [];
    const { backend } = backendWith([note({ id: "old", updatedAt: 1 })], { save: (notes) => (saves.push(notes), gate.promise) });
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();

    const result = store.add("  buy milk \n", "newid");
    expect(store.getSnapshot().map((n) => n.id)).toContain("newid");
    expect(store.getSnapshot().find((n) => n.id === "newid")!.text).toBe("  buy milk \n");
    await flush();
    expect(saves).toHaveLength(1);
    expect(saves[0].map((n) => n.id).sort()).toEqual(["newid", "old"]);

    const stored = saves[0].map((n) => (n.id === "newid" ? { ...n, text: "server text" } : n));
    gate.resolve(stored);
    await expect(result).resolves.toBe(true);
    expect(store.getSnapshot().find((n) => n.id === "newid")!.text).toBe("server text");
  });

  it("reverts and resolves false when the save fails", async () => {
    const store = createNotesStore(backendWith([note({ id: "old" })], { save: () => Promise.resolve(null) }).backend);
    store.subscribe(() => {});
    await flush();
    const result = store.add("lost", "temp");
    expect(store.getSnapshot().some((n) => n.id === "temp")).toBe(true);
    await expect(result).resolves.toBe(false);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["old"]);
  });

  it("treats a rejecting backend like a failed save", async () => {
    const store = createNotesStore(backendWith([], { save: () => Promise.reject(new Error("boom")) }).backend);
    store.subscribe(() => {});
    await flush();
    await expect(store.add("x", "temp")).resolves.toBe(false);
    expect(store.getSnapshot()).toEqual([]);
    // The queue survives: a later change still goes out.
    expect(await store.add("again", "temp2").catch(() => "threw")).toBe(false);
  });

  it("refuses blank text, a duplicate id and a bad id without saving", async () => {
    const { backend, saves } = backendWith([note({ id: "taken" })]);
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    await expect(store.add("   \n")).resolves.toBe(false);
    await expect(store.add("hello", "taken")).resolves.toBe(false);
    await expect(store.add("hello", "no spaces allowed")).resolves.toBe(false);
    expect(saves).toHaveLength(0);
  });

  it("clips text to 10,000 characters", async () => {
    const { backend, saves } = backendWith();
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    await store.add("a".repeat(NOTE_MAX_CHARS + 50), "big");
    expect(saves[0][0].text).toHaveLength(NOTE_MAX_CHARS);
  });

  it("refuses to add a 501st note", async () => {
    const full = Array.from({ length: NOTES_MAX }, (_, i) => note({ id: `n${i}`, updatedAt: 1000 + i }));
    const { backend, saves } = backendWith(full);
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await flush();
    await expect(store.add("one too many")).resolves.toBe(false);
    expect(saves).toHaveLength(0);
    expect(store.getSnapshot()).toHaveLength(NOTES_MAX);
  });

  it("stamps createdAt = updatedAt = now and starts unpinned", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 6, 12, 0, 0));
    const { backend, saves } = backendWith();
    const store = createNotesStore(backend);
    store.subscribe(() => {});
    await vi.waitFor(() => expect(store.isLoaded()).toBe(true));
    await store.add("now", "stamped");
    expect(saves[0][0]).toMatchObject({ id: "stamped", createdAt: Date.now(), updatedAt: Date.now(), pinned: false });
  });
});

describe("notes store: update, pin, remove", () => {
  async function ready(initial: Note[], overrides: Partial<NotesBackend> = {}) {
    const ctx = backendWith(initial, overrides);
    const store = createNotesStore(ctx.backend);
    store.subscribe(() => {});
    await flush();
    return { ...ctx, store };
  }

  it("update changes the text and bumps updatedAt, keeping createdAt and pin", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(5000);
    const { store, saves } = await ready([note({ id: "a", text: "before", createdAt: 1000, updatedAt: 1000, pinned: true })]);
    await expect(store.update("a", "after")).resolves.toBe(true);
    expect(saves[0][0]).toEqual({ id: "a", text: "after", createdAt: 1000, updatedAt: 5000, pinned: true });
  });

  it("update refuses blank text and unknown ids, and does nothing for unchanged text", async () => {
    const { store, saves } = await ready([note({ id: "a", text: "same" })]);
    await expect(store.update("a", "  ")).resolves.toBe(false);
    await expect(store.update("nope", "x")).resolves.toBe(false);
    await expect(store.update("a", "same")).resolves.toBe(true);
    expect(saves).toHaveLength(0);
    expect(store.getSnapshot()[0].text).toBe("same");
  });

  it("update reverts when the save fails", async () => {
    const { store } = await ready([note({ id: "a", text: "before" })], { save: () => Promise.resolve(null) });
    const result = store.update("a", "after");
    expect(store.getSnapshot()[0].text).toBe("after");
    await expect(result).resolves.toBe(false);
    expect(store.getSnapshot()[0].text).toBe("before");
  });

  it("togglePin moves a note to the top and back, keeping updatedAt", async () => {
    const { store, saves } = await ready([note({ id: "new", updatedAt: 9 }), note({ id: "old", updatedAt: 1 })]);
    await expect(store.togglePin("old")).resolves.toBe(true);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["old", "new"]);
    expect(saves[0].find((n) => n.id === "old")!.updatedAt).toBe(1);
    await store.togglePin("old");
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["new", "old"]);
  });

  it("two quick pin toggles end where they started (each decided from what the user saw)", async () => {
    const { store } = await ready([note({ id: "a" })]);
    const first = store.togglePin("a");
    expect(store.getSnapshot()[0].pinned).toBe(true);
    const second = store.togglePin("a");
    expect(store.getSnapshot()[0].pinned).toBe(false);
    await Promise.all([first, second]);
    // The second click saw "pinned" in the optimistic view, so it asked for unpinned.
    expect(store.getSnapshot()[0].pinned).toBe(false);
  });

  it("remove drops the note and reverts when the save fails", async () => {
    const ok = await ready([note({ id: "a" }), note({ id: "b" })]);
    const gone = ok.store.remove("a");
    expect(ok.store.getSnapshot().map((n) => n.id)).toEqual(["b"]);
    await expect(gone).resolves.toBe(true);
    expect(ok.saves[0].map((n) => n.id)).toEqual(["b"]);

    const bad = await ready([note({ id: "a" })], { save: () => Promise.resolve(null) });
    const failed = bad.store.remove("a");
    expect(bad.store.getSnapshot()).toEqual([]);
    await expect(failed).resolves.toBe(false);
    expect(bad.store.getSnapshot().map((n) => n.id)).toEqual(["a"]);
  });

  it("remove and togglePin of an unknown id resolve false without saving", async () => {
    const { store, saves } = await ready([note({ id: "a" })]);
    await expect(store.remove("zzz")).resolves.toBe(false);
    await expect(store.togglePin("zzz")).resolves.toBe(false);
    expect(saves).toHaveLength(0);
  });
});

describe("notes store: ordering and concurrency", () => {
  it("sends saves one at a time, in the order the changes were made", async () => {
    const gates = [deferred<Note[] | null>(), deferred<Note[] | null>()];
    const sent: Note[][] = [];
    let calls = 0;
    const ctx = backendWith([], {
      save: (notes) => {
        sent.push(notes);
        return gates[calls++].promise;
      },
    });
    const store = createNotesStore(ctx.backend);
    store.subscribe(() => {});
    await flush();

    const first = store.add("one", "id1");
    const second = store.add("two", "id2");
    await flush();
    // Only the first is in flight; the second waits for its answer.
    expect(sent).toHaveLength(1);
    expect(sent[0].map((n) => n.id)).toEqual(["id1"]);

    gates[0].resolve(sent[0]);
    await first;
    await flush();
    expect(sent).toHaveLength(2);
    // The second save is built on what the first one stored.
    expect(sent[1].map((n) => n.id).sort()).toEqual(["id1", "id2"]);
    gates[1].resolve(sent[1]);
    await expect(second).resolves.toBe(true);
    expect(store.getSnapshot().map((n) => n.id).sort()).toEqual(["id1", "id2"]);
  });

  it("a failed save does not hold back the ones after it", async () => {
    let calls = 0;
    const store = createNotesStore(
      backendWith([], {
        save: (notes) => Promise.resolve(++calls === 1 ? null : notes),
      }).backend
    );
    store.subscribe(() => {});
    await flush();
    const first = store.add("fails", "id1");
    const second = store.add("works", "id2");
    await expect(first).resolves.toBe(false);
    await expect(second).resolves.toBe(true);
    expect(store.getSnapshot().map((n) => n.id)).toEqual(["id2"]);
  });

  it("keeps pending changes visible when a notes-changed event arrives mid-save", async () => {
    const gate = deferred<Note[] | null>();
    const ctx = backendWith([note({ id: "a" })], { save: () => gate.promise });
    const store = createNotesStore(ctx.backend);
    store.subscribe(() => {});
    await flush();
    const pendingAdd = store.add("mine", "mine1");
    ctx.emit([note({ id: "a" }), note({ id: "from-center", updatedAt: 5000 })]);
    expect(store.getSnapshot().map((n) => n.id).sort()).toEqual(["a", "from-center", "mine1"]);
    gate.resolve([note({ id: "a" }), note({ id: "from-center", updatedAt: 5000 }), note({ id: "mine1" })]);
    await pendingAdd;
    expect(store.getSnapshot()).toHaveLength(3);
  });

  it("does not resurrect a note the Center removed while an update was waiting", async () => {
    const ctx = backendWith([note({ id: "a", text: "before" })]);
    const store = createNotesStore(ctx.backend);
    store.subscribe(() => {});
    await flush();
    // The Center deletes "a" before our update is sent (the confirmed list loses it).
    const result = store.update("a", "after");
    ctx.emit([]);
    await expect(result).resolves.toBe(false);
    expect(ctx.saves).toHaveLength(0);
    expect(store.getSnapshot()).toEqual([]);
  });

  it("never creates a timer", async () => {
    vi.useFakeTimers();
    const ctx = backendWith([note({ id: "a" })]);
    const store = createNotesStore(ctx.backend);
    store.subscribe(() => {});
    await vi.advanceTimersByTimeAsync(0);
    await store.add("x", "id9");
    await store.togglePin("id9");
    await store.update("id9", "y");
    await store.remove("id9");
    expect(vi.getTimerCount()).toBe(0);
  });
});
