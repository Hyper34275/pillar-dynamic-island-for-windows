import { useId } from "react";
import { WORDMARK_B_PATH, WORDMARK_VIEWBOX } from "./paths";

// The Yuval wordmark: the name in a connected brush signature (an outline drawing derived from the Mr Dafoe font,
// SIL OFL 1.1; see assets/brand/LOGO_NOTES.md). It is wordmark-b*.svg as an inline component.

const [, , VB_W, VB_H] = WORDMARK_VIEWBOX.split(" ").map(Number);

/** Width divided by height of the wordmark's box. */
export const WORDMARK_ASPECT = VB_W / VB_H;

export type WordmarkTone =
  /** Follows the surrounding text colour (the default; works on any theme). */
  | "current"
  /** White, for dark backgrounds (wordmark-b-dark.svg). */
  | "dark"
  /** Near-black, for light backgrounds (wordmark-b-light.svg). */
  | "light"
  /** The aurora gradient (wordmark-b.svg); readable on both backgrounds. */
  | "aurora";

export interface YuvalWordmarkProps {
  /** Rendered height in px; the width follows from the aspect ratio. */
  height?: number;
  tone?: WordmarkTone;
  className?: string;
  /** Accessible name; the default is the product name. */
  title?: string;
}

const AURORA_STOPS: readonly [number, string][] = [
  [0, "#2FB4DA"],
  [0.3, "#5E5CE6"],
  [0.58, "#B24FEA"],
  [0.82, "#E040C8"],
  [1, "#F870B4"],
];

export function YuvalWordmark({ height = 24, tone = "current", className, title = "Yuval" }: YuvalWordmarkProps) {
  const uid = useId().replace(/:/g, "");
  const gradient = `${uid}-aurora`;
  const fill = tone === "current" ? "currentColor" : tone === "dark" ? "#FFFFFF" : tone === "light" ? "#14141C" : `url(#${gradient})`;
  return (
    <svg
      width={height * WORDMARK_ASPECT}
      height={height}
      viewBox={WORDMARK_VIEWBOX}
      className={className}
      role="img"
      aria-label={title}
      focusable="false"
    >
      <title>{title}</title>
      {tone === "aurora" ? (
        <defs>
          <linearGradient id={gradient} gradientUnits="userSpaceOnUse" x1={-124} y1={-501} x2={2196} y2={21}>
            {AURORA_STOPS.map(([offset, color]) => (
              <stop key={offset} offset={offset} stopColor={color} />
            ))}
          </linearGradient>
        </defs>
      ) : null}
      <path d={WORDMARK_B_PATH} fill={fill} />
    </svg>
  );
}
