# Smart AI Search: progress

Each phase lists what changed, which tests ran and their results, the commit, and the limits that
are real. A phase is marked done only when it has been tested; "not verified" says where.

| # | Phase | Status | Commit | Tests | Real limits |
|---|---|---|---|---|---|
| 1 | Code review + research (8 parallel read-only agents) | done | n/a | n/a | Research notes cover the code map, Outlook stores and mail, the Windows 10 overlay, Glowing, files/apps and the intent design |
| 1b | Contract + compiling skeleton (`docs/AI_SEARCH.md`) | done | 4e46238 | cargo test 237 ✓, vitest 829 ✓, tsc ✓ | none |
| 2 | Glow effect (own implementation, inspired by Glowing) | built, not live-verified | b19031d | integrated run: cargo 474 ✓ (6 ignored live probes), vitest 943 ✓, dotnet 161 ✓, tsc ✓, vite build ✓ | Never run live with the new build; a search-page fix is still landing |
| 3 | Search bar POC (anchor, button, hotkey, input window) | anchor geometry verified on Windows 10 21H2; app not yet run there | 3a9fdd8, a6beed8, b4e96f6 | layout tests incl. the measured Win10 geometry; live on Win11: Ctrl+Alt+Space opens the bar with focus, glow states, Hebrew question answered | Probe on a real Windows 10 Enterprise 19044 PC (2026-10-08): TrayDummySearchControl is a direct child of Shell_TrayWnd, 344x40 at 48,1160 (1920x1200, 96 DPI, bottom taskbar 40 px); SearchboxTaskbarMode is ABSENT with the default setting, now handled. The input sits flush on the box (window = box). Not yet seen on Win10: the button drawing, z-order against the taskbar, Win+S/Alt+Tab hiding, Explorer restart, colour/font match with the native box |
| 4 | Intent engine (offline, rules) | done at unit level, fixes landing | e9deb75 | Corpus 227 cases: execute precision 100%, holdout recall 100% (optimistic: corpus written alongside the engine). Independent set of 173 new phrasings: ~80% pass before fixes. Latency p95 0.34-0.44 ms (debug) | Failing review phrasings (senders "מ-Avi", "מוזכר", calculator "17% ממאתיים", spelled hours, cancel/move verbs) are being fixed |
| 5 | Mailbox discovery + multi-mailbox mail search | built, not live-verified | 9f0309d | unit tests ✓; fix round: continuation, key cap, free/busy guard | Dev profile is not Exchange: shared/delegate mailboxes and archives NOT verified. Live read-only probe: Outlook GetTable + DASL with a Hebrew term works |
| 6 | Shared calendar understanding (owner names) | built, not live-verified | 4e1e421 | unit tests ✓; fix round: quarantine, DST | Free/busy against a GAL NOT verified. Table dates are UTC (live probe) |
| 7 | File & notes search (+ apps, calculator) | built, not live-verified | 62b8acf | unit tests ✓; files/apps fixes still landing | Live probes: Windows Search index via ADO 50-230 ms; Apps folder 160 apps in ~0.8-1 s. Hebrew word-breaking of the index NOT verified |
| 8 | Island responses | built, not live-verified | 18653ad | integrated run; end-to-end unit test c79b0b4 | Never run live with the new build |
| 9 | Island Center Smart Search (chat) | built, not live-verified | ae08652 | compiled; dotnet 161 ✓ | Chat UI never rendered; Center pipe fixes still landing |
| 10 | Optimization (8 GB) + load tests | pending | | | Performance on a real 8 GB / integrated-GPU Windows 10 PC not measured |
| 11 | Offline installer | built and verified, not installed yet | 02deac7 | verify-installer.ps1: OK; CompanyIsland_1.0.12_x64-setup.exe 230.2 MiB, SHA256 80F03D1BD654C11A8B411DD19EA9E469820A41C509E4947C48D06D34E45BEE52; Center included; search.html in the bundle | Same NSIS offline mechanism as 1.0.11 (WebView2 offline installer, Center bundled); nothing is downloaded at run time. Not yet installed on a Windows 10 PC |
| 12 | QA & regression | pending | | See "Smart AI Search" in `docs/QA_MATRIX.md` | Live test of the new build approved by the user, not yet run |

Also done: assistant layer 3d95e2e (fix round: privacy off-switch, undismiss, 31-day clamp). A fix round for 36 confirmed
adversarial-review findings is being merged now.

End-to-end (unit level, real intent engine + fake sources, c79b0b4): "מה יש לאיציק ביומן מחר?" gives "מחר יש לאיציק 3 פגישות" plus times;
"תמצא את המייל עם המילה תקציב." with 3 mailboxes asks "באיזו תיבת דואר לחפש?"; typed "אני לא יודע" searches all mailboxes with a
10 s budget, partial result plus extend; "רק מהשבוע שעבר" refines the same search.

## Glowing audit (phase 2 input)

- Repo: https://github.com/brunnolou/glowing, inspected at commit `f20947f9733d3c53efad2bd226f50da3ffe0a2c7` (2025-04-24, v0.2.3).
- **Licence:** `package.json` says MIT, but the repository has no LICENSE file. An Apache-2.0 LICENSE.txt was added and later removed (`970a83d`). The licence is therefore unclear, so **no code is copied**; only the visual idea (a rotating conic-gradient ring behind a mask) is used.
- **Dependencies:** none at run time. The dev dependencies are tsup, vite, vitest, typescript and prettier.
- **Not embedded, for these reasons:**
  - It animates `@property --angle` every frame (a repaint plus three blurs per frame), which is costly on integrated GPUs and over RDP.
  - It has no reduced-motion handling.
  - The blur bleeds outside the element.
  - Its event listeners leak.
  - It is imperative DOM code, not React.
