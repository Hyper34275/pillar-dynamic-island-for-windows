//! Short-term conversation context (memory only, never persisted) and the merge rules for
//! follow-ups ("ומה לגבי יום חמישי?") and refinements ("רק מהשבוע שעבר").

use super::dates::{local_at, DateParse};
use super::types::*;

/// Two minutes, per the contract; every remembered turn restarts it.
const TTL_MS: i64 = 120_000;

/// Short-term conversation context. Follow-ups inherit the previous turn's capability and slots
/// while it is fresh.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    last: Option<(Interpretation, i64)>,
}

impl Ctx {
    /// Remember a turn that was executed (or answered with a clarification) at `now_ms`.
    pub fn remember(&mut self, interpretation: &Interpretation, now_ms: i64) {
        self.last = Some((interpretation.clone(), now_ms));
    }

    pub fn clear(&mut self) {
        self.last = None;
    }

    /// The previous turn, if it is still fresh at `now_ms`.
    pub fn last(&self, now_ms: i64) -> Option<&Interpretation> {
        self.last.as_ref().filter(|(_, at)| (0..=TTL_MS).contains(&(now_ms - at))).map(|(i, _)| i)
    }
}

/// The capability a turn was about, if any (a clarification keeps the capability it asked for).
pub fn last_cap(last: &Interpretation) -> Option<CapId> {
    match &last.decision {
        Decision::Execute { cap } | Decision::Confirm { cap } => Some(*cap),
        Decision::Clarify { cap, .. } => *cap,
        Decision::MultiSource { caps } => caps.first().copied(),
        Decision::NoMatch => None,
    }
}

pub fn is_mail_cap(cap: CapId) -> bool {
    matches!(cap, caps::EMAIL_SEARCH | caps::EMAIL_OPEN | caps::EMAIL_DISCOVER_MAILBOXES)
}

/// The open capability that belongs to a search capability ("תפתח אותו" after a search).
pub fn open_of(cap: CapId) -> Option<CapId> {
    match cap {
        caps::EMAIL_SEARCH => Some(caps::EMAIL_OPEN),
        caps::FILES_SEARCH => Some(caps::FILES_OPEN),
        caps::NOTES_SEARCH => Some(caps::NOTES_OPEN),
        caps::APPS_SEARCH => Some(caps::APPS_LAUNCH),
        _ => None,
    }
}

/// The time of a follow-up. A part of the day or an hour on its own ("אחה"צ", "ב-15:00") applies
/// to the day of the previous turn, so it can answer "באיזו שעה?".
pub fn merge_time(prev: Option<&TimeSpec>, new: &DateParse) -> Option<TimeSpec> {
    if let (Some(prev), false, Some((s, e))) = (prev, new.has_date, new.tod) {
        let day = prev.from.date_naive();
        return Some(TimeSpec { from: local_at(day, s), to: local_at(day, e), grain: Grain::Range });
    }
    // "ומחרתיים?" after "אני פנוי מחר ב-11?": the same hours on the new day
    if let (Some(prev), Some(t), None) = (prev, &new.time, new.tod) {
        let same_day = prev.grain == Grain::Range && prev.from.date_naive() == (prev.to - chrono::Duration::minutes(1)).date_naive();
        if same_day && t.grain == Grain::Day {
            let (s, e) = (prev.from.time(), prev.to - chrono::Duration::minutes(1));
            let mins = |h: u32, m: u32| h * 60 + m;
            use chrono::Timelike;
            let day = t.from.date_naive();
            return Some(TimeSpec { from: local_at(day, mins(s.hour(), s.minute())), to: local_at(day, mins(e.hour(), e.minute()) + 1), grain: Grain::Range });
        }
    }
    new.time.clone().or_else(|| prev.cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(decision: Decision) -> Interpretation {
        Interpretation { decision, slots: Slots::default(), confidence: 1.0, lang: Lang::He, follow_up: false, ranked: vec![] }
    }

    #[test]
    fn context_expires_after_two_minutes() {
        let mut c = Ctx::default();
        assert!(c.last(0).is_none());
        c.remember(&turn(Decision::Execute { cap: caps::CALENDAR_LIST_EVENTS }), 1_000);
        assert!(c.last(1_000 + 119_000).is_some());
        assert!(c.last(1_000 + 121_000).is_none());
        // a clock that went backwards does not keep it alive
        assert!(c.last(0).is_none());
        c.clear();
        assert!(c.last(1_000).is_none());
    }

    #[test]
    fn capability_of_a_turn() {
        assert_eq!(last_cap(&turn(Decision::Clarify { ask: AskKind::Date, cap: Some(caps::EMAIL_SEARCH) })), Some(caps::EMAIL_SEARCH));
        assert_eq!(last_cap(&turn(Decision::NoMatch)), None);
        assert_eq!(open_of(caps::EMAIL_SEARCH), Some(caps::EMAIL_OPEN));
        assert_eq!(open_of(caps::CALCULATOR_EVALUATE), None);
    }
}
