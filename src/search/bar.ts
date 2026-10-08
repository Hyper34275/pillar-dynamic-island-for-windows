import { glow } from "../design/tokens";
import type { SearchBarState } from "../lib/assistant/types";

/** Floating bar until the backend says otherwise: 560x48 DIP plus the glow margin. */
export const DEFAULT_BAR: SearchBarState = {
  anchored: false,
  width: 560 + 2 * glow.margin,
  height: 48 + 2 * glow.margin,
  radius: 4,
  scale: 1,
  highContrast: false,
  edge: "bottom",
};

/** The backend's state, made safe: finite sizes, never too small to hold the glow margin and an input. */
export function sanitizeBar(raw: Partial<SearchBarState> | null | undefined): SearchBarState {
  const n = (v: unknown, fallback: number, min: number) => (typeof v === "number" && Number.isFinite(v) && v >= min ? v : fallback);
  if (!raw) return DEFAULT_BAR;
  const min = 2 * glow.margin + 16;
  return {
    anchored: raw.anchored === true,
    width: n(raw.width, DEFAULT_BAR.width, min),
    height: n(raw.height, DEFAULT_BAR.height, min),
    radius: n(raw.radius, 0, 0),
    scale: n(raw.scale, 1, 0.1),
    highContrast: raw.highContrast === true,
    edge: typeof raw.edge === "string" ? raw.edge : "bottom",
  };
}
