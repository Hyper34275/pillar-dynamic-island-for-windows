//! Talking to the assistant itself: "מה אתה יודע לעשות" / "help", "שלום", "תודה", "מי אתה" (see
//! `intent::commands`, which recognises them). The answers are fixed text: they touch no data source, run
//! no search and open nothing, so the cards are plain `Execute` answers with no click.
//!
//! Wire: the same shapes as any answer. The help card has one `ItemKind::Info` item (not openable) per
//! example, in groups of the kinds that exist already ("calendar", "mail", "files", "notes", "apps",
//! "action", "calc"), so the Center draws each ability under its own heading and glyph. The groups are
//! taken in turn (`Outcome::interleave`), which makes the three rows of the island the first example of
//! calendar, mail and files; the Center lists everything.

use super::exec::{Group, Outcome, Run};
use super::wire::{AssistantItem, ItemKind};
use crate::intent::{caps, CapId, Lang};

/// The capabilities this file answers.
pub fn handles(cap: CapId) -> bool {
    caps::is_talk(cap)
}

/// What a talk capability says.
pub fn build(r: &Run, cap: CapId) -> Outcome {
    let he = r.lang == Lang::He;
    match cap {
        caps::ASSISTANT_HELP => help(he),
        caps::ASSISTANT_HELLO => Outcome::answer(if he { "היי! אפשר לשאול אותי למשל 'מה יש לי היום?'" } else { "Hi! You can ask me things like 'What do I have today?'" }, ""),
        caps::ASSISTANT_THANKS => Outcome::answer(if he { "בכיף!" } else { "You're welcome!" }, ""),
        caps::ASSISTANT_ABOUT => Outcome::answer(
            if he {
                "אני יובל, העוזר של המחשב הזה. אני מחפש ביומן, במיילים, בפתקים ובקבצים, ופותח דברים בשבילך."
            } else {
                "I'm Yuval, this PC's assistant. I search your calendar, mail, notes and files, and open things for you."
            },
            "",
        ),
        _ => Outcome::answer(if he { "אפשר לשאול אותי למשל 'מה יש לי היום?'" } else { "You can ask me things like 'What do I have today?'" }, ""),
    }
}

/// One ability: the group it belongs to, and its examples as (what to type, what it does).
struct Ability {
    kind: &'static str,
    title: (&'static str, &'static str),
    examples: &'static [((&'static str, &'static str), (&'static str, &'static str))],
}

/// In the order of the card. The first example of the first three abilities is what the island shows.
/// Every example is a sentence the engine understands (a test runs them all).
const ABILITIES: [Ability; 7] = [
    Ability {
        kind: "calendar",
        title: ("יומן", "Calendar"),
        examples: &[
            (("מה יש לי מחר?", "What do I have tomorrow?"), ("הפגישות שלך", "Your meetings")),
            (("מה יש ליומן של איציק השבוע?", "What's on Itzik's calendar this week?"), ("היומן של עמית", "A colleague's calendar")),
        ],
    },
    Ability { kind: "mail", title: ("מיילים", "Mail"), examples: &[(("המייל האחרון מדני", "The latest email from Dani"), ("חיפוש במיילים", "Search your mail"))] },
    Ability { kind: "files", title: ("קבצים", "Files"), examples: &[(("תמצא את המצגת של הכנס", "Find the conference presentation"), ("חיפוש קבצים", "Search your files"))] },
    Ability {
        kind: "notes",
        title: ("פתקים", "Notes"),
        examples: &[
            (("מה רשמתי על התקציב?", "What did I note about the budget?"), ("חיפוש בפתקים, כולל Sticky Notes", "Search your notes, Sticky Notes included")),
            (("תרשום פתק: לקנות חלב", "Write a note: buy milk"), ("פתק חדש", "A new note")),
        ],
    },
    Ability { kind: "apps", title: ("אפליקציות", "Apps"), examples: &[(("פתח אקסל", "Open Excel"), ("פתיחת אפליקציות", "Open apps"))] },
    Ability {
        kind: "action",
        title: ("גוגל, אתרים והגדרות", "Google, websites and settings"),
        examples: &[
            (("תחפש בגוגל מחיר דולר", "Search Google for dollar rate"), ("חיפוש בגוגל", "Search the web")),
            (("תפתח את ynet", "Open ynet"), ("פתיחת אתרים", "Open websites")),
            (("פתח הגדרות wifi", "Open wifi settings"), ("הגדרות Windows", "Windows settings")),
        ],
    },
    Ability { kind: "calc", title: ("מחשבון", "Calculator"), examples: &[(("כמה זה 15% מ-240", "How much is 15% of 240"), ("חישובים", "Calculations"))] },
];

/// The examples of the help card in one language, as typed: the same sentences the card lists.
#[cfg(test)]
pub fn example_texts(lang: Lang) -> Vec<&'static str> {
    ABILITIES.iter().flat_map(|a| a.examples.iter().map(move |e| if lang == Lang::He { e.0 .0 } else { e.0 .1 })).collect()
}

fn pick(pair: (&'static str, &'static str), he: bool) -> &'static str {
    if he {
        pair.0
    } else {
        pair.1
    }
}

fn info(title: &str, subtitle: &str) -> AssistantItem {
    AssistantItem {
        id: String::new(),
        kind: ItemKind::Info,
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        time: None,
        end_time: None,
        accent: None,
        openable: false,
        unread: false,
        source: None,
    }
}

fn help(he: bool) -> Outcome {
    let mut o = Outcome::answer(
        if he { "הנה מה שאני יודע לעשות" } else { "Here's what I can do" },
        if he { "אפשר לשאול בעברית או באנגלית, כמו בדוגמאות" } else { "Ask in Hebrew or English, like the examples" },
    );
    for a in &ABILITIES {
        let items = a.examples.iter().map(|(example, what)| (info(pick(*example, he), pick(*what, he)), super::store::Target::None)).collect();
        o.groups.push(Group { kind: a.kind, title: pick(a.title, he).to_string(), mailbox: None, items, truncated: false, error_code: None });
    }
    o.interleave = true;
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::exec::Sources;
    use crate::assistant::flow::Engine;
    use crate::assistant::prefs::Prefs;
    use crate::assistant::wire::{AssistantCard, CardPhase};
    use crate::calendar::{CalendarSourceDto, RangeRead};
    use crate::intent::{interpret, Ctx, Decision, Known};
    use crate::local::{AppHit, FileSearch, NoteHit};
    use crate::outlook_mail::{FreeBusy, MailCursor, MailQuery, MailSearchResult, MailboxInfo};
    use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Sources that count every call: the talk answers must not use any of them.
    #[derive(Default)]
    struct Quiet {
        calls: AtomicUsize,
    }

    impl Quiet {
        fn touched(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
        fn hit(&self) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Sources for Quiet {
        fn cached_mailboxes(&self) -> Vec<MailboxInfo> {
            Vec::new()
        }
        fn discover_mailboxes(&self, _force: bool) -> Result<Vec<MailboxInfo>, String> {
            self.hit();
            Err("OUTLOOK-101: not running".into())
        }
        fn search_mail(&self, _q: &MailQuery, _c: Option<MailCursor>) -> Result<MailSearchResult, String> {
            self.hit();
            Ok(MailSearchResult::default())
        }
        fn open_mail(&self, _key: &str) -> Result<(), String> {
            self.hit();
            Err("x".into())
        }
        fn free_busy(&self, _n: &str, _f: DateTime<Utc>, _t: DateTime<Utc>) -> Result<FreeBusy, String> {
            self.hit();
            Err("x".into())
        }
        fn calendars(&self) -> Vec<CalendarSourceDto> {
            Vec::new()
        }
        fn people(&self) -> Vec<String> {
            Vec::new()
        }
        fn query_range(&self, _f: DateTime<Utc>, _t: DateTime<Utc>, _o: Option<Vec<String>>) -> Result<RangeRead, String> {
            self.hit();
            Err("x".into())
        }
        fn prefetched(&self, _f: DateTime<Utc>, _t: DateTime<Utc>, _o: Option<&[String]>) -> Option<RangeRead> {
            self.hit();
            None
        }
        fn open_event(&self, _s: DateTime<Utc>) -> Result<(), String> {
            self.hit();
            Err("x".into())
        }
        fn search_files(&self, _t: &[Vec<String>], _e: Option<&str>, _l: usize, _b: u64) -> Result<FileSearch, String> {
            self.hit();
            Ok(FileSearch::default())
        }
        fn open_file(&self, _k: &str) -> Result<(), String> {
            self.hit();
            Err("x".into())
        }
        fn search_apps(&self, _n: &[String], _l: usize) -> Result<Vec<AppHit>, String> {
            self.hit();
            Ok(Vec::new())
        }
        fn launch_app(&self, _k: &str) -> Result<(), String> {
            self.hit();
            Err("x".into())
        }
        fn search_notes(&self, _t: &[Vec<String>], _l: bool, _n: usize) -> Result<Vec<NoteHit>, String> {
            self.hit();
            Ok(Vec::new())
        }
        fn open_note(&self, _id: &str) -> Result<(), String> {
            self.hit();
            Err("x".into())
        }
        fn load_prefs(&self) -> Option<Prefs> {
            None
        }
        fn save_prefs(&self, _p: &Prefs) -> Result<(), String> {
            self.hit();
            Ok(())
        }
    }

    fn at() -> DateTime<Local> {
        let ndt = NaiveDateTime::parse_from_str("2026-10-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        Local.from_local_datetime(&ndt).earliest().unwrap()
    }

    fn ask(engine: &Engine, src: &Quiet, text: &str) -> AssistantCard {
        engine.submit(src, text, "q0000000000000001", at())
    }

    #[test]
    fn the_help_card_lists_one_example_per_ability() {
        let (engine, src) = (Engine::new(), Quiet::default());
        let c = ask(&engine, &src, "מה אתה יודע לעשות");
        assert_eq!(c.phase, CardPhase::Answer);
        assert_eq!(c.title, "הנה מה שאני יודע לעשות");
        assert!(c.question.is_none() && c.choices.is_empty() && !c.partial && !c.can_extend && c.error_code.is_none());
        // the island shows three rows: the most useful abilities first
        let island: Vec<&str> = c.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(island, ["מה יש לי מחר?", "המייל האחרון מדני", "תמצא את המצגת של הכנס"]);
        // the Center lists all of them
        let all = engine.results(&c.query_id, at().timestamp_millis()).unwrap();
        let titles: Vec<&str> = all.groups.iter().flat_map(|g| g.items.iter().map(|i| i.title.as_str())).collect();
        assert_eq!(c.total as usize, titles.len());
        assert_eq!(titles.len(), 11);
        for example in [
            "מה יש לי מחר?",
            "מה יש ליומן של איציק השבוע?",
            "המייל האחרון מדני",
            "מה רשמתי על התקציב?",
            "תמצא את המצגת של הכנס",
            "פתח אקסל",
            "תחפש בגוגל מחיר דולר",
            "תפתח את ynet",
            "פתח הגדרות wifi",
            "כמה זה 15% מ-240",
            "תרשום פתק: לקנות חלב",
        ] {
            assert!(titles.contains(&example), "{example} is not on the card: {titles:?}");
        }
        // information only: nothing to click, nothing to open
        assert!(all.groups.iter().flat_map(|g| &g.items).all(|i| i.kind == ItemKind::Info && !i.openable && !i.unread && i.subtitle.is_some() && i.time.is_none()));
        assert_eq!(src.touched(), 0, "help must not read any source");
    }

    #[test]
    fn the_help_card_uses_only_kinds_that_exist_and_the_old_wire_shape() {
        let (engine, src) = (Engine::new(), Quiet::default());
        let c = ask(&engine, &src, "what can you do");
        assert_eq!(c.title, "Here's what I can do");
        let all = engine.results(&c.query_id, at().timestamp_millis()).unwrap();
        let kinds: Vec<&str> = all.groups.iter().map(|g| g.kind.as_str()).collect();
        assert_eq!(kinds, ["calendar", "mail", "files", "notes", "apps", "action", "calc"]);
        assert!(all.groups.iter().all(|g| !g.title.is_empty() && !g.truncated && g.error_code.is_none() && g.mailbox.is_none()));
        // exactly the old item and card fields: nothing was added to the wire
        let item = serde_json::to_value(&c.items[0]).unwrap();
        let mut keys: Vec<&str> = item.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["accent", "endTime", "id", "kind", "openable", "source", "subtitle", "time", "title", "unread"]);
        assert_eq!(item["kind"], "info");
        assert_eq!(item["openable"], false);
        let card = serde_json::to_value(&c).unwrap();
        let mut card_keys: Vec<&str> = card.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        card_keys.sort();
        assert_eq!(
            card_keys,
            vec!["canExtend", "choices", "createdAt", "errorCode", "followUp", "items", "lang", "partial", "phase", "query", "queryId", "question", "sources", "summary", "title", "total"]
        );
        // the island's wire reader keeps at most 8 source names
        assert!(c.sources.len() <= 8, "{:?}", c.sources);
        // the Center reads the group kind as a string, and its heading is the title sent
        let json = serde_json::to_value(&all.groups[0]).unwrap();
        assert_eq!(json["kind"], "calendar");
        assert_eq!(json["title"], "Calendar");
        assert_eq!(c.items.len(), 3);
        assert_eq!(c.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["What do I have tomorrow?", "The latest email from Dani", "Find the conference presentation"]);
    }

    #[test]
    fn nothing_on_the_help_card_can_be_opened() {
        let (engine, src) = (Engine::new(), Quiet::default());
        let c = ask(&engine, &src, "עזרה");
        let all = engine.results(&c.query_id, at().timestamp_millis()).unwrap();
        for item in all.groups.iter().flat_map(|g| &g.items) {
            assert!(engine.open(&src, &c.query_id, &item.id, at().timestamp_millis()).is_err(), "{}", item.title);
        }
        assert_eq!(src.touched(), 0);
    }

    #[test]
    fn every_example_on_the_card_is_a_sentence_the_engine_understands() {
        let known = Known::default();
        for lang in [Lang::He, Lang::En] {
            for text in example_texts(lang) {
                let i = interpret(text, &Ctx::default(), at(), &known);
                let ok = matches!(i.decision, Decision::Execute { .. } | Decision::MultiSource { .. } | Decision::Confirm { .. });
                assert!(ok, "{text:?}: {:?}", i.decision);
                // and none of them is itself a help or small-talk phrase
                assert!(!matches!(i.decision, Decision::Execute { cap } if caps::is_talk(cap)), "{text:?}");
            }
        }
    }

    #[test]
    fn small_talk_is_one_short_read_only_line() {
        let cases: [(&str, &str); 14] = [
            ("שלום", "היי! אפשר לשאול אותי למשל 'מה יש לי היום?'"),
            ("היי", "היי! אפשר לשאול אותי למשל 'מה יש לי היום?'"),
            ("הי", "היי! אפשר לשאול אותי למשל 'מה יש לי היום?'"),
            ("בוקר טוב", "היי! אפשר לשאול אותי למשל 'מה יש לי היום?'"),
            ("ערב טוב", "היי! אפשר לשאול אותי למשל 'מה יש לי היום?'"),
            ("hi", "Hi! You can ask me things like 'What do I have today?'"),
            ("hello", "Hi! You can ask me things like 'What do I have today?'"),
            ("תודה", "בכיף!"),
            ("תודה רבה", "בכיף!"),
            ("thanks", "You're welcome!"),
            ("thank you", "You're welcome!"),
            ("מי אתה", "אני יובל, העוזר של המחשב הזה. אני מחפש ביומן, במיילים, בפתקים ובקבצים, ופותח דברים בשבילך."),
            ("מי אתה?", "אני יובל, העוזר של המחשב הזה. אני מחפש ביומן, במיילים, בפתקים ובקבצים, ופותח דברים בשבילך."),
            ("what are you", "I'm Yuval, this PC's assistant. I search your calendar, mail, notes and files, and open things for you."),
        ];
        for (text, title) in cases {
            let (engine, src) = (Engine::new(), Quiet::default());
            let c = ask(&engine, &src, text);
            assert_eq!(c.phase, CardPhase::Answer, "{text}");
            assert_eq!(c.title, title, "{text}");
            assert!(c.summary.is_empty() && c.items.is_empty() && c.total == 0 && c.choices.is_empty() && c.question.is_none(), "{text}: {c:?}");
            assert!(c.sources.is_empty() && !c.partial && !c.can_extend && c.error_code.is_none(), "{text}");
            assert_eq!(src.touched(), 0, "{text}: small talk must not read any source");
            // a sentence, not a page: one line
            assert!(!c.title.contains('\n') && c.title.chars().count() < 120, "{text}");
        }
    }

    #[test]
    fn who_are_you_names_the_product() {
        let (engine, src) = (Engine::new(), Quiet::default());
        for text in ["מי אתה", "מי את", "who are you", "what are you"] {
            let c = ask(&engine, &src, text);
            assert!(c.title.contains("יובל") || c.title.contains("Yuval"), "{text}: {}", c.title);
            assert!(c.title.contains("יומן") || c.title.contains("calendar"), "{text}: {}", c.title);
        }
    }

    #[test]
    fn the_language_of_the_answer_is_the_language_of_the_question() {
        let (engine, src) = (Engine::new(), Quiet::default());
        assert!(ask(&engine, &src, "help").title.starts_with("Here's"));
        assert!(ask(&engine, &src, "עזרה").title.starts_with("הנה"));
        assert_eq!(ask(&engine, &src, "thanks").title, "You're welcome!");
        assert_eq!(ask(&engine, &src, "תודה").title, "בכיף!");
        assert_eq!(ask(&engine, &src, "hello").lang, Lang::En);
        assert_eq!(ask(&engine, &src, "שלום").lang, Lang::He);
    }

    #[test]
    fn small_talk_does_not_end_the_conversation_before_it() {
        // "ומה לגבי מחר?" after a calendar answer, a "תודה" and a "שלום" between them: still a follow-up
        let (engine, src) = (Engine::new(), Quiet::default());
        let _first = ask(&engine, &src, "מה יש לי היום");
        let calls = src.touched();
        assert_eq!(ask(&engine, &src, "תודה").title, "בכיף!");
        assert_eq!(ask(&engine, &src, "מה אתה יודע לעשות").title, "הנה מה שאני יודע לעשות");
        assert_eq!(src.touched(), calls, "small talk read a source");
        let next = ask(&engine, &src, "ומה לגבי מחר?");
        assert!(next.follow_up, "{next:?}");
        assert_ne!(next.title, "לא הבנתי בדיוק. מה תרצה לעשות?");
    }

    #[test]
    fn help_is_never_a_web_offer_and_a_search_stays_a_search() {
        let (engine, src) = (Engine::new(), Quiet::default());
        for t in ["עזרה", "שלום", "תודה", "מי אתה", "help", "thanks"] {
            let c = ask(&engine, &src, t);
            assert!(c.items.iter().all(|i| i.kind != ItemKind::Action), "{t}: {:?}", c.items);
            assert!(!c.title.contains("בגוגל") && !c.title.contains("Google"), "{t}: {}", c.title);
        }
        // a request that has more to say is not small talk
        let c = ask(&engine, &src, "תעזור לי למצוא את הקובץ של התקציב");
        assert!(!c.title.starts_with("הנה מה שאני"), "{}", c.title);
        assert!(src.touched() > 0, "the files were searched");
    }

    #[test]
    fn talk_wording_is_fixed_text_without_the_users_words() {
        let (engine, src) = (Engine::new(), Quiet::default());
        let c = ask(&engine, &src, "היי יובל מה אתה יכול לעשות בבקשה");
        assert_eq!(c.title, "הנה מה שאני יודע לעשות");
        assert!(!c.title.contains("בבקשה"));
        let line = crate::assistant::flow::log_line(&c, "execute:assistant.help", 1);
        assert!(!line.contains("היי"), "{line}");
    }
}
