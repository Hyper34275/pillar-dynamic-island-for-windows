//! Wording of the answers (pure): headlines, summaries, plural forms, times, error texts.
//! Hebrew and English; times are local `HH:MM`.

use crate::intent::{weekday_name, Grain, Lang};
use chrono::{DateTime, Datelike, Duration, Local, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Noun {
    Meeting,
    Mail,
    File,
    Note,
    App,
    Result,
}

impl Noun {
    /// (singular, plural, feminine) in Hebrew.
    fn he(self) -> (&'static str, &'static str, bool) {
        match self {
            Noun::Meeting => ("פגישה", "פגישות", true),
            Noun::Mail => ("מייל", "מיילים", false),
            Noun::File => ("קובץ", "קבצים", false),
            Noun::Note => ("פתק", "פתקים", false),
            Noun::App => ("אפליקציה", "אפליקציות", true),
            Noun::Result => ("תוצאה", "תוצאות", true),
        }
    }

    fn en(self) -> (&'static str, &'static str) {
        match self {
            Noun::Meeting => ("meeting", "meetings"),
            Noun::Mail => ("email", "emails"),
            Noun::File => ("file", "files"),
            Noun::Note => ("note", "notes"),
            Noun::App => ("app", "apps"),
            Noun::Result => ("result", "results"),
        }
    }
}

/// "פגישה אחת", "2 פגישות", "1 meeting", "3 meetings". Zero: "0 פגישות".
pub fn count_text(n: u32, noun: Noun, lang: Lang) -> String {
    match lang {
        Lang::He => {
            let (one, many, fem) = noun.he();
            match n {
                1 => format!("{one} {}", if fem { "אחת" } else { "אחד" }),
                _ => format!("{n} {many}"),
            }
        }
        Lang::En => {
            let (one, many) = noun.en();
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        }
    }
}

/// "נמצאו 12 מיילים", "נמצא מייל אחד", "לא נמצאו מיילים"; "Found 12 emails", "No emails found".
pub fn found_text(n: u32, noun: Noun, lang: Lang) -> String {
    match lang {
        Lang::He => {
            let (one, many, fem) = noun.he();
            match n {
                0 => format!("לא נמצאו {many}"),
                1 => format!("{} {one} {}", if fem { "נמצאה" } else { "נמצא" }, if fem { "אחת" } else { "אחד" }),
                _ => format!("נמצאו {n} {many}"),
            }
        }
        Lang::En => {
            let (_, many) = noun.en();
            match n {
                0 => format!("No {many} found"),
                _ => format!("Found {}", count_text(n, noun, lang)),
            }
        }
    }
}

/// "עד כה" variant for a search that is still running out of time.
pub fn found_so_far(n: u32, noun: Noun, lang: Lang) -> String {
    match lang {
        Lang::He => match n {
            0 => format!("עדיין לא נמצאו {}", noun.he().1),
            _ => format!("{} עד כה", found_text(n, noun, lang)),
        },
        Lang::En => match n {
            0 => format!("No {} found yet", noun.en().1),
            _ => format!("{} so far", found_text(n, noun, lang)),
        },
    }
}

pub fn hm(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}

pub fn dm(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%d/%m").to_string()
}

pub fn range_hm(a: DateTime<Utc>, b: DateTime<Utc>) -> String {
    format!("{}–{}", hm(a), hm(b))
}

fn week_index(d: chrono::NaiveDate) -> i64 {
    // Sunday-first weeks: days since an arbitrary Sunday (1970-01-04), floored.
    let days = d.num_days_from_ce() as i64 - chrono::NaiveDate::from_ymd_opt(1970, 1, 4).map_or(0, |s| s.num_days_from_ce() as i64);
    days.div_euclid(7)
}

/// The words for the asked window: "היום", "מחר", "ביום חמישי", "השבוע" / "today", "tomorrow",
/// "on Thursday", "this week".
pub fn day_label(from: DateTime<Local>, to: DateTime<Local>, grain: Grain, now: DateTime<Local>, lang: Lang) -> String {
    let diff = (from.date_naive() - now.date_naive()).num_days();
    let he = lang == Lang::He;
    match grain {
        Grain::Week => match week_index(from.date_naive()) - week_index(now.date_naive()) {
            0 => if he { "השבוע".into() } else { "this week".into() },
            1 => if he { "בשבוע הבא".into() } else { "next week".into() },
            -1 => if he { "בשבוע שעבר".into() } else { "last week".into() },
            _ => {
                let d = from.format("%d/%m");
                if he { format!("בשבוע של {d}") } else { format!("in the week of {d}") }
            }
        },
        Grain::Instant => {
            let t = from.format("%H:%M");
            if he { format!("בשעה {t}") } else { format!("at {t}") }
        }
        Grain::Day | Grain::Range => {
            let last = (to - Duration::minutes(1)).date_naive();
            if grain == Grain::Range && last != from.date_naive() {
                return format!("{}–{}", from.format("%d/%m"), last.format("%d/%m"));
            }
            match diff {
                0 => if he { "היום".into() } else { "today".into() },
                1 => if he { "מחר".into() } else { "tomorrow".into() },
                2 if he => "מחרתיים".into(),
                -1 => if he { "אתמול".into() } else { "yesterday".into() },
                -2 if he => "שלשום".into(),
                3..=6 => {
                    let name = weekday_name(from.weekday(), lang);
                    if he { format!("ביום {name}") } else { format!("on {name}") }
                }
                _ => {
                    let d = from.format("%d/%m");
                    if he { format!("ב-{d}") } else { format!("on {d}") }
                }
            }
        }
    }
}

/// "לך" or "לאיציק" ("ל-Dana" for a Latin name).
pub fn he_to(person: Option<&str>) -> String {
    match person {
        None => "לך".into(),
        Some(p) if p.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) => format!("ל-{p}"),
        Some(p) => format!("ל{p}"),
    }
}

/// "שלך" or "של איציק".
pub fn he_of(person: Option<&str>) -> String {
    match person {
        None => "שלך".into(),
        Some(p) => format!("של {p}"),
    }
}

/// The headline of a meetings answer: "מחר יש לאיציק 3 פגישות", "היום אין לך פגישות".
pub fn meetings_title(n: u32, day: &str, person: Option<&str>, lang: Lang) -> String {
    match lang {
        Lang::He => {
            let to = he_to(person);
            if n == 0 {
                format!("{day} אין {to} פגישות")
            } else {
                format!("{day} יש {to} {}", count_text(n, Noun::Meeting, lang))
            }
        }
        Lang::En => {
            let (who, has) = match person {
                None => ("You".to_string(), "have"),
                Some(p) => (p.to_string(), "has"),
            };
            if n == 0 {
                format!("{who} {has} no meetings {day}")
            } else {
                format!("{who} {has} {} {day}", count_text(n, Noun::Meeting, lang))
            }
        }
    }
}

/// "09:00 · 11:30 · 14:00" (all-day events: "כל היום"). With `with_date`: "15/01 09:00".
pub fn times_line(events: &[(DateTime<Utc>, bool)], with_date: bool, lang: Lang) -> String {
    const MAX: usize = 6;
    let mut parts: Vec<String> = events
        .iter()
        .take(MAX)
        .map(|(start, all_day)| {
            let t = if *all_day {
                if lang == Lang::He { "כל היום".to_string() } else { "all day".to_string() }
            } else {
                hm(*start)
            };
            if with_date {
                format!("{} {t}", dm(*start))
            } else {
                t
            }
        })
        .collect();
    if events.len() > MAX {
        parts.push("…".into());
    }
    parts.join(" · ")
}

pub fn slots_line(slots: &[(DateTime<Utc>, DateTime<Utc>)], with_date: bool, lang: Lang) -> String {
    const MAX: usize = 4;
    let label = if lang == Lang::He { "פנוי" } else { "Free" };
    let mut parts: Vec<String> = slots
        .iter()
        .take(MAX)
        .map(|(a, b)| if with_date { format!("{} {}", dm(*a), range_hm(*a, *b)) } else { range_hm(*a, *b) })
        .collect();
    if slots.len() > MAX {
        parts.push("…".into());
    }
    format!("{label}: {}", parts.join(" · "))
}

pub fn busy_label(kind: BusyKind, lang: Lang) -> &'static str {
    match (kind, lang) {
        (BusyKind::Busy, Lang::He) => "תפוס",
        (BusyKind::Busy, Lang::En) => "Busy",
        (BusyKind::Tentative, Lang::He) => "אולי תפוס",
        (BusyKind::Tentative, Lang::En) => "Tentative",
        (BusyKind::Oof, Lang::He) => "מחוץ למשרד",
        (BusyKind::Oof, Lang::En) => "Out of office",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusyKind {
    Busy,
    Tentative,
    Oof,
}

/// A number for the calculator: up to 6 decimals, no trailing zeros, '.' as the separator.
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return "∞".into();
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

pub fn processing_title(lang: Lang) -> &'static str {
    if lang == Lang::He { "מחפש…" } else { "Searching…" }
}

pub fn no_title(lang: Lang) -> &'static str {
    if lang == Lang::He { "(ללא נושא)" } else { "(no subject)" }
}

/// The asked range was longer than the calendar can read at once.
pub fn range_clamped_note(days: i64, lang: Lang) -> String {
    if lang == Lang::He {
        format!("הטווח ארוך מדי — מוצגים {days} הימים הראשונים.")
    } else {
        format!("That range is too long — showing the first {days} days.")
    }
}

pub fn partial_note(lang: Lang) -> &'static str {
    if lang == Lang::He {
        "החיפוש לא הסתיים — מוצגות התוצאות שנמצאו עד כה."
    } else {
        "The search did not finish — showing what was found so far."
    }
}

pub fn mailbox_question(lang: Lang) -> &'static str {
    if lang == Lang::He { "באיזו תיבת דואר לחפש?" } else { "Which mailbox should I search?" }
}

pub fn all_mailboxes_label(lang: Lang) -> &'static str {
    if lang == Lang::He {
        "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן."
    } else {
        "I don't know — search every mailbox I can access."
    }
}

pub fn calendar_question(lang: Lang) -> &'static str {
    if lang == Lang::He { "באיזה יומן לחפש?" } else { "Which calendar do you mean?" }
}

pub fn tap_to_open(lang: Lang) -> &'static str {
    if lang == Lang::He { "לחץ כדי לפתוח" } else { "Click to open" }
}

pub fn tap_to_launch(lang: Lang) -> &'static str {
    if lang == Lang::He { "לחץ כדי להפעיל" } else { "Click to launch" }
}

pub fn busy_only_note(lang: Lang) -> &'static str {
    if lang == Lang::He {
        "אני רואה רק זמנים תפוסים, לא את שמות הפגישות."
    } else {
        "I can see busy times only, not the meeting titles."
    }
}

pub fn calendar_not_found(person: &str, lang: Lang) -> String {
    if lang == Lang::He {
        format!("לא מצאתי יומן של {person} — פתח אותו ב-Outlook")
    } else {
        format!("I could not find {person}'s calendar — open it in Outlook")
    }
}

pub fn mailbox_suffix(kind: crate::outlook_mail::MailboxKind, lang: Lang) -> &'static str {
    use crate::outlook_mail::MailboxKind as K;
    match (kind, lang) {
        (K::Shared, Lang::He) => " (משותפת)",
        (K::Shared, Lang::En) => " (shared)",
        (K::Additional, Lang::He) => " (נוספת)",
        (K::Additional, Lang::En) => " (additional)",
        (K::Archive, Lang::He) => " (ארכיון)",
        (K::Archive, Lang::En) => " (archive)",
        (K::DataFile, Lang::He) => " (קובץ נתונים)",
        (K::DataFile, Lang::En) => " (data file)",
        _ => "",
    }
}

pub fn no_match(lang: Lang) -> (&'static str, &'static str) {
    if lang == Lang::He {
        ("אפשר לשאול למשל: מה יש לי היום?", "מצא את המייל עם המילה תקציב\nמה יש לאיציק ביומן מחר?\nכמה זה 12 כפול 7")
    } else {
        ("You can ask, for example: what do I have today?", "Find the email with the word budget\nWhat does Dana have tomorrow?\nWhat is 12 times 7")
    }
}

pub fn calc_by_zero(lang: Lang) -> &'static str {
    if lang == Lang::He { "אי אפשר לחלק באפס" } else { "Cannot divide by zero" }
}

pub fn calc_failed(lang: Lang) -> &'static str {
    if lang == Lang::He { "לא הצלחתי לחשב את זה" } else { "I could not work that out" }
}

pub fn clarify_text(ask: crate::intent::AskKind, lang: Lang) -> &'static str {
    use crate::intent::AskKind as A;
    match (ask, lang) {
        (A::Date, Lang::He) => "לאיזה תאריך התכוונת?",
        (A::Date, Lang::En) => "Which date do you mean?",
        (A::Time, Lang::He) => "באיזו שעה: בבוקר או אחר הצהריים?",
        (A::Time, Lang::En) => "At what time: morning or afternoon?",
        (A::Person, Lang::He) => "של מי היומן?",
        (A::Person, Lang::En) => "Whose calendar do you mean?",
        (A::Content, Lang::He) => "מה לחפש?",
        (A::Content, Lang::En) => "What should I search for?",
        (A::Intent, Lang::He) => "לא הבנתי בדיוק. מה תרצה לעשות?",
        (A::Intent, Lang::En) => "I did not quite get that. What would you like to do?",
        (A::Mailbox, _) => mailbox_question(lang),
    }
}

/// A short, user-facing text for an error code.
pub fn error_text(code: &str, lang: Lang) -> &'static str {
    let he = lang == Lang::He;
    match (code, he) {
        ("MAIL-101", true) => "אין הרשאה לתיבת הדואר",
        ("MAIL-101", false) => "No permission for the mailbox",
        ("MAIL-102", true) => "תיבת הדואר כבר לא זמינה",
        ("MAIL-102", false) => "The mailbox is no longer available",
        ("MAIL-103", true) => "תיבת הדואר לא מחוברת כרגע",
        ("MAIL-103", false) => "The mailbox is offline",
        ("MAIL-104", true) => "המייל כבר לא זמין",
        ("MAIL-104", false) => "The mail is no longer available",
        ("MAIL-105", true) => "תיבת הדואר לא הגיבה בזמן",
        ("MAIL-105", false) => "The mailbox did not answer in time",
        ("OUTLOOK-101", true) => "Outlook לא פועל. פתח אותו ונסה שוב",
        ("OUTLOOK-101", false) => "Outlook is not running. Open it and try again",
        ("OUTLOOK-104", true) => "Outlook החדש אינו נתמך",
        ("OUTLOOK-104", false) => "The new Outlook is not supported",
        (c, true) if c.starts_with("OUTLOOK-") => "אין חיבור ל-Outlook כרגע",
        (c, false) if c.starts_with("OUTLOOK-") => "Outlook is not reachable right now",
        (c, true) if c.starts_with("FILES-") => "חיפוש הקבצים נכשל",
        (c, false) if c.starts_with("FILES-") => "File search failed",
        (c, true) if c.starts_with("APPS-") => "חיפוש האפליקציות נכשל",
        (c, false) if c.starts_with("APPS-") => "App search failed",
        (_, true) => "לא הצלחתי להשלים את הבקשה",
        (_, false) => "I could not complete the request",
    }
}

/// The `PREFIX-123` code in an error string, else `fallback`.
pub fn code_of(err: &str, fallback: &str) -> String {
    crate::diagnostics::code_in(err).unwrap_or(fallback).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local(d: u32, h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2027, 3, d, h, m, 0).unwrap()
    }

    #[test]
    fn hebrew_plurals() {
        assert_eq!(count_text(1, Noun::Meeting, Lang::He), "פגישה אחת");
        assert_eq!(count_text(2, Noun::Meeting, Lang::He), "2 פגישות");
        assert_eq!(count_text(1, Noun::Mail, Lang::He), "מייל אחד");
        assert_eq!(count_text(12, Noun::Mail, Lang::He), "12 מיילים");
        assert_eq!(found_text(0, Noun::Mail, Lang::He), "לא נמצאו מיילים");
        assert_eq!(found_text(1, Noun::Mail, Lang::He), "נמצא מייל אחד");
        assert_eq!(found_text(1, Noun::App, Lang::He), "נמצאה אפליקציה אחת");
        assert_eq!(found_text(12, Noun::Mail, Lang::He), "נמצאו 12 מיילים");
        assert_eq!(found_so_far(0, Noun::Mail, Lang::He), "עדיין לא נמצאו מיילים");
        assert_eq!(found_so_far(3, Noun::File, Lang::He), "נמצאו 3 קבצים עד כה");
    }

    #[test]
    fn english_plurals() {
        assert_eq!(count_text(1, Noun::Meeting, Lang::En), "1 meeting");
        assert_eq!(count_text(3, Noun::Meeting, Lang::En), "3 meetings");
        assert_eq!(found_text(0, Noun::Note, Lang::En), "No notes found");
        assert_eq!(found_text(1, Noun::Mail, Lang::En), "Found 1 email");
        assert_eq!(found_text(12, Noun::Mail, Lang::En), "Found 12 emails");
        assert_eq!(found_so_far(2, Noun::Mail, Lang::En), "Found 2 emails so far");
    }

    #[test]
    fn the_final_scenario_headline() {
        assert_eq!(meetings_title(3, "מחר", Some("איציק"), Lang::He), "מחר יש לאיציק 3 פגישות");
        assert_eq!(meetings_title(1, "מחר", Some("איציק"), Lang::He), "מחר יש לאיציק פגישה אחת");
        assert_eq!(meetings_title(0, "היום", None, Lang::He), "היום אין לך פגישות");
        assert_eq!(meetings_title(2, "היום", None, Lang::He), "היום יש לך 2 פגישות");
        assert_eq!(meetings_title(0, "מחר", Some("דנה"), Lang::He), "מחר אין לדנה פגישות");
        assert_eq!(meetings_title(2, "tomorrow", Some("Dana"), Lang::En), "Dana has 2 meetings tomorrow");
        assert_eq!(meetings_title(0, "today", None, Lang::En), "You have no meetings today");
        assert_eq!(meetings_title(1, "today", Some("Dan Lee"), Lang::En), "Dan Lee has 1 meeting today");
        assert_eq!(he_to(Some("Dana")), "ל-Dana");
    }

    #[test]
    fn times_are_local_hh_mm() {
        let a = local(10, 9, 0).with_timezone(&Utc);
        let b = local(10, 11, 30).with_timezone(&Utc);
        let c = local(10, 14, 0).with_timezone(&Utc);
        assert_eq!(times_line(&[(a, false), (b, false), (c, false)], false, Lang::He), "09:00 · 11:30 · 14:00");
        assert_eq!(times_line(&[(a, true)], false, Lang::He), "כל היום");
        assert_eq!(times_line(&[(a, false)], true, Lang::En), "10/03 09:00");
        assert_eq!(range_hm(a, b), "09:00–11:30");
        let many: Vec<_> = (0..8).map(|_| (a, false)).collect();
        assert!(times_line(&many, false, Lang::He).ends_with('…'));
    }

    #[test]
    fn day_labels() {
        let now = local(10, 12, 0);
        let day = |d: u32| (local(d, 0, 0), local(d + 1, 0, 0));
        let lab = |d: u32, lang| {
            let (f, t) = day(d);
            day_label(f, t, Grain::Day, now, lang)
        };
        assert_eq!(lab(10, Lang::He), "היום");
        assert_eq!(lab(11, Lang::He), "מחר");
        assert_eq!(lab(12, Lang::He), "מחרתיים");
        assert_eq!(lab(9, Lang::He), "אתמול");
        assert_eq!(lab(11, Lang::En), "tomorrow");
        // 2027-03-10 is a Wednesday; the 14th is a Sunday
        assert_eq!(lab(14, Lang::He), "ביום ראשון");
        assert_eq!(lab(14, Lang::En), "on Sunday");
        assert_eq!(lab(25, Lang::He), "ב-25/03");
    }

    #[test]
    fn week_labels_start_on_sunday() {
        // Wednesday 10 March 2027; its week started Sunday the 7th
        let now = local(10, 12, 0);
        let wk = |start: u32| day_label(local(start, 0, 0), local(start + 7, 0, 0), Grain::Week, now, Lang::He);
        assert_eq!(wk(7), "השבוע");
        assert_eq!(wk(14), "בשבוע הבא");
        assert_eq!(wk(21), "בשבוע של 21/03");
        let prev = day_label(Local.with_ymd_and_hms(2027, 2, 28, 0, 0, 0).unwrap(), local(7, 0, 0), Grain::Week, now, Lang::En);
        assert_eq!(prev, "last week");
    }

    #[test]
    fn numbers() {
        assert_eq!(format_number(4.0), "4");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(1.0 / 3.0), "0.333333");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(2.0000004), "2");
        assert_eq!(format_number(f64::INFINITY), "∞");
    }

    #[test]
    fn error_texts_and_codes() {
        assert_eq!(code_of("OUTLOOK-101: classic Outlook is not running", "APP-001"), "OUTLOOK-101");
        assert_eq!(code_of("nothing here", "MAIL-109"), "MAIL-109");
        assert_eq!(code_of("MAIL-105 timeout", "x"), "MAIL-105");
        assert!(error_text("MAIL-101", Lang::He).contains("הרשאה"));
        assert!(error_text("OUTLOOK-108", Lang::En).contains("Outlook"));
        assert!(!error_text("ZZZ", Lang::En).is_empty());
    }

    #[test]
    fn mailbox_labels() {
        use crate::outlook_mail::MailboxKind as K;
        assert_eq!(mailbox_suffix(K::Shared, Lang::He), " (משותפת)");
        assert_eq!(mailbox_suffix(K::Primary, Lang::He), "");
        assert_eq!(mailbox_question(Lang::He), "באיזו תיבת דואר לחפש?");
        assert_eq!(mailbox_question(Lang::En), "Which mailbox should I search?");
    }
}
