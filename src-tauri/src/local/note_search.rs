//! Notes search: an in-memory scan of `notes::load()` (at most 500 notes) together with the user's
//! Windows Sticky Notes (read only, see `sticky_notes`; none when that is not available). Folded token
//! matching, ranked by matches x recency with a bonus for pinned notes. A Sticky Notes hit has the id
//! `sticky:<guid>`: the assistant labels it "Sticky Notes" and opens the Sticky Notes app for it.
//! Note text is never logged.

use super::{clip, NoteHit};
use crate::intent::fold;
use crate::notes::{self, Note};
use crate::sticky_notes::{self, StickyNote};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const TITLE_MAX: usize = 80;
const SNIPPET_MAX: usize = 120;
const DAY_MS: f64 = 86_400_000.0;

/// Sticky Notes as searchable notes: ids carry the `sticky:` prefix (so they can never collide with the
/// island's own ids and say where they open), nothing is pinned.
fn as_notes(sticky: &[StickyNote]) -> Vec<Note> {
    sticky
        .iter()
        .map(|s| Note { id: sticky_notes::hit_id(s), text: s.text.clone(), created_at: s.created_at, updated_at: s.updated_at, pinned: false })
        .collect()
}

pub(super) fn search(terms: &[Vec<String>], latest: bool, limit: usize) -> Result<Vec<NoteHit>, String> {
    let mut all = notes::load()?;
    // Never an error: without Sticky Notes (or when it cannot be read) there is just nothing extra.
    all.extend(as_notes(&sticky_notes::for_search().notes));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    Ok(rank_with(&all, terms, latest, limit, now, &mut cache))
}

/// Folding is the cost of a notes search (500 notes x 10,000 chars is about 5 million characters
/// to fold, 200 ms), so the folded lines are kept per note, keyed by its last edit, and dropped
/// after `CACHE_IDLE` without a search so a idle app holds nothing.
const CACHE_IDLE: Duration = Duration::from_secs(120);

struct FoldCache {
    /// note id -> (updated_at, text length, folded non-empty lines)
    map: HashMap<String, (i64, usize, Arc<Vec<String>>)>,
    last_used: Option<Instant>,
}

static CACHE: LazyLock<Mutex<FoldCache>> = LazyLock::new(|| Mutex::new(FoldCache { map: HashMap::new(), last_used: None }));

impl FoldCache {
    fn begin(&mut self, live: &[Note]) {
        if self.last_used.is_some_and(|t| t.elapsed() > CACHE_IDLE) || self.map.len() > live.len() {
            // Idle for long, or notes were deleted: rebuild from what is there.
            let ids: HashSet<&str> = live.iter().map(|n| n.id.as_str()).collect();
            if self.last_used.is_some_and(|t| t.elapsed() > CACHE_IDLE) {
                self.map.clear();
            } else {
                self.map.retain(|k, _| ids.contains(k.as_str()));
            }
        }
        self.last_used = Some(Instant::now());
    }

    fn lines(&mut self, n: &Note, lines: &[&str]) -> Arc<Vec<String>> {
        if let Some((u, len, f)) = self.map.get(&n.id) {
            if *u == n.updated_at && *len == n.text.len() {
                return f.clone();
            }
        }
        let f = Arc::new(lines.iter().map(|l| fold(l)).collect::<Vec<_>>());
        self.map.insert(n.id.clone(), (n.updated_at, n.text.len(), f.clone()));
        f
    }
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

#[cfg(test)]
pub(super) fn rank_notes(all: &[Note], terms: &[Vec<String>], latest: bool, limit: usize, now_ms: i64) -> Vec<NoteHit> {
    let mut cache = FoldCache { map: HashMap::new(), last_used: None };
    rank_with(all, terms, latest, limit, now_ms, &mut cache)
}

fn rank_with(all: &[Note], terms: &[Vec<String>], latest: bool, limit: usize, now_ms: i64, cache: &mut FoldCache) -> Vec<NoteHit> {
    let groups = fold_terms(terms);
    if groups.is_empty() && !latest {
        return Vec::new();
    }
    cache.begin(all);
    let mut scored: Vec<(f64, &Note, NoteHit)> = Vec::new();
    for n in all {
        if n.text.trim().is_empty() {
            continue;
        }
        let lines: Vec<&str> = n.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        // Nothing to match without terms ("latest"): no need to fold.
        let folded_lines: Arc<Vec<String>> = if groups.is_empty() { Arc::new(Vec::new()) } else { cache.lines(n, &lines) };
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

    /// Load: the maximum store (500 notes x 10,000 chars, Hebrew and English, 100 searches).
    /// Prints the timing; the bound is generous (debug build, shared CI machine).
    #[test]
    fn load_500_notes_of_10k_chars_100_searches() {
        let words = ["תקציב", "ישיבה", "לקוח", "budget", "meeting", "client", "פרויקט", "דוח", "invoice", "חשבונית"];
        let all: Vec<Note> = (0..500)
            .map(|i| {
                let mut text = format!("{} {}\n", words[i % words.len()], i);
                let mut w = i;
                while text.chars().count() < 10_000 {
                    w = w.wrapping_mul(31).wrapping_add(7);
                    text.push_str(words[w % words.len()]);
                    text.push(if w % 9 == 0 { '\n' } else { ' ' });
                }
                note(&format!("n{i}"), &text, (i % 90) as i64, i % 50 == 0)
            })
            .collect();
        assert!(all.iter().all(|n| n.text.chars().count() >= 10_000));
        let queries = [terms(&[&["תקציב", "budget"]]), terms(&[&["לקוח"], &["חשבונית", "invoice"]]), terms(&[&["zzzznotthere"]])];
        // Cold: every search folds all the text (no cache), as before the fold cache.
        let start = std::time::Instant::now();
        for i in 0..5 {
            rank_notes(&all, &queries[i % queries.len()], false, 5, NOW);
        }
        let cold = start.elapsed();
        // Warm: the folded lines are kept between searches.
        let mut cache = FoldCache { map: HashMap::new(), last_used: None };
        let start = std::time::Instant::now();
        let mut hits = 0;
        for i in 0..100 {
            hits += rank_with(&all, &queries[i % queries.len()], false, 5, NOW, &mut cache).len();
        }
        let total = start.elapsed();
        println!(
            "notes load: 500 x 10k chars, 100 searches: {} ms total, {:.1} ms per search warm; cold (no cache) {:.1} ms per search; {} hits",
            total.as_millis(),
            total.as_secs_f64() * 10.0,
            cold.as_secs_f64() * 200.0,
            hits
        );
        assert!(hits > 0);
        // Same answers with and without the cache.
        for q in &queries {
            let a: Vec<String> = rank_notes(&all, q, false, 5, NOW).into_iter().map(|h| h.id).collect();
            let b: Vec<String> = rank_with(&all, q, false, 5, NOW, &mut cache).into_iter().map(|h| h.id).collect();
            assert_eq!(a, b);
        }
        assert!(total < Duration::from_secs(30), "{total:?}");
    }

    #[test]
    fn fold_cache_follows_edits_and_deletes() {
        let mut cache = FoldCache { map: HashMap::new(), last_used: None };
        let mut v = vec![note("a", "כותרת\nתקציב", 1, false), note("b", "other\nline", 1, false)];
        assert_eq!(rank_with(&v, &terms(&[&["תקציב"]]), false, 5, NOW, &mut cache).len(), 1);
        assert_eq!(cache.map.len(), 2);
        // An edit (new text, new updated_at) must not be answered from the old folded lines.
        v[1].text = "other\nתקציב חדש".into();
        v[1].updated_at += 5;
        assert_eq!(rank_with(&v, &terms(&[&["תקציב"]]), false, 5, NOW, &mut cache).len(), 2);
        // A deleted note leaves the cache.
        v.remove(0);
        rank_with(&v, &terms(&[&["תקציב"]]), false, 5, NOW, &mut cache);
        assert_eq!(cache.map.len(), 1);
        // Idle for long: dropped.
        cache.last_used = Some(Instant::now() - CACHE_IDLE - Duration::from_secs(1));
        rank_with(&v, &terms(&[&["תקציב"]]), false, 5, NOW, &mut cache);
        assert_eq!(cache.map.len(), 1);
    }

    fn sticky(id: &str, text: &str, age_days: i64) -> StickyNote {
        StickyNote {
            id: id.into(),
            text: text.into(),
            title: sticky_notes::title_of(text),
            colour: "yellow".into(),
            updated_at: NOW - age_days * DAY,
            created_at: NOW - age_days * DAY - 1000,
        }
    }

    #[test]
    fn sticky_notes_are_searched_with_the_own_notes_and_keep_their_own_hit_id() {
        let mut all = fixture();
        all.extend(as_notes(&[
            sticky("11111111-aaaa", "תקציב רבעון\nלהכין מצגת\nלשלוח להנהלה", 3),
            sticky("22222222-bbbb", "Pick up the dry cleaning", 1),
        ]));
        let r = rank_notes(&all, &terms(&[&["תקציב", "budget"]]), false, 10, NOW);
        let ids: Vec<&str> = r.iter().map(|h| h.id.as_str()).collect();
        assert!(ids.contains(&"sticky:11111111-aaaa"), "{ids:?}");
        assert!(ids.contains(&"a") && ids.contains(&"c"), "the island's own notes still match: {ids:?}");
        assert!(!ids.contains(&"sticky:22222222-bbbb"));
        let hit = r.iter().find(|h| h.id == "sticky:11111111-aaaa").unwrap();
        assert_eq!(hit.title, "תקציב רבעון");
        assert!(!hit.pinned);
        assert!(sticky_notes::is_hit_id(&hit.id));
        assert!(ids.iter().filter(|i| !sticky_notes::is_hit_id(i)).all(|i| crate::notes::valid_id(i)));
    }

    #[test]
    fn sticky_notes_join_the_latest_and_a_group_needs_all_terms() {
        let mut all = fixture();
        all.extend(as_notes(&[sticky("s-new", "הערה אחרונה\nחשובה", 0), sticky("s-old", "ישן\nמאוד", 300)]));
        let r = rank_notes(&all, &[], true, 2, NOW);
        assert_eq!(r.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), vec!["sticky:s-new", "d"]);
        // AND of groups across a Sticky Note too.
        let r = rank_notes(&all, &terms(&[&["הערה"], &["חשובה"]]), false, 10, NOW);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "sticky:s-new");
        assert_eq!(r[0].snippet, "חשובה");
    }

    #[test]
    fn no_sticky_notes_changes_nothing() {
        assert!(as_notes(&[]).is_empty());
        let plain = rank_notes(&fixture(), &terms(&[&["תקציב"]]), false, 10, NOW);
        let mut with_none = fixture();
        with_none.extend(as_notes(&[]));
        assert_eq!(rank_notes(&with_none, &terms(&[&["תקציב"]]), false, 10, NOW), plain);
    }

    #[test]
    fn limit_and_blank_notes() {
        let mut v = fixture();
        v.push(note("e", "   \n  ", 0, false));
        assert_eq!(rank_notes(&v, &[], true, 100, NOW).len(), 4);
        assert_eq!(rank_notes(&v, &[], true, 1, NOW).len(), 1);
    }
}
