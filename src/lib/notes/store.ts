import { clipNoteText, compareNotes, normalizeNote, normalizeNotes, NOTES_MAX, type Note } from "../ipc";
import { dlog } from "../debugLog";

// One notes store for the island, backed by notes_load / notes_save and the 'notes-changed' event.
// Rust owns notes.json and always stores the whole list, so every save sends the full list.
//
// view = confirmed (last list from Rust) + pending operations (made, not yet acknowledged), so the
// UI answers a click at once and a failed save simply drops its operation. Operations are applied
// to the confirmed list at the moment they are sent, one at a time and in the order they were
// made, so nothing the Center changed in between is overwritten with a stale copy.
//
// Note text never reaches a log. No timers either: the island must stay idle while collapsed.

type Listener = () => void;

export interface NotesBackend {
  load: () => Promise<Note[] | null>;
  save: (notes: Note[]) => Promise<Note[] | null>;
  /** Fires with the raw `notes-changed` payload whenever the stored list changed (here or in the Center). */
  subscribe: (handler: (raw: unknown) => void) => () => void;
}

/** What a mutation does to a list; returns the same array when there is nothing to do. */
type Operation = (notes: readonly Note[]) => readonly Note[];

interface Pending {
  id: number;
  apply: Operation;
}

/** Canonical order, as Rust returns it: pinned first, then newest update, then id. */
export function sortNotes(notes: readonly Note[]): Note[] {
  return [...notes].sort(compareNotes);
}

/** A fresh note id: 16 lowercase hex characters from the platform CSPRNG. */
export function newNoteId(): string {
  const bytes = new Uint8Array(8);
  globalThis.crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function sameNote(a: Note, b: Note): boolean {
  return a === b || (a.id === b.id && a.text === b.text && a.createdAt === b.createdAt && a.updatedAt === b.updatedAt && a.pinned === b.pinned);
}

function sameNotes(a: readonly Note[], b: readonly Note[]): boolean {
  return a.length === b.length && a.every((note, i) => sameNote(note, b[i]));
}

function applyPending(confirmed: readonly Note[], pending: readonly Pending[]): Note[] {
  return sortNotes(pending.reduce<readonly Note[]>((acc, p) => p.apply(acc), confirmed));
}

const replaceNote = (notes: readonly Note[], next: Note): Note[] => [...notes.filter((n) => n.id !== next.id), next];

export function createNotesStore(backend: NotesBackend) {
  let confirmed: readonly Note[] = [];
  let pending: Pending[] = [];
  let view: Note[] = [];
  let nextId = 1;
  let started = false;
  // False until Rust has answered once (or the attempt failed): until then `view` is only empty.
  let loaded = false;
  // The initial load failed or timed out and no change event has filled the list since: `view` is
  // empty only because it is unknown, so the UI says so and nothing is saved over the real list.
  let loadFailed = false;
  // A notes-changed event is newer than any load request sent before it.
  let eventApplied = false;
  let loading = false;
  // Saves go out one at a time so Rust applies them in the order the user made them.
  let queue: Promise<unknown> = Promise.resolve();
  const listeners = new Set<Listener>();

  function publish() {
    const next = applyPending(confirmed, pending);
    if (sameNotes(next, view)) return;
    view = next;
    listeners.forEach((l) => l());
  }

  /** Updates the load flags and tells the listeners when either changed. */
  function setLoadState(nextLoaded: boolean, nextFailed: boolean) {
    if (loaded === nextLoaded && loadFailed === nextFailed) return;
    loaded = nextLoaded;
    loadFailed = nextFailed;
    listeners.forEach((l) => l());
  }

  function loadNow() {
    if (loading) return;
    loading = true;
    // An event that lands while this request is out is newer than its answer.
    eventApplied = false;
    void backend
      .load()
      .catch(() => null)
      .then((notes) => {
        loading = false;
        // The answer is older than an event that already gave the list: it must not overwrite it.
        if (!eventApplied && notes) confirmed = notes;
        // The list first, then the flag: a "loaded" render must never see the previous empty list.
        publish();
        setLoadState(true, !eventApplied && !notes);
      });
  }

  function start() {
    if (started) return;
    started = true;
    backend.subscribe((raw) => {
      confirmed = normalizeNotes(raw);
      eventApplied = true;
      publish();
      setLoadState(true, false);
    });
    loadNow();
  }

  /** Asks Rust for the list again after a failed load (no-op otherwise). */
  function retry() {
    if (!loadFailed || loading) return;
    setLoadState(false, false);
    loadNow();
  }

  /** Optimistic apply + queued save. Resolves true when Rust stored it, false when refused or failed. */
  function commit(apply: Operation): Promise<boolean> {
    const entry: Pending = { id: nextId++, apply };
    pending = [...pending, entry];
    publish();
    const run = queue.then(async () => {
      const done = (ok: boolean) => {
        pending = pending.filter((p) => p.id !== entry.id);
        publish();
        return ok;
      };
      const next = apply(confirmed);
      // The note vanished (removed in the Center meanwhile): nothing to save.
      if (next === confirmed) return done(false);
      const result = await backend.save(sortNotes(next)).catch(() => null);
      if (!result) {
        dlog("warn", "notes", "save failed; change reverted");
        return done(false);
      }
      confirmed = result;
      return done(true);
    });
    queue = run.catch(() => {});
    return run;
  }

  const find = (id: string): Note | undefined => view.find((n) => n.id === id);

  return {
    subscribe(listener: Listener): () => void {
      start();
      // A failed load is tried again when the notes are next looked at (the first subscriber).
      if (listeners.size === 0) retry();
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: (): readonly Note[] => view,
    isLoaded: () => loaded,
    /** True when the list could not be read: an empty snapshot then means "unknown", not "no notes". */
    isLoadFailed: () => loadFailed,
    retry,

    /** Adds a note with `text` as typed. False for blank text, a full list (500) or a failed save. */
    add(text: string, id: string = newNoteId()): Promise<boolean> {
      const now = Date.now();
      const note = normalizeNote({ id, text: clipNoteText(text), createdAt: now, updatedAt: now, pinned: false }, now);
      // Never save a list built on an unknown one: it would replace the stored notes.
      if (!note || loadFailed || view.length >= NOTES_MAX || find(note.id)) return Promise.resolve(false);
      return commit((notes) => (notes.some((n) => n.id === note.id) ? notes : replaceNote(notes, note)));
    },

    /** Replaces a note's text. False for blank text, an unknown id or a failed save; unchanged text is a no-op that succeeds. */
    update(id: string, text: string): Promise<boolean> {
      const current = find(id);
      const clipped = clipNoteText(text);
      if (!current || clipped.trim() === "") return Promise.resolve(false);
      if (current.text === clipped) return Promise.resolve(true);
      const now = Date.now();
      return commit((notes) => {
        const existing = notes.find((n) => n.id === id);
        return existing ? replaceNote(notes, { ...existing, text: clipped, updatedAt: Math.max(now, existing.createdAt, existing.updatedAt) }) : notes;
      });
    },

    remove(id: string): Promise<boolean> {
      if (!find(id)) return Promise.resolve(false);
      return commit((notes) => (notes.some((n) => n.id === id) ? notes.filter((n) => n.id !== id) : notes));
    },

    /** Pins or unpins; the update time stays, so an unpinned note returns to its place by date. */
    togglePin(id: string): Promise<boolean> {
      const current = find(id);
      if (!current) return Promise.resolve(false);
      const pinned = !current.pinned;
      return commit((notes) => {
        const existing = notes.find((n) => n.id === id);
        return existing && existing.pinned !== pinned ? replaceNote(notes, { ...existing, pinned }) : notes;
      });
    },
  };
}

export type NotesStore = ReturnType<typeof createNotesStore>;
