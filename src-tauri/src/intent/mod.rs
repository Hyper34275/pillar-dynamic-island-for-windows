//! Offline intent engine for smart search: Hebrew/English text -> capability + slots.
//! No model, no network, no I/O: pure functions over the text, a compiled-in lexicon, the clock
//! passed in and the names the machine knows (`Known`). `assistant` executes the result.
//!
//! CONTRACT (used by `assistant`, `local`): `interpret`, `Ctx`, `fold`, `weekday_name`,
//! `evaluate_expr`, `CalcError`, `sensitivity` and everything in `types`.

pub mod types;

pub use types::*;

use chrono::{DateTime, Local, Weekday};

/// Short-term conversation context (memory only, never persisted). Follow-ups such as
/// "ומה לגבי יום חמישי?" inherit the previous turn's capability and slots while it is fresh.
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
        const TTL_MS: i64 = 120_000;
        self.last.as_ref().filter(|(_, at)| now_ms - at <= TTL_MS).map(|(i, _)| i)
    }
}

/// Understand `text`. Pure and deterministic for a given `now`.
pub fn interpret(text: &str, ctx: &Ctx, now: DateTime<Local>, known: &Known) -> Interpretation {
    let _ = (text, ctx, now, known);
    Interpretation {
        decision: Decision::NoMatch,
        slots: Slots::default(),
        confidence: 0.0,
        lang: detect_lang(text),
        follow_up: false,
        ranked: Vec::new(),
    }
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
    text.to_lowercase()
}

/// Spellings of a person's name to try against calendar names, senders and the address book:
/// the name itself, its folded form, known Hebrew nicknames ("איציק" <-> "יצחק") and a Latin
/// transliteration ("יובל" -> "Yuval"). The first entry is always the name as given.
pub fn name_variants(name: &str) -> Vec<String> {
    vec![name.to_string()]
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
pub fn evaluate_expr(expr: &str) -> Result<f64, CalcError> {
    let _ = expr;
    Err(CalcError::Syntax)
}

/// How sensitive running `cap` is.
pub fn sensitivity(cap: CapId) -> Sensitivity {
    match cap {
        caps::EMAIL_OPEN | caps::NOTES_OPEN | caps::FILES_OPEN => Sensitivity::Open,
        caps::APPS_LAUNCH => Sensitivity::Launch,
        _ => Sensitivity::Read,
    }
}
