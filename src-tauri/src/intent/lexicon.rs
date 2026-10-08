//! The compiled-in lexicon (`lexicon/*.json`, `include_str!`): surface form -> concept, plus the
//! name tables. Loaded once, so an offline install carries everything and nothing can fail at
//! runtime because a file is missing.
//!
//! A concept is a short id such as `N_MAIL` or `T_TOMORROW`; `score.rs` and `dates.rs` work on
//! concepts, never on surface words.

use super::normalize::{fold, is_he, Token};
use super::spell;
use super::stem;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const WORDS: &str = include_str!("lexicon/words.json");
const TIME: &str = include_str!("lexicon/time.json");
const NAMES: &str = include_str!("lexicon/names.json");

const PHRASES: &str = include_str!("lexicon/phrases.json");
const REAL_WORDS: &str = include_str!("lexicon/real_words.txt");

/// The concept of the second and later words of a multi-word phrase: not content, no feature.
pub const PART: &str = "PART";

/// A lexicon match for one token.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub concept: &'static str,
    /// Proclitic letters that were stripped ("ב" in "ביומן"), folded.
    pub prefix: String,
    /// Matched through the spelling correction, not exactly.
    pub typo: bool,
    /// Weighted edit cost of a spelling correction (0 for an exact match).
    pub cost: f32,
    /// The typed word is itself a real Hebrew word (corrected only with context support).
    pub real_word: bool,
    /// The first word of a multi-word phrase ("לוח זמנים").
    pub phrase: bool,
}

impl Hit {
    fn exact(concept: &'static str, prefix: String) -> Hit {
        Hit { concept, prefix, typo: false, cost: 0.0, real_word: false, phrase: false }
    }
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
    /// Possessive forms of the nouns (see `possessive_forms`).
    possessive: HashMap<String, &'static str>,
    /// Every first name the name tables know, folded.
    name_set: HashSet<String>,
    /// Folded name -> the nickname groups it belongs to (indexes into `nick_groups`).
    nick_index: HashMap<String, Vec<usize>>,
    /// Folded Hebrew name -> its Latin spellings; folded Latin spelling -> Hebrew names.
    latin_of: HashMap<String, Vec<String>>,
    hebrew_of: HashMap<String, Vec<String>>,
    /// Spelling-correction candidates: (form, concept, Zipf prior), by (length, first letter).
    typo: HashMap<(usize, char), Vec<(Vec<char>, &'static str, f32)>>,
    /// Multi-word phrases: first word -> (all words, concept), longest first.
    phrases: HashMap<String, Vec<(Vec<String>, &'static str)>>,
    /// Real Hebrew words that sit one edit away from a keyword ("חושב", "קצבים", "הים").
    real_words: HashSet<String>,
    nick_groups: Vec<Vec<String>>,
    latin: Vec<(String, Vec<String>)>,
    not_names: HashSet<String>,
    dup: Vec<String>,
}

/// Concepts that the spelling correction may select. Opening and launching verbs, names and
/// grammar words are left out on purpose: a wrong guess there would change what the engine does.
/// Search / show verbs only ever lead to a read, and a misspelt write verb ("תמוחק", "תבתל") is
/// read as the refusal it asks for, which fails safe.
fn typo_eligible(concept: &str) -> bool {
    concept.starts_with("N_")
        || concept == "VETO"
        || matches!(
            concept,
            "T_TODAY"
                | "T_TOMORROW"
                | "T_YESTERDAY"
                | "T_DAYAFTER"
                | "T_WEEK"
                | "T_MONTH"
                | "T_NEXT"
                | "T_PAST"
                | "FREE"
                | "M_LATEST_ONE"
                | "M_LATEST_MANY"
                | "V_SEARCH"
                | "V_SHOW"
                | "YESH"
                | "UNREAD"
                | "H_NOTE"
                | "H_MAIL"
                | "A_EXCEL"
                | "A_PPT"
                | "A_WORD"
                | "A_PDF"
                | "D_SUN"
                | "D_MON"
                | "D_TUE"
                | "D_WED"
                | "D_THU"
                | "D_FRI"
        )
}

/// Possessive endings (folded): my, your, his, our, your (pl.), their.
const POSSESSIVE: [&str; 6] = ["י", "כ", "ו", "נו", "כמ", "המ"];

/// Every possessive form of the nouns, built once: "יומנ" -> "יומני", "קבצימ" -> "קבציי" / "קבציו",
/// "פגישות" -> "פגישותיי", "פגישה" -> "פגישתי". Only nouns take one, and a form that is a word of
/// its own in the lexicon is never overridden.
fn possessive_forms(map: &HashMap<String, &'static str>) -> HashMap<String, &'static str> {
    let mut out = HashMap::new();
    for (form, concept) in map.iter().filter(|(f, c)| c.starts_with("N_") && f.chars().count() >= 3 && f.chars().all(is_he)) {
        let mut stems: Vec<String> = Vec::new();
        if let Some(s) = form.strip_suffix("ימ") {
            // plural: קבצימ -> קבצי + (י|כ|ו|נו|כמ|המ), the possessive keeps the yod
            stems.push(format!("{s}י"));
        } else if form.ends_with("ות") {
            stems.push(format!("{form}י"));
            stems.push(form.clone());
        } else if let Some(s) = form.strip_suffix('ה') {
            stems.push(format!("{s}ת"));
        } else {
            stems.push(form.clone());
        }
        for s in stems {
            for end in POSSESSIVE {
                let w = format!("{s}{end}");
                if !map.contains_key(&w) {
                    out.entry(w).or_insert(*concept);
                }
            }
        }
    }
    out
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
        let mut typo: HashMap<(usize, char), Vec<(Vec<char>, &'static str, f32)>> = HashMap::new();
        for (form, concept) in &map {
            let cs: Vec<char> = form.chars().collect();
            if typo_eligible(concept) && cs.len() >= 3 && cs.iter().all(|c| c.is_alphabetic()) {
                let prior = spell::zipf(form).unwrap_or(0.0);
                typo.entry((cs.len(), cs[0])).or_default().push((cs, *concept, prior));
            }
        }
        for list in typo.values_mut() {
            list.sort_by(|a, b| a.0.cmp(&b.0));
        }
        let mut phrases: HashMap<String, Vec<(Vec<String>, &'static str)>> = HashMap::new();
        let pv: serde_json::Value = serde_json::from_str(PHRASES).unwrap_or_default();
        for (concept, forms) in pv.as_object().into_iter().flatten() {
            let concept: &'static str = leak(concept);
            for f in forms.as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
                let words: Vec<String> = fold(f).split(' ').map(String::from).collect();
                if words.len() >= 2 {
                    phrases.entry(words[0].clone()).or_default().push((words, concept));
                }
            }
        }
        for list in phrases.values_mut() {
            list.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        }
        let real_words = REAL_WORDS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(fold).collect();
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
        let nick_groups: Vec<Vec<String>> = nick_groups;
        let name_set = nick_groups.iter().flatten().map(|n| fold(n)).chain(latin.iter().map(|(he, _)| fold(he))).collect();
        let mut nick_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (gi, g) in nick_groups.iter().enumerate() {
            for n in g {
                let e = nick_index.entry(fold(n)).or_default();
                if !e.contains(&gi) {
                    e.push(gi);
                }
            }
        }
        let mut latin_of: HashMap<String, Vec<String>> = HashMap::new();
        let mut hebrew_of: HashMap<String, Vec<String>> = HashMap::new();
        for (he, lat) in &latin {
            latin_of.entry(fold(he)).or_default().extend(lat.iter().cloned());
            for l in lat {
                hebrew_of.entry(fold(l)).or_default().push(he.clone());
            }
        }
        let possessive = possessive_forms(&map);
        Lexicon { map, possessive, name_set, nick_index, latin_of, hebrew_of, typo, phrases, real_words, nick_groups, latin, not_names, dup }
    }

    /// A form, or a noun with a possessive ending ("יומני", "קבציו", "פגישותיי").
    fn form(&self, w: &str) -> Option<&'static str> {
        self.map.get(w).or_else(|| self.possessive.get(w)).copied()
    }

    /// Exact match, then inflection, then proclitics (shortest first), each against the lexicon.
    pub fn lookup(&self, norm: &str) -> Option<Hit> {
        if let Some(c) = self.map.get(norm) {
            return Some(Hit::exact(c, String::new()));
        }
        for v in stem::suffix_variants(norm).iter().skip(1) {
            if let Some(c) = self.map.get(v.as_str()) {
                return Some(Hit::exact(c, String::new()));
            }
        }
        if let Some(c) = self.form(norm) {
            return Some(Hit::exact(c, String::new()));
        }
        for (prefix, rest) in stem::prefix_splits(norm) {
            for v in stem::suffix_variants(&rest) {
                if let Some(c) = self.map.get(v.as_str()) {
                    // a verb takes only "and" / "that" ("ותחפש", "שתבדוק"): "המספר" is not ה+מ+ספר
                    if c.starts_with("V_") && !prefix.chars().all(|p| p == 'ו' || p == 'ש') {
                        continue;
                    }
                    // "כהן" is not כ + הן, "הבה" is not ה + בה: a two-letter grammar word takes no proclitic
                    if v.chars().count() == 2 && !prefix.chars().all(|p| p == 'ו' || p == 'ש') && (matches!(*c, "STOP" | "CONJ" | "REFBACK") || c.starts_with("P_")) {
                        continue;
                    }
                    return Some(Hit::exact(c, prefix));
                }
            }
            // a possessive noun behind a proclitic ("ביומני"); plain forms were tried above
            if let Some(c) = self.possessive.get(&rest) {
                return Some(Hit::exact(c, prefix));
            }
        }
        None
    }

    /// The typed word is a real Hebrew word (listed, or common in the frequency table).
    pub fn is_real_word(&self, norm: &str) -> bool {
        self.real_words.contains(norm) || spell::zipf(norm).map_or(false, |z| z >= spell::REAL_WORD_ZIPF)
    }

    /// Spelling correction for keywords: the candidate with the lowest weighted edit cost (see
    /// `spell`) within the length's budget, the first letter kept; ties go to the more frequent
    /// form. Tried on the word and on the word without its proclitics. Names and terms never go
    /// through here. The caller decides whether a correction of a real word is supported.
    pub fn lookup_typo(&self, norm: &str) -> Option<Hit> {
        let mut variants = vec![(String::new(), norm.to_string())];
        variants.extend(stem::prefix_splits(norm).into_iter().take(2));
        let real_word = self.is_real_word(norm);
        let mut best: Option<(f32, f32, Hit)> = None;
        // the best guess that is not a refusal: it wins over a refusal that is barely cheaper
        let mut best_other: Option<(f32, Hit)> = None;
        for (k, (prefix, w)) in variants.into_iter().enumerate() {
            let cs: Vec<char> = w.chars().collect();
            let n = cs.len();
            if n < 3 || !cs.iter().all(|c| c.is_alphabetic()) {
                continue;
            }
            // a stripped proclitic is a guess too
            let extra = if k == 0 { 0.0 } else { 0.25 };
            let max = spell::max_cost(if k == 0 { n } else { norm.chars().count() });
            for (len, first) in (n.saturating_sub(2)..=n + 2).flat_map(|len| spell::first_letters(cs[0]).map(move |f| (len, f))) {
                let Some(list) = self.typo.get(&(len, first)) else { continue };
                for (form, concept, prior) in list {
                    let Some(cost) = spell::weighted(&cs, form, max) else { continue };
                    let cost = cost + extra;
                    // a verb is only corrected for a cheap slip ("תחפס"), never from a name ("הראל"),
                    // and takes only "and" / "that" in front
                    if concept.starts_with("V_") && (cost > 0.75 || !prefix.chars().all(|p| p == 'ו' || p == 'ש')) {
                        continue;
                    }
                    // a refusal from a guess only for a cheap slip of a whole word
                    if *concept == "VETO" && (cost > 0.75 || !prefix.is_empty() || n < 4) {
                        continue;
                    }
                    if *concept != "VETO" && best_other.as_ref().map_or(true, |(c, _)| cost < *c - 1e-6) {
                        best_other = Some((cost, Hit { concept, prefix: prefix.clone(), typo: true, cost, real_word, phrase: false }));
                    }
                    let better = match &best {
                        None => true,
                        Some((bc, bp, _)) => cost < *bc - 1e-6 || ((cost - *bc).abs() < 1e-6 && *prior > *bp),
                    };
                    if better {
                        best = Some((cost, *prior, Hit { concept, prefix: prefix.clone(), typo: true, cost, real_word, phrase: false }));
                    }
                }
            }
        }
        match (best, best_other) {
            (Some((c, _, h)), Some((oc, other))) if h.concept == "VETO" && oc <= c + 0.25 + 1e-6 => {
                let _ = c;
                Some(other)
            }
            (best, _) => best.map(|(_, _, h)| h),
        }
    }

    /// The longest phrase starting at token `i` ("לוח זמנים", "איפה שמרתי"): its concept, the
    /// proclitic of its first word and its length in tokens.
    fn phrase_at(&self, toks: &[Token], i: usize) -> Option<(&'static str, String, usize)> {
        let first = &toks[i].norm;
        let mut heads = vec![(String::new(), first.clone())];
        heads.extend(stem::prefix_splits(first));
        for (prefix, head) in heads {
            let Some(list) = self.phrases.get(&head) else { continue };
            for (words, concept) in list {
                let n = words.len();
                if i + n <= toks.len() && (1..n).all(|k| !toks[i + k].quoted && toks[i + k].norm == words[k]) {
                    return Some((concept, prefix, n));
                }
            }
        }
        None
    }

    pub fn is_not_name(&self, norm: &str) -> bool {
        self.not_names.contains(norm)
    }

    /// A first name the tables know (a nickname group member or a Latin-spelling key), folded.
    pub fn is_known_name(&self, norm: &str) -> bool {
        self.name_set.contains(norm)
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

/// Two words typed without the space between them ("מהיש", "מחרבבוקר"): split where both halves
/// are keywords and neither is a grammar word, so a name such as "מיכל" (מי + כל) stays whole.
fn split_joined(lex: &Lexicon, t: &Token) -> Option<(Token, Token)> {
    let cs: Vec<char> = t.raw.chars().collect();
    if cs.len() < 4 || !cs.iter().all(|c| is_he(*c)) || lex.is_known_name(&t.norm) {
        return None;
    }
    let weak = |h: &Hit| matches!(h.concept, "STOP" | "SELF" | "P_OF" | "P_WITH" | "P_ABOUT" | "P_AT" | "NOT" | "CONJ" | "REFBACK");
    let strong = |h: &Hit| ["N_", "T_", "V_", "D_"].iter().any(|p| h.concept.starts_with(p));
    for k in 2..=cs.len() - 2 {
        let left: String = cs[..k].iter().collect();
        let right: String = cs[k..].iter().collect();
        let (l, r) = (Token::word(left), Token::word(right));
        // "המחשב" is ה + מחשב, not הם + חשב: proclitic letters before a verb are its prefix
        if cs[..k].iter().all(|c| stem::PREFIX_LETTERS.contains(c)) && lex.lookup(&r.norm).map_or(false, |h| h.concept.starts_with("V_")) {
            continue;
        }
        if lex.lookup(&l.norm).map_or(false, |h| strong(&h)) && super::numwords::is_number_word(r.norm.trim_start_matches('ב')) {
            return Some((l, r));
        }
        match (lex.lookup(&l.norm), lex.lookup(&r.norm)) {
            // "חפשלי", "לימחר": a grammar word may join a keyword, never another grammar word
            (Some(a), Some(b)) if (!weak(&a) && !weak(&b)) || strong(&a) || strong(&b) => return Some((l, r)),
            _ => {}
        }
    }
    None
}

/// Whether the rest of the sentence supports reading the real word at `i` as the keyword its
/// correction found: the keyword fills a role nothing else fills ("תחפש לי קצבים": a search with
/// no object, so קבצים; "מייל על קצבים": the mail is the object, so קצבים stays a search word).
fn correction_supported(a: &[Ann], i: usize) -> bool {
    let Some(h) = &a[i].hit else { return false };
    let others = |pred: &dyn Fn(&Hit) -> bool| a.iter().enumerate().any(|(k, t)| k != i && t.hit.as_ref().map_or(false, |x| !x.typo && pred(x)));
    let asks = others(&|x| matches!(x.concept, "V_SEARCH" | "V_SHOW" | "Q_WHAT" | "Q_WHICH" | "Q_WHERE" | "Q_HOWMANY"))
        || others(&|x| x.concept.starts_with("T_") || x.concept.starts_with("D_"));
    let c = h.concept;
    if c.starts_with("N_") {
        asks && !others(&|x| matches!(x.concept, "N_MAIL" | "N_FILE" | "N_NOTE" | "N_APP" | "N_CAL" | "N_MEETING"))
    } else if c.starts_with("T_") || c.starts_with("D_") {
        let cue = others(&|x| matches!(x.concept, "YESH" | "FREE" | "N_CAL" | "N_MEETING" | "Q_WHAT" | "Q_WHEN"))
            || (!is_day_word(c) && others(&|x| matches!(x.concept, "N_MAIL" | "N_FILE" | "N_NOTE" | "N_PPT") || x.concept.starts_with("A_")));
        cue && !others(&|x| is_day_word(x.concept))
    } else if c.starts_with("V_") {
        i <= 1 && !others(&|x| x.concept.starts_with("V_"))
    } else {
        false
    }
}

/// A word that names a day (parts of the day such as "בבוקר" go with one, so they are not here).
fn is_day_word(concept: &str) -> bool {
    concept.starts_with("D_") || matches!(concept, "T_TODAY" | "T_TOMORROW" | "T_DAYAFTER" | "T_YESTERDAY" | "T_2DAGO")
}

/// Whether a correction stands without asking the rest of the sentence. A time word never takes
/// the definite ה ("המחיר" is not ה + מחר) and is not read next to another day word; a guess
/// that also stripped a proclitic stands only for one cheap edit.
fn correction_stands(a: &[Ann], k: usize) -> bool {
    let Some(h) = &a[k].hit else { return false };
    if h.concept.starts_with("T_") || h.concept.starts_with("D_") {
        if h.prefix.contains('ה') {
            return false;
        }
        let other_day = is_day_word(h.concept) && a.iter().enumerate().any(|(j, t)| j != k && t.hit.as_ref().map_or(false, |x| !x.typo && is_day_word(x.concept)));
        if other_day {
            return false;
        }
        return !h.real_word || correction_supported(a, k);
    }
    if h.real_word || (!h.prefix.is_empty() && h.cost > 0.5 + 0.25 + 1e-6) {
        return correction_supported(a, k);
    }
    true
}

/// Annotate tokens: exact / inflected / prefixed match first, then multi-word phrases, then words
/// typed together, then the spelling correction for what is left.
pub fn annotate(tokens: &[Token]) -> Vec<Ann> {
    let lex = get();
    let mut toks: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut a: Vec<Ann> = Vec::with_capacity(tokens.len());
    for t in tokens {
        let hit = if t.sym || t.quoted { None } else { lex.lookup(&t.norm) };
        if hit.is_none() && !t.sym && !t.quoted {
            if let Some((l, r)) = split_joined(lex, t) {
                for part in [l, r] {
                    let hit = lex.lookup(&part.norm);
                    a.push(Ann { tok: part.clone(), hit, used: false });
                    toks.push(part);
                }
                continue;
            }
        }
        a.push(Ann { tok: t.clone(), hit, used: false });
        toks.push(t.clone());
    }
    let mut i = 0;
    while i < a.len() {
        if a[i].tok.sym || a[i].tok.quoted {
            i += 1;
            continue;
        }
        if let Some((concept, prefix, n)) = lex.phrase_at(&toks, i) {
            a[i].hit = Some(Hit { phrase: true, ..Hit::exact(concept, prefix) });
            for k in 1..n {
                a[i + k].hit = Some(Hit::exact(PART, String::new()));
            }
            i += n;
            continue;
        }
        i += 1;
    }
    let mut topic_left = 0;
    for k in 0..a.len() {
        if k > 0 && (a[k - 1].is("P_ABOUT") || a[k - 1].is("MARK") || a[k - 1].is("P_REGARD")) {
            topic_left = 3;
        }
        let t = &mut a[k];
        if topic_left > 0 {
            topic_left -= 1;
            if t.hit.is_none() {
                continue;
            }
            topic_left = 0;
        }
        // names and number words ("ארבעים ושתיים") are never corrected into keywords
        let number = super::numwords::is_number_word(&t.tok.norm) || t.tok.norm.strip_prefix('ו').map_or(false, super::numwords::is_number_word);
        if t.hit.is_none() && !t.tok.sym && !t.tok.quoted && !number && !lex.is_known_name(&t.tok.norm) {
            t.hit = lex.lookup_typo(&t.tok.norm);
        }
    }
    for k in 0..a.len() {
        if a[k].hit.as_ref().map_or(false, |h| h.typo) && !correction_stands(&a, k) {
            a[k].hit = None;
        }
    }
    a
}

/// Why a write / out-of-scope word is refused, for the answer's wording (see `Analysis`).
/// How a VETO word stops a request (see `score::Features::topic_veto`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VetoClass {
    /// An action the app never does (delete, send, install...).
    Write,
    /// A topic it cannot answer (weather, news).
    Topic,
    /// A topic word that is also a normal subject of a document (forecast, dollar, stocks, a game).
    Subject,
}

pub fn veto_class(norm: &str) -> VetoClass {
    let w = norm;
    let has = |list: &[&str]| list.iter().any(|x| w == *x || w.ends_with(x));
    if has(&["תחזית", "forecast", "דולר", "מניה", "מניית", "מניות", "בורסה", "משחק", "ניצח", "ניצחה"]) {
        VetoClass::Subject
    } else if has(&["מזג", "weather", "חדשות", "news", "כותרות", "גשמ", "טמפרטורה"]) {
        VetoClass::Topic
    } else {
        VetoClass::Write
    }
}

pub fn unsupported_kind(norm: &str) -> &'static str {
    let w = norm;
    let has = |list: &[&str]| list.iter().any(|x| w == *x || w.ends_with(x));
    if has(&["מזג", "weather", "forecast", "גשמ", "טמפרטורה"]) {
        "weather"
    } else if has(&["חדשות", "news", "כותרות", "דולר", "מניה", "מניית", "מניות", "בורסה", "משחק", "ניצח", "ניצחה"]) {
        "news"
    } else if has(&["תרגמ", "תתרגמ", "translate"]) {
        "translate"
    } else if has(&["הורד", "תוריד", "download", "install", "התקנ", "תתקינ"]) {
        "install"
    } else if has(&["shutdown", "restart", "format", "wipe", "כבה", "תכבה", "לכבות", "אתחל", "תאתחל", "לאתחל", "reboot"]) {
        "power"
    } else {
        "write"
    }
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
        for he in lex.hebrew_of.get(&folded).into_iter().flatten() {
            hebrew.push(fold(he));
            add(he.clone(), &mut out);
        }
    }
    for h in hebrew.clone() {
        for &gi in lex.nick_index.get(&h).into_iter().flatten() {
            for g in &lex.nick_groups[gi] {
                add(g.clone(), &mut out);
                let gf = fold(g);
                if gf != h {
                    hebrew.push(gf);
                }
            }
        }
    }
    for h in hebrew {
        match lex.latin_of.get(&h) {
            Some(lat) => {
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

    /// The lexicon is built lazily on the first question (no work at app start). Its cost and size,
    /// printed with `--nocapture`.
    #[test]
    fn build_is_cheap() {
        let t = std::time::Instant::now();
        let lex = Lexicon::build();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let typo: usize = lex.typo.values().map(Vec::len).sum();
        println!(
            "lexicon build {ms:.1} ms: {} forms, {} possessive forms, {} spelling candidates, {} phrases",
            lex.map.len(),
            lex.possessive.len(),
            typo,
            lex.phrases.values().map(Vec::len).sum::<usize>()
        );
        assert!(ms < 500.0, "lexicon build took {ms:.1} ms");
    }

    #[test]
    fn possessive_and_phrase_forms() {
        let lex = get();
        assert_eq!(lex.lookup(&fold("יומני")).map(|h| h.concept), Some("N_CAL"));
        assert_eq!(lex.lookup(&fold("ביומני")).map(|h| (h.concept, h.prefix)), Some(("N_CAL", "ב".to_string())));
        assert_eq!(lex.lookup(&fold("פגישותיי")).map(|h| h.concept), Some("N_MEETING"));
        assert_eq!(lex.lookup(&fold("קבציו")).map(|h| h.concept), Some("N_FILE"));
        // a verb takes no article: "המספר" is not ה + מ + ספר
        assert!(lex.lookup(&fold("המספר")).map_or(true, |h| !h.concept.starts_with("V_")));
        // a two-letter grammar word takes no proclitic other than ו / ש
        assert!(lex.lookup(&fold("כהן")).is_none());
        assert_eq!(lex.lookup(&fold("ועל")).map(|h| h.concept), Some("P_ABOUT"));
        let a = annotate(&super::super::normalize::tokenize("מה יש לי בלוח הזמנים"));
        assert!(a.iter().any(|t| t.is("N_CAL") && t.hit.as_ref().map_or(false, |h| h.phrase)));
    }

    #[test]
    fn spelling_correction_needs_context_for_real_words() {
        let concepts = |t: &str| -> Vec<&'static str> { annotate(&super::super::normalize::tokenize(t)).iter().map(|a| a.concept()).collect() };
        // a search with no object: the real word "קצבים" is read as "קבצים"
        assert!(concepts("תחפש לי קצבים").contains(&"N_FILE"));
        // the mail is the object, so "קצבים" stays a search word
        assert!(!concepts("מייל על קצבים").contains(&"N_FILE"));
        // the sea is not today when a day is already named
        assert!(!concepts("מה יש לי מחר בים").contains(&"T_TODAY"));
        // numbers and names are never corrected
        assert!(concepts("ארבעים ושתיים").iter().all(|c| c.is_empty()));
        assert_eq!(concepts("מכחה לי מחר")[0], "YESH");
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
