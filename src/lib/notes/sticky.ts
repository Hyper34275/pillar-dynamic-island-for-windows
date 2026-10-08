// The user's Windows Sticky Notes, READ ONLY, as the backend reports them (src-tauri/src/sticky_notes.rs).
// Editing happens in the Sticky Notes app itself; the island only shows the notes and opens that app.
//
// The page never sees an error from here: a Sticky Notes that is missing, never used, in a layout the
// backend does not know, or unreadable is an `availability` state, and "the backend did not answer" is
// `null` (nothing is shown at all). The store re-asks every STICKY_POLL_MS while something is subscribed
// (the Notes tab being open) and not at all otherwise; the backend answers from a cache unless the
// Sticky Notes files changed, so the poll costs two `stat` calls.

/** `ok`: read; `noData`: installed, no notes file yet; `notInstalled`; `unsupported`: a layout this version does not know; `unavailable`: could not be read. */
export type StickyAvailability = "ok" | "noData" | "notInstalled" | "unsupported" | "unavailable";

export type StickyColour = "yellow" | "green" | "blue" | "purple" | "pink" | "gray" | "charcoal";

export type StickyNote = {
  id: string;
  /** Plain text (paragraph prefixes and formatting markers already removed by the backend). */
  text: string;
  /** The first non-empty line. */
  title: string;
  colour: StickyColour;
  /** Unix ms; 0 when the notes file does not say. */
  updatedAt: number;
  createdAt: number;
};

export type StickySnapshot = {
  availability: StickyAvailability;
  /** Newest first; empty unless `availability` is `ok`. */
  notes: StickyNote[];
  /** Changes whenever the backend's answer changed. */
  revision: number;
};

/** How often the Notes tab asks while it is open (the task's floor is 10 s). */
export const STICKY_POLL_MS = 10_000;

const AVAILABILITIES: readonly StickyAvailability[] = ["ok", "noData", "notInstalled", "unsupported", "unavailable"];
const COLOURS: readonly StickyColour[] = ["yellow", "green", "blue", "purple", "pink", "gray", "charcoal"];
/** The same limits the backend applies. */
const MAX_STICKY = 500;
const MAX_TEXT = 10_000;
const MAX_ID = 200;

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function time(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : 0;
}

/** One wire note, or null when it is unusable (no id, blank text). */
export function normalizeStickyNote(raw: unknown): StickyNote | null {
  if (!isRecord(raw)) return null;
  if (typeof raw.id !== "string" || raw.id === "" || raw.id.length > MAX_ID) return null;
  if (typeof raw.text !== "string" || raw.text.trim() === "") return null;
  const text = raw.text.length > MAX_TEXT ? Array.from(raw.text).slice(0, MAX_TEXT).join("") : raw.text;
  const firstLine = text.split(/\r?\n/).map((l) => l.trim()).find((l) => l !== "") ?? "";
  const updatedAt = time(raw.updatedAt);
  return {
    id: raw.id,
    text,
    title: typeof raw.title === "string" && raw.title !== "" ? raw.title : firstLine,
    colour: COLOURS.includes(raw.colour as StickyColour) ? (raw.colour as StickyColour) : "yellow",
    updatedAt,
    createdAt: time(raw.createdAt),
  };
}

/**
 * What `sticky_notes_list` answers. A page that already shows `revision` (it sent `since`) gets
 * `unchanged: true` and no notes: it keeps what it has.
 */
export type StickyAnswer = StickySnapshot & { unchanged: boolean };

/** The backend's answer, or null when it is not one (the page then shows nothing). */
export function normalizeStickyAnswer(raw: unknown): StickyAnswer | null {
  if (!isRecord(raw)) return null;
  const availability = AVAILABILITIES.includes(raw.availability as StickyAvailability) ? (raw.availability as StickyAvailability) : "unavailable";
  const seen = new Set<string>();
  const notes: StickyNote[] = [];
  if (availability === "ok" && Array.isArray(raw.notes)) {
    for (const item of raw.notes) {
      const note = normalizeStickyNote(item);
      if (!note || seen.has(note.id)) continue;
      seen.add(note.id);
      notes.push(note);
      if (notes.length >= MAX_STICKY) break;
    }
  }
  const revision = typeof raw.revision === "number" && Number.isFinite(raw.revision) ? raw.revision : 0;
  return { availability, notes, revision, unchanged: raw.unchanged === true };
}

/** The snapshot of an answer that carries one (not an `unchanged` one); null otherwise. */
export function normalizeStickySnapshot(raw: unknown): StickySnapshot | null {
  const answer = normalizeStickyAnswer(raw);
  if (!answer || answer.unchanged) return null;
  return { availability: answer.availability, notes: answer.notes, revision: answer.revision };
}

/** Whether the Notes tab has anything to say about Sticky Notes. Not installed, or not known: nothing. */
export function stickyHasSection(snapshot: StickySnapshot | null): snapshot is StickySnapshot {
  return snapshot !== null && snapshot.availability !== "notInstalled";
}

// -----------------------------------------------------------------------------
// The store
// -----------------------------------------------------------------------------

export interface StickyBackend {
  /**
   * The backend's answer; null when it could not answer (no backend, a failed call). `since` is the
   * revision the page already shows (undefined before the first answer).
   */
  load: (since?: number) => Promise<StickyAnswer | null>;
}

export interface StickyStoreOptions {
  pollMs?: number;
  /** False while the page cannot be seen: the poll then skips its turn. */
  visible?: () => boolean;
}

const pageVisible = () => typeof document === "undefined" || document.visibilityState !== "hidden";

/**
 * One shared copy of the Sticky Notes answer. While there is at least one subscriber (the open Notes
 * tab) it asks at once and then every `pollMs`; with none, no timer exists. A failed ask keeps the last
 * good answer; before any answer the snapshot stays null.
 */
export function createStickyStore(backend: StickyBackend, options: StickyStoreOptions = {}) {
  const pollMs = Math.max(options.pollMs ?? STICKY_POLL_MS, STICKY_POLL_MS);
  const visible = options.visible ?? pageVisible;
  const listeners = new Set<() => void>();
  let snapshot: StickySnapshot | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let inflight: Promise<void> | null = null;

  function apply(next: StickyAnswer | null) {
    // "Unchanged" carries no notes: the page keeps what it has (and with nothing yet, there is nothing to keep).
    if (!next || next.unchanged) return;
    if (snapshot && snapshot.revision === next.revision && snapshot.availability === next.availability) return;
    snapshot = { availability: next.availability, notes: next.notes, revision: next.revision };
    listeners.forEach((l) => l());
  }

  /** Asks the backend now (one ask at a time). Resolves when the answer is applied; never rejects. */
  function refresh(): Promise<void> {
    if (inflight) return inflight;
    inflight = backend
      .load(snapshot?.revision)
      .catch(() => null)
      .then(apply)
      .finally(() => {
        inflight = null;
      });
    return inflight;
  }

  function startTimer() {
    if (timer !== null) return;
    timer = setInterval(() => {
      if (visible()) void refresh();
    }, pollMs);
  }

  function stopTimer() {
    if (timer === null) return;
    clearInterval(timer);
    timer = null;
  }

  return {
    subscribe(listener: () => void): () => void {
      const first = listeners.size === 0;
      listeners.add(listener);
      if (first) {
        void refresh();
        startTimer();
      }
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) stopTimer();
      };
    },
    getSnapshot: (): StickySnapshot | null => snapshot,
    refresh,
  };
}

export type StickyStore = ReturnType<typeof createStickyStore>;
