import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import pkg from "./package.json";

// Production-like build of the design gallery only (gallery.html), used by the visual tests
// (playwright.config.ts). Its own outDir under node_modules/.cache so nothing new needs ignoring and
// the app's `npm run build` output (dist/) is untouched.
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  build: {
    target: "chrome111",
    outDir: "node_modules/.cache/gallery-dist",
    emptyOutDir: true,
    rollupOptions: { input: { gallery: "gallery.html" } },
  },
  preview: { port: 4179, strictPort: true },
});
