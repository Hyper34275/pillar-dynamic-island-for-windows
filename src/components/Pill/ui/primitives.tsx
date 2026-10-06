import { motion } from "motion/react";
import type { ReactNode } from "react";
import { useId } from "react";

// =============================================================================
// Island design primitives. Everything sits on a pure-black surface, so depth
// comes from translucent white fills (6–14%) rather than borders and gradients.
// =============================================================================

/** iOS system palette — used for app avatars and status tints. */
export const SYSTEM_COLORS = {
  red: "#FF453A",
  orange: "#FF9F0A",
  yellow: "#FFD60A",
  green: "#30D158",
  mint: "#63E6E2",
  teal: "#40C8E0",
  blue: "#0A84FF",
  indigo: "#5E5CE6",
  purple: "#BF5AF2",
  pink: "#FF375F",
} as const;

const AVATAR_PALETTE = [
  SYSTEM_COLORS.blue,
  SYSTEM_COLORS.green,
  SYSTEM_COLORS.purple,
  SYSTEM_COLORS.orange,
  SYSTEM_COLORS.pink,
  SYSTEM_COLORS.teal,
  SYSTEM_COLORS.indigo,
  SYSTEM_COLORS.red,
];

const avatarCache = new Map<string, string>();

/** Stable per-app color derived from the app name. */
export function appColor(name: string): string {
  const cached = avatarCache.get(name);
  if (cached) return cached;
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
  const color = AVATAR_PALETTE[Math.abs(hash) % AVATAR_PALETTE.length];
  if (avatarCache.size > 200) avatarCache.clear();
  avatarCache.set(name, color);
  return color;
}

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
// App avatar — rounded square with initial, tinted per app
// -----------------------------------------------------------------------------

export function AppAvatar({ name, size = 28, radius }: { name: string; size?: number; radius?: number }) {
  const label = cleanAppName(name) || name;
  const color = appColor(label);
  return (
    <div
      className="flex items-center justify-center flex-shrink-0 font-semibold text-white uppercase"
      style={{
        width: size,
        height: size,
        borderRadius: radius ?? Math.round(size * 0.3),
        background: `linear-gradient(160deg, ${color}, color-mix(in srgb, ${color} 70%, black))`,
        fontSize: Math.round(size * 0.42),
        boxShadow: "inset 0 0.5px 0 rgba(255,255,255,0.25)",
      }}
      aria-hidden="true"
    >
      {label.charAt(0)}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Segmented control with a sliding thumb
// -----------------------------------------------------------------------------

interface SegmentedProps<T extends string> {
  options: ReadonlyArray<{ id: T; label: ReactNode; ariaLabel?: string }>;
  value: T;
  onChange: (id: T) => void;
  size?: "sm" | "md";
  className?: string;
  ariaLabel?: string;
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  size = "sm",
  className = "",
  ariaLabel,
}: SegmentedProps<T>) {
  const groupId = useId();
  const pad = size === "sm" ? "px-2.5 h-[22px] text-[11px]" : "px-3 h-[28px] text-[12px]";
  return (
    <div
      className={`relative inline-flex items-center p-[2px] rounded-full bg-white/[0.08] ${className}`}
      role="group"
      aria-label={ariaLabel}
    >
      {options.map((opt) => {
        const active = opt.id === value;
        return (
          <button
            key={opt.id}
            type="button"
            className={`relative flex-1 ${pad} rounded-full font-semibold transition-colors whitespace-nowrap ${
              active ? "text-white" : "text-white/50 hover:text-white/80"
            }`}
            aria-pressed={active}
            aria-label={opt.ariaLabel}
            onClick={() => onChange(opt.id)}
          >
            {active && (
              <motion.span
                layoutId={`seg-${groupId}`}
                className="absolute inset-0 rounded-full bg-white/[0.16]"
                style={{ boxShadow: "0 1px 3px rgba(0,0,0,0.35), inset 0 0.5px 0 rgba(255,255,255,0.12)" }}
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
// iOS switch
// -----------------------------------------------------------------------------

export function Switch({
  checked,
  onChange,
  label,
  tint = SYSTEM_COLORS.green,
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
      className="relative w-[38px] h-[23px] rounded-full flex-shrink-0 transition-colors duration-200"
      style={{ background: checked ? tint : "rgba(120,120,128,0.36)" }}
    >
      <motion.span
        className="absolute top-[2px] left-[2px] w-[19px] h-[19px] rounded-full bg-white"
        style={{ boxShadow: "0 2px 5px rgba(0,0,0,0.3)" }}
        animate={{ x: checked ? 15 : 0 }}
        transition={{ type: "spring", stiffness: 600, damping: 36 }}
      />
    </button>
  );
}

// -----------------------------------------------------------------------------
// Inset grouped list
// -----------------------------------------------------------------------------

export function Group({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`rounded-[16px] bg-white/[0.07] overflow-hidden divide-y divide-white/[0.06] ${className}`}>
      {children}
    </div>
  );
}

export function SectionLabel({ children, trailing }: { children: ReactNode; trailing?: ReactNode }) {
  return (
    <div className="flex items-center justify-between px-1 mb-1.5 mt-0.5">
      <span className="text-[11px] font-semibold text-white/40 uppercase tracking-[0.06em]">{children}</span>
      {trailing}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Empty state
// -----------------------------------------------------------------------------

export function EmptyState({
  icon,
  title,
  subtitle,
  tint = "rgba(255,255,255,0.1)",
  children,
}: {
  icon: ReactNode;
  title: string;
  subtitle?: string;
  tint?: string;
  children?: ReactNode;
}) {
  // No entrance animation of its own: it appears with its tab panel, whose fade is part of the
  // island's single transition. A second, independent fade here would lag behind it.
  return (
    <div className="flex flex-col items-center justify-center text-center py-6 gap-1">
      <div
        className="w-12 h-12 rounded-full flex items-center justify-center text-white/80 mb-1.5"
        style={{ background: tint }}
      >
        {icon}
      </div>
      {/* Each text picks its own direction: the layout is LTR, but Hebrew with an English product name ("ממתין ל-Outlook") must not be shaped as an LTR paragraph. */}
      <span dir="auto" className="text-white text-[14px] font-semibold" style={{ unicodeBidi: "plaintext" }}>
        {title}
      </span>
      {subtitle && (
        <span dir="auto" className="text-white/45 text-[12px]" style={{ unicodeBidi: "plaintext" }}>
          {subtitle}
        </span>
      )}
      {children}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Pill button
// -----------------------------------------------------------------------------

export function PillButton({
  children,
  onClick,
  variant = "gray",
  tint,
  className = "",
  disabled,
  ariaLabel,
  type = "button",
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "gray" | "tinted" | "filled" | "plain";
  tint?: string;
  className?: string;
  disabled?: boolean;
  ariaLabel?: string;
  type?: "button" | "submit";
}) {
  const color = tint ?? SYSTEM_COLORS.blue;
  const style =
    variant === "filled"
      ? { background: color, color: "#fff" }
      : variant === "tinted"
        ? { background: `color-mix(in srgb, ${color} 22%, transparent)`, color }
        : undefined;
  const base =
    variant === "gray"
      ? "bg-white/[0.1] text-white/85 hover:bg-white/[0.15]"
      : variant === "plain"
        ? "text-white/55 hover:text-white"
        : "hover:brightness-110";
  return (
    <motion.button
      type={type}
      className={`inline-flex items-center justify-center gap-1.5 rounded-full font-semibold transition-[background,filter,color] disabled:opacity-40 disabled:cursor-not-allowed ${base} ${className}`}
      style={style}
      onClick={onClick}
      disabled={disabled}
      aria-label={ariaLabel}
      whileTap={disabled ? undefined : { scale: 0.94 }}
      transition={{ type: "spring", stiffness: 600, damping: 30 }}
    >
      {children}
    </motion.button>
  );
}
