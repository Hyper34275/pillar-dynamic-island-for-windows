//! Availability arithmetic (pure): busy intervals, overlap, free slots in working hours.

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};

pub const WORK_START_HOUR: u32 = 8;
pub const WORK_END_HOUR: u32 = 18;
/// Shorter gaps are not worth listing.
const MIN_SLOT_MIN: i64 = 15;
const MAX_DAYS: i64 = 32;

pub type Span = (DateTime<Utc>, DateTime<Utc>);

/// Sorted, overlapping/touching intervals merged.
pub fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.retain(|(s, e)| e > s);
    spans.sort();
    let mut out: Vec<Span> = Vec::new();
    for (s, e) in spans {
        match out.last_mut() {
            Some(last) if s <= last.1 => {
                if e > last.1 {
                    last.1 = e;
                }
            }
            _ => out.push((s, e)),
        }
    }
    out
}

/// Does any busy interval overlap `[from, to)`?
pub fn overlaps(busy: &[Span], from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    busy.iter().any(|(s, e)| *s < to && *e > from)
}

fn local_at(day: NaiveDate, hour: u32) -> Option<DateTime<Local>> {
    let naive = day.and_hms_opt(hour, 0, 0)?;
    Local.from_local_datetime(&naive).earliest()
}

/// Free slots of 08:00-18:00 local on each day of `[from, to)`, minus the busy intervals.
pub fn free_slots(busy: &[Span], from: DateTime<Local>, to: DateTime<Local>) -> Vec<Span> {
    let busy = merge(busy.to_vec());
    let (from_u, to_u) = (from.with_timezone(&Utc), to.with_timezone(&Utc));
    let mut out = Vec::new();
    let mut day = from.date_naive();
    for _ in 0..MAX_DAYS {
        let (Some(ws), Some(we)) = (local_at(day, WORK_START_HOUR), local_at(day, WORK_END_HOUR)) else {
            break;
        };
        if ws.with_timezone(&Utc) >= to_u {
            break;
        }
        let mut cursor = ws.with_timezone(&Utc).max(from_u);
        let end = we.with_timezone(&Utc).min(to_u);
        let start = cursor;
        for (s, e) in busy.iter().filter(|(s, e)| *s < end && *e > start) {
            if *s > cursor && (*s - cursor) >= Duration::minutes(MIN_SLOT_MIN) {
                out.push((cursor, *s));
            }
            if *e > cursor {
                cursor = *e;
            }
        }
        if end > cursor && (end - cursor) >= Duration::minutes(MIN_SLOT_MIN) {
            out.push((cursor, end));
        }
        day = day.succ_opt().unwrap_or(day);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2027, 3, 10, h, m, 0).unwrap()
    }
    fn u(h: u32, m: u32) -> DateTime<Utc> {
        l(h, m).with_timezone(&Utc)
    }
    fn day() -> (DateTime<Local>, DateTime<Local>) {
        (l(0, 0), Local.with_ymd_and_hms(2027, 3, 11, 0, 0, 0).unwrap())
    }

    #[test]
    fn merges_touching_and_overlapping() {
        let m = merge(vec![(u(9, 0), u(10, 0)), (u(9, 30), u(11, 0)), (u(11, 0), u(12, 0)), (u(14, 0), u(15, 0))]);
        assert_eq!(m, vec![(u(9, 0), u(12, 0)), (u(14, 0), u(15, 0))]);
    }

    #[test]
    fn overlap_is_half_open() {
        let busy = vec![(u(9, 0), u(10, 0))];
        assert!(overlaps(&busy, u(9, 30), u(10, 30)));
        assert!(!overlaps(&busy, u(10, 0), u(11, 0)));
        assert!(!overlaps(&busy, u(8, 0), u(9, 0)));
    }

    #[test]
    fn slots_between_meetings_in_working_hours() {
        let (f, t) = day();
        let slots = free_slots(&[(u(9, 0), u(10, 0)), (u(11, 30), u(12, 0))], f, t);
        assert_eq!(slots, vec![(u(8, 0), u(9, 0)), (u(10, 0), u(11, 30)), (u(12, 0), u(18, 0))]);
    }

    #[test]
    fn empty_day_is_one_slot_and_full_day_none() {
        let (f, t) = day();
        assert_eq!(free_slots(&[], f, t), vec![(u(8, 0), u(18, 0))]);
        assert!(free_slots(&[(u(7, 0), u(19, 0))], f, t).is_empty());
    }

    #[test]
    fn busy_outside_working_hours_and_short_gaps() {
        let (f, t) = day();
        let slots = free_slots(&[(u(6, 0), u(8, 10)), (u(8, 20), u(18, 30))], f, t);
        // 8:10-8:20 is shorter than 15 minutes
        assert!(slots.is_empty());
    }

    #[test]
    fn window_starting_midday_is_clipped() {
        let t = Local.with_ymd_and_hms(2027, 3, 11, 0, 0, 0).unwrap();
        assert_eq!(free_slots(&[], l(13, 0), t), vec![(u(13, 0), u(18, 0))]);
        assert!(free_slots(&[], l(19, 0), t).is_empty());
    }
}
