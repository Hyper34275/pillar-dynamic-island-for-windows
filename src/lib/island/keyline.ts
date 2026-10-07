// The island's adaptive keyline.
//
// The island is a black silhouette. On a bright or mid backdrop it needs nothing: black against
// light is the strongest edge there is, and any permanent border only makes it look like a Windows
// panel. It is on a VERY DARK backdrop (a dark wallpaper, a maximised dark title bar) that black
// shape and background merge, and there Apple's Dynamic Island shows a faint hairline. This module
// decides when, with the three rules that keep it from ever looking like a border:
//   1. it is ONE physical pixel (1 / devicePixelRatio CSS px), an inset shadow, so it follows the
//      shape's own radius exactly and never lies outside the window region;
//   2. it is never on a bright or mid backdrop, and it has hysteresis (on below KEYLINE_ON_BELOW,
//      off only above KEYLINE_OFF_ABOVE) so a backdrop hovering at the threshold does not flicker;
//   3. it only changes while the island is at rest (`atRest`), never mid-morph, and the style that
//      carries it fades (islandEdge.ts), so nothing appears, thickens or changes colour while moving.
//
// The backdrop brightness comes from the backend (backdrop.rs samples a thin ring of screen pixels
// just OUTSIDE the island's window region: those pixels belong to whatever is behind) as Rec. 709
// luma of the gamma-encoded colour, 0 (black) .. 1 (white): the scale people judge "dark" on.
// Unknown (outside Tauri, a locked session, a failed capture) means no keyline.
//
// Windows' "increase contrast" (prefers-contrast: more) asks for visible edges, so there the keyline
// is always drawn, in the stronger --ci-island-keyline colour. Forced colours (a Windows contrast
// theme) keep their own `outline` rule in index.css; box-shadows are dropped there anyway.

import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { ipc, onEvent } from "../ipc";

/** The keyline turns on when the backdrop is darker than this ... */
export const KEYLINE_ON_BELOW = 0.12;
/** ... and off again only when it is brighter than this. */
export const KEYLINE_OFF_ABOVE = 0.18;

/** The next keyline state for a backdrop brightness; unknown (null) or invalid means off. */
export function nextKeyline(luminance: number | null, current: boolean): boolean {
  if (luminance === null || !Number.isFinite(luminance)) return false;
  if (current) return luminance <= KEYLINE_OFF_ABOVE;
  return luminance < KEYLINE_ON_BELOW;
}

/** Rec. 709 luma of an sRGB colour given as 0..255 channels (what backdrop.rs computes). */
export function lumaOf(r: number, g: number, b: number): number {
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

/** One physical pixel in CSS px at this pixel ratio (1 / 1.25 = 0.8, 1 / 1.5 = 0.667, ...). */
export function physicalPixel(devicePixelRatio: number): number {
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  // Rounded to 3 decimals: a stable string (no render churn) that still lands on one device pixel.
  return Math.round((1 / ratio) * 1000) / 1000;
}

/** The keyline's box-shadow: inset, zero blur, one physical pixel. `colour` is a CSS colour or var(). */
export function keylineShadow(devicePixelRatio: number, colour: string): string {
  return `inset 0 0 0 ${physicalPixel(devicePixelRatio)}px ${colour}`;
}

// ---------------------------------------------------------------------------------------------
// The store: the last backdrop brightness and the keyline state derived from it with hysteresis.
// ---------------------------------------------------------------------------------------------
let luminance: number | null = null;
let wanted = false;
const listeners = new Set<() => void>();

export function getKeylineWanted(): boolean {
  return wanted;
}

export function getBackdropLuminance(): number | null {
  return luminance;
}

/** A new backdrop reading (null = unknown). Subscribers run only when the keyline state changes. */
export function setBackdropLuminance(next: number | null): void {
  luminance = next !== null && Number.isFinite(next) ? Math.min(1, Math.max(0, next)) : null;
  const state = nextKeyline(luminance, wanted);
  if (state === wanted) return;
  wanted = state;
  for (const listener of [...listeners]) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** For tests. */
export function resetKeyline(): void {
  luminance = null;
  wanted = false;
  listeners.clear();
}

// One backend subscription for as many hooks as there are mounted.
let users = 0;
let stop: (() => void) | null = null;

function startSync(): void {
  if (users++ > 0) return;
  const offEvent = onEvent<{ luminance: number | null }>("island-backdrop", (payload) => setBackdropLuminance(payload?.luminance ?? null));
  let live = true;
  ipc
    .getIslandBackdrop()
    .then((value) => {
      if (live) setBackdropLuminance(value);
    })
    .catch(() => {
      // the backend cannot answer: no keyline
    });
  stop = () => {
    live = false;
    offEvent();
  };
}

function stopSync(): void {
  if (--users > 0) return;
  stop?.();
  stop = null;
}

function prefersMoreContrast(): boolean {
  try {
    return typeof matchMedia === "function" && matchMedia("(prefers-contrast: more)").matches;
  } catch {
    return false;
  }
}

/**
 * The island's box-shadow keyline, or null for none. `atRest` is false while the island is
 * morphing (or being pressed): the answer then stays what it was, so the keyline never appears,
 * thickens or recolours mid-animation; it catches up as soon as the island rests. Put it on the
 * island element (islandEdge.ts) so it follows the shape's radius.
 */
export function useIslandKeyline(atRest = true): string | null {
  const live = useSyncExternalStore(subscribe, getKeylineWanted, getKeylineWanted);
  const [ratio, setRatio] = useState(() => (typeof window === "undefined" ? 1 : window.devicePixelRatio));
  const [contrast, setContrast] = useState(prefersMoreContrast);
  const shown = useRef(live);
  if (atRest) shown.current = live;

  useEffect(() => {
    startSync();
    return stopSync;
  }, []);

  useEffect(() => {
    const onResize = () => setRatio(window.devicePixelRatio);
    window.addEventListener("resize", onResize);
    const onContrast = () => setContrast(prefersMoreContrast());
    let media: MediaQueryList | null = null;
    try {
      media = typeof matchMedia === "function" ? matchMedia("(prefers-contrast: more)") : null;
      media?.addEventListener("change", onContrast);
    } catch {
      media = null;
    }
    return () => {
      window.removeEventListener("resize", onResize);
      media?.removeEventListener("change", onContrast);
    };
  }, []);

  if (contrast || shown.current) return keylineShadow(ratio, "var(--ci-island-keyline)");
  return null;
}
