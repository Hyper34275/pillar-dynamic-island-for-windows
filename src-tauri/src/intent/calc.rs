//! The calculator: pulling an arithmetic expression out of a sentence, and evaluating it without
//! `eval` (a small recursive-descent parser).
//!
//! Grammar (ASCII after normalisation; `×`, `÷`, `x` are accepted too):
//! ```text
//! expr    = term (('+' | '-') term)*
//! term    = unary (('*' | '/') unary)*
//! unary   = '-' unary | power
//! power   = postfix ('^' unary)?
//! postfix = primary '%'*
//! primary = number | '(' expr ')'
//! ```
//! Semantics, pinned down:
//! - `x%` alone is `x / 100`, so "17% of 2400" is `17%*2400` = 408.
//! - In a sum the percent is of the left side, like every pocket calculator: `200+10%` = 220,
//!   `200-10%` = 180.
//! - Errors: `Syntax` (anything that does not parse, including an empty string or `,`),
//!   `DivideByZero` (including `0^-1`), `TooLarge` (result or intermediate beyond 1e15 in
//!   magnitude, a number literal over 15 digits, nesting deeper than 32, more than 200 chars).

use super::normalize::fold;
use super::CalcError;

const MAX_LEN: usize = 200;
const MAX_DEPTH: usize = 32;
const LIMIT: f64 = 1e15;

enum Node {
    Num(f64),
    Neg(Box<Node>),
    Pct(Box<Node>),
    Bin(Box<Node>, char, Box<Node>),
}

struct Parser<'a> {
    cs: Vec<char>,
    pos: usize,
    depth: usize,
    _src: &'a str,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.cs.get(self.pos).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<Node, CalcError> {
        let mut left = self.term()?;
        while let Some(op) = self.peek() {
            if op != '+' && op != '-' {
                break;
            }
            self.pos += 1;
            let right = self.term()?;
            left = Node::Bin(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Node, CalcError> {
        let mut left = self.unary()?;
        while let Some(op) = self.peek() {
            if op != '*' && op != '/' {
                break;
            }
            self.pos += 1;
            let right = self.unary()?;
            left = Node::Bin(Box::new(left), op, Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, CalcError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(CalcError::TooLarge);
        }
        let r = if self.eat('-') {
            self.unary().map(|n| Node::Neg(Box::new(n)))
        } else if self.eat('+') {
            self.unary()
        } else {
            self.power()
        };
        self.depth -= 1;
        r
    }

    fn power(&mut self) -> Result<Node, CalcError> {
        let base = self.postfix()?;
        if self.eat('^') {
            let exp = self.unary()?;
            return Ok(Node::Bin(Box::new(base), '^', Box::new(exp)));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Node, CalcError> {
        let mut n = self.primary()?;
        while self.eat('%') {
            n = Node::Pct(Box::new(n));
        }
        Ok(n)
    }

    fn primary(&mut self) -> Result<Node, CalcError> {
        if self.eat('(') {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err(CalcError::TooLarge);
            }
            let n = self.expr()?;
            self.depth -= 1;
            if !self.eat(')') {
                return Err(CalcError::Syntax);
            }
            return Ok(n);
        }
        let start = self.pos;
        let mut dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == '.' && !dot {
                dot = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let lit: String = self.cs[start..self.pos].iter().collect();
        if lit.is_empty() || lit == "." {
            return Err(CalcError::Syntax);
        }
        if lit.chars().filter(|c| c.is_ascii_digit()).count() > 15 {
            return Err(CalcError::TooLarge);
        }
        lit.parse::<f64>().map(Node::Num).map_err(|_| CalcError::Syntax)
    }
}

fn check(v: f64) -> Result<f64, CalcError> {
    if !v.is_finite() || v.abs() > LIMIT {
        Err(CalcError::TooLarge)
    } else {
        Ok(v)
    }
}

fn eval(n: &Node) -> Result<f64, CalcError> {
    match n {
        Node::Num(v) => Ok(*v),
        Node::Neg(a) => Ok(-eval(a)?),
        Node::Pct(a) => check(eval(a)? / 100.0),
        Node::Bin(a, op, b) => {
            let l = eval(a)?;
            let r = match (&**b, op) {
                // 200 + 10% -> 10% of 200
                (Node::Pct(inner), '+' | '-') => check(l * eval(inner)? / 100.0)?,
                _ => eval(b)?,
            };
            let v = match op {
                '+' => l + r,
                '-' => l - r,
                '*' => l * r,
                '/' => {
                    if r == 0.0 {
                        return Err(CalcError::DivideByZero);
                    }
                    l / r
                }
                _ => {
                    if l == 0.0 && r < 0.0 {
                        return Err(CalcError::DivideByZero);
                    }
                    l.powf(r)
                }
            };
            check(v)
        }
    }
}

/// Evaluate an arithmetic expression without `eval`: `+ - * / ^ %`, parentheses, unary minus.
pub fn evaluate_expr(expr: &str) -> Result<f64, CalcError> {
    if expr.chars().count() > MAX_LEN {
        return Err(CalcError::TooLarge);
    }
    // "2 3" is two numbers, not 23: whitespace between digits is a syntax error
    let raw: Vec<char> = expr.chars().collect();
    for w in 1..raw.len().saturating_sub(1) {
        if raw[w].is_whitespace() {
            let before = raw[..w].iter().rev().find(|c| !c.is_whitespace());
            let after = raw[w..].iter().find(|c| !c.is_whitespace());
            if matches!((before, after), (Some(a), Some(b)) if (a.is_ascii_digit() || *a == '.') && (b.is_ascii_digit() || *b == '.')) {
                return Err(CalcError::Syntax);
            }
        }
    }
    let cs: Vec<char> = expr
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '\u{00D7}' | 'x' | 'X' => '*',
            '\u{00F7}' => '/',
            '\u{2212}' => '-',
            c => c,
        })
        .collect();
    if cs.is_empty() {
        return Err(CalcError::Syntax);
    }
    let mut p = Parser { cs, pos: 0, depth: 0, _src: expr };
    let node = p.expr()?;
    if p.pos != p.cs.len() {
        return Err(CalcError::Syntax);
    }
    let mut v = check(eval(&node)?)?;
    // 17/100*2400 is 408.00000000000006 in binary floating point: round the noise away
    if v.abs() < 1e6 {
        v = (v * 1e9).round() / 1e9;
    }
    Ok(if v == 0.0 { 0.0 } else { v })
}

/// "1,000" -> "1000", "2,500,000" -> "2500000": a comma between a group of up to three digits and
/// exactly three more is a thousands separator. "1,23", "1,2,3" and "12,3456" are left alone.
/// Only sentences go through this; `evaluate_expr` stays strict (a bare "1,000" is a syntax error).
fn strip_thousands(t: &str) -> String {
    let cs: Vec<char> = t.chars().collect();
    let mut out = String::with_capacity(t.len());
    for (i, &c) in cs.iter().enumerate() {
        if c == ',' && i > 0 && cs[i - 1].is_ascii_digit() {
            let group_ok = (1..=3).all(|k| cs.get(i + k).map_or(false, |d| d.is_ascii_digit())) && !cs.get(i + 4).map_or(false, |d| d.is_ascii_digit());
            // the digits before the comma: one to three of them, not part of a decimal, no leading 0
            let mut s = i;
            while s > 0 && cs[s - 1].is_ascii_digit() {
                s -= 1;
            }
            let run = i - s;
            let after_group = s > 0 && cs[s - 1] == ',';
            let decimal = s > 0 && cs[s - 1] == '.';
            let lead_ok = after_group || cs[s] != '0';
            if group_ok && !decimal && lead_ok && ((1..=3).contains(&run)) && (!after_group || run == 3) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// A spelled-out number starting at `start` (words separated by single spaces): its value, the
/// index after its last word, and whether a "מ" ("of") was glued to the front of it ("ממאתיים",
/// only looked for right after a percent).
fn number_phrase(cs: &[char], start: usize, after_pct: bool) -> Option<(u64, usize, bool)> {
    let mut words: Vec<String> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let mut i = start;
    while words.len() < 6 && i < cs.len() && cs[i].is_alphabetic() {
        let s = i;
        while i < cs.len() && cs[i].is_alphabetic() {
            i += 1;
        }
        words.push(cs[s..i].iter().collect());
        ends.push(i);
        if i < cs.len() && cs[i] == ' ' {
            i += 1;
        } else {
            break;
        }
    }
    let refs: Vec<&str> = words.iter().map(|w| w.as_str()).collect();
    if let Some((v, k)) = super::numwords::parse(&refs) {
        return Some((v, ends[k - 1], false));
    }
    if after_pct {
        if let Some(rest) = words.first().and_then(|w| w.strip_prefix('מ')) {
            let mut refs = refs.clone();
            refs[0] = rest;
            if let Some((v, k)) = super::numwords::parse(&refs) {
                return Some((v, ends[k - 1], true));
            }
        }
    }
    None
}

/// "100/3", "7 / 0": only numbers and one slash, and not a date ("7/8" is the 7th of August).
fn is_plain_division(t: &str) -> bool {
    let s: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    if s.matches('/').count() != 1 || !s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '/') {
        return false;
    }
    let reference = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default();
    matches!(super::dates::numeric_date(&s, reference), super::dates::Numeric::NotDate)
}

/// Normalised expression found in a sentence, or `None`.
///
/// Numbers and operators are collected, filler words are ignored, and the spoken operators are
/// mapped: פלוס/ועוד/plus `+`, פחות/מינוס/minus `-`, כפול/times/x `*`, חלקי/divided by/over `/`,
/// אחוז/percent `%`, בחזקת/power `^`, and "of"/"מ"/"של" right after a percent `*`. The result must
/// parse; "a/b" and a lone minus (which are also how dates are written) need `trigger`, a
/// calculator word ("חשב", "כמה", "what is") in the sentence.
pub fn extract_expr(text: &str, trigger: bool) -> Option<String> {
    let t = strip_thousands(&fold(text));
    let cs: Vec<char> = t.chars().collect();
    let n = cs.len();
    let mut items: Vec<String> = Vec::new();
    let mut strong = false;
    let mut weak = false;
    let mut numbers = 0;
    // a percent was followed by a word that says how it continues, but not by a number
    let mut dangling = false;
    // "מע"מ 17% על 500": the VAT amount, i.e. 17% of 500
    let mut vat = false;
    let mut i = 0;
    let last_is_operand = |items: &Vec<String>| {
        items.last().map_or(false, |s| s == ")" || s == "%" || s.chars().next().map_or(false, |c| c.is_ascii_digit() || c == '.'))
    };
    while i < n {
        let c = cs[i];
        if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).map_or(false, |x| x.is_ascii_digit())) {
            let start = i;
            let mut dots = 0;
            while i < n && (cs[i].is_ascii_digit() || (cs[i] == '.' && cs.get(i + 1).map_or(false, |x| x.is_ascii_digit()))) {
                if cs[i] == '.' {
                    dots += 1;
                }
                i += 1;
            }
            if dots > 1 || cs.get(i).map_or(false, |x| *x == ':' || *x == '/' && cs.get(i + 1).map_or(false, |y| y.is_ascii_digit()) && dots > 0) {
                return None;
            }
            // two operands in a row means this is not an expression ("3 4")
            if last_is_operand(&items) {
                return None;
            }
            items.push(cs[start..i].iter().collect());
            numbers += 1;
            continue;
        }
        match c {
            '+' | '*' | '^' | '\u{00D7}' => {
                items.push(if c == '\u{00D7}' { "*".into() } else { c.to_string() });
                strong = true;
            }
            '\u{00F7}' => {
                items.push("/".into());
                strong = true;
            }
            '/' => {
                items.push("/".into());
                weak = true;
            }
            '-' => {
                // minus only after an operand (or as unary before a number); "ב-15" is a hyphen
                let prev_letter = i > 0 && cs[i - 1].is_alphabetic();
                if !prev_letter {
                    items.push("-".into());
                    weak = true;
                }
            }
            '%' => {
                items.push("%".into());
                strong = true;
            }
            '(' | ')' => items.push(c.to_string()),
            _ => {
                if c.is_alphabetic() {
                    let start = i;
                    while i < n && (cs[i].is_alphabetic() || cs[i] == '\'' || cs[i] == '"') {
                        i += 1;
                    }
                    let w: String = cs[start..i].iter().collect();
                    // a spelled-out number ("עשרים ושלוש", "ממאתיים" right after a percent)
                    let after_pct = items.last().map_or(false, |s| s == "%");
                    if let Some((value, end, glued_of)) = number_phrase(&cs, start, after_pct) {
                        if glued_of {
                            items.push("*".into());
                        } else if last_is_operand(&items) {
                            return None;
                        }
                        items.push(value.to_string());
                        numbers += 1;
                        dangling = false;
                        i = end;
                        continue;
                    }
                    if matches!(w.as_str(), "מע\"מ" | "מעמ" | "vat") {
                        vat = true;
                        continue;
                    }
                    let op = match w.as_str() {
                        "על" | "on" if vat && after_pct => Some("*"),
                        "פלוס" | "ועוד" | "plus" => Some("+"),
                        "פחות" | "מינוס" | "minus" => Some("-"),
                        "כפול" | "times" | "multiplied" => Some("*"),
                        "חלקי" | "divided" | "over" => Some("/"),
                        "אחוז" | "אחוזימ" | "percent" => Some("%"),
                        "בחזקת" | "power" => Some("^"),
                        "x" if last_is_operand(&items) && cs.get(i).map_or(false, |d| d.is_ascii_digit() || *d == ' ' || *d == '(') => Some("*"),
                        "מ" | "של" | "of" | "מתוכ" if items.last().map_or(false, |s| s == "%") => Some("*"),
                        _ => None,
                    };
                    if let Some(op) = op {
                        items.push(op.to_string());
                        strong = true; // spelled out, so not a date
                    } else if after_pct && !matches!(w.as_str(), "בבקשה" | "תודה" | "please" | "thanks" | "thank" | "you" | "לי") {
                        dangling = true;
                    }
                    continue;
                }
            }
        }
        i += 1;
    }
    if numbers == 0 || items.len() < 2 {
        return None;
    }
    // "17% from two hundred": a word we do not understand after the percent would leave a bare
    // "17%" (0.17), a confident wrong number. Better to ask.
    if dangling && items.last().map_or(false, |s| s == "%") {
        return None;
    }
    if !(strong || (weak && trigger) || is_plain_division(&t)) {
        return None;
    }
    let expr: String = items.concat();
    match evaluate_expr(&expr) {
        Err(CalcError::Syntax) => None,
        _ => Some(expr),
    }
}

/// True if the text has a word that asks for a calculation.
pub fn has_calc_word(text: &str) -> bool {
    let t = fold(text);
    t.split_whitespace().any(|w| {
        let w: String = w.chars().filter(|c| c.is_alphabetic()).collect();
        matches!(w.as_str(), "חשב" | "תחשב" | "חשבי" | "תחשבי" | "כמה" | "calculate" | "calc" | "compute" | "equals")
    }) || t.contains("what is ") || t.contains("whats ") || t.contains("how much")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> Result<f64, CalcError> {
        evaluate_expr(s)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("1+2*3"), Ok(7.0));
        assert_eq!(ev("(1+2)*3"), Ok(9.0));
        assert_eq!(ev("12*(3+4)"), Ok(84.0));
        assert_eq!(ev("2^3^2"), Ok(512.0));
        assert_eq!(ev("-2^2"), Ok(-4.0));
        assert_eq!(ev("2^-1"), Ok(0.5));
        assert_eq!(ev("10/4"), Ok(2.5));
        assert_eq!(ev("--3"), Ok(3.0));
        assert_eq!(ev(" 7 \u{00D7} 6 "), Ok(42.0));
        assert_eq!(ev("0.1+0.2").map(|v| (v * 10.0).round()), Ok(3.0));
    }

    #[test]
    fn percent_semantics() {
        assert_eq!(ev("17%*2400"), Ok(408.0));
        assert_eq!(ev("50%"), Ok(0.5));
        assert_eq!(ev("200+10%"), Ok(220.0));
        assert_eq!(ev("200-10%"), Ok(180.0));
    }

    #[test]
    fn errors() {
        assert_eq!(ev("5/0"), Err(CalcError::DivideByZero));
        assert_eq!(ev("0^-1"), Err(CalcError::DivideByZero));
        assert_eq!(ev(""), Err(CalcError::Syntax));
        assert_eq!(ev("2+"), Err(CalcError::Syntax));
        assert_eq!(ev("(2+3"), Err(CalcError::Syntax));
        assert_eq!(ev("2 3"), Err(CalcError::Syntax));
        assert_eq!(ev("1,000"), Err(CalcError::Syntax));
        assert_eq!(ev("2 + abc"), Err(CalcError::Syntax));
        assert_eq!(ev("9^99"), Err(CalcError::TooLarge));
        assert_eq!(ev("1234567890123456+1"), Err(CalcError::TooLarge));
        assert_eq!(ev(&"(".repeat(40)), Err(CalcError::TooLarge));
        assert_eq!(ev(&"1+".repeat(150)), Err(CalcError::TooLarge));
        // no code is ever evaluated
        assert_eq!(ev("system('calc')"), Err(CalcError::Syntax));
    }

    #[test]
    fn extracts_from_sentences() {
        assert_eq!(extract_expr("כמה זה 15 אחוז מ-200", true).as_deref(), Some("15%*200"));
        assert_eq!(extract_expr("17% מ-2400", false).as_deref(), Some("17%*2400"));
        assert_eq!(extract_expr("12*(3+4)", false).as_deref(), Some("12*(3+4)"));
        assert_eq!(extract_expr("what is 7 plus 5", true).as_deref(), Some("7+5"));
        assert_eq!(extract_expr("כמה זה 15 כפול 4", true).as_deref(), Some("15*4"));
        assert_eq!(extract_expr("תחשב 120 חלקי 8", true).as_deref(), Some("120/8"));
        assert_eq!(extract_expr("חשב 5 חלקי 0", true).as_deref(), Some("5/0"));
        assert_eq!(extract_expr("3x4", false).as_deref(), Some("3*4"));
    }

    #[test]
    fn percent_of_a_spelled_number_is_not_a_bare_percent() {
        // #31: "ממאתיים" used to be dropped and the answer was 0.17
        assert_eq!(extract_expr("17% ממאתיים", false).as_deref(), Some("17%*200"));
        assert_eq!(extract_expr("20% של חמש מאות", false).as_deref(), Some("20%*500"));
        // a continuation we do not understand asks instead of showing a confident 0.2
        assert_eq!(extract_expr("20% from five hundred", false), None);
        // politeness after the percent is fine
        assert_eq!(extract_expr("חשב 50% בבקשה", false).as_deref(), Some("50%"));
    }

    #[test]
    fn spelled_numbers_thousands_vat_and_plain_division() {
        assert_eq!(extract_expr("עשרים ושלוש כפול ארבע", false).as_deref(), Some("23*4"));
        assert_eq!(extract_expr("שבע פלוס שמונה", false).as_deref(), Some("7+8"));
        assert_eq!(extract_expr("1,000 * 1.17", false).as_deref(), Some("1000*1.17"));
        assert_eq!(extract_expr("2,500,000 חלקי 4", true).as_deref(), Some("2500000/4"));
        // not thousands separators
        assert_eq!(extract_expr("1,23 + 4", false), None);
        // VAT amount: X% of Y
        assert_eq!(extract_expr("מע\"מ 17% על 500", false).as_deref(), Some("17%*500"));
        assert_eq!(ev("17%*500"), Ok(85.0));
        // 100/3 and 1/0 are not dates; 7/8 and 15/10 are
        assert_eq!(extract_expr("100/3", false).as_deref(), Some("100/3"));
        assert_eq!(extract_expr("1/0", false).as_deref(), Some("1/0"));
        assert_eq!(extract_expr("7/8", false), None);
        assert_eq!(extract_expr("15/10", false), None);
        // evaluate_expr itself stays strict
        assert_eq!(ev("1,000"), Err(CalcError::Syntax));
    }

    #[test]
    fn dates_are_not_expressions() {
        assert_eq!(extract_expr("meetings on 15/10", false), None);
        assert_eq!(extract_expr("מה יש לי ב-15.10", false), None);
        assert_eq!(extract_expr("פגישות ב 3.11.26", false), None);
        assert_eq!(extract_expr("2026-10-15", false), None);
        assert_eq!(extract_expr("מה יש לי היום", false), None);
        assert_eq!(extract_expr("אני פנוי ב3", false), None);
    }
}
