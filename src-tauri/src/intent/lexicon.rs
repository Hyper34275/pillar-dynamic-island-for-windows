//! The compiled-in lexicon (`lexicon/*.json`, `include_str!`): surface form -> concept, plus the
//! name tables. Loaded once, so an offline install carries everything and nothing can fail at
//! runtime because a file is missing.
//!
//! A concept is a short id such as `N_MAIL` or `T_TOMORROW`; `score.rs` and `dates.rs` work on
//! concepts, never on surface words.

use super::normalize::{fold, is_he, Token};
use super::stem;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const WORDS: &str = include_str!("lexicon/words.json");
const TIME: &str = include_str!("lexicon/time.json");
const NAMES: &str = include_str!("lexicon/names.json");

/// A lexicon match for one token.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub concept: &'static str,
    /// Proclitic letters that were stripped ("ב" in "ביומן"), folded.
    pub prefix: String,
    /// Matched through the typo tolerance, not exactly.
    pub typo: bool,
}

/// A token with its lexicon match and a "used" flag that the extraction passes set when they have
/// consumed the token (so it no longer counts as a content word or as a feature).
#[derive(Clone, Debug)]
pub struct Ann {
    pub tok: Token,
    pub hit: Option<Hit>,
    pub used: bool,
}

impl Ann {
    pub fn is(&self, concept: &str) -> bool {
        self.hit.as_ref().map_or(false, |h| h.concept == concept)
    }
    pub fn starts(&self, pre: &str) -> bool {
        self.hit.as_ref().map_or(false, |h| h.concept.starts_with(pre))
    }
    pub fn concept(&self) -> &'static str {
        self.hit.as_ref().map_or("", |h| h.concept)
    }
    pub fn norm(&self) -> &str {
        &self.tok.norm
    }
    pub fn raw(&self) -> &str {
        &self.tok.raw
    }
    /// An unused word that is not in the lexicon (a candidate name or search term).
    pub fn is_content(&self) -> bool {
        !self.used && self.hit.is_none() && !self.tok.sym && (self.tok.quoted || self.tok.norm.chars().count() >= 2)
    }
}

pub struct Lexicon {
    map: HashMap<String, &'static str>,
    /// Typo candidates: (form, concept), grouped by first character.
    typo: HashMap<char, Vec<(Vec<char>, &'static str)>>,
    nick_groups: Vec<Vec<String>>,
    latin: Vec<(String, Vec<String>)>,
    not_names: HashSet<String>,
    dup: Vec<String>,
}

/// Concepts that the typo tolerance may select. Verbs, question words and names are left out on
/// purpose: a wrong guess there would change what the engine does, not just what it understands.
fn typo_eligible(concept: &str) -> bool {
    concept.starts_with("N_")
        || matches!(
            concept,
            "T_TODAY" | "T_TOMORROW" | "T_YESTERDAY" | "T_DAYAFTER" | "T_WEEK" | "T_MONTH" | "T_NEXT" | "FREE" | "M_LATEST_ONE"
        )
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn load_concepts(src: &str, map: &mut HashMap<String, &'static str>, dup: &mut Vec<String>) {
    let v: serde_json::Value = serde_json::from_str(src).unwrap_or_default();
    if let Some(obj) = v.as_object() {
        for (concept, forms) in obj {
            let concept: &'static str = leak(concept);
            for f in forms.as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
                let key = fold(f);
                if let Some(prev) = map.insert(key.clone(), concept) {
                    if prev != concept {
                        dup.push(format!("{key}: {prev} / {concept}"));
                    }
                }
            }
        }
    }
}

impl Lexicon {
    fn build() -> Lexicon {
        let mut map = HashMap::new();
        let mut dup = Vec::new();
        load_concepts(WORDS, &mut map, &mut dup);
        load_concepts(TIME, &mut map, &mut dup);
        let mut typo: HashMap<char, Vec<(Vec<char>, &'static str)>> = HashMap::new();
        for (form, concept) in &map {
            let cs: Vec<char> = form.chars().collect();
            if typo_eligible(concept) && cs.len() >= 4 && cs.iter().all(|c| c.is_alphabetic()) {
                typo.entry(cs[0]).or_default().push((cs, *concept));
            }
        }
        for list in typo.values_mut() {
            list.sort();
        }
        let names: serde_json::Value = serde_json::from_str(NAMES).unwrap_or_default();
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(String::from).collect()
        };
        let nick_groups = names["nicknames"].as_array().into_iter().flatten().map(strings).collect();
        let mut latin: Vec<(String, Vec<String>)> = names["latin"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), strings(v)))
            .collect();
        latin.sort();
        let not_names = strings(&names["not_names"]).iter().map(|s| fold(s)).collect();
        Lexicon { map, typo, nick_groups, latin, not_names, dup }
    }

    /// Exact match, then inflection, then proclitics (shortest first), each against the lexicon.
    pub fn lookup(&self, norm: &str) -> Option<Hit> {
        if let Some(c) = self.map.get(norm) {
            return Some(Hit { concept: c, prefix: String::new(), typo: false });
        }
        for v in stem::suffix_variants(norm).iter().skip(1) {
            if let Some(c) = self.map.get(v.as_str()) {
                return Some(Hit { concept: c, prefix: String::new(), typo: false });
            }
        }
        for (prefix, rest) in stem::prefix_splits(norm) {
            for v in stem::suffix_variants(&rest) {
                if let Some(c) = self.map.get(v.as_str()) {
                    return Some(Hit { concept: c, prefix, typo: false });
                }
            }
        }
        None
    }

    /// Bounded typo tolerance for keywords: same first letter, distance 1 for 4-6 letters and 2
    /// for 7+, or a 3-letter word that only lacks one letter of a 4-letter keyword with the same
    /// first and last letter ("הים" -> "היום"). Names and terms never go through here.
    pub fn lookup_typo(&self, norm: &str) -> Option<Hit> {
        let mut variants = vec![(String::new(), norm.to_string())];
        if let Some(first) = stem::prefix_splits(norm).into_iter().next() {
            variants.push(first);
        }
        let mut best: Option<(usize, Hit)> = None;
        for (prefix, w) in variants {
            let cs: Vec<char> = w.chars().collect();
            let n = cs.len();
            if n < 3 || !cs.iter().all(|c| c.is_alphabetic()) {
                continue;
            }
            let max = if n >= 7 { 2 } else { 1 };
            let Some(list) = self.typo.get(&cs[0]) else { continue };
            for (form, concept) in list {
                if n == 3 && (form.len() != 4 || form[3] != cs[2]) {
                    continue;
                }
                if let Some(d) = stem::osa(&cs, form, max) {
                    if best.as_ref().map_or(true, |(bd, _)| d < *bd) {
                        best = Some((d, Hit { concept, prefix: prefix.clone(), typo: true }));
                    }
                }
            }
        }
        best.map(|(_, h)| h)
    }

    pub fn is_not_name(&self, norm: &str) -> bool {
        self.not_names.contains(norm)
    }

    /// A first name the tables know (a nickname group member or a Latin-spelling key), folded.
    pub fn is_known_name(&self, norm: &str) -> bool {
        self.nick_groups.iter().any(|g| g.iter().any(|n| fold(n) == norm)) || self.latin.iter().any(|(he, _)| fold(he) == norm)
    }

    /// Duplicate surface forms across concepts (a lexicon bug; checked by a test).
    pub fn duplicates(&self) -> &[String] {
        &self.dup
    }

    pub fn form_count(&self) -> usize {
        self.map.len()
    }
}

pub fn get() -> &'static Lexicon {
    static LEX: OnceLock<Lexicon> = OnceLock::new();
    LEX.get_or_init(Lexicon::build)
}

/// Annotate tokens: exact / inflected / prefixed match first, typo tolerance for what is left.
pub fn annotate(tokens: &[Token]) -> Vec<Ann> {
    let lex = get();
    tokens
        .iter()
        .map(|t| {
            let hit = if t.sym || t.quoted {
                None
            } else {
                lex.lookup(&t.norm).or_else(|| lex.lookup_typo(&t.norm))
            };
            Ann { tok: t.clone(), hit, used: false }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------

/// Hebrew -> Latin letters, vowels guessed: a vowel letter (י, ו, א at the start) between
/// consonants becomes i / u, and an "a" is inserted between two consonants.
fn transliterate_he(name: &str) -> String {
    let cs: Vec<char> = name.chars().filter(|c| is_he(*c)).collect();
    let consonant = |c: char| !matches!(c, 'י' | 'ו' | 'א' | 'ה' | 'ע');
    let mut out = String::new();
    for (i, &c) in cs.iter().enumerate() {
        let first = i == 0;
        let last = i + 1 == cs.len();
        let prev_cons = i > 0 && consonant(cs[i - 1]);
        let next_cons = i + 1 < cs.len() && consonant(cs[i + 1]);
        let piece: &str = match c {
            'א' | 'ע' => {
                if first {
                    if next_cons {
                        "a"
                    } else {
                        ""
                    }
                } else {
                    ""
                }
            }
            'ב' => {
                if first {
                    "b"
                } else {
                    "v"
                }
            }
            'ג' => "g",
            'ד' => "d",
            'ה' => {
                if last {
                    "a"
                } else {
                    "h"
                }
            }
            'ו' => {
                if first {
                    "v"
                } else {
                    "u"
                }
            }
            'ז' => "z",
            'ח' => "ch",
            'ט' | 'ת' => "t",
            'י' => {
                if first {
                    "y"
                } else {
                    "i"
                }
            }
            'כ' | 'ך' => {
                if first {
                    "k"
                } else {
                    "ch"
                }
            }
            'ל' => "l",
            'מ' | 'ם' => "m",
            'נ' | 'ן' => "n",
            'ס' => "s",
            'פ' | 'ף' => {
                if first {
                    "p"
                } else {
                    "f"
                }
            }
            'צ' | 'ץ' => "tz",
            'ק' => "k",
            'ר' => "r",
            'ש' => "sh",
            _ => "",
        };
        // Short vowel between two consonants ("Yuval", "Dana").
        if consonant(c) && prev_cons && !first && !piece.is_empty() {
            if !out.ends_with('a') {
                out.push('a');
            }
        }
        out.push_str(piece);
    }
    let mut it = out.chars();
    match it.next() {
        Some(f) => f.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

/// Spellings of a person's name to try against calendar names, senders and the address book:
/// the name itself, its folded form, known Hebrew nicknames ("איציק" <-> "יצחק") and a Latin
/// transliteration ("יובל" -> "Yuval"; "Dana" -> "דנה"). The first entry is always the name as
/// given. Deduplicated, at most 24 entries.
pub fn name_variants(name: &str) -> Vec<String> {
    let name = name.trim();
    let lex = get();
    let mut out: Vec<String> = vec![name.to_string()];
    let add = |s: String, out: &mut Vec<String>| {
        if !s.is_empty() && !out.contains(&s) && out.len() < 24 {
            out.push(s);
        }
    };
    let folded = fold(name);
    add(folded.clone(), &mut out);

    // Hebrew spellings to expand from: the name, or the Hebrew names that list it as Latin.
    let mut hebrew: Vec<String> = Vec::new();
    if name.chars().any(is_he) {
        hebrew.push(folded.clone());
    } else {
        for (he, lat) in &lex.latin {
            if lat.iter().any(|l| fold(l) == folded) {
                hebrew.push(fold(he));
                add(he.clone(), &mut out);
            }
        }
    }
    for h in hebrew.clone() {
        for group in &lex.nick_groups {
            if group.iter().any(|g| fold(g) == h) {
                for g in group {
                    add(g.clone(), &mut out);
                    if fold(g) != h {
                        hebrew.push(fold(g));
                    }
                }
            }
        }
    }
    for h in hebrew {
        let known = lex.latin.iter().find(|(he, _)| fold(he) == h);
        match known {
            Some((_, lat)) => {
                for l in lat {
                    add(l.clone(), &mut out);
                }
            }
            None => add(transliterate_he(&h), &mut out),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexicon_has_no_duplicate_forms() {
        assert!(get().duplicates().is_empty(), "duplicates: {:?}", get().duplicates());
        assert!(get().form_count() > 400);
    }

    #[test]
    fn lookup_handles_prefixes_and_plurals() {
        let lex = get();
        assert_eq!(lex.lookup(&fold("ביומן")).map(|h| (h.concept, h.prefix)), Some(("N_CAL", "ב".to_string())));
        assert_eq!(lex.lookup(&fold("המיילים")).map(|h| h.concept), Some("N_MAIL"));
        assert_eq!(lex.lookup(&fold("פגישות")).map(|h| h.concept), Some("N_MEETING"));
        assert_eq!(lex.lookup(&fold("ומחר")).map(|h| h.concept), Some("T_TOMORROW"));
        assert_eq!(lex.lookup(&fold("מהשבוע")).map(|h| h.concept), Some("T_WEEK"));
        assert_eq!(lex.lookup("emails").map(|h| h.concept), Some("N_MAIL"));
        // a whole-word match wins over a prefix split: מחר is not מ + חר
        assert_eq!(lex.lookup(&fold("מחר")).map(|h| h.prefix), Some(String::new()));
        assert!(lex.lookup(&fold("מיובל")).is_none());
        assert!(lex.lookup(&fold("תקציב")).is_none());
    }

    #[test]
    fn typo_tolerance_is_bounded() {
        let lex = get();
        assert_eq!(lex.lookup_typo(&fold("הים")).map(|h| h.concept), Some("T_TODAY"));
        assert_eq!(lex.lookup_typo(&fold("מיל")).map(|h| h.concept), Some("N_MAIL"));
        assert_eq!(lex.lookup_typo(&fold("tomorow")).map(|h| h.concept), Some("T_TOMORROW"));
        // verbs are never typo-matched: חושב must not become חשב
        assert!(lex.lookup_typo(&fold("חושב")).is_none());
        assert!(lex.lookup_typo(&fold("יובל")).is_none());
        assert!(lex.lookup_typo(&fold("דנה")).is_none());
    }

    #[test]
    fn names_nicknames_and_transliteration() {
        let v = name_variants("איציק");
        assert_eq!(v[0], "איציק");
        assert!(v.contains(&"יצחק".to_string()));
        assert!(v.contains(&"Itzik".to_string()));
        let y = name_variants("יובל");
        assert!(y.contains(&"Yuval".to_string()));
        let d = name_variants("Dana");
        assert_eq!(d[0], "Dana");
        assert!(d.contains(&"דנה".to_string()));
        // unknown Hebrew name falls back to the rule based transliteration
        assert_eq!(transliterate_he("דנה"), "Dana");
        assert_eq!(transliterate_he("יובל"), "Yuval");
        assert!(name_variants("זמבלה").len() >= 2);
        let mut seen = std::collections::HashSet::new();
        assert!(name_variants("יצחק").iter().all(|s| seen.insert(s.clone())));
    }
}
