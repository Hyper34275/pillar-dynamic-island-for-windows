//! Explicit commands of the smart search: "תחפש בגוגל חתולים", "תפתח את ynet", "פתח הגדרות wifi",
//! "פתח הורדות", "תרשום פתק: לקנות חלב", "תכתוב מייל לדני בנושא תקציב", "תנעל את המחשב", "איפה נמצא
//! הכותל", "תתרגם שלום", ... in Hebrew and English.
//!
//! [`detect`] runs first inside `interpret`. It is deliberately conservative: it returns `Some` only
//! when the text has an explicit command verb (or a fixed command pattern such as "מפה של X") together
//! with an object it knows; everything else returns `None` and goes through the normal engine untouched
//! ("מה יש לי ביומן", "המייל מגוגל", "פגישה עם גוגל מחר" stay calendar / mail questions).
//!
//! Every command is a `Decision::Confirm { cap }`: the assistant shows what it understood and an openable
//! item, and nothing happens until the click (see `assistant::actions`). The words live in
//! `lexicon/commands.json` (compiled in); this file holds the grammar.
//!
//! The one exception is talking to the assistant itself: a sentence that is entirely "מה אתה יודע לעשות" /
//! "help", "שלום", "תודה" or "מי אתה" is a read-only `Decision::Execute` of an `assistant.*` capability (see
//! `talk` below and `assistant::talk`). It comes after the commands and after the answer to a question this
//! file asked, so it never takes a real request: only the whole sentence counts.
//!
//! Hebrew handling: the one-letter proclitics (ב ל מ ה ו ש כ), also joined by a hyphen ("ב-גוגל"), are
//! stripped from the object words that are looked up in a catalogue (engines, sites, settings, folders,
//! languages), longest phrase first. Verbs are matched as written (the lexicon lists the forms), with an
//! optional leading ש or ו ("שתפתח", "ותחפש"). The text the user typed (a query, a note, a subject) is
//! kept as typed; only wrapping quotes and a trailing full stop are removed.

use super::normalize::{clean, fold};
use super::types::*;
use super::{detect_lang, Ctx};
use crate::local::system_actions as sys;
use chrono::{DateTime, Local};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const LEXICON: &str = include_str!("lexicon/commands.json");
const MAX_CHARS: usize = 400;
const MAX_WORDS: usize = 40;
/// How many lead-in words ("תוכל בבקשה ל...") are skipped before the verb.
const MAX_LEAD: usize = 8;

/// The Hebrew one-letter prefixes.
fn is_pre(c: char) -> bool {
    matches!(c, 'ב' | 'ל' | 'מ' | 'ה' | 'ו' | 'ש' | 'כ')
}

// =============================================================================
// Words
// =============================================================================

/// One whitespace-separated word of the text.
#[derive(Clone, Debug)]
struct W {
    /// As typed (quotes unified).
    orig: String,
    /// `orig` without punctuation at its edges.
    raw: String,
    /// `fold(raw)`; a hyphenated prefix ("ב-גוגל") is already removed.
    f: String,
    /// The prefix letter of "ב-גוגל".
    pre: Option<char>,
    /// The word ended with a colon ("פתק:").
    colon: bool,
}

impl W {
    fn new(orig: &str) -> W {
        let colon = orig.trim_end_matches(['.', '!', '?', ',', ';', '"', '\'']).ends_with(':');
        let mut raw = orig.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        let mut pre = None;
        let cs: Vec<char> = raw.chars().collect();
        if cs.len() >= 3 && is_pre(cs[0]) && cs[1] == '-' {
            pre = Some(cs[0]);
            raw = cs[2..].iter().collect();
        }
        W { orig: orig.to_string(), f: fold(&raw), raw, pre, colon }
    }

    /// `orig` with a hyphenated prefix removed ("ל-תל" -> "תל").
    fn orig_no_pre(&self) -> String {
        match self.pre {
            Some(p) => self.orig.replacen(&format!("{p}-"), "", 1),
            None => self.orig.clone(),
        }
    }
}

/// The words of a cleaned text. A token with no letter or digit in it ("+", "&", "=", "/", "?", "–") is no
/// word of the grammar, but it is part of what the user typed ("2 + 2", "AT & T"): it stays attached to
/// the word before it, so a query, a note or a subject keeps it.
fn words(cleaned: &str) -> Vec<W> {
    let mut out: Vec<W> = Vec::new();
    for tok in cleaned.split_whitespace() {
        let word = W::new(tok);
        if !word.f.is_empty() {
            out.push(word);
        } else if let Some(prev) = out.last_mut() {
            prev.orig.push(' ');
            prev.orig.push_str(tok);
            prev.colon |= tok.trim_end_matches(['.', '!', '?', ',', ';', '"', '\'']).ends_with(':');
        }
    }
    out
}

/// A word and its forms without proclitics: `(form, the prefix letter that mattered)`.
fn stems(w: &W) -> Vec<(String, Option<char>)> {
    let mut v = vec![(w.f.clone(), w.pre)];
    let cs: Vec<char> = w.f.chars().collect();
    if w.pre.is_none() && cs.len() >= 3 && is_pre(cs[0]) {
        v.push((cs[1..].iter().collect(), Some(cs[0])));
        if cs.len() >= 4 && is_pre(cs[1]) {
            // "ובגוגל": the meaningful letter is the second one; "מהאתר": the first.
            let prep = if matches!(cs[0], 'ו' | 'ש' | 'כ') { cs[1] } else { cs[0] };
            v.push((cs[2..].iter().collect(), Some(prep)));
        }
    }
    v
}

/// Join the typed words (a query, a note, a subject).
fn join_orig(ws: &[W], strip_first_prep: bool) -> String {
    ws.iter().enumerate().map(|(k, w)| if k == 0 && strip_first_prep { w.orig_no_pre() } else { w.orig.clone() }).collect::<Vec<_>>().join(" ")
}

/// The text the user wants searched / written: leading separators, a trailing full stop and wrapping
/// quotes are removed; nothing else is changed.
fn text_of(ws: &[W], strip_first_prep: bool) -> Option<String> {
    let joined = join_orig(ws, strip_first_prep);
    let t = joined.trim().trim_end_matches(['.', ',', ';']).trim();
    let t = t.trim_start_matches([':', '-', '\u{2013}', '\u{2014}', ' ']);
    sys::clean_query(t)
}

// =============================================================================
// Lexicon
// =============================================================================

struct Cat {
    /// folded phrase -> key
    map: HashMap<String, String>,
    max_len: usize,
}

struct Lex {
    lists: HashMap<String, Vec<Vec<String>>>,
    sets: HashMap<String, HashSet<String>>,
    /// Every phrase of a list as one folded string ("look up"), for whole-phrase lookups.
    joined: HashMap<String, HashSet<String>>,
    cats: HashMap<String, Cat>,
}

fn words_of(phrase: &str) -> Vec<String> {
    fold(phrase).split(' ').filter(|w| !w.is_empty()).map(String::from).collect()
}

/// "מה {אתה|את} {יודע|יודעת}" -> the four phrases. A group may have an empty alternative ("{אני|} צריך");
/// groups do not nest. A phrase without braces is returned as it is.
fn expand(phrase: &str) -> Vec<String> {
    let Some(open) = phrase.find('{') else { return vec![phrase.to_string()] };
    let Some(close) = phrase[open..].find('}').map(|c| open + c) else { return vec![phrase.to_string()] };
    let (head, tail) = (&phrase[..open], &phrase[close + 1..]);
    phrase[open + 1..close].split('|').flat_map(|alt| expand(&format!("{head}{alt}{tail}"))).collect()
}

impl Lex {
    fn load() -> Lex {
        let v: serde_json::Value = serde_json::from_str(LEXICON).unwrap_or_default();
        let mut lex = Lex { lists: HashMap::new(), sets: HashMap::new(), joined: HashMap::new(), cats: HashMap::new() };
        let Some(obj) = v.as_object() else { return lex };
        for (name, value) in obj {
            match value {
                serde_json::Value::Array(items) => {
                    let mut phrases: Vec<Vec<String>> = items.iter().filter_map(|x| x.as_str()).flat_map(expand).map(|p| words_of(&p)).filter(|p| !p.is_empty()).collect();
                    // longest first, so "look up" wins over "look"
                    phrases.sort_by(|a, b| b.len().cmp(&a.len()));
                    let set: HashSet<String> = phrases.iter().filter(|p| p.len() == 1).map(|p| p[0].clone()).collect();
                    lex.sets.insert(name.clone(), set);
                    lex.joined.insert(name.clone(), phrases.iter().map(|p| p.join(" ")).collect());
                    lex.lists.insert(name.clone(), phrases);
                }
                serde_json::Value::Object(groups) => {
                    let mut cat = Cat { map: HashMap::new(), max_len: 1 };
                    for (key, forms) in groups {
                        for f in forms.as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                            let ws = words_of(f);
                            if ws.is_empty() {
                                continue;
                            }
                            cat.max_len = cat.max_len.max(ws.len());
                            cat.map.entry(ws.join(" ")).or_insert_with(|| key.clone());
                        }
                    }
                    lex.cats.insert(name.clone(), cat);
                }
                _ => {}
            }
        }
        lex
    }

    fn has(&self, list: &str, f: &str) -> bool {
        self.sets.get(list).map_or(false, |s| s.contains(f))
    }

    /// A word that carries no meaning in front of an object ("לי", "את", "the", "my").
    fn is_filler(&self, f: &str) -> bool {
        self.has("fillers", f) || self.has("articles", f)
    }

    /// The word, or one of its forms without proclitics, is in the (single-word) list.
    fn has_stem(&self, list: &str, w: &W) -> bool {
        stems(w).iter().any(|(s, _)| self.has(list, s))
    }

    /// The folded phrase (one or more words) is an entry of the list.
    fn in_list(&self, list: &str, phrase: &str) -> bool {
        self.joined.get(list).map_or(false, |l| l.contains(phrase))
    }

    /// `w[i..i + len]` (an engine name found by `cat_at`) is a generic word ("web", "online", "map", "ברשת"):
    /// it names a search engine only when the sentence leaves no doubt (see `m_search`).
    fn generic_engine(&self, w: &[W], i: usize, len: usize) -> bool {
        if len == 1 {
            return self.has_stem("generic_engines", &w[i]);
        }
        let phrase = w[i..i + len].iter().map(|x| x.f.as_str()).collect::<Vec<_>>().join(" ");
        self.in_list("generic_engines", &phrase)
    }

    /// Something on this PC is named (a mail, a file, a meeting, a note, an app, "my ...").
    fn names_local(&self, w: &W) -> bool {
        self.has("local_nouns", &w.f) || self.has_stem("local_nouns", w)
    }

    /// A phrase of the list starts at any of the first `within` words.
    fn phrase_near_start(&self, list: &str, w: &[W], within: usize) -> bool {
        (0..w.len().min(within)).any(|k| self.list_at(list, w, k, false).is_some())
    }

    /// Length of the longest phrase of `list` that starts at `w[i]`. `verb`: the first word may carry a
    /// leading ש or ו.
    fn list_at(&self, list: &str, w: &[W], i: usize, verb: bool) -> Option<usize> {
        let first = w.get(i)?.f.as_str();
        // "שתפתח", "ותחפש": the verb without its leading letter (no allocation: this runs for every text)
        let stripped: Option<&str> = if verb && first.chars().count() > 3 { first.strip_prefix(['ש', 'ו']) } else { None };
        for phrase in self.lists.get(list)? {
            if phrase[0] != first && Some(phrase[0].as_str()) != stripped {
                continue;
            }
            if i + phrase.len() > w.len() {
                continue;
            }
            if phrase[1..].iter().enumerate().all(|(k, p)| w[i + 1 + k].f == *p) {
                return Some(phrase.len());
            }
        }
        None
    }

    /// The longest catalogue phrase starting at `w[i]`: `(words, key, prefix letter)`.
    fn cat_at(&self, cat: &str, w: &[W], i: usize) -> Option<(usize, String, Option<char>)> {
        let c = self.cats.get(cat)?;
        let first = w.get(i)?;
        let room = (w.len() - i).min(c.max_len);
        for n in (1..=room).rev() {
            let rest: Vec<&str> = w[i + 1..i + n].iter().map(|x| x.f.as_str()).collect();
            for (stem, prep) in stems(first) {
                let key = if rest.is_empty() { stem } else { format!("{stem} {}", rest.join(" ")) };
                if let Some(k) = c.map.get(&key) {
                    return Some((n, k.clone(), prep));
                }
            }
        }
        None
    }

    /// The catalogue key when `ws` as a whole is one phrase.
    fn cat_all(&self, cat: &str, ws: &[&W]) -> Option<String> {
        if ws.is_empty() {
            return None;
        }
        let owned: Vec<W> = ws.iter().map(|w| (*w).clone()).collect();
        let (n, key, _) = self.cat_at(cat, &owned, 0)?;
        (n == owned.len()).then_some(key)
    }
}

fn lex() -> &'static Lex {
    static LEX: OnceLock<Lex> = OnceLock::new();
    LEX.get_or_init(Lex::load)
}

// =============================================================================
// Results
// =============================================================================

struct Cmd {
    cap: CapId,
    slots: Slots,
    /// Something is missing: ask for it instead of offering a click.
    ask: Option<AskKind>,
}

fn cmd(cap: CapId) -> Cmd {
    Cmd { cap, slots: Slots::default(), ask: None }
}

fn needs_text(cap: CapId, slots: Slots) -> Cmd {
    Cmd { cap, slots, ask: Some(AskKind::Content) }
}

/// What kind of verb opens the sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VClass {
    Search,
    Open,
    Launch,
    Go,
    Show,
    Play,
    Google,
    Change,
    Turn,
}

fn verb_class(l: &Lex, w: &[W], i: usize) -> Option<(usize, VClass)> {
    let table: [(&str, VClass); 9] = [
        ("v_google", VClass::Google),
        ("v_search", VClass::Search),
        ("v_open", VClass::Open),
        ("v_launch", VClass::Launch),
        ("v_go", VClass::Go),
        ("v_play", VClass::Play),
        ("v_show", VClass::Show),
        ("v_change", VClass::Change),
        ("v_turn", VClass::Turn),
    ];
    // the longest phrase over all classes ("go to" before "go"); the first listed class wins a tie
    let mut best: Option<(usize, VClass)> = None;
    for (list, class) in table {
        if let Some(n) = l.list_at(list, w, i, true) {
            if best.map_or(true, |(bn, _)| n > bn) {
                best = Some((n, class));
            }
        }
    }
    best
}

/// Skip the words that carry no meaning between the verb and its object ("לי", "את", "the").
fn skip_fillers(l: &Lex, w: &[W], mut i: usize) -> usize {
    while i < w.len() && l.has("fillers", &w[i].f) {
        i += 1;
    }
    i
}

/// Skip a leading "את" / "על" / "for" / "about" in front of a query, when something follows.
fn skip_query_lead(w: &[W], mut i: usize) -> usize {
    let mut guard = 0;
    while i + 1 < w.len() && guard < 2 && matches!(w[i].f.as_str(), "את" | "על" | "של" | "for" | "about" | "of" | "up") {
        i += 1;
        guard += 1;
    }
    i
}

/// English prepositions that introduce a search engine ("on google").
const EN_ENGINE_PREPS: [&str; 7] = ["on", "in", "using", "with", "via", "at", "through"];

// =============================================================================
// The grammar
// =============================================================================

impl Lex {
    fn parse(&self, text: &str) -> Option<Cmd> {
        let cleaned: String = clean(text).chars().take(MAX_CHARS).collect();
        if cleaned.contains('\\') {
            return None;
        }
        let mut w: Vec<W> = words(&cleaned);
        if w.is_empty() || w.len() > MAX_WORDS {
            return None;
        }
        // "... בבקשה", "... please" at the end belongs to no query
        while w.len() > 1 && self.has("trailing_polite", &w[w.len() - 1].f) {
            w.pop();
        }
        // lead-in words ("תוכל בבקשה", "can you please")
        let mut i = 0;
        while i < w.len().saturating_sub(1) && i < MAX_LEAD && self.has("lead", &w[i].f) {
            i += 1;
        }
        self.m_lock(&w, i)
            .or_else(|| self.m_note(&w, i))
            .or_else(|| self.m_compose(&w, i))
            .or_else(|| self.m_translate(&w, i))
            .or_else(|| self.m_nav(&w, i))
            .or_else(|| self.m_search(&w, i))
            .or_else(|| self.m_settings(&w, i))
            .or_else(|| self.m_folders(&w, i))
            .or_else(|| self.m_app(&w, i))
            .or_else(|| self.m_web_open(&w, i))
    }

    // ---- programs the general launcher words misread ----

    /// "פתח את סייר הקבצים" is not a file, "פתח פתקיות" is not a search of the notes: a few program names
    /// that the general words (קבצים, פתק) would otherwise pull towards files and notes. The slot holds the
    /// canonical name; `local::apps` resolves it (and its Hebrew / English aliases) when the card is built.
    fn m_app(&self, w: &[W], i: usize) -> Option<Cmd> {
        let (vn, class) = verb_class(self, w, i)?;
        if !matches!(class, VClass::Open | VClass::Launch) {
            return None;
        }
        let toks: Vec<&W> = w[i + vn..].iter().filter(|x| !self.is_filler(&x.f)).collect();
        let key = self.cat_all("apps_known", &toks)?;
        let mut c = cmd(caps::APPS_LAUNCH);
        c.slots.app = Some(key);
        Some(c)
    }

    // ---- lock ----

    fn m_lock(&self, w: &[W], i: usize) -> Option<Cmd> {
        let n = self.list_at("v_lock", w, i, true)?;
        let rest = &w[i + n..];
        if rest.iter().all(|x| self.is_filler(&x.f) || self.has("lock_objects", &x.f) || self.has_stem("lock_objects", x)) {
            Some(cmd(caps::SYSTEM_LOCK))
        } else {
            None
        }
    }

    // ---- notes ----

    fn m_note(&self, w: &[W], i: usize) -> Option<Cmd> {
        let mut j = i;
        let verb = self.list_at("v_note", w, j, true);
        if let Some(n) = verb {
            j += n;
        }
        // "תרשום לי פתק חדש", "take a note", "create a new note"
        let mut saw_new = false;
        while j < w.len() && (self.has("new_adj", &w[j].f) || self.has("fillers", &w[j].f)) {
            saw_new |= matches!(w[j].f.as_str(), "חדש" | "חדשה" | "new");
            j += 1;
        }
        let n = self.list_at("note_nouns", w, j, false)?;
        let colon = w[j + n - 1].colon;
        j += n;
        // "פתק חדש", "new note"
        while j < w.len() && matches!(w[j].f.as_str(), "חדש" | "חדשה" | "new") {
            saw_new = true;
            j += 1;
        }
        // Without a verb the text must still be unmistakably a new note: "note: X", "פתק חדש X".
        if verb.is_none() && !(colon || saw_new) {
            return None;
        }
        // optional intro ("שאומר", "saying", "to self")
        while j + 1 < w.len() {
            match self.list_at("note_intro", w, j, false) {
                Some(k) if j + k < w.len() => j += k,
                _ => break,
            }
        }
        let mut slots = Slots::default();
        match text_of(&w[j.min(w.len())..], false) {
            Some(t) => {
                slots.query = Some(t);
                Some(Cmd { cap: caps::NOTES_CREATE, slots, ask: None })
            }
            None => Some(needs_text(caps::NOTES_CREATE, slots)),
        }
    }

    // ---- a new mail ----

    fn m_compose(&self, w: &[W], i: usize) -> Option<Cmd> {
        let mut j = i;
        // "תכתוב לדני מייל": the recipient stands in front of the word "מייל"
        let mut pre_recipient: Option<usize> = None;
        // "email Dana about the budget"
        let email_verb = matches!(w[j].f.as_str(), "email" | "e-mail") && w.get(j + 1).map_or(false, |n| !self.has("email_not_verb", &n.f));
        if email_verb {
            j += 1;
        } else {
            let vn = self.list_at("v_compose", w, j, true)?;
            // "פתח מייל" alone is opening the mailbox; only "פתח מייל חדש" is a new message
            let open_verb = self.list_at("v_open", w, j, true).is_some();
            let verb_word = w[j].f.clone();
            j += vn;
            while j < w.len() && (self.has("mail_pre", &w[j].f) || self.has("fillers", &w[j].f)) {
                j += 1;
            }
            if self.list_at("mail_nouns", w, j, false).is_none() && j + 1 < w.len() && (w[j].pre == Some('ל') || w[j].f.starts_with('ל')) && self.list_at("mail_nouns", w, j + 1, false).is_some() {
                pre_recipient = Some(j);
                j += 1;
            }
            j += self.list_at("mail_nouns", w, j, false)?;
            let mut saw_new = false;
            while j < w.len() && matches!(w[j].f.as_str(), "חדש" | "חדשה" | "new") {
                saw_new = true;
                j += 1;
            }
            if open_verb && !saw_new {
                return None;
            }
            // English verbs that also describe mail someone is looking for ("new mail from Dan", "new mail
            // today", "create email rule"): a draft only when nothing but a recipient or a subject follows.
            if matches!(verb_word.as_str(), "new" | "start" | "begin" | "create" | "make" | "prepare" | "open") {
                let to_marker = |x: &W| matches!(x.f.as_str(), "to" | "for" | "אל");
                let fits = match w.get(j) {
                    // a bare "new mail" is not a draft either: it is the newest mail
                    None => verb_word != "new",
                    Some(x) if verb_word == "new" => to_marker(x),
                    Some(x) => to_marker(x) || self.list_at("mail_stop", w, j, false).is_some(),
                };
                if !fits {
                    return None;
                }
            }
        }
        let mut slots = Slots::default();
        // recipients: "לדני", "אל דני כהן", "to Dana", "לדני ולדנה"
        let hebrew = w.iter().any(|x| x.f.chars().any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c)));
        let mut names: Vec<String> = Vec::new();
        if let Some(p) = pre_recipient {
            names.push(name_of(&w[p]));
        }
        j = skip_fillers(self, w, j);
        let mut prep_seen = email_verb;
        if j < w.len() && matches!(w[j].f.as_str(), "אל" | "to") {
            j += 1;
            prep_seen = true;
        }
        if pre_recipient.is_none() && j < w.len() && !self.has("mail_stop", &w[j].f) {
            let first = &w[j];
            let is_addr = looks_like_address(&first.raw);
            let starts_lamed = first.f.chars().count() >= 3 && first.f.starts_with('ל');
            if is_addr || prep_seen || first.pre == Some('ל') || (hebrew && starts_lamed) {
                // the first name, without its ל
                let mut name_words: Vec<String> = Vec::new();
                let mut k = j;
                let one = if is_addr {
                    first.raw.clone()
                } else if first.pre == Some('ל') || (!prep_seen && starts_lamed) {
                    name_of(first)
                } else {
                    first.raw.clone()
                };
                name_words.push(one);
                k += 1;
                // a second word of the same name ("דני כהן"), not a clause ("שאני מאחר") or a marker
                if !is_addr {
                    if let Some(second) = w.get(k) {
                        let clause = second.f.starts_with('ש') && second.f.chars().count() > 3;
                        let conj = second.f.starts_with("ול") || matches!(second.f.as_str(), "ו" | "and" | "&");
                        if !clause && !conj && !self.has("mail_stop", &second.f) && second.f.chars().count() <= 14 && second.raw.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-') && !looks_like_address(&second.raw) {
                            name_words.push(second.raw.clone());
                            k += 1;
                        }
                    }
                }
                names.push(name_words.join(" "));
                // more recipients: "ולדנה"
                while names.len() < 3 && k < w.len() {
                    let t = &w[k];
                    if t.f.starts_with("ול") && t.f.chars().count() >= 4 {
                        names.push(t.raw.chars().skip(2).collect());
                        k += 1;
                    } else if matches!(t.f.as_str(), "and" | "&") && k + 1 < w.len() {
                        names.push(w[k + 1].raw.clone());
                        k += 2;
                    } else {
                        break;
                    }
                }
                j = k;
            }
        }
        if !names.is_empty() {
            slots.mail_to = Some(names.join("; "));
        }
        // subject: after a marker, or whatever is left
        let mut s = j;
        if let Some(n) = self.list_at("mail_stop", w, s, false) {
            s += n;
        }
        // "בקשר ל" / "בנושא ה": the preposition stays attached to the subject as typed
        if let Some(t) = text_of(&w[s.min(w.len())..], false) {
            slots.mail_subject = Some(t.chars().take(255).collect());
        }
        Some(Cmd { cap: caps::MAIL_COMPOSE, slots, ask: None })
    }

    // ---- translation ----

    fn m_translate(&self, w: &[W], i: usize) -> Option<Cmd> {
        let mut j = i;
        let mut needs_lang = false;
        if let Some(n) = self.list_at("v_translate", w, j, true) {
            j += n;
        } else if let Some(n) = self.list_at("t_how", w, j, false) {
            j += n;
            needs_lang = true;
        } else if let Some(n) = self.list_at("t_what", w, j, false) {
            j += n;
            needs_lang = true;
        } else {
            return None;
        }
        j = skip_fillers(self, w, j);
        let mut toks: Vec<W> = w[j..].to_vec();
        let (mut to, mut from): (Option<String>, Option<String>) = (None, None);
        // markers at the front: "לאנגלית X", "מעברית לאנגלית X", "to english: X"
        loop {
            let Some(first) = toks.first() else { break };
            if let Some((n, key, prep)) = self.cat_at("languages", &toks, 0) {
                if n == 1 && matches!(prep, Some('ל') | Some('ב')) {
                    to = Some(key);
                    toks.remove(0);
                    continue;
                }
                if n == 1 && prep == Some('מ') {
                    from = Some(key);
                    toks.remove(0);
                    continue;
                }
            }
            if self.has("t_to", &first.f) && first.f.len() > 1 && toks.len() > 1 {
                if let Some((1, key, None)) = self.cat_at("languages", &toks, 1) {
                    to = Some(key);
                    toks.drain(0..2);
                    continue;
                }
            }
            if first.f == "from" && toks.len() > 1 {
                if let Some((1, key, None)) = self.cat_at("languages", &toks, 1) {
                    from = Some(key);
                    toks.drain(0..2);
                    continue;
                }
            }
            break;
        }
        // markers at the end: "X באנגלית", "X to english", "X in hebrew"
        loop {
            let n = toks.len();
            if n == 0 {
                break;
            }
            if let Some((1, key, prep)) = self.cat_at("languages", &toks, n - 1) {
                if matches!(prep, Some('ל') | Some('ב')) {
                    to = to.or(Some(key));
                    toks.pop();
                    continue;
                }
                if prep == Some('מ') {
                    from = from.or(Some(key));
                    toks.pop();
                    continue;
                }
                if prep.is_none() && n >= 2 {
                    let before = toks[n - 2].f.as_str();
                    if matches!(before, "to" | "into" | "in") {
                        to = to.or(Some(key));
                        toks.truncate(n - 2);
                        continue;
                    }
                    if before == "from" {
                        from = from.or(Some(key));
                        toks.truncate(n - 2);
                        continue;
                    }
                }
            }
            break;
        }
        if needs_lang && to.is_none() && from.is_none() {
            return None;
        }
        let mut slots = Slots { engine: Some("translate".into()), lang_from: from, ..Slots::default() };
        match text_of(&toks, false) {
            Some(t) => {
                slots.lang_to = Some(to.unwrap_or_else(|| sys::default_translate_target(&t).to_string()));
                slots.query = Some(t);
                Some(Cmd { cap: caps::WEB_SEARCH, slots, ask: None })
            }
            None if !needs_lang => {
                slots.lang_to = to;
                Some(needs_text(caps::WEB_SEARCH, slots))
            }
            None => None,
        }
    }

    // ---- maps and navigation ----

    fn m_nav(&self, w: &[W], i: usize) -> Option<Cmd> {
        let mut j = i;
        let mut guarded = false; // "where is X" / "map of X": X must not be a mail, a file...
        let mut dest_has_prep = false;
        if let Some(n) = self.list_at("v_nav", w, j, true) {
            j += n;
            dest_has_prep = true;
        } else if let Some(n) = self.list_at("nav_phrases", w, j, false) {
            j += n;
            dest_has_prep = true;
        } else if let Some(n) = self.list_at("where_phrases", w, j, false) {
            j += n;
            guarded = true;
        } else {
            // "מפה של X", optionally after "תראה לי"
            let mut k = i;
            if let Some((n, VClass::Show)) = verb_class(self, w, k) {
                k = skip_fillers(self, w, k + n);
            }
            let n = self.list_at("map_of", w, k, false)?;
            j = k + n;
            guarded = true;
        }
        j = skip_fillers(self, w, j);
        let mut engine = "maps".to_string();
        // "תנווט בווייז לתל אביב"
        if let Some((n, key, prep)) = self.cat_at("engines", w, j) {
            if (key == "waze" || key == "maps") && (prep == Some('ב') || EN_ENGINE_PREPS.contains(&w[j].f.as_str())) && j + n < w.len() {
                engine = key;
                j = skip_fillers(self, w, j + n);
            }
        } else if j + 1 < w.len() && EN_ENGINE_PREPS.contains(&w[j].f.as_str()) {
            if let Some((n, key, None)) = self.cat_at("engines", w, j + 1) {
                if (key == "waze" || key == "maps") && j + 1 + n < w.len() {
                    engine = key;
                    j = skip_fillers(self, w, j + 1 + n);
                }
            }
        }
        // the destination: "לתל אביב", "אל הכותל", "to Haifa"
        let mut strip = false;
        if dest_has_prep {
            if j < w.len() && matches!(w[j].f.as_str(), "אל" | "to") {
                j += 1;
            } else if j < w.len() && (w[j].pre == Some('ל') || w[j].f.starts_with('ל')) && w[j].f.chars().count() >= 3 {
                strip = true;
            }
        }
        let mut end = w.len();
        // "... בווייז" at the end
        if end > j + 1 {
            if let Some((n, key, prep)) = self.cat_at("engines", w, end - 1) {
                if n == 1 && (key == "waze") && prep == Some('ב') {
                    engine = key;
                    end -= 1;
                }
            }
        }
        if j >= end {
            return None;
        }
        let dest_words = &w[j..end];
        if dest_words.len() > 8 {
            return None;
        }
        // "איפה נמצאת הפגישה", "איך מגיעים לפגישה של מחר", "navigate to my meeting": a place on a map is not a
        // mail, a file or a meeting, so these stay with the normal engine. After "directions to" / "נווט ל"
        // a possessive alone ("my office", "בית שלי") is still a place.
        const POSSESSIVE: [&str; 5] = ["שלי", "שלנו", "my", "our", "mine"];
        let local = |x: &W| self.names_local(x) && (guarded || !stems(x).iter().any(|(s, _)| POSSESSIVE.contains(&s.as_str())));
        if dest_words.iter().any(local) {
            return None;
        }
        let first_text = if strip && dest_words[0].pre.is_none() { strip_lamed(&dest_words[0].orig) } else { dest_words[0].orig_no_pre() };
        let mut ws: Vec<W> = dest_words.to_vec();
        ws[0] = W::new(&first_text);
        let q = text_of(&ws, false)?;
        let mut c = cmd(caps::WEB_SEARCH);
        c.slots.engine = Some(engine);
        c.slots.query = Some(q);
        Some(c)
    }

    // ---- web search ----

    fn m_search(&self, w: &[W], i: usize) -> Option<Cmd> {
        let n = w.len();
        let search_like = |c: VClass| matches!(c, VClass::Search | VClass::Show | VClass::Play);
        // engine in front: "בגוגל תחפש X", "ביוטיוב תחפש X"
        if let Some((len, key, prep)) = self.cat_at("engines", w, i) {
            if prep == Some('ב') {
                if let Some((vn, class)) = verb_class(self, w, i + len) {
                    if search_like(class) || class == VClass::Google {
                        let j = skip_query_lead(w, skip_fillers(self, w, i + len + vn));
                        return Some(self.search_result(&key, w, j));
                    }
                }
            }
        }
        // "תגגל X", "google X"
        if let Some((vn, VClass::Google)) = verb_class(self, w, i) {
            let j = skip_query_lead(w, skip_fillers(self, w, i + vn));
            return Some(self.search_result("google", w, j));
        }
        if matches!(w[i].f.as_str(), "גוגל" | "google") && w[i].pre.is_none() {
            // "google drive" is a website, not a search for "drive"
            if let Some((len, _, _)) = self.cat_at("sites", w, i) {
                if len >= 2 && i + len == n {
                    return None;
                }
            }
            if i + 1 < n {
                // "google sent me a file", "גוגל שלחו לי מייל", "google calendar invite": Google as a sender or
                // a product of Google, not a search (the verb forms below are explicit and need no such check)
                let product = self.cat_at("sites", w, i).map_or(false, |(len, _, _)| len >= 2);
                if product || w[i + 1..].iter().any(|x| self.names_local(x)) {
                    return None;
                }
                let j = skip_query_lead(w, i + 1);
                return Some(self.search_result("google", w, j));
            }
            return None;
        }
        let (vn, class) = verb_class(self, w, i)?;
        if !matches!(class, VClass::Search | VClass::Show | VClass::Play | VClass::Open | VClass::Launch | VClass::Go) {
            return None;
        }
        let mut j = skip_fillers(self, w, i + vn);
        // engine after the verb: "בגוגל", "on youtube", "google for"
        let mut en_prep = false;
        if j < n && EN_ENGINE_PREPS.contains(&w[j].f.as_str()) && j + 1 < n {
            en_prep = true;
            j += 1;
            j = skip_fillers(self, w, j);
        }
        if let Some((len, key, prep)) = self.cat_at("engines", w, j) {
            let hebrew_form = prep == Some('ב');
            // "search web design", "find online order", "check internet bill": a generic word ("web", "online",
            // "net", "map") in front of the query names an engine only with "for" / "about" after it
            let generic_needs_for = !en_prep && self.generic_engine(w, j, len) && !matches!(w.get(j + len).map(|x| x.f.as_str()), Some("for" | "about"));
            let english_form = en_prep || (class == VClass::Search && prep.is_none() && w[j].f.is_ascii() && !generic_needs_for);
            if hebrew_form || english_form {
                let k = skip_query_lead(w, skip_fillers(self, w, j + len));
                if k >= n {
                    // no text: ask when the verb was about searching; "פתח ביוטיוב" is an open command
                    return if search_like(class) { Some(needs_text(caps::WEB_SEARCH, Slots { engine: Some(key), ..Slots::default() })) } else { None };
                }
                if !search_like(class) && class != VClass::Open && class != VClass::Launch && class != VClass::Go {
                    return None;
                }
                return Some(self.search_result(&key, w, k));
            }
        }
        // the engine at the end: "תחפש מתכון לפיצה בגוגל", "play lofi on youtube"
        if search_like(class) {
            let start = skip_fillers(self, w, i + vn);
            for len in 1..=3usize {
                if n < start + len + 1 {
                    break;
                }
                let s = n - len;
                if let Some((l, key, prep)) = self.cat_at("engines", w, s) {
                    if s + l != n {
                        continue;
                    }
                    let he = prep == Some('ב');
                    let en = s > start && EN_ENGINE_PREPS.contains(&w[s - 1].f.as_str());
                    if he || en {
                        let q_end = if en { s - 1 } else { s };
                        if q_end <= start {
                            continue;
                        }
                        let k = skip_query_lead(w, start);
                        if k >= q_end {
                            continue;
                        }
                        // "תחפש את הקובץ ברשת", "find the map file in maps": a generic word at the end is no engine
                        // when the query names something on this PC
                        if self.generic_engine(w, s, l) && w[k..q_end].iter().any(|x| self.names_local(x)) {
                            continue;
                        }
                        return text_of(&w[k..q_end], false).map(|q| {
                            let mut c = cmd(caps::WEB_SEARCH);
                            c.slots.engine = Some(key);
                            c.slots.query = Some(q);
                            c
                        });
                    }
                }
            }
        }
        None
    }

    fn search_result(&self, engine: &str, w: &[W], from: usize) -> Cmd {
        let mut slots = Slots { engine: Some(engine.to_string()), ..Slots::default() };
        match text_of(&w[from.min(w.len())..], false) {
            Some(q) => {
                slots.query = Some(q);
                Cmd { cap: caps::WEB_SEARCH, slots, ask: None }
            }
            None => needs_text(caps::WEB_SEARCH, slots),
        }
    }

    // ---- Windows settings ----

    /// The settings page named by `toks` (which are all the words after the verb), and whether a word like
    /// "הגדרות" / "settings" was among them.
    fn settings_target(&self, toks: &[&W], strip_articles: bool) -> Option<(String, bool)> {
        let t1: Vec<&W> = toks.iter().copied().filter(|x| if strip_articles { !self.is_filler(&x.f) } else { !self.has("fillers", &x.f) }).collect();
        if let Some(key) = self.cat_all("settings", &t1) {
            return Some((key, false));
        }
        const REAL: [&str; 9] = ["הגדרות", "הגדרת", "settings", "setting", "הגדרה", "ההגדרות", "בהגדרות", "options", "אפשרויות"];
        let t2: Vec<&W> = t1.iter().copied().filter(|x| !self.has("settings_words", &x.f)).collect();
        let had_word = t1.iter().any(|x| REAL.contains(&x.f.as_str()) || stems(x).iter().any(|(s, _)| REAL.contains(&s.as_str())));
        if t2.is_empty() {
            return had_word.then(|| ("home".to_string(), true));
        }
        if let Some(key) = self.cat_all("settings", &t2) {
            return Some((key, had_word));
        }
        if had_word {
            if let Some(key) = self.cat_all("settings_weak", &t2) {
                return Some((key, true));
            }
        }
        None
    }

    fn m_settings(&self, w: &[W], i: usize) -> Option<Cmd> {
        let (vn, class) = match verb_class(self, w, i) {
            Some((n, c)) if matches!(c, VClass::Open | VClass::Launch | VClass::Go | VClass::Show | VClass::Change | VClass::Turn) => (n, Some(c)),
            _ => (0, None),
        };
        let toks: Vec<&W> = w[i + vn..].iter().collect();
        if toks.is_empty() {
            return None;
        }
        // first with "שלי" / "my" kept ("my documents" style phrases), then without the articles
        let (key, had_word) = self.settings_target(&toks, false).or_else(|| self.settings_target(&toks, true))?;
        if class.is_none() && !had_word {
            return None;
        }
        let mut c = cmd(caps::SYSTEM_OPEN_SETTINGS);
        c.slots.setting = Some(key);
        Some(c)
    }

    // ---- folders ----

    fn m_folders(&self, w: &[W], i: usize) -> Option<Cmd> {
        let (vn, class) = verb_class(self, w, i)?;
        if !matches!(class, VClass::Open | VClass::Launch | VClass::Go | VClass::Show) {
            return None;
        }
        // first with "שלי" / "my" kept ("המחשב שלי", "my documents"), then without the articles
        for strip_articles in [false, true] {
            let toks: Vec<&W> = w[i + vn..].iter().filter(|x| if strip_articles { !self.is_filler(&x.f) } else { !self.has("fillers", &x.f) }).collect();
            let t2: Vec<&W> = toks.iter().copied().filter(|x| !self.has("folder_words", &x.f) && !self.has_stem("folder_words", x)).collect();
            let had_folder_word = t2.len() < toks.len();
            if class == VClass::Show && !had_folder_word {
                continue;
            }
            if let Some(key) = self.cat_all("folders", &t2) {
                let mut c = cmd(caps::FOLDERS_OPEN);
                c.slots.folder = Some(key);
                return Some(c);
            }
        }
        None
    }

    // ---- websites ----

    fn m_web_open(&self, w: &[W], i: usize) -> Option<Cmd> {
        let (vn, class) = verb_class(self, w, i)?;
        if !matches!(class, VClass::Open | VClass::Launch | VClass::Go) {
            return None;
        }
        self.web_open_with(&w[i + vn..], false).or_else(|| self.web_open_with(&w[i + vn..], true))
    }

    /// `strip_articles`: also drop "the" / "my" / "שלי" in front of the name.
    fn web_open_with(&self, rest: &[W], strip_articles: bool) -> Option<Cmd> {
        let toks: Vec<&W> = rest.iter().filter(|x| if strip_articles { !self.is_filler(&x.f) } else { !self.has("fillers", &x.f) }).collect();
        if toks.is_empty() {
            return None;
        }
        let site = |key: String| {
            let mut c = cmd(caps::WEB_OPEN);
            c.slots.site = Some(key);
            Some(c)
        };
        if let Some(key) = self.cat_all("sites", &toks) {
            return site(key);
        }
        // "האתר של X", "X web": qualifiers removed; a program that is also a website needs one of them
        let t2: Vec<&W> = toks.iter().copied().filter(|x| !self.has("web_qualifiers", &x.f) && !self.has_stem("web_qualifiers", x)).collect();
        let had_web = t2.len() < toks.len();
        if t2.is_empty() {
            return None;
        }
        if let Some(key) = self.cat_all("sites", &t2) {
            return site(key);
        }
        if had_web {
            if let Some(key) = self.cat_all("web_sites", &t2) {
                return site(key);
            }
        }
        // a typed address (a Hebrew proclitic is dropped: "לexample.com")
        if t2.len() == 1 {
            let raw = t2[0].raw.as_str();
            let mut candidates = vec![raw.to_string()];
            if let Some(rest) = raw.strip_prefix(is_pre) {
                candidates.push(rest.to_string());
            }
            for cand in candidates {
                if cand.contains('.') || cand.contains("://") {
                    if let Some(url) = sys::normalize_web_url(&cand) {
                        let mut c = cmd(caps::WEB_OPEN);
                        c.slots.url = Some(url);
                        return Some(c);
                    }
                }
            }
        }
        None
    }
}

// =============================================================================
// Talking to the assistant: help, greetings, thanks, "who are you"
// =============================================================================

/// A sentence this long is not small talk.
const MAX_TALK_WORDS: usize = 9;
/// The name the assistant answers to; in a sentence that is otherwise small talk it only says who is addressed
/// ("שלום יובל", "hi Yuval, what can you do").
const ASSISTANT_NAMES: [&str; 2] = ["יובל", "yuval"];

impl Lex {
    /// `Some(cap)` when the WHOLE sentence is small talk or a question about the assistant ("מה אתה יודע
    /// לעשות", "help", "שלום", "תודה", "מי אתה"). Whole sentence on purpose: "תעזור לי למצוא את הקובץ של
    /// התקציב", "help desk ticket", "שלום מדני" and "תודה על המייל" have more to say and stay with the
    /// normal engine. The sentence is compared as typed (minus a closing "בבקשה"), then without the assistant's
    /// name, then without the filler words of `talk_noise` that open it, then without all of them ("היי,
    /// תגיד, מה אתה יודע לעשות בכלל?").
    fn talk(&self, text: &str) -> Option<CapId> {
        let cleaned: String = clean(text).chars().take(MAX_CHARS).collect();
        let w = words(&cleaned);
        if w.is_empty() || w.len() > MAX_TALK_WORDS {
            return None;
        }
        let mut typed: Vec<&str> = w.iter().map(|x| x.f.as_str()).collect();
        while typed.len() > 1 && self.has("trailing_polite", typed[typed.len() - 1]) {
            typed.pop();
        }
        let nameless: Vec<&str> = typed.iter().copied().filter(|x| !ASSISTANT_NAMES.contains(x)).collect();
        // "hi, thank you so much": the greeting goes, the "so" that belongs to the phrase stays
        let opened: Vec<&str> = nameless.iter().copied().skip_while(|x| self.has("talk_noise", x)).collect();
        let bare: Vec<&str> = nameless.iter().copied().filter(|x| !self.has("talk_noise", x)).collect();
        const KINDS: [(&str, CapId); 4] = [
            ("talk_help", caps::ASSISTANT_HELP),
            ("talk_about", caps::ASSISTANT_ABOUT),
            ("talk_thanks", caps::ASSISTANT_THANKS),
            ("talk_hello", caps::ASSISTANT_HELLO),
        ];
        [typed, nameless, opened, bare]
            .iter()
            .map(|f| f.join(" "))
            .filter(|s| !s.is_empty())
            .find_map(|s| KINDS.iter().find(|(list, _)| self.in_list(list, &s)).map(|(_, cap)| *cap))
    }
}

/// Small talk and help, answered by `assistant::talk` from fixed text. Read-only, so it is an `Execute`.
fn talk(text: &str) -> Option<Interpretation> {
    let cap = lex().talk(text)?;
    Some(Interpretation { decision: Decision::Execute { cap }, slots: Slots::default(), confidence: 0.95, lang: detect_lang(text), follow_up: false, ranked: vec![(cap, 1.0)] })
}

fn looks_like_address(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '@' | '.' | '-' | '_' | '+'))
}

/// A recipient word without its "to" letter: "לדני" -> "דני", "ל-דני" -> "דני", "ל-לאה" -> "לאה".
fn name_of(w: &W) -> String {
    if w.pre == Some('ל') {
        w.raw.clone()
    } else {
        strip_lamed(&w.raw)
    }
}

/// "לתל" -> "תל", "ללאה" -> "לאה"; a word of two letters or less is left alone.
fn strip_lamed(word: &str) -> String {
    let mut cs = word.chars();
    match cs.next() {
        Some('ל') if word.chars().count() >= 3 => cs.collect(),
        _ => word.to_string(),
    }
}

// =============================================================================
// Entry points
// =============================================================================

/// Understand an explicit command, or return `None` and leave the text to the normal engine.
pub fn detect(text: &str, ctx: &Ctx, now: DateTime<Local>, _known: &Known) -> Option<Interpretation> {
    // An answer to a question asked here ("תתרגם" -> "שלום") comes before small talk: "שלום" is the text to translate.
    let Some(cmd) = lex().parse(text) else { return reply_to_question(text, ctx, now).or_else(|| talk(text)).or_else(|| web_topic(text)) };
    let decision = match cmd.ask {
        Some(ask) => Decision::Clarify { ask, cap: Some(cmd.cap) },
        None => Decision::Confirm { cap: cmd.cap },
    };
    Some(Interpretation {
        decision,
        slots: cmd.slots,
        confidence: 0.95,
        lang: detect_lang(text),
        follow_up: false,
        ranked: vec![(cmd.cap, 1.0)],
    })
}

/// The answer to a question this file asked ("מה לחפש?", "מה לרשום בפתק?", "מה לתרגם?"): the typed text
/// becomes the missing query and the command is offered again with everything else it had. Not an answer: a
/// text that opens with a verb or a question word (the user moved on to something else), or has no word.
fn reply_to_question(text: &str, ctx: &Ctx, now: DateTime<Local>) -> Option<Interpretation> {
    let last = ctx.last(now.timestamp_millis())?;
    let Decision::Clarify { ask: AskKind::Content, cap: Some(cap) } = last.decision else { return None };
    if !matches!(cap, caps::WEB_SEARCH | caps::NOTES_CREATE) {
        return None;
    }
    let l = lex();
    let cleaned: String = clean(text).chars().take(MAX_CHARS).collect();
    let w = words(&cleaned);
    if w.is_empty() || w.len() > MAX_WORDS || verb_class(l, &w, 0).is_some() || l.has("question_words", &w[0].f) {
        return None;
    }
    let query = text_of(&w, false)?;
    let mut slots = last.slots.clone();
    if slots.engine.as_deref() == Some("translate") && slots.lang_to.is_none() {
        slots.lang_to = Some(sys::default_translate_target(&query).to_string());
    }
    slots.query = Some(query);
    Some(Interpretation { decision: Decision::Confirm { cap }, slots, confidence: 0.9, lang: detect_lang(text), follow_up: true, ranked: vec![(cap, 1.0)] })
}

/// A question about the weather, a currency rate, the news or a sports result: nothing on this PC answers
/// it, so the engine must not guess a mail or file search for it ("שער הדולר", "תוצאות הכדורגל"). It is
/// `NoMatch`, and the assistant turns that into the offer to search the web. Texts that name something
/// local (a mail, a file, a meeting, a note...) are left to the normal engine.
fn web_topic(text: &str) -> Option<Interpretation> {
    let l = lex();
    let w = offerable_words(l, text)?;
    if topic_of(l, &w) == Fallback::Generic || local_hint(l, &w) {
        return None;
    }
    // "תחפש את תחזית המכירות", "find the exchange rate report", "תמצא את הדוח על המניות": a request to find
    // something is a search of this PC (the normal engine), whatever words it contains
    if l.phrase_near_start("find_verbs", &w, 4) {
        return None;
    }
    Some(Interpretation { decision: Decision::NoMatch, slots: Slots::default(), confidence: 0.0, lang: detect_lang(text), follow_up: false, ranked: Vec::new() })
}

/// The words name something of the user's own: a mail, a file, a meeting, a note, an app, "my ...", or a
/// question about their own day ("מה יש לי עם מכבי מחר", "do I have a meeting about stocks").
fn local_hint(l: &Lex, w: &[W]) -> bool {
    w.iter().any(|x| l.names_local(x)) || l.phrase_near_start("personal_markers", w, MAX_WORDS)
}

/// `Some(kind)` for a question that only the web answers (weather, a currency rate, the news, a sports
/// result) and that names nothing of the user's own: "מה שער הדולר", but not "מה יש לי עם מכבי מחר".
pub fn web_topic_kind(text: &str) -> Option<Fallback> {
    let l = lex();
    let w = offerable_words(l, text)?;
    let kind = topic_of(l, &w);
    (kind != Fallback::Generic && !local_hint(l, &w)).then_some(kind)
}

/// The words of a text that may be offered as a web search, or `None`: nothing to search (no letters), a
/// path or a program, a very long text, or a request to change / delete / send something.
fn offerable_words(l: &Lex, text: &str) -> Option<Vec<W>> {
    let cleaned: String = clean(text).chars().take(MAX_CHARS).collect();
    let t = cleaned.trim();
    if t.is_empty() || t.chars().count() > 200 || !t.chars().any(|c| c.is_alphabetic()) || t.contains('\\') || t.contains(":/") {
        return None;
    }
    let w: Vec<W> = t.split_whitespace().map(W::new).filter(|w| !w.f.is_empty()).collect();
    if w.is_empty() || w.len() > MAX_WORDS {
        return None;
    }
    if w.iter().take(3).any(|x| l.has("no_offer_verbs", &x.f) || l.has("no_offer_verbs", x.f.strip_prefix('ש').unwrap_or(&x.f))) {
        return None;
    }
    if w.iter().any(|x| x.raw.rsplit_once('.').map_or(false, |(stem, ext)| !stem.is_empty() && super::EXEC_EXTS.contains(&ext.to_lowercase().as_str()))) {
        return None;
    }
    Some(w)
}

/// Which topic the words are about (the first of weather, currency, news, sports that appears).
fn topic_of(l: &Lex, w: &[W]) -> Fallback {
    let mut found = [false; 4];
    for i in 0..w.len() {
        if let Some((_, kind, _)) = l.cat_at("fallback", w, i) {
            match kind.as_str() {
                "weather" => found[0] = true,
                "currency" => found[1] = true,
                "news" => found[2] = true,
                "sports" => found[3] = true,
                _ => {}
            }
        }
    }
    match found {
        [true, ..] => Fallback::Weather,
        [_, true, ..] => Fallback::Currency,
        [_, _, true, _] => Fallback::News,
        [_, _, _, true] => Fallback::Sports,
        _ => Fallback::Generic,
    }
}

/// What an unrecognised text is about, for the "search it on Google" offer of the NoMatch card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    Generic,
    Weather,
    Currency,
    News,
    Sports,
}

/// `Some` when a text that nothing recognised may be offered as a Google search. Not offered: nothing to
/// search (no letters), a path or a program, an address that is not a question, a very long text, or a
/// request to change / delete / send something (a web search would not be an answer to that).
pub fn fallback_kind(text: &str) -> Option<Fallback> {
    let l = lex();
    let w = offerable_words(l, text)?;
    Some(topic_of(l, &w))
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod commands_tests;
