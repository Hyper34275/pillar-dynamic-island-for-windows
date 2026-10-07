import { describe, expect, it, vi } from "vitest";
import { deliverNotification } from "../../hooks/useNotifications";
import type { IslandNotification } from "../ipc";
import { createNotificationHistory } from "../notifications/history";
import { createDoNotDisturb } from "./dnd";
import { createSilence } from "./silence";

function backend(initial: boolean | null) {
  let on = initial;
  return {
    get: vi.fn(async () => on),
    set: vi.fn(async (next: boolean) => {
      if (on === null) return null;
      on = next;
      return on;
    }),
  };
}

describe("Do not disturb store", () => {
  it("is unknown (no bell) until Windows has been read", async () => {
    const store = createDoNotDisturb(backend(false));
    expect(store.getSnapshot()).toBeNull();
    expect(store.isOn()).toBe(false);
    await store.refresh();
    expect(store.getSnapshot()).toBe(false);
  });

  it("toggles through the backend and reports the state Windows has after the change", async () => {
    const b = backend(false);
    const store = createDoNotDisturb(b);
    const seen: (boolean | null)[] = [];
    store.subscribe(() => seen.push(store.getSnapshot()));
    await store.refresh();
    await store.toggle();
    expect(b.set).toHaveBeenCalledWith(true);
    expect(store.isOn()).toBe(true);
    await store.toggle();
    expect(b.set).toHaveBeenLastCalledWith(false);
    expect(seen).toEqual([false, true, false]);
  });

  it("goes back when the write fails, and never toggles before the state is known", async () => {
    const failing = { get: vi.fn(async () => false), set: vi.fn(async () => null) };
    const store = createDoNotDisturb(failing);
    await store.toggle();
    expect(failing.set).not.toHaveBeenCalled();
    await store.refresh();
    await store.toggle();
    expect(store.getSnapshot()).toBe(false);
  });

  it("keeps the known state when a later read fails", async () => {
    let result: boolean | null = true;
    const store = createDoNotDisturb({ get: async () => result, set: async () => null });
    await store.refresh();
    result = null;
    await store.refresh();
    expect(store.getSnapshot()).toBe(true);
  });
});

describe("notifications while Do not disturb is on", () => {
  const note: IslandNotification = { id: 7, appName: "Teams", title: "Hi", body: "", timestamp: 0, aumid: null };

  it("go to the history, marked silenced, without popping up", () => {
    const history = createNotificationHistory();
    const popped: IslandNotification[] = [];
    expect(deliverNotification(note, 500, history, createSilence(), (n) => popped.push(n), { isOn: () => true })).toBe(false);
    expect(popped).toHaveLength(0);
    expect(history.getSnapshot()[0]).toMatchObject({ silenced: true });
    expect(deliverNotification({ ...note, id: 8 }, 600, history, createSilence(), (n) => popped.push(n), { isOn: () => false })).toBe(true);
    expect(popped.map((n) => n.id)).toEqual([8]);
  });
});
