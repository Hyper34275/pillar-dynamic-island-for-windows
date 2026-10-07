import type { ReactNode, SyntheticEvent } from "react";
import type { IslandSize } from "../components/Pill/animations";
import { CrossFade } from "./crossFade";

// React 18 has no `inert` prop: as a plain attribute it does the same (nothing inside can be
// focused, clicked or read by assistive tech).
const INERT = { inert: "" } as Record<string, string>;

// Belt and braces next to inert and pointer-events: a click, press or key that still reached the
// stage (an engine without inert, a script) is stopped before any handler of the real components
// below sees it, so none of them can call the backend, open a link or start Outlook.
const swallow = (e: SyntheticEvent) => {
  e.stopPropagation();
  e.preventDefault();
};

interface TourStageProps {
  size: IslandSize;
  /** What the island shows; a new key cross-fades, the same key only re-renders. */
  layer: string;
  children: ReactNode;
}

/**
 * A faux screen with the island at its top centre. Physically left to right like the island
 * itself, and not interactive in any way: inert, hidden from assistive tech (the explanation next
 * to it says the same in words) and deaf to the pointer, so nothing in it can reach the backend,
 * a link or Outlook. The island's size follows the real size functions through a CSS transition
 * (tour.css), which also runs under reduced motion, like the real island's morph.
 */
export function TourStage({ size, layer, children }: TourStageProps) {
  return (
    <div
      className="tour-screen"
      dir="ltr"
      aria-hidden="true"
      data-testid="tour-stage"
      style={{ pointerEvents: "none" }}
      onClickCapture={swallow}
      onAuxClickCapture={swallow}
      onContextMenuCapture={swallow}
      onMouseDownCapture={swallow}
      onPointerDownCapture={swallow}
      onPointerUpCapture={swallow}
      onKeyDownCapture={swallow}
      onKeyUpCapture={swallow}
      onSubmitCapture={swallow}
      {...INERT}
    >
      <div className="tour-island" data-layer={layer} style={{ width: size.width, height: size.height, borderRadius: size.radius }}>
        <CrossFade id={layer}>{children}</CrossFade>
      </div>
    </div>
  );
}
