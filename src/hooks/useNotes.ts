import { useCallback, useSyncExternalStore } from "react";
import { ipc, onEvent, type Note } from "../lib/ipc";
import { createNotesStore } from "../lib/notes/store";

// The island's notes, from the one store over notes_load / notes_save and 'notes-changed'
// (see lib/notes/store.ts). Shared by every component that shows notes.

const store = createNotesStore({
  load: ipc.notesLoad,
  save: ipc.notesSave,
  subscribe: (handler) => onEvent<unknown>("notes-changed", handler),
});

export interface UseNotesResult {
  /** Canonical order: pinned first, then newest update. */
  notes: readonly Note[];
  /** False until the backend answered (or could not). */
  loaded: boolean;
  /** The list could not be read: an empty list then means "unknown", not "no notes". */
  loadFailed: boolean;
  /** Asks the backend for the list again after a failed load. */
  retry: () => void;
  /** Each resolves true when Rust stored the change; on false the UI has already reverted it. */
  add: (text: string) => Promise<boolean>;
  update: (id: string, text: string) => Promise<boolean>;
  remove: (id: string) => Promise<boolean>;
  togglePin: (id: string) => Promise<boolean>;
}

export function useNotes(): UseNotesResult {
  const notes = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const loaded = useSyncExternalStore(store.subscribe, store.isLoaded);
  const loadFailed = useSyncExternalStore(store.subscribe, store.isLoadFailed);
  const retry = useCallback(() => store.retry(), []);
  const add = useCallback((text: string) => store.add(text), []);
  const update = useCallback((id: string, text: string) => store.update(id, text), []);
  const remove = useCallback((id: string) => store.remove(id), []);
  const togglePin = useCallback((id: string) => store.togglePin(id), []);
  return { notes, loaded, loadFailed, retry, add, update, remove, togglePin };
}
