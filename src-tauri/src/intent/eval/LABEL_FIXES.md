# Test set: label fixes and the audit

`test.jsonl` was written by a separate author from `GUIDE.md` alone. It was not used for tuning
while the engine was developed on `dev.jsonl` and `dev2.jsonl`. Its misses were first read on
2026-10-08, after commit 2111b51.

## Frozen result (before the audit)

Commit 2111b51, labels as written: **242 / 300 = 80.7 %** success. Success means the decision,
the capability and every expected slot are right.

| measure | value |
|---|---|
| intent accuracy | 90.3 % (271 / 300) |
| slot accuracy (cases) | 87.1 % (195 / 224) |
| slot accuracy (slots) | 92.5 % (380 / 411) |
| execute precision | 80.3 % |
| execute recall | 80.7 % |
| false clarification | 1.8 % (4 / 228) |
| under-clarification | 17.5 % (11 / 63) |
| sensitive executes | 0 |

For comparison, the engine before this work (8a29212) scored 52.3 % (157 / 300) on the same labels.

## Label fixes

A label is changed only when it contradicts `GUIDE.md`.

| id | text | was | now | rule |
|---|---|---|---|---|
| t043 | פגישות ביום ראשון הבא | date 2026-10-18 | date 2026-10-11 | GUIDE: a weekday with "הבא" is that day of next week (2026-10-11..17) |

## Guide amendment (decided on dev, before the test misses were read)

Listing notes with no filter is `exec` / `notes.search`. Three test labels say `clarify` for this:
t228, t239 and t244. They are left as written and counted here.

**Frozen result with the fix and the amendment: 246 / 300 = 82.0 %.**

## Changes made after the audit (the test set is no longer blind for these)

1. **Misspelt write verbs.** "תבתל", "תמוחק" and "להזמין" fell through to a read-only search
   instead of a refusal. Nothing was changed by it, since searches only read, but the user got no
   "I can't do that".

   Fix: a misspelt write verb is now corrected to the refusal, which fails safe. A different
   correction that is almost as cheap wins over the refusal. "להזמין" was added.
2. **"שער" removed from the refusal words.** It also means a gate ("קוד של שער").
3. **"תסנן" is now a narrowing word,** like "רק". Found on dev after change 1.

**Score after these changes (labels fixed, amendment not applied): 247 / 300 = 82.3 %.** With the
amendment: 250 / 300 = 83.3 %. These numbers are no longer an unbiased estimate. A new blind set
is needed for that.

## What the remaining misses are (engine errors, labels kept)

**Spelling mistakes inside search words** (about 11 cases): "ביתוח", "התקצב", "חשבונת", "הזמנוט",
"פרוטוקל". The engine corrects keywords only, against its own lexicon. Correcting arbitrary
Hebrew words needs a general dictionary with frequencies, such as wordfreq. That is waiting for
licence approval (see `docs/HEBREW_ENGINE.md`). Even then, a correction would be added as an OR
alternative and never replace the word as typed.

**Other misses:**
- availability with no date ("אני פנוי?"): the engine answers for today, the guide asks for a date;
- two people in one question written with "של X ושל Y";
- follow-ups of a calculation ("ועכשיו תחלק ב-4");
- follow-ups that give only a number ("תן לי 10");
- "installed apps" questions;
- loanword synonyms ("פרזנטציה");
- typos in time words that are also real words ("בשבט", "שלשוום", "מחך");
- a meeting noun used as a file topic without a proclitic ("פרוטוקול ישיבה").

## After the frequency data (2026-10-09, not blind)

The wordfreq frequencies and the office word list (`docs/HEBREW_ENGINE.md` §6) add common spellings
of misspelt search words as alternatives. Test: **255 / 300 = 85.0 %**.
