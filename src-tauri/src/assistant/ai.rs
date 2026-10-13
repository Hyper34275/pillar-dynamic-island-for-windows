//! The user's own AI server in smart search (see `llm` and docs/AI_API.md).
//!
//! Which questions it gets:
//! - mode `fallback`: what the built-in engine did not understand (`Decision::NoMatch`);
//! - mode `always`: also small talk and every question that read the user's data (calendar, mail,
//!   notes, files), the built-in search's results going with it when `shareResults` is on. Without
//!   them the AI could only guess, so the built-in answer stays.
//!
//! Never: a clarification, a request to open or run something (those wait for a click), a refused
//! action, a calculation (the built-in one is exact) or the help card. The built-in results stay on
//! the card either way, clickable as before; the AI only writes the words above them. When the AI
//! fails, the built-in answer is shown with the AI's error code.

use super::exec::{Group, Outcome, Sources};
use super::signals::Signals;
use super::wire::CardPhase;
use crate::intent::{caps, CapId, Decision, Lang};
use crate::llm::Mode;
use chrono::{DateTime, Local, TimeZone};

/// How the AI is set up, as `Sources::ai_setup` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub mode: Mode,
    pub share_results: bool,
}

/// What reaches the AI for one question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Send {
    /// The question alone.
    Bare,
    /// The question and the built-in results.
    WithResults,
}

/// Capabilities whose answer is the user's data.
fn reads_data(cap: CapId) -> bool {
    [
        caps::CALENDAR_LIST_EVENTS,
        caps::CALENDAR_SEARCH_EVENTS,
        caps::CALENDAR_CHECK_AVAILABILITY,
        caps::CALENDAR_RESOLVE_SHARED,
        caps::EMAIL_SEARCH,
        caps::NOTES_SEARCH,
        caps::FILES_SEARCH,
    ]
    .contains(&cap)
}

fn plan(setup: Setup, decision: &Decision, phase: CardPhase, signals: Signals) -> Option<Send> {
    if phase != CardPhase::Answer || signals.is_action() {
        return None;
    }
    match (decision, setup.mode) {
        (Decision::NoMatch, _) => Some(Send::Bare),
        (_, Mode::Fallback) => None,
        (Decision::Execute { cap }, Mode::Always) if *cap == caps::ASSISTANT_HELP => None,
        (Decision::Execute { cap }, Mode::Always) if caps::is_talk(*cap) => Some(Send::Bare),
        (Decision::Execute { cap }, Mode::Always) if reads_data(*cap) => setup.share_results.then_some(Send::WithResults),
        (Decision::MultiSource { caps }, Mode::Always) if caps.iter().all(|c| reads_data(*c)) => setup.share_results.then_some(Send::WithResults),
        _ => None,
    }
}

const MAX_RESULT_LINES: usize = 60;

fn when(ms: i64, end: Option<i64>, now: DateTime<Local>) -> String {
    let Some(t) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    let day = if t.date_naive() == now.date_naive() { "today".to_string() } else { t.format("%a %d/%m/%Y").to_string() };
    let mut s = format!("{day} {}", t.format("%H:%M"));
    if let Some(e) = end.and_then(|e| Local.timestamp_millis_opt(e).single()) {
        if e.date_naive() == t.date_naive() {
            s.push_str(&format!("-{}", e.format("%H:%M")));
        } else {
            s.push_str(&format!(" until {}", e.format("%a %d/%m %H:%M")));
        }
    }
    s
}

/// The built-in answer as plain lines for the model: its headline, then each group and its items.
pub fn results_text(o: &Outcome, now: DateTime<Local>) -> String {
    let mut lines = Vec::new();
    let head = [o.title.trim(), o.summary.trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" — ");
    if !head.is_empty() {
        lines.push(format!("The PC's own answer: {head}"));
    }
    let mut items = 0usize;
    for Group { kind, title, items: group_items, truncated, error_code, .. } in &o.groups {
        if *kind == "action" {
            continue;
        }
        let more = if *truncated { ", more not listed" } else { "" };
        let failed = error_code.as_deref().map(|c| format!(", failed: {c}")).unwrap_or_default();
        lines.push(format!("{kind} — {title} ({} items{more}{failed}):", group_items.len()));
        for (item, _) in group_items {
            if items == MAX_RESULT_LINES {
                lines.push("(more items not listed)".into());
                return lines.join("\n");
            }
            items += 1;
            let mut parts = Vec::new();
            if let Some(t) = item.time {
                parts.push(when(t, item.end_time, now));
            }
            parts.push(item.title.clone());
            if let Some(s) = item.subtitle.as_deref().filter(|s| !s.is_empty()) {
                parts.push(s.to_string());
            }
            if let Some(s) = item.source.as_deref().filter(|s| !s.is_empty()) {
                parts.push(format!("({s})"));
            }
            if item.unread {
                parts.push("[unread]".into());
            }
            lines.push(format!("- {}", parts.join(" · ")));
        }
    }
    if items == 0 {
        lines.push("Nothing was found.".into());
    }
    lines.join("\n")
}

fn unavailable(lang: Lang) -> &'static str {
    if lang == Lang::He {
        "שרת ה-AI לא ענה הפעם"
    } else {
        "The AI server did not answer this time"
    }
}

/// Let the AI write the answer when the settings and the question call for it. Returns the outcome
/// (with `ai` set when the AI's words are on it) and, when the AI was asked but failed, its error: the
/// caller adds it with [`note_failure`] once the built-in answer is complete.
pub fn augment(src: &dyn Sources, text: &str, lang: Lang, decision: &Decision, signals: Signals, mut o: Outcome, now: DateTime<Local>) -> (Outcome, Option<String>) {
    let setup = match src.ai_setup() {
        None => return (o, None),
        Some(Ok(setup)) => setup,
        Some(Err(e)) => {
            // switched on but the settings are incomplete or unreadable: a question only the AI could have
            // answered says so
            let failure = matches!(decision, Decision::NoMatch) && plan(Setup { mode: Mode::Fallback, share_results: false }, decision, o.phase, signals).is_some();
            return (o, failure.then_some(e));
        }
    };
    let Some(send) = plan(setup, decision, o.phase, signals) else { return (o, None) };
    let results = (send == Send::WithResults).then(|| results_text(&o, now));
    match src.ask_ai(text, lang == Lang::He, results.as_deref(), now) {
        Ok(answer) => {
            let (title, summary) = crate::llm::split_answer(&answer);
            o.title = title;
            o.summary = summary;
            o.ai = true;
            // the AI answered the question: the built-in "I did not understand" has nothing left to add
            if matches!(decision, Decision::NoMatch) {
                o.groups.clear();
            }
            (o, None)
        }
        Err(e) => (o, Some(e)),
    }
}

/// Say on the card that the AI did not answer, with its code.
pub fn note_failure(o: &mut Outcome, e: &str, lang: Lang) {
    let code = super::answer::code_of(e, "AI-002");
    let line = format!("{} ({code})", unavailable(lang));
    o.summary = if o.summary.trim().is_empty() { line } else { format!("{}\n{line}", o.summary.trim()) };
    if o.error_code.is_none() {
        o.error_code = Some(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::store::Target;
    use crate::assistant::wire::{AssistantItem, ItemKind};

    fn setup(mode: Mode, share: bool) -> Setup {
        Setup { mode, share_results: share }
    }

    #[test]
    fn what_goes_to_the_ai() {
        let s = Signals::default();
        let answer = CardPhase::Answer;
        let cal = Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS };
        assert_eq!(plan(setup(Mode::Fallback, true), &Decision::NoMatch, answer, s), Some(Send::Bare));
        assert_eq!(plan(setup(Mode::Fallback, true), &cal, answer, s), None, "fallback leaves understood questions alone");
        assert_eq!(plan(setup(Mode::Always, true), &cal, answer, s), Some(Send::WithResults));
        assert_eq!(plan(setup(Mode::Always, false), &cal, answer, s), None, "without the results the built-in answer is better");
        assert_eq!(plan(setup(Mode::Always, true), &Decision::Execute { cap: caps::ASSISTANT_HELLO }, answer, s), Some(Send::Bare));
        for cap in [caps::ASSISTANT_HELP, caps::CALCULATOR_EVALUATE, caps::APPS_LAUNCH, caps::WEB_SEARCH, caps::SYSTEM_LOCK, caps::NOTES_CREATE] {
            assert_eq!(plan(setup(Mode::Always, true), &Decision::Execute { cap }, answer, s), None, "{}", cap.as_str());
        }
        assert_eq!(plan(setup(Mode::Always, true), &Decision::Confirm { cap: caps::APPS_LAUNCH }, answer, s), None);
        assert_eq!(plan(setup(Mode::Always, true), &Decision::MultiSource { caps: vec![caps::EMAIL_SEARCH, caps::FILES_SEARCH] }, answer, s), Some(Send::WithResults));
        assert_eq!(plan(setup(Mode::Always, true), &Decision::NoMatch, CardPhase::Choices, s), None, "a clarification waits for the user");
        let refused = Signals { unsupported: Some("write"), ..Signals::default() };
        assert_eq!(plan(setup(Mode::Always, true), &Decision::NoMatch, answer, refused), None, "a refused action is not sent");
    }

    #[test]
    fn results_are_listed_for_the_model() {
        let now = Local.with_ymd_and_hms(2026, 10, 13, 8, 0, 0).unwrap();
        let start = Local.with_ymd_and_hms(2026, 10, 13, 10, 0, 0).unwrap().timestamp_millis();
        let end = Local.with_ymd_and_hms(2026, 10, 13, 11, 30, 0).unwrap().timestamp_millis();
        let item = AssistantItem {
            id: String::new(),
            kind: ItemKind::Event,
            title: "ישיבת צוות".into(),
            subtitle: Some("חדר 2".into()),
            time: Some(start),
            end_time: Some(end),
            accent: None,
            openable: true,
            unread: false,
            source: Some("יומן".into()),
        };
        let mut o = Outcome::answer("היום יש לך פגישה אחת", "");
        o.groups.push(Group { kind: "calendar", title: "יומן".into(), mailbox: None, items: vec![(item, Target::None)], truncated: false, error_code: None });
        let text = results_text(&o, now);
        assert!(text.starts_with("The PC's own answer: היום יש לך פגישה אחת"), "{text}");
        assert!(text.contains("calendar — יומן (1 items):"), "{text}");
        assert!(text.contains("- today 10:00-11:30 · ישיבת צוות · חדר 2 · (יומן)"), "{text}");
        assert!(results_text(&Outcome::answer("", ""), now).contains("Nothing was found."));
    }
}
