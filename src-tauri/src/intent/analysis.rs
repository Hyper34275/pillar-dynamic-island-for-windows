//! Why the engine read a question the way it did: the data source, the confidence parts, the
//! spelling corrections and short reason codes. For logs and diagnostics, so it never holds the
//! user's text: corrections and reasons are concept ids and fixed codes only.
//!
//! `intent::analyze` returns it next to the `Interpretation`; `intent::interpret` drops it.

use super::types::*;
use serde::Serialize;

/// One keyword the engine read through the spelling correction.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    /// The concept the misspelt word was read as ("N_FILE").
    pub concept: &'static str,
    /// Weighted edit cost of the correction (0.5 per common Hebrew confusion, 1 per other edit).
    pub cost: f32,
    /// The typed word is itself a real word, so context had to support the correction.
    pub real_word: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    /// Where the answer comes from: "calendar" | "mail" | "files" | "notes" | "apps" | "calc", or
    /// several for a multi-source search. Empty when nothing runs.
    pub sources: Vec<&'static str>,
    /// Best capability score and the relative margin to the runner-up (the confidence inputs).
    pub score: f32,
    pub margin: f32,
    pub corrections: Vec<Correction>,
    /// Short fixed codes: "noun:mail", "verb:search", "time", "person", "phrase:N_CAL", ...
    pub reasons: Vec<String>,
    /// The user asked for an exact word ("המילה X", quotes, "בדיוק X"): rank whole-word matches
    /// first and do not widen the term.
    pub exact_terms: bool,
    /// "התיבה המשותפת" with no mailbox name: only the shared mailboxes.
    pub shared_mailbox: bool,
    /// Why a request is out of scope: "write" (delete / send / move ...), "weather", "news",
    /// "translate", "install", "power". `None` when it is in scope or simply not understood.
    pub unsupported: Option<&'static str>,
}

impl Analysis {
    pub(super) fn reason(&mut self, code: impl Into<String>) {
        let code = code.into();
        if !self.reasons.contains(&code) {
            self.reasons.push(code);
        }
    }

    /// Fill what follows from the final decision.
    pub(super) fn finish(&mut self, i: &Interpretation) {
        let caps: Vec<CapId> = match &i.decision {
            Decision::Execute { cap } | Decision::Confirm { cap } => vec![*cap],
            Decision::MultiSource { caps } => caps.clone(),
            _ => Vec::new(),
        };
        self.sources = Vec::new();
        for c in caps {
            let s = source_of(c);
            if !self.sources.contains(&s) {
                self.sources.push(s);
            }
        }
        if i.follow_up {
            self.reason("follow-up");
        }
        if !matches!(i.decision, Decision::NoMatch) {
            self.unsupported = None;
        }
        if i.slots.terms.is_empty() {
            self.exact_terms = false;
        }
    }
}

/// The data source behind a capability.
pub fn source_of(cap: CapId) -> &'static str {
    let s = cap.as_str();
    if s.starts_with("calendar.") {
        "calendar"
    } else if s.starts_with("email.") {
        "mail"
    } else if s.starts_with("files.") {
        "files"
    } else if s.starts_with("notes.") {
        "notes"
    } else if s.starts_with("apps.") {
        "apps"
    } else if s.starts_with("calculator.") {
        "calc"
    } else {
        "other"
    }
}
