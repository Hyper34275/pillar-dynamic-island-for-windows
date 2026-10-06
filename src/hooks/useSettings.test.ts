import { describe, expect, it, vi } from "vitest";
import { createSettingsStore, type SettingsBackend } from "./useSettings";
import { SETTINGS_DEFAULTS, type Settings, type SettingsPatch } from "../lib/ipc";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function backendWith(overrides: Partial<SettingsBackend> = {}): SettingsBackend & { emit: (raw: unknown) => void } {
  let handler: (raw: unknown) => void = () => {};
  return {
    get: () => Promise.resolve(SETTINGS_DEFAULTS),
    update: (patch: SettingsPatch) => Promise.resolve({ ...SETTINGS_DEFAULTS, ...patch }),
    subscribe: (h) => {
      handler = h;
      return () => {};
    },
    ...overrides,
    emit: (raw) => handler(raw),
  };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("settings store", () => {
  it("loads the authoritative settings on first subscribe", async () => {
    const store = createSettingsStore(backendWith({ get: () => Promise.resolve({ ...SETTINGS_DEFAULTS, reminderMinutes: 15 }) }));
    store.subscribe(() => {});
    await flush();
    expect(store.getSnapshot().reminderMinutes).toBe(15);
  });

  it("applies a change optimistically and keeps the value Rust returns", async () => {
    const gate = deferred<Settings | null>();
    const store = createSettingsStore(backendWith({ update: () => gate.promise }));
    store.subscribe(() => {});
    await flush();

    const done = store.update({ launchWithWindows: false });
    expect(store.getSnapshot().launchWithWindows).toBe(false); // before Rust answered

    gate.resolve({ ...SETTINGS_DEFAULTS, launchWithWindows: false });
    expect(await done).toBe(true);
    expect(store.getSnapshot().launchWithWindows).toBe(false);
  });

  it("reverts a change Rust rejected", async () => {
    const store = createSettingsStore(backendWith({ update: () => Promise.resolve(null) }));
    store.subscribe(() => {});
    await flush();

    const done = store.update({ hideInFullscreen: false });
    expect(store.getSnapshot().hideInFullscreen).toBe(false);
    expect(await done).toBe(false);
    expect(store.getSnapshot().hideInFullscreen).toBe(true);
  });

  it("sends only the fields that changed, in the order they were made", async () => {
    const calls: SettingsPatch[] = [];
    const store = createSettingsStore(
      backendWith({
        update: (patch) => {
          calls.push(patch);
          return Promise.resolve({ ...SETTINGS_DEFAULTS, ...Object.assign({}, ...calls) });
        },
      })
    );
    store.subscribe(() => {});
    await flush();

    const a = store.update({ reminderMinutes: 10 });
    const b = store.update({ notificationsEnabled: false });
    await Promise.all([a, b]);
    expect(calls).toEqual([{ reminderMinutes: 10 }, { notificationsEnabled: false }]);
    expect(store.getSnapshot()).toMatchObject({ reminderMinutes: 10, notificationsEnabled: false });
  });

  it("does not lose a later pending change when an earlier one is acknowledged", async () => {
    const first = deferred<Settings | null>();
    const second = deferred<Settings | null>();
    const queue = [first, second];
    const store = createSettingsStore(backendWith({ update: () => queue.shift()!.promise }));
    store.subscribe(() => {});
    await flush();

    const a = store.update({ reminderMinutes: 5 });
    const b = store.update({ launchWithWindows: false });

    first.resolve({ ...SETTINGS_DEFAULTS, reminderMinutes: 5 }); // Rust has not seen the second yet
    await a;
    expect(store.getSnapshot()).toMatchObject({ reminderMinutes: 5, launchWithWindows: false });

    second.resolve({ ...SETTINGS_DEFAULTS, reminderMinutes: 5, launchWithWindows: false });
    await b;
    expect(store.getSnapshot()).toMatchObject({ reminderMinutes: 5, launchWithWindows: false });
  });

  it("follows settings-changed events from the backend and notifies subscribers", async () => {
    const backend = backendWith();
    const store = createSettingsStore(backend);
    const listener = vi.fn();
    store.subscribe(listener);
    await flush();
    listener.mockClear();

    backend.emit({ ...SETTINGS_DEFAULTS, meetingReminderEnabled: false, reminderMinutes: 15 });
    expect(store.getSnapshot()).toMatchObject({ meetingReminderEnabled: false, reminderMinutes: 15 });
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("falls back to defaults for malformed events instead of crashing", async () => {
    const backend = backendWith();
    const store = createSettingsStore(backend);
    store.subscribe(() => {});
    await flush();
    backend.emit({ reminderMinutes: "soon", launchWithWindows: "yes" });
    expect(store.getSnapshot()).toEqual(SETTINGS_DEFAULTS);
  });

  it("keeps a stable snapshot identity when nothing changed", async () => {
    const store = createSettingsStore(backendWith());
    store.subscribe(() => {});
    await flush();
    const before = store.getSnapshot();
    await store.update({ launchWithWindows: SETTINGS_DEFAULTS.launchWithWindows });
    expect(store.getSnapshot()).toBe(before);
  });
});
