// Windows "Do not disturb", switched by the bell in the island's header. While it is on, Windows
// holds its own banners back, and the island does the same: notifications and meeting invitations
// still go to the Notifications tab and the unseen count but do not pop up (like a silenced
// meeting, see silence.ts). Meeting reminders are not affected. The state lives in Windows; this
// store only mirrors it (null = not read yet, or not available on this machine: no bell).

import { useEffect, useState } from "react";
import { ipc, onEvent } from "../ipc";

export interface DoNotDisturb {
  isOn(): boolean;
  /** Re-read the state from Windows. */
  refresh(): Promise<void>;
  /** Only ever from an explicit click. */
  toggle(): Promise<void>;
  subscribe(listener: () => void): () => void;
  getSnapshot(): boolean | null;
}

export function createDoNotDisturb(backend: { get: () => Promise<boolean | null>; set: (on: boolean) => Promise<boolean | null> }): DoNotDisturb {
  let on: boolean | null = null;
  let busy = false;
  const listeners = new Set<() => void>();
  const set = (next: boolean | null) => {
    if (next === on) return;
    on = next;
    listeners.forEach((listener) => listener());
  };
  return {
    isOn: () => on === true,
    async refresh() {
      const next = await backend.get();
      // A failed read keeps what is known; it only hides the bell when nothing ever was.
      if (next !== null || on === null) set(next);
    },
    async toggle() {
      // Not optimistic: the state only changes once Windows has it, so a failed write never looks
      // like Do not disturb ending (which would replay what it held, see useMissedReplay).
      if (busy || on === null) return;
      busy = true;
      try {
        const next = await backend.set(!on);
        if (next !== null) set(next);
      } finally {
        busy = false;
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot: () => on,
  };
}

export const doNotDisturb = createDoNotDisturb({ get: ipc.dndGet, set: ipc.dndSet });

/** Follow Windows: read once, then every change the backend reports (`dnd-changed`). */
export function useDoNotDisturbSync(store: DoNotDisturb = doNotDisturb): void {
  useEffect(() => {
    void store.refresh();
    return onEvent<boolean>("dnd-changed", () => void store.refresh());
  }, [store]);
}

/**
 * Do not disturb: true / false, or null when unknown (no bell). Re-renders on change. Plain state
 * plus a subscription: inside PillShell, useSyncExternalStore followed the change to "on" but
 * missed the change back to "off" (the closed island kept its slashed bell; dndShell.test.tsx).
 */
export function useDoNotDisturb(store: DoNotDisturb = doNotDisturb): boolean | null {
  const [on, setOn] = useState(store.getSnapshot);
  useEffect(() => {
    setOn(store.getSnapshot());
    return store.subscribe(() => setOn(store.getSnapshot()));
  }, [store]);
  return on;
}
