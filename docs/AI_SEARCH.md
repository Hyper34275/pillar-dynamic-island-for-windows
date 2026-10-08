# Smart AI Search: design contract

Status: the contract and the compiling skeleton exist; the implementation follows. Progress, test
results and known limits are kept in [AI_SEARCH_PROGRESS.md](AI_SEARCH_PROGRESS.md).

The user asks a question in Hebrew or English from a bar on the Windows taskbar. The answer appears
in the island at the top of the screen, and the full results open in the Island Center. Everything
runs locally:

- no cloud, no language model, no runtime downloads;
- no DLL injection and nothing loaded into explorer.exe;
- the native Windows search keeps working exactly as before.

## 1. Pieces and data flow

```
 taskbar AI button / Ctrl+Alt+Space / tray "חיפוש חכם"
          │ (user action)
          ▼
 search_bar (Rust) ──creates──► window "search" (search.html: input + glow)
                                   │ Enter → assistant_submit(text, "searchBar")
                                   ▼
 assistant (Rust) ── intent::interpret(text, ctx, now, known)   pure, no I/O
          │       ── executes capabilities (calendar, outlook_mail, local::{files,apps,notes}, calc)
          │       ── keeps results in memory (opaque ids → EntryIDs/paths/AUMIDs)
          ├─ event "assistant-update" (AssistantCard) ──► island: processing → answer/choices
          │                                            ──► search page: glow state
          └─ pipe event "search-ready" {queryId} ──► Island Center (pulls searchResults/searchHistory)
 island "הצג את כל התוצאות" → assistant_open_center(queryId) → center::open("search:<id>")
 Center Smart Search page (chat) → pipe searchSubmit/searchChoose/searchExtend/searchOpen
```

One conversation context (`intent::Ctx`, two minutes, memory only) is shared by the search bar,
the island and the Center chat. So "רק מהשבוע שעבר" typed in the Center refines the last mail
search, even when that search started in the search bar.

## 2. File ownership (implementation phase)

| Owner | Files (new unless marked) |
|---|---|
| A intent | `src-tauri/src/intent/**` (types.rs: additive changes only), `src-tauri/src/intent/corpus.jsonl` |
| B assistant | `src-tauri/src/assistant/**` (wire.rs: additive only), `src-tauri/src/diagnostics.rs` (CODE_PREFIXES only) |
| C mail | `src-tauri/src/outlook_mail.rs` |
| D calendar | `src-tauri/src/calendar.rs`, `src-tauri/src/outlook.rs` (except `run_on_outlook`, already `pub(crate)`) |
| E local | `src-tauri/src/local/**`, `src-tauri/src/com.rs` (additive: `Dispatch::create`), `src-tauri/src/notifications.rs` (expose `launch_aumid`/`is_valid_aumid` as `pub(crate)` only) |
| F search bar (Rust) | `src-tauri/src/search_bar/**`, `src-tauri/src/tray.rs` (one menu item), `src-tauri/src/window.rs` (visibility of helpers only), `src-tauri/capabilities/search.json` |
| G search bar (page) | `search.html`, `src/search/**`, `src/design/tokens.ts` (additive `glow` group), `docs/DESIGN_SYSTEM.md` (glow section) |
| H island | `src/lib/island/state.ts`, `src/hooks/useIslandState.ts`, `src/lib/island/timing.ts`, `src/components/Pill/{PillShell,IslandLayer,usePillGeometry}.tsx/ts`, new `src/components/Pill/{AssistantCard.tsx,assistantLayout.ts}`, `src/gallery/*`, their tests |
| I Center | `center/**`, `src-tauri/src/center.rs`, `src-tauri/src/center_ipc.rs` |
| shared, append-only | `src/lib/i18n.ts` (keys `ai.*` H, `search.*` G), `src-tauri/Cargo.toml` (windows features only), `src/lib/ipc.ts`, `src/lib/assistant/types.ts`, `src-tauri/src/lib.rs` |

Rules:
- Never edit another owner's file. If you need a change there, write it in your report.
- Keep every existing feature and behaviour.
- No new crates and no new npm packages; the build must work offline. New `windows` crate features are allowed.

## 3. Contract (exact signatures are in the skeleton)

- **intent** (`src-tauri/src/intent/mod.rs`, `types.rs`)
  - `interpret(text, &Ctx, now, &Known) -> Interpretation`
  - `Ctx::{remember, clear, last}`
  - `fold`, `detect_lang`, `name_variants`, `weekday_name`, `evaluate_expr`, `sensitivity`
  - `caps::*` (14 capabilities), `Decision`, `Slots`, `AskKind`, `TimeSpec`, `Grain`, `Lang`, `Known`
- **assistant** (`src-tauri/src/assistant/mod.rs`, `wire.rs`)
  - pub fns: `submit`, `choose`, `extend`, `open_item`, `results`, `history`
  - Tauri commands: `assistant_submit`, `assistant_choose`, `assistant_extend`, `assistant_open_item`, `assistant_open_center`, `assistant_dismiss`
  - Wire types: `AssistantCard`, `AssistantItem`, `Choice`, `ResultGroup`, `SearchResults`
- **outlook_mail**
  - `discover_mailboxes(force)`, `cached_mailboxes()`
  - `search_mail(&MailQuery, Option<MailCursor>) -> MailSearchResult`
  - `open_mail(key)`, `free_busy(name, from, to)`
- **calendar**
  - `query_range(from, to, only) -> RangeRead`
  - `known_sources(app)`
- **local**
  - `search_files`, `open_file`
  - `search_apps`, `launch_app`, `warm_up`
  - `search_notes`
- **search_bar**
  - `start`, `apply_settings`, `open`, `close`
  - Commands: `search_bar_state`, `search_bar_close`
  - The `search` window, label `search`, created lazily
  - Event `search-bar-state` (payload `SearchBarState`) to the search page when its geometry or contrast changes
- **Front end**
  - `src/lib/assistant/types.ts`: `AssistantCard` + `normalizeAssistantCard`, `ASSISTANT_UPDATE_EVENT`
  - `ipc.assistant*` and `ipc.searchBar*`
  - `CenterPage` gains `search` / `search:<id>`
- **Settings**: `aiSearchEnabled`, `aiSearchButton`, `aiSearchHotkey`, all default `true`, in Rust, TS and C#.
- **Pipe** (Center ⇄ Rust), all camelCase:

  | Command | Args | Result |
  |---|---|---|
  | `searchSubmit` | `{text}` | `AssistantCard` |
  | `searchChoose` | `{queryId, optionId, remember}` | `AssistantCard` |
  | `searchExtend` | `{queryId}` | `AssistantCard` |
  | `searchOpen` | `{queryId, itemId}` | `null` |
  | `searchResults` | `{queryId}` | `SearchResults` |
  | `searchHistory` | none | `SearchResults[]` |

  - Event `search-ready` `{queryId}` (sent after every final card, any origin).
  - Commands that run searches need a request timeout of at least 45 s on the C# side.
- **Center page**: `search` or `search:<queryId>`. The queryId is 1..64 chars of `[A-Za-z0-9_-]`, the same rule as note ids.

## 4. Behaviour

### 4.1 Understanding (intent)

The intent engine is pure Rust with no dependencies, and its lexicon is compiled in.

- **Normalisation:** niqqud and bidi marks are stripped, final letters folded, geresh and quotes unified.
- **Hebrew prefixes:** ו/ה/ב/ל/מ/ש/כ are stripped, with a whole-word guard first.
- **Verb lemmas:** for example תחפש/חפשי/תמצא map to search/find.
- **Synonyms and typos:** a synonym lexicon, plus a bounded typo distance for keywords only. Names and terms are never typo-corrected.
- **Entities:**
  - person: מ/של/ל/עם + name, "מיובל" → sender יובל
  - "המילה X" / "על X" / quotes → terms
  - mailbox names (matched against the real mailboxes only)
  - numbers and expressions
- **Dates:** the week starts on Sunday.
  - Words: היום, מחר, מחרתיים, אתמול, השבוע, שבוע הבא, שבוע שעבר.
  - Weekday names; dd/mm, dd.mm(.yy).
  - Times of day: בבוקר, אחרי הצהריים.
  - Relative: בעוד שעה/יומיים.
  - An invalid date such as 31.2 → Clarify(Date).
  - An hour 1–6 with no AM/PM marker → Clarify(Time).
- **Decisions:** thresholds on score and margin (constants in `intent/score.rs`).
  - Execute: read-only capabilities only.
  - MultiSource: two read-only candidates that are close.
  - Clarify: a missing or ambiguous slot.
  - Confirm: open/launch. These are never executed from text; a search runs and the candidate is offered for a click.
  - NoMatch.
- **Follow-ups:** while the context is fresh, these inherit the last capability and its slots:
  - "ומה לגבי X", "ומחר?", "what about Tuesday"
  - "רק מ..." / "רק מהשבוע שעבר" / "only from last week" (refinement)
- **Corpus:** `intent/corpus.jsonl` holds at least 120 cases (he, en, mixed, typos, follow-ups, refinements, safety). Gates, enforced by a cargo test:
  - execute precision ≥ 98%
  - zero sensitive executions
  - recall of exec cases ≥ 85%

### 4.2 Execution (assistant)

**Response speed**
- A processing card is emitted at once (before any Outlook call). There is no artificial delay.
- `submit` returns the final card.

**Calendar**
- Default source: the active calendars through `calendar::query_range`.
- **A person** ("מה יש לאיציק ביומן מחר?"):
  1. Match `name_variants(person)` against `calendar::known_sources` names. These are the calendars the user already has in Outlook's Calendar module, so their permission is already given. Then read with `only=[id]`.
  2. No calendar matches → `outlook_mail::free_busy`, which gives busy blocks without titles. The answer says so.
  3. Both fail → an error card telling the user to open that calendar in Outlook.
- The answer gives the number of meetings and their times, e.g. "מחר יש לאיציק 3 פגישות" with "09:00 · 11:30 · 14:00".
- Availability: free means no Busy/OOF/Tentative event overlaps the window. The answer lists the free slots during working hours (08:00–18:00).

**Mail**
- Mailbox plan:
  - A named mailbox → that mailbox.
  - The user asked for all mailboxes, or named a sender → every searchable mailbox.
  - Only one searchable mailbox → that one.
  - A saved preference whose mailbox set has not changed since it was saved → the preference.
  - Otherwise → a Choices card: "באיזו תיבת דואר לחפש?". The options are the real mailboxes, plus the AllMailboxes option "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן."
- A typed reply to a pending question is matched against the offered options first. "אני לא יודע", "לא יודע", "כולן", "בכל התיבות" and "all" select AllMailboxes.
- Search budget: 10 s. Results found so far are shown with `partial`/`canExtend`. `extend` continues from the cursor for another 10 s.

**Files, notes, apps, calculator**
- Files, notes and apps: search, then show the results. Opening or launching needs a click on an item.
- Calculator: answered inline.

**Results, ids and logging**
- The store keeps the last 20 queries for 30 minutes.
- Item ids are opaque and resolve to the real target only inside the store.
- The log records the capability, decision, counts and duration. It never records the text, names, subjects or paths.

**Error codes**
- APP-040: smart search is off
- APP-041: the search has expired
- APP-042: invalid question
- MAIL-101: no permission
- MAIL-102: mailbox gone
- MAIL-103: mailbox offline
- MAIL-104: mail gone
- MAIL-105: mailbox timed out
- MAIL-109: other
- FILES-101: index unavailable (fallback used)
- FILES-104: file gone
- APPS-104: app gone
- `MAIL`, `FILES` and `APPS` are added to `diagnostics::CODE_PREFIXES`.

### 4.3 Mail search (outlook_mail)

- **Attach-only.** One STA thread per call (`outlook::run_on_outlook`), so a hung store cannot block the calendar.
- **Discovery:**
  - Read `Session.Stores` and skip public folders (ExchangeStoreType 2).
  - Kind comes from ExchangeStoreType and IsDataFileStore.
  - Access and availability come from opening `GetDefaultFolder(6)` on each store. E_ACCESSDENIED → Denied. Network codes → Offline.
  - Stores are read under a per-store time budget, and the result is cached for 10 minutes.
  - `GetDefaultNavigationGroup`, `Logon` and `GetSharedDefaultFolder` are never called.
- **Search:**
  - `Folder.GetTable` with a DASL `@SQL=` filter: subject LIKE, sender name LIKE, and date / unread. Where the store's instant search is on, the filter also uses `ci_phrasematch` on the body (best effort).
  - Columns: EntryID, subject, sender name, received, unread.
  - Tier 1 is the Inbox and Sent Items of every chosen mailbox. Tier 2 is their subfolders (depth ≤ 4, ≤ 200 folders, mail folders only; Deleted, Junk, Drafts, Outbox and Sync Issues are skipped).
  - User terms are sanitised: `'` is doubled, and `% _ [ ] "` are stripped. There are at most 6 terms of at most 40 chars each.
  - A failure in one mailbox is reported for that mailbox only.
- **Open:** `GetItemFromID(entryId, storeId).Display(false)` after `AllowSetForegroundWindow(outlook pid)`.

### 4.4 Search bar (AI Mode)

**Anchor (Windows 10)**
- The search box rect is found under `Shell_TrayWnd` (`TrayDummySearchControl`; UI Automation is the fallback). It is used only when `SearchboxTaskbarMode` = 2 and the taskbar is at the bottom or top.
- Position is tracked with out-of-context WinEvent hooks on the explorer thread, debounced to about 50 ms, plus `TaskbarCreated` (Explorer restart) and DPI/display changes.
- The button is hidden when:
  - the taskbar auto-hides off screen;
  - something is fullscreen;
  - the Windows search flyout is open (Win+S);
  - Alt+Tab is up.

**AI button**
- A native layered tool window, about 24 DIP, `WS_EX_NOACTIVATE`, drawn without GDI aliasing.
- It sits at the right edge inside the search box and never takes focus.
- A click toggles AI Mode.

**Input window**
- A Tauri window, label `search`, created on first use.
- **Size:** anchored, it covers the search box rect plus a 6-DIP glow margin. Unanchored, it is a floating 560×48 DIP bar, centred, 16 DIP above the taskbar of the primary monitor.
- **Focus:** it takes the keyboard; the user's click or hotkey grants the foreground.
- **Leaving AI Mode:** Esc, the AI button again, or a click elsewhere (deactivation) closes it.

**Fallbacks**
- Ctrl+Alt+Space (RegisterHotKey) and the tray item "חיפוש חכם" open the same input.
- These cover Windows 11, an icon-only or hidden search box, a vertical taskbar, and any failure of the anchor probe.
- In AI Mode off, nothing is intercepted.

**Glow** (inspired by brunnolou/glowing; no code copied, see THIRD_PARTY_NOTICES.md)
- A conic-gradient ring (Cyan #40C8E0, Violet #BF5AF2, Indigo #5E5CE6, Magenta #E040C8, Soft Pink #FFA3C7) behind an opaque light plate that has the bar's own size.
- A pre-blurred aura.
- Only `transform` and `opacity` animate. Nothing animates while Idle or Disabled.
- `contain: paint` and the window region clip it, so nothing bleeds outside.
- States: Idle, Activated, Typing, Submitting, Processing (only while the request runs), Completed, Error, Disabled.
- Reduced motion: a static gradient; states shown by opacity and colour only.
- High contrast: a 2 px system-colour ring, no colours.

### 4.5 Island

**The assistant surface**
- A new temporary view with priority 2.6: above notifications and the ringer, below meeting alerts.
- A meeting alert pre-empts it, and the card comes back afterwards.

**Layout**
- Processing: a single line with a sparkle.
- Answer: headline, up to 2 summary lines, and up to 3 items with time, title and source.
- Then actions:
  - "הצג את כל התוצאות" whenever `total > items shown` or there is more than one source;
  - "חפש עוד 10 שניות" when `canExtend`;
  - the choices for a Choices card.
- It stays within the existing 400×440 bound, so the stage size does not change.

**Behaviour**
- It never takes the keyboard. Choices are clicked, or answered by typing in the search bar.
- Esc or close dismisses it (`assistant_dismiss`).
- Unattended timing:
  - an answer stays 12 s;
  - a Choices card stays until answered, at most 60 s.
  - Both pause on hover.

### 4.6 Island Center: Smart Search page

- A new navigation item "חיפוש חכם" and page kind `Search`.
- **Chat layout:**
  - The conversation from `searchHistory`.
  - User bubbles; answer cards with result cards grouped by source (mail grouped per mailbox).
  - Choice buttons and an extend button.
  - An "פתח" button per openable item.
- A RTL text box at the bottom submits through `searchSubmit`. Follow-ups and refinements work there too.
- It refreshes on `search-ready` and opens on `search:<id>` (scrolled to that query).
- Settings page: a "חיפוש חכם" section with the three switches and a privacy note (everything local, nothing stored).
- Hebrew strings in `Strings.cs`.

## 5. Privacy and safety

- Query text, answers, subjects, names, paths and note text are memory only. They are never logged or persisted. The one exception is the mailbox preference, which stores hashed mailbox ids only, in `state/assistant_prefs.json`.
- Open and launch run only from an explicit click on an item id minted by Rust. Executable file types are revealed in Explorer, not run. Raw paths and commands typed by the user are never executed.
- Nothing is sent over the network beyond what Outlook itself does: address-book resolution for free/busy, and Exchange searches.

## 6. Verification limits on the dev machine

The dev machine runs Windows 11 with a non-Exchange Outlook profile. Not verifiable here:

- the Windows 10 search box anchor;
- Exchange shared mailboxes and archives;
- free/busy against a GAL;
- Hebrew word-breaking of the Windows Search indexer.

`scripts/win10-taskbar-probe.ps1` is a read-only dump to run once on a Windows 10 21H2 PC; its output confirms or corrects the anchor's class names.

## 7. Implementation status

Phases 2-9 are implemented and unit-tested (cargo 474 passed with 6 ignored live probes, vitest 943, dotnet 161, tsc clean, vite build OK) but the new build has not been run live yet. A fix round for 36 adversarial-review findings is being merged. Phases 10-12 (optimization, installer 1.0.12, QA) are pending. Per-phase commits, tests and limits are in `docs/AI_SEARCH_PROGRESS.md`; the limits in section 6 still apply, and the Center chat UI has only been compiled, never rendered.

## 8. Commands

"תחפש בגוגל חתולים", "תפתח את ynet", "פתח הגדרות wifi", "פתח הורדות", "תרשום פתק: לקנות חלב", "תכתוב מייל לדני בנושא תקציב", "תנעל את המחשב". The smart search understands explicit commands in Hebrew and English (masculine, feminine, infinitive and imperative forms; with or without "בבקשה", "תוכל", "אפשר", "לי"; the Hebrew letters ב ל מ ה ו ש כ attached to a word, also with a hyphen: "ב-גוגל").

### 8.1 The confirm policy

**A command never runs from text.** Every capability below is `Decision::Confirm { cap }` (sensitivity `Open`, `Launch` for the lock): the card says what it understood and shows one openable item; only a click on that item does it. A command that is missing its text (for example "תחפש בגוגל" alone) is a `Clarify(Content)` with a specific question ("מה לחפש?", "מה לרשום בפתק?", "מה לתרגם?").

- The click is `assistant_open_item` (island) or `searchOpen` (Center). The item is a native button, so Enter and Space on the focused button work; a screen reader hears the item together with the headline.
- Nothing typed by the user is ever executed: an item holds a validated https address, or a key of a fixed table (settings page, folder, site), or the text of a note / the recipient and subject of a new mail. The ids are opaque (section 5).
- The wire shape is unchanged: the item has the new `kind: "action"` (`ItemKind::Action`) in a group of kind `"action"`, and no new field. The Center reads `kind` as a string, so an older Center shows it as information with its "פתח" button; the island's TypeScript accepts it (`ITEM_KINDS`).
- Shutdown, restart, delete and "send" are not commands at all; "שלח מייל לדנה" stays unrecognised.

### 8.2 Capabilities (`intent::caps`, now 21)

| Capability | Sensitivity | Slots (new, all optional) | What the click does |
|---|---|---|---|
| `web.search` | Open | `engine`, `query`, `lang_to`, `lang_from` | Opens the search in the default browser (`ShellExecuteW "open"`, https only) |
| `web.open` | Open | `site` (table key) or `url` (typed address) | Opens the website |
| `system.open_settings` | Open | `setting` (table key) | Opens an `ms-settings:` page, the Control Panel (`control.exe`) or the screen snip (`ms-screenclip:`) |
| `folders.open` | Open | `folder` (table key) | Opens the folder in Explorer; the path comes from the shell (`SHGetKnownFolderPath`), never from text |
| `notes.create` | Open | `query` (the note) | Saves the note through `notes::save` exactly like the island's composer; refuses a full list instead of dropping the oldest note |
| `mail.compose` | Open | `mail_to`, `mail_subject` | Displays a NEW message in the user's own running classic Outlook (`CreateItem` + `Display`), To and Subject filled. **Never sent**: the code has no path that sends, and a test fails if one appears |
| `system.lock` | Launch | none | `LockWorkStation` |

`apps.launch` is unchanged ("תפתח/פתח/תריץ/תפעיל/הפעל/open/launch/start X" reach it; the Hebrew names of common programs were added to `local::apps`: מחשבון, פנקס רשימות, צייר, כלי חיתוך, מנהל המשימות, סייר הקבצים, שורת הפקודה, פאוורשל, אאוטלוק, וורד, אקסל, פאוורפוינט, וואן נוט, טימס, כרום, אדג', פיירפוקס, זום, וואטסאפ, ספוטיפיי, פתקיות, מצלמה, שעון). A program that is also a website opens as the program ("פתח וואטסאפ"); the website needs a word that says so ("וואטסאפ ווב", "האתר של וואטסאפ", "outlook web").

### 8.3 What is understood

- **Web search** (`web.search`): Google by default ("תחפש בגוגל X", "תגגל X", "גוגל X", "google X", "תחפש באינטרנט X", "search the web for X"); YouTube ("תחפש ביוטיוב X", "שים ביוטיוב X", "play X on youtube"); Wikipedia (he/en by the query's script); Google Maps ("איפה נמצא X", "מפה של X", "תנווט ל-X", "איך מגיעים ל-X", "directions to X"); Waze (`https://waze.com/ul?q=X`); Bing only when named; Google Translate ("תתרגם X", "איך אומרים X באנגלית", "translate X to hebrew" -> `translate.google.com/?sl=auto&tl=<code>&text=X`; the target is English for Hebrew text and Hebrew for the rest unless a language is named). The engine can stand in front of the query, behind the verb or at the end ("תחפש X בגוגל"). The query is kept as typed (wrapping quotes, a leading "את" / "על" and a final full stop removed) and percent-encoded (UTF-8, so Hebrew is safe); an address longer than 2000 characters is refused, never cut.
- **Websites** (`web.open`): a built-in table of 53 sites (ynet, walla, mako, haaretz, kan, youtube, gmail, google drive / calendar / maps / translate, waze, wikipedia, facebook, instagram, linkedin, x, netflix, chatgpt, github, whatsapp web, spotify web, outlook web, microsoft 365, teams web, and Israeli news, bank and government sites whose address is certain) with their Hebrew spellings; any typed domain or http(s) address ("תפתח את example.co.il"). Only `http` and `https` are ever opened. A name that is a file ("report.pdf", "setup.exe") is never taken for a domain (the top-level domains accepted are a list without the file extensions).
- **Windows settings** (`system.open_settings`): about 50 pages in the `SETTINGS` table of `local/system_actions.rs`, each URI checked against Microsoft's "Launch the Windows Settings app" reference for Windows 10 22H2 (a unit test compares the table with the reference list; an unknown key opens `ms-settings:`). Some words are also program names ("מצלמה", "רשת"): they open the settings only with the word "הגדרות" ("פתח הגדרות מצלמה"), otherwise the program.
- **Folders** (`folders.open`): הורדות, מסמכים, שולחן העבודה, תמונות, מוזיקה, סרטונים, סל המחזור, המחשב הזה, OneDrive (only when it is set up on the PC).
- **Notes and mail**: "תרשום פתק: X", "תוסיף פתק X", "פתק חדש: X", "note: X", "take a note: X"; "תכתוב מייל לדני בנושא X", "תכתוב לדני מייל", "write an email to X about Y", "email X about Y". "פתח מייל" alone is still opening the mailbox; only "פתח מייל חדש" is a new message.
- **Not a command** (stays with the normal engine): "מה יש לי ביומן", "המייל מגוגל", "פגישה עם גוגל מחר", "תפתח את המייל האחרון", "איפה נמצא הקובץ של דני" (a mail, file, note or meeting word anywhere in the object), a path, a program file, anything with an unknown object.

### 8.4 "I don't know" still offers the web

When nothing is recognised the card keeps its honest message ("אפשר לשאול למשל: ...") and also offers one item, "חפש בגוגל: ״<text>״" (a click, like everything else). Questions about the weather, currency rates, the news and sports ("מה מזג האוויר", "שער הדולר", "תוצאות הכדורגל") get a fitting headline ("את מזג האוויר אפשר לבדוק בגוגל") and are never guessed as a mail or file search (they stay `NoMatch` at the intent level unless they name something local). The offer is not made for a path, a program file, a very long text, or a request to delete, cancel, send, move or install something.

### 8.5 Code and tests

- `intent/commands.rs` (grammar), `intent/lexicon/commands.json` (words and catalogues, compiled in), `intent/commands_tests.rs` (more than 150 Hebrew and 50 English phrasings with the decision, the capability and the extracted query / site / setting / folder, the negatives above, and consistency checks between the lexicon and the tables).
- `local/system_actions.rs`: the tables (engines, sites, settings, folders, languages), URL building and validation, and the effects. `assistant/actions.rs`: the cards, the click (`Action`, kept in the store as `Target::Action`) and the notes saving. `exec.rs`, `flow.rs`, `store.rs`, `wire.rs` and `mod.rs` of `assistant` only gained the hooks (`Sources::run_action`, one arm in `confirm`, one in `Engine::open`).
- Error codes (family `APP`): APP-050 an address could not be opened, APP-051 a settings page, APP-052 a folder (missing on this PC or could not open), APP-053 locking, APP-054 a note (empty or the list is full). Text typed by the user is never logged.
- Not verifiable on the dev machine: the Outlook new-message window on an Exchange profile, `LockWorkStation` and the settings pages on a real Windows 10 22H2 PC (the table is checked against the documentation, not on a device).
