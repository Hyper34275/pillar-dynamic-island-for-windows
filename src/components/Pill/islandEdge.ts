// The island's edge: nothing, or the adaptive keyline (lib/island/keyline.ts).
//
// Why this exists instead of a constant hairline: the island used to carry a permanent
// `inset 0 0 0 0.5px rgba(255,255,255,0.10)`. Measured on the live window (PrintWindow of the
// installed build) that painted a 1 px rim of rgb(14,14,14) on EVERY edge, in every state, and
// under prefers-contrast: more (tokens.ts) rgb(77,77,77): a permanent grey line that made a black
// silhouette read as a bordered panel. Now the edge is absent unless the backdrop behind the island
// is almost black, and then it is one physical pixel at ~14% white.
//
// The transition is on the shadow only (a fade between none and the keyline), short, and the
// keyline never changes while the island morphs (`useIslandKeyline(atRest)`).

import type { CSSProperties } from "react";

/** The island element's edge styles for `useIslandKeyline()`'s answer. Merge into its `style`. */
export function islandEdgeStyle(keyline: string | null): Pick<CSSProperties, "boxShadow" | "transition"> {
  return {
    boxShadow: keyline ?? "none",
    // Both ends are inset shadows (none is an empty list), so this interpolates as a fade.
    transition: "box-shadow 160ms ease-out",
  };
}
