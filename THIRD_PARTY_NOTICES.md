# Third-party notices

## Glowing (not used as code)

The Smart Search glow was inspired by https://github.com/brunnolou/glowing (inspected at commit
`f20947f9733d3c53efad2bd226f50da3ffe0a2c7`). That repository has no LICENSE file: its `package.json` says MIT,
and an Apache-2.0 file was added and then removed (commit `970a83d`). Because the licence is unclear, **no code
from Glowing was copied**. Our glow is an original implementation of the general idea (a rotating gradient ring).

## Runtime dependencies (high level)

Licences are as commonly published by each project; check the package itself for the authoritative text.

| Component | Used for | Licence |
|---|---|---|
| Tauri, wry, tao (Rust) | App shell, webview, windowing | MIT or Apache-2.0 |
| serde (Rust) | Serialization | MIT or Apache-2.0 |
| tokio (Rust) | Async runtime | MIT |
| chrono (Rust) | Dates and times | MIT or Apache-2.0 |
| windows-rs (Rust) | Windows API bindings | MIT or Apache-2.0 |
| sha2 (Rust) | Hashing (mailbox ids) | MIT or Apache-2.0 |
| React, react-dom (JS) | UI | MIT |
| motion (JS) | Animation | MIT |
| Windows App SDK / WinUI 3 | Island Center | MIT |
| WebView2 runtime | Web content host | Microsoft licence (redistributable terms) |

Exact versions are locked in `Cargo.lock`, `package-lock.json` and the Center `.csproj`. Transitive dependencies
carry their own licences, which are recorded in those lock files.
