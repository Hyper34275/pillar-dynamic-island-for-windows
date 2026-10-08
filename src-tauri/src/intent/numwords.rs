//! Hebrew number words ("שלוש", "עשרים ושלוש", "חמש מאות", "אלף ומאתיים") for the calculator,
//! hours ("בשתיים") and days of the month ("בעשרים באוקטובר"). Works on folded words (final
//! letters regular), masculine and feminine forms, up to 999,999.
//!
//! Not recognised on purpose: "שני" (it is also Monday and "second"), "שנים" (years) except in
//! front of "עשר" ("שנים עשר" is 12), and ordinals ("ראשון" is Sunday).

use super::normalize::fold;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Unit(u64),
    /// עשר / עשרה: 10 alone, the teen part after a unit ("שלוש עשרה").
    Ten,
    Tens(u64),
    /// מאה 100, מאתיים 200.
    Hundred(u64),
    /// מאות after a unit: "חמש מאות".
    Hundreds,
    /// אלף
    Thousand,
    /// אלפים after a unit: "שלושת אלפים" is not supported, "שלושה אלפים" is.
    Thousands,
    /// אלפיים
    TwoThousand,
}

fn table() -> &'static HashMap<String, Kind> {
    static T: OnceLock<HashMap<String, Kind>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        let mut put = |forms: &[&str], k: Kind| {
            for f in forms {
                m.insert(fold(f), k);
            }
        };
        put(&["אחד", "אחת"], Kind::Unit(1));
        put(&["שניים", "שתיים", "שתים"], Kind::Unit(2));
        put(&["שלוש", "שלושה"], Kind::Unit(3));
        put(&["ארבע", "ארבעה"], Kind::Unit(4));
        put(&["חמש", "חמישה"], Kind::Unit(5));
        put(&["שש", "שישה"], Kind::Unit(6));
        put(&["שבע", "שבעה"], Kind::Unit(7));
        put(&["שמונה"], Kind::Unit(8));
        put(&["תשע", "תשעה"], Kind::Unit(9));
        put(&["עשר", "עשרה"], Kind::Ten);
        put(&["עשרים"], Kind::Tens(20));
        put(&["שלושים"], Kind::Tens(30));
        put(&["ארבעים"], Kind::Tens(40));
        put(&["חמישים"], Kind::Tens(50));
        put(&["שישים"], Kind::Tens(60));
        put(&["שבעים"], Kind::Tens(70));
        put(&["שמונים"], Kind::Tens(80));
        put(&["תשעים"], Kind::Tens(90));
        put(&["מאה"], Kind::Hundred(100));
        put(&["מאתיים"], Kind::Hundred(200));
        put(&["מאות"], Kind::Hundreds);
        put(&["אלף"], Kind::Thousand);
        put(&["אלפים"], Kind::Thousands);
        put(&["אלפיים"], Kind::TwoThousand);
        m
    })
}

fn lookup(w: &str, next: Option<&str>) -> Option<Kind> {
    if let Some(k) = table().get(w) {
        return Some(*k);
    }
    if w == fold("שנים") && next.map_or(false, |n| table().get(n) == Some(&Kind::Ten)) {
        return Some(Kind::Unit(2));
    }
    None
}

/// True if `w` (folded) is a number word on its own.
pub fn is_number_word(w: &str) -> bool {
    lookup(w, None).is_some()
}

/// The number the leading words spell, and how many words it takes. Words after the first may
/// carry the "ו" (and) prefix ("עשרים ושלוש"). `None` if the first word is not a number word.
pub fn parse(words: &[&str]) -> Option<(u64, usize)> {
    let mut total = 0u64;
    let mut cur = 0u64;
    let mut last: Option<Kind> = None;
    let mut unit = 0u64;
    let mut end = 0;
    for (i, raw) in words.iter().enumerate() {
        let mut conj = false;
        let mut kind = lookup(raw, words.get(i + 1).copied());
        if kind.is_none() && i > 0 {
            if let Some(rest) = raw.strip_prefix('ו') {
                kind = lookup(rest, words.get(i + 1).copied());
                conj = kind.is_some();
            }
        }
        let Some(kind) = kind else { break };
        let after_big = matches!(last, Some(Kind::Hundred(_)) | Some(Kind::Hundreds) | Some(Kind::Thousand) | Some(Kind::Thousands) | Some(Kind::TwoThousand));
        let after_tens = matches!(last, Some(Kind::Tens(_)));
        let ok = match kind {
            Kind::Unit(_) => last.is_none() || (conj && (after_big || after_tens)) || (!conj && matches!(last, Some(Kind::Thousand | Kind::Thousands | Kind::TwoThousand | Kind::Hundred(_)))),
            Kind::Ten => last.is_none() || (!conj && matches!(last, Some(Kind::Unit(_)))) || (conj && after_big),
            Kind::Tens(_) => last.is_none() || (after_big && (conj || matches!(last, Some(Kind::Thousand | Kind::Thousands | Kind::TwoThousand)))),
            Kind::Hundred(_) => last.is_none() || matches!(last, Some(Kind::Thousand | Kind::Thousands | Kind::TwoThousand)),
            Kind::Hundreds => !conj && matches!(last, Some(Kind::Unit(u)) if u >= 3),
            Kind::Thousand => last.is_none() || matches!(last, Some(Kind::Hundred(_) | Kind::Hundreds | Kind::Tens(_) | Kind::Unit(_) | Kind::Ten)),
            Kind::Thousands => !conj && matches!(last, Some(Kind::Unit(u)) if u >= 3),
            Kind::TwoThousand => last.is_none(),
        };
        if !ok {
            break;
        }
        match kind {
            Kind::Unit(u) => {
                cur += u;
                unit = u;
            }
            Kind::Ten => cur += 10,
            Kind::Tens(t) => cur += t,
            Kind::Hundred(h) => cur += h,
            Kind::Hundreds => cur = cur - unit + unit * 100,
            Kind::Thousand => {
                total += cur.max(1) * 1000;
                cur = 0;
            }
            Kind::Thousands => {
                total += cur * 1000;
                cur = 0;
            }
            Kind::TwoThousand => total += 2000,
        }
        last = Some(kind);
        end = i + 1;
    }
    (end > 0).then_some((total + cur, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Option<(u64, usize)> {
        let f = fold(s);
        let words: Vec<&str> = f.split(' ').collect();
        parse(&words)
    }

    #[test]
    fn spells_numbers() {
        assert_eq!(n("שלוש"), Some((3, 1)));
        assert_eq!(n("עשר"), Some((10, 1)));
        assert_eq!(n("אחת עשרה"), Some((11, 2)));
        assert_eq!(n("שתים עשרה"), Some((12, 2)));
        assert_eq!(n("שנים עשר"), Some((12, 2)));
        assert_eq!(n("עשרים ושלוש"), Some((23, 2)));
        assert_eq!(n("שלושים ואחד"), Some((31, 2)));
        assert_eq!(n("ארבעים ושתיים"), Some((42, 2)));
        assert_eq!(n("מאתיים"), Some((200, 1)));
        assert_eq!(n("חמש מאות"), Some((500, 2)));
        assert_eq!(n("מאה ועשרים"), Some((120, 2)));
        assert_eq!(n("אלף ומאתיים"), Some((1200, 2)));
        assert_eq!(n("שלושה אלפים וחמש מאות"), Some((3500, 4)));
        assert_eq!(n("אלפיים"), Some((2000, 1)));
    }

    #[test]
    fn stops_where_the_number_stops() {
        // two numbers in a row are two numbers
        assert_eq!(n("שלוש ארבע"), Some((3, 1)));
        assert_eq!(n("שבע פלוס שמונה"), Some((7, 1)));
        assert_eq!(n("עשרים ועוד חמש"), Some((20, 1)));
        // "שני" and "שנים" alone are not numbers (Monday, years)
        assert_eq!(n("שני"), None);
        assert_eq!(n("שנים"), None);
        assert_eq!(n("תקציב"), None);
    }
}
