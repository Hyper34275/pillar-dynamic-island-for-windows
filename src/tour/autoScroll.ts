import { useEffect, type RefObject } from "react";
import type { TabId } from "../components/Pill/tabs";
import { t, type MessageKey } from "../lib/i18n";

/** The panel starts to move this long after its step appears (the cross-fade and the morph first). */
export const SCROLL_START_MS = 900;
/** Time between two stops of the panel; one autoplay step (5 s) fits three of them. */
export const SCROLL_STEP_MS = 1500;

/** A stop of the scroll: an element of the panel to bring to the top, or the very end. */
export type ScrollStop = (panel: HTMLElement) => HTMLElement | null | "end";

/** The element carrying this message as its aria-label (compared as text, so no selector escaping). */
const labelled = (key: MessageKey): ScrollStop => (panel) => [...panel.querySelectorAll<HTMLElement>("[aria-label]")].find((el) => el.getAttribute("aria-label") === t(key)) ?? null;

const section = (name: string): ScrollStop => (panel) => panel.querySelector<HTMLElement>(`[data-section="${name}"]`);

/**
 * Where the (inert, never user-scrolled) panel of each tab moves to, in order. Settings is the
 * longest: it shows the island display choice, then the Island Center and tour buttons, then the
 * end of the diagnostics. The other lists simply end at the bottom of their content.
 */
export const SCROLL_STOPS: Partial<Record<TabId, ScrollStop[]>> = {
  calendar: [() => "end"],
  notifications: [() => "end"],
  notes: [() => "end"],
  settings: [labelled("settings.islandDisplay"), section("center"), () => "end"],
};

const prefersReducedMotion = () => typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The scrollTop that puts `target` (or the end) at the top of the panel, whatever scale the stage is drawn at. */
export function scrollTopFor(panel: HTMLElement, target: HTMLElement | null | "end"): number | null {
  const max = Math.max(0, panel.scrollHeight - panel.clientHeight);
  if (target === "end") return max;
  if (!target) return null;
  const panelRect = panel.getBoundingClientRect();
  const scale = panel.clientHeight > 0 && panelRect.height > 0 ? panelRect.height / panel.clientHeight : 1;
  const offset = (target.getBoundingClientRect().top - panelRect.top) / scale + panel.scrollTop;
  return Math.min(max, Math.max(0, offset - 4));
}

function scrollPanel(panel: HTMLElement, top: number) {
  const behavior = prefersReducedMotion() ? "auto" : "smooth";
  if (typeof panel.scrollTo === "function") panel.scrollTo({ top, behavior });
  else panel.scrollTop = top;
}

/**
 * Moves the panel of the tab on screen through its stops, with one timer at a time (the next is
 * set when the previous fires; cleanup clears it). The stage stays inert: this is code scrolling,
 * not the pointer, and it runs under reduced motion too (then without the smooth animation).
 * `host` holds the cross-fading layers; the layer of the current tab is the one to move.
 */
export function useAutoScroll(host: RefObject<HTMLElement | null>, tab: TabId) {
  useEffect(() => {
    const stops = SCROLL_STOPS[tab];
    if (!stops || stops.length === 0) return;
    let index = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const next = () => {
      const layers = host.current?.querySelectorAll<HTMLElement>(`:scope > [data-layer-id="${tab}"]`);
      const panel = layers && layers[layers.length - 1];
      if (panel) {
        const top = scrollTopFor(panel, stops[index](panel));
        if (top !== null) scrollPanel(panel, top);
      }
      index += 1;
      if (index < stops.length) timer = setTimeout(next, SCROLL_STEP_MS);
    };
    timer = setTimeout(next, SCROLL_START_MS);
    return () => clearTimeout(timer);
  }, [host, tab]);
}
