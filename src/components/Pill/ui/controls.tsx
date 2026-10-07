import type { ButtonHTMLAttributes, CSSProperties, MouseEvent, PointerEvent, ReactNode } from "react";
import { color, control } from "../../../design/tokens";

// =============================================================================
// Action controls: the one button family of the island. Every action a person can take in an
// expanded card (join, snooze, accept / maybe / decline) is an ActionButton inside an ActionRow,
// which fills the card's content width with equal columns. Small icon-only controls (dismiss,
// join in a list row, silence) are RoundButtons with a 44px hit area.
// =============================================================================

export type ActionVariant = "primary" | "neutral" | "destructive";

/**
 * Normal / hover / pressed per variant. Primary is the one filled control (affirmative: join,
 * accept); neutral is the quiet secondary (snooze, maybe); destructive is a tint, never a fill, so
 * it is identifiable without being the loudest thing in the card.
 */
const VARIANTS: Record<ActionVariant, { bg: string; hover: string; pressed: string; fg: string }> = {
  primary: { bg: color.positiveFill, hover: color.positiveFillHover, pressed: color.positiveFillPressed, fg: "#FFFFFF" },
  neutral: { bg: color.fill, hover: color.fillHover, pressed: color.fillPressed, fg: color.fg },
  destructive: { bg: color.destructiveSoft, hover: color.destructiveSoftHover, pressed: color.destructiveSoft, fg: color.destructiveText },
};

/** Buttons inside clickable surfaces (a toast, a card, the island) must not trigger the surface too. */
function stop(e: MouseEvent | PointerEvent) {
  e.stopPropagation();
}

interface ActionButtonProps {
  variant?: ActionVariant;
  onPress: () => void;
  children: ReactNode;
  /** A glyph before the label (16px, tokens.control.iconSize). */
  icon?: ReactNode;
  ariaLabel?: string;
  disabled?: boolean;
  /** A toggle's state (aria-pressed), e.g. the meeting's silence switch. */
  pressed?: boolean;
  title?: string;
  className?: string;
}

/**
 * A 40px action surface with a 44px hit area (`hit-area`), its label centred (the label is a
 * plain line box of the label role, so Hebrew and Latin sit on the same optical centre). States:
 * hover lightens the fill, press darkens it and squeezes to 0.98 at once (no spring, no bounce),
 * keyboard focus draws the global ring around the capsule's own radius, disabled is 40% opacity.
 */
export function ActionButton({ variant = "neutral", onPress, children, icon, ariaLabel, disabled, pressed, title, className = "" }: ActionButtonProps) {
  const v = VARIANTS[variant];
  const style = {
    "--btn-bg": v.bg,
    "--btn-bg-hover": v.hover,
    "--btn-bg-pressed": v.pressed,
    color: v.fg,
    height: control.height,
    paddingInline: control.paddingX,
    gap: control.gap,
  } as CSSProperties;
  return (
    <button
      type="button"
      className={`ci-action hit-area min-w-0 inline-flex items-center justify-center rounded-control text-label whitespace-nowrap select-none disabled:opacity-40 disabled:cursor-default ${className}`}
      style={style}
      aria-label={ariaLabel}
      aria-pressed={pressed}
      title={title}
      disabled={disabled}
      onPointerDown={stop}
      onClick={(e) => {
        stop(e);
        onPress();
      }}
    >
      {icon && (
        <span className="flex-shrink-0 flex" aria-hidden="true">
          {icon}
        </span>
      )}
      <span className="min-w-0 truncate">{children}</span>
    </button>
  );
}

/**
 * A row of actions that fills the content width: equal columns, 8px apart, whatever the labels.
 * In RTL the first action is the rightmost (the layout's leading edge).
 */
export function ActionRow({ children, className = "", style }: { children: ReactNode; className?: string; style?: CSSProperties }) {
  return (
    <div className={`grid grid-flow-col auto-cols-fr ${className}`} style={{ gap: control.gap, ...style }}>
      {children}
    </div>
  );
}

interface RoundButtonProps {
  onPress: () => void;
  ariaLabel: string;
  children: ReactNode;
  /** Glyph colour; defaults to secondary text. */
  tint?: string;
  /** Background: neutral fill by default; "tint" uses the tint at 18%; "none" is a bare glyph that fills on hover. */
  fill?: "neutral" | "tint" | "none";
  size?: number;
  pressed?: boolean;
  title?: string;
  className?: string;
  /** The capsule grows with a short text label (a confirming "Delete?") instead of staying round; same height, same states. */
  grow?: boolean;
  disabled?: boolean;
  /** Extra attributes for the same element (data-*, focus / key handlers): a control that changes content in place keeps its focus. */
  buttonProps?: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "type" | "className" | "style" | "onClick" | "onPointerDown" | "aria-label" | "title"> & { [dataAttr: `data-${string}`]: string | undefined };
}

/** An icon-only round control (dismiss, join, silence): 28px visible, 44px hit area. */
export function RoundButton({ onPress, ariaLabel, children, tint, fill = "neutral", size = control.round, pressed, title, className = "", grow = false, disabled, buttonProps }: RoundButtonProps) {
  const fg = tint ?? color.fgSecondary;
  const bg = fill === "neutral" ? color.fill : fill === "tint" ? `color-mix(in srgb, ${fg} 18%, transparent)` : "transparent";
  const hover = fill === "neutral" ? color.fillHover : fill === "tint" ? `color-mix(in srgb, ${fg} 28%, transparent)` : color.fill;
  const style = { "--btn-bg": bg, "--btn-bg-hover": hover, "--btn-bg-pressed": bg, color: fg, height: size, ...(grow ? { minWidth: size, paddingInline: control.gap } : { width: size }) } as unknown as CSSProperties;
  return (
    <button
      type="button"
      className={`ci-action hit-area flex-shrink-0 rounded-full flex items-center justify-center disabled:opacity-25 disabled:pointer-events-none ${grow ? "text-meta whitespace-nowrap" : ""} ${className}`}
      style={style}
      {...buttonProps}
      aria-label={ariaLabel}
      aria-pressed={pressed}
      title={title}
      disabled={disabled}
      onPointerDown={stop}
      onClick={(e) => {
        stop(e);
        onPress();
      }}
    >
      {children}
    </button>
  );
}
