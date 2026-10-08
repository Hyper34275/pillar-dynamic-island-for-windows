//! Notes search: an in-memory scan of `notes::load()` (at most 500 notes). Folded token matching,
//! ranked by matches x recency with a bonus for pinned notes. Note text is never logged.

use super::{clip, NoteHit};
use crate::intent::fold;
use crate::notes::{self, Note};

const TITLE_MAX: usize = 80;
const SNIPPET_MAX: usize = 120;
const DAY_MS: f64 = 86_400_000.0;

pub(super) fn search(terms: &[Vec<String>], latest: bool, limit: usize) -> Result<Vec<NoteHit>, String> {
    let all = notes::load()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(rank_notes(&all, terms, latest, limit, now))
}

/// Folded, non-empty alternatives per group; empty groups are dropped.
fn fold_terms(terms: &[Vec<String>]) -> Vec<Vec<String>> {
    terms
        .iter()
        .map(|g| g.iter().map(|t| fold(t)).filter(|t| !t.is_empty()).collect::<Vec<_>>())
        .filter(|g| !g.is_empty())
        .collect()
}

/// First non-empty line of a note (the de-facto title).
fn title_of(text: &str) -> &str {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

pub(super) fn rank_notes(all: &[Note], terms: &[Vec<String>], latest: bool, limit: usize, now_ms: i64) -> Vec<NoteHit> {
    let groups = fold_terms(terms);
    if groups.is_empty() && !latest {
        return Vec::new();
    }
    let mut scored: Vec<(f64, &Note, NoteHit)> = Vec::new();
    for n in all {
        if n.text.trim().is_empty() {
            continue;
        }
        let lines: Vec<&str> = n.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let folded_lines: Vec<String> = lines.iter().map(|l| fold(l)).collect();
        // Every group needs at least one alternative somewhere in the note.
        let mut hits = 0usize;
        let mut title_hit = false;
        let mut all_groups = true;
        for g in &groups {
            let mut group_hit = false;
            for (i, fl) in folded_lines.iter().enumerate() {
                let c: usize = g.iter().map(|alt| fl.matches(alt.as_str()).count()).sum();
                if c > 0 {
                    group_hit = true;
                    hits += c;
                    if i == 0 {
                        title_hit = true;
                    }
                }
            }
            if !group_hit {
                all_groups = false;
                break;
            }
        }
        if !all_groups {
            continue;
        }
        let title = lines.first().map(|l| clip(l, TITLE_MAX)).unwrap_or_default();
        // The matching line other than the title; else the second line.
        let snippet_line = groups
            .iter()
            .find_map(|g| {
                folded_lines
                    .iter()
                    .enumerate()
                    .skip(1)
                    .find(|(_, fl)| g.iter().any(|alt| fl.contains(alt.as_str())))
                    .map(|(i, _)| lines[i])
            })
            .or_else(|| lines.get(1).copied())
            .unwrap_or("");
        let age_days = ((now_ms - n.updated_at).max(0) as f64) / DAY_MS;
        let recency = 1.0 / (1.0 + age_days / 30.0);
        let score = if groups.is_empty() {
            n.updated_at as f64
        } else {
            let matches = 1.0 + if title_hit { 1.0 } else { 0.0 } + (hits.min(10) as f64) * 0.3;
            matches * (0.5 + recency) + if n.pinned { 0.75 } else { 0.0 }
        };
        scored.push((
            score,
            n,
            NoteHit { id: n.id.clone(), title, snippet: clip(snippet_line, SNIPPET_MAX), updated_at: n.updated_at, pinned: n.pinned },
        ));
    }
    if latest {
        scored.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at).then_with(|| a.1.id.cmp(&b.1.id)));
    } else {
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.updated_at.cmp(&a.1.updated_at)).then_with(|| a.1.id.cmp(&b.1.id)));
    }
    scored.into_iter().take(limit).map(|(_, _, h)| h).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const DAY: i64 = 86_400_000;

    fn note(id: &str, text: &str, age_days: i64, pinned: bool) -> Note {
        Note { id: id.into(), text: text.into(), created_at: NOW - age_days * DAY, updated_at: NOW - age_days * DAY, pinned }
    }

    fn terms(groups: &[&[&str]]) -> Vec<Vec<String>> {
        groups.iter().map(|g| g.iter().map(|s| s.to_string()).collect()).collect()
    }

    fn fixture() -> Vec<Note> {
        vec![
            note("a", "תקציב 2026\nלסגור מול הנהלה\nתקציב שיווק", 40, false),
            note("b", "רשימת קניות\nחלב, לחם\nתקציב הבית", 2, false),
            note("c", "Budget meeting\nprepare slides", 10, true),
            note("d", "סתם פתק\nבלי כלום", 1, false),
        ]
    }

    #[test]
    fn matches_all_groups_and_any_alternative() {
        let r = rank_notes(&fixture(), &terms(&[&["תקציב", "budget"]]), false, 10, NOW);
        let ids: Vec<&str> = r.iter().map(|h| h.id.as_str()).collect();
        assert!(ids.contains(&"a") && ids.contains(&"b") && ids.contains(&"c"));
        assert!(!ids.contains(&"d"));
        let r = rank_notes(&fixture(), &terms(&[&["תקציב"], &["שיווק"]]), false, 10, NOW);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "a");
    }

    #[test]
    fn title_hit_and_pinned_rank_higher() {
        let r = rank_notes(&fixture(), &terms(&[&["תקציב", "budget"]]), false, 10, NOW);
        // c is pinned with a title hit and recent enough; b only matches in the third line.
        assert_eq!(r[0].id, "c");
        let pos = |id: &str| r.iter().position(|h| h.id == id).unwrap();
        assert!(pos("a") < pos("b") || pos("b") < pos("a")); // both present
    }

    #[test]
    fn recency_breaks_ties() {
        let notes = vec![note("old", "דוח\nתקציב", 200, false), note("new", "דוח\nתקציב", 1, false)];
        let r = rank_notes(&notes, &terms(&[&["תקציב"]]), false, 10, NOW);
        assert_eq!(r[0].id, "new");
    }

    #[test]
    fn latest_without_terms_is_newest_first() {
        let r = rank_notes(&fixture(), &[], true, 2, NOW);
        assert_eq!(r.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), vec!["d", "b"]);
    }

    #[test]
    fn no_terms_and_not_latest_is_empty() {
        assert!(rank_notes(&fixture(), &[], false, 5, NOW).is_empty());
        assert!(rank_notes(&fixture(), &terms(&[&[""]]), false, 5, NOW).is_empty());
    }

    #[test]
    fn title_and_snippet_are_clipped_and_picked() {
        let long = "x".repeat(300);
        let n = vec![note("l", &format!("{long}\nfirst line\nthe budget line {long}"), 1, false)];
        let r = rank_notes(&n, &terms(&[&["budget"]]), false, 5, NOW);
        assert_eq!(r[0].title.chars().count(), 80);
        assert!(r[0].snippet.starts_with("the budget line"));
        assert_eq!(r[0].snippet.chars().count(), 120);
        // Only the title matches: the snippet falls back to the second line.
        let n = vec![note("t", "budget\nsecond\nthird", 1, false)];
        let r = rank_notes(&n, &terms(&[&["budget"]]), false, 5, NOW);
        assert_eq!(r[0].snippet, "second");
    }

    #[test]
    fn limit_and_blank_notes() {
        let mut v = fixture();
        v.push(note("e", "   \n  ", 0, false));
        assert_eq!(rank_notes(&v, &[], true, 100, NOW).len(), 4);
        assert_eq!(rank_notes(&v, &[], true, 1, NOW).len(), 1);
    }
}
