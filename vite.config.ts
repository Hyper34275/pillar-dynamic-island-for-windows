import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import pkg from "./package.json";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],

  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },

  // WebView2 (Chromium 111+) is guaranteed by the installer, so modern CSS such as
  // color-mix() can ship untranspiled.
  build: {
    target: "chrome111",
  },

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    include: ["src/**/*.test.{ts,tsx}"],
    environment: "node",
  },
});
