# Hebrew language engine

The understanding layer of smart search (`src-tauri/src/intent`). It reads a free Hebrew or
English question and returns one of three outcomes:

- a capability with its slots, which `assistant` then runs against the real sources: Outlook
  mail, own and shared calendars, notes, files, apps and the calculator;
- a short clarifying question;
- a refusal.

It is not a second search system: it never reads data. Everything is local, with no model, no
network, no Python and no runtime downloads.

Contract and data flow: [AI_SEARCH.md](AI_SEARCH.md). Measured results:
[eval/LABEL_FIXES.md](../src-tauri/src/intent/eval/LABEL_FIXES.md).

## 1. Pipeline

`analyze(text, ctx, now, known) -> (Interpretation, Analysis)`. `interpret()` is
`analyze().0`. Every step is pure.

| step | file | what it does |
|---|---|---|
| normalise | `normalize.rs` | Strips niqqud and bidi marks, unifies gershayim and quotes (לו״ז = לו"ז), folds final letters, lowercases Latin. Keeps quoted phrases and abbreviations as one token. |
| annotate | `lexicon.rs` | Each word becomes a concept: exact form, inflection, possessive form, then proclitics (ו/ה/ב/ל/מ/ש/כ). Next come multi-word phrases and words typed without a space ("מהיש"). Spelling correction runs last, on what is still unknown. |
| spell | `spell.rs` | Weighted edit distance with ranking (see 3). |
| dates | `dates.rs` | Relative and absolute dates, weekdays, parts of the day, hours, "בעוד", "השבוע האחרון", "N הימים האחרונים". |
| entities | `entities.rs` | People, senders, mailboxes, search terms, exact words, flags (unread, latest, limit). |
| score | `score.rs` | Features to a score per capability. Thresholds on the score and the margin give Execute, MultiSource, Clarify, Confirm or NoMatch. |
| context | `mod.rs`, `context.rs` | Follow-ups and refinements within two minutes. |
| analysis | `analysis.rs` | Data source, confidence parts, corrections and reason codes. |

## 2. The concept dictionary

The dictionary is data compiled into the binary, so nothing can be missing on a closed network.

| file | holds |
|---|---|
| `lexicon/words.json` | Concept → single-word forms. For example, `N_MAIL`: מייל, אימייל, דוא"ל, הודעה, התכתבות, אינבוקס, … |
| `lexicon/phrases.json` | Concept → multi-word phrases: "לוח זמנים", "סדר היום", "איפה שמרתי", "יש לי זמן", "תזרוק לי". The first word carries the concept, the rest are `PART`. |
| `lexicon/time.json` | Day, week, month and part-of-day words. |
| `lexicon/names.json` | Nickname groups (איציק/יצחק), Latin spellings, words that are never names. |
| `lexicon/real_words.txt` | Real Hebrew words one cheap edit from a keyword ("קצבים", "הים", "מחיר"). They are corrected only with context support. |
| `lexicon/freq_he.tsv`, `lexicon/office_words.txt` | Word frequencies (wordfreq, CC BY-SA 4.0) and hand-written office words; see 6. |

The main concepts:
- `N_MAIL`, `N_CAL`, `N_MEETING`, `N_FILE`, `N_PPT`, `N_NOTE`, `N_APP`, `N_MAILBOX`, `N_SHARED` (objects)
- `H_FILE`, `H_NOTE`, `H_MAIL` (hints such as "איפה שמרתי", "רשמתי", "קיבלתי")
- `V_SEARCH`, `V_SHOW`, `V_OPEN`, `V_LAUNCH` (verbs)
- `VETO` (write and out-of-scope words, refused)
- `YESH` / `FREE` (calendar questions)
- `MARK` (exact-word markers)
- `P_*` (prepositions)
- `T_*` / `D_*` (time)

**Adding a word:** put it in the right concept in `words.json`, or in `phrases.json` for several
words. Then run the tests:

```
cargo test --lib intent::
```

The tests check that no form is in two concepts.

**Ambiguous words:**
- "הודעה" means mail.
- "אירוע" means a calendar event.
- An object noun only decides the capability when nothing else contradicts it. In "המצגת
  מהישיבה" the meeting is the file's topic. In "מיילים עם קובץ מצורף" the file is an attribute of
  the mail.
- A question with words but no object ("תחפש את החשבונית") searches mail, files and notes together
  (MultiSource), rather than guessing one.

## 3. Inflections and spelling

**Inflections:**
- Proclitics are stripped only when the rest is a known word. A verb takes only ו/ש in front, so
  "המספר" is not ה + מ + ספר. A two-letter grammar word takes only ו/ש, so "כהן" is not כ + הן.
- Plural, construct and possessive forms are built when the lexicon loads: יומני, ביומני,
  פגישותיי, קבציו, פגישתי.
- Search terms keep the word as typed and add the form without a proclitic as an OR alternative
  ("התקציב" → התקציב | תקציב).
- Exact words get no alternative at all.

**Spelling correction** applies to keywords only, never to names, search terms, numbers, or the
words right after a topic marker. Edit costs:

| cost | edit |
|---|---|
| 0.5 | sound or look confusions (ק/כ, ס/ש, ט/ת, א/ע, ח/כ, ב/ו, ו/י, א/ה, ד/ר, ה/ח); a missing or extra vowel letter (ו/י/א/ה/ע); a doubled letter |
| 0.75 | a neighbouring key on the SI-1452 Hebrew keyboard; two swapped letters; a letter missing at the end |
| 1 | anything else |

**Rules:**
- **Budget by word length:** 3 letters: 0.5; 4: 0.75; 5–6: 1.0; 7+: 1.5.
- **First letter:** must match, or be a cheap confusion.
- **Ranking:** cheapest edit first. Ties go to the more frequent form when `freq_he.tsv` has data.
- **Real words** (`real_words.txt`, or Zipf ≥ 3 once frequencies are loaded) are corrected only
  when the keyword fills a role nothing else fills. "תחפש לי קצבים" has no object, so it reads
  קבצים; "מייל על קצבים" already has an object, so קצבים stays a search word.
- **Time words** never take the article ה ("המחיר" is not ה + מחר). A day word is not corrected
  next to another day word.
- **Verbs** are corrected only for a cheap slip. Open and launch verbs are never corrected.
- **A misspelt write verb** becomes a refusal ("תמוחק"), which fails safe, unless another
  correction is about as cheap.

## 4. People, mailboxes, dates

**People and names:**
- The person is found from the calendar-question patterns: "מה יש לX", "היומן של X", "אצל X",
  "מה X עושה", "האם X פנוי".
- Senders come from "מX", "מאת X", "המייל של X", "מה X שלח לי", "X ענה לי" and "שקיבלתי מX".
  For mail, "ההתכתבות עם X" also means the sender X.
- Full names and rooms are completed against the calendars and people the machine knows
  ("דנה כהן", "בחדר ישיבות").
- A misspelt first name is fixed only towards a known name ("יבול" → "יובל").
- Two people in one question ask whose calendar is meant.

**Mailboxes:** matched against the real mailbox names only. One distinctive word after a cue is
enough: "בתיבת התמיכה" → "תמיכה טכנית", "המיילים של מכירות". One cheap slip is forgiven
("מכירת"). "התיבה המשותפת" without a name sets `shared_mailbox`.

**Dates:**
- Weeks run Sunday to Saturday.
- A bare weekday or "dd.mm" is the nearest one from today on. For mail, files and notes it is
  read as the past one instead.
- "השבוע / החודש האחרון" is the last 7 / 30 days.
- An impossible date asks again.
- A bare hour from 1 to 6 asks whether it is morning or afternoon.

## 5. Confidence and decisions

Each capability's score comes from features: the object noun, the verb, the question words and
the slots found.

| decision | when |
|---|---|
| Execute | A read-only capability scores at least `T_EXEC` (2.0) with a clear margin. |
| MultiSource | Two or three read-only searches are within the margin. |
| Clarify | A required slot is missing or ambiguous: date, hour, person, what to search, or which request. The question names the likely capability, so the island can say "באילו קבצים לחפש?". |
| Confirm | Open / launch. Never run from text: candidates are shown for a click. |
| NoMatch | Out of scope or nothing recognisable. `Analysis.unsupported` says why: `write`, `weather`, `news`, `translate`, `install` or `power`. |

**`Analysis`** has these fields:
- `sources`
- `score`, `margin`
- `corrections` (concept, cost, real word)
- `reasons` (codes such as `noun:mail`, `verb:search`, `time`, `sender`, `phrase:N_CAL`,
  `follow-up`)
- `exact_terms`
- `shared_mailbox`
- `unsupported`

It holds no user text, so it can be logged.

**Exact words:** "המילה X", "בדיוק X" or a quoted X keep the word as typed and set
`exact_terms`. The executor ranks whole-word matches first (requested from the assistant owner).
No stem or synonym widening is applied: "חושב" stays חושב.

## 6. Word frequencies and wordfreq

| file | holds | licence |
|---|---|---|
| `lexicon/freq_he.tsv` | The 48,448 words of Hebrew letters among the 50,000 most frequent of wordfreq 3.2.0's `large_he` list (Robyn Speer), as Zipf × 100 | CC BY-SA 4.0 |
| `lexicon/freq_he.LICENSE.txt` | The credit and the sources | — |
| `lexicon/office_words.txt` | Hand-written office words ("חשבונית", "פרוטוקול", …) that general text rarely uses | ours, no notice needed |

wordfreq's code is Apache-2.0. Its data may be redistributed under CC BY-SA 4.0 with credit to
Robyn Speer and the sources (Wikipedia, OpenSubtitles, Google Books Ngrams, OSCAR, Twitter
counts). The data is frozen at about 2021. It gives frequencies only: no synonyms, lemmas or
sentence understanding.

**What it is used for:**
- **Real words.** A typed word with Zipf ≥ 3 is a real word. A keyword correction of it needs the
  sentence's support (see 3).
- **Ties.** The more frequent candidate wins.
- **Misspelt search words.** A Hebrew search word of 4 or more letters that is not listed (not
  even without its proclitics) gets the closest common spelling(s) as OR alternatives:
  - "ביתוח" → ביתוח | ביטוח
  - "חשבונת" → חשבונת | חשבונות | חשבונית

  The rules for these alternatives:
  - The candidates are words with Zipf ≥ 3.5, plus the office list.
  - The budget is 0.75 for 4–5 letters and 1.0 for longer words.
  - At most two alternatives, all at the cheapest cost.
  - Never for an exact word, a quoted phrase or a known name.
  - The word as typed is always searched too.

**Credit policy (the user's decision, 2026-10-09):** the credit lives in the code and in a text
file in the install folder (`freq_he.LICENSE.txt`, copied by the installer). It never appears in
the UI: no window and no About page.

**Size:** about 760 KB of text in the binary. The tables are built on the first question, or at
start-up off the UI thread by the assistant's warm-up, and take a few MB of memory.

## 7. Weak machines and closed networks

**Constraints:**
- Target: Windows 10 21H2, 8 GB RAM, no internet.
- No new crates, no Python, no services.
- The lexicon is about 1,000 forms. It is built lazily on the first question: about 6 ms on a
  debug build, and a few hundred KB of memory.
- Analysis runs inside the async `assistant_submit` command (`run_query` worker), never on the
  UI thread.

**Measured** (one thread):

| build | per question | 20,000 questions | lexicon build |
|---|---|---|---|
| release | p50 0.05 ms, p95 0.21 ms, p99 0.34 ms | 1.4 s | 1.3 ms |
| debug | p50 0.25 ms, p95 1.6–1.9 ms | 9–10 s | ~6 ms |

On the eval sets, release takes p50 0.07–0.10 ms and p95 0.26–0.43 ms per question, including
follow-up set-up.

## 8. Evaluation

| file | cases | role |
|---|---|---|
| `eval/must.jsonl` | 22 | The sentences of the request, the "תבדוק את מה יש ביומן של איציק" complaint, and guard cases. Must pass. |
| `eval/dev.jsonl` | 300 | Tuning set, author 1. |
| `eval/dev2.jsonl` | 300 | Tuning set, author 3. Hand-written, with weight on typos, synonyms and exact words. |
| `eval/test.jsonl` | 300 | Held out, author 2. Written from `eval/GUIDE.md` only. |
| `corpus.jsonl`, `fresh.jsonl` | 275 + 40 | The earlier gates: execute precision ≥ 98 %, zero sensitive executes. |

Run `cargo test --lib intent::eval -- --nocapture` for the report:
- success
- intent accuracy
- slot accuracy (cases and slots)
- execute precision and recall
- false clarifications
- under-clarification
- latency
- success per tag

| set | old engine (8a29212) | now |
|---|---|---|
| test, blind (frozen at 2111b51) | 52.3 % | **80.7 %** (82.0 % with one label fix and the notes amendment) |
| test after the audit (not blind) | – | 85.0 % (with the frequency data) |
| dev | 55.7 % | 96.7 % |
| dev2 (not tuned on at first: 70.0 %) | – | 90.0 % |
| must | 68.2 % | 100 % |

The initial target of 90 % on a representative test set of everyday requests is **not reached**:
the blind test measures 80.7 %.

**Remaining errors, largest first:**
1. Spelling mistakes inside search words: partly fixed by the frequency data of 6.
2. Questions with two people.
3. Number-only and calculator follow-ups.
4. Availability with no date.
5. Loanword synonyms.

Any further tuning should be measured on a new blind set, because the current test set has now
been read.

## 9. Integration notes for the assistant owner

- **Exact words:** `Analysis.exact_terms`, and later `Slots::exact_terms`. Order whole-word hits
  first; never widen.
- **Shared mailbox:** `Analysis.shared_mailbox`, and later `Slots::shared_mailbox`. Search only
  the shared mailboxes, or ask which one.
- **Refusals:** `Analysis.unsupported`. "write" says "I can only search and read". Weather and
  news say "I can't check that offline" and keep the Google button.
- **Warm-up (optional):** warm the lexicon off the UI thread after start-up, so the first question
  skips the build.
