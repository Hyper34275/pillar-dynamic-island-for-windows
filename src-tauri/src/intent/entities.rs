//! Entity extraction: people, senders, mailboxes, search terms and the small flags
//! (latest / unread / limit / "it"). Works on annotated tokens after `dates` has consumed the
//! date words; every token an extraction uses is marked `used`, so it neither becomes a search
//! term nor counts as a feature in `score`.
//!
//! Names and search terms are never typo-corrected: they are taken as typed (a proclitic is
//! stripped where it is grammar: "מיובל" -> "יובל", "לאיציק" -> "איציק").

use super::lexicon::{self, Ann};
use super::normalize::{fold, is_he};
use super::stem::term_variants;
use super::types::Known;

#[derive(Clone, Debug, Default)]
pub struct Entities {
    /// The calendar owner ("מה יש לאיציק ביומן", "אצל דנה", "Dana's calendar").
    pub person: Option<String>,
    pub persons: usize,
    /// The mail sender ("מיובל", "from Dana").
    pub sender: Option<String>,
    pub senders: usize,
    /// "עם X": the other attendee of a meeting.
    pub with_names: Vec<String>,
    pub terms: Vec<Vec<String>>,
    /// The terms came from a marker (המילה / על / quotes), not from leftover words.
    pub explicit_terms: bool,
    pub mailbox: Option<String>,
    pub all_mailboxes: bool,
    /// "אני לא יודע" (the answer to the mailbox question).
    pub dont_know: bool,
    pub latest: bool,
    pub limit: Option<u32>,
    pub unread: bool,
    /// File extension of an explicit file name ("budget.xlsx").
    pub explicit_ext: Option<String>,
    pub refers_back: bool,
    /// "מה שלח לי X": no mail noun was said but the question is about mail.
    pub implied_mail: bool,
    /// An exact word was asked for ("המילה X", quotes, "בדיוק X").
    pub exact: bool,
    /// "התיבה המשותפת" with no mailbox name.
    pub shared_mailbox: bool,
    /// "עם הלקוח": a definite noun after "with" (a topic, which still makes "מתי X עם Y" a meeting).
    pub with_topic: bool,
    /// Meeting / calendar nouns that are the topic of a file, note or mail search ("המצגת מהישיבה").
    pub topic_nouns: Vec<Vec<String>>,
}

const FILE_EXTS: [&str; 14] =
    ["xlsx", "xls", "docx", "doc", "pdf", "pptx", "ppt", "txt", "csv", "msg", "eml", "zip", "png", "jpg"];

/// Extension implied by an application / format concept.
pub fn ext_of(concept: &str) -> Option<&'static str> {
    Some(match concept {
        "A_EXCEL" => "xlsx",
        "A_WORD" => "docx",
        "A_PPT" | "N_PPT" => "pptx",
        "A_PDF" => "pdf",
        "A_CSV" => "csv",
        "A_TXT" => "txt",
        _ => return None,
    })
}

fn strip_chars(s: &str, n: usize) -> String {
    s.chars().skip(n).collect()
}

/// "'s" possessive off an English name.
fn strip_poss(s: &str) -> &str {
    s.strip_suffix("'s").or_else(|| s.strip_suffix("\u{2019}s")).unwrap_or(s)
}

fn next_content(a: &[Ann], from: usize, skip_stop: bool) -> Option<usize> {
    let mut j = from;
    while j < a.len() {
        if a[j].is_content() {
            return Some(j);
        }
        if skip_stop && !a[j].used && a[j].is("STOP") {
            j += 1;
            continue;
        }
        return None;
    }
    None
}

/// A Hebrew name token without its "ל" (to) proclitic.
fn without_lamed(raw: &str) -> Option<String> {
    let mut cs = raw.chars();
    if cs.next() == Some('ל') && raw.chars().count() >= 3 {
        Some(strip_chars(raw, 1))
    } else {
        None
    }
}

/// "שלח", "כתבה" ...: the past tense of "wrote / sent" (also the imperative of שלח, so a caller
/// checks the context: a question word before it, or a relative marker).
fn is_wrote_verb(norm: &str) -> bool {
    matches!(norm, "שלח" | "שלחה" | "שלחו" | "כתב" | "כתבה" | "כתבו" | "ענה" | "ענתה" | "ענו" | "השיב" | "השיבה")
}

/// A first name (or full name) of a calendar or a person the machine knows, folded.
fn known_name(known: &Known, norm: &str) -> bool {
    known.calendars.iter().map(|c| c.name.as_str()).chain(known.people.iter().map(String::as_str)).any(|full| {
        let f = fold(full);
        f == norm || f.split(' ').next() == Some(norm)
    })
}

/// The first place where one of `names` appears in full (2+ words), the first word possibly with
/// a proclitic ("בחדר ישיבות", "לדנה כהן"): (start, token count, the name as typed).
fn full_name_at<'n>(a: &[Ann], names: impl Iterator<Item = &'n str>) -> Option<(usize, usize, String)> {
    for name in names {
        let words: Vec<String> = fold(name).split(' ').map(String::from).collect();
        if words.len() < 2 {
            continue;
        }
        for i in 0..a.len() {
            if i + words.len() > a.len() || a[i].used {
                continue;
            }
            let head = a[i].norm();
            let stripped = crate::intent::stem::prefix_splits(head).into_iter().find(|(_, rest)| *rest == words[0]);
            let first = if head == words[0] {
                a[i].raw().to_string()
            } else if let Some((p, _)) = stripped {
                strip_chars(a[i].raw(), p.chars().count())
            } else {
                continue;
            };
            if (1..words.len()).all(|k| !a[i + k].used && a[i + k].norm() == words[k]) {
                let rest: Vec<&str> = (1..words.len()).map(|k| a[i + k].raw()).collect();
                return Some((i, words.len(), format!("{first} {}", rest.join(" "))));
            }
        }
    }
    None
}

/// A name as the user wrote it, completed with the surname that follows it when together they are
/// a person or calendar the machine knows ("מירי אברהם"), and with a misspelt first name fixed when
/// exactly one known first name is one cheap edit away ("יבול" -> "יובל"). Nothing else changes:
/// a name is never corrected towards a word.
fn complete_name(a: &mut [Ann], known: &Known, name: &str) -> String {
    let folded = fold(name);
    let fulls: Vec<String> = known.calendars.iter().map(|c| fold(&c.name)).chain(known.people.iter().map(|p| fold(p))).collect();
    if let Some(i) = a.iter().position(|t| t.used && (t.raw() == name || t.raw().ends_with(name))) {
        if a.get(i + 1).map_or(false, |t| t.is_content()) {
            let joined = format!("{folded} {}", a[i + 1].norm());
            if fulls.iter().any(|f| *f == joined) {
                a[i + 1].used = true;
                return format!("{name} {}", a[i + 1].raw());
            }
        }
    }
    let cs: Vec<char> = folded.chars().collect();
    if cs.len() < 3 || !cs.iter().all(|c| is_he(*c)) || fulls.iter().any(|f| f.split(' ').next() == Some(folded.as_str())) {
        return name.to_string();
    }
    let originals: Vec<&str> = known.calendars.iter().map(|c| c.name.as_str()).chain(known.people.iter().map(String::as_str)).collect();
    let mut close: Vec<&str> = Vec::new();
    for full in &originals {
        let Some(first) = full.split(' ').next() else { continue };
        let fc: Vec<char> = fold(first).chars().collect();
        if crate::intent::spell::weighted(&cs, &fc, 0.75).is_some() && !close.iter().any(|c| fold(c) == fold(first)) {
            close.push(first);
        }
    }
    match close[..] {
        [one] => one.to_string(),
        _ => name.to_string(),
    }
}

fn push_unique(list: &mut Vec<String>, name: String) {
    let f = fold(&name);
    if !list.iter().any(|n| fold(n) == f) {
        list.push(name);
    }
}

pub fn extract(a: &mut [Ann], known: &Known, mail_hint: bool) -> Entities {
    let lex = lexicon::get();
    let mut e = Entities::default();
    let n = a.len();

    // ---- quoted phrases ----
    for t in a.iter_mut() {
        if t.tok.quoted && !t.used {
            e.terms.push(vec![t.tok.raw.clone()]);
            e.explicit_terms = true;
            e.exact = true;
            t.used = true;
        }
    }

    // ---- flags ----
    for i in 0..n {
        if a[i].used {
            continue;
        }
        if a[i].is("REFBACK") {
            e.refers_back = true;
            a[i].used = true;
        } else if a[i].is("UNREAD") {
            let word = a[i].norm() == "unread";
            let negated = i > 0 && a[i - 1].is("NOT");
            if word || negated {
                e.unread = true;
                a[i].used = true;
                if negated {
                    a[i - 1].used = true;
                }
            }
        } else if a[i].is("NOT") && i + 1 < n && matches!(a[i + 1].norm(), "יודע" | "יודעת" | "בטוח" | "בטוחה" | "ידוע") {
            e.dont_know = true;
            a[i].used = true;
            a[i + 1].used = true;
        } else if matches!(a[i].norm(), "don't" | "dont") && i + 1 < n && a[i + 1].norm() == "know" {
            e.dont_know = true;
            a[i].used = true;
            a[i + 1].used = true;
        } else if a[i].is("M_ONLY") || a[i].is("M_ALSO") {
            a[i].used = true;
        }
    }

    // "מיילים עם קובץ מצורף": an attribute of the mail, not a request for files
    if mail_hint || a.iter().any(|t| !t.used && t.is("N_MAIL")) {
        for i in 0..n.saturating_sub(1) {
            if !a[i].used && a[i].is("N_FILE") && !a[i + 1].used && (a[i + 1].norm().starts_with("מצורפ") || a[i + 1].norm().starts_with("attach")) {
                a[i].used = true;
            }
        }
    }

    // ---- latest / limit ----
    for i in 0..n {
        if a[i].used || !(a[i].is("M_LATEST_ONE") || a[i].is("M_LATEST_MANY")) {
            continue;
        }
        // "הפרויקט החדש": "new" right after a topic word describes the topic
        if i > 0 && a[i - 1].is_content() && a[i].norm().starts_with("חדש") || i > 0 && a[i - 1].is_content() && a[i].norm().starts_with("החדש") {
            a[i].hit = None;
            continue;
        }
        let many = a[i].is("M_LATEST_MANY");
        e.latest = true;
        a[i].used = true;
        let lo = i.saturating_sub(2);
        let hi = (i + 2).min(n - 1);
        let num = (lo..=hi).find(|&k| !a[k].used && !a[k].tok.sym && a[k].norm().chars().all(|c| c.is_ascii_digit()) && a[k].norm().len() <= 3);
        match num {
            Some(k) => {
                if let Ok(v) = a[k].norm().parse::<u32>() {
                    if (1..=500).contains(&v) {
                        e.limit = Some(v);
                        a[k].used = true;
                    }
                }
            }
            None if !many => e.limit = Some(1),
            None => {}
        }
    }

    // ---- "all mailboxes" and the cue words around a mailbox ----
    for i in 0..n {
        if a[i].used || !a[i].is("M_ALL") {
            continue;
        }
        let mailbox_after = (i + 1..(i + 4).min(n)).find(|&k| a[k].is("N_MAILBOX"));
        if let Some(k) = mailbox_after {
            e.all_mailboxes = true;
            a[i].used = true;
            a[k].used = true;
            if k + 1 < n && a[k + 1].norm() == "דואר" {
                a[k + 1].used = true;
            }
            for t in i + 1..k {
                if a[t].is("STOP") || a[t].is("SELF") || a[t].is("N_SHARED") {
                    a[t].used = true;
                }
            }
        } else if mail_hint && matches!(a[i].norm(), "כולנ" | "כולמ" | "all") {
            e.all_mailboxes = true;
            a[i].used = true;
        } else if !mail_hint {
            // "כל המיילים": plain quantifier
            a[i].used = true;
        }
    }

    // ---- named mailbox (matched against the real mailboxes only) ----
    if !known.mailboxes.is_empty() {
        let cue = a.iter().any(|t| t.is("N_MAILBOX"));
        let mut best: Option<(usize, &str, Vec<usize>)> = None;
        let mut tie = false;
        for mb in &known.mailboxes {
            let words: Vec<String> = fold(&mb.name)
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty() && !matches!(*w, "mailbox" | "תיבת" | "תיבה" | "shared" | "משותפת" | "inbox" | "-"))
                .map(String::from)
                .collect();
            if words.is_empty() || (words.len() < 2 && !cue) {
                continue;
            }
            let mut hits = Vec::new();
            for w in &words {
                let found = (0..n).find(|&k| {
                    !a[k].tok.sym && !hits.contains(&k) && {
                        let nm = a[k].norm();
                        nm == w || crate::intent::stem::prefix_splits(nm).iter().any(|(_, rest)| rest == w)
                    }
                });
                match found {
                    Some(k) => hits.push(k),
                    None => break,
                }
            }
            if hits.len() == words.len() {
                match &best {
                    Some((len, _, _)) if *len > words.len() => {}
                    Some((len, _, _)) if *len == words.len() => tie = true,
                    _ => {
                        best = Some((words.len(), &mb.id, hits));
                        tie = false;
                    }
                }
            }
        }
        // one distinctive word of a mailbox name after a mailbox cue ("בתיבת התמיכה" -> "תמיכה טכנית"),
        // or with a proclitic in a mail follow-up ("ובתמיכה?"), when only one mailbox has it
        if best.is_none() && (cue || mail_hint) {
            let mut found: Vec<(&str, usize)> = Vec::new();
            for mb in &known.mailboxes {
                for w in fold(&mb.name).split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3) {
                    let at = (0..n).find(|&k| {
                        let nm = a[k].norm();
                        !a[k].tok.sym && ((nm == w && cue) || crate::intent::stem::prefix_splits(nm).iter().any(|(_, rest)| rest == w))
                    });
                    if let Some(k) = at {
                        if !found.iter().any(|(id, _)| *id == mb.id.as_str()) {
                            found.push((&mb.id, k));
                        }
                    }
                }
            }
            if let [(id, k)] = found[..] {
                best = Some((1, id, vec![k]));
                tie = false;
            }
        }
        if let (Some((_, id, hits)), false) = (best, tie) {
            e.mailbox = Some(id.to_string());
            for k in hits {
                a[k].used = true;
            }
            for t in a.iter_mut() {
                if t.is("N_MAILBOX") || t.is("N_SHARED") {
                    t.used = true;
                }
            }
        }
    }
    // "התיבה המשותפת": a shared mailbox, none named
    if e.mailbox.is_none() && !e.all_mailboxes {
        let shared = (0..n).find(|&k| !a[k].used && a[k].is("N_SHARED"));
        let mailbox = (0..n).find(|&k| !a[k].used && a[k].is("N_MAILBOX"));
        if let (Some(s), Some(m)) = (shared, mailbox) {
            if s.abs_diff(m) <= 2 {
                e.shared_mailbox = true;
                a[s].used = true;
                a[m].used = true;
            }
        }
    }
    // "תיבת דואר": the second word is part of the noun
    for i in 0..n.saturating_sub(1) {
        if a[i].is("N_MAILBOX") && a[i + 1].norm() == "דואר" {
            a[i + 1].used = true;
        }
    }

    // ---- explicit file names ----
    for t in a.iter_mut() {
        if t.is_content() && !t.tok.quoted {
            if let Some((stem, ext)) = t.tok.norm.rsplit_once('.') {
                if !stem.is_empty() && FILE_EXTS.contains(&ext) {
                    e.terms.push(vec![t.tok.raw.clone()]);
                    e.explicit_ext = Some(ext.to_string());
                    e.explicit_terms = true;
                    t.used = true;
                }
            }
        }
    }

    // ---- marker words: "המילה X", "containing X" ----
    for i in 0..n {
        if a[i].used || !a[i].is("MARK") {
            continue;
        }
        a[i].used = true;
        let mut j = i + 1;
        while j < n && !a[j].used && a[j].is("STOP") {
            j += 1;
        }
        let mut taken = 0;
        let marked = |t: &Ann| !t.used && !t.tok.sym && (t.is_content() || (t.hit.is_some() && !t.is("MARK") && !t.starts("P_") && t.norm().chars().count() >= 2));
        while j < n && taken < 3 && (a[j].is_content() || (taken == 0 && marked(&a[j]))) {
            // the exact word, as typed: no proclitic-stripped variant
            e.terms.push(vec![a[j].raw().to_string()]);
            e.explicit_terms = true;
            e.exact = true;
            a[j].used = true;
            taken += 1;
            j += 1;
        }
    }

    // ---- topics: "על X", "בנושא X", "about X" ----
    for i in 0..n {
        let regard = a[i].is("P_REGARD") && i > 0 && !a[i - 1].is("Q_WHAT");
        if a[i].used || !(a[i].is("P_ABOUT") || regard) {
            continue;
        }
        let mut j = i + 1;
        let mut skipped = 0;
        while j < n && !a[j].used && a[j].is("STOP") && skipped < 2 {
            j += 1;
            skipped += 1;
        }
        if j >= n || a[j].used {
            continue;
        }
        let mut taken = 0;
        // the first word may be a noun ("notes about the meeting")
        if a[j].starts("N_") && !a[j].used {
            e.terms.push(term_variants(a[j].raw()));
            a[j].used = true;
            taken += 1;
            j += 1;
        }
        while j < n && taken < 4 && a[j].is_content() {
            e.terms.push(term_variants(a[j].raw()));
            a[j].used = true;
            taken += 1;
            j += 1;
        }
        if taken > 0 {
            e.explicit_terms = true;
            a[i].used = true;
        }
    }

    // ---- "עם X" (the other attendee) ----
    for i in 0..n {
        if a[i].used || !a[i].is("P_WITH") {
            continue;
        }
        if let Some(j) = next_content(a, i + 1, true) {
            let raw = a[j].raw().to_string();
            // "עם הקוד", "עם הלקוח": a definite noun is a topic, not a person
            let calendar_owner = !a.iter().any(|t| t.is("N_MEETING")) && known.calendars.iter().any(|c| fold(&c.name).split(' ').next() == Some(fold(&raw).as_str()));
            if raw.starts_with('ה') && raw.chars().count() >= 4 && !known_name(known, &fold(&raw)) {
                e.terms.push(term_variants(&raw));
                e.explicit_terms = true;
                e.with_topic = true;
            } else if calendar_owner && a.iter().any(|t| t.is("N_CAL") || t.is("YESH")) {
                // "מה קורה עם דנה מחר ביומן": a calendar the user reads, so whose day it is
                a[j].used = true;
                a[i].used = true;
                e.person.get_or_insert(raw);
                continue;
            } else {
                push_unique(&mut e.with_names, strip_poss(&raw).to_string());
            }
            a[j].used = true;
            a[i].used = true;
        }
    }

    // ---- the calendar owner ----
    let mut persons: Vec<String> = Vec::new();
    let mut last_person_idx: Option<usize> = None;
    if let Some(p) = e.person.take() {
        push_unique(&mut persons, p);
    }
    let calendar_talk = a.iter().any(|t| t.is("N_CAL") || t.is("N_MEETING") || t.is("YESH") || t.is("FREE") || t.is("P_AT"));
    if calendar_talk {
        // a calendar the user can read, named in full ("בחדר ישיבות", "של דנה כהן")
        if let Some((start, len, name)) = full_name_at(a, known.calendars.iter().map(|c| c.name.as_str())) {
            push_unique(&mut persons, name);
            for k in start..start + len {
                a[k].used = true;
            }
            last_person_idx = Some(start + len - 1);
        }
    }
    for i in 0..n {
        if a[i].used {
            continue;
        }
        let mut found: Option<(usize, String)> = None;
        if a[i].is("YESH") && matches!(a[i].norm(), "עושה" | "עושימ" | "עושות") && i > 0 && a[i - 1].is_content() {
            // "מה איציק עושה מחר", "מה דנה ואיציק עושים"
            let mut j = i - 1;
            if a[j].raw().starts_with('ו') && j > 0 && a[j - 1].is_content() {
                let second = strip_chars(a[j].raw(), 1);
                a[j].used = true;
                j -= 1;
                push_unique(&mut persons, second);
            }
            found = Some((j, a[j].raw().to_string()));
        } else if a[i].is("YESH") && a[i].hit.as_ref().map_or(false, |h| h.prefix.is_empty() || h.prefix == "ו") {
            if let Some(j) = next_content(a, i + 1, false) {
                if let Some(name) = without_lamed(a[j].raw()) {
                    found = Some((j, name));
                }
            }
            // English: "does Dana have"
            if found.is_none() && i >= 2 && a[i - 1].is_content() && matches!(a[i - 2].norm(), "does" | "did" | "will") {
                found = Some((i - 1, strip_poss(a[i - 1].raw()).to_string()));
            }
        } else if a[i].is("P_AT") {
            if let Some(j) = next_content(a, i + 1, false) {
                found = Some((j, a[j].raw().to_string()));
            }
        } else if (a[i].is("P_OF") || a[i].is("P_FOR")) && i > 0 && (a[i - 1].is("N_CAL") || a[i - 1].is("N_MEETING")) {
            if let Some(j) = next_content(a, i + 1, true) {
                found = Some((j, a[j].raw().to_string()));
            }
        } else if a[i].is_content() && a[i].tok.norm.ends_with("'s") && i + 1 < n && (a[i + 1].is("N_CAL") || a[i + 1].is("N_MEETING")) {
            found = Some((i, strip_poss(a[i].raw()).to_string()));
        } else if a[i].is("FREE") && i > 0 && a[i - 1].is_content() {
            found = Some((i - 1, a[i - 1].raw().to_string()));
        }
        if let Some((j, name)) = found {
            push_unique(&mut persons, name);
            a[j].used = true;
            last_person_idx = Some(j);
        }
    }
    // a second name joined with "ו" / and: "ליובל ולדנה"
    if let Some(j) = last_person_idx {
        if let Some(k) = next_content(a, j + 1, false) {
            let raw = a[k].raw().to_string();
            if raw.starts_with('ו') && raw.chars().count() >= 3 && is_he(raw.chars().nth(1).unwrap_or(' ')) {
                let rest = strip_chars(&raw, 1);
                push_unique(&mut persons, without_lamed(&rest).unwrap_or(rest));
                a[k].used = true;
            }
        } else if j + 2 < n && a[j + 1].is("CONJ") {
            if let Some(k) = next_content(a, j + 2, false) {
                push_unique(&mut persons, a[k].raw().to_string());
                a[k].used = true;
            }
        }
    }
    e.persons = persons.len();
    e.person = persons.into_iter().next();

    // ---- the mail sender ----
    let mut mail_ctx = mail_hint || a.iter().any(|t| !t.used && (t.is("N_MAIL") || t.is("P_FROM")));
    let mut senders: Vec<String> = Vec::new();

    // "מה שלח לי שרון", "מי כתב לי היום": a question about who wrote is a mail search. The verb is
    // taken here, otherwise it would count as a "send" command or as a search term.
    // "מה שרון שלחה לי", "מה דני כתב לי": the name between the question and the verb
    for i in 0..n.saturating_sub(2) {
        let asks = !a[i].used && (a[i].is("Q_WHAT") || matches!(a[i].norm(), "מי" | "who"));
        if asks && a[i + 1].is_content() && !a[i + 2].used && is_wrote_verb(a[i + 2].norm()) && !lex.is_not_name(a[i + 1].norm()) {
            push_unique(&mut senders, strip_poss(a[i + 1].raw()).to_string());
            a[i + 1].used = true;
            a[i + 2].used = true;
            e.implied_mail = true;
            mail_ctx = true;
            if i + 3 < n && a[i + 3].norm() == "לי" {
                a[i + 3].used = true;
            }
        }
    }
    for i in 0..n.saturating_sub(1) {
        let asks = !a[i].used && (a[i].is("Q_WHAT") || matches!(a[i].norm(), "מי" | "who"));
        if !asks || a[i + 1].used || !is_wrote_verb(a[i + 1].norm()) {
            continue;
        }
        a[i + 1].used = true;
        e.implied_mail = true;
        mail_ctx = true;
        let mut k = i + 2;
        if k < n && !a[k].used && a[k].norm() == "לי" {
            a[k].used = true;
            k += 1;
        }
        if k < n && a[k].is_content() {
            push_unique(&mut senders, strip_poss(a[k].raw()).to_string());
            a[k].used = true;
        }
    }
    // "אם דני רוזן ענה לי": a name right before the wrote / replied verb
    for i in 1..n {
        // ("שמוטי שלח" is the relative form below)
        if !a[i].used && is_wrote_verb(a[i].norm()) && a[i - 1].is_content() && !a[i - 1].norm().starts_with('ש') && a[i].hit.as_ref().map_or(true, |h| h.prefix.is_empty()) {
            let mut j = i - 1;
            // a surname: take the first name before it
            if j > 0 && a[j - 1].is_content() {
                j -= 1;
            }
            if !lex.is_not_name(a[j].norm()) {
                push_unique(&mut senders, a[j].raw().to_string());
                a[j].used = true;
                a[i].used = true;
                e.implied_mail = true;
                mail_ctx = true;
            }
        }
    }
    // "המייל שמוטי שלח", "מייל ששלח יובל": the relative form of the same thing
    if mail_ctx {
        for j in 0..n {
            if a[j].used {
                continue;
            }
            let rel_name = a[j].is_content() && !a[j].tok.quoted && a[j].norm().starts_with('ש') && a[j].raw().chars().count() >= 4;
            if rel_name && j + 1 < n && !a[j + 1].used && is_wrote_verb(a[j + 1].norm()) {
                let name = strip_chars(a[j].raw(), 1);
                if !lex.is_not_name(&fold(&name)) {
                    push_unique(&mut senders, name);
                    a[j].used = true;
                    a[j + 1].used = true;
                }
            } else if a[j].hit.as_ref().map_or(false, |h| h.concept == "VETO" && h.prefix.contains('ש')) && is_wrote_verb(a[j].norm().strip_prefix('ש').unwrap_or("")) {
                if let Some(k) = next_content(a, j + 1, false) {
                    push_unique(&mut senders, strip_poss(a[k].raw()).to_string());
                    a[k].used = true;
                    a[j].used = true;
                }
            }
        }
    }

    for i in 0..n {
        if a[i].used {
            continue;
        }
        // "המייל האחרון של יובל": של after the mail noun names the sender
        let of_mail = a[i].is("P_OF") && (1..=2).any(|d| i >= d && a[i - d].is("N_MAIL"));
        if a[i].is("P_FROM") || of_mail {
            if let Some(j) = next_content(a, i + 1, true) {
                push_unique(&mut senders, strip_poss(a[j].raw()).to_string());
                a[j].used = true;
                a[i].used = true;
            }
            continue;
        }
        // "מ-Avi", "מ דנה": the lone "מ" (the hyphen is a token of its own) takes the next word
        if mail_ctx && a[i].norm() == "מ" && !a[i].tok.sym && !a[i].tok.quoted {
            let mut j = i + 1;
            if j < n && a[j].tok.sym && a[j].norm() == "-" {
                j += 1;
            }
            if j < n && a[j].is_content() {
                push_unique(&mut senders, strip_poss(a[j].raw()).to_string());
                for t in i..=j {
                    a[t].used = true;
                }
                continue;
            }
        }
        if mail_ctx && a[i].is_content() && !a[i].tok.quoted {
            let norm = a[i].tok.norm.clone();
            let raw = a[i].raw().to_string();
            if !norm.starts_with('מ') || raw.chars().count() < 4 || lex.is_not_name(&norm) {
                continue;
            }
            // after a relative marker ("שאני מאחר", "שבו מוזכר") the word is not a sender
            if i > 0 && a[i - 1].hit.as_ref().map_or(false, |h| h.prefix.contains('ש') && h.concept != "H_MAIL") {
                continue;
            }
            // a name that itself starts with מ ("מוטי", "מיכאל") is the name, not "from" + name
            let whole_is_name = lex.is_known_name(&norm);
            let mut name = if whole_is_name { raw.clone() } else { strip_chars(&raw, 1) };
            if !whole_is_name && name.starts_with('ה') && name.chars().count() >= 4 {
                name = strip_chars(&name, 1);
            }
            let known_person = known.people.iter().any(|p| fold(p).split(' ').any(|w| w == fold(&name)));
            if !known_person && !whole_is_name && (lex.is_not_name(&fold(&name)) || lex.lookup(&fold(&name)).is_some()) {
                continue;
            }
            // "מו..." is the passive participle shape (מוזכר, מוסכם, מוכן), not "from ו...": a
            // sender with a vav-initial name is spelled out by the user with "מ-" or "מאת"
            if !known_person && !whole_is_name && norm.starts_with("מו") && name.starts_with('ו') {
                continue;
            }
            push_unique(&mut senders, name);
            a[i].used = true;
        }
    }
    e.senders = senders.len();
    e.sender = senders.into_iter().next().map(|s| complete_name(a, known, &s));
    e.person = e.person.take().map(|p| complete_name(a, known, &p));

    // ---- a meeting noun with "מ"/"של" or the article next to a file / note noun is a topic ----
    let object = a.iter().any(|t| t.is("N_FILE") || t.is("N_PPT") || t.is("N_NOTE") || t.starts("A_"));
    if object {
        for t in a.iter_mut() {
            if !t.used && t.is("N_MEETING") && t.hit.as_ref().map_or(false, |h| !h.prefix.is_empty()) {
                let rest = strip_chars(t.raw(), t.hit.as_ref().map_or(0, |h| h.prefix.chars().count()));
                e.topic_nouns.push(vec![t.raw().to_string(), rest]);
                t.used = true;
            }
        }
    }

    // ---- leftover content words are the search terms ----
    if e.terms.len() < 6 {
        let mut extra = Vec::new();
        for t in a.iter() {
            if t.is_content() && !t.tok.quoted {
                let digits = t.tok.norm.chars().all(|c| c.is_ascii_digit());
                if digits && t.tok.norm.len() < 3 {
                    continue;
                }
                extra.push(term_variants(t.raw()));
            }
        }
        for g in extra {
            if e.terms.len() < 6 {
                e.terms.push(g);
            }
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::lexicon::annotate;
    use crate::intent::normalize::tokenize;
    use crate::intent::types::KnownName;

    fn run(text: &str, known: &Known, mail_hint: bool) -> Entities {
        let mut a = annotate(&tokenize(text));
        // dates are consumed by dates::parse in the real pipeline
        extract(&mut a, known, mail_hint)
    }

    #[test]
    fn topic_and_marker_terms() {
        let e = run("תחפש את המייל שבו דיברו על התקציב", &Known::default(), false);
        assert_eq!(e.terms, vec![vec!["התקציב".to_string(), "תקציב".to_string()]]);
        let e = run("תמצא את המייל עם המילה תקציב", &Known::default(), false);
        assert_eq!(e.terms, vec![vec!["תקציב".to_string()]]);
        assert!(e.with_names.is_empty());
        let e = run("find the file named budget.xlsx", &Known::default(), false);
        assert_eq!(e.explicit_ext.as_deref(), Some("xlsx"));
        let e = run("notes about the meeting", &Known::default(), false);
        assert_eq!(e.terms, vec![vec!["meeting".to_string()]]);
    }

    #[test]
    fn sender_and_latest() {
        let e = run("תחפש לי את המייל האחרון מיובל שיש בו את המילה תשלום", &Known::default(), false);
        assert_eq!(e.sender.as_deref(), Some("יובל"));
        assert!(e.latest);
        assert_eq!(e.limit, Some(1));
        assert_eq!(e.terms, vec![vec!["תשלום".to_string()]]);
        let e = run("חפשי מייל מהמנהל", &Known::default(), false);
        assert_eq!(e.sender.as_deref(), Some("מנהל"));
        let e = run("email from Dana's containing invoice", &Known::default(), false);
        assert_eq!(e.sender.as_deref(), Some("Dana"));
        // two senders
        let e = run("תחפש מייל מיובל או מדנה", &Known::default(), false);
        assert_eq!(e.senders, 2);
        // a מ-noun is not a sender
        let e = run("תחפש מייל על מחיר", &Known::default(), false);
        assert!(e.sender.is_none());
    }

    #[test]
    fn ordinary_words_starting_with_mem_are_not_senders() {
        // #29
        for t in ["מייל שבו מוזכר פרויקט אלפא", "שלח מייל לדנה שאני מאחר", "מייל מסכם על הפגישה"] {
            assert!(run(t, &Known::default(), false).sender.is_none(), "{t}");
        }
        // a name that starts with מ is the name
        assert_eq!(run("מייל מוטי", &Known::default(), false).sender.as_deref(), Some("מוטי"));
        assert_eq!(run("מייל מיובל", &Known::default(), false).sender.as_deref(), Some("יובל"));
    }

    #[test]
    fn hyphenated_and_relative_senders() {
        // #30: the hyphen is a token of its own
        let e = run("מייל מ-Avi על התקציב", &Known::default(), false);
        assert_eq!(e.sender.as_deref(), Some("Avi"));
        assert_eq!(e.terms.len(), 1);
        assert_eq!(run("מייל מ-דנה", &Known::default(), false).sender.as_deref(), Some("דנה"));
        assert_eq!(run("המייל שמוטי שלח", &Known::default(), false).sender.as_deref(), Some("מוטי"));
        assert_eq!(run("מייל ששלח יובל", &Known::default(), false).sender.as_deref(), Some("יובל"));
        // #35: who wrote to me is a mail question even without the noun
        let e = run("מה שלח לי שרון", &Known::default(), false);
        assert_eq!(e.sender.as_deref(), Some("שרון"));
        assert!(e.implied_mail);
    }

    #[test]
    fn calendar_owner() {
        for (t, who) in [
            ("מה יש לאיציק ביומן", "איציק"),
            ("ומה אצל דנה", "דנה"),
            ("מה יש ליובל", "יובל"),
            ("היומן של איציק", "איציק"),
            ("האם איציק פנוי", "איציק"),
            ("Dana's calendar", "Dana"),
            ("what does Dana have", "Dana"),
            ("is Itzik free", "Itzik"),
        ] {
            let e = run(t, &Known::default(), false);
            assert_eq!(e.person.as_deref(), Some(who), "{t}");
        }
        assert!(run("מה יש לי היום", &Known::default(), false).person.is_none());
        assert!(run("אני פנוי", &Known::default(), false).person.is_none());
        assert_eq!(run("מה יש ליובל ולדנה", &Known::default(), false).persons, 2);
    }

    #[test]
    fn mailbox_matching_uses_real_names() {
        let known = Known {
            mailboxes: vec![
                KnownName { id: "mb1".into(), name: "Yuval Cohen".into() },
                KnownName { id: "mb2".into(), name: "מכירות".into() },
            ],
            ..Known::default()
        };
        let e = run("תחפש מייל על תקציב בתיבה המשותפת של מכירות", &known, false);
        assert_eq!(e.mailbox.as_deref(), Some("mb2"));
        assert_eq!(e.terms, vec![vec!["תקציב".to_string()]]);
        // a single-word name needs the mailbox cue
        let e = run("מייל על מכירות", &known, false);
        assert!(e.mailbox.is_none());
        let e = run("search mailbox yuval cohen for refund", &known, false);
        assert_eq!(e.mailbox.as_deref(), Some("mb1"));
        let e = run("חפש בכל התיבות", &known, false);
        assert!(e.all_mailboxes);
        let e = run("אני לא יודע", &known, true);
        assert!(e.dont_know);
    }

    #[test]
    fn flags() {
        assert!(run("מיילים שלא נקראו", &Known::default(), false).unread);
        assert!(run("unread mail", &Known::default(), false).unread);
        assert!(!run("מיילים נקראו", &Known::default(), false).unread);
        let e = run("show the last 5 emails", &Known::default(), false);
        assert_eq!((e.latest, e.limit), (true, Some(5)));
        assert!(run("תפתח אותו", &Known::default(), false).refers_back);
    }
}
