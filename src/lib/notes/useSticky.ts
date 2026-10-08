import { useSyncExternalStore } from "react";
import { ipc } from "../ipc";
import { createStickyStore, type StickySnapshot } from "./sticky";

// The Windows Sticky Notes for the Notes tab: one shared store over sticky_notes_list (see sticky.ts).
// It only talks to the backend while a component is subscribed, i.e. while the Notes tab is open.

const store = createStickyStore({ load: ipc.stickyNotesList });

/** The latest answer; null until the backend has answered (or when it cannot: outside the app). */
export function useStickyNotes(): StickySnapshot | null {
  return useSyncExternalStore(store.subscribe, store.getSnapshot);
}
