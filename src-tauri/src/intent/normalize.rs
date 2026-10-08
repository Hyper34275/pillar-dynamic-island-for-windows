//! Text normalisation and tokenising for the intent engine. Pure, no allocation tricks: queries are
//! a few dozen characters.
//!
//! Two forms of every word are kept: `raw` (what the user typed, marks removed, final letters and
//! case kept, used for search terms and names) and `norm` (the comparison form from [`fold`]).

/// Hebrew letter (final forms included).
pub fn is_he(c: char) -> bool {
    ('\u{05D0}'..='\u{05EA}').contains(&c)
}

/// Characters that carry no meaning for matching: niqqud / cantillation (U+0591..U+05C7, except the
/// maqaf which acts as a hyphen), bidi controls, zero-width characters and the BOM.
fn is_ignorable(c: char) -> bool {
    match c {
        '\u{05BE}' => false,
        '\u{0591}'..='\u{05C7}' => true,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{061C}' | '\u{FEFF}' => true,
        _ => false,
    }
}

/// Unify quotes and dashes and map Arabic-Indic digits; leaves letters, case and finals alone.
fn map_char(c: char) -> char {
    match c {
        '\u{05F3}' | '\u{2019}' | '\u{2018}' | '\u{201B}' | '`' | '\u{00B4}' | '\u{02BC}' => '\'',
        '\u{05F4}' | '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{00AB}' | '\u{00BB}' => '"',
        '\u{05BE}' | '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' => '-',
        '\u{0660}'..='\u{0669}' => char::from_digit(c as u32 - 0x0660, 10).unwrap_or(c),
        '\u{06F0}'..='\u{06F9}' => char::from_digit(c as u32 - 0x06F0, 10).unwrap_or(c),
        '\u{00A0}' | '\u{2007}' | '\u{202F}' => ' ',
        _ => c,
    }
}

/// Final Hebrew letters -> their regular forms.
fn fold_final(c: char) -> char {
    match c {
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        _ => c,
    }
}

/// Remove marks and unify quotes, keeping case and final letters (the "display" form).
pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !is_ignorable(*c)).map(map_char).collect()
}

/// The comparison form of any text: niqqud and bidi marks removed, final letters folded, geresh /
/// gershayim / curly quotes unified, Latin lowercased, whitespace collapsed.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars() {
        if is_ignorable(c) {
            continue;
        }
        let c = map_char(c);
        if c.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
            continue;
        }
        space = false;
        for l in fold_final(c).to_lowercase() {
            out.push(l);
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// As typed (marks removed). For a quoted phrase: the text inside the quotes.
    pub raw: String,
    /// `fold(raw)`.
    pub norm: String,
    pub quoted: bool,
    /// A lone operator character (`+ * ^ % × ÷ = / -`).
    pub sym: bool,
}

impl Token {
    fn word(raw: String) -> Token {
        let norm = fold(&raw);
        Token { raw, norm, quoted: false, sym: false }
    }
}

const SYMBOLS: &str = "+*^%\u{00D7}\u{00F7}=/-";

/// Split into word / number / symbol tokens.
/// - Hebrew and digits are split apart ("ב3" -> "ב", "3"); Latin+digits stay together ("3pm").
/// - Kept inside a word: `'` after a letter (ה', what's), `"` between Hebrew letters (אחה"צ),
///   `.` `:` `/` `@` `_` between alphanumerics (15.10, 14:00, a@b.c), `-` between digits or
///   between Latin letters (2026-10-15, e-mail).
/// - "..." and '...' at a word start are one quoted token.
pub fn tokenize(text: &str) -> Vec<Token> {
    let cs: Vec<char> = clean(text).chars().collect();
    let n = cs.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if (c == '"' || c == '\'') && (i == 0 || !cs[i - 1].is_alphanumeric()) && i + 1 < n && !cs[i + 1].is_whitespace() {
            if let Some(j) = find_close(&cs, i) {
                let inner: String = cs[i + 1..j].iter().collect();
                let inner = inner.trim().to_string();
                if !inner.is_empty() {
                    let norm = fold(&inner);
                    out.push(Token { raw: inner, norm, quoted: true, sym: false });
                }
                i = j + 1;
                continue;
            }
            i += 1;
            continue;
        }
        if c.is_alphanumeric() {
            let start = i;
            let mut j = i + 1;
            while j < n {
                let ch = cs[j];
                let prev = cs[j - 1];
                let next = cs.get(j + 1).copied();
                if ch.is_alphanumeric() {
                    if (is_he(prev) && ch.is_ascii_digit()) || (prev.is_ascii_digit() && is_he(ch)) {
                        break;
                    }
                    j += 1;
                    continue;
                }
                let both_alnum = prev.is_alphanumeric() && next.map_or(false, |x| x.is_alphanumeric());
                let keep = match ch {
                    '\'' => prev.is_alphabetic(),
                    '"' => is_he(prev) && next.map_or(false, is_he),
                    '.' => both_alnum && !(is_he(prev) && next.map_or(false, is_he)),
                    ':' | '/' => prev.is_ascii_digit() && next.map_or(false, |x| x.is_ascii_digit()),
                    '@' | '_' => both_alnum,
                    '-' => {
                        (prev.is_ascii_digit() && next.map_or(false, |x| x.is_ascii_digit()))
                            || (prev.is_ascii_alphabetic() && next.map_or(false, |x| x.is_ascii_alphabetic()))
                    }
                    _ => false,
                };
                if !keep {
                    break;
                }
                j += 1;
            }
            out.push(Token::word(cs[start..j].iter().collect()));
            i = j;
            continue;
        }
        if SYMBOLS.contains(c) {
            let raw = c.to_string();
            out.push(Token { norm: raw.clone(), raw, quoted: false, sym: true });
        }
        i += 1;
    }
    out
}

/// Index of the quote that closes the one opened at `open`: the same character, not preceded by
/// whitespace and not followed by a letter or digit.
fn find_close(cs: &[char], open: usize) -> Option<usize> {
    let q = cs[open];
    for j in open + 2..cs.len() {
        if cs[j] == q && !cs[j - 1].is_whitespace() && cs.get(j + 1).map_or(true, |x| !x.is_alphanumeric()) {
            return Some(j);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raws(t: &str) -> Vec<String> {
        tokenize(t).into_iter().map(|t| t.raw).collect()
    }

    #[test]
    fn fold_removes_niqqud_bidi_and_finals() {
        assert_eq!(fold("\u{202B}מַה יֵשׁ לִי הַיּוֹם\u{202C}"), "מה יש לי היומ");
        assert_eq!(fold("  Hello   WORLD "), "hello world");
        assert_eq!(fold("שלום ךםןףץ"), "שלומ כמנפצ");
        assert_eq!(fold("ה׳ ״x״ ’s"), "ה' \"x\" 's");
    }

    #[test]
    fn tokenize_keeps_hebrew_abbreviations() {
        assert_eq!(raws("ביום ה' הבא"), vec!["ביום", "ה'", "הבא"]);
        assert_eq!(raws("אחה\"צ ב-3"), vec!["אחה\"צ", "ב", "-", "3"]);
        assert_eq!(raws("ב3"), vec!["ב", "3"]);
        assert_eq!(raws("3pm 14:00 15.10 15/10 2026-10-15"), vec!["3pm", "14:00", "15.10", "15/10", "2026-10-15"]);
        assert_eq!(raws("budget.xlsx e-mail what's"), vec!["budget.xlsx", "e-mail", "what's"]);
    }

    #[test]
    fn tokenize_quotes_and_punctuation() {
        let t = tokenize("תחפש \"הצעת מחיר\" בבקשה?");
        assert_eq!(t.len(), 3);
        assert!(t[1].quoted);
        assert_eq!(t[1].raw, "הצעת מחיר");
        assert_eq!(raws("תשלום."), vec!["תשלום"]);
        assert_eq!(raws("12*(3+4)"), vec!["12", "*", "3", "+", "4"]);
    }
}
