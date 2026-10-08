# Hebrew evaluation set: labelling guide

The evaluation set measures how well the offline intent engine (`src-tauri/src/intent`) understands
free Hebrew. It has two halves:

- `dev.jsonl`: the engine may be tuned on it.
- `test.jsonl`: written by a different author from this guide alone. It is never used for
  tuning. When a test case fails, the only allowed fix is in the label, and only when the label
  breaks this guide. Each such label fix is recorded in `LABEL_FIXES.md`.

Authors write sentences the way real office workers type into a search box: short, in natural
Hebrew, sometimes careless. Do not copy the engine's examples.

## Fixed world

**Clock:** Thursday 2026-10-08, 09:00 local time.

**Weeks** run Sunday to Saturday:
- This week: 2026-10-04 to 2026-10-10.
- Last week: 2026-09-27 to 2026-10-03.
- Next week: 2026-10-11 to 2026-10-17.

**Mailboxes the user has** (id: name):
- `mb1`: "Yuval Cohen" (own)
- `mb2`: "מכירות" (shared)
- `mb3`: "תמיכה טכנית" (shared)

**Calendars the user can read** (id: name):
- `cal1`: "איציק לוי"
- `cal2`: "דנה כהן"
- `cal3`: "חדר ישיבות"
- `cal4`: "משה פרץ"

**People seen recently:** "יובל כהן", "דני רוזן", "שרון לוי", "מירי אברהם".

## What the engine can do (capabilities)

All of these only read. Opening or launching is never done from text alone; the user must click.

| cap | meaning |
|---|---|
| `calendar.list_events` | What is in my / X's calendar for a day or range ("מה יש לי מחר", "מה יש לאיציק ביומן") |
| `calendar.check_availability` | Am I / is X free or busy ("אני פנוי ב...", "דנה תפוסה מחר?") |
| `calendar.search_events` | Find meetings by topic or attendee ("מתי הפגישה עם דנה", "פגישות על התקציב") |
| `calendar.resolve_shared_calendar` | Which shared calendars do I have |
| `email.search` | Find mail by sender, words, date, unread, latest, mailbox |
| `email.discover_mailboxes` | Which mailboxes do I have |
| `files.search` | Find files and documents on this PC |
| `notes.search` | Find the user's notes (פתקים) |
| `apps.search` | Which apps exist ("איזה אפליקציות יש לי") |
| `calculator.evaluate` | Arithmetic |
| `email.open`, `files.open`, `notes.open`, `apps.launch` | Open or launch. The decision is always `confirm`, never `exec` |

**Out of scope** (decision `nomatch`):
- Anything that changes data: delete, send, reply, forward, move, cancel, postpone, edit, rename,
  create a meeting, book.
- Weather, news, translation, music, ordering things, installing.

**Do not write** sentences about the following. Another component owns them and they are not
measured here:
- web or Google search, or opening websites;
- Windows settings, opening folders, locking the PC;
- composing a new mail or creating a new note;
- Sticky Notes.

## Decisions

| `decision` | when |
|---|---|
| `exec` | One read-only capability clearly fits and it has what it needs. |
| `multi` | A search with words but no object noun at all ("תחפש את החשבונית"): `caps` = `["email.search","files.search","notes.search"]`, in this order. |
| `clarify` | The request is understood but something needed is missing or ambiguous. Give `ask` (see below) and `cap` when one capability is clear. |
| `confirm` | Open or launch something specific. Give `cap`. |
| `nomatch` | Out of scope, unsafe, or meaningless. |

**`ask` values:**
- `date`: a calendar question with no date and no calendar word. "מה יש לי?" alone is
  `clarify`/`date`.
- `time`: a bare hour 1–6 with no morning or afternoon marker ("ב-3").
- `person`: two people at once ("מה יש ליובל ולדנה"), or a first name that matches two calendars.
- `content`: mail, files or notes with nothing to search for. "תחפש לי קבצים" is
  `clarify`/`content`/`files.search`. The same goes for "דואר נכנס" with no sender, word, date,
  unread or latest.
- `intent`: a follow-up with no previous turn, or a bare date or name with nothing to attach it
  to.

**Rules:**
- A calendar question that has a calendar word (יומן, לו"ז, פגישות, אירועים) or a person, but no
  date, defaults to today. Give `date` `2026-10-08`.
- A mail search runs (`exec`) when it has at least one constraint: sender, words, a date or time,
  unread, latest or newest, or a mailbox. "כל המיילים" alone is `clarify`/`content`.
- "הודעה/הודעות" in an office search means mail.
- "קבעתי / מתוכנן / יש לי משהו" are calendar questions.
- The person in "מה יש לX", "היומן של X", "מה X עושה", "האם X פנוי" goes in `person`, not
  `sender`.

## Slots

Write only the slots that matter for the sentence. The checker compares only what you list.

| key | value |
|---|---|
| `date` | "YYYY-MM-DD", for one whole day |
| `from` | Start, "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM" (prefix match) |
| `to` | End, exclusive, same format |
| `person` | Calendar owner, as typed, without the proclitic: "לאיציק" → "איציק" |
| `sender` | Mail sender, as typed, without the proclitic: "מיובל" → "יובל". For mail, "ההתכתבות עם דני" and "מיילים עם דני" also put "דני" in `sender`. |
| `terms` | Search words: a list of groups, each a list with ONE word. Use the bare form without ה/ב/ל/ו/מ/ש: "התקציב" → `[["תקציב"]]`. List every content word the search must contain and no grammar words. A file type word (אקסל, PDF, מצגת) is not a term; use `ext` for it. |
| `exact` | `true` when the user asks for an exact word ("המילה X", "בדיוק X", or X in quotes) |
| `mailbox` | Mailbox id when a mailbox is named ("בתיבה של מכירות" → "mb2") |
| `all_mailboxes` | `true` for "בכל התיבות" |
| `shared_mailbox` | `true` for "התיבה המשותפת" or "תיבות משותפות" without a name |
| `unread` | `true` for "שלא קראתי", "שלא נקראו" |
| `latest` | `true` for "האחרון / האחרונים / החדש ביותר" |
| `limit` | A number: "5 המיילים האחרונים" → 5. A singular "האחרון" → 1. |
| `ext` | "xlsx" (אקסל), "docx" (וורד), "pptx" (מצגת), "pdf", "csv", "txt" |
| `app` | The app name for `apps.launch`, as typed |
| `value` | The numeric result of a calculation |
| `follow_up` | `true` for a case with `ctx` that continues the previous turn |

**Dates and times:**
- היום = 2026-10-08, מחר = 10-09, מחרתיים = 10-10, אתמול = 10-07, שלשום = 10-06.
- A bare weekday is the nearest one from today, today included. "חמישי" = 10-08, "ראשון" = 10-11,
  "שני" = 10-12. With "הבא" it is next week's day: "חמישי הבא" = 10-15. With "שעבר" it is the last
  one before today: "שני שעבר" = 10-05.
- השבוע = from 2026-10-04 to 2026-10-11. שבוע הבא = from 10-11 to 10-18. שבוע שעבר = from 09-27
  to 10-04.
- "השבוע האחרון", "בשבוע האחרון", "7 הימים האחרונים" mean the last 7 days: `from` "2026-10-01".
- Parts of the day:
  - בבוקר = 06:00–12:00
  - בצהריים = 12:00–14:00
  - אחה"צ = 12:00–18:00
  - בערב = 18:00–24:00
- An hour 7–12 is taken as written, 13–23 is 24-hour time, and an explicit hour is a one-hour
  window. "מחר ב-10" → `from` "2026-10-09T10:00".
- "dd.mm" or "dd/mm" is the nearest such date from today, so 15.10 = 2026-10-15. An impossible
  date (31.2) is `clarify`/`date`.

## Follow-ups

A follow-up case has `"ctx": "<id of the earlier case>"`. The earlier case runs first and its
answer is remembered, then this text runs 10 seconds later. The earlier case must be in the same
file. Label the follow-up with what the combined request means, plus `"follow_up": true`.

Examples:
- After "מה יש לי היום?", the text "ומה לגבי מחר?" → `exec` `calendar.list_events`, `date`
  2026-10-09.
- After "תחפש מיילים מיובל", the text "רק מהשבוע שעבר" → `exec` `email.search`, `sender` "יובל",
  `from` "2026-09-27".

A follow-up with no `ctx` ("ומה לגבי מחר?" alone) is `clarify`/`intent`.

## Line format (JSONL, one case per line)

```json
{"id":"t001","text":"...","expect":{"decision":"exec","cap":"email.search","slots":{"sender":"יובל"}},"tags":["he","mail","sender"]}
```

- Ids are `d###` for dev and `t###` for test.
- `ctx` is optional.

**Tags** (use every one that applies):
- `he`, `typo`, `slang`, `synonym`, `inflection`, `indirect`, `follow-up`, `ambiguous`,
  `person`, `multi-mailbox`, `shared-calendar`, `unsupported`, `multi-condition`, `niqqud`,
  `abbrev`, `exact-word`, `relative-date`;
- plus a domain tag: `calendar`, `mail`, `files`, `notes`, `apps`, `calc` or `other`.

## Quotas per file (300 cases)

| cases | category |
|---|---|
| 60 | calendar: own day, range and part of day; other people's calendars; availability; meeting search |
| 60 | mail: sender, words, exact word, date, unread, latest, mailbox, several conditions |
| 35 | files: topic, type (אקסל/PDF/מצגת/וורד), time, "איפה שמרתי", typos such as "קצבים" for "קבצים" |
| 25 | notes |
| 25 | follow-ups and refinements (with `ctx`) |
| 25 | unclear requests that should ask (`clarify`) |
| 25 | unsupported (`nomatch`): write actions phrased many ways, weather, news, ... |
| 15 | open or launch (`confirm`) and apps |
| 10 | calculator |
| 20 | spread over everything: heavy slang, niqqud, missing spaces, two typos, gershayim (לו״ז, דוא״ל, אחה״צ) |

**At least:**
- 60 cases with a spelling mistake;
- 40 with slang or colloquial phrasing;
- 40 with indirect questions ("יש מצב ש...", "אפשר לדעת אם...", "תבדוק לי בבקשה מה...");
- 30 with several conditions.

Hebrew only, except a few mixed cases where a person or file name is in English.

## Amendments (made after `test.jsonl` was written)

- 2026-10-08: listing notes with no filter ("הפתקים שלי", "תראה לי פתקים") is `exec` /
  `notes.search`: there are few notes, so they are listed instead of asking. Files and mail with no
  filter still ask (`clarify` / `content`). Test cases that this amendment changes are counted in
  `LABEL_FIXES.md`. Their labels in `test.jsonl` are left as written.
