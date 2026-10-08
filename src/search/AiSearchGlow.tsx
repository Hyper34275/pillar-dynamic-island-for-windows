import { useEffect, useRef, useState, type CSSProperties } from "react";
import { glow as glowTokens } from "../design/tokens";
import { glowAnimates, glowVisible, sweepPeriodSeconds, type GlowMode, type GlowState } from "./searchState";
import "./glow.css";

export type AiSearchGlowProps = {
  state: GlowState;
  mode: GlowMode;
  /** The window's size in DIPs: the bar plus the glow margin on every side. */
  width: number;
  height: number;
  /** The bar's own corner radius. */
  radius: number;
  /** Room around the bar for the aura (0 when sitting flush on the taskbar search box). */
  margin?: number;
  /** document.hidden: whatever moves is paused. */
  hidden?: boolean;
};

/** Base turn used for the Web Animations rotation; the playback rate scales it per state. */
const BASE_PERIOD_S = 12;

/**
 * The glow behind the search bar. It renders nothing in Idle and Disabled (the layers are removed,
 * not hidden). The bar on top paints the light plate, inset by the ring width, so only a thin ring
 * and the aura show. It never takes pointer events or focus.
 */
export function AiSearchGlow({ state, mode, width, height, radius, margin = glowTokens.margin, hidden = false }: AiSearchGlowProps) {
  const visible = glowVisible(state);
  const [entered, setEntered] = useState(false);
  const sweepRef = useRef<HTMLDivElement | null>(null);
  const animRef = useRef<Animation | null>(null);

  // Fade in from transparent on mount.
  useEffect(() => {
    if (!visible) {
      setEntered(false);
      return;
    }
    const id = requestAnimationFrame(() => setEntered(true));
    return () => cancelAnimationFrame(id);
  }, [visible]);

  const animating = visible && glowAnimates(state, mode, hidden);
  const period = sweepPeriodSeconds(state);

  // Rotation: transform only, compositor-driven. One animation object lives while the layers do;
  // a new state changes its playback rate, so the ring never jumps.
  useEffect(() => {
    const el = sweepRef.current;
    if (!visible || mode !== "full" || !el || typeof el.animate !== "function") {
      animRef.current?.cancel();
      animRef.current = null;
      return;
    }
    if (!animRef.current) {
      animRef.current = el.animate([{ transform: "rotate(0deg)" }, { transform: "rotate(360deg)" }], {
        duration: BASE_PERIOD_S * 1000,
        iterations: Infinity,
        easing: "linear",
      });
    }
    const anim = animRef.current;
    if (animating && period !== null) {
      anim.updatePlaybackRate(BASE_PERIOD_S / period);
      anim.play();
    } else {
      anim.pause(); // Completed / Error / hidden: hold the angle
    }
  }, [visible, mode, animating, period]);

  useEffect(
    () => () => {
      animRef.current?.cancel();
      animRef.current = null;
    },
    [],
  );

  if (!visible) return null;

  const barW = Math.max(0, width - 2 * margin);
  const barH = Math.max(0, height - 2 * margin);
  const barRadius = Math.max(0, Math.min(radius, barH / 2));
  const style = {
    "--g-m": `${margin}px`,
    "--g-rr": `${barRadius}px`,
    "--g-hr": `${Math.min(barRadius + margin, height / 2)}px`,
    "--g-s": `${Math.ceil(Math.hypot(barW, barH))}px`,
    "--g-cyan": glowTokens.cyan,
    "--g-violet": glowTokens.violet,
    "--g-indigo": glowTokens.indigo,
    "--g-magenta": glowTokens.magenta,
    "--g-pink": glowTokens.softPink,
    "--g-error": glowTokens.error,
  } as CSSProperties;

  if (mode === "plain") {
    return (
      <div className="ci-glow ci-glow--plain" data-state={state} data-mode={mode} style={style} aria-hidden="true">
        <div className="ci-glow__plain" />
      </div>
    );
  }

  return (
    <div
      className="ci-glow"
      data-state={state}
      data-mode={mode}
      data-animated={animating ? "true" : "false"}
      data-paused={hidden ? "true" : "false"}
      data-entered={entered ? "true" : "false"}
      style={style}
      aria-hidden="true"
    >
      <div className="ci-glow__aura" />
      <div className="ci-glow__ring">
        <div className="ci-glow__sweep" ref={sweepRef} />
      </div>
    </div>
  );
}
