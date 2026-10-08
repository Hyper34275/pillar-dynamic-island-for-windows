import { useEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { glow as g, spotlight } from "../design/tokens";
import type { SearchBarState } from "../lib/assistant/types";
import { glowAnimates, sweepPeriodSeconds, type GlowMode, type GlowState } from "./searchState";
import "./spotlight.css";

export type SpotlightCapsuleProps = {
  bar: SearchBarState;
  glow: GlowState;
  mode: GlowMode;
  /** document.hidden: whatever moves is paused. */
  hidden: boolean;
  rtl: boolean;
  /** The field holds text: the shortcut chip gives way to the Enter chip. */
  hasText: boolean;
  /** The text input (owned by SearchBar, which keeps all the logic). */
  input: ReactNode;
  keysLabel: string;
  statusText: string;
  /** A click on the capsule outside the input hands the focus back to it. */
  onBackground: () => void;
};

/** Base turn of the rim; the playback rate scales it per state so the rim never jumps. */
const BASE_PERIOD_S = 12;

/**
 * The spotlight variant of the search bar: a black-and-white capsule with a hairline rim whose white
 * highlight drifts, a soft coloured halo hugging it inside the window margin, and a breathing spark.
 * Three composited layers at most: the rim (transform), the halo (opacity), the spark (transform).
 */
export function SpotlightCapsule({ bar, glow, mode, hidden, rtl, hasText, input, keysLabel, statusText, onBackground }: SpotlightCapsuleProps) {
  const rimRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<Animation | null>(null);
  const animating = glowAnimates(glow, mode, hidden);
  const period = sweepPeriodSeconds(glow);
  const margin = spotlight.margin;
  const rate = period ? BASE_PERIOD_S / period : 1;

  // Rim drift: one transform animation while the window lives; states only change its rate, and
  // it is paused (not removed) when idle, hidden or completed so nothing moves for nothing.
  useEffect(() => {
    const el = rimRef.current;
    if (mode !== "full" || !el || typeof el.animate !== "function") {
      animRef.current?.cancel();
      animRef.current = null;
      return;
    }
    if (!animRef.current) {
      animRef.current = el.animate([{ transform: "translate(-50%, -50%) rotate(0deg)" }, { transform: "translate(-50%, -50%) rotate(360deg)" }], {
        duration: BASE_PERIOD_S * 1000,
        iterations: Infinity,
        easing: "linear",
      });
    }
    const a = animRef.current;
    if (animating) {
      a.updatePlaybackRate(rate);
      a.play();
    } else {
      a.pause();
    }
  }, [mode, animating, rate]);

  useEffect(
    () => () => {
      animRef.current?.cancel();
      animRef.current = null;
    },
    [],
  );

  const vars = {
    width: bar.width,
    height: bar.height,
    "--sp-margin": `${margin}px`,
    "--sp-radius": `${Math.min(bar.radius, (bar.height - 2 * margin) / 2)}px`,
    "--sp-ink-top": spotlight.inkTop,
    "--sp-ink-bottom": spotlight.inkBottom,
    "--sp-cyan": g.cyan,
    "--sp-violet": g.violet,
    "--sp-magenta": g.magenta,
    "--sp-error": g.error,
  } as CSSProperties;

  return (
    <div className="sp-root" style={vars} data-state={glow} data-mode={mode} data-moving={animating ? "on" : "off"} dir={rtl ? "rtl" : "ltr"}>
      <div className="sp-halo" aria-hidden="true" />
      <div
        className="sp-capsule"
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).tagName !== "INPUT") {
            e.preventDefault();
            onBackground();
          }
        }}
      >
        <div className="sp-rim" ref={rimRef} aria-hidden="true" />
        <div className="sp-body">
          <span className="sp-spark" aria-hidden="true">
            <SparkMark />
          </span>
          {input}
          <span className="sp-chip" title={keysLabel} dir="ltr" data-kind={hasText ? "enter" : "keys"}>
            {hasText ? (
              <kbd>Enter ↵</kbd>
            ) : (
              <>
                <kbd>Alt</kbd>
                <i>+</i>
                <kbd>`</kbd>
              </>
            )}
          </span>
        </div>
      </div>
      <span className="sp-status" role="status" aria-live="polite">
        {statusText}
      </span>
    </div>
  );
}

/** The AI sparkle: one large four-point star and a small companion, in white like the rest of the capsule. */
function SparkMark() {
  return (
    <svg width="28" height="28" viewBox="0 0 26 26" focusable="false">
      <defs>
        <linearGradient id="sp-spark" x1="2" y1="24" x2="24" y2="2" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#A8A8AE" />
          <stop offset="1" stopColor="#FFFFFF" />
        </linearGradient>
      </defs>
      <path
        className="sp-star"
        fill="url(#sp-spark)"
        d="M11 3c.6 4.6 2.4 7.4 7 8-4.6.6-6.4 3.4-7 8-.6-4.6-2.4-7.4-7-8 4.6-.6 6.4-3.4 7-8z"
      />
      <path className="sp-star" fill="#C8C8CD" d="M20 15c.3 2.3 1.2 3.7 3.5 4-2.3.3-3.2 1.7-3.5 4-.3-2.3-1.2-3.7-3.5-4 2.3-.3 3.2-1.7 3.5-4z" />
    </svg>
  );
}
