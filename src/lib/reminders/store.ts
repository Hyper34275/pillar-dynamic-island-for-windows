import { ipc } from "../ipc";
import type { ReminderStore } from "./types";

/** Reminder state in the per-user state directory, through the Rust commands. */
export function createIpcReminderStore(): ReminderStore {
  return {
    async load() {
      return (await ipc.reminderStateLoad()) ?? {};
    },
    async save(fired) {
      if (!(await ipc.reminderStateSave(fired))) throw new Error("reminder_state_save did not run");
    },
  };
}
