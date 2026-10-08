//! The executors of the explicit commands (`intent::commands`): what the card says for "תחפש בגוגל X",
//! "תפתח את ynet", "פתח הגדרות wifi", "פתח הורדות", "תרשום פתק: X", "תכתוב מייל לדני", "תנעל את המחשב", and
//! what a click on its item does.
//!
//! Policy (docs/AI_SEARCH.md, "Commands"): nothing here ever runs from text. [`build`] only describes the
//! action and puts it behind one openable item; the click arrives as `assistant_open_item` ->
//! `flow::Engine::open` -> `Sources::run_action` -> [`perform`]. A card never contains an address, a
//! command line or a path of the user's: an [`Action`] holds a validated https URL, or a key of a table in
//! `local::system_actions`, or the text of a note / the recipient and subject of a draft.
//!
//! Wire: the item has `ItemKind::Action` (the Center shows an unknown kind as plain information, with the
//! "פתח" button because `openable` is set) in a group of kind `"action"`.

use super::exec::{Group, Outcome, Run};
use super::store::Target;
use super::wire::{AssistantItem, CardPhase, ItemKind};
use crate::intent::{caps, commands, AskKind, CapId, Decision, Interpretation, Lang};
use crate::local::system_actions as sys;
use crate::notes::{self, Note};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

/// What a click does. Held in memory by the store, resolved only there; never serialised to a page.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// An `https` / `http` address that passed `normalize_web_url`.
    OpenUrl(String),
    /// A key of `system_actions::SETTINGS`.
    OpenSetting(String),
    /// A key of `system_actions::FOLDERS`.
    OpenFolder(String),
    /// Save a note through the notes module (exactly like the island's composer).
    SaveNote(String),
    /// Display a NEW message in Outlook with these fields. Never sent.
    ComposeMail { to: Option<String>, subject: Option<String> },
    /// `LockWorkStation`.
    Lock,
}

/// The capabilities this file executes.
pub fn handles(cap: CapId) -> bool {
    matches!(
        cap,
        caps::WEB_SEARCH | caps::WEB_OPEN | caps::SYSTEM_OPEN_SETTINGS | caps::FOLDERS_OPEN | caps::NOTES_CREATE | caps::MAIL_COMPOSE | caps::SYSTEM_LOCK
    )
}

// =============================================================================
// Wording
// =============================================================================

fn he(lang: Lang) -> bool {
    lang == Lang::He
}

/// Clip to `max` characters with an ellipsis.
fn clip(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let mut s: String = t.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

/// "״text״" in Hebrew, "“text”" in English.
fn quoted(text: &str, lang: Lang) -> String {
    if he(lang) {
        format!("\u{05F4}{text}\u{05F4}")
    } else {
        format!("\u{201C}{text}\u{201D}")
    }
}

fn tap_to_confirm(lang: Lang) -> &'static str {
    if he(lang) {
        "לחץ כדי לבצע"
    } else {
        "Click to do it"
    }
}

/// (title start, item title) of a web search.
fn engine_phrases(engine: &str, lang: Lang) -> (&'static str, &'static str) {
    match (engine, he(lang)) {
        ("youtube", true) => ("לחפש ביוטיוב", "חפש ביוטיוב"),
        ("youtube", false) => ("Search YouTube for", "Search YouTube"),
        ("wikipedia", true) => ("לחפש בוויקיפדיה", "חפש בוויקיפדיה"),
        ("wikipedia", false) => ("Search Wikipedia for", "Search Wikipedia"),
        ("maps", true) => ("לחפש בגוגל מפות", "חפש בגוגל מפות"),
        ("maps", false) => ("Search Google Maps for", "Search Google Maps"),
        ("waze", true) => ("לחפש ב-Waze", "חפש ב-Waze"),
        ("waze", false) => ("Search Waze for", "Search Waze"),
        ("bing", true) => ("לחפש בבינג", "חפש בבינג"),
        ("bing", false) => ("Search Bing for", "Search Bing"),
        ("translate", true) => ("לתרגם", "תרגם בגוגל תרגום"),
        ("translate", false) => ("Translate", "Translate with Google"),
        (_, true) => ("לחפש בגוגל", "חפש בגוגל"),
        (_, false) => ("Search Google for", "Search Google"),
    }
}

/// "לאנגלית" / "מעברית לאנגלית" / "to English" / "from Hebrew to English".
fn translate_line(from: Option<&str>, to: Option<&str>, lang: Lang) -> String {
    let name = |c: &str| sys::language_name(c, he(lang));
    match (from.and_then(name), to.and_then(name), he(lang)) {
        (Some(f), Some(t), true) => format!("מ{f} ל{t}"),
        (None, Some(t), true) => format!("ל{t}"),
        (Some(f), Some(t), false) => format!("from {f} to {t}"),
        (None, Some(t), false) => format!("to {t}"),
        _ => String::new(),
    }
}

fn folder_label(key: &str, lang: Lang) -> String {
    let he_label = match key {
        "downloads" => "תיקיית ההורדות",
        "documents" => "תיקיית המסמכים",
        "desktop" => "שולחן העבודה",
        "pictures" => "תיקיית התמונות",
        "music" => "תיקיית המוזיקה",
        "videos" => "תיקיית הסרטונים",
        "recycle" => "סל המחזור",
        "thispc" => "המחשב הזה",
        "onedrive" => "OneDrive",
        _ => "",
    };
    if he(lang) && !he_label.is_empty() {
        return he_label.to_string();
    }
    sys::folder_by_key(key).map(|f| if he(lang) { f.he } else { f.en }).unwrap_or("").to_string()
}

/// The question for a command that has no text yet ("מה לחפש?").
pub fn clarify_question(interp: &Interpretation, lang: Lang) -> Option<&'static str> {
    let Decision::Clarify { ask: AskKind::Content, cap: Some(cap) } = &interp.decision else { return None };
    Some(match (*cap, interp.slots.engine.as_deref(), he(lang)) {
        (caps::WEB_SEARCH, Some("translate"), true) => "מה לתרגם?",
        (caps::WEB_SEARCH, Some("translate"), false) => "What should I translate?",
        (caps::WEB_SEARCH, _, true) => "מה לחפש?",
        (caps::WEB_SEARCH, _, false) => "What should I search for?",
        (caps::NOTES_CREATE, _, true) => "מה לרשום בפתק?",
        (caps::NOTES_CREATE, _, false) => "What should the note say?",
        _ => return None,
    })
}

// =============================================================================
// The card
// =============================================================================

fn action_item(title: String, subtitle: Option<String>) -> AssistantItem {
    AssistantItem {
        id: String::new(),
        kind: ItemKind::Action,
        title,
        subtitle,
        time: None,
        end_time: None,
        accent: None,
        openable: true,
        unread: false,
        source: None,
    }
}

fn group_title(lang: Lang) -> &'static str {
    if he(lang) {
        "פעולה"
    } else {
        "Action"
    }
}

/// One answer: a headline, an optional line, and one openable item behind `action`.
fn offer(title: String, summary: String, item: AssistantItem, action: Action, lang: Lang) -> Outcome {
    let tap = tap_to_confirm(lang);
    let summary = if summary.is_empty() { tap.to_string() } else { format!("{summary}\n{tap}") };
    let mut o = Outcome::answer(title, summary);
    o.groups.push(Group { kind: "action", title: group_title(lang).to_string(), mailbox: None, items: vec![(item, Target::Action(action))], truncated: false, error_code: None });
    o
}

fn failed(code: &str, text_he: &str, text_en: &str, lang: Lang) -> Outcome {
    Outcome::error_text(code, if he(lang) { text_he } else { text_en })
}

/// Describe a command and put its action behind one click. Nothing is done here.
pub fn build(r: &Run, interp: &Interpretation, cap: CapId) -> Outcome {
    let lang = r.lang;
    let s = &interp.slots;
    match cap {
        caps::WEB_SEARCH => {
            let engine = s.engine.as_deref().unwrap_or("google");
            let Some(query) = s.query.as_deref().and_then(sys::clean_query) else {
                return failed("APP-042", "מה לחפש?", "What should I search for?", lang);
            };
            let Some(url) = sys::search_url(engine, &query, s.lang_from.as_deref(), s.lang_to.as_deref()) else {
                return failed("APP-042", "לא הצלחתי להבין את החיפוש", "I could not work out that search", lang);
            };
            let (start, item_title) = engine_phrases(engine, lang);
            let q = quoted(&clip(&query, 70), lang);
            let title = if he(lang) { format!("{start}: {q}") } else { format!("{start} {q}") };
            let summary = if engine == "translate" { translate_line(s.lang_from.as_deref(), s.lang_to.as_deref(), lang) } else { String::new() };
            let item = action_item(item_title.to_string(), sys::url_host(&url));
            offer(title, summary, item, Action::OpenUrl(url), lang)
        }
        caps::WEB_OPEN => {
            let (url, label) = if let Some(site) = s.site.as_deref().and_then(sys::site_by_key) {
                (site.url.to_string(), if he(lang) { site.he } else { site.en }.to_string())
            } else if let Some(url) = s.url.as_deref().and_then(sys::normalize_web_url) {
                let host = sys::url_host(&url).unwrap_or_default();
                (url, host)
            } else {
                return failed("APP-042", "לא הצלחתי להבין איזה אתר לפתוח", "I could not tell which website to open", lang);
            };
            let title = if he(lang) { format!("לפתוח את {label}") } else { format!("Open {label}") };
            let item = action_item(title.clone(), sys::url_host(&url));
            offer(title, String::new(), item, Action::OpenUrl(url), lang)
        }
        caps::SYSTEM_OPEN_SETTINGS => {
            let setting = sys::setting_or_home(s.setting.as_deref().unwrap_or("home"));
            let label = if he(lang) { setting.he } else { setting.en };
            let title = if he(lang) { format!("לפתוח: {label}") } else { format!("Open: {label}") };
            let item = action_item(title.clone(), Some(if he(lang) { "Windows" } else { "Windows" }.to_string()));
            offer(title, String::new(), item, Action::OpenSetting(setting.key.to_string()), lang)
        }
        caps::FOLDERS_OPEN => {
            let Some(key) = s.folder.as_deref().filter(|k| sys::folder_by_key(k).is_some()) else {
                return failed("APP-042", "לא הצלחתי להבין איזו תיקייה לפתוח", "I could not tell which folder to open", lang);
            };
            if sys::resolve_folder(key).is_none() {
                return failed("APP-052", "התיקייה הזאת לא קיימת במחשב הזה", "That folder does not exist on this PC", lang);
            }
            let label = folder_label(key, lang);
            let title = if he(lang) { format!("לפתוח את {label}") } else { format!("Open {label}") };
            let item = action_item(title.clone(), Some(if he(lang) { "סייר הקבצים" } else { "File Explorer" }.to_string()));
            offer(title, String::new(), item, Action::OpenFolder(key.to_string()), lang)
        }
        caps::NOTES_CREATE => {
            let Some(text) = s.query.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
                return failed("APP-042", "מה לרשום בפתק?", "What should the note say?", lang);
            };
            let title = if he(lang) { format!("לשמור פתק: {}", quoted(&clip(text, 80), lang)) } else { format!("Save a note: {}", quoted(&clip(text, 80), lang)) };
            let item = action_item(if he(lang) { "שמור פתק" } else { "Save note" }.to_string(), Some(if he(lang) { "יישמר בפתקים של האי" } else { "Saved with your island notes" }.to_string()));
            offer(title, String::new(), item, Action::SaveNote(text.to_string()), lang)
        }
        caps::MAIL_COMPOSE => {
            let to = s.mail_to.clone().filter(|t| !t.trim().is_empty());
            let subject = s.mail_subject.clone().filter(|t| !t.trim().is_empty());
            let mut parts: Vec<String> = Vec::new();
            if let Some(t) = &to {
                parts.push(if he(lang) { format!("אל: {}", clip(t, 60)) } else { format!("To: {}", clip(t, 60)) });
            }
            if let Some(t) = &subject {
                parts.push(if he(lang) { format!("נושא: {}", clip(t, 60)) } else { format!("Subject: {}", clip(t, 60)) });
            }
            let title = if he(lang) { "לפתוח הודעת מייל חדשה" } else { "Open a new email" }.to_string();
            let item = action_item(
                if he(lang) { "פתח הודעה חדשה ב-Outlook" } else { "New message in Outlook" }.to_string(),
                Some(if he(lang) { "ההודעה רק נפתחת, היא לא נשלחת" } else { "It only opens, nothing is sent" }.to_string()),
            );
            offer(title, parts.join(" · "), item, Action::ComposeMail { to, subject }, lang)
        }
        caps::SYSTEM_LOCK => {
            let title = if he(lang) { "לנעול את המחשב" } else { "Lock the PC" }.to_string();
            let item = action_item(if he(lang) { "נעל עכשיו" } else { "Lock now" }.to_string(), Some("Windows".to_string()));
            offer(title, String::new(), item, Action::Lock, lang)
        }
        _ => failed("APP-042", "לא הצלחתי להבין את הבקשה", "I could not understand the request", lang),
    }
}

// =============================================================================
// "I don't know" still offers the web
// =============================================================================

fn kind_title(kind: commands::Fallback, lang: Lang) -> Option<&'static str> {
    use commands::Fallback as F;
    Some(match (kind, he(lang)) {
        (F::Weather, true) => "את מזג האוויר אפשר לבדוק בגוגל",
        (F::Weather, false) => "Google can show you the weather",
        (F::Currency, true) => "שערי מטבע אפשר לבדוק בגוגל",
        (F::Currency, false) => "Google can show you exchange rates",
        (F::News, true) => "חדשות אפשר לראות בגוגל",
        (F::News, false) => "Google can show you the news",
        (F::Sports, true) => "תוצאות ספורט אפשר לראות בגוגל",
        (F::Sports, false) => "Google can show you sports results",
        (F::Generic, _) => return None,
    })
}

/// When nothing was understood, keep the honest message and also offer "search it on Google" as a click.
/// Applies to a `NoMatch` answer, and to a question the engine asked about a weather / currency / news /
/// sports text (the engine mistakes "מה שער הדולר" for a search with no subject); every other outcome is
/// returned unchanged.
pub fn with_web_offer(mut o: Outcome, interp: &Interpretation, text: &str) -> Outcome {
    if o.phase != CardPhase::Answer || !o.groups.is_empty() {
        return o;
    }
    let lang = interp.lang;
    let Some(kind) = commands::fallback_kind(text) else { return o };
    match interp.decision {
        Decision::NoMatch if o.question.is_none() => {}
        Decision::Clarify { .. } if kind != commands::Fallback::Generic => o.question = None,
        _ => return o,
    }
    let Some(query) = sys::clean_query(text) else { return o };
    let Some(url) = sys::search_url("google", &query, None, None) else { return o };
    if let Some(title) = kind_title(kind, lang) {
        o.title = title.to_string();
        o.summary = String::new();
    }
    let item_title = if he(lang) { format!("חפש בגוגל: {}", quoted(&clip(&query, 60), lang)) } else { format!("Search Google: {}", quoted(&clip(&query, 60), lang)) };
    let item = action_item(item_title, sys::url_host(&url));
    o.groups.push(Group { kind: "action", title: group_title(lang).to_string(), mailbox: None, items: vec![(item, Target::Action(Action::OpenUrl(url)))], truncated: false, error_code: None });
    o
}

// =============================================================================
// The click
// =============================================================================

fn new_note_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = Sha256::new();
    h.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    h.update(nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.finalize()[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The notes list with one new note, or an error when it would not really be saved: blank text, or a
/// full list (the notes module keeps the newest 500 and would silently drop the oldest one).
pub fn add_note(existing: Vec<Note>, text: &str, now_ms: i64, id: String) -> Result<Vec<Note>, String> {
    if text.trim().is_empty() {
        return Err("APP-054: the note is empty".into());
    }
    let before = existing.len();
    let mut list = existing;
    list.push(Note { id: id.clone(), text: text.to_string(), created_at: now_ms, updated_at: now_ms, pinned: false });
    let out = notes::sanitize(list, now_ms);
    if out.len() != before + 1 || !out.iter().any(|n| n.id == id) {
        return Err("APP-054: the notes list is full".into());
    }
    Ok(out)
}

fn save_note(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    let now = chrono::Local::now().timestamp_millis();
    let list = add_note(notes::load()?, text, now, new_note_id())?;
    notes::save(app, list).map(|_| ())
}

/// Do what the clicked item stands for. Every branch re-validates its input.
pub fn perform(app: &tauri::AppHandle, action: &Action) -> Result<(), String> {
    match action {
        Action::OpenUrl(url) => sys::open_url(url),
        Action::OpenSetting(key) => sys::open_setting(key),
        Action::OpenFolder(key) => sys::open_folder(key),
        Action::SaveNote(text) => save_note(app, text),
        Action::ComposeMail { to, subject } => sys::compose_mail(to.as_deref(), subject.as_deref()),
        Action::Lock => sys::lock_workstation(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::exec::Sources;
    use crate::assistant::flow::Engine;
    use crate::assistant::prefs::Prefs;
    use crate::assistant::wire::AssistantCard;
    use crate::calendar::{CalendarSourceDto, RangeRead};
    use crate::local::{AppHit, FileSearch, NoteHit};
    use crate::outlook_mail::{FreeBusy, MailCursor, MailQuery, MailSearchResult, MailboxInfo};
    use chrono::{DateTime, Local, Utc};
    use std::sync::Mutex;

    /// Sources that find nothing and record every action a click performs.
    #[derive(Default)]
    struct Probe {
        performed: Mutex<Vec<Action>>,
        refuse: bool,
    }

    impl Sources for Probe {
        fn cached_mailboxes(&self) -> Vec<MailboxInfo> {
            Vec::new()
        }
        fn discover_mailboxes(&self, _force: bool) -> Result<Vec<MailboxInfo>, String> {
            Err("OUTLOOK-101: not running".into())
        }
        fn search_mail(&self, _q: &MailQuery, _c: Option<MailCursor>) -> Result<MailSearchResult, String> {
            Ok(MailSearchResult::default())
        }
        fn open_mail(&self, _key: &str) -> Result<(), String> {
            Err("MAIL-104".into())
        }
        fn free_busy(&self, _n: &str, _f: DateTime<Utc>, _t: DateTime<Utc>) -> Result<FreeBusy, String> {
            Err("OUTLOOK-101: not running".into())
        }
        fn calendars(&self) -> Vec<CalendarSourceDto> {
            Vec::new()
        }
        fn people(&self) -> Vec<String> {
            Vec::new()
        }
        fn query_range(&self, _f: DateTime<Utc>, _t: DateTime<Utc>, _o: Option<Vec<String>>) -> Result<RangeRead, String> {
            Err("OUTLOOK-101: not running".into())
        }
        fn prefetched(&self, _f: DateTime<Utc>, _t: DateTime<Utc>, _o: Option<&[String]>) -> Option<RangeRead> {
            None
        }
        fn open_event(&self, _s: DateTime<Utc>) -> Result<(), String> {
            Err("x".into())
        }
        fn search_files(&self, _t: &[Vec<String>], _e: Option<&str>, _l: usize, _b: u64) -> Result<FileSearch, String> {
            Ok(FileSearch::default())
        }
        fn open_file(&self, _k: &str) -> Result<(), String> {
            Err("x".into())
        }
        fn search_apps(&self, _n: &[String], _l: usize) -> Result<Vec<AppHit>, String> {
            Ok(Vec::new())
        }
        fn launch_app(&self, _k: &str) -> Result<(), String> {
            Err("x".into())
        }
        fn search_notes(&self, _t: &[Vec<String>], _l: bool, _n: usize) -> Result<Vec<NoteHit>, String> {
            Ok(Vec::new())
        }
        fn open_note(&self, _id: &str) -> Result<(), String> {
            Err("x".into())
        }
        fn load_prefs(&self) -> Option<Prefs> {
            None
        }
        fn save_prefs(&self, _p: &Prefs) -> Result<(), String> {
            Ok(())
        }
        fn run_action(&self, action: &Action) -> Result<(), String> {
            if self.refuse {
                return Err("APP-050: no".into());
            }
            self.performed.lock().unwrap().push(action.clone());
            Ok(())
        }
    }

    fn ask(engine: &Engine, src: &Probe, text: &str) -> AssistantCard {
        engine.submit(src, text, "q0000000000000001", Local::now())
    }

    fn click(engine: &Engine, src: &Probe, card: &AssistantCard) -> Result<(), String> {
        engine.open(src, &card.query_id, &card.items[0].id, Local::now().timestamp_millis())
    }

    #[test]
    fn every_command_waits_for_the_click_and_then_does_exactly_one_thing() {
        let google_cats = "https://www.google.com/search?q=%D7%97%D7%AA%D7%95%D7%9C%D7%99%D7%9D".to_string();
        let cases: Vec<(&str, Action)> = vec![
            ("תחפש בגוגל חתולים", Action::OpenUrl(google_cats)),
            ("search youtube for lofi music", Action::OpenUrl("https://www.youtube.com/results?search_query=lofi%20music".into())),
            ("תנווט בווייז לתל אביב", Action::OpenUrl("https://waze.com/ul?q=%D7%AA%D7%9C%20%D7%90%D7%91%D7%99%D7%91".into())),
            ("איפה נמצא הכותל", Action::OpenUrl("https://www.google.com/maps/search/?api=1&query=%D7%94%D7%9B%D7%95%D7%AA%D7%9C".into())),
            ("תתרגם שלום", Action::OpenUrl("https://translate.google.com/?sl=auto&tl=en&text=%D7%A9%D7%9C%D7%95%D7%9D".into())),
            ("תפתח את ynet", Action::OpenUrl("https://www.ynet.co.il".into())),
            ("open gmail", Action::OpenUrl("https://mail.google.com".into())),
            ("פתח את example.co.il", Action::OpenUrl("https://example.co.il".into())),
            ("פתח הגדרות wifi", Action::OpenSetting("wifi".into())),
            ("open display settings", Action::OpenSetting("display".into())),
            ("פתח את לוח הבקרה", Action::OpenSetting("control-panel".into())),
            ("פתח הורדות", Action::OpenFolder("downloads".into())),
            ("open my documents", Action::OpenFolder("documents".into())),
            ("תרשום פתק: לקנות חלב", Action::SaveNote("לקנות חלב".into())),
            ("take a note: buy milk", Action::SaveNote("buy milk".into())),
            ("תכתוב מייל לדני בנושא תקציב", Action::ComposeMail { to: Some("דני".into()), subject: Some("תקציב".into()) }),
            ("תכתוב מייל", Action::ComposeMail { to: None, subject: None }),
            ("תנעל את המחשב", Action::Lock),
            ("lock", Action::Lock),
        ];
        for (text, expected) in cases {
            let engine = Engine::new();
            let src = Probe::default();
            let card = ask(&engine, &src, text);
            assert_eq!(card.phase, CardPhase::Answer, "{text}: {card:?}");
            assert_eq!(card.items.len(), 1, "{text}");
            assert_eq!(card.total, 1, "{text}");
            assert!(card.items[0].openable && card.items[0].kind == ItemKind::Action, "{text}");
            assert_eq!(card.sources, vec!["action".to_string()], "{text}");
            assert!(!card.can_extend && !card.partial);
            // asking never does anything
            assert!(src.performed.lock().unwrap().is_empty(), "{text}: ran before the click");
            click(&engine, &src, &card).unwrap();
            assert_eq!(*src.performed.lock().unwrap(), vec![expected], "{text}");
        }
    }

    #[test]
    fn the_card_says_what_will_happen() {
        let engine = Engine::new();
        let src = Probe::default();
        let c = ask(&engine, &src, "תחפש בגוגל חתולים");
        assert_eq!(c.title, "לחפש בגוגל: \u{05F4}חתולים\u{05F4}");
        assert_eq!(c.items[0].title, "חפש בגוגל");
        assert_eq!(c.items[0].subtitle.as_deref(), Some("www.google.com"));
        assert!(c.summary.contains("לחץ"));
        let c = ask(&engine, &src, "search google for cats");
        assert_eq!(c.title, "Search Google for \u{201C}cats\u{201D}");
        assert_eq!(c.items[0].title, "Search Google");
        let c = ask(&engine, &src, "תחפש ביוטיוב שירים");
        assert_eq!(c.title, "לחפש ביוטיוב: \u{05F4}שירים\u{05F4}");
        assert_eq!(c.items[0].subtitle.as_deref(), Some("www.youtube.com"));
        let c = ask(&engine, &src, "תתרגם מעברית לרוסית תודה");
        assert_eq!(c.title, "לתרגם: \u{05F4}תודה\u{05F4}");
        assert!(c.summary.starts_with("מעברית לרוסית"));
        let c = ask(&engine, &src, "תפתח את ynet");
        assert_eq!(c.title, "לפתוח את ynet");
        assert_eq!(c.items[0].subtitle.as_deref(), Some("www.ynet.co.il"));
        let c = ask(&engine, &src, "פתח הגדרות wifi");
        assert_eq!(c.title, "לפתוח: הגדרות Wi-Fi");
        let c = ask(&engine, &src, "פתח הורדות");
        assert_eq!(c.title, "לפתוח את תיקיית ההורדות");
        let c = ask(&engine, &src, "תרשום פתק: לקנות חלב");
        assert_eq!(c.title, "לשמור פתק: \u{05F4}לקנות חלב\u{05F4}");
        assert_eq!(c.items[0].title, "שמור פתק");
        let c = ask(&engine, &src, "תכתוב מייל לדני בנושא תקציב");
        assert_eq!(c.title, "לפתוח הודעת מייל חדשה");
        assert!(c.summary.starts_with("אל: דני · נושא: תקציב"), "{}", c.summary);
        assert!(c.items[0].subtitle.as_deref().unwrap().contains("לא נשלחת"));
        let c = ask(&engine, &src, "תנעל את המחשב");
        assert_eq!(c.title, "לנעול את המחשב");
        assert_eq!(c.items[0].title, "נעל עכשיו");
    }

    #[test]
    fn a_long_query_is_clipped_on_the_card_but_not_in_the_address() {
        let engine = Engine::new();
        let src = Probe::default();
        let long = "חתולים חמודים ".repeat(15);
        let c = ask(&engine, &src, &format!("תחפש בגוגל {long}"));
        assert!(c.title.chars().count() < 100, "{}", c.title.chars().count());
        assert!(c.title.contains('…'));
        click(&engine, &src, &c).unwrap();
        let performed = src.performed.lock().unwrap();
        let Action::OpenUrl(url) = &performed[0] else { panic!("{performed:?}") };
        assert!(url.len() > 200 && url.len() < 2000);
    }

    #[test]
    fn a_command_without_its_text_asks_for_it() {
        let engine = Engine::new();
        let src = Probe::default();
        for (text, question) in [("תחפש בגוגל", "מה לחפש?"), ("תרשום פתק", "מה לרשום בפתק?"), ("תתרגם", "מה לתרגם?"), ("search google", "What should I search for?"), ("take a note", "What should the note say?")] {
            let c = ask(&engine, &src, text);
            assert_eq!(c.question.as_deref(), Some(question), "{text}");
            assert!(c.items.is_empty(), "{text}");
        }
        assert!(src.performed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failing_action_reports_its_error_and_nothing_else_happens() {
        let engine = Engine::new();
        let src = Probe { refuse: true, ..Probe::default() };
        let c = ask(&engine, &src, "תפתח את ynet");
        let err = click(&engine, &src, &c).unwrap_err();
        assert!(err.starts_with("APP-050"));
        assert!(src.performed.lock().unwrap().is_empty());
    }

    #[test]
    fn an_item_of_another_query_opens_nothing() {
        let engine = Engine::new();
        let src = Probe::default();
        let c = ask(&engine, &src, "תפתח את ynet");
        let now = Local::now().timestamp_millis();
        assert!(engine.open(&src, &c.query_id, "ffffffffffff", now).is_err());
        assert!(engine.open(&src, "no-such-query", &c.items[0].id, now).is_err());
        assert!(src.performed.lock().unwrap().is_empty());
    }

    #[test]
    fn the_wire_shape_is_what_the_center_already_reads() {
        let engine = Engine::new();
        let src = Probe::default();
        let c = ask(&engine, &src, "תחפש בגוגל חתולים");
        let json = serde_json::to_value(&c.items[0]).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort();
        // exactly the old item fields: no new field, only a new `kind` value
        assert_eq!(keys, vec!["accent", "endTime", "id", "kind", "openable", "source", "subtitle", "time", "title", "unread"]);
        assert_eq!(json["kind"], "action");
        assert_eq!(json["openable"], true);
        let results = engine.results(&c.query_id, Local::now().timestamp_millis()).unwrap();
        assert_eq!(results.groups.len(), 1);
        assert_eq!(results.groups[0].kind, "action");
        assert_eq!(serde_json::to_value(&results.groups[0]).unwrap()["kind"], "action");
    }

    #[test]
    fn nothing_typed_by_the_user_reaches_the_log_line() {
        let engine = Engine::new();
        let src = Probe::default();
        let c = ask(&engine, &src, "תחפש בגוגל הסוד שלי");
        let line = crate::assistant::flow::log_line(&c, "confirm:web.search", 3);
        assert!(!line.contains("הסוד") && !line.contains("google"), "{line}");
    }

    #[test]
    fn an_unknown_folder_or_a_missing_text_is_an_error_card_not_an_action() {
        let src = Probe::default();
        let run = Run { src: &src, now: Local::now(), lang: Lang::He, prefs: None, inherited: None, allow_ask: true };
        let mut interp = crate::intent::interpret("פתח הורדות", &crate::intent::Ctx::default(), Local::now(), &crate::intent::Known::default());
        interp.slots.folder = Some("C:\\Windows".into());
        let o = build(&run, &interp, caps::FOLDERS_OPEN);
        assert_eq!(o.phase, CardPhase::Error);
        assert!(o.groups.is_empty());
        // a settings key that does not exist opens the Settings home page, never a typed URI
        interp.slots.setting = Some("ms-settings:evil".into());
        let o = build(&run, &interp, caps::SYSTEM_OPEN_SETTINGS);
        assert_eq!(o.groups[0].items[0].1, Target::Action(Action::OpenSetting("home".into())));
        // an address that is not http(s) is refused
        interp.slots.site = None;
        interp.slots.url = Some("file:///C:/Windows/system32/cmd.exe".into());
        let o = build(&run, &interp, caps::WEB_OPEN);
        assert_eq!(o.phase, CardPhase::Error);
        interp.slots.url = Some("javascript:alert(1)".into());
        assert_eq!(build(&run, &interp, caps::WEB_OPEN).phase, CardPhase::Error);
        // a search without text
        interp.slots.query = Some("   ".into());
        interp.slots.engine = Some("google".into());
        assert_eq!(build(&run, &interp, caps::WEB_SEARCH).phase, CardPhase::Error);
        interp.slots.query = None;
        assert_eq!(build(&run, &interp, caps::NOTES_CREATE).phase, CardPhase::Error);
    }

    // ---- notes ----

    fn note(id: &str, text: &str, at: i64) -> Note {
        Note { id: id.into(), text: text.into(), created_at: at, updated_at: at, pinned: false }
    }

    #[test]
    fn a_new_note_joins_the_list_like_the_composers() {
        let now = 1_800_000_000_000;
        let out = add_note(vec![note("old1", "ישן", now - 1000)], "  לקנות חלב \n", now, "abc123".into()).unwrap();
        assert_eq!(out.len(), 2);
        let n = out.iter().find(|n| n.id == "abc123").unwrap();
        assert_eq!(n.text, "  לקנות חלב \n", "the text is kept as typed");
        assert_eq!((n.created_at, n.updated_at, n.pinned), (now, now, false));
        // newest first, like every notes list
        assert_eq!(out[0].id, "abc123");
        assert!(notes::valid_id(&new_note_id()));
        assert_ne!(new_note_id(), new_note_id());
    }

    #[test]
    fn a_note_is_never_silently_lost() {
        let now = 1_800_000_000_000;
        assert!(add_note(Vec::new(), "   \n", now, "x".into()).unwrap_err().starts_with("APP-054"));
        assert!(add_note(Vec::new(), "ok", now, "bad id".into()).unwrap_err().starts_with("APP-054"), "an invalid id would be dropped");
        // a full list refuses instead of dropping the oldest note
        let full: Vec<Note> = (0..500).map(|i| note(&format!("n{i}"), "x", now - 10_000 + i)).collect();
        assert!(add_note(full, "one more", now, "newest".into()).unwrap_err().starts_with("APP-054"));
        // text over the limit is cut by the notes module, the note still exists
        let long = "ש".repeat(20_000);
        let out = add_note(Vec::new(), &long, now, "long1".into()).unwrap();
        assert_eq!(out[0].text.chars().count(), 10_000);
    }

    // ---- the fallback offer ----

    #[test]
    fn nothing_understood_still_offers_the_web() {
        let engine = Engine::new();
        let src = Probe::default();
        // weather: a fitting title and the Google search as the one click
        let c = ask(&engine, &src, "מה מזג האוויר");
        assert_eq!(c.phase, CardPhase::Answer);
        assert_eq!(c.title, "את מזג האוויר אפשר לבדוק בגוגל");
        assert_eq!(c.items.len(), 1);
        assert_eq!(c.items[0].title, "חפש בגוגל: \u{05F4}מה מזג האוויר\u{05F4}");
        click(&engine, &src, &c).unwrap();
        assert_eq!(*src.performed.lock().unwrap(), vec![Action::OpenUrl("https://www.google.com/search?q=%D7%9E%D7%94%20%D7%9E%D7%96%D7%92%20%D7%94%D7%90%D7%95%D7%95%D7%99%D7%A8".into())]);
        let c = ask(&engine, &src, "מה שער הדולר");
        assert_eq!(c.title, "שערי מטבע אפשר לבדוק בגוגל");
        let c = ask(&engine, &src, "what's the latest news");
        assert_eq!(c.title, "Google can show you the news");
        assert_eq!(c.items[0].title, "Search Google: \u{201C}what's the latest news\u{201D}");
        let c = ask(&engine, &src, "תוצאות הכדורגל");
        assert_eq!(c.title, "תוצאות ספורט אפשר לראות בגוגל");
        // anything else: the honest message stays, the offer is added
        let c = ask(&engine, &src, "כמה אנשים גרים בישראל");
        assert_eq!(c.title, "אפשר לשאול למשל: מה יש לי היום?");
        assert!(c.summary.contains("מצא את המייל"));
        assert_eq!(c.items.len(), 1);
        assert_eq!(c.items[0].title, "חפש בגוגל: \u{05F4}כמה אנשים גרים בישראל\u{05F4}");
    }

    #[test]
    fn the_web_is_not_offered_for_what_a_search_cannot_answer() {
        let engine = Engine::new();
        let src = Probe::default();
        for t in ["תמחק את כל המיילים", "run setup.exe", "OPEN 'C:\\Windows\\system32\\cmd.exe'", "שלח מייל לדנה שאני מאחר", "תבטל את הפגישה של מחר", "???"] {
            let c = ask(&engine, &src, t);
            assert!(c.items.is_empty(), "{t}: {:?}", c.items);
            assert!(c.title.starts_with("אפשר לשאול") || c.title.starts_with("You can ask"), "{t}: {}", c.title);
        }
    }

    #[test]
    fn the_web_offer_only_touches_a_no_match_answer() {
        let src = Probe::default();
        let run = Run { src: &src, now: Local::now(), lang: Lang::He, prefs: None, inherited: None, allow_ask: true };
        let interp = crate::intent::interpret("תחפש בגוגל חתולים", &crate::intent::Ctx::default(), Local::now(), &crate::intent::Known::default());
        let built = crate::assistant::exec::execute(&run, &interp);
        let after = with_web_offer(built.clone(), &interp, "תחפש בגוגל חתולים");
        assert_eq!(after.groups.len(), built.groups.len(), "a command already has its one item");
        assert_eq!(after.title, built.title);
    }

    // ---- guards in the source ----

    #[test]
    fn nothing_here_sends_mail_or_runs_a_typed_command() {
        let src = include_str!("actions.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for forbidden in [format!("\"{}\"", "Send"), format!("{}AndSave", "Send"), "std::process::Command".to_string(), "cmd.exe".to_string(), "powershell".to_string()] {
            assert!(!code.contains(&forbidden), "{forbidden}");
        }
        // the action that exists for mail only carries a recipient and a subject
        let a = Action::ComposeMail { to: Some("a".into()), subject: Some("b".into()) };
        assert!(format!("{a:?}").contains("to") && format!("{a:?}").contains("subject") && !format!("{a:?}").to_lowercase().contains("body"));
    }
}
