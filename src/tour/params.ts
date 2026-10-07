import { TOUR_STEP_COUNT } from "./steps";

export interface TourParams {
  /** 0-based first step. */
  initialStep: number;
  autoplay: boolean;
  /** Regional format for numbers and the clock (BCP-47). */
  format: string;
}

/** The page's own default: Israel's regional format (the tour cannot ask Windows for the real one). */
export const DEFAULT_FORMAT = "he-IL";

/**
 * Reads the address: `?step=N` opens on step N (1-based) without autoplay, for screenshots
 * (`&autoplay=1` keeps it); `?format=en-US` shows the clock and dates the way that region does.
 */
export function parseTourParams(search: string): TourParams {
  const params = new URLSearchParams(search);
  const stepParam = Number.parseInt(params.get("step") ?? "", 10);
  const hasStep = Number.isFinite(stepParam);
  return {
    initialStep: hasStep ? Math.min(TOUR_STEP_COUNT, Math.max(1, stepParam)) - 1 : 0,
    autoplay: hasStep ? params.get("autoplay") === "1" : true,
    format: params.get("format") || DEFAULT_FORMAT,
  };
}
