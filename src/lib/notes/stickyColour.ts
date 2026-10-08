import { identityColors } from "../../design/tokens";
import type { StickyColour } from "./sticky";

// The small colour mark of a Sticky Note card. Identity only (which note is which), never status, so the
// island's identity tints are used where they have the hue; the three Sticky Notes colours the palette
// does not have are the same iOS system tints the identity set is taken from.
// identityColors: [0] blue, [1] green, [2] purple, [4] pink.
const MARKS: Record<StickyColour, string> = {
  yellow: "#FFD60A",
  green: identityColors[1],
  blue: identityColors[0],
  purple: identityColors[2],
  pink: identityColors[4],
  gray: "#98989D",
  charcoal: "#636366",
};

export function stickyMarkColor(colour: StickyColour): string {
  return MARKS[colour] ?? MARKS.yellow;
}
