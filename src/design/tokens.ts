// =============================================================================
// Yuval design tokens: the ONE place every dimension, radius, type role and colour of the
// island is decided. Components read these (TS constants for geometry and inline styles) or the
// Tailwind classes generated from them (tailwind.config.js imports this file), never their own
// numbers. Colours are CSS custom properties (installed by the Tailwind plugin in
// tailwind.config.js) so high-contrast mode can override them in one place.
//
// All lengths are DIPs (CSS px). Tauri converts them with the window's scale factor.
//
// GEOMETRY RULES (see docs/DESIGN_SYSTEM.md):
//  1. Spacing comes from the 4-pt scale (4, 8, 12, 16, 20, 24). Anything else is an optical
//     correction and is commented where it is used.
//  2. A rounded shape nested in another is concentric: innerRadius = outerRadius - inset.
//  3. Pills (the compact island, badges, the ring/silent pill) are capsules: radius = height / 2.
//  4. Height of the compact island never changes; only its width follows its content.
// =============================================================================

/** The 4-pt spacing scale. Tailwind's default spacing (1 = 4px) is the same scale. */
export const space = { 1: 4, 2: 8, 3: 12, 4: 16, 5: 20, 6: 24 } as const;

// -----------------------------------------------------------------------------
// Type roles. Seven roles, nothing else. Weights: Hebrew falls back to Segoe UI, which has
// 400/600/700 only, so a 500 role renders as 400 in Hebrew (Latin gets the variable 500).
// -----------------------------------------------------------------------------
export interface TypeRole {
  size: number;
  lineHeight: number;
  weight: number;
  /** em */
  tracking: number;
}

export const type = {
  /** Panel header ("התראות"). */
  largeTitle: { size: 20, lineHeight: 24, weight: 700, tracking: -0.01 },
  /** The hero line of an expanded card: the meeting subject. */
  title: { size: 17, lineHeight: 22, weight: 600, tracking: -0.005 },
  /** Notification title, list-card title, the compact island's primary label (the clock), ring/silent label. */
  headline: { size: 15, lineHeight: 20, weight: 600, tracking: 0 },
  /** Buttons and short compact labels (date, weekday, meeting status). */
  label: { size: 14, lineHeight: 18, weight: 600, tracking: 0 },
  /** Notification body, list rows, descriptions. */
  body: { size: 13, lineHeight: 18, weight: 400, tracking: 0 },
  /** Source app, timestamps, time ranges, rooms, context labels. */
  meta: { size: 12, lineHeight: 16, weight: 500, tracking: 0 },
  /** Tab labels, badge digits, section labels. The smallest text anywhere. */
  micro: { size: 11, lineHeight: 14, weight: 600, tracking: 0 },
} as const satisfies Record<string, TypeRole>;

export type TypeRoleName = keyof typeof type;

/**
 * "CI Hebrew" (src/design/fonts.css) is first: it only covers the Hebrew blocks and maps the
 * weights to the real Segoe UI faces (400 / 600 / 700), so Hebrew text never depends on the
 * browser rounding a variable-font weight onto a face that does not exist. Everything else falls
 * through to Segoe UI Variable. Hebrew rule: normal = 400, emphasis = 600 (a 500 role renders as
 * 400 in Hebrew; the type roles that carry meaning use 600).
 */
export const fontFamily = {
  text: '"CI Hebrew", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif',
  display: '"CI Hebrew", "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif',
} as const;

// -----------------------------------------------------------------------------
// Colours (semantic). The values live in CSS variables; TS gets `var(--…)` references.
// -----------------------------------------------------------------------------
export const palette = {
  island: "#000000",
  /** Raised surface on the island: list cards, the dock, grouped settings (≈ #121212). */
  surface: "rgba(255,255,255,0.07)",
  surfaceHover: "rgba(255,255,255,0.10)",
  surfacePressed: "rgba(255,255,255,0.05)",
  /** The selected tab's shape, on top of the dock's surface. */
  selection: "rgba(255,255,255,0.12)",
  /** Neutral control fill (secondary buttons, the dismiss button). */
  fill: "rgba(255,255,255,0.12)",
  fillHover: "rgba(255,255,255,0.17)",
  fillPressed: "rgba(255,255,255,0.09)",
  separator: "rgba(255,255,255,0.08)",
  /** Text: primary ~19:1, secondary ~9:1, tertiary ~4.8:1 on the island (all pass AA). */
  fg: "#F5F5F7",
  fgSecondary: "rgba(235,235,245,0.68)",
  fgTertiary: "rgba(235,235,245,0.52)",
  fgQuaternary: "rgba(235,235,245,0.32)",
  /**
   * A muted / silenced / quiet state (the slashed bell, the "Silent" pill): a calm grey, the same as
   * fgTertiary. Muted is a user's choice, not a danger: it must never use destructive (red), which
   * is reserved for deleting, declining and critical failures.
   */
  muted: "rgba(235,235,245,0.52)",
  /** The unfilled part of a progress indicator: a hairline of light on the black island. */
  track: "rgba(255,255,255,0.14)",
  /** Information / calendar identity / the unseen indicator. 5.7:1 on black. */
  accent: "#0A84FF",
  accentSoft: "rgba(10,132,255,0.24)",
  /** Accent text on accentSoft: the blue mixed with white, ~6.7:1. */
  accentOnSoft: "#54A9FF",
  positive: "#30D158",
  /** Filled primary/positive button: deep enough for white text at 4.6:1. */
  positiveFill: "#248A3D",
  positiveFillHover: "#2A9A45",
  positiveFillPressed: "#1F7A35",
  destructive: "#FF453A",
  /** Destructive label on its tint (~6:1). */
  destructiveText: "#FF6961",
  destructiveSoft: "rgba(255,69,58,0.18)",
  destructiveSoftHover: "rgba(255,69,58,0.26)",
  warning: "#FF9F0A",
  /** The resting edge of a text field: enough to read as "type here" on the black island. */
  fieldEdge: "rgba(255,255,255,0.24)",
  fieldEdgeHover: "rgba(255,255,255,0.36)",
  /** Keyboard focus ring. */
  focus: "rgba(255,255,255,0.75)",
  /** A hairline around surfaces: transparent normally, visible in high contrast. */
  outline: "transparent",
  /** The island's own edge against bright wallpapers. */
  islandEdge: "rgba(255,255,255,0.10)",
  /** The island's adaptive keyline (lib/island/keyline.ts): ONE physical pixel, drawn only while the
   *  backdrop behind the island is very dark; never a permanent border. */
  islandKeyline: "rgba(255,255,255,0.14)",
} as const;

export type ColorName = keyof typeof palette;

/** prefers-contrast: more — stronger text and visible outlines. */
export const paletteHighContrast: Partial<Record<ColorName, string>> = {
  fgSecondary: "rgba(235,235,245,0.86)",
  fgTertiary: "rgba(235,235,245,0.78)",
  fgQuaternary: "rgba(235,235,245,0.6)",
  muted: "rgba(235,235,245,0.78)",
  surface: "rgba(255,255,255,0.12)",
  fill: "rgba(255,255,255,0.2)",
  outline: "rgba(255,255,255,0.6)",
  islandEdge: "rgba(255,255,255,0.6)",
  fieldEdge: "rgba(255,255,255,0.7)",
  fieldEdgeHover: "rgba(255,255,255,0.85)",
  /** prefers-contrast: more asks for a visible edge: the keyline is then always drawn. */
  islandKeyline: "rgba(255,255,255,0.6)",
  focus: "#FFFFFF",
};

const kebab = (name: string) => name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
export const cssVarName = (name: ColorName) => `--ci-${kebab(name)}`;

/** `var(--ci-…)` for inline styles. */
export const color = Object.fromEntries(Object.keys(palette).map((name) => [name, `var(${cssVarName(name as ColorName)})`])) as Record<ColorName, string>;

export function cssVariables(values: Partial<Record<ColorName, string>>): Record<string, string> {
  return Object.fromEntries(Object.entries(values).map(([name, value]) => [cssVarName(name as ColorName), value as string]));
}

/** Avatar / app-identity tints (iOS system palette). Identity only, never status. */
export const identityColors = ["#0A84FF", "#30D158", "#BF5AF2", "#FF9F0A", "#FF375F", "#40C8E0", "#5E5CE6", "#FF453A"] as const;

// -----------------------------------------------------------------------------
// Radii. One family, derived (see rule 2).
// -----------------------------------------------------------------------------
const PANEL_RADIUS = 30;
const PANEL_INSET = 12;
const SMALL_EXPANDED_RADIUS = 28;
const SMALL_EXPANDED_PADDING = 16;
const DOCK_SELECTION_INSET = 4;

export const radius = {
  /** The open island (all tabs). */
  panel: PANEL_RADIUS,
  /** Meeting alert and notification toast (small expanded). */
  smallExpanded: SMALL_EXPANDED_RADIUS,
  /** Level-1 surfaces inside the panel (cards, dock, groups): panel − panel inset. */
  surface: PANEL_RADIUS - PANEL_INSET,
  /** The selected tab inside the dock: surface − selection inset. */
  selection: PANEL_RADIUS - PANEL_INSET - DOCK_SELECTION_INSET,
  /** Buttons and app icons: small expanded − its padding (concentric in toasts and alerts). */
  control: SMALL_EXPANDED_RADIUS - SMALL_EXPANDED_PADDING,
  icon: SMALL_EXPANDED_RADIUS - SMALL_EXPANDED_PADDING,
} as const;

// -----------------------------------------------------------------------------
// Controls
// -----------------------------------------------------------------------------
export const control = {
  /** Visible action-button height. */
  height: 40,
  /** Minimum interactive height (the button's hit area extends past its visible surface). */
  hit: 44,
  gap: 8,
  paddingX: 16,
  /** Glyph inside a button. */
  iconSize: 16,
  /** Pressed: an immediate, barely-there squeeze (no bounce). */
  pressedScale: 0.98,
  /** Small round icon-only buttons (dismiss X, join, silence). */
  round: 28,
} as const;

export const icon = {
  /** App / event identity tile: one size everywhere (toasts, list cards, invitations). */
  app: 40,
  appRadius: radius.icon,
  /** Glyph sizes. */
  small: 14,
  medium: 16,
  large: 18,
  /** The glyph of an empty / error state block (ui/states.tsx). */
  state: 24,
  /** Tab bar glyphs. */
  tab: 18,
} as const;

// -----------------------------------------------------------------------------
// Island presentations
// -----------------------------------------------------------------------------
export const compact = {
  height: 36,
  radius: 18,
  /** Text content keeps this far from the curved ends. */
  paddingX: 12,
  /** Between logically separate elements. */
  gap: 8,
  /** Between tightly related elements (digits and AM/PM). */
  gapTight: 4,
  minWidth: 96,
  /** The display "clock" holds one short label. */
  clockMinWidth: 72,
  maxWidth: 280,
  /** Status dot of a meeting (its calendar colour). */
  statusDot: 8,
} as const;

/** The unseen indicator; shared by the compact island and anything else that counts. */
export const badge = {
  /** Count capsule height and minimum width (a circle up to 9). */
  size: 20,
  /** Width for "9+". */
  wide: 28,
  /** One unseen: a calm dot, centred in the same 20px slot (so 1 → 2 never changes the width). */
  dot: 8,
  /**
   * Concentric with the compact island's end cap: (height − size) / 2, so the badge's gap to the
   * curved end equals its gap to the top and bottom.
   */
  endInset: (compact.height - 20) / 2,
} as const;

/**
 * A meeting in progress (compact island): the status dot becomes a progress ring in the same
 * leading slot. Nothing is added above or below the text, so the row stays centred in the 36px
 * pill and the count badge stays concentric with the end cap.
 */
export const progress = {
  /** Ring diameter (twice the status dot: it takes the dot's place and gives progress its own space). */
  ring: 16,
  /**
   * Optical: 2.5 gives the ring about the visual weight of the 8px dot beside the 600-weight label
   * text (2 looks thin at 100%, 3 closes the 16px ring's counter).
   */
  ringStroke: 2.5,
} as const;

export const ringer = {
  height: compact.height,
  width: 176,
  icon: 18,
} as const;

export const smallExpanded = {
  /** Meeting alerts (fixed: they carry an action row) and the toast maximum. */
  width: 368,
  radius: SMALL_EXPANDED_RADIUS,
  padding: SMALL_EXPANDED_PADDING,
} as const;

export const toast = {
  minWidth: 200,
  maxWidth: smallExpanded.width,
  padding: SMALL_EXPANDED_PADDING,
  radius: SMALL_EXPANDED_RADIUS,
  iconGap: 12,
  titleMaxLines: 1,
  bodyMaxLines: 2,
  /** source → title, title → body. */
  stackGap: 2,
  /** body → action row. */
  actionsGap: 12,
} as const;

export const panel = {
  width: 400,
  height: 440,
  radius: PANEL_RADIUS,
  /** Every surface (header, cards, dock) shares this horizontal inset. */
  inset: PANEL_INSET,
  paddingTop: 16,
  paddingBottom: PANEL_INSET,
  /** Header row height (holds a 44px hit area of its action through negative margins). */
  headerHeight: 28,
  headerGap: 8,
  /** Space between the scrolling content and the dock. */
  dockGap: 8,
} as const;

export const dock = {
  height: 56,
  radius: radius.surface,
  selectionInset: DOCK_SELECTION_INSET,
  selectionRadius: radius.selection,
  iconSize: icon.tab,
  labelGap: 4,
} as const;

export const card = {
  radius: radius.surface,
  padding: 12,
  listGap: 8,
  iconGap: 12,
  /** Unread mark. */
  unreadDot: 8,
} as const;

/** The alert's vertical rhythm (context label → title → time → place → actions). */
export const alert = {
  labelGap: 4,
  detailGap: 2,
  actionsGap: 16,
  subjectMaxLines: 2,
} as const;

/** A thin overlay-style scrollbar that lives in the panel's side inset, never over the cards. */
export const scrollbar = {
  /** The gutter = panel inset, so the cards keep their bounds whether it shows or not. */
  gutter: PANEL_INSET,
  thumb: 4,
  /** Kept away from the header and dock ends of the track. */
  trackInset: 8,
} as const;

/**
 * The smart search bar's glow (src/search/AiSearchGlow.tsx). These hues are not semantic island
 * colours: they live on the light Windows 10 search bar only, never on the black island.
 */
export const glow = {
  cyan: "#40C8E0",
  violet: "#BF5AF2",
  indigo: "#5E5CE6",
  magenta: "#E040C8",
  softPink: "#FFA3C7",
  /** Error recolours the whole ring to this single red. */
  error: "#FF453A",
  /** Disabled: a desaturated grey. */
  disabled: "rgba(120,120,120,0.5)",
  /** Window margin around the bar that the aura may use (DIPs). Blur never exceeds it. */
  margin: 6,
  /** Visible thickness of the ring: the light plate is inset by this much. */
  ringWidth: 1.5,
  /** A finished answer keeps its glow this long before settling (visual hold, not a delay). */
  completedHoldMs: 600,
} as const;

/**
 * The spotlight variant of the smart search bar (centre of the screen, "Aurora capsule").
 * The capsule is always dark ink: it floats over any wallpaper or window.
 */
export const spotlight = {
  /** Capsule size (DIPs); the window is the capsule plus `margin` on every side. */
  width: 680,
  height: 60,
  margin: 28,
  radius: 30,
  inkTop: "#161833",
  inkBottom: "#0E0F1A",
  /** Rim thickness (DIPs). */
  rim: 1.25,
} as const;

/** The conic stops in order; cyan repeats at the end so the loop closes without a seam. */
export const glowStops = [glow.cyan, glow.violet, glow.indigo, glow.magenta, glow.softPink, glow.cyan] as const;

// -----------------------------------------------------------------------------
// Tailwind theme (tailwind.config.js imports this)
// -----------------------------------------------------------------------------
const twFont = (role: TypeRole) => [
  `${role.size}px`,
  { lineHeight: `${role.lineHeight}px`, fontWeight: String(role.weight), letterSpacing: `${role.tracking}em` },
];

export const tailwindTheme = {
  colors: Object.fromEntries(Object.keys(palette).map((name) => [kebab(name), color[name as ColorName]])),
  fontSize: Object.fromEntries(Object.entries(type).map(([name, role]) => [kebab(name), twFont(role)])),
  borderRadius: {
    panel: `${radius.panel}px`,
    expanded: `${radius.smallExpanded}px`,
    surface: `${radius.surface}px`,
    selection: `${radius.selection}px`,
    control: `${radius.control}px`,
    icon: `${radius.icon}px`,
  },
  spacing: {
    control: `${control.height}px`,
    hit: `${control.hit}px`,
    round: `${control.round}px`,
    "app-icon": `${icon.app}px`,
    dock: `${dock.height}px`,
    "panel-inset": `${panel.inset}px`,
    "card-pad": `${card.padding}px`,
    "expanded-pad": `${smallExpanded.padding}px`,
  },
  fontFamily: { text: fontFamily.text, display: fontFamily.display },
};
