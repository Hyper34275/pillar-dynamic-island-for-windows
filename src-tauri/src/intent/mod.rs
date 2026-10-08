//! Offline intent engine for smart search: Hebrew/English text -> capability + slots.
//! No model, no network, no I/O: pure functions over the text, a compiled-in lexicon, the clock
//! passed in and the names the machine knows (`Known`). `assistant` executes the result.
//!
//! CONTRACT (used by `assistant`, `local`): `interpret`, `Ctx`, `fold`, `weekday_name`,
//! `evaluate_expr`, `CalcError`, `sensitivity`, `name_variants` and everything in `types`.
//!
//! Pipeline (one pass, microseconds): `normalize` (tokens) -> `lexicon` (concepts, prefixes,
//! typo tolerance for keywords) -> `calc` / `dates` (numbers and times) -> `entities` (people,
//! mailbox, terms) -> `score` (features, candidates, decision table) -> `context` (follow-ups).
//!
//! Decisions, in short (details are next to the code that makes them):
//! - Only read-only capabilities are ever `Execute`d. Open / launch is always `Confirm`.
//! - `Clarify(Mailbox)` is never produced here: `assistant` knows the mailboxes and decides.
//! - "מה יש לי" with no time and no calendar word asks for the date; with a calendar word
//!   ("ביומן", "פגישות") or a person ("לאיציק") the day defaults to today.
//! - A fresh context turns "ומה ...", "רק ..." and bare dates / names into follow-ups.

pub mod analysis;
pub mod calc;
pub mod context;
pub mod dates;
pub mod entities;
pub mod lexicon;
pub mod normalize;
pub mod numwords;
pub mod score;
pub mod spell;
pub mod stem;
pub mod types;

#[cfg(test)]
mod corpus_tests;
#[cfg(test)]
mod eval_tests;

pub use analysis::{Analysis, Correction};
pub use context::Ctx;
pub use types::*;

use chrono::{DateTime, Duration, Local, Weekday};
use entities::Entities;

/// Longest query looked at; the rest is ignored (a pasted paragraph is not a question).
const MAX_QUERY_CHARS: usize = 400;

/// Executable / script extensions: a raw path or file of these in the text is never understood.
const EXEC_EXTS: [&str; 16] =
    ["exe", "bat", "cmd", "ps1", "msi", "lnk", "vbs", "vbe", "js", "jar", "com", "scr", "reg", "dll", "hta", "wsf"];
/// Program names that are not offered even as a launch candidate.
const SHELLS: [&str; 14] =
    ["cmd", "powershell", "pwsh", "wscript", "cscript", "regedit", "mshta", "rundll32", "bash", "wsl", "msiexec", "taskkill", "reg", "sc"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Lead {
    None,
    /// "ומה ...", "מה לגבי ...", "what about ...", "גם ...": continues the last turn.
    Follow,
    /// "רק ...", "only ...": narrows the last turn.
    Only,
}

fn lead_of(a: &[lexicon::Ann]) -> Lead {
    let Some(t0) = a.first() else { return Lead::None };
    if t0.is("M_ONLY") {
        return Lead::Only;
    }
    if t0.is("M_ALSO") || (t0.is("CONJ") && t0.norm() == "and") {
        return Lead::Follow;
    }
    if t0.hit.as_ref().map_or(false, |h| h.prefix.starts_with('ו')) {
        return Lead::Follow;
    }
    // "ובתמיכה?", "ולדנה?": a short question that starts with "and"
    if a.len() <= 3 && t0.hit.is_none() && t0.norm().starts_with('ו') && t0.norm().chars().count() >= 4 {
        return Lead::Follow;
    }
    if t0.is("Q_WHAT") || t0.norm() == "how" {
        if let Some(t1) = a.get(1) {
            if t1.is("P_REGARD") || t1.is("P_ABOUT") || t1.is("P_WITH") || t1.is("P_AT") {
                return Lead::Follow;
            }
        }
    }
    Lead::None
}

/// Understand `text`. Pure and deterministic for a given `now`.
pub fn interpret(text: &str, ctx: &Ctx, now: DateTime<Local>, known: &Known) -> Interpretation {
    analyze(text, ctx, now, known).0
}

/// [`interpret`], plus why: the data source, confidence parts, spelling corrections and reason
/// codes (no user text, safe to log).
pub fn analyze(text: &str, ctx: &Ctx, now: DateTime<Local>, known: &Known) -> (Interpretation, Analysis) {
    let mut an = Analysis::default();
    let i = understand(text, ctx, now, known, &mut an);
    an.finish(&i);
    (i, an)
}

fn understand(text: &str, ctx: &Ctx, now: DateTime<Local>, known: &Known, an: &mut Analysis) -> Interpretation {
    let lang = detect_lang(text);
    let last = ctx.last(now.timestamp_millis());
    let text: String = text.chars().take(MAX_QUERY_CHARS).collect();
    let make = |decision: Decision, slots: Slots, confidence: f32, follow_up: bool, ranked: Vec<(CapId, f32)>| Interpretation {
        decision,
        slots,
        confidence,
        lang,
        follow_up,
        ranked,
    };
    let nothing = || make(Decision::NoMatch, Slots::default(), 0.0, false, Vec::new());

    let tokens = normalize::tokenize(&text);
    if tokens.is_empty() || looks_like_path(&text) {
        return nothing();
    }
    let mut a = lexicon::annotate(&tokens);
    let lead = lead_of(&a);
    let mail_hint = last.and_then(context::last_cap).map_or(false, context::is_mail_cap);

    // numbers first: "12*(3+4)" must not be read as a date
    let expr = calc::extract_expr(&text, calc::has_calc_word(&text));
    let dp = if expr.is_some() { dates::DateParse::default() } else { dates::parse(&mut a, now) };
    let mut ent = entities::extract(&mut a, known, mail_hint);
    lone_name(&a, &mut ent, last, known);

    let ext = a
        .iter()
        .find(|t| !t.used && entities::ext_of(t.concept()).is_some())
        .and_then(|t| entities::ext_of(t.concept()))
        .map(String::from)
        .or_else(|| ent.explicit_ext.clone());
    let has_time = dp.time.is_some() || dp.error.is_some();
    let f = score::Features::build(&a, &ent, has_time, expr.is_some(), ext.is_some());
    let cands = score::candidates(&f);
    let eff = score::Effects {
        time_error: dp.error,
        multi_person: ent.persons > 1,
        multi_sender: ent.senders > 1,
        constraint: !ent.terms.is_empty()
            || !ent.with_names.is_empty()
            || ent.sender.is_some()
            || ent.unread
            || ent.latest
            || has_time
            || ext.is_some()
            || ent.mailbox.is_some()
            || ent.shared_mailbox
            || ent.all_mailboxes,
    };
    let mut decision = score::decide(&f, &cands, &eff);
    let own_best = cands.first().map_or(0.0, |c| c.1);
    let conf = score::confidence(&cands);
    let ranked: Vec<(CapId, f32)> = cands.iter().take(5).copied().collect();
    record(an, &a, &f, &cands, &ent, &dp);
    let app = app_text(&a);
    let own = Own { ent: &ent, dp: &dp, expr: expr.clone(), ext, app: app.clone(), now, replace_terms: lead == Lead::Follow };

    let mut slots = slots_for(&decision, &own);
    let mut follow_up = false;

    // "תפתח אותו": opens what the previous turn found (still only a Confirm)
    if f.refers_back && (f.v_open || f.v_launch) && !f.mail && !f.file && !f.note {
        if let Some((l, c)) = last.and_then(|l| context::last_cap(l).map(|c| (l, c))) {
            let target = context::open_of(c).or(if sensitivity(c) != Sensitivity::Read { Some(c) } else { None });
            if let Some(cap) = target {
                let mut s = l.slots.clone();
                s.refers_back = true;
                return make(Decision::Confirm { cap }, s, conf.max(0.6), true, ranked);
            }
        }
        return make(Decision::Clarify { ask: AskKind::Intent, cap: None }, Slots::default(), conf, false, ranked);
    }

    // ---- follow-ups and refinements ----
    let strong = matches!(decision, Decision::Execute { .. } | Decision::MultiSource { .. } | Decision::Confirm { .. }) && own_best >= score::T_EXEC - 1e-4;
    let same_cap = match (&decision, last.and_then(context::last_cap)) {
        (Decision::Execute { cap }, Some(l)) | (Decision::Confirm { cap }, Some(l)) => *cap == l,
        (Decision::MultiSource { caps }, Some(l)) => caps.contains(&l),
        _ => false,
    };
    let info = dp.time.is_some()
        || dp.tod.is_some()
        || dp.error.is_some()
        || ent.person.is_some()
        || ent.sender.is_some()
        || ent.unread
        || ent.mailbox.is_some()
        || ent.all_mailboxes
        || ent.shared_mailbox
        || ent.dont_know
        || ent.latest
        || own.ext.is_some();
    let has_terms = ent.explicit_terms || !ent.with_names.is_empty();
    // "ובקבצים?", "רק במיילים": the same search in another source
    let switch = {
        let objs: Vec<CapId> = [(f.mail, caps::EMAIL_SEARCH), (f.file, caps::FILES_SEARCH), (f.note, caps::NOTES_SEARCH)]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, c)| *c)
            .collect();
        let last_search = last.and_then(context::last_cap).filter(|c| matches!(*c, caps::EMAIL_SEARCH | caps::FILES_SEARCH | caps::NOTES_SEARCH));
        let multi_before = matches!(last.map(|l| &l.decision), Some(Decision::MultiSource { .. }));
        match objs[..] {
            [c] if lead != Lead::None && ent.terms.is_empty() && ent.sender.is_none() && ent.person.is_none() && (last_search.map_or(false, |l| l != c) || multi_before) => Some(c),
            _ => None,
        }
    };
    let content_reply = matches!(last.map(|l| &l.decision), Some(Decision::Clarify { ask: AskKind::Content, .. }));
    let use_merge = switch.is_some() || match lead {
        Lead::Only => true,
        Lead::Follow => !strong || same_cap,
        Lead::None => last.is_some() && own_best < score::T_LOW && !f.veto && (info || (content_reply && !ent.terms.is_empty())),
    };
    if use_merge {
        let allow_terms = has_terms || content_reply;
        let merged = last
            .filter(|_| info || allow_terms || switch.is_some())
            .and_then(|l| merge_followup(l, &own, allow_terms || (content_reply && !ent.terms.is_empty()), switch));
        match merged {
            Some((d, s)) => {
                decision = d;
                slots = s;
                follow_up = true;
            }
            None if lead != Lead::None => {
                decision = Decision::Clarify { ask: AskKind::Intent, cap: None };
                slots = Slots::default();
            }
            None => {}
        }
    } else if decision == Decision::NoMatch
        && !f.veto
        && !(f.v_open || f.v_launch || f.v_search || f.v_show || f.v_calc)
        && (dp.time.is_some() || ent.person.is_some() || ent.sender.is_some())
    {
        // a bare date or name with nothing to attach it to
        decision = Decision::Clarify { ask: AskKind::Intent, cap: None };
    }

    // ---- final checks on the decision ----
    if let Decision::Execute { cap } = decision {
        if sensitivity(cap) != Sensitivity::Read {
            decision = Decision::Confirm { cap };
        }
    }
    if let (Decision::Execute { cap } | Decision::Clarify { cap: Some(cap), .. }, Some(p)) = (&decision, slots.person.as_deref()) {
        if cap.as_str().starts_with("calendar.") && matches!(decision, Decision::Execute { .. }) && ambiguous_person(known, p) {
            decision = Decision::Clarify { ask: AskKind::Person, cap: Some(*cap) };
        }
    }
    if let Decision::Confirm { cap: caps::APPS_LAUNCH } = decision {
        match slots.app.as_deref() {
            None | Some("") => decision = Decision::Clarify { ask: AskKind::Content, cap: Some(caps::APPS_LAUNCH) },
            Some(name) if normalize::fold(name).split(' ').any(|w| SHELLS.contains(&w)) => {
                return nothing();
            }
            _ => {}
        }
    }
    make(decision, slots, conf, follow_up, ranked)
}

/// Fill the analysis from what the passes found (concept ids and fixed codes only).
fn record(an: &mut Analysis, a: &[lexicon::Ann], f: &score::Features, cands: &[(CapId, f32)], ent: &Entities, dp: &dates::DateParse) {
    if let Some(&(_, s1)) = cands.first() {
        let s2 = cands.get(1).map_or(0.0, |c| c.1);
        an.score = s1;
        an.margin = ((s1 - s2) / s1).clamp(0.0, 1.0);
    }
    for t in a {
        if let Some(h) = &t.hit {
            if h.typo {
                an.corrections.push(Correction { concept: h.concept, cost: h.cost, real_word: h.real_word });
            }
            if h.phrase {
                an.reason(format!("phrase:{}", h.concept));
            }
        }
    }
    let flags = [
        (f.mail, "noun:mail"),
        (f.file, "noun:file"),
        (f.note, "noun:note"),
        (f.app, "noun:app"),
        (f.cal, "noun:calendar"),
        (f.meeting, "noun:meeting"),
        (f.mailbox, "noun:mailbox"),
        (f.free, "free"),
        (f.v_search, "verb:search"),
        (f.v_show, "verb:show"),
        (f.v_open || f.v_launch, "verb:open"),
        (f.veto, "verb:write"),
        (f.q_what && f.yesh, "q:what-have"),
        (dp.time.is_some(), "time"),
        (dp.error.is_some(), "time:invalid"),
        (ent.person.is_some(), "person"),
        (ent.sender.is_some(), "sender"),
        (!ent.terms.is_empty(), "terms"),
        (ent.unread, "unread"),
        (ent.latest, "latest"),
        (ent.mailbox.is_some() || ent.all_mailboxes || ent.shared_mailbox, "mailbox"),
    ];
    for (on, code) in flags {
        if on {
            an.reason(code);
        }
    }
    an.exact_terms = ent.exact;
    an.shared_mailbox = ent.shared_mailbox;
    an.unsupported = a.iter().find_map(|t| t.is("VETO").then(|| lexicon::unsupported_kind(t.norm())));
}

/// What the current text found, for building slots.
struct Own<'a> {
    ent: &'a Entities,
    dp: &'a dates::DateParse,
    expr: Option<String>,
    ext: Option<String>,
    app: Option<String>,
    now: DateTime<Local>,
    /// "ומה עם/לגבי X": the new subject replaces the old one ("רק ..." narrows, so it adds).
    replace_terms: bool,
}

fn day_span(now: DateTime<Local>) -> TimeSpec {
    let today = now.date_naive();
    TimeSpec { from: dates::local_at(today, 0), to: dates::local_at(today + Duration::days(1), 0), grain: Grain::Day }
}

fn slots_for(decision: &Decision, o: &Own) -> Slots {
    match decision {
        Decision::Execute { cap } | Decision::Confirm { cap } | Decision::Clarify { cap: Some(cap), .. } => build_slots(*cap, o),
        Decision::MultiSource { caps } => {
            let mut s = build_slots(caps.first().copied().unwrap_or(caps::EMAIL_SEARCH), o);
            s.file_ext = o.ext.clone();
            s
        }
        _ => Slots::default(),
    }
}

/// The time window for searching things that already exist (mail, files, notes): a date that was
/// resolved forwards ("ביום שני", "6.10") means the past one.
fn past_time(o: &Own) -> Option<TimeSpec> {
    match (&o.dp.time, &o.dp.past) {
        (Some(t), Some(p)) if t.from > o.now => Some(p.clone()),
        (t, _) => t.clone(),
    }
}

/// Only the slots that belong to `cap`; the rest stay empty so the executor sees a clean request.
fn build_slots(cap: CapId, o: &Own) -> Slots {
    let e = o.ent;
    let mut s = Slots { refers_back: e.refers_back, ..Slots::default() };
    let terms_all = || {
        let mut t = e.terms.clone();
        for n in &e.with_names {
            t.push(name_variants(n));
        }
        t
    };
    match cap {
        caps::CALENDAR_LIST_EVENTS | caps::CALENDAR_CHECK_AVAILABILITY => {
            s.person = e.person.clone();
            s.time = o.dp.time.clone().or_else(|| Some(day_span(o.now)));
        }
        caps::CALENDAR_RESOLVE_SHARED => {
            s.person = e.person.clone();
        }
        caps::CALENDAR_SEARCH_EVENTS => {
            s.person = e.person.clone();
            s.terms = terms_all();
            s.time = o.dp.time.clone();
        }
        caps::EMAIL_SEARCH | caps::EMAIL_OPEN => {
            s.sender = e.sender.clone();
            s.terms = terms_all();
            // "ההתכתבות עם דני": the person mail was exchanged with
            if s.sender.is_none() && e.with_names.len() == 1 {
                s.sender = e.with_names.first().cloned();
                s.terms = e.terms.clone();
            }
            s.mailbox = e.mailbox.clone();
            s.all_mailboxes = e.all_mailboxes;
            s.latest = e.latest;
            s.limit = e.limit;
            s.unread = e.unread;
            s.time = past_time(o);
        }
        caps::FILES_SEARCH | caps::FILES_OPEN => {
            s.terms = terms_all();
            s.terms.extend(e.topic_nouns.iter().cloned());
            s.file_ext = o.ext.clone();
            s.latest = e.latest;
            s.limit = e.limit;
            s.time = past_time(o);
        }
        caps::NOTES_SEARCH | caps::NOTES_OPEN => {
            s.terms = terms_all();
            s.terms.extend(e.topic_nouns.iter().cloned());
            s.latest = e.latest;
            s.limit = e.limit;
            s.time = past_time(o);
        }
        caps::APPS_SEARCH => {
            s.terms = terms_all();
        }
        caps::APPS_LAUNCH => {
            s.app = o.app.clone();
        }
        caps::CALCULATOR_EVALUATE => {
            s.expr = o.expr.clone();
        }
        _ => {}
    }
    s
}

/// Continue the previous turn: same capability, its slots, with what the new text changes.
/// Time replaces time (a part of the day alone applies to the previous day), a person or sender
/// replaces the old one, unread / latest are switched on, terms are added.
fn merge_followup(last: &Interpretation, o: &Own, allow_terms: bool, switch: Option<CapId>) -> Option<(Decision, Slots)> {
    let cap = switch.or_else(|| context::last_cap(last))?;
    if sensitivity(cap) != Sensitivity::Read || cap == caps::CALCULATOR_EVALUATE {
        return None;
    }
    let e = o.ent;
    let mut s = last.slots.clone();
    s.refers_back = false;
    if switch.is_some() && !context::is_mail_cap(cap) {
        // mail-only filters do not travel to files / notes
        s.sender = None;
        s.unread = false;
        s.mailbox = None;
        s.all_mailboxes = false;
    }
    let calendar = cap.as_str().starts_with("calendar.");
    if o.dp.time.is_some() || o.dp.tod.is_some() {
        s.time = context::merge_time(last.slots.time.as_ref(), o.dp);
        if !cap.as_str().starts_with("calendar.") {
            if let (Some(t), Some(p)) = (&s.time, &o.dp.past) {
                if t.from > o.now {
                    s.time = Some(p.clone());
                }
            }
        }
    }
    if let Some(ask) = o.dp.error {
        return Some((Decision::Clarify { ask, cap: Some(cap) }, s));
    }
    if calendar {
        if let Some(p) = &e.person {
            s.person = Some(p.clone());
        } else if e.with_names.len() == 1 && cap != caps::CALENDAR_SEARCH_EVENTS {
            // "ומה עם משה": the calendar of the person now named
            s.person = e.with_names.first().cloned();
        }
        if cap != caps::CALENDAR_RESOLVE_SHARED && s.time.is_none() && cap != caps::CALENDAR_SEARCH_EVENTS {
            s.time = Some(day_span(o.now));
        }
    } else {
        if context::is_mail_cap(cap) || matches!(last.decision, Decision::MultiSource { .. }) {
            if let Some(x) = &e.sender {
                s.sender = Some(x.clone());
            }
            if e.unread {
                s.unread = true;
            }
            if let Some(m) = &e.mailbox {
                s.mailbox = Some(m.clone());
                s.all_mailboxes = false;
            }
            if e.all_mailboxes || e.dont_know {
                s.all_mailboxes = true;
                s.mailbox = None;
            }
        }
        if e.latest {
            s.latest = true;
            s.limit = e.limit;
        }
        if matches!(cap, caps::FILES_SEARCH) || matches!(last.decision, Decision::MultiSource { .. }) {
            if let Some(x) = &o.ext {
                s.file_ext = Some(x.clone());
            }
        }
    }
    if allow_terms {
        let mut add = e.terms.clone();
        let with_is_person = calendar && e.person.is_none() && e.with_names.len() == 1 && cap != caps::CALENDAR_SEARCH_EVENTS;
        if !with_is_person {
            for n in &e.with_names {
                add.push(name_variants(n));
            }
        }
        if o.replace_terms && !add.is_empty() {
            s.terms.clear();
        }
        for g in add {
            if !s.terms.contains(&g) {
                s.terms.push(g);
            }
        }
    }
    let decision = match &last.decision {
        Decision::MultiSource { caps } if switch.is_none() => Decision::MultiSource { caps: caps.clone() },
        _ => Decision::Execute { cap },
    };
    Some((decision, s))
}

/// The program name after an open / launch verb: the words that are not grammar.
fn app_text(a: &[lexicon::Ann]) -> Option<String> {
    let verb = a.iter().position(|t| t.is("V_OPEN") || t.is("V_LAUNCH"))?;
    let words: Vec<&str> = a[verb + 1..]
        .iter()
        .filter(|t| !t.tok.sym && (t.hit.is_none() || t.starts("A_") || (t.is("MARK") && t.norm() == "word")))
        .map(|t| t.raw())
        .collect();
    // "את המחשבון", "את הכרום": the article is grammar, not part of the name
    let words: Vec<String> = words
        .iter()
        .map(|w| match w.strip_prefix('ה') {
            Some(rest) if rest.chars().count() >= 3 && rest.chars().all(normalize::is_he) => rest.to_string(),
            _ => w.to_string(),
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// A question that is only a name. After a calendar turn "ולאיציק?" names whose calendar, after a
/// mail turn "ומיובל?" names the sender; with nothing before it a name the machine knows ("דנה")
/// becomes a person, so the engine asks what to do with it instead of giving up.
fn lone_name(a: &[lexicon::Ann], ent: &mut Entities, last: Option<&Interpretation>, known: &Known) {
    // the answer to "what should I search for?" is a word, not a name
    if matches!(last.map(|l| &l.decision), Some(Decision::Clarify { ask: AskKind::Content, .. })) {
        return;
    }
    let last_cap = last.and_then(context::last_cap);
    if ent.person.is_some() || ent.sender.is_some() || !ent.with_names.is_empty() {
        return;
    }
    let content: Vec<&lexicon::Ann> = a.iter().filter(|t| t.is_content()).collect();
    let others = a.iter().filter(|t| !t.is_content() && !t.tok.sym && !t.is("STOP")).count();
    let [t] = content[..] else { return };
    if others > 0 || t.tok.quoted {
        return;
    }
    let bare = t.norm().trim_start_matches(|c| matches!(c, 'ו' | 'ב' | 'ל'));
    if numwords::is_number_word(t.norm()) || numwords::is_number_word(bare) {
        return;
    }
    let mut name = t.raw().to_string();
    let he = |s: &str| s.chars().count() >= 2 && s.chars().all(normalize::is_he);
    let strip = |s: &str, c: char| s.strip_prefix(c).filter(|r| he(r)).map(String::from);
    let known_first = |n: &str| {
        let f = fold(n);
        known.calendars.iter().map(|c| c.name.as_str()).chain(known.people.iter().map(String::as_str)).any(|full| fold(full).split(' ').next() == Some(f.as_str()))
    };
    match last_cap {
        // only a name the machine knows, or one with the grammar of the question ("ולאיציק", "ומיובל")
        Some(cap) if cap.as_str().starts_with("calendar.") => {
            let and = strip(&name, 'ו');
            let to = strip(and.as_deref().unwrap_or(&name), 'ל');
            let n = to.clone().or(and.clone()).unwrap_or(name.clone());
            if to.is_some() || known_first(&n) {
                ent.person = Some(n);
                ent.terms.clear();
            }
        }
        Some(cap) if context::is_mail_cap(cap) => {
            let and = strip(&name, 'ו');
            let from = strip(and.as_deref().unwrap_or(&name), 'מ');
            let n = from.clone().or(and.clone()).unwrap_or(name.clone());
            if from.is_some() || known_first(&n) {
                ent.sender = Some(n);
                ent.terms.clear();
            }
        }
        Some(_) => {}
        None => {
            if known_first(&name) {
                ent.person = Some(name);
                ent.terms.clear();
            }
        }
    }
}

/// A Windows path or a program / script file in the text: never understood, never offered.
fn looks_like_path(text: &str) -> bool {
    if text.contains('\\') || text.contains(":/") {
        return true;
    }
    text.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        w.rsplit_once('.').map_or(false, |(stem, ext)| !stem.is_empty() && EXEC_EXTS.contains(&ext.to_lowercase().as_str()))
    })
}

/// More than one calendar the user can read fits the typed name (first name only, for example),
/// and none of them is the exact full name.
fn ambiguous_person(known: &Known, person: &str) -> bool {
    let variants: Vec<String> = name_variants(person).iter().map(|v| normalize::fold(v)).collect();
    let full = normalize::fold(person);
    let mut matches = 0;
    for c in &known.calendars {
        let name = normalize::fold(&c.name);
        if name == full {
            return false;
        }
        if name.split(|ch: char| !ch.is_alphanumeric()).any(|w| !w.is_empty() && variants.iter().any(|v| v == w)) {
            matches += 1;
        }
    }
    matches > 1
}

/// Hebrew if the text has any Hebrew letter, else English.
pub fn detect_lang(text: &str) -> Lang {
    if text.chars().any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c)) {
        Lang::He
    } else {
        Lang::En
    }
}

/// The comparison form of any text (notes, subjects, file names, app names): niqqud and bidi marks
/// removed, final letters folded, quotes unified, Latin lowercased, whitespace collapsed. Search
/// code in other modules uses this so matching behaves the same everywhere.
pub fn fold(text: &str) -> String {
    normalize::fold(text)
}

/// Spellings of a person's name to try against calendar names, senders and the address book:
/// the name itself, its folded form, known Hebrew nicknames ("איציק" <-> "יצחק") and a Latin
/// transliteration ("יובל" -> "Yuval"). The first entry is always the name as given.
pub fn name_variants(name: &str) -> Vec<String> {
    lexicon::name_variants(name)
}

/// "יום חמישי" / "Thursday".
pub fn weekday_name(day: Weekday, lang: Lang) -> &'static str {
    let i = day.num_days_from_sunday() as usize;
    match lang {
        Lang::He => ["ראשון", "שני", "שלישי", "רביעי", "חמישי", "שישי", "שבת"][i],
        Lang::En => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][i],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcError {
    Syntax,
    DivideByZero,
    TooLarge,
}

/// Evaluate an arithmetic expression without `eval`: `+ - * / ^ %`, parentheses, unary minus.
/// `x%` is `x/100`; in a sum the percent is of the left side (`200+10%` = 220). See `calc`.
pub fn evaluate_expr(expr: &str) -> Result<f64, CalcError> {
    calc::evaluate_expr(expr)
}

/// How sensitive running `cap` is.
pub fn sensitivity(cap: CapId) -> Sensitivity {
    match cap {
        caps::EMAIL_OPEN | caps::NOTES_OPEN | caps::FILES_OPEN => Sensitivity::Open,
        caps::APPS_LAUNCH => Sensitivity::Launch,
        _ => Sensitivity::Read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDateTime, TimeZone};

    fn now() -> DateTime<Local> {
        let ndt = NaiveDateTime::parse_from_str("2026-10-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        Local.from_local_datetime(&ndt).earliest().unwrap()
    }

    fn run(text: &str) -> Interpretation {
        interpret(text, &Ctx::default(), now(), &Known::default())
    }

    #[test]
    fn spec_sentences() {
        let i = run("מה יש לאיציק ביומן מחר?");
        assert_eq!(i.decision, Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS });
        assert_eq!(i.slots.person.as_deref(), Some("איציק"));
        assert_eq!(i.slots.time.unwrap().from.format("%Y-%m-%d").to_string(), "2026-10-09");
        let i = run("תמצא את המייל עם המילה תקציב.");
        assert_eq!(i.decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
        assert_eq!(i.slots.terms, vec![vec!["תקציב".to_string()]]);
        assert!(!i.slots.all_mailboxes && i.slots.mailbox.is_none());
    }

    #[test]
    fn follow_up_inherits_and_expires() {
        let mut ctx = Ctx::default();
        let first = interpret("אני פנוי מחר?", &ctx, now(), &Known::default());
        assert_eq!(first.decision, Decision::Execute { cap: caps::CALENDAR_CHECK_AVAILABILITY });
        ctx.remember(&first, now().timestamp_millis());
        let next = interpret("ומה לגבי יום חמישי?", &ctx, now() + Duration::seconds(20), &Known::default());
        assert_eq!(next.decision, Decision::Execute { cap: caps::CALENDAR_CHECK_AVAILABILITY });
        assert!(next.follow_up);
        assert_eq!(next.slots.time.unwrap().from.format("%Y-%m-%d").to_string(), "2026-10-08");
        let late = interpret("ומה לגבי יום חמישי?", &ctx, now() + Duration::seconds(300), &Known::default());
        assert_eq!(late.decision, Decision::Clarify { ask: AskKind::Intent, cap: None });
    }

    #[test]
    fn refinement_keeps_the_mail_search() {
        let mut ctx = Ctx::default();
        let first = interpret("תחפש לי את המייל האחרון מיובל שיש בו את המילה תשלום.", &ctx, now(), &Known::default());
        ctx.remember(&first, now().timestamp_millis());
        let r = interpret("רק מהשבוע שעבר", &ctx, now() + Duration::seconds(5), &Known::default());
        assert_eq!(r.decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
        assert!(r.follow_up);
        assert_eq!(r.slots.sender.as_deref(), Some("יובל"));
        assert_eq!(r.slots.terms, vec![vec!["תשלום".to_string()]]);
        assert_eq!(r.slots.time.unwrap().from.format("%Y-%m-%d").to_string(), "2026-09-27");
    }

    #[test]
    fn nothing_dangerous_is_understood() {
        for t in ["תמחק את כל המיילים", "מה מזג האוויר", "OPEN 'C:\\Windows\\system32\\cmd.exe'", "run setup.exe", "הפעל cmd", ""] {
            assert_eq!(run(t).decision, Decision::NoMatch, "{t}");
        }
        assert_eq!(run("פתח את אקסל").decision, Decision::Confirm { cap: caps::APPS_LAUNCH });
    }

    #[test]
    fn unhandled_write_verbs_are_not_searched() {
        // #36: cancel / move / forward / update used to run a search with the verb as a term
        for t in [
            "תבטל את הפגישה של מחר",
            "cancel my meeting tomorrow",
            "תזיז את הפגישה למחר",
            "העבר את המייל לדנה",
            "שלח מייל לדנה שאני מאחר",
            "תדחה את הפגישה למחר",
            "תעדכן את הפגישה של מחר",
            "שלח לי את הקובץ",
        ] {
            assert_eq!(run(t).decision, Decision::NoMatch, "{t}");
        }
        // "who sent" is the same word as the command, but a question
        assert_eq!(run("מה שלח לי שרון").decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
        // opening with nothing to say which one asks
        assert_eq!(run("הפעל את הקובץ").decision, Decision::Clarify { ask: AskKind::Content, cap: Some(caps::FILES_OPEN) });
        assert_eq!(run("פתח את המייל האחרון").decision, Decision::Confirm { cap: caps::EMAIL_OPEN });
    }

    #[test]
    fn daily_mail_phrases() {
        // #35
        let i = run("מיילים שלא קראתי");
        assert_eq!(i.decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
        assert!(i.slots.unread && i.slots.terms.is_empty());
        let i = run("מיילים עם קובץ מצורף מאתמול");
        assert_eq!(i.decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
        assert!(i.slots.terms.is_empty());
        assert_eq!(run("דואר נכנס").decision, Decision::Clarify { ask: AskKind::Content, cap: Some(caps::EMAIL_SEARCH) });
        assert_eq!(run("מי כתב לי היום").decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
    }

    #[test]
    fn long_and_odd_input_is_safe() {
        let long = "מה יש לי ".repeat(500);
        let _ = run(&long);
        let _ = run("\u{202E}\u{0000}\u{FFFF}'\"\"''(((");
        let _ = run(&"9".repeat(300));
    }

    #[test]
    fn a_first_name_matching_two_calendars_asks_who() {
        let n = |id: &str, name: &str| KnownName { id: id.into(), name: name.into() };
        let known = Known { calendars: vec![n("a", "דנה כהן"), n("b", "דנה לוי"), n("c", "איציק לוי")], ..Known::default() };
        let ask = interpret("מה יש לדנה מחר", &Ctx::default(), now(), &known);
        assert_eq!(ask.decision, Decision::Clarify { ask: AskKind::Person, cap: Some(caps::CALENDAR_LIST_EVENTS) });
        // a nickname finds the one calendar it belongs to
        let one = interpret("מה יש ליצחק מחר", &Ctx::default(), now(), &known);
        assert_eq!(one.decision, Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS });
        // two people at once
        let two = run("מה יש ליובל ולדנה מחר");
        assert_eq!(two.decision, Decision::Clarify { ask: AskKind::Person, cap: Some(caps::CALENDAR_LIST_EVENTS) });
    }

    #[test]
    fn mailbox_question_is_left_to_the_assistant() {
        // many mailboxes, no hint: the engine still executes; `assistant` asks which mailbox
        let known = Known {
            mailboxes: vec![KnownName { id: "1".into(), name: "A".into() }, KnownName { id: "2".into(), name: "B".into() }],
            ..Known::default()
        };
        let i = interpret("תחפש לי את המייל עם המילה חושב", &Ctx::default(), now(), &known);
        assert_eq!(i.decision, Decision::Execute { cap: caps::EMAIL_SEARCH });
        assert!(i.slots.mailbox.is_none() && !i.slots.all_mailboxes);
    }

    #[test]
    fn weekday_names() {
        assert_eq!(weekday_name(Weekday::Thu, Lang::He), "חמישי");
        assert_eq!(weekday_name(Weekday::Sun, Lang::En), "Sunday");
    }
}
