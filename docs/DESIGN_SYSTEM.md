# Yuval — island design system

One visual language for every island presentation: the compact pill, the ring/silent pill, the
meeting alert, notification toasts, and the open island (panel) with its tabs and dock. Apple's
Dynamic Island / Live Activities principles are the benchmark (concentric shapes, even margins,
glanceable heavy-enough type, use only the space needed); the implementation is Windows-native
(Segoe UI Variable / Segoe UI, DIPs, mouse + keyboard). No Apple fonts are bundled or referenced.

**Source of truth:** `src/design/tokens.ts`. Components read tokens (TS constants) or the Tailwind
classes generated from them (`tailwind.config.js` imports tokens.ts). Colours are CSS variables
(`--ci-*`) so high contrast overrides them in one place. A number that is not a token is either
derived from tokens in code or an optical correction with a comment saying why.

## 1. Rules

1. **Spacing** — 4-pt scale only: 4, 8, 12, 16, 20, 24 (Tailwind 1–6). No `0.5`, `1.5`, `2.5`,
   `[3px]`, `[10.5px]`… Optical corrections are allowed and must be commented.
2. **Concentric radii** — a rounded shape nested in another: `inner = outer − inset`.
3. **Capsules** — pills (compact island, badges, ring/silent) use `radius = height / 2`.
4. **One compact height** — 36 for every collapsed state; only the width follows the content.
5. **Direction** — three levels (src/design/direction.ts):
   - *Shell*: the island's layout follows the UI language (`uiDirection()`, the pinned locale:
     Hebrew → `dir="rtl"`, leading edge = right). The Notification Center keeps EVERY card in the
     shell direction (icon always on the same side, no zig-zag); a standalone toast may follow its
     own content (an English Snipping Tool toast is LTR, a Hebrew Teams toast RTL).
   - *Fields*: an app/source name takes its own first-strong direction, isolated (an English app
     name stays LTR in a Hebrew card); a title/body paragraph is rtl when it contains Hebrew letters
     (so "Project Alpha_v2.pptx מוכן לבדיקה" reads as the Hebrew sentence it is; first-strong gets
     it backwards), ltr when Latin only, inherits when neutral. Alignment follows the shell.
   - *Tokens*: inside any text, URLs, e-mail addresses, Windows/UNC paths, file names, IPv4,
     times/time ranges and dates are isolated LTR runs (`BidiText` → `<bdi dir="ltr">`), so
     "הישיבה ב-11:00–12:00" never reads 12:00–11:00 and a path never breaks apart.
   Real Unicode isolation only: never margins, `dir="auto"` or `unicode-bidi: plaintext`. The tab
   dock is the one exception to the shell: it always runs left to right (`DOCK_DIRECTION`). Use logical classes (`ms-`, `me-`,
   `ps-`, `pe-`, `start-`, `end-`, `text-start`) — never physical `ml-/mr-/pl-/pr-/left-/right-/text-left/text-right`.
6. **Type** — only the seven roles below (Tailwind `text-<role>`), no other font sizes.
7. **Colour is semantic** — blue = information / calendar / selection, green = positive /
   confirm / join, red = destructive / reject / critical failure only, grey (`muted`) = muted /
   silenced / quiet (muting is a choice, not a danger), orange = warning, identity colours only for app
   tiles. Never decoration.
8. **Pixel model** — every size and position stays a floating-point DIP in the frontend (no
   "even width" rule: even DIPs are not whole pixels at 125/150/175 %). Snapping happens once, at
   the native boundary (monitors.rs), with the target monitor's real DPI, and it snaps EDGES from
   the float centre (`left = round(cx − w·s/2)`, `right = round(cx + w·s/2)`), never the width and
   the x independently, so the centre cannot drift.
9. **Placement** — the island hangs 8 DIP below the top of the screen (`ISLAND_TOP_INSET`, in both
   monitors.rs and animations.ts, kept equal by a test). The stage window itself starts at the
   monitor's top and its region includes an invisible bridge from the screen edge down to the
   island, exactly the island's width: throwing the pointer against the top edge hits the island
   while the visible gap stays. No bridge under a top-docked taskbar (it would take its clicks).
10. **Responsive panel** — 400 × 440 is the preferred size. Rust reports the target monitor's
   usable area (`get_island_limits`, and the `display-changed` payload): width = monitor − 2·12,
   height = work-area bottom − island top − 12, in DIPs at that monitor's DPI. The panel takes
   min(preferred, limit) (floor: header + one card + dock); header and dock keep their size and the
   scrolling body absorbs the rest. Alerts and toasts are clamped the same way; the stage never
   exceeds the work area.
11. **Controls** — every action in a card is an `ActionButton` in an `ActionRow` (fills the row,
   equal columns). Icon-only controls are `RoundButton`s. All have hover, pressed, focus,
   disabled states and a ≥ 44px hit area.

## 2. Tokens

### Type roles (`type`)
| Role | Size/line | Weight | Used for |
|---|---|---|---|
| `largeTitle` | 20/24 | 700 | panel header ("התראות") |
| `title` | 17/22 | 600 | meeting subject in alert / next-meeting card |
| `headline` | 15/20 | 600 | notification title, list-card title, compact clock (primary), ring/silent label |
| `label` | 14/18 | 600 | buttons, compact date / weekday / meeting status |
| `body` | 13/18 | 400 | notification body, list rows, descriptions |
| `meta` | 12/16 | 500 | source app, timestamps, time ranges, rooms, context label |
| `micro` | 11/14 | 600 | tab labels, badge digits, section labels (smallest text anywhere) |

Hebrew is explicit, not left to fallback: Segoe UI Variable has no Hebrew, so `fonts.css` maps Hebrew (unicode-range) to the real Segoe UI faces through the "CI Hebrew" family, first in every stack: weights 300–449 → Segoe UI (normal **400**), 450–649 → Segoe UI Semibold (emphasis **600**), 650–900 → Segoe UI Bold. A 500 role (`meta`) therefore renders Hebrew at a real 600 next to Latin's variable 500 (Segoe UI has no 500; matching would otherwise fall to 400 and make Hebrew read lighter). Canvas measuring waits for these faces (`preloadUiFonts`) and re-measures when they arrive.

### Colours (`palette`, CSS `--ci-*`, Tailwind `text-fg`, `bg-surface`, …)
island `#000` · surface white 7% (≈ #121212) · surfaceHover 10% · selection 12% · fill 12% (neutral
buttons) · fg `#F5F5F7` · fgSecondary 68% · fgTertiary 52% (≥ 4.5:1 on black) · accent `#0A84FF` ·
positive `#30D158` · positiveFill `#248A3D` (white text 4.6:1) · destructive `#FF453A` /
destructiveText `#FF6961` on destructiveSoft 18% · warning `#FF9F0A`. High contrast
(`prefers-contrast: more`) raises the secondary/tertiary text and shows surface outlines.

### Radii (`radius`)
panel 30 · smallExpanded (alert, toast) 28 · surface (cards, dock, groups) 18 = 30 − 12 ·
selection (selected tab) 14 = 18 − 4 · control (buttons) 12 = 28 − 16 · icon (app tile) 12.

### Presentations
| | Size | Radius | Inset |
|---|---|---|---|
| Compact | 36 high, width = content (96–280; clock 72–280) | 18 | text 12; badge 8 (concentric with the end cap) |
| Ring / silent | 176 × 36 | 18 | 12 |
| Meeting alert | 368 wide, content height | min(28, h/2) | 16 |
| Toast | 200–368 wide (content-aware), content height | min(28, h/2) | 16 |
| Panel (open island) | 400 × 440 | 30 | 12 for every surface (header, cards, dock) |

### Controls
ActionButton 40 visible / 44 hit, radius 12, padding-x 16, gap 8, label role, glyph 16.
Variants: `primary` (filled positiveFill, white), `neutral` (fill, fg), `destructive`
(destructiveSoft tint, destructiveText). Pressed = darker + scale 0.98 at once (no bounce).
RoundButton 28 visible / 44 hit. CountBadge 20 high (28 for "9+"), dot 8 for one unseen.

## 3. Compositions

### Compact (CompactIsland)
Height 36, radius 18, text inset 12, separate elements 8 apart, tight pairs 4.
- **Date & time** (display "full"): date, clock, weekday physically left → right (Hebrew reads
  weekday → clock → date). Clock is primary (`headline`, fg); date and weekday are secondary
  (`label`, fgSecondary). "clock": the clock alone. "date": date + weekday (`label`, date fg).
- **Meeting** (soon / now): status dot 8 (event colour) at the leading edge (right in RTL), 8,
  status text (`label`), then the silenced bell (`muted` grey, never red), then the badge at the
  trailing end.
- **Progress** (meeting now): a 16px ring (2.5 stroke, event colour on `track`, clockwise from
  12 o'clock) in the status dot's slot. It has its own space, so nothing moves: the text stays
  vertically centred and the badge stays concentric with the end cap (the old bottom track lifted
  the row 3px and broke that). The island is 8 wider while the ring shows.
- **Unseen badge**: always the trailing slot, 8 from the end (concentric), 8 after the content.
- No app icons in compact: the smallest useful identifier is the status dot (meeting) or the
  count (notifications).

### Meeting alert (MeetingAlert)
368 wide, padding 16, layout RTL. Context label (`meta` 600, accent) · 4 · subject (`title`,
≤ 2 lines, ellipsis) · 4 · time range (`body`, fgSecondary, tabular) · 2 · room (`body`,
fgTertiary, 1 line) · 16 · ActionRow [Join (primary) | Snooze (neutral)], 40 high, 8 gap.

### Notification grammar (toast and Notification Center card) — `ui/notification.tsx`
```
[icon 40]  SOURCE (meta, tertiary) · adornment ........ meta slot (time · unread · dismiss)
   12      Title (headline, 1 line in toasts / 2 in cards)
           Body (body, secondary, 2 lines in toasts / 3 in cards)
[ action ][ action ][ action ]   ← ActionRow across the full content width, 12 below the text
```
- Icon: `AppIcon` 40 × 40 radius 12 for every app (Teams, Outlook, Snipping Tool, calendar
  invitation = `CalendarAppIcon`). Its top aligns with the top of the text stack.
- Source → title → body: 2 apart. The source is never as strong as the title, the time never
  as strong as the body.
- The meta slot sits at the end of the source row in both places: toasts reserve it for the
  hover/focus dismiss button; center cards show the received time there (the dismiss button
  replaces it on hover/focus) and the unread dot after it.
- Direction: `contentDirection(title, body)`; app name, file names, IPs and times stay LTR runs
  inside a Hebrew card thanks to `bidi`.
- Accessible name: `notificationAccessibleLabel()` — "Teams notification. Title. Body. Received …".

### Toast (NotificationToast + toastLayout.ts)
Padding 16, radius min(28, h/2), width = content (200–368), title 1 line, body ≤ 2 lines. An
invitation always takes 368 and adds ActionRow [Accept (primary) | Maybe (neutral) | Decline
(destructive)]. Click opens the app, the X (RoundButton) dismisses, swipe is an extra.

### Panel (ExpandedIsland, TabDock)
400 × 440, radius 30, `dir` = UI direction. Every surface shares the 12 inset.
- Header row (28 high, 16 from the top): tab title (`largeTitle`) at the leading edge, the tab's
  action (e.g. "נקה הכל", `label` accent, 44 hit) at the trailing edge, same row, centred.
- 8 → scrolling content (`island-scroll`: 12px gutter on both sides, a 4px overlay-like thumb in
  the gutter, never over the cards; in RTL it is on the left as in Windows RTL apps) → 8 → dock.
- Cards (`ci-surface`, radius 18, padding 12, 8 apart). Notification card = notification grammar.
- Dock: 56 high, radius 18, surface; each tab is the full dock height (≥ 44 hit), icon 18, 4,
  label `micro`. Selection: inset 4, radius 14 (concentric). Unselected fgTertiary, hover
  fgSecondary, selected fg. The dock always runs left to right (calendar leftmost). Keyboard (WAI-ARIA tabs, on the tablist only — arrows
  inside a text box or a control never switch tabs): ArrowRight/ArrowLeft = next/previous in visual
  order (= DOM order = screen-reader order, wrapping), Home/End, automatic activation, focus moves
  with the selection. Escape closes the current presentation (never deletes), and is ignored when an
  inner control handled it; Delete removes a notification (toast or card), its X likewise in the
  Center; a toast's X and Escape only close the toast.

### Empty, error and crash states (`ui/states.tsx`)
One grammar: glyph 24 (tertiary), title (`headline`), one-sentence hint (`body`, secondary), an
optional neutral `ActionButton` (retry / open the app) only when the person can act. The block is
`flex-1` and centred in the tab (nudged up by 24 of bottom padding), so it has no size of its own
and cannot change the island's geometry. `EmptyState` is a status, `ErrorState` an alert (same
shape, no red; optional code line). Used by: Calendar (no events, waiting / connecting = empty;
new Outlook, permissions, unresponsive, failed = error with code, retry for the last two; a day
that is loading = empty, a day that failed = error + retry), Notes (empty, load failed + retry),
Notifications (empty), `TabBoundary` (a crashed tab: Unavailable + code + Try again) and
`CrashBoundary` (the collapsed island is only 36 high, so `ErrorPill` is the same grammar on one line).

### Notes
The tab root adds no horizontal padding (the scroll box provides the 12 inset). Composer = a
`ci-surface` card with the text field, focus shown by a ring on the card (`ci-field`, drawn inside
its edge); Save is a primary `ActionButton`; Ctrl+Enter saves; Escape leaves the field and calls
`preventDefault` so the shell's Escape-collapse ignores it. Note cards: `ci-surface rounded-surface p-card-pad`,
8 apart; pin / copy / delete are `RoundButton`s (the delete one `grow`s into a "Delete?" capsule in
the same element, so focus stays); the section row has the label and a `HeaderActionButton`.

### High contrast
`prefers-contrast: more` strengthens the colour tokens and shows the `--ci-outline` hairlines.
Windows high-contrast themes (`forced-colors: active`, block at the end of index.css) drop
backgrounds and shadows, so every shape keeps its identity on its own radius: the island and all
`ci-surface` shapes get a 1px CanvasText outline (outlines follow border-radius, no layout), buttons a
1px ButtonText border, selection is Highlight / HighlightText (dock indicator, selected day, segment,
switch), coloured marks keep their colour (`ci-mark`), badges and the progress ring use Highlight
(`ci-badge`, `ci-progress`), app tiles get a border (`ci-tile`), focus rings are 2px Highlight on the
control's own radius. Tailwind's `outline-none` (a transparent outline) is neutralised there. Text-only
buttons: `ci-link` (underlined) / `ci-bare` (no box).

### Scrollbar (`island-scroll`)
Verified in Edge at 1 / 1.25 / 1.5 / 2 ×: `scrollbar-gutter: stable both-edges` reserves 12 per side so
cards are 376 wide with or without scrolling; the thumb is 4 visible in a 12-wide grabbable strip on the
left in RTL; hover and active brighten it; in forced colours it is ButtonText.

### Search bar glow (`src/search/`, tokens `glow`, `glowStops`)
The smart search bar (a light Windows 10 style bar in its own window) glows while AI Mode is on. It is
an original implementation with three composited layers and no filter: (1) a static aura of coloured
box-shadows whose blur never exceeds the 6 DIP window margin, (2) a conic-gradient square rotated with
`transform` only and clipped to the bar, which shows as a 1.5 px ring, (3) the bar's own opaque
padding box. Colours: Cyan `#40C8E0`, Violet `#BF5AF2`, Indigo `#5E5CE6`, Magenta `#E040C8`, Soft
Pink `#FFA3C7`; Error is one red, `#FF453A`.

| State | Look |
|---|---|
| Idle, Disabled | nothing is rendered (no layers, no animation) |
| Activated | ring turns every 12 s, aura 0.55 |
| Typing | slower, 18 s |
| Submitting | 6 s, one 180 ms pulse |
| Processing | 3.5 s, aura breathes; only while the request runs, never a minimum duration |
| Completed | frozen for 600 ms, then Activated |
| Error | red ring, held until the next input |

Only `transform` and `opacity` animate, only in Activated / Typing / Submitting / Processing, and not
while the document is hidden. Reduced motion: the gradient is frozen and states show by opacity and
colour only (a scoped override keeps 120 ms fades alive under the global 0.01 ms rule). Forced colours or
`highContrast`: a plain 2 px system-colour ring, no gradient.

## 4. Verification

`npm run dev` → `/gallery.html` renders every presentation from the real components with the
tour's data plus long / mixed / English / empty / 30-item cases (`?only=id1,id2` for one).
Screenshots at 1×, 1.25×, 1.5×, 1.75×, 2× via the Chrome DevTools protocol.

**Visual regression** (`npm run test:visual`, tests/visual/README.md): 31 states from the gallery,
rendered by the installed Microsoft Edge (the WebView2 engine) at 1× and 2×, each compared with a
committed baseline (threshold 0.2, maxDiffPixelRatio 0.003: a 1–2px shift fails) AND checked for
geometry read from the DOM against `tokens.ts` (island size and radius, action-row columns, card
x/width/radius, dock and header). A radius change is a few corner pixels, so the geometry checks
are what catch it. Update baselines only after looking at the diffs (`npm run test:visual:update`).

**Motion** (the real app, not the gallery): the island's fades are tuned on frames measured in the
installed WebView2 (every rendered frame sampled through the DevTools protocol). Closing, the
compact content starts at progress 0.72 (shape ≈ 220×140) while the panel body holds until 0.9; no
frame shows less than ~29 % content; the shape never leaves [compact, panel], never snaps and never
plateaus inside a motion.

## Spotlight search bar ("Aurora capsule")

Variant `spotlight` of the smart search bar (`SearchBarState.variant`, Alt + `` ` ``). Source: `src/search/SpotlightCapsule.tsx`, `src/search/spotlight.css`, tokens `spotlight` in `src/design/tokens.ts`.

- Geometry: capsule 680x60 DIP, radius 30; the window adds a 28 DIP transparent margin on every side (736x116) for the under-glow and shadow. Nothing paints outside the window.
- Always dark ink (`#161833` to `#0E0F1A`), so it floats over any wallpaper; 1.25 px iridescent rim from the glow palette (cyan, violet, indigo, magenta, soft pink) that drifts slowly.
- Leading AI spark (breathes while submitting/processing, pulses once on completed); 21 px text, `dir="auto"`; a frosted key chip shows `Alt + `` ` ``` when empty and `Enter` once there is text. No suggestion chips (the window is fixed-size).
- States reuse the eight glow states: processing speeds the rim up (playback rate, never a jump); error turns rim, spark and glow red; completed pulses.
- Budget: at most three composited layers (rim transform, under-glow opacity, spark transform). Nothing animates when idle, hidden, reduced-motion or plain.
- `prefers-reduced-motion`: static rim, opacity-only. Forced colours / high contrast: Canvas/CanvasText, 2 px ring, no glow.
- Dev preview: `search.html?preview=1&variant=spotlight&state=processing&text=...`.
