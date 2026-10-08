# Smart AI Search: progress

Each phase lists what changed, which tests ran and their results, the commit, and the limits that
are real. A phase is marked done only when it has been tested; "not verified" says where.

| # | Phase | Status | Commit | Tests | Real limits |
|---|---|---|---|---|---|
| 1 | Code review + research (8 parallel read-only agents) | done | n/a | n/a | Research notes cover the code map, Outlook stores and mail, the Windows 10 overlay, Glowing, files/apps and the intent design |
| 1b | Contract + compiling skeleton (`docs/AI_SEARCH.md`) | done | (this commit) | cargo test 237 ✓, vitest 829 ✓, tsc ✓ | none |
| 2 | Glow effect (own implementation, inspired by Glowing) | pending | | | |
| 3 | Search bar POC (anchor, button, hotkey, input window) | pending | | | The Windows 10 anchor cannot be tested on the Windows 11 dev box |
| 4 | Intent engine (offline, rules) | pending | | | |
| 5 | Mailbox discovery + multi-mailbox mail search | pending | | | The dev profile is not Exchange |
| 6 | Shared calendar understanding (owner names) | pending | | | |
| 7 | File & notes search (+ apps, calculator) | pending | | | |
| 8 | Island responses | pending | | | |
| 9 | Island Center Smart Search (chat) | pending | | | |
| 10 | Optimization (8 GB) + load tests | pending | | | |
| 11 | Offline installer | pending | | | |
| 12 | QA & regression | pending | | | |

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
