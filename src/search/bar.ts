import { glow, spotlight } from "../design/tokens";
import type { SearchBarState, SearchBarVariant } from "../lib/assistant/types";

/** Floating bar until the backend says otherwise: 560x48 DIP plus the glow margin. */
export const DEFAULT_BAR: SearchBarState = {
  variant: "floating",
  anchored: false,
  width: 560 + 2 * glow.margin,
  height: 48 + 2 * glow.margin,
  radius: 4,
  scale: 1,
  highContrast: false,
  edge: "bottom",
};

/** Spotlight window until the backend says otherwise: the 680 DIP sheet plus its side margins, as tall as the tallest sheet plus the margins above and below. */
export const SPOTLIGHT_BAR: SearchBarState = {
  variant: "spotlight",
  anchored: false,
  width: spotlight.width + 2 * spotlight.marginX,
  height: spotlight.marginTop + spotlight.sheetMax + spotlight.marginBottom,
  radius: spotlight.radius,
  scale: 1,
  highContrast: false,
  edge: "bottom",
};

const VARIANTS: readonly SearchBarVariant[] = ["taskbar", "floating", "spotlight"];

/** The backend's state, made safe: finite sizes, never too small to hold the glow margin and an input. */
export function sanitizeBar(raw: Partial<SearchBarState> | null | undefined): SearchBarState {
  const n = (v: unknown, fallback: number, min: number) => (typeof v === "number" && Number.isFinite(v) && v >= min ? v : fallback);
  if (!raw) return DEFAULT_BAR;
  const anchored = raw.anchored === true;
  // An older backend sends no variant: anchored means the Windows 10 box, otherwise the floating bar.
  const variant: SearchBarVariant = VARIANTS.includes(raw.variant as SearchBarVariant)
    ? (raw.variant as SearchBarVariant)
    : anchored
      ? "taskbar"
      : "floating";
  const common = {
    scale: n(raw.scale, 1, 0.1),
    highContrast: raw.highContrast === true,
    edge: typeof raw.edge === "string" ? raw.edge : "bottom",
  };
  if (variant === "spotlight") {
    // The page draws its own sheet: fixed radius, never anchored.
    return {
      variant,
      anchored: false,
      width: n(raw.width, SPOTLIGHT_BAR.width, 2 * spotlight.marginX + 16),
      height: n(raw.height, SPOTLIGHT_BAR.height, spotlight.marginTop + spotlight.marginBottom + 16),
      radius: spotlight.radius,
      ...common,
    };
  }
  const min = 2 * glow.margin + 16;
  return {
    variant,
    anchored,
    width: n(raw.width, DEFAULT_BAR.width, min),
    height: n(raw.height, DEFAULT_BAR.height, min),
    radius: n(raw.radius, 0, 0),
    ...common,
  };
}
