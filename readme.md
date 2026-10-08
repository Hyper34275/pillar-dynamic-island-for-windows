# Yuval (יובל)

A small always-on-top "Dynamic Island" for Windows 10 (21H2+) and Windows 11, built for managed
company PCs. It shows today's date and weekday at the top of the screen, and expands into a
date and time view, your next Outlook meetings, and an About page with diagnostics.

- **Standard users only.** Nothing needs administrator rights at runtime; all data lives under
  `%LOCALAPPDATA%\Yuval\` (an older `CompanyIsland` folder is moved there once, on the first start).
- **Offline.** No cloud services, no accounts, no telemetry.
- **Classic Outlook.** Meetings are read from the running classic Outlook (attach only, default
  calendar, read only). The new Outlook is detected and reported but not supported.
- **Private.** Meeting subjects, locations and notification text never reach logs, diagnostics
  or crash reports.
- **Hebrew and English UI.** Dates and times follow the Windows language through `Intl`; the
  island itself keeps a fixed left-to-right layout (date left, weekday right).

The design contract (IPC commands, events, error codes, privacy rules) is in
[`docs/ENTERPRISE_DESIGN.md`](docs/ENTERPRISE_DESIGN.md); building, deploying and signing the installer
is described in [`docs/INSTALLER.md`](docs/INSTALLER.md).

## Development

Requirements: Windows 10/11, Node.js 18+, [Rust](https://rustup.rs/).

```bash
npm install
npm run tauri dev     # run the app
npm test              # unit tests (vitest)
npx tsc --noEmit      # type check
npm run build:installer  # NSIS installer, see docs/INSTALLER.md
```

The frontend is React 18 + TypeScript + Tailwind + Motion on Vite; the backend is Tauri 2 (Rust).
Frontend code talks to Rust only through `src/lib/ipc.ts`.

## Upstream and license

Yuval (formerly CompanyIsland) descends from [PILLAR](https://github.com/warpirate/pillar-dynamic-island-for-windows)
(MIT License) and keeps its license; see [LICENSE](LICENSE).
