import plugin from "tailwindcss/plugin";
import { cssVariables, palette, paletteHighContrast, tailwindTheme } from "./src/design/tokens";

/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./tour.html", "./gallery.html", "./src/**/*.{js,ts,jsx,tsx}"],
  // Every island dimension, radius, type role and colour comes from src/design/tokens.ts.
  theme: {
    extend: tailwindTheme,
  },
  plugins: [
    // The colour tokens as CSS variables (one source, overridable for high contrast).
    plugin(({ addBase }) => {
      addBase({
        ":root": cssVariables(palette),
        "@media (prefers-contrast: more)": { ":root": cssVariables(paletteHighContrast) },
      });
    }),
  ],
};
