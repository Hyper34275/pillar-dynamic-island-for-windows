# Visual regression tests

Screenshots and geometry of the island's core states, rendered from the real components and the
tour's fixed data (`src/gallery`, `gallery.html`).

## Run

```
npm run test:visual           # compare against the committed baselines
npm run test:visual:update    # regenerate baselines (review the diff of the PNGs before committing)
npx playwright test panel-notes   # one state
```

Playwright builds the gallery (`vite.gallery.config.ts`, output in `node_modules/.cache/gallery-dist`,
so the app's `dist/` is untouched) and serves it with `vite preview` on port 4179. A failing
screenshot leaves expected/actual/diff images under `node_modules/.cache/pw-results`.

## What is covered

Each state is its own exhibit, opened alone at its exact island size on a flat stage:
`/gallery.html?exhibit=<id>` (the island element is `[data-testid="island"]`). The full page
`/gallery.html` still shows all of them.

- Compact: `compact-date` (date/time/day + count), `compact-dot`, `compact-plain` (quiet), `compact-soon`
  (meeting countdown), `compact-now` (in progress + progress bar + muted), `compact-now-badge`,
  `compact-muted`, `ringer`
- Meeting alerts: `alert` (reminder), `alert-now` (starting now), `alert-long`
- Toasts: `invite`, `teams` (Hebrew), `teams-long`, `mixed` (Hebrew/English), `snipping`,
  `snipping-short` (title only), `outlook-en`, `long-app`, `long-hebrew`
- Panels: `panel-notifications` (several), `-empty`, `-1`, `-30` (scrolling), `-mixed`, `-bidi`
  (titles/bodies with paths, IPs, URLs, e-mail, emoji, parentheses), `panel-calendar`, `panel-notes`,
  `panel-notes-empty`, `panel-settings`, `panel-about`

Per state: (a) `toHaveScreenshot` of the island, and (b) a geometry test reading
`getBoundingClientRect` / `getComputedStyle` against `src/design/tokens.ts` (island size and radius,
compact capsule, alert/toast radius and widths, action row: equal columns filling the row 8px apart and
40px high, dock height 56 and inset, header height 28, card x / width / radius / 8px gap). Geometry does
not depend on antialiasing, so it also catches what the tolerance lets through (see below).
Key states are also rendered at deviceScaleFactor 2 (tests titled `@2x`).

## Determinism

`Date` is frozen to the gallery's `TOUR_NOW` (`page.clock.setFixedTime`), timezone `Asia/Jerusalem`,
locale `he-IL`, dark scheme, `prefers-reduced-motion: reduce`, transitions/animations switched off by an
injected stylesheet, `document.fonts.ready` awaited, fixed viewport, `--disable-lcd-text` and no font
hinting. Playwright also waits for two identical consecutive frames. Nothing is masked.

## Tolerance

`threshold: 0.2` (per-pixel colour distance) and `maxDiffPixelRatio: 0.003` (of the island's pixels).
Repeated runs on one machine were pixel-identical, so this only absorbs antialiasing differences between
Edge versions. Measured against deliberate regressions: content shifted 1px or 2px, a 2px wider island,
a hidden dock all fail (260 to 5400 differing pixels, ratio 0.01 to 0.07); a corner radius change of 2px
or a hidden small icon is below the ratio and is caught by the geometry tests instead.

## Why Edge

The app runs in WebView2, which is the Edge Chromium engine. `channel: "msedge"` uses the installed
Edge, so no browser is downloaded and rendering (text shaping, Segoe UI) matches the app. Baselines
are tied to this machine's fonts and Edge major version; regenerate them after changing either.
