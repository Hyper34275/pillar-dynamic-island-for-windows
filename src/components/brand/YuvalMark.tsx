import { useId } from "react";
import { MARK_Y_PATH } from "./paths";

// The Yuval icon: an ink circle, an aurora ring (the island's rim) and a glowing Y. This is icon-2.svg /
// icon-2-small.svg from assets/brand as an inline component, so it can sit anywhere a glyph sits at any size.
// Up to 48 px the small drawing is used (a bolder Y, a brighter and thinner ring, no hairlines), as in the .ico.

/** At or below this size the small drawing is used. */
export const MARK_SMALL_MAX = 48;

export interface YuvalMarkProps {
  /** Rendered width and height in px (the mark is square). */
  size?: number;
  /** Force one drawing; by default the size decides. */
  small?: boolean;
  className?: string;
  /** Accessible name; without it the mark is decorative (aria-hidden). */
  title?: string;
}

// Skeleton of the small Y: a long right arm with a hooked tail and a short left arm.
const SMALL_Y_LONG = "M668 268C640 392 568 528 488 640C444 702 392 756 322 764";
const SMALL_Y_SHORT = "M352 286C360 398 424 476 524 508";
const SMALL_TRANSFORM = (scale: number) => `translate(4 6) translate(512 512) scale(${scale}) translate(-512 -512)`;
const GLOW_LAYERS = [160, 124, 92, 64, 40, 20];

function SmallY({ long, short, stroke, opacity = 1, scale = 1 }: { long: number; short: number; stroke: string; opacity?: number; scale?: number }) {
  return (
    <g transform={SMALL_TRANSFORM(scale)} fill="none" stroke={stroke} strokeOpacity={opacity} strokeLinecap="round" strokeLinejoin="round">
      <path d={SMALL_Y_LONG} strokeWidth={long} />
      <path d={SMALL_Y_SHORT} strokeWidth={short} />
    </g>
  );
}

export function YuvalMark({ size = 24, small, className, title }: YuvalMarkProps) {
  // useId returns ":r0:"; colons are legal in an id but awkward in url(#...), so they go.
  const uid = useId().replace(/:/g, "");
  const id = (name: string) => `${uid}-${name}`;
  const ref = (name: string) => `url(#${id(name)})`;
  const compact = small ?? size <= MARK_SMALL_MAX;
  const ringOuter = compact ? 500 : 480;
  const ringInner = compact ? 438 : 440;
  const haloR = compact ? 330 : 360;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 1024 1024"
      className={className}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
    >
      {title ? <title>{title}</title> : null}
      <defs>
        <linearGradient id={id("rim")} gradientUnits="userSpaceOnUse" x1={80} y1={200} x2={940} y2={840}>
          {compact ? (
            <>
              <stop offset={0} stopColor="#5FDCF5" />
              <stop offset={0.3} stopColor="#8C88FF" />
              <stop offset={0.58} stopColor="#D47DFF" />
              <stop offset={0.82} stopColor="#FF72DA" />
              <stop offset={1} stopColor="#FFBCD9" />
            </>
          ) : (
            <>
              <stop offset={0} stopColor="#40C8E0" />
              <stop offset={0.3} stopColor="#5E5CE6" />
              <stop offset={0.58} stopColor="#BF5AF2" />
              <stop offset={0.82} stopColor="#E040C8" />
              <stop offset={1} stopColor="#FFA3C7" />
            </>
          )}
        </linearGradient>
        <radialGradient id={id("ink")} gradientUnits="userSpaceOnUse" cx={512} cy={360} r={560}>
          <stop offset={0} stopColor="#1B1D44" />
          <stop offset={0.55} stopColor="#07070F" />
          <stop offset={1} stopColor="#000000" />
        </radialGradient>
        <linearGradient id={id("y")} gradientUnits="userSpaceOnUse" x1={330} y1={230} x2={640} y2={800}>
          {compact ? (
            <>
              <stop offset={0} stopColor="#FFFFFF" />
              <stop offset={0.5} stopColor="#F1E6FF" />
              <stop offset={1} stopColor="#FFE2F3" />
            </>
          ) : (
            <>
              <stop offset={0} stopColor="#B8F6FF" />
              <stop offset={0.5} stopColor="#D6B4FF" />
              <stop offset={1} stopColor="#FFB0DC" />
            </>
          )}
        </linearGradient>
        <radialGradient id={id("halo")} gradientUnits="userSpaceOnUse" cx={512} cy={520} r={haloR}>
          <stop offset={0} stopColor="#C58CFF" stopOpacity={0.34} />
          <stop offset={1} stopColor="#C58CFF" stopOpacity={0} />
        </radialGradient>
        <radialGradient
          id={id("pool")}
          gradientUnits="userSpaceOnUse"
          cx={512}
          cy={940}
          r={420}
          gradientTransform="translate(512 940) scale(1 0.5) translate(-512 -940)"
        >
          <stop offset={0} stopColor="#BF5AF2" stopOpacity={0.55} />
          <stop offset={1} stopColor="#5E5CE6" stopOpacity={0} />
        </radialGradient>
        <clipPath id={id("clip")}>
          <circle cx={512} cy={512} r={ringInner} />
        </clipPath>
      </defs>
      <circle cx={512} cy={512} r={ringOuter} fill={ref("rim")} />
      <circle cx={512} cy={512} r={ringInner} fill={ref("ink")} />
      <g clipPath={ref("clip")}>
        <ellipse cx={512} cy={940} rx={420} ry={210} fill={ref("pool")} />
        <circle cx={512} cy={520} r={haloR} fill={ref("halo")} />
      </g>
      {compact ? (
        <>
          <SmallY long={260} short={230} stroke="#C58CFF" opacity={0.09} />
          <SmallY long={200} short={170} stroke="#C58CFF" opacity={0.09} />
          <SmallY long={150} short={120} stroke="#C58CFF" opacity={0.09} />
          <SmallY long={142} short={114} stroke={ref("y")} scale={1.02} />
        </>
      ) : (
        <>
          {GLOW_LAYERS.map((w) => (
            <path key={w} d={MARK_Y_PATH} fill="#C58CFF" fillOpacity={0.055} stroke="#C58CFF" strokeOpacity={0.055} strokeWidth={w} strokeLinejoin="round" />
          ))}
          <path d={MARK_Y_PATH} fill={ref("y")} stroke={ref("y")} strokeWidth={6} strokeLinejoin="round" />
        </>
      )}
    </svg>
  );
}
