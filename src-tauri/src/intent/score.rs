//! Scoring and the decision table. Features come from the annotated tokens (only tokens that no
//! extraction pass has consumed count), each capability sums weighted features, and thresholds
//! on the best score and its margin to the runner-up decide between Execute, MultiSource,
//! Clarify, Confirm and NoMatch.
//!
//! All thresholds are constants here and were tuned on the `dev` half of the corpus; the
//! `holdout` half checks that they do not overfit.

use super::entities::Entities;
use super::lexicon::Ann;
use super::types::*;

/// Execute needs at least this score ...
pub const T_EXEC: f32 = 2.0;
/// ... below this nothing is understood; between the two a missing slot is asked for.
pub const T_LOW: f32 = 1.2;
/// Relative gap to the runner-up that makes the winner clear: (s1 - s2) / s1.
pub const MARGIN: f32 = 0.35;
/// A sensitive capability (open / launch) is offered for a click from this score on.
pub const T_CONFIRM: f32 = 2.0;
/// A typo match counts this much of an exact one.
pub const TYPO_FACTOR: f32 = 0.6;

#[derive(Clone, Debug, Default)]
pub struct Features {
    pub mail: bool,
    pub file: bool,
    pub note: bool,
    pub app: bool,
    pub cal: bool,
    pub meeting: bool,
    pub mailbox: bool,
    pub shared: bool,
    /// The noun was matched only through the typo tolerance.
    pub mail_typo: bool,
    pub cal_typo: bool,
    pub meeting_typo: bool,
    pub file_typo: bool,
    pub note_typo: bool,
    pub app_typo: bool,
    pub v_search: bool,
    pub v_show: bool,
    pub v_open: bool,
    pub v_launch: bool,
    pub v_calc: bool,
    pub veto: bool,
    pub q_what: bool,
    pub q_which: bool,
    pub q_when: bool,
    pub q_howmany: bool,
    pub yesh: bool,
    pub free: bool,
    pub has_time: bool,
    pub has_person: bool,
    pub has_with: bool,
    pub has_sender: bool,
    pub has_terms: bool,
    pub has_unread: bool,
    pub has_latest: bool,
    pub has_mailbox: bool,
    pub has_expr: bool,
    /// An application / format word (אקסל, pdf) or an explicit file extension.
    pub has_ext: bool,
    pub refers_back: bool,
}

impl Features {
    pub fn build(a: &[Ann], e: &Entities, has_time: bool, has_expr: bool, has_ext_word: bool) -> Features {
        let mut f = Features::default();
        let mut seen_exact: Vec<&str> = Vec::new();
        for t in a.iter().filter(|t| !t.used) {
            let Some(h) = &t.hit else { continue };
            if !h.typo {
                seen_exact.push(h.concept);
            }
            match h.concept {
                "N_MAIL" => f.mail = true,
                "N_FILE" => f.file = true,
                "N_NOTE" => f.note = true,
                "N_APP" => f.app = true,
                "N_CAL" => f.cal = true,
                "N_MEETING" => f.meeting = true,
                "N_MAILBOX" => f.mailbox = true,
                "N_SHARED" => f.shared = true,
                "V_SEARCH" => f.v_search = true,
                "V_SHOW" => f.v_show = true,
                "V_OPEN" => f.v_open = true,
                "V_LAUNCH" => f.v_launch = true,
                "V_CALC" => f.v_calc = true,
                "VETO" => f.veto = true,
                "Q_WHAT" => f.q_what = true,
                "Q_WHICH" => f.q_which = true,
                "Q_WHEN" => f.q_when = true,
                "Q_HOWMANY" => f.q_howmany = true,
                "YESH" => f.yesh = true,
                "FREE" => f.free = true,
                _ => {}
            }
        }
        f.mail_typo = f.mail && !seen_exact.contains(&"N_MAIL");
        f.cal_typo = f.cal && !seen_exact.contains(&"N_CAL");
        f.meeting_typo = f.meeting && !seen_exact.contains(&"N_MEETING");
        f.file_typo = f.file && !seen_exact.contains(&"N_FILE");
        f.note_typo = f.note && !seen_exact.contains(&"N_NOTE");
        f.app_typo = f.app && !seen_exact.contains(&"N_APP");
        // "מה שלח לי שרון": the question is about mail even if the word is not there
        f.mail = f.mail || e.implied_mail;
        f.has_time = has_time;
        f.has_person = e.person.is_some();
        f.has_with = !e.with_names.is_empty();
        f.has_sender = e.sender.is_some();
        f.has_terms = !e.terms.is_empty();
        f.has_unread = e.unread;
        f.has_latest = e.latest;
        f.has_mailbox = e.mailbox.is_some() || e.all_mailboxes;
        f.has_expr = has_expr;
        f.has_ext = has_ext_word || e.explicit_ext.is_some();
        f.refers_back = e.refers_back;
        f
    }

    fn other_noun(&self) -> bool {
        self.mail || self.file || self.note || self.app
    }

    fn any_noun(&self) -> bool {
        self.other_noun() || self.cal || self.meeting || self.mailbox || self.shared
    }
}

fn typo_w(base: f32, typo: bool) -> f32 {
    if typo {
        base * TYPO_FACTOR
    } else {
        base
    }
}

/// Score every capability; only those above zero are returned, best first (stable order on ties:
/// the order of `caps::ALL`).
pub fn candidates(f: &Features) -> Vec<(CapId, f32)> {
    let b = |x: bool| if x { 1.0f32 } else { 0.0 };
    let opening = f.v_open || f.v_launch;
    let mut out: Vec<(CapId, f32)> = Vec::new();
    let mut add = |cap: CapId, s: f32| {
        if s > 0.0 {
            out.push((cap, s));
        }
    };

    // ---- calendar ----
    let cal_noun = typo_w(1.2, f.cal_typo) * b(f.cal) + typo_w(1.4, f.meeting_typo) * b(f.meeting);
    let list = cal_noun
        + 1.4 * b(f.q_what && f.yesh)
        + 0.4 * b(f.yesh && !f.q_what)
        + 0.8 * b(f.yesh && !f.q_what && f.has_time)
        + 0.8 * b(f.has_person && f.q_what)
        + 0.6 * b(f.q_what && (f.cal || f.meeting))
        + 0.6 * b(f.q_which && f.meeting)
        + 0.8 * b(f.q_howmany && f.meeting)
        + 1.0 * b(f.has_time)
        + 0.6 * b(f.has_person)
        + 0.4 * b(f.v_show)
        - 3.0 * b(f.free)
        - 1.5 * b(f.has_terms || f.has_with)
        - 2.0 * b(f.shared)
        - 1.5 * b(f.other_noun())
        - 1.0 * b(opening)
        - 3.0 * b(f.has_expr);
    add(caps::CALENDAR_LIST_EVENTS, list);
    let avail = 2.8 * b(f.free) + 0.6 * b(f.has_time) + 0.3 * b(f.has_person) + 0.3 * b(f.q_when && f.free) - 1.5 * b(f.other_noun());
    add(caps::CALENDAR_CHECK_AVAILABILITY, if f.free { avail } else { 0.0 });
    let search = if (f.meeting || f.cal) && (f.has_terms || f.has_with) {
        typo_w(1.4, f.meeting_typo) * b(f.meeting)
            + typo_w(0.8, f.cal_typo) * b(f.cal)
            + 1.4
            + 0.8 * b(f.q_when)
            + 0.6 * b(f.v_search)
            + 0.4 * b(f.has_time)
            - 1.5 * b(f.other_noun())
            - 2.0 * b(f.free)
            - 1.0 * b(opening)
    } else {
        0.0
    };
    add(caps::CALENDAR_SEARCH_EVENTS, search);
    let shared = if f.shared && f.cal {
        0.6 + 2.2 + 0.6 * b(f.q_which || f.q_what || f.v_show) - 1.0 * b(f.has_time) - 2.0 * b(f.mail)
    } else {
        0.0
    };
    add(caps::CALENDAR_RESOLVE_SHARED, shared);

    // ---- mail ----
    let constraint = f.has_terms || f.has_sender || f.has_unread || f.has_latest || f.has_time || f.has_mailbox;
    let mail = if f.mail {
        typo_w(2.4, f.mail_typo)
            + 0.6 * b(f.v_search)
            + 0.4 * b(f.v_show)
            + 0.4 * b(constraint)
            + 0.3 * b(f.has_sender)
            - 1.5 * b(opening)
            - 0.5 * b(f.file || f.note)
    } else {
        0.0
    };
    add(caps::EMAIL_SEARCH, mail);
    let discover = if f.mailbox {
        2.6 + 0.6 * b(f.q_which || f.q_what || f.v_show || f.q_howmany) - 1.5 * b(f.mail) - 1.0 * b(f.has_terms) - 1.0 * b(f.v_search)
    } else {
        0.0
    };
    add(caps::EMAIL_DISCOVER_MAILBOXES, discover);

    // ---- files, notes, apps ----
    let file = if f.file || (f.has_ext && f.v_search && f.has_terms) {
        let base = if f.file { typo_w(2.4, f.file_typo) } else { 1.0 };
        base + 0.6 * b(f.v_search) + 0.4 * b(f.has_terms || f.has_ext || f.has_latest || f.has_time) - 1.5 * b(opening) - 0.5 * b(f.mail || f.note)
    } else {
        0.0
    };
    add(caps::FILES_SEARCH, file);
    let note = if f.note {
        typo_w(2.4, f.note_typo) + 0.6 * b(f.v_search) + 0.4 * b(f.v_show || f.has_terms || f.has_latest) - 1.5 * b(opening) - 0.5 * b(f.mail || f.file)
    } else {
        0.0
    };
    add(caps::NOTES_SEARCH, note);
    let app = if f.app { typo_w(2.4, f.app_typo) + 0.4 * b(f.v_show || f.v_search) - 2.0 * b(opening) } else { 0.0 };
    add(caps::APPS_SEARCH, app);

    // A search with no object noun at all ("תחפש את החשבונית") fits mail, files and notes equally.
    if !f.any_noun() && !f.has_expr && !f.free && !f.has_ext && (f.v_search || f.v_show) && f.has_terms && !f.has_time && !f.has_person {
        for cap in [caps::EMAIL_SEARCH, caps::FILES_SEARCH, caps::NOTES_SEARCH] {
            add(cap, 1.7);
        }
    }

    // ---- open / launch (never executed from text: the decision turns them into Confirm) ----
    add(caps::EMAIL_OPEN, if f.mail && opening { 3.8 } else { 0.0 });
    add(caps::FILES_OPEN, if f.file && opening { 3.8 } else { 0.0 });
    add(caps::NOTES_OPEN, if f.note && opening { 3.8 } else { 0.0 });
    let launch = if f.v_launch && !(f.mail || f.file || f.note) {
        2.8
    } else if f.v_open && !(f.mail || f.file || f.note || f.cal || f.meeting || f.mailbox) {
        2.4 + 0.4 * b(f.has_ext)
    } else {
        0.0
    } - 2.5 * b(f.refers_back);
    add(caps::APPS_LAUNCH, launch);

    // ---- calculator ----
    add(caps::CALCULATOR_EVALUATE, if f.has_expr { 3.6 + 0.4 * b(f.v_calc) + 0.2 * b(f.q_howmany) } else { 0.0 });

    out.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Whether `cap` may run with the slots found (a mail search with no constraint would list a
/// whole mailbox, so it asks what to look for instead).
pub fn needs_constraint(cap: CapId) -> bool {
    matches!(cap, caps::EMAIL_SEARCH | caps::FILES_SEARCH | caps::CALENDAR_SEARCH_EVENTS)
}

/// The question to ask when `cap` is the likely capability but not enough is known.
pub fn ask_for(cap: CapId) -> AskKind {
    match cap {
        caps::CALENDAR_LIST_EVENTS | caps::CALENDAR_CHECK_AVAILABILITY | caps::CALENDAR_SEARCH_EVENTS => AskKind::Date,
        caps::EMAIL_SEARCH | caps::FILES_SEARCH | caps::NOTES_SEARCH | caps::APPS_SEARCH | caps::APPS_LAUNCH => AskKind::Content,
        _ => AskKind::Intent,
    }
}

pub struct Effects {
    /// Invalid date / ambiguous hour found by `dates`.
    pub time_error: Option<AskKind>,
    pub multi_person: bool,
    pub multi_sender: bool,
    /// The query has something to search for (terms, sender, time, ...), per capability family.
    pub constraint: bool,
}

/// Turn the ranked candidates into a decision.
pub fn decide(f: &Features, cands: &[(CapId, f32)], eff: &Effects) -> Decision {
    if f.veto {
        return Decision::NoMatch;
    }
    // "open the meeting": there is nothing to open in a calendar from here
    if (f.v_open || f.v_launch) && (f.meeting || f.cal) && !(f.mail || f.file || f.note) {
        return Decision::NoMatch;
    }
    let Some(&(c1, s1)) = cands.first() else {
        return if f.v_search { Decision::Clarify { ask: AskKind::Content, cap: None } } else { Decision::NoMatch };
    };
    if s1 < T_LOW {
        return if f.v_search && !f.any_noun() { Decision::Clarify { ask: AskKind::Content, cap: None } } else { Decision::NoMatch };
    }
    if crate::intent::sensitivity(c1) != Sensitivity::Read {
        // "הפעל את הקובץ": opening something with nothing to say which one asks what, not a bare Confirm
        if matches!(c1, caps::EMAIL_OPEN | caps::FILES_OPEN | caps::NOTES_OPEN) && !eff.constraint && !f.refers_back {
            return Decision::Clarify { ask: AskKind::Content, cap: Some(c1) };
        }
        return if s1 >= T_CONFIRM { Decision::Confirm { cap: c1 } } else { Decision::Clarify { ask: AskKind::Content, cap: Some(c1) } };
    }
    let is_calendar = c1.as_str().starts_with("calendar.");
    if let Some(ask) = eff.time_error {
        if is_calendar || s1 >= T_LOW {
            return Decision::Clarify { ask, cap: Some(c1) };
        }
    }
    if eff.multi_person && is_calendar {
        return Decision::Clarify { ask: AskKind::Person, cap: Some(c1) };
    }
    if eff.multi_sender && c1 == caps::EMAIL_SEARCH {
        return Decision::Clarify { ask: AskKind::Person, cap: Some(c1) };
    }
    if needs_constraint(c1) && !eff.constraint {
        return Decision::Clarify { ask: AskKind::Content, cap: Some(c1) };
    }
    let close: Vec<CapId> = cands
        .iter()
        .filter(|(c, s)| *s >= T_LOW && (s1 - *s) / s1 < MARGIN && crate::intent::sensitivity(*c) == Sensitivity::Read)
        .map(|(c, _)| *c)
        .take(3)
        .collect();
    if close.len() >= 2 {
        // keep the registry order so the labels are stable
        let mut caps_sorted = close;
        const ORDER: [CapId; 3] = [caps::EMAIL_SEARCH, caps::FILES_SEARCH, caps::NOTES_SEARCH];
        caps_sorted.sort_by_key(|c| ORDER.iter().position(|x| x == c).unwrap_or(99));
        return Decision::MultiSource { caps: caps_sorted };
    }
    if s1 >= T_EXEC - 1e-4 {
        Decision::Execute { cap: c1 }
    } else if is_calendar && f.has_time {
        // the date is there, so "which date" would be a silly question
        Decision::Clarify { ask: AskKind::Intent, cap: Some(c1) }
    } else {
        Decision::Clarify { ask: ask_for(c1), cap: Some(c1) }
    }
}

/// 0..1 for logs and "did you mean" wording.
pub fn confidence(cands: &[(CapId, f32)]) -> f32 {
    let Some(&(_, s1)) = cands.first() else { return 0.0 };
    let s2 = cands.get(1).map_or(0.0, |c| c.1);
    let margin = ((s1 - s2) / s1).clamp(0.0, 1.0);
    ((s1 / 4.0).min(1.0) * margin.max(0.25).sqrt()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(f: &Features, constraint: bool) -> Decision {
        let c = candidates(f);
        decide(f, &c, &Effects { time_error: None, multi_person: false, multi_sender: false, constraint })
    }

    #[test]
    fn calendar_list_vs_availability() {
        let list = Features { q_what: true, yesh: true, has_time: true, ..Features::default() };
        assert_eq!(dec(&list, false), Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS });
        let free = Features { free: true, has_time: true, ..Features::default() };
        assert_eq!(dec(&free, false), Decision::Execute { cap: caps::CALENDAR_CHECK_AVAILABILITY });
        // "מה יש לי" with nothing else asks for the date
        let bare = Features { q_what: true, yesh: true, ..Features::default() };
        assert_eq!(dec(&bare, false), Decision::Clarify { ask: AskKind::Date, cap: Some(caps::CALENDAR_LIST_EVENTS) });
    }

    #[test]
    fn open_is_never_executed() {
        let f = Features { v_open: true, mail: true, has_latest: true, ..Features::default() };
        assert_eq!(dec(&f, true), Decision::Confirm { cap: caps::EMAIL_OPEN });
        let f = Features { v_launch: true, ..Features::default() };
        assert_eq!(dec(&f, false), Decision::Confirm { cap: caps::APPS_LAUNCH });
    }

    #[test]
    fn nounless_search_is_multi_source() {
        let f = Features { v_search: true, has_terms: true, ..Features::default() };
        assert_eq!(
            dec(&f, true),
            Decision::MultiSource { caps: vec![caps::EMAIL_SEARCH, caps::FILES_SEARCH, caps::NOTES_SEARCH] }
        );
    }

    #[test]
    fn veto_and_nothing() {
        let f = Features { veto: true, mail: true, ..Features::default() };
        assert_eq!(dec(&f, true), Decision::NoMatch);
        assert_eq!(dec(&Features::default(), false), Decision::NoMatch);
        let f = Features { v_search: true, ..Features::default() };
        assert_eq!(dec(&f, false), Decision::Clarify { ask: AskKind::Content, cap: None });
    }
}
