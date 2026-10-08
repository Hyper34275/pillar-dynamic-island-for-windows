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
            // "מחר בשעה 15:00": the day is part of the answer unless it is today
            let t = from.format("%H:%M");
            let at = if he { format!("בשעה {t}") } else { format!("at {t}") };
            if diff == 0 {
                at
            } else {
                format!("{} {at}", relative_day(from, diff, lang))
            }
        }
        Grain::Day | Grain::Range => {
            let last = (to - Duration::minutes(1)).date_naive();
            if grain == Grain::Range && last != from.date_naive() {
                return format!("{}–{}", from.format("%d/%m"), last.format("%d/%m"));
            }
            let day = relative_day(from, diff, lang);
            // a part of one day ("מחר ב-15:00", "אחר הצהריים") says which hours
            if grain == Grain::Range && !is_whole_day(from, to) {
                let end = if to.time() == chrono::NaiveTime::MIN { "24:00".to_string() } else { to.format("%H:%M").to_string() };
                return format!("{day} {}–{end}", from.format("%H:%M"));
            }
            day
        }
    }
}

/// From local midnight to local midnight.
fn is_whole_day(from: DateTime<Local>, to: DateTime<Local>) -> bool {
    from.time() == chrono::NaiveTime::MIN && to.time() == chrono::NaiveTime::MIN
}

/// One day in words, `diff` days from today: "היום", "מחר", "ביום חמישי", "ב-25/03".
fn relative_day(from: DateTime<Local>, diff: i64, lang: Lang) -> String {
    let he = lang == Lang::He;
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

fn is_he_letter(c: char) -> bool {
    ('\u{05D0}'..='\u{05EA}').contains(&c)
}

/// "לך" or "לאיציק" ("ל-Dana" for a name that does not open with a Hebrew letter). A name that
/// opens with a vav takes it doubled, as Hebrew spells a prefix before a consonant vav:
/// ל+ורד = "לוורד", not "לורד" (which reads as "lord").
pub fn he_to(person: Option<&str>) -> String {
    let Some(p) = person.map(str::trim).filter(|p| !p.is_empty()) else { return "לך".into() };
    let second = p.chars().nth(1);
    match p.chars().next() {
        Some('ו') if second.is_some_and(|n| n != 'ו') => format!("לו{p}"),
        Some(c) if is_he_letter(c) => format!("ל{p}"),
        _ => format!("ל-{p}"),
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

/// Why a person's calendar could not be read: picks the one line that tells how to fix it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoAccess {
    /// Not in Outlook's Calendar module and not resolved in the address book (not shared, not
    /// Exchange, offline...).
    NotShared,
    /// The calendar is in Outlook but its owner has not allowed reading it (CAL-SHARED-101).
    NoPermission,
    /// Outlook, or the calendar, did not answer in time or is busy (CAL-SHARED-104/105,
    /// OUTLOOK-105/109).
    Unreachable,
    /// Classic Outlook is not running or cannot be reached (OUTLOOK-101 and the other OUTLOOK codes).
    OutlookDown,
    /// The new Outlook is in use (OUTLOOK-104).
    NewOutlook,
}

impl NoAccess {
    pub fn from_code(code: &str) -> Self {
        match code {
            // our own marker for "the name resolved to nobody"
            "OUTLOOK-107" => NoAccess::NotShared,
            "OUTLOOK-104" => NoAccess::NewOutlook,
            "OUTLOOK-105" | "OUTLOOK-109" | "CAL-SHARED-104" | "CAL-SHARED-105" => NoAccess::Unreachable,
            c if c.starts_with("OUTLOOK-") => NoAccess::OutlookDown,
            "CAL-SHARED-101" => NoAccess::NoPermission,
            _ => NoAccess::NotShared,
        }
    }
}

/// "אין לי גישה ליומן של איציק" / "I can't access Dana's calendar".
pub fn no_access_title(who: &str, lang: Lang) -> String {
    if lang == Lang::He {
        format!("אין לי גישה ליומן {}", he_of(Some(who)))
    } else {
        format!("I can't access {who}'s calendar")
    }
}

/// The one short line that tells how to fix it (no gendered pronoun for the person).
pub fn no_access_hint(kind: NoAccess, code: &str, lang: Lang) -> String {
    let he = lang == Lang::He;
    match kind {
        NoAccess::NotShared if he => "כדי שאוכל לבדוק, פתח ב-Outlook את היומן הזה (הוסף יומן ← מפנקס הכתובות)".into(),
        NoAccess::NotShared => "To let me check, open that calendar in Outlook (Add Calendar → From Address Book)".into(),
        NoAccess::NoPermission if he => "היומן נמצא ב-Outlook, אבל אין לי הרשאה לקרוא אותו. בקש מהבעלים לשתף אותו איתך".into(),
        NoAccess::NoPermission => "The calendar is in Outlook, but I am not allowed to read it. Ask its owner to share it with you".into(),
        NoAccess::Unreachable if he => "Outlook לא הגיב בזמן. נסה שוב בעוד רגע".into(),
        NoAccess::Unreachable => "Outlook did not answer in time. Try again in a moment".into(),
        NoAccess::OutlookDown | NoAccess::NewOutlook => error_text(code, lang).to_string(),
    }
}

/// The question above the calendars offered when the asked person's own calendar is out of reach.
pub fn no_access_choose(who: &str, lang: Lang) -> String {
    if lang == Lang::He {
        format!("{}. לבדוק אחד מהיומנים האלה?", no_access_title(who, lang))
    } else {
        format!("{}. Check one of these instead?", no_access_title(who, lang))
    }
}

/// Several calendars fit the name: "איזה יומן של איציק?".
pub fn which_calendar_of(who: &str, lang: Lang) -> String {
    if lang == Lang::He {
        format!("איזה יומן {}?", he_of(Some(who)))
    } else {
        format!("Which of {who}'s calendars?")
    }
}

/// "היומן של איציק פנוי מחר בשעה 15:00" / "תפוס"; "היומן שלך ..." for the user.
pub fn calendar_state(person: Option<&str>, busy: bool, day: &str, lang: Lang) -> String {
    if lang == Lang::He {
        format!("היומן {} {} {day}", he_of(person), if busy { "תפוס" } else { "פנוי" })
    } else {
        let whose = person.map_or("Your calendar".to_string(), |p| format!("{p}'s calendar"));
        format!("{whose} is {} {day}", if busy { "busy" } else { "free" })
    }
}

/// "תפוס: 10:00–11:00 · 14:00–15:30" (what free/busy shows: times only, no titles). A tentative or
/// out-of-office block says so; with `with_date` every block carries its day.
pub fn busy_summary(blocks: &[(DateTime<Utc>, DateTime<Utc>, BusyKind)], with_date: bool, lang: Lang) -> String {
    const MAX: usize = 6;
    let he = lang == Lang::He;
    let mut parts: Vec<String> = blocks
        .iter()
        .take(MAX)
        .map(|(a, b, kind)| {
            let range = if with_date { format!("{} {}", dm(*a), range_hm(*a, *b)) } else { range_hm(*a, *b) };
            match kind {
                BusyKind::Busy => range,
                BusyKind::Tentative => format!("{range} ({})", if he { "אולי" } else { "tentative" }),
                BusyKind::Oof => format!("{range} ({})", busy_label(BusyKind::Oof, lang)),
            }
        })
        .collect();
    if blocks.len() > MAX {
        parts.push("…".into());
    }
    format!("{}: {}", busy_label(BusyKind::Busy, lang), parts.join(" · "))
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

    #[test]
    fn a_name_takes_the_prefix_hebrew_gives_it() {
        assert_eq!(he_to(Some("איציק")), "לאיציק");
        assert_eq!(he_to(Some("דנה")), "לדנה");
        assert_eq!(he_to(Some("בני")), "לבני");
        assert_eq!(he_to(Some("לירון")), "ללירון");
        assert_eq!(he_to(Some("  יובל ")), "ליובל");
        assert_eq!(he_to(Some("איציק כהן")), "לאיציק כהן");
        // a vav that opens the name is doubled, or "לורד" reads as "lord"
        assert_eq!(he_to(Some("ורד")), "לוורד");
        assert_eq!(he_to(Some("ויקי")), "לוויקי");
        assert_eq!(he_to(Some("וון")), "לוון");
        // not a Hebrew letter: a hyphen
        assert_eq!(he_to(Some("Dana")), "ל-Dana");
        assert_eq!(he_to(Some("Éric")), "ל-Éric");
        assert_eq!(he_to(Some("3M")), "ל-3M");
        assert_eq!(he_to(None), "לך");
        assert_eq!(he_to(Some("  ")), "לך");
        assert_eq!(he_of(Some("ורד")), "של ורד");
    }

    #[test]
    fn headlines_for_names_that_open_with_vav_bet_and_lamed() {
        assert_eq!(meetings_title(2, "מחר", Some("ורד"), Lang::He), "מחר יש לוורד 2 פגישות");
        assert_eq!(meetings_title(0, "היום", Some("בני"), Lang::He), "היום אין לבני פגישות");
        assert_eq!(meetings_title(1, "היום", Some("לירון"), Lang::He), "היום יש ללירון פגישה אחת");
        assert_eq!(meetings_title(0, "היום", Some("דנה"), Lang::He), "היום אין לדנה פגישות");
        assert_eq!(meetings_title(2, "tomorrow", Some("Ron"), Lang::En), "Ron has 2 meetings tomorrow");
    }

    #[test]
    fn a_moment_and_a_part_of_a_day_say_which_day_and_hours() {
        let now = local(10, 12, 0);
        let at = |d, h, m| local(d, h, m);
        assert_eq!(day_label(at(10, 15, 0), at(10, 16, 0), Grain::Instant, now, Lang::He), "בשעה 15:00");
        assert_eq!(day_label(at(11, 15, 0), at(11, 16, 0), Grain::Instant, now, Lang::He), "מחר בשעה 15:00");
        assert_eq!(day_label(at(11, 15, 0), at(11, 16, 0), Grain::Instant, now, Lang::En), "tomorrow at 15:00");
        assert_eq!(day_label(at(14, 9, 0), at(14, 10, 0), Grain::Instant, now, Lang::He), "ביום ראשון בשעה 09:00");
        // an explicit hour comes as a one-hour range
        assert_eq!(day_label(at(11, 15, 0), at(11, 16, 0), Grain::Range, now, Lang::He), "מחר 15:00–16:00");
        assert_eq!(day_label(at(11, 12, 0), at(12, 0, 0), Grain::Range, now, Lang::En), "tomorrow 12:00–24:00");
        // a whole day, however it is labelled, has no hours
        assert_eq!(day_label(at(11, 0, 0), at(12, 0, 0), Grain::Range, now, Lang::He), "מחר");
        assert_eq!(day_label(at(11, 0, 0), at(12, 0, 0), Grain::Day, now, Lang::He), "מחר");
    }

    #[test]
    fn no_access_texts_name_the_person_and_the_fix() {
        assert_eq!(no_access_title("איציק", Lang::He), "אין לי גישה ליומן של איציק");
        assert_eq!(no_access_title("Dana", Lang::En), "I can't access Dana's calendar");
        assert_eq!(NoAccess::from_code("OUTLOOK-107"), NoAccess::NotShared);
        assert_eq!(NoAccess::from_code("MAIL-109"), NoAccess::NotShared);
        assert_eq!(NoAccess::from_code("CAL-SHARED-101"), NoAccess::NoPermission);
        assert_eq!(NoAccess::from_code("CAL-SHARED-104"), NoAccess::Unreachable);
        assert_eq!(NoAccess::from_code("OUTLOOK-101"), NoAccess::OutlookDown);
        assert_eq!(NoAccess::from_code("OUTLOOK-104"), NoAccess::NewOutlook);
        assert_eq!(no_access_hint(NoAccess::NotShared, "OUTLOOK-107", Lang::He), "כדי שאוכל לבדוק, פתח ב-Outlook את היומן הזה (הוסף יומן ← מפנקס הכתובות)");
        assert_eq!(no_access_hint(NoAccess::OutlookDown, "OUTLOOK-101", Lang::En), "Outlook is not running. Open it and try again");
        assert_eq!(no_access_choose("איציק", Lang::He), "אין לי גישה ליומן של איציק. לבדוק אחד מהיומנים האלה?");
        assert_eq!(which_calendar_of("איציק", Lang::He), "איזה יומן של איציק?");
        assert_eq!(which_calendar_of("Dana", Lang::En), "Which of Dana's calendars?");
    }

    #[test]
    fn busy_times_and_calendar_state() {
        let t = |h, m| local(10, h, m).with_timezone(&Utc);
        let blocks = [(t(10, 0), t(11, 0), BusyKind::Busy), (t(14, 0), t(15, 30), BusyKind::Tentative), (t(16, 0), t(17, 0), BusyKind::Oof)];
        assert_eq!(busy_summary(&blocks, false, Lang::He), "תפוס: 10:00–11:00 · 14:00–15:30 (אולי) · 16:00–17:00 (מחוץ למשרד)");
        assert_eq!(busy_summary(&blocks[..1], true, Lang::En), "Busy: 10/03 10:00–11:00");
        let many: Vec<_> = (0..8).map(|_| blocks[0]).collect();
        assert!(busy_summary(&many, false, Lang::He).ends_with('…'));
        assert_eq!(calendar_state(Some("איציק"), false, "מחר", Lang::He), "היומן של איציק פנוי מחר");
        assert_eq!(calendar_state(Some("איציק"), true, "מחר", Lang::He), "היומן של איציק תפוס מחר");
        assert_eq!(calendar_state(None, true, "היום", Lang::He), "היומן שלך תפוס היום");
        assert_eq!(calendar_state(Some("Dana"), false, "today", Lang::En), "Dana's calendar is free today");
        assert_eq!(calendar_state(None, true, "today", Lang::En), "Your calendar is busy today");
    }
}
