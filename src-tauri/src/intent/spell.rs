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
//! Word frequencies (`lexicon/freq_he.tsv`, Zipf scale, derived from wordfreq by Robyn Speer, CC BY-SA
//! 4.0: see `lexicon/freq_he.LICENSE.txt`) break ties between candidates, mark a typed word as a
//! real word, and give misspelt search words a likely spelling ([`correct_term`]).

use std::collections::HashMap;
use std::sync::OnceLock;

const FREQ: &str = include_str!("lexicon/freq_he.tsv");
/// Office words (hand-written, `lexicon/office_words.txt`): correction targets and real words even
/// where general Hebrew rarely uses them ("חשבונית" is not in the top 50,000).
const OFFICE: &str = include_str!("lexicon/office_words.txt");
/// The frequency an office word counts with: common enough to win a tie against a rare listed word.
const OFFICE_ZIPF: f32 = 4.0;

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
    // vowel letters and the gutturals people drop ("שבו" for "שבוע")
    if matches!(c, 'ו' | 'י' | 'א' | 'ה' | 'ע') {
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
            // a letter missing at the very end ("אתמו") is a common slip too
            let ins = if j > 1 && b[j - 1] == b[j - 2] {
                0.5
            } else if i == n && j == m {
                indel_cost(b[j - 1]).min(0.75)
            } else {
                indel_cost(b[j - 1])
            };
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
        _ => 1.5,
    }
}

/// The first letters must agree, or be a cheap confusion: people rarely mistype the first letter.
pub fn first_letter_ok(a: char, b: char) -> bool {
    a == b || same_pair(a, b, &CONFUSABLE)
}

/// The first letters a keyword may have when the typed word starts with `c`.
pub fn first_letters(c: char) -> impl Iterator<Item = char> {
    std::iter::once(c).chain(CONFUSABLE.iter().filter_map(move |&(x, y)| if x == c { Some(y) } else if y == c { Some(x) } else { None }))
}

/// The frequency table, built on first use (the first question, off the UI thread).
struct Freq {
    /// Folded word -> Zipf (the highest when two spellings fold together).
    zipf: HashMap<String, f32>,
    /// Correction targets for search words, by (length, first letter): (folded letters, letter
    /// mask, the word as listed, Zipf). Only words common enough to be what someone meant.
    targets: HashMap<(usize, char), Vec<(Vec<char>, u32, String, f32)>>,
}

/// One bit per Hebrew letter (finals folded). One edit changes at most two bits, so two words
/// whose masks differ in more than `2 x edits` bits cannot be that close: a cheap filter before
/// the edit distance.
fn letter_mask(cs: &[char]) -> u32 {
    cs.iter().fold(0u32, |m, &c| {
        let i = (c as u32).wrapping_sub(0x05D0);
        if i < 27 {
            m | (1 << i)
        } else {
            m | (1 << 31)
        }
    })
}

/// A misspelt search word is only corrected towards a word at least this common.
const TARGET_ZIPF: f32 = 3.5;

fn table() -> &'static Freq {
    static T: OnceLock<Freq> = OnceLock::new();
    T.get_or_init(|| {
        let mut zipf: HashMap<String, f32> = HashMap::new();
        let mut targets: HashMap<(usize, char), Vec<(Vec<char>, u32, String, f32)>> = HashMap::new();
        let listed = FREQ.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).filter_map(|l| {
            let (w, z) = l.split_once('\t')?;
            Some((w, z.trim().parse::<f32>().ok()? / 100.0))
        });
        let office = OFFICE.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(|w| (w, OFFICE_ZIPF));
        for (w, z) in listed.chain(office) {
            let f = super::normalize::fold(w);
            if zipf.get(&f).map_or(true, |old| z > *old) {
                zipf.insert(f.clone(), z);
            }
            let cs: Vec<char> = f.chars().collect();
            let known = targets.get(&(cs.len(), cs.first().copied().unwrap_or(' '))).map_or(false, |v| v.iter().any(|t| t.0 == cs));
            if z >= TARGET_ZIPF && cs.len() >= 3 && cs.len() < MAX_WORD && !known {
                let mask = letter_mask(&cs);
                targets.entry((cs.len(), cs[0])).or_default().push((cs, mask, w.to_string(), z));
            }
        }
        Freq { zipf, targets }
    })
}

/// Zipf frequency (log10 per billion words) of a folded word, when the table knows it.
pub fn zipf(norm: &str) -> Option<f32> {
    table().zipf.get(norm).copied()
}

/// The likely spellings of a misspelt Hebrew search word ("ביתוח" -> "ביטוח", "התקצב" -> "התקציב";
/// "חשבונת" -> both "חשבונות" and "חשבונית", which are equally close), empty when the word is itself
/// listed (a real word is never "corrected"), is short, or no common word is one cheap slip away.
/// The caller adds them as alternatives: the word as typed is always searched too.
pub fn correct_term(word: &str) -> Vec<String> {
    let f = super::normalize::fold(word);
    let cs: Vec<char> = f.chars().collect();
    let n = cs.len();
    if n < 4 || n >= MAX_WORD || !cs.iter().all(|c| super::normalize::is_he(*c)) {
        return Vec::new();
    }
    let t = table();
    // listed as typed, or with its proclitics off ("והתקציב"): a real word
    if t.zipf.contains_key(&f) || super::stem::prefix_splits(&f).iter().any(|(_, rest)| t.zipf.contains_key(rest)) {
        return Vec::new();
    }
    let max = if n <= 5 { 0.75 } else { 1.0 };
    let mask = letter_mask(&cs);
    // (cost, zipf, word) of every candidate within the budget
    let mut found: Vec<(f32, f32, &str)> = Vec::new();
    for len in n - 1..=n + 1 {
        for first in first_letters(cs[0]) {
            for (form, m, listed, z) in t.targets.get(&(len, first)).into_iter().flatten() {
                if (mask ^ m).count_ones() > 4 {
                    continue;
                }
                if let Some(cost) = weighted(&cs, form, max) {
                    found.push((cost, *z, listed.as_str()));
                }
            }
        }
    }
    let Some(best) = found.iter().map(|c| c.0).fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.min(c)))) else { return Vec::new() };
    // the cheapest ones, most frequent first, at most two
    found.retain(|c| c.0 <= best + 1e-6);
    found.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    found.into_iter().take(2).map(|c| c.2.to_string()).collect()
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
    fn misspelt_search_words_get_the_common_spelling() {
        assert_eq!(correct_term("ביתוח"), vec!["ביטוח"]);
        // equally close: both, the more frequent first; an office word counts even when rare in general text
        let both = correct_term("חשבונת");
        assert!(both.contains(&"חשבונית".to_string()) && both.contains(&"חשבונות".to_string()), "{both:?}");
        assert_eq!(correct_term("פרוטוקל"), vec!["פרוטוקול"]);
        // real words and short words are left alone
        for w in ["חושב", "קצבים", "התקציב", "דנה", "budget"] {
            assert!(correct_term(w).is_empty(), "{w}");
        }
    }

    #[test]
    fn short_words_allow_only_cheap_edits() {
        assert_eq!(max_cost(3), 0.5);
        assert!(first_letter_ok('ק', 'כ'));
        assert!(!first_letter_ok('ק', 'מ'));
    }
}
