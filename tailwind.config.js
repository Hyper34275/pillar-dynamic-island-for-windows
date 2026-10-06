/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  // The UI uses stock Tailwind utilities only; add design tokens here when a component needs one.
  theme: {
    extend: {},
  },
  plugins: [],
}
