//! Date and time understanding, Hebrew and English, against an injected `now`.
//!
//! Semantics, pinned down (each has a test or a corpus case):
//! - Weeks are Sunday-first. "השבוע" is Sunday..next Sunday (today inclusive).
//! - A bare weekday ("יום חמישי", "Thursday") is the nearest day from today on, TODAY INCLUDED, so
//!   on a Thursday "יום חמישי" is today. With "הבא"/"next" it is that weekday of NEXT week
//!   ("ביום ה' הבא" on Thursday 8.10 is 15.10, "next Monday" is Monday of next week). With
//!   "שעבר"/"last" it is the latest such day strictly before today.
//! - "dd.mm", "dd/mm" without a year is the nearest such date from today on (today inclusive);
//!   "dd.mm.yy(yy)" and ISO are exact. A date that does not exist (31.2, 29.2.2027) is an error
//!   (`Clarify(Date)`), never silently clamped. Day-first only.
//! - Parts of the day: בבוקר 06-12, בצהריים 12-14, אחה"צ / אחרי הצהריים 12-18, בערב 18-24, בלילה
//!   21-24. On their own they mean today.
//! - A bare hour 1-6 with no marker (בבוקר / אחה"צ / am / pm ...) is ambiguous: `Clarify(Time)`.
//!   7-12 are taken as written, 13-23 and 0 are 24-hour. An explicit hour is a one-hour window.
//! - "בעוד שעה/שעתיים/N שעות/דקות" is an `Instant` (from the offset moment, one hour long);
//!   "בעוד יומיים/N ימים/שבוע" is that day.
//! - Local midnights are built from the calendar date, not by adding 24 h, so DST days are right.

use super::lexicon::Ann;
use super::types::{AskKind, Grain, TimeSpec};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike};

#[derive(Clone, Debug, Default)]
pub struct DateParse {
    /// The resolved window. With an `error` of `Time`, the day only (hour unknown).
    pub time: Option<TimeSpec>,
    pub error: Option<AskKind>,
    /// A day / week / month / instant was named (not only a time of day).
    pub has_date: bool,
    /// A part of the day or an hour was named: minutes from midnight, `[start, end)`.
    pub tod: Option<(u32, u32)>,
}

/// Local time at `minutes` after midnight of `date` (>= 1440 rolls into the next day).
pub fn local_at(date: NaiveDate, minutes: u32) -> DateTime<Local> {
    let (d, m) = if minutes >= 1440 { (date + Duration::days(1), minutes - 1440) } else { (date, minutes) };
    let ndt = d.and_hms_opt(m / 60, m % 60, 0).unwrap_or_default();
    Local
        .from_local_datetime(&ndt)
        .earliest()
        // a local time skipped by a DST jump: take the hour after it
        .or_else(|| Local.from_local_datetime(&(ndt + Duration::hours(1))).earliest())
        .unwrap_or_else(|| Local.from_utc_datetime(&ndt))
}

pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_sunday() as i64)
}

fn span(from: NaiveDate, to: NaiveDate, grain: Grain) -> TimeSpec {
    TimeSpec { from: local_at(from, 0), to: local_at(to, 0), grain }
}

fn weekday_index(concept: &str) -> Option<u32> {
    Some(match concept {
        "D_SUN" => 0,
        "D_MON" => 1,
        "D_TUE" => 2,
        "D_WED" => 3,
        "D_THU" => 4,
        "D_FRI" => 5,
        "D_SAT" => 6,
        _ => return None,
    })
}

fn month_index(concept: &str) -> Option<u32> {
    concept.strip_prefix("MO_").and_then(|s| s.parse().ok())
}

#[derive(Clone, Copy, PartialEq)]
enum Mod {
    Next,
    Past,
    This,
}

/// The modifier next to token `i` (after it in Hebrew, before it in English); marks it used.
fn take_modifier(a: &mut [Ann], i: usize) -> Option<Mod> {
    let classify = |t: &Ann| -> Option<Mod> {
        if t.used {
            return None;
        }
        if t.is("T_NEXT") {
            Some(Mod::Next)
        } else if t.is("T_PAST") || t.norm() == "last" {
            Some(Mod::Past)
        } else if t.is("T_THIS") {
            Some(Mod::This)
        } else {
            None
        }
    };
    if i + 1 < a.len() {
        if let Some(m) = classify(&a[i + 1]) {
            a[i + 1].used = true;
            return Some(m);
        }
        // "החודש האחרון" / "השבוע האחרון": the last one, i.e. the previous
        if !a[i + 1].used && a[i + 1].is("M_LATEST_ONE") && matches!(a[i].concept(), "T_WEEK" | "T_MONTH") {
            a[i + 1].used = true;
            return Some(Mod::Past);
        }
    }
    if i > 0 {
        if let Some(m) = classify(&a[i - 1]) {
            a[i - 1].used = true;
            return Some(m);
        }
    }
    None
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let first = NaiveDate::from_ymd_opt(y, m, 1);
    let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, m + 1, 1) };
    match (first, next) {
        (Some(a), Some(b)) => (b - a).num_days() as u32,
        _ => 31,
    }
}

enum Numeric {
    Date(NaiveDate),
    /// Looked like a date but does not exist (31.2).
    Invalid,
    /// Not a date at all (a version number, 13.13 ...).
    NotDate,
}

/// `dd.mm`, `dd/mm`, `dd.mm.yy`, `dd.mm.yyyy`, `yyyy-mm-dd`.
fn numeric_date(s: &str, today: NaiveDate) -> Numeric {
    let sep = if s.contains('.') {
        '.'
    } else if s.contains('/') {
        '/'
    } else if s.matches('-').count() == 2 {
        '-'
    } else {
        return Numeric::NotDate;
    };
    let parts: Vec<&str> = s.split(sep).collect();
    if !(2..=3).contains(&parts.len()) || !parts.iter().all(|p| all_digits(p)) {
        return Numeric::NotDate;
    }
    let num = |p: &str| p.parse::<u32>().unwrap_or(0);
    let (d, m, y): (u32, u32, Option<i32>) = if sep == '-' {
        if parts[0].len() != 4 {
            return Numeric::NotDate;
        }
        (num(parts[2]), num(parts[1]), Some(num(parts[0]) as i32))
    } else {
        if parts[0].len() > 2 || parts[1].len() > 2 {
            return Numeric::NotDate;
        }
        let y = parts.get(2).map(|p| {
            let v = num(p) as i32;
            if p.len() == 2 {
                2000 + v
            } else {
                v
            }
        });
        if parts.get(2).map_or(false, |p| p.len() != 2 && p.len() != 4) {
            return Numeric::NotDate;
        }
        (num(parts[0]), num(parts[1]), y)
    };
    if !(1..=31).contains(&d) || !(1..=12).contains(&m) {
        return Numeric::NotDate;
    }
    match y {
        Some(y) => match NaiveDate::from_ymd_opt(y, m, d) {
            Some(date) => Numeric::Date(date),
            None => Numeric::Invalid,
        },
        None => nearest_future(today, m, d),
    }
}

/// The first `d/m` from `today` on (today included).
fn nearest_future(today: NaiveDate, m: u32, d: u32) -> Numeric {
    if d > days_in_month(today.year(), m) && d > days_in_month(today.year() + 1, m) {
        return Numeric::Invalid;
    }
    for y in [today.year(), today.year() + 1, today.year() + 2, today.year() + 3, today.year() + 4] {
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            if date >= today {
                return Numeric::Date(date);
            }
        }
    }
    Numeric::Invalid
}

/// Resolve an hour for the markers present. `definitive`: the user wrote it unambiguously
/// (24-hour, leading zero). Returns the 24-hour value, or `None` when it needs a question.
fn resolve_hour(h: u32, definitive: bool, pm: bool, am: bool, night: bool) -> Option<u32> {
    if h > 23 {
        return None;
    }
    if h == 0 || h >= 13 || definitive {
        return Some(h);
    }
    if night {
        return Some(if h >= 6 { (h + 12) % 24 } else { h });
    }
    if pm {
        return Some(if h < 12 { h + 12 } else { h });
    }
    if am {
        return Some(if h == 12 { 0 } else { h });
    }
    if h <= 6 {
        None
    } else {
        Some(h)
    }
}

/// Parse a clock token: "14:00", "9:30", "3pm", "3:30pm". Returns (hour, minute, definitive,
/// explicit pm, explicit am), or `Err` for an impossible time.
fn clock_token(s: &str) -> Option<Result<(u32, u32, bool, bool, bool), ()>> {
    let (body, pm, am) = if let Some(b) = s.strip_suffix("pm") {
        (b, true, false)
    } else if let Some(b) = s.strip_suffix("am") {
        (b, false, true)
    } else {
        (s, false, false)
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => {
            if !all_digits(h) || !all_digits(m) || h.len() > 2 || m.len() != 2 {
                return None;
            }
            (h, m)
        }
        None => {
            if !(pm || am) || !all_digits(body) || body.len() > 2 {
                return None;
            }
            (body, "00")
        }
    };
    let (hv, mv): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    if hv > 23 || mv > 59 || ((pm || am) && hv > 12) {
        return Some(Err(()));
    }
    let definitive = h.starts_with('0') && h.len() == 2 || hv >= 13;
    Some(Ok((hv, mv, definitive, pm, am)))
}

pub fn parse(a: &mut [Ann], now: DateTime<Local>) -> DateParse {
    let today = now.date_naive();
    let n = a.len();
    // The window [from, to) in dates, with its grain.
    let mut day: Option<(NaiveDate, NaiveDate, Grain)> = None;
    let mut tod: Option<(u32, u32)> = None;
    let mut hour: Option<(u32, u32, bool)> = None; // (h, m, definitive)
    let mut instant: Option<DateTime<Local>> = None;
    let mut error: Option<AskKind> = None;

    // markers that settle AM / PM for an explicit hour
    let live = |a: &[Ann], c: &[&str]| a.iter().any(|t| !t.used && c.iter().any(|x| t.is(x)));
    let pm = live(a, &["T_AFTERNOON", "T_EVENING", "T_NOON", "T_TONIGHT", "PM"]);
    let am = live(a, &["T_MORNING", "AM"]);
    let night = live(a, &["T_NIGHT"]);

    // ---- numeric tokens: clock times and numeric dates ----
    for i in 0..n {
        if a[i].used || a[i].tok.sym || a[i].tok.quoted {
            continue;
        }
        let s = a[i].tok.norm.clone();
        if !s.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            continue;
        }
        if s.contains(':') || s.ends_with("pm") || s.ends_with("am") {
            match clock_token(&s) {
                Some(Ok((h, m, definitive, tpm, tam))) => {
                    a[i].used = true;
                    match resolve_hour(h, definitive, pm || tpm, am || tam, night) {
                        Some(h24) => hour = Some((h24, m, true)),
                        None => {
                            error = error.or(Some(AskKind::Time));
                            hour = None;
                        }
                    }
                }
                Some(Err(())) => {
                    a[i].used = true;
                    error = error.or(Some(AskKind::Time));
                }
                None => {}
            }
            continue;
        }
        match numeric_date(&s, today) {
            Numeric::Date(d) => {
                a[i].used = true;
                day = Some((d, d + Duration::days(1), Grain::Day));
            }
            Numeric::Invalid => {
                a[i].used = true;
                error = Some(AskKind::Date);
            }
            Numeric::NotDate => {}
        }
    }

    // ---- offsets: "בעוד שעה", "in 2 days" ----
    for i in 0..n {
        if a[i].used || !a[i].is("T_IN") {
            continue;
        }
        let mut j = i + 1;
        while j < n && matches!(a[j].norm(), "a" | "an") {
            j += 1;
        }
        if j >= n {
            continue;
        }
        let count_of = |t: &Ann| if all_digits(t.norm()) { t.norm().parse::<i64>().ok() } else { None };
        let (qty, unit_at) = match count_of(&a[j]) {
            Some(q) => (q, j + 1),
            None => (1, j),
        };
        if unit_at >= n || a[unit_at].used {
            continue;
        }
        let u = &a[unit_at];
        let q = if u.is("U_HOUR2") || u.is("U_DAY2") || u.is("U_WEEK2") { 2 } else { qty };
        if !(0..=1000).contains(&q) {
            continue;
        }
        let in_hours = u.is("U_HOUR") || u.is("U_HOUR2");
        let in_mins = u.is("U_MIN");
        let in_days = u.is("U_DAYS") || u.is("U_DAY2") || u.is("T_DAY");
        let in_weeks = u.is("T_WEEK") || u.is("U_WEEK2");
        if in_hours || in_mins {
            let base = now.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(now);
            instant = Some(base + if in_hours { Duration::hours(q) } else { Duration::minutes(q) });
        } else if in_days || in_weeks {
            let d = today + Duration::days(if in_weeks { q * 7 } else { q });
            day = Some((d, d + Duration::days(1), Grain::Day));
        } else {
            continue;
        }
        for t in i..=unit_at {
            a[t].used = true;
        }
    }

    // ---- an explicit hour after a preposition: "ב-3", "בשעה 15", "at 3" ----
    for i in 0..n {
        if a[i].used || a[i].tok.sym || !all_digits(a[i].norm()) || a[i].norm().len() > 2 || hour.is_some() {
            continue;
        }
        let v: u32 = a[i].norm().parse().unwrap_or(99);
        let mut p = i;
        if p > 0 && a[p - 1].tok.sym && a[p - 1].norm() == "-" {
            p -= 1;
        }
        if p == 0 {
            continue;
        }
        let prev = &a[p - 1];
        let marker = !prev.used
            && (matches!(prev.norm(), "ב" | "ל" | "at" | "@" | "around" | "בסביבות") || (prev.is("U_HOUR") && prev.norm().ends_with("שעה")));
        if !marker {
            continue;
        }
        if let Some(next) = a.get(i + 1) {
            if next.starts("U_") || next.is("T_WEEK") || next.is("T_MONTH") || next.is("T_DAY") || next.starts("MO_") {
                continue;
            }
        }
        if v > 24 {
            continue;
        }
        match resolve_hour(v, false, pm, am, night) {
            Some(h24) => hour = Some((h24, 0, true)),
            None => error = error.or(Some(AskKind::Time)),
        }
        a[i].used = true;
        for t in p - 1..p {
            a[t].used = true;
        }
        if p != i {
            a[p].used = true;
        }
    }

    // ---- words: days, weeks, months, weekdays, parts of the day ----
    for i in 0..n {
        if a[i].used || a[i].tok.sym || a[i].tok.quoted {
            continue;
        }
        let Some(hit) = a[i].hit.clone() else { continue };
        let c = hit.concept;
        match c {
            "T_TODAY" | "T_TOMORROW" | "T_DAYAFTER" | "T_YESTERDAY" | "T_2DAGO" => {
                let off = match c {
                    "T_TODAY" => 0,
                    "T_TOMORROW" => 1,
                    "T_DAYAFTER" => 2,
                    "T_YESTERDAY" => -1,
                    _ => -2,
                };
                let d = today + Duration::days(off);
                day = Some((d, d + Duration::days(1), Grain::Day));
                a[i].used = true;
            }
            "T_NOW" => {
                instant = Some(now.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(now));
                a[i].used = true;
            }
            "T_TONIGHT" => {
                day = Some((today, today + Duration::days(1), Grain::Day));
                tod = Some((18 * 60, 24 * 60));
                a[i].used = true;
            }
            "T_WEEKEND" | "T_WEEK" | "T_MONTH" => {
                let is_week = c != "T_MONTH";
                let weekend = c == "T_WEEKEND" || (c == "T_WEEK" && i > 0 && a[i - 1].is("T_END") && !a[i - 1].used);
                if c == "T_WEEK" && weekend {
                    a[i - 1].used = true;
                }
                let m = take_modifier(a, i);
                if is_week {
                    let start = week_start(today)
                        + Duration::days(match m {
                            Some(Mod::Next) => 7,
                            Some(Mod::Past) => -7,
                            _ => 0,
                        });
                    if weekend {
                        day = Some((start + Duration::days(5), start + Duration::days(7), Grain::Range));
                    } else {
                        day = Some((start, start + Duration::days(7), Grain::Week));
                    }
                } else {
                    let (y, mo) = (today.year(), today.month() as i32);
                    let shift = match m {
                        Some(Mod::Next) => 1,
                        Some(Mod::Past) => -1,
                        _ => 0,
                    };
                    let idx = y * 12 + mo - 1 + shift;
                    let (ny, nm) = (idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1);
                    let first = NaiveDate::from_ymd_opt(ny, nm, 1).unwrap_or(today);
                    let next = if nm == 12 { NaiveDate::from_ymd_opt(ny + 1, 1, 1) } else { NaiveDate::from_ymd_opt(ny, nm + 1, 1) };
                    day = Some((first, next.unwrap_or(first + Duration::days(30)), Grain::Range));
                }
                a[i].used = true;
            }
            _ if weekday_index(c).is_some() => {
                let idx = weekday_index(c).unwrap_or(0);
                let he = a[i].tok.norm.chars().any(super::normalize::is_he);
                let letter_form = a[i].tok.norm.ends_with('\'') && a[i].tok.norm.chars().count() == 2;
                let prev_day = i > 0 && a[i - 1].is("T_DAY");
                let pre_ok = hit.prefix.ends_with('ב') || hit.prefix.ends_with('ל');
                let ok = !he || prev_day || (pre_ok && !letter_form) || a[i].tok.norm == "שבת";
                if !ok || (letter_form && !prev_day) {
                    continue;
                }
                let m = take_modifier(a, i);
                let cur = today.weekday().num_days_from_sunday();
                let date = match m {
                    Some(Mod::Next) => week_start(today) + Duration::days(7 + idx as i64),
                    Some(Mod::Past) => {
                        let back = (cur + 7 - idx) % 7;
                        today - Duration::days(if back == 0 { 7 } else { back as i64 })
                    }
                    _ => today + Duration::days(((idx + 7 - cur) % 7) as i64),
                };
                day = Some((date, date + Duration::days(1), Grain::Day));
                a[i].used = true;
                if prev_day {
                    a[i - 1].used = true;
                }
            }
            _ if month_index(c).is_some() => {
                let mo = month_index(c).unwrap_or(1);
                // the day number next to the month name ("15 באוקטובר", "October 15")
                let mut num: Option<usize> = None;
                for k in [i.wrapping_sub(1), i.wrapping_sub(2), i + 1] {
                    if k < n && !a[k].used && all_digits(a[k].norm()) && a[k].norm().len() <= 2 {
                        let between_ok = k + 1 == i || k + 2 == i && a[k + 1].tok.sym || k == i + 1 || k + 2 == i && a[k + 1].norm().chars().count() == 1;
                        if between_ok {
                            num = Some(k);
                            break;
                        }
                    }
                }
                match num {
                    Some(k) => {
                        let d: u32 = a[k].norm().parse().unwrap_or(0);
                        match nearest_future(today, mo, d) {
                            Numeric::Date(date) if (1..=31).contains(&d) => {
                                day = Some((date, date + Duration::days(1), Grain::Day));
                            }
                            _ => error = Some(AskKind::Date),
                        }
                        for t in k.min(i)..=k.max(i) {
                            a[t].used = true;
                        }
                    }
                    None => {
                        let y = if mo >= today.month() { today.year() } else { today.year() + 1 };
                        let first = NaiveDate::from_ymd_opt(y, mo, 1).unwrap_or(today);
                        let next = if mo == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, mo + 1, 1) };
                        day = Some((first, next.unwrap_or(first + Duration::days(30)), Grain::Range));
                        a[i].used = true;
                    }
                }
            }
            "T_AFTER" => {
                if i + 1 < n && a[i + 1].is("T_NOON") && !a[i + 1].used {
                    a[i].used = true;
                    a[i + 1].used = true;
                    tod = Some((12 * 60, 18 * 60));
                }
            }
            "T_MORNING" | "T_NOON" | "T_AFTERNOON" | "T_EVENING" | "T_NIGHT" => {
                a[i].used = true;
                if hour.is_none() {
                    tod = Some(match c {
                        "T_MORNING" => (6 * 60, 12 * 60),
                        "T_NOON" => (12 * 60, 14 * 60),
                        "T_AFTERNOON" => (12 * 60, 18 * 60),
                        "T_EVENING" => (18 * 60, 24 * 60),
                        _ => (21 * 60, 24 * 60),
                    });
                }
            }
            "AM" | "PM" => a[i].used = true,
            _ => {}
        }
    }

    // ---- assemble ----
    let has_date = day.is_some() || instant.is_some();
    let window = hour.map(|(h, m, _)| (h * 60 + m, h * 60 + m + 60)).or(tod);
    if error == Some(AskKind::Date) {
        return DateParse { time: None, error, has_date, tod: window };
    }
    let time = if let Some(inst) = instant {
        Some(TimeSpec { from: inst, to: inst + Duration::hours(1), grain: Grain::Instant })
    } else if error == Some(AskKind::Time) {
        let (d0, d1, g) = day.unwrap_or((today, today + Duration::days(1), Grain::Day));
        Some(span(d0, d1, g))
    } else if day.is_some() || window.is_some() {
        let (d0, d1, g) = day.unwrap_or((today, today + Duration::days(1), Grain::Day));
        match (g, window) {
            (Grain::Day, Some((s, e))) => Some(TimeSpec { from: local_at(d0, s), to: local_at(d0, e), grain: Grain::Range }),
            _ => Some(span(d0, d1, g)),
        }
    } else {
        None
    };
    DateParse { time, error, has_date, tod: window }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::lexicon::annotate;
    use crate::intent::normalize::tokenize;
    use chrono::NaiveDateTime;

    /// Thursday 2026-10-08 09:00 local.
    fn now() -> DateTime<Local> {
        let ndt = NaiveDateTime::parse_from_str("2026-10-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
        Local.from_local_datetime(&ndt).earliest().unwrap()
    }

    fn p(text: &str) -> DateParse {
        let mut a = annotate(&tokenize(text));
        parse(&mut a, now())
    }

    fn range(text: &str) -> (String, String, Grain) {
        let t = p(text).time.unwrap_or_else(|| panic!("no time for {text}"));
        (t.from.format("%Y-%m-%dT%H:%M").to_string(), t.to.format("%Y-%m-%dT%H:%M").to_string(), t.grain)
    }

    #[test]
    fn day_words() {
        assert_eq!(range("היום"), ("2026-10-08T00:00".into(), "2026-10-09T00:00".into(), Grain::Day));
        assert_eq!(range("מחר").0, "2026-10-09T00:00");
        assert_eq!(range("מחרתיים").0, "2026-10-10T00:00");
        assert_eq!(range("אתמול").0, "2026-10-07T00:00");
        assert_eq!(range("שלשום").0, "2026-10-06T00:00");
        assert_eq!(range("tomorrow").0, "2026-10-09T00:00");
        assert_eq!(range("yesterday").0, "2026-10-07T00:00");
    }

    #[test]
    fn weeks_are_sunday_first() {
        assert_eq!(range("השבוע"), ("2026-10-04T00:00".into(), "2026-10-11T00:00".into(), Grain::Week));
        assert_eq!(range("בשבוע הבא").0, "2026-10-11T00:00");
        assert_eq!(range("בשבוע שעבר").0, "2026-09-27T00:00");
        assert_eq!(range("מהשבוע שעבר").0, "2026-09-27T00:00");
        assert_eq!(range("next week").0, "2026-10-11T00:00");
        assert_eq!(range("last week").0, "2026-09-27T00:00");
        assert_eq!(range("סוף השבוע"), ("2026-10-09T00:00".into(), "2026-10-11T00:00".into(), Grain::Range));
        assert_eq!(range("החודש").0, "2026-10-01T00:00");
        assert_eq!(range("חודש הבא").0, "2026-11-01T00:00");
    }

    #[test]
    fn weekdays_today_included() {
        // today is Thursday
        assert_eq!(range("ביום חמישי").0, "2026-10-08T00:00");
        assert_eq!(range("Thursday").0, "2026-10-08T00:00");
        assert_eq!(range("ביום ה' הבא").0, "2026-10-15T00:00");
        assert_eq!(range("ביום חמישי הבא").0, "2026-10-15T00:00");
        assert_eq!(range("next Thursday").0, "2026-10-15T00:00");
        assert_eq!(range("ביום ראשון").0, "2026-10-11T00:00");
        assert_eq!(range("next Monday").0, "2026-10-12T00:00");
        assert_eq!(range("Tuesday").0, "2026-10-13T00:00");
        assert_eq!(range("ביום שלישי שעבר").0, "2026-10-06T00:00");
        assert_eq!(range("last Thursday").0, "2026-10-01T00:00");
        // a bare "שני" is not Monday (it also means "second")
        assert!(p("שני מיילים").time.is_none());
        assert_eq!(range("ברביעי").0, "2026-10-14T00:00");
    }

    #[test]
    fn numeric_dates() {
        assert_eq!(range("15.10").0, "2026-10-15T00:00");
        assert_eq!(range("15/10").0, "2026-10-15T00:00");
        assert_eq!(range("3.11.26").0, "2026-11-03T00:00");
        assert_eq!(range("3.11.2026").0, "2026-11-03T00:00");
        assert_eq!(range("2026-10-15").0, "2026-10-15T00:00");
        // already past this year -> next year
        assert_eq!(range("1.3").0, "2027-03-01T00:00");
        assert_eq!(range("8.10").0, "2026-10-08T00:00");
        assert_eq!(range("15 באוקטובר").0, "2026-10-15T00:00");
        assert_eq!(range("October 20").0, "2026-10-20T00:00");
    }

    #[test]
    fn invalid_dates_are_errors() {
        assert_eq!(p("31.2").error, Some(AskKind::Date));
        assert!(p("31.2").time.is_none());
        assert_eq!(p("29.2.2027").error, Some(AskKind::Date));
        assert_eq!(p("31 בנובמבר").error, Some(AskKind::Date));
        // not dates: no error, no time
        assert!(p("13.13").error.is_none());
        assert!(p("13.13").time.is_none());
    }

    #[test]
    fn parts_of_day_and_hours() {
        assert_eq!(range("ביום ראשון בבוקר"), ("2026-10-11T06:00".into(), "2026-10-11T12:00".into(), Grain::Range));
        assert_eq!(range("היום אחרי הצהריים"), ("2026-10-08T12:00".into(), "2026-10-08T18:00".into(), Grain::Range));
        assert_eq!(range("הערב").0, "2026-10-08T18:00");
        assert_eq!(range("מחר ב-14:00"), ("2026-10-09T14:00".into(), "2026-10-09T15:00".into(), Grain::Range));
        assert_eq!(range("מחר בשעה 15").0, "2026-10-09T15:00");
        assert_eq!(range("מחר ב3 אחה\"צ").0, "2026-10-09T15:00");
        assert_eq!(range("tomorrow at 3pm").0, "2026-10-09T15:00");
        assert_eq!(range("מחר ב-9").0, "2026-10-09T09:00");
        assert_eq!(range("מחר ב-2 בלילה").0, "2026-10-09T02:00");
        assert_eq!(range("מחר ב-8 בערב").0, "2026-10-09T20:00");
    }

    #[test]
    fn ambiguous_hour_asks() {
        let r = p("מחר ב3");
        assert_eq!(r.error, Some(AskKind::Time));
        // the day is kept, so the answer "אחה\"צ" can complete it
        assert_eq!(r.time.unwrap().from.format("%Y-%m-%dT%H:%M").to_string(), "2026-10-09T00:00");
        assert_eq!(p("ב3").error, Some(AskKind::Time));
        assert_eq!(p("at 5").error, Some(AskKind::Time));
        assert_eq!(p("25:00").error, Some(AskKind::Time));
        assert!(p("ב-3 ימים").error.is_none());
    }

    #[test]
    fn relative_offsets() {
        assert_eq!(range("בעוד שעה"), ("2026-10-08T10:00".into(), "2026-10-08T11:00".into(), Grain::Instant));
        assert_eq!(range("בעוד שעתיים").0, "2026-10-08T11:00");
        assert_eq!(range("בעוד 3 שעות").0, "2026-10-08T12:00");
        assert_eq!(range("בעוד 30 דקות").0, "2026-10-08T09:30");
        assert_eq!(range("in 2 hours").0, "2026-10-08T11:00");
        assert_eq!(range("בעוד יומיים"), ("2026-10-10T00:00".into(), "2026-10-11T00:00".into(), Grain::Day));
        assert_eq!(range("בעוד 3 ימים").0, "2026-10-11T00:00");
        assert_eq!(range("in 3 days").0, "2026-10-11T00:00");
        assert_eq!(range("בעוד שבוע").0, "2026-10-15T00:00");
        assert!(p("in the inbox").time.is_none());
    }

    #[test]
    fn dst_days_are_built_from_dates() {
        // wherever the test runs, midnight to midnight of a date is that date
        let d = NaiveDate::from_ymd_opt(2026, 3, 27).unwrap();
        let t = span(d, d + Duration::days(1), Grain::Day);
        assert_eq!(t.from.date_naive(), d);
        assert_eq!(t.to.date_naive(), d + Duration::days(1));
    }
}
