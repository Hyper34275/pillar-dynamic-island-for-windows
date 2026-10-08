//! What the language engine noticed besides the capability (`intent::Analysis`), in the form the
//! executors use: an exact word was asked for, only the shared mailboxes are meant, or the request
//! is out of scope and the reason is known.
//!
//! This is the one place that reads those signals. Today they come from `Analysis`; when the
//! intent owner adds `Slots::exact_terms` / `Slots::shared_mailbox` (so a follow-up inherits them),
//! only [`Signals::of`] changes.
//!
//! Also here: the whole-word test behind "המילה X" (hits that have the word first, never fewer hits).

use crate::intent::{self, stem, Analysis, Interpretation};

/// Small and `Copy`, so it travels inside `exec::Run` and a pending question.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    /// "המילה X", a quoted word: whole-word hits first. The search itself is never widened.
    pub exact_terms: bool,
    /// "בתיבה המשותפת": only the shared / delegate mailboxes.
    pub shared_mailbox: bool,
    /// Why a request the engine did not take is out of scope: "write" | "weather" | "news" |
    /// "translate" | "install" | "power". Only ever set on a `NoMatch`.
    pub unsupported: Option<&'static str>,
}

impl Signals {
    pub fn of(interp: &Interpretation, an: &Analysis) -> Signals {
        // the slots carry them across follow-ups ("רק מהתיבה המשותפת", then "ומאתמול?")
        Signals {
            exact_terms: an.exact_terms || interp.slots.exact_terms,
            shared_mailbox: an.shared_mailbox || interp.slots.shared_mailbox,
            unsupported: an.unsupported,
        }
    }

    /// A request the app never carries out (change, send, install, switch off): no web search is
    /// offered for it, because the web would not be an answer.
    pub fn is_action(&self) -> bool {
        matches!(self.unsupported, Some("write" | "install" | "power"))
    }
}

// =============================================================================
// Whole words
// =============================================================================

/// The words of a text in comparison form: folded, cut at anything that is not a letter or a digit.
fn words(text: &str) -> Vec<String> {
    intent::fold(text).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Is the typed word `term` this very word, ignoring the Hebrew proclitics (ו ה ב ל מ ש כ, up to
/// three, "והתקציב") that attach to it?
fn same_word(word: &str, term: &str) -> bool {
    word == term || stem::prefix_splits(word).iter().any(|(_, rest)| rest == term)
}

/// Does `needle` (one or more words) occur in `hay` as whole words, in a row?
fn contains_words(hay: &[String], needle: &[String]) -> bool {
    let n = needle.len();
    n > 0 && hay.len() >= n && hay.windows(n).any(|w| w.iter().zip(needle).all(|(h, t)| same_word(h, t)))
}

/// How many of the terms (the AND-ed groups of `Slots::terms`; any spelling of a group counts) the
/// text has as whole words. "תקציב" is whole in "תקציב 2027" and "בתקציב", not in "תקציבים".
pub fn whole_words(text: &str, terms: &[Vec<String>]) -> usize {
    let hay = words(text);
    terms.iter().filter(|alts| alts.iter().any(|alt| contains_words(&hay, &words(alt)))).count()
}

/// Reorder `items` so that those with more of the terms as whole words in their `text_of` come
/// first. Stable (the existing order, newest first for mail, stays within a rank); nothing is added
/// or dropped.
pub fn exact_first<T>(items: &mut [T], text_of: impl Fn(&T) -> &str, terms: &[Vec<String>]) {
    if terms.iter().all(|alts| alts.iter().all(|a| a.trim().is_empty())) {
        return;
    }
    items.sort_by_cached_key(|i| std::cmp::Reverse(whole_words(text_of(i), terms)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(alts: &[&str]) -> Vec<String> {
        alts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn signals_come_from_the_analysis_of_the_real_engine() {
        let ask = |text: &str| {
            let (i, an) = intent::analyze(text, &intent::Ctx::default(), chrono::Local::now(), &intent::Known::default());
            Signals::of(&i, &an)
        };
        assert_eq!(ask("מצא את המייל עם המילה תקציב"), Signals { exact_terms: true, shared_mailbox: false, unsupported: None });
        assert_eq!(ask("תחפש מייל על תקציב בתיבה המשותפת"), Signals { exact_terms: false, shared_mailbox: true, unsupported: None });
        assert_eq!(ask("תמחק את המייל מדנה"), Signals { exact_terms: false, shared_mailbox: false, unsupported: Some("write") });
        assert_eq!(ask("מה יש לי היום"), Signals::default());
        assert!(ask("delete the email from Dana").is_action());
        assert!(!Signals { unsupported: Some("weather"), ..Signals::default() }.is_action());
        assert!(!Signals { unsupported: Some("translate"), ..Signals::default() }.is_action());
    }

    #[test]
    fn a_word_is_whole_with_or_without_an_attached_proclitic() {
        let terms = vec![t(&["תקציב"])];
        for yes in ["תקציב", "תקציב 2027", "re: תקציב שנתי", "בתקציב", "והתקציב", "לתקציב", "שהתקציב אושר", "דוח (תקציב)", "Q3-תקציב", "תקציב."] {
            assert_eq!(whole_words(yes, &terms), 1, "{yes}");
        }
        for no in ["תקציבים", "תקציבי", "התקציבים", "מתקציב", "מתקצבים", "פתקציב", "budget", ""] {
            // "מתקציב" is "from the budget": a proclitic, so it IS the word
            let expect = usize::from(no == "מתקציב");
            assert_eq!(whole_words(no, &terms), expect, "{no}");
        }
    }

    #[test]
    fn latin_words_ignore_case_and_never_take_a_hebrew_prefix() {
        let terms = vec![t(&["Budget"])];
        assert_eq!(whole_words("the BUDGET file", &terms), 1);
        assert_eq!(whole_words("budget.xlsx", &terms), 1);
        assert_eq!(whole_words("budgets", &terms), 0);
        assert_eq!(whole_words("subbudget", &terms), 0);
    }

    #[test]
    fn a_phrase_is_whole_when_its_words_come_in_a_row() {
        let terms = vec![t(&["דוח רבעוני"])];
        assert_eq!(whole_words("הדוח הרבעוני של Q3", &terms), 1);
        assert_eq!(whole_words("דוח שנתי רבעוני", &terms), 0);
    }

    #[test]
    fn terms_count_per_group_and_any_spelling_of_a_group() {
        let terms = vec![t(&["התקציב", "תקציב"]), t(&["דנה"])];
        assert_eq!(whole_words("תקציב של דנה", &terms), 2);
        assert_eq!(whole_words("תקציב של איציק", &terms), 1);
        assert_eq!(whole_words("שום דבר", &terms), 0);
        assert_eq!(whole_words("תקציב", &[]), 0);
    }

    #[test]
    fn exact_first_is_stable_and_never_drops_or_adds() {
        let terms = vec![t(&["תקציב"])];
        let mut v = vec!["תקציבים 1", "תקציב א", "ללא", "תקציבי ב", "ב-תקציב ג"];
        exact_first(&mut v, |s| s, &terms);
        assert_eq!(v, ["תקציב א", "ב-תקציב ג", "תקציבים 1", "ללא", "תקציבי ב"]);
        // no usable term: untouched
        let mut w = vec!["b", "a"];
        exact_first(&mut w, |s| s, &[t(&[" "])]);
        assert_eq!(w, ["b", "a"]);
        exact_first(&mut w, |s| s, &[]);
        assert_eq!(w, ["b", "a"]);
    }
}
