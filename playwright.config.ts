import { defineConfig } from "@playwright/test";

// Visual regression of the island's states. Uses the INSTALLED Microsoft Edge (same Chromium engine
// as the WebView2 runtime the app runs in); no Playwright browser download. See tests/visual/README.md.
export default defineConfig({
  testDir: "tests/visual",
  testMatch: /.*\.spec\.ts/,
  snapshotPathTemplate: "{testDir}/__screenshots__/{projectName}/{arg}{ext}",
  outputDir: "node_modules/.cache/pw-results",
  fullyParallel: true,
  workers: 4,
  retries: 0,
  reporter: [["list"]],
  timeout: 30_000,
  expect: {
    toHaveScreenshot: {
      // Per-pixel colour distance (0..1, YIQ) before a pixel counts as different, and the share of
      // differing pixels tolerated. Tuned in tests/visual/README.md.
      threshold: 0.2,
      maxDiffPixelRatio: 0.003,
      animations: "disabled",
      caret: "hide",
    },
  },
  use: {
    channel: "msedge",
    baseURL: "http://127.0.0.1:4179",
    viewport: { width: 800, height: 600 },
    locale: "he-IL",
    timezoneId: "Asia/Jerusalem",
    colorScheme: "dark",
    reducedMotion: "reduce",
    launchOptions: { args: ["--font-render-hinting=none", "--disable-lcd-text"] },
  },
  projects: [
    { name: "1x", use: { deviceScaleFactor: 1 } },
    { name: "2x", use: { deviceScaleFactor: 2 }, grep: /@2x/ },
  ],
  webServer: {
    command: "npx vite build --config vite.gallery.config.ts && npx vite preview --config vite.gallery.config.ts --host 127.0.0.1",
    url: "http://127.0.0.1:4179/gallery.html",
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
