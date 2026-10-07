// How large the island may be on the monitor it is on. The panel's 400x440 is a PREFERRED size:
// the backend (monitors.rs `island_limits`) measures the target monitor's width and work area
// (taskbar excluded) and its scale, and the frontend shrinks the panel to fit. This module is the
// one small store of that answer, so everything that sizes the island (PillShell's target, the
// stage window, PanelFrame's layer) reads the same limits without passing them through props.
//
// Until the backend has answered (or outside Tauri, or if it fails) there is no limit: the
// preferred sizes apply, exactly as before limits existed.

import { useSyncExternalStore } from "react";

/** Logical px, as sent by `get_island_limits` and the `display-changed` payload. */
export interface IslandLimits {
  maxWidth: number;
  maxHeight: number;
  /** The monitor's scale (1 = 100%); informational, sizes stay in DIPs. */
  scale: number;
}

export const NO_LIMITS: IslandLimits = { maxWidth: Number.POSITIVE_INFINITY, maxHeight: Number.POSITIVE_INFINITY, scale: 1 };

/** A backend answer, or null when it is not a usable one (missing, NaN, zero or negative sizes). */
export function parseIslandLimits(raw: unknown): IslandLimits | null {
  if (!raw || typeof raw !== "object") return null;
  const { maxWidth, maxHeight, scale } = raw as Record<string, unknown>;
  if (typeof maxWidth !== "number" || typeof maxHeight !== "number") return null;
  if (!Number.isFinite(maxWidth) || !Number.isFinite(maxHeight) || maxWidth <= 0 || maxHeight <= 0) return null;
  return { maxWidth, maxHeight, scale: typeof scale === "number" && Number.isFinite(scale) && scale > 0 ? scale : 1 };
}

let current: IslandLimits = NO_LIMITS;
const listeners = new Set<() => void>();

export function getIslandLimits(): IslandLimits {
  return current;
}

/** Replaces the limits; subscribers run only when something changed (the snapshot stays referentially stable). */
export function setIslandLimits(next: IslandLimits): void {
  if (next.maxWidth === current.maxWidth && next.maxHeight === current.maxHeight && next.scale === current.scale) return;
  current = next;
  listeners.forEach((listener) => listener());
}

export function subscribeIslandLimits(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The current limits, re-rendering the component when they change. */
export function useIslandLimits(): IslandLimits {
  return useSyncExternalStore(subscribeIslandLimits, getIslandLimits, getIslandLimits);
}
