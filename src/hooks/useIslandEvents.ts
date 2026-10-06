import { useEffect, useRef } from "react";
import { ipc, onEvent } from "../lib/ipc";
import { isTabId, type TabId } from "../components/Pill/tabs";

interface IslandEventHandlers {
  /** Tray click or second launch: toggle the island, optionally on a tab. */
  onToggle?: (tab?: TabId) => void;
  onFullscreenChanged?: (fullscreen: boolean) => void;
  /** Monitor layout or DPI changed: the native window must be re-sized and re-centred. */
  onDisplayChanged?: () => void;
  /** Another app's window became active (e.g. the user clicked the desktop, the taskbar or an app). */
  onForegroundChanged?: () => void;
}

function fullscreenFrom(payload: unknown): boolean {
  if (typeof payload === "boolean") return payload;
  if (payload && typeof payload === "object") {
    const p = payload as { fullscreen?: unknown; isFullscreen?: unknown };
    return p.fullscreen === true || p.isFullscreen === true;
  }
  return false;
}

/** Subscribes to the backend's island-level events (only those with a handler) for the lifetime of the component. */
export function useIslandEvents(handlers: IslandEventHandlers): void {
  const ref = useRef(handlers);
  useEffect(() => {
    ref.current = handlers;
  }, [handlers]);

  const wantsToggle = !!handlers.onToggle;
  const wantsFullscreen = !!handlers.onFullscreenChanged;
  const wantsDisplay = !!handlers.onDisplayChanged;
  const wantsForeground = !!handlers.onForegroundChanged;

  useEffect(() => {
    const offs: Array<() => void> = [];
    if (wantsToggle) {
      offs.push(
        onEvent<{ tab?: unknown } | null>("island-toggle", (payload) => {
          const tab = payload?.tab;
          ref.current.onToggle?.(isTabId(tab) ? tab : undefined);
        })
      );
    }
    if (wantsFullscreen) {
      offs.push(onEvent<unknown>("fullscreen-changed", (payload) => ref.current.onFullscreenChanged?.(fullscreenFrom(payload))));
      // The backend only announces changes: ask once for the state that was already in effect.
      let disposed = false;
      void ipc.getFullscreenState().then((fullscreen) => {
        if (!disposed && fullscreen !== null) ref.current.onFullscreenChanged?.(fullscreen);
      });
      offs.push(() => {
        disposed = true;
      });
    }
    if (wantsDisplay) {
      offs.push(onEvent<unknown>("display-changed", () => ref.current.onDisplayChanged?.()));
    }
    if (wantsForeground) {
      offs.push(onEvent<unknown>("foreground-changed", () => ref.current.onForegroundChanged?.()));
    }
    return () => offs.forEach((off) => off());
  }, [wantsToggle, wantsFullscreen, wantsDisplay, wantsForeground]);
}
