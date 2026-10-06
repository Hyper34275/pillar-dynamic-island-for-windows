import { ipc } from "../ipc";
import type { ReminderStore } from "./types";

/** Reminder state in the per-user state directory, through the Rust commands. */
export function createIpcReminderStore(): ReminderStore {
  return {
    async load() {
      // A failed read is an error, not "nothing fired yet": the engine logs it instead of
      // silently starting from an empty set.
      const fired = await ipc.reminderStateLoad();
      if (fired === null) throw new Error("reminder_state_load did not run");
      return fired;
    },
    async save(fired) {
      if (!(await ipc.reminderStateSave(fired))) throw new Error("reminder_state_save did not run");
    },
  };
}
