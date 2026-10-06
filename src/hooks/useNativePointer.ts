import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauriAvailable, tauriInvoke } from "../lib/tauri";
import { dlog } from "../lib/debugLog";

/** Hit-test rectangle in CSS px, relative to the webview viewport's top-left. */
export interface HitRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Payload of the backend's `pill-outside-press` event (screen coordinates). */
export interface OutsidePress {
  x: number;
  y: number;
  button: "left" | "right" | "middle" | "x";
}

interface UseNativePointerOptions {
  /** Interactive region(s) of the window. Empty = nothing is inside. */
  rects: HitRect[];
  /** Report presses outside the region (set while the island is expanded). */
  armed: boolean;
  /** Cursor crossed the region boundary. A press that started inside keeps it
   *  "inside" until release, so slider drags that slip off the edge don't count. */
  onPointerChange: (inside: boolean) => void;
  /** Any mouse button pressed anywhere outside the region while armed. */
  onOutsidePress: (press: OutsidePress) => void;
}

/** Consecutive set_pill_hit_region failures before giving up (old backend / missing command). */
const MAX_CONSECUTIVE_FAILURES = 3;
/** rAF doesn't run while the webview is hidden/throttled — never let a push stall on it. */
const FLUSH_FALLBACK_MS = 50;
const INVOKE_TIMEOUT_MS = 2000;

// Integer CSS px, rounded outward so the region never ends up smaller than asked.
function snapRect(r: HitRect): HitRect {
  const x = Math.floor(r.x);
  const y = Math.floor(r.y);
  return { x, y, w: Math.ceil(r.x + r.w) - x, h: Math.ceil(r.y + r.h) - y };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Native (OS-level) pointer tracking for the island.
 *
 * The overlay window is small and transparent, so DOM mouseenter/mouseleave/click
 * only see the cursor while it's over OUR window: leaving onto the desktop, or
 * clicking another app / the taskbar, is invisible to the page. The backend instead
 * watches the global cursor against the hit region pushed from here and emits
 * `pill-pointer` / `pill-outside-press`.
 *
 * `nativeActive` is true once the backend confirms tracking is running. While it's
 * false (plain browser, older backend, command failed) callers must use DOM events.
 */
export function useNativePointer({
  rects,
  armed,
  onPointerChange,
  onOutsidePress,
}: UseNativePointerOptions): { nativeActive: boolean } {
  const [nativeActive, setNativeActive] = useState(false);

  const onPointerChangeRef = useRef(onPointerChange);
  const onOutsidePressRef = useRef(onOutsidePress);
  useEffect(() => {
    onPointerChangeRef.current = onPointerChange;
    onOutsidePressRef.current = onOutsidePress;
  }, [onPointerChange, onOutsidePress]);

  // Latest inputs, read at flush time so a burst of changes collapses into one push.
  const inputsRef = useRef({ rects, armed });
  inputsRef.current = { rects, armed };
  const scheduleRef = useRef<() => void>(() => {});

  // Push pipeline: at most one push per animation frame, one in flight at a time
  // (so pushes can't land out of order), and identical payloads are skipped.
  useEffect(() => {
    if (!isTauriAvailable()) return;

    let disposed = false;
    let rafId: number | null = null;
    let fallbackId: ReturnType<typeof setTimeout> | null = null;
    let inFlight = false;
    let pending = false;
    let lastKey: string | null = null;
    let lastActive: boolean | null = null;
    let failures = 0;
    let dprQuery: MediaQueryList | null = null;

    function cancelScheduled() {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (fallbackId !== null) {
        clearTimeout(fallbackId);
        fallbackId = null;
      }
    }

    function reportActive(active: boolean, why: string) {
      if (active === lastActive) return;
      lastActive = active;
      dlog(active ? "info" : "warn", "pointer", active ? "native pointer tracking active" : `native pointer tracking unavailable (${why}) — using DOM fallback`);
      if (!disposed) setNativeActive(active);
    }

    function flush() {
      cancelScheduled();
      if (disposed || failures >= MAX_CONSECUTIVE_FAILURES) return;
      if (inFlight) {
        pending = true;
        return;
      }

      const payload = {
        rects: inputsRef.current.rects.map(snapRect),
        dpr: window.devicePixelRatio || 1,
        armed: inputsRef.current.armed,
      };
      const key = JSON.stringify(payload);
      if (key === lastKey) return;

      inFlight = true;
      tauriInvoke<boolean>("set_pill_hit_region", payload, { silent: true, timeoutMs: INVOKE_TIMEOUT_MS })
        .then((result) => {
          failures = 0;
          lastKey = key;
          reportActive(result === true, "backend returned false");
        })
        .catch((error) => {
          failures++;
          // Don't retry the identical payload in a loop; the next real change retries.
          lastKey = key;
          reportActive(false, `set_pill_hit_region failed: ${errorMessage(error)}`);
          if (failures >= MAX_CONSECUTIVE_FAILURES) {
            dlog("warn", "pointer", `set_pill_hit_region failed ${failures}x in a row — giving up for this session`);
          }
        })
        .finally(() => {
          inFlight = false;
          if (pending) {
            pending = false;
            schedule();
          }
        });
    }

    function schedule() {
      if (disposed || rafId !== null || fallbackId !== null) return;
      rafId = requestAnimationFrame(flush);
      fallbackId = setTimeout(flush, FLUSH_FALLBACK_MS);
    }

    // Rects are viewport-relative, so a window resize changes what they mean; a DPI
    // switch changes the physical scale. Re-push on either (dedupe skips no-ops).
    function onDprChange() {
      watchDpr();
      schedule();
    }
    function watchDpr() {
      dprQuery?.removeEventListener("change", onDprChange);
      dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
      dprQuery.addEventListener("change", onDprChange);
    }

    scheduleRef.current = schedule;
    schedule();
    window.addEventListener("resize", schedule);
    watchDpr();

    return () => {
      disposed = true;
      cancelScheduled();
      scheduleRef.current = () => {};
      window.removeEventListener("resize", schedule);
      dprQuery?.removeEventListener("change", onDprChange);
      // Don't leave a stale armed region behind (unmount / HMR).
      if (failures < MAX_CONSECUTIVE_FAILURES) {
        tauriInvoke(
          "set_pill_hit_region",
          { rects: [], dpr: window.devicePixelRatio || 1, armed: false },
          { silent: true, timeoutMs: INVOKE_TIMEOUT_MS }
        ).catch(() => {});
      }
    };
  }, []);

  // Rect arrays are rebuilt every render; key on content, not identity.
  const rectsKey = JSON.stringify(rects);
  useEffect(() => {
    scheduleRef.current();
  }, [rectsKey, armed]);

  // Backend events — same disposal pattern as useWorkflowEvents.
  useEffect(() => {
    if (!isTauriAvailable()) return;
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    function subscribe<T>(event: string, handler: (payload: T) => void) {
      listen<T>(event, (e) => {
        if (disposed) return;
        handler(e.payload);
      })
        .then((fn) => {
          if (disposed) {
            fn();
            return;
          }
          unlisteners.push(fn);
        })
        .catch(() => {
          // no-op in non-tauri or unsupported environment
        });
    }

    subscribe<{ inside: boolean }>("pill-pointer", (p) => onPointerChangeRef.current(!!p?.inside));
    subscribe<OutsidePress>("pill-outside-press", (p) => {
      if (p) onOutsidePressRef.current(p);
    });

    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  return { nativeActive };
}
