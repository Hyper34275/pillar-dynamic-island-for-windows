import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import pkg from "./package.json";

/**
 * tour.html carries a strict meta CSP (script-src 'self') for the built page. The dev server
 * (plugin-react) injects an inline React-refresh script into every page, which that CSP blocks and
 * leaves the page blank, so under `npm run dev` the meta is dropped. A build keeps it.
 */
function tourDevCsp(): Plugin {
  return {
    name: "tour-dev-csp",
    apply: "serve",
    transformIndexHtml: {
      order: "pre",
      handler: (html) => html.replace(/<meta\s+http-equiv="Content-Security-Policy"[^>]*>\s*/i, ""),
    },
  };
}

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react(), tourDevCsp()],

  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },

  // WebView2 (Chromium 111+) is guaranteed by the installer, so modern CSS such as
  // color-mix() can ship untranspiled.
  build: {
    target: "chrome111",
    // Three pages: the island, the tour (tour.html, shown by the Island Center) and the smart search bar.
    rollupOptions: {
      input: {
        main: "index.html",
        tour: "tour.html",
        // The smart search input + glow (search_bar window, label "search").
        search: "search.html",
      },
    },
  },

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**", "**/center/**"],
    },
  },

  test: {
    include: ["src/**/*.test.{ts,tsx}"],
    environment: "node",
  },
});
