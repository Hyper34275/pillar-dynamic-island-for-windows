//! Light Hebrew / English stemming and typo distance. Everything works on the folded form
//! (`normalize::fold`), so final letters are already regular ones ("ים" is "ימ" here).
//!
//! The lexicon is the authority: a prefix or suffix is stripped only when the remainder is a
//! known word (the callers try the variants against the lexicon), except for search-term
//! variants, which are alternatives that widen a search, never a replacement.

/// Hebrew proclitics: and, the, in, to, from, that, as.
pub const PREFIX_LETTERS: [char; 7] = ['ו', 'ה', 'ב', 'ל', 'מ', 'ש', 'כ'];

fn is_prefix_letter(c: char) -> bool {
    PREFIX_LETTERS.contains(&c)
}

/// All `(prefix, rest)` splits with 1..=3 proclitic letters and a rest of at least 2 characters.
/// Shortest prefix first.
pub fn prefix_splits(norm: &str) -> Vec<(String, String)> {
    let cs: Vec<char> = norm.chars().collect();
    let mut out = Vec::new();
    for k in 1..=3usize {
        if cs.len() < k + 2 || !cs[..k].iter().all(|c| is_prefix_letter(*c)) {
            break;
        }
        out.push((cs[..k].iter().collect(), cs[k..].iter().collect()));
    }
    out
}

/// `w` first, then plural / construct / English inflection variants of it.
/// Hebrew: ימ -> "" (מיילים), ות -> "" / "ה" (פגישות), ת -> "ה" (פגישת). English: 's, s, es, ing, ed.
pub fn suffix_variants(w: &str) -> Vec<String> {
    let mut out = vec![w.to_string()];
    let cs: Vec<char> = w.chars().collect();
    let n = cs.len();
    let he = cs.iter().any(|c| super::normalize::is_he(*c));
    let push = |out: &mut Vec<String>, s: String| {
        if s.chars().count() >= 2 && !out.contains(&s) {
            out.push(s);
        }
    };
    if he {
        if n > 3 && w.ends_with("ימ") {
            push(&mut out, cs[..n - 2].iter().collect());
        }
        if n > 3 && w.ends_with("ות") {
            push(&mut out, cs[..n - 2].iter().collect());
            let mut s: String = cs[..n - 2].iter().collect();
            s.push('ה');
            push(&mut out, s);
        }
        if n > 3 && w.ends_with('ת') {
            let mut s: String = cs[..n - 1].iter().collect();
            s.push('ה');
            push(&mut out, s);
        }
    } else if cs.iter().all(|c| c.is_ascii_alphabetic() || *c == '\'' || *c == '-') {
        let strip = |s: &str, suf: &str| -> Option<String> {
            if s.len() >= suf.len() + 3 && s.ends_with(suf) {
                Some(s[..s.len() - suf.len()].to_string())
            } else {
                None
            }
        };
        for suf in ["'s", "es", "s", "ing", "ed"] {
            if let Some(s) = strip(w, suf) {
                push(&mut out, s);
            }
        }
    }
    out
}

/// Optimal-string-alignment distance (insert / delete / substitute / adjacent swap), `None` if it
/// exceeds `max`.
pub fn osa(a: &[char], b: &[char], max: usize) -> Option<usize> {
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > max {
        return None;
    }
    let mut prev2 = vec![0usize; m + 1];
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[m];
    (d <= max).then_some(d)
}

/// Alternative spellings of one search word, as typed first: the word, and the word without its
/// proclitics when that is plausible ("התקציב" -> "תקציב"). A single ה / ב / ל / ו needs a rest of
/// 4+ letters; a stack that ends in the definite ה ("וה", "מה", "בה") needs 3+. These are OR-ed in
/// a search, so an extra variant only widens the match.
pub fn term_variants(raw: &str) -> Vec<String> {
    let mut out = vec![raw.to_string()];
    let cs: Vec<char> = raw.chars().collect();
    if cs.first().map_or(true, |c| !super::normalize::is_he(*c)) {
        return out;
    }
    for k in 1..=3usize {
        if cs.len() < k + 3 || !cs[..k].iter().all(|c| is_prefix_letter(*c)) {
            break;
        }
        let last = cs[k - 1];
        let ok = if last == 'ה' { true } else { k == 1 && cs.len() >= k + 3 && matches!(last, 'ב' | 'ל' | 'ו') };
        if ok {
            let s: String = cs[k..].iter().collect();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn splits_prefixes() {
        let s = prefix_splits("ומחר");
        assert_eq!(s[0], ("ו".to_string(), "מחר".to_string()));
        assert!(prefix_splits("יום").is_empty());
        assert_eq!(prefix_splits("מהשבוע").len(), 3);
    }

    #[test]
    fn suffixes() {
        assert!(suffix_variants("מיילימ").contains(&"מייל".to_string()));
        assert!(suffix_variants("פגישות").contains(&"פגישה".to_string()));
        assert!(suffix_variants("פגישת").contains(&"פגישה".to_string()));
        assert!(suffix_variants("emails").contains(&"email".to_string()));
        assert!(suffix_variants("dana's").contains(&"dana".to_string()));
    }

    #[test]
    fn distance() {
        assert_eq!(osa(&cs("היומ"), &cs("הימ"), 1), Some(1));
        assert_eq!(osa(&cs("מייל"), &cs("מיל"), 1), Some(1));
        assert_eq!(osa(&cs("abcd"), &cs("abdc"), 1), Some(1));
        assert_eq!(osa(&cs("abcd"), &cs("wxyz"), 2), None);
    }

    #[test]
    fn term_variant_rules() {
        assert_eq!(term_variants("התקציב"), vec!["התקציב", "תקציב"]);
        assert_eq!(term_variants("חשבונית"), vec!["חשבונית"]);
        assert_eq!(term_variants("בנק"), vec!["בנק"]);
        assert_eq!(term_variants("invoice"), vec!["invoice"]);
        assert!(term_variants("בתקציב").contains(&"תקציב".to_string()));
    }
}
