// Text width measurement for sizing the collapsed island. Uses a shared canvas context;
// without a canvas (tests, very old engines) it falls back to a per-character estimate.

let context: CanvasRenderingContext2D | null | undefined;

function getContext(): CanvasRenderingContext2D | null {
  if (context !== undefined) return context;
  try {
    context = typeof document === "undefined" ? null : document.createElement("canvas").getContext("2d");
  } catch {
    context = null;
  }
  return context;
}

/** Average glyph width as a fraction of font size, for the no-canvas fallback. */
const ESTIMATED_EM = 0.56;

export function measureText(text: string, font: string, fontSizePx: number): number {
  const ctx = getContext();
  if (!ctx) return Math.ceil(text.length * fontSizePx * ESTIMATED_EM);
  ctx.font = font;
  return Math.ceil(ctx.measureText(text).width);
}
