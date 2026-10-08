//! Spelling correction for keywords: a weighted edit distance that knows how Hebrew is mistyped,
//! and the ranking of candidates (noisy channel: the cheapest edit wins, then the more frequent
//! word).
//!
//! Edit costs (all on folded text, so final letters are already regular):
//! - 0.5: letters that sound alike or are often swapped (ק/כ, ס/ש, ט/ת, א/ע, ח/כ, ב/ו, ו/י, א/ה),
//!   and dropping or adding a vowel letter (ו, י) or a silent א / ה ("מיל" -> "מייל");
//! - 0.75: a neighbour key on the standard Israeli keyboard (SI-1452), or two swapped letters
//!   ("קצבים" -> "קבצים");
//! - 1: any other edit.
//!
//! Word frequencies (`lexicon/freq_he.tsv`, Zipf scale) break ties between candidates and mark a
//! typed word as a real word. The file shipped holds no third-party data (see
//! `docs/HEBREW_ENGINE.md`); an empty table only means ties fall back to the lexicon order.

use std::collections::HashMap;
use std::sync::OnceLock;

const FREQ: &str = include_str!("lexicon/freq_he.tsv");

/// Pairs that are confused by sound, by habit or by their look (ד/ר, ה/ח).
const CONFUSABLE: [(char, char); 10] =
    [('ק', 'כ'), ('ס', 'ש'), ('ט', 'ת'), ('א', 'ע'), ('ח', 'כ'), ('ב', 'ו'), ('ו', 'י'), ('א', 'ה'), ('ד', 'ר'), ('ה', 'ח')];

/// The Hebrew keyboard rows, for neighbour keys. The final-letter keys are `_`: text is folded,
/// and a final letter typed mid-word is not the slip this models.
const ROWS: [&str; 3] = ["קראטו__פ", "שדגכעיחל__", "זסבהנמצת_"];

fn same_pair(a: char, b: char, pairs: &[(char, char)]) -> bool {
    pairs.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a))
}

/// Row and column of a key, final letters folded.
fn key_pos(c: char) -> Option<(usize, usize)> {
    let c = match c {
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        _ => c,
    };
    // the top row holds two unshifted punctuation keys before ק on a real keyboard
    let offsets = [2usize, 0, 0];
    for (r, row) in ROWS.iter().enumerate() {
        if let Some(col) = row.chars().position(|k| k == c) {
            return Some((r, col + offsets[r]));
        }
    }
    None
}

fn neighbours(a: char, b: char) -> bool {
    match (key_pos(a), key_pos(b)) {
        (Some((ra, ca)), Some((rb, cb))) => (ra == rb && ca.abs_diff(cb) == 1) || (ra.abs_diff(rb) == 1 && (ca == cb || ca + 1 == cb || cb + 1 == ca)),
        _ => false,
    }
}

pub fn sub_cost(a: char, b: char) -> f32 {
    if a == b {
        0.0
    } else if same_pair(a, b, &CONFUSABLE) {
        0.5
    } else if neighbours(a, b) {
        0.75
    } else {
        1.0
    }
}

pub fn indel_cost(c: char) -> f32 {
    if matches!(c, 'ו' | 'י' | 'א' | 'ה') {
        0.5
    } else {
        1.0
    }
}

const SWAP_COST: f32 = 0.75;

/// Longest word (in letters) the correction looks at.
const MAX_WORD: usize = 16;

/// Weighted optimal-string-alignment distance, `None` when it exceeds `max`.
pub fn weighted(a: &[char], b: &[char], max: f32) -> Option<f32> {
    let (n, m) = (a.len(), b.len());
    // keywords are short; a fixed table keeps this allocation-free
    if n.abs_diff(m) as f32 * 0.5 > max || n >= MAX_WORD || m >= MAX_WORD {
        return None;
    }
    let mut d = [[0f32; MAX_WORD]; MAX_WORD];
    for i in 1..=n {
        d[i][0] = d[i - 1][0] + indel_cost(a[i - 1]);
    }
    for j in 1..=m {
        d[0][j] = d[0][j - 1] + indel_cost(b[j - 1]);
    }
    for i in 1..=n {
        let mut row_min = f32::MAX;
        for j in 1..=m {
            // a doubled letter ("מאתמולל", "היוםם") is as cheap as a vowel letter
            let del = if i > 1 && a[i - 1] == a[i - 2] { 0.5 } else { indel_cost(a[i - 1]) };
            let ins = if j > 1 && b[j - 1] == b[j - 2] { 0.5 } else { indel_cost(b[j - 1]) };
            let mut v = (d[i - 1][j] + del)
                .min(d[i][j - 1] + ins)
                .min(d[i - 1][j - 1] + sub_cost(a[i - 1], b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] && a[i - 1] != a[i - 2] {
                v = v.min(d[i - 2][j - 2] + SWAP_COST);
            }
            d[i][j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max + 1e-6 {
            return None;
        }
    }
    let v = d[n][m];
    (v <= max + 1e-6).then_some(v)
}

/// The largest correction allowed for a typed word of `len` letters. Short words allow only one
/// cheap edit, because every short word is close to many others.
pub fn max_cost(len: usize) -> f32 {
    match len {
        0..=2 => 0.0,
        3 => 0.5,
        4 => 0.75,
        5..=6 => 1.0,
        _ => 2.0,
    }
}

/// The first letters must agree, or be a cheap confusion: people rarely mistype the first letter.
pub fn first_letter_ok(a: char, b: char) -> bool {
    a == b || same_pair(a, b, &CONFUSABLE)
}

fn table() -> &'static HashMap<String, f32> {
    static T: OnceLock<HashMap<String, f32>> = OnceLock::new();
    T.get_or_init(|| {
        FREQ.lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let (w, z) = l.split_once('\t')?;
                let z: f32 = z.trim().parse().ok()?;
                Some((super::normalize::fold(w), z / 100.0))
            })
            .collect()
    })
}

/// Zipf frequency (log10 per billion words) of a folded word, when the table knows it.
pub fn zipf(norm: &str) -> Option<f32> {
    table().get(norm).copied()
}

/// A typed word this common is a real word and is not corrected without support from context.
pub const REAL_WORD_ZIPF: f32 = 3.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn hebrew_confusions_are_cheap() {
        assert_eq!(weighted(&cs("קצבימ"), &cs("קבצימ"), 1.0), Some(0.75));
        assert_eq!(weighted(&cs("מיל"), &cs("מייל"), 0.5), Some(0.5));
        assert_eq!(weighted(&cs("פגיסה"), &cs("פגישה"), 1.0), Some(0.5));
        assert_eq!(weighted(&cs("היומ"), &cs("הימ"), 0.5), Some(0.5));
        // a far word is rejected early
        assert_eq!(weighted(&cs("תקציב"), &cs("פגישה"), 1.0), None);
    }

    #[test]
    fn keyboard_neighbours() {
        assert!(neighbours('ש', 'ד'));
        assert!(neighbours('ק', 'ר'));
        assert!(neighbours('ד', 'ז'));
        assert!(!neighbours('ש', 'ת'));
        assert_eq!(sub_cost('ג', 'ד'), 0.75);
        assert_eq!(sub_cost('מ', 'ת'), 1.0);
    }

    #[test]
    fn short_words_allow_only_cheap_edits() {
        assert_eq!(max_cost(3), 0.5);
        assert!(first_letter_ok('ק', 'כ'));
        assert!(!first_letter_ok('ק', 'מ'));
    }
}
