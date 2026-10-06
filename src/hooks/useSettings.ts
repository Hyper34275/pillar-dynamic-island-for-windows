import { useCallback, useSyncExternalStore } from "react";
import { ipc, normalizeSettings, onEvent, SETTINGS_DEFAULTS, type Settings, type SettingsPatch } from "../lib/ipc";
import { dlog } from "../lib/debugLog";

// One settings store for the whole UI, backed by get_settings / update_settings and the
// 'settings-changed' event. There is no read-modify-write: the UI only ever sends the
// fields it changed, Rust merges them and returns the authoritative result.
//
// view = confirmed (last value from Rust) + pending patches (sent, not yet acknowledged),
// so rapid toggles stay responsive and a failed save simply drops its patch.

type Listener = () => void;

interface Pending {
  id: number;
  patch: SettingsPatch;
}

export interface SettingsBackend {
  get: () => Promise<Settings | null>;
  update: (patch: SettingsPatch) => Promise<Settings | null>;
  subscribe: (handler: (raw: unknown) => void) => () => void;
}

export function applyPending(confirmed: Settings, pending: readonly Pending[]): Settings {
  return pending.reduce<Settings>((acc, p) => ({ ...acc, ...p.patch }), confirmed);
}

export function createSettingsStore(backend: SettingsBackend) {
  let confirmed: Settings = SETTINGS_DEFAULTS;
  let pending: Pending[] = [];
  let view: Settings = SETTINGS_DEFAULTS;
  let nextId = 1;
  let started = false;
  // Updates go out one at a time so Rust applies them in the order the user made them.
  let queue: Promise<unknown> = Promise.resolve();
  const listeners = new Set<Listener>();

  function publish() {
    const next = applyPending(confirmed, pending);
    if (JSON.stringify(next) === JSON.stringify(view)) return;
    view = next;
    listeners.forEach((l) => l());
  }

  function start() {
    if (started) return;
    started = true;
    backend.subscribe((raw) => {
      confirmed = normalizeSettings(raw);
      publish();
    });
    void backend.get().then((settings) => {
      if (settings) confirmed = settings;
      publish();
    });
  }

  function update(patch: SettingsPatch): Promise<boolean> {
    const entry: Pending = { id: nextId++, patch };
    pending = [...pending, entry];
    publish();
    const run = queue.then(async () => {
      const result = await backend.update(patch);
      pending = pending.filter((p) => p.id !== entry.id);
      if (result) {
        confirmed = result;
      } else {
        dlog("warn", "settings", "update failed; change reverted");
      }
      publish();
      return result !== null;
    });
    queue = run.catch(() => {});
    return run;
  }

  return {
    subscribe(listener: Listener): () => void {
      start();
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => view,
    update,
  };
}

const store = createSettingsStore({
  get: ipc.getSettings,
  update: ipc.updateSettings,
  subscribe: (handler) => onEvent<unknown>("settings-changed", handler),
});

export interface UseSettingsResult {
  settings: Settings;
  /** Resolves true when Rust accepted the change; on false the UI has already reverted it. */
  update: (patch: SettingsPatch) => Promise<boolean>;
}

export function useSettings(): UseSettingsResult {
  const settings = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const update = useCallback((patch: SettingsPatch) => store.update(patch), []);
  return { settings, update };
}
