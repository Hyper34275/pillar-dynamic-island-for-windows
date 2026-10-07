// Typing a note inside the island. The Notes tab's text box takes the keyboard (ipc.islandKeyboard)
// only while it has focus; `islandTyping` tells the pointer rules (usePillState) not to collapse the
// island under the user's typing, and `noteDraft` keeps unsaved text when the island closes (the
// tab unmounts) so the next open continues where the user stopped.

type Listener = () => void;

let typing = false;
const listeners = new Set<Listener>();

export const islandTyping = {
  get: (): boolean => typing,
  set(next: boolean): void {
    if (typing === next) return;
    typing = next;
    for (const listener of listeners) listener();
  },
  subscribe(listener: Listener): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
};

let draft = "";

export const noteDraft = {
  get: (): string => draft,
  set(text: string): void {
    draft = text;
  },
};
