import { motion } from "motion/react";
import type { ReactNode } from "react";
import { useId } from "react";
import { color } from "../../../design/tokens";

// =============================================================================
// Island design primitives, on the design tokens (src/design/tokens.ts). Everything sits on a
// pure-black island, so depth comes from the translucent surface / fill tokens, not borders.
// =============================================================================

const KNOWN_APPS: Array<[RegExp, string]> = [
  [/applemusic|itunes/i, "Apple Music"],
  [/spotify/i, "Spotify"],
  [/youtube/i, "YouTube"],
  [/chrome/i, "Chrome"],
  [/firefox/i, "Firefox"],
  [/msedge|edge/i, "Edge"],
  [/vlc/i, "VLC"],
  [/plex/i, "Plex"],
  [/zunemusic|groove|media\s?player/i, "Media Player"],
  [/discord/i, "Discord"],
  [/teams/i, "Teams"],
  [/whatsapp/i, "WhatsApp"],
  [/telegram/i, "Telegram"],
  [/outlook/i, "Outlook"],
  [/slack/i, "Slack"],
  [/tidal/i, "TIDAL"],
  [/deezer/i, "Deezer"],
  [/amazonmusic/i, "Amazon Music"],
];

const appNameCache = new Map<string, string>();

/**
 * Turn raw source ids ("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App",
 * "Spotify.exe", "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic")
 * into a short human label.
 */
export function cleanAppName(raw: string | undefined | null): string {
  if (!raw?.trim()) return "";
  const cached = appNameCache.get(raw);
  if (cached) return cached;

  let result: string | null = null;
  for (const [pattern, label] of KNOWN_APPS) {
    if (pattern.test(raw)) {
      result = label;
      break;
    }
  }

  if (!result) {
    // Strip package family suffix (_hash), entry point (!App), and .exe
    let base = raw.split("!")[0].split("_")[0].replace(/\.exe$/i, "");
    // Reverse-DNS package ids: keep the last meaningful segment
    if (base.includes(".")) {
      const parts = base.split(".").filter(Boolean);
      base = parts[parts.length - 1] ?? base;
    }
    // Split CamelCase and trailing "Win"
    base = base.replace(/Win$/, "").replace(/([a-z])([A-Z])/g, "$1 $2").trim();
    result = base.length > 20 ? `${base.slice(0, 20)}…` : base || raw;
  }

  if (appNameCache.size > 200) appNameCache.clear();
  appNameCache.set(raw, result);
  return result;
}

// -----------------------------------------------------------------------------
// Segmented control with a sliding thumb: a capsule (radius = height / 2) on the island black;
// the selected thumb is the selection token, concentric with the capsule (inset 2).
// -----------------------------------------------------------------------------

interface SegmentedProps<T extends string> {
  options: ReadonlyArray<{ id: T; label: ReactNode; ariaLabel?: string }>;
  value: T;
  onChange: (id: T) => void;
  /** sm: 28 high (inside a list row), md: 32 high. */
  size?: "sm" | "md";
  className?: string;
  ariaLabel?: string;
}

export function Segmented<T extends string>({ options, value, onChange, size = "sm", className = "", ariaLabel }: SegmentedProps<T>) {
  const groupId = useId();
  // Container = thumb height + 2 × 2 inset.
  const thumbHeight = size === "sm" ? 24 : 28;
  return (
    <div
      className={`relative inline-flex items-center rounded-full ${className}`}
      style={{ padding: 2, background: color.island, boxShadow: `inset 0 0 0 1px ${color.outline}` }}
      role="group"
      aria-label={ariaLabel}
    >
      {options.map((opt) => {
        const active = opt.id === value;
        return (
          <button
            key={opt.id}
            type="button"
            // hit-area: 44px tall target around the 24px segment (the row has room; segments are wider than 44).
            className={`ci-seg hit-area relative flex-1 px-3 rounded-full whitespace-nowrap transition-colors ${size === "sm" ? "text-meta" : "text-label"} ${
              active ? "text-fg" : "text-fg-tertiary hover:text-fg-secondary"
            }`}
            style={{ height: thumbHeight }}
            aria-pressed={active}
            aria-label={opt.ariaLabel}
            onClick={() => onChange(opt.id)}
          >
            {active && (
              <motion.span
                layoutId={`seg-${groupId}`}
                className="ci-selected absolute inset-0 rounded-full"
                style={{ background: color.selection }}
                transition={{ type: "spring", stiffness: 520, damping: 38 }}
              />
            )}
            <span className="relative">{opt.label}</span>
          </button>
        );
      })}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Switch: 40 × 24 (thumb 20, inset 2), the positive green when on. Smaller than the iOS 51 × 31
// because the island's rows are 44 high with 12 padding. The thumb slides along the inline axis,
// so it rests at the leading edge in both directions without knowing which one it is.
// -----------------------------------------------------------------------------

const SWITCH = { width: 40, height: 24, thumb: 20, inset: 2 } as const;

export function Switch({
  checked,
  onChange,
  label,
  tint = color.positive,
}: {
  checked: boolean;
  onChange: () => void;
  label: string;
  tint?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={onChange}
      className="hit-area relative rounded-full flex-shrink-0 transition-colors duration-200"
      style={{ width: SWITCH.width, height: SWITCH.height, background: checked ? tint : color.fill }}
    >
      <span
        className="absolute rounded-full bg-white transition-[inset-inline-start] duration-200 ease-out"
        style={{
          top: SWITCH.inset,
          width: SWITCH.thumb,
          height: SWITCH.thumb,
          insetInlineStart: checked ? SWITCH.width - SWITCH.thumb - SWITCH.inset : SWITCH.inset,
          boxShadow: "0 1px 3px rgba(0,0,0,0.3)",
        }}
        aria-hidden="true"
      />
    </button>
  );
}

// -----------------------------------------------------------------------------
// Inset grouped list: one surface card (radius 18). Rows are separated by a hairline inset 12 from
// both edges (the rows' own text inset), drawn by a pseudo-element on every row after the first.
// -----------------------------------------------------------------------------

/** Shared by every grouped list (also `<ul>` lists): the surface card plus the inset hairline between rows. */
export const GROUP_CLASS =
  "ci-surface rounded-surface overflow-hidden [&>*+*]:relative [&>*+*]:before:content-[''] [&>*+*]:before:absolute [&>*+*]:before:top-0 [&>*+*]:before:inset-x-3 [&>*+*]:before:h-px [&>*+*]:before:bg-separator";

export function Group({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`${GROUP_CLASS} ${className}`}>{children}</div>;
}

/** A section label above a group: micro, tertiary, aligned with the group's text inset (12). */
export function SectionLabel({ children, trailing }: { children: ReactNode; trailing?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2 px-3 mb-2">
      <span className="text-micro text-fg-tertiary">{children}</span>
      {trailing}
    </div>
  );
}
