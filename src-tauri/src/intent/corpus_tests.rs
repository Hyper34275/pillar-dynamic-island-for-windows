//! The corpus gates. `corpus.jsonl` holds the labelled sentences (he / en / mixed, typos,
//! follow-ups, refinements, safety). Every case runs with a fixed clock (Thursday 2026-10-08
//! 09:00 local) and a fixed `Known`; cases with a `ctx` first run the case they point at and
//! remember its result. Gates (see `docs/AI_SEARCH.md` 4.1):
//! - execute precision >= 98%
//! - zero sensitive executions
//! - recall of exec cases >= 85% on the holdout half
//! - under-clarification <= 1% (a case that should ask / confirm / refuse but executed)
//!
//! Run `cargo test --lib intent::corpus -- --nocapture` for the per-capability coverage table.

use super::*;
use chrono::{NaiveDateTime, TimeZone};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

const CORPUS: &str = include_str!("corpus.jsonl");

fn fixed_now() -> DateTime<Local> {
    let ndt = NaiveDateTime::parse_from_str("2026-10-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
    Local.from_local_datetime(&ndt).earliest().unwrap()
}

fn known() -> Known {
    let n = |id: &str, name: &str| KnownName { id: id.into(), name: name.into() };
    Known {
        mailboxes: vec![n("mb1", "Yuval Cohen"), n("mb2", "מכירות")],
        calendars: vec![n("cal1", "איציק לוי"), n("cal2", "דנה כהן")],
        people: vec!["יובל כהן".into()],
    }
}

struct Case {
    id: String,
    text: String,
    ctx: Option<String>,
    ctx_age_s: i64,
    expect: Value,
    split: String,
    tags: Vec<String>,
}

fn load() -> Vec<Case> {
    CORPUS
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap_or_else(|e| panic!("bad corpus line {l}: {e}"));
            Case {
                id: v["id"].as_str().expect("id").to_string(),
                text: v["text"].as_str().expect("text").to_string(),
                ctx: v["ctx"].as_str().map(String::from),
                ctx_age_s: v["ctx_age"].as_i64().unwrap_or(10),
                expect: v["expect"].clone(),
                split: v["split"].as_str().expect("split").to_string(),
                tags: v["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
            }
        })
        .collect()
}

fn by_id(cases: &[Case]) -> HashMap<&str, &Case> {
    cases.iter().map(|c| (c.id.as_str(), c)).collect()
}

/// Run a case, first running (and remembering) the case it continues.
fn run(case: &Case, all: &HashMap<&str, &Case>) -> Interpretation {
    run_text(&case.text, case, all)
}

fn run_text(text: &str, case: &Case, all: &HashMap<&str, &Case>) -> Interpretation {
    let mut ctx = Ctx::default();
    let now = fixed_now();
    if let Some(prior) = case.ctx.as_deref().and_then(|id| all.get(id)) {
        let first = run(prior, all);
        ctx.remember(&first, now.timestamp_millis());
    }
    interpret(text, &ctx, now + Duration::seconds(case.ctx_age_s), &known())
}

fn ask_name(a: AskKind) -> &'static str {
    match a {
        AskKind::Mailbox => "mailbox",
        AskKind::Date => "date",
        AskKind::Time => "time",
        AskKind::Person => "person",
        AskKind::Content => "content",
        AskKind::Intent => "intent",
    }
}

fn kind(d: &Decision) -> &'static str {
    match d {
        Decision::Execute { .. } => "exec",
        Decision::MultiSource { .. } => "multi",
        Decision::Clarify { .. } => "clarify",
        Decision::Confirm { .. } => "confirm",
        Decision::NoMatch => "nomatch",
    }
}

fn fmt(t: &DateTime<Local>) -> String {
    t.format("%Y-%m-%dT%H:%M").to_string()
}

/// `Err` says what differs. Decision first, then the expected slots as a subset.
fn check(case: &Case, i: &Interpretation) -> (bool, Vec<String>) {
    let e = &case.expect;
    let mut bad = Vec::new();
    let want = e["decision"].as_str().unwrap_or("");
    if kind(&i.decision) != want {
        bad.push(format!("decision {} (wanted {want})", kind(&i.decision)));
        return (false, bad);
    }
    let decision_ok_so_far = bad.len();
    match &i.decision {
        Decision::Execute { cap } | Decision::Confirm { cap } => {
            if let Some(c) = e["cap"].as_str() {
                if cap.as_str() != c {
                    bad.push(format!("cap {} (wanted {c})", cap.as_str()));
                }
            }
        }
        Decision::MultiSource { caps } => {
            if let Some(w) = e["caps"].as_array() {
                let got: Vec<&str> = caps.iter().map(|c| c.as_str()).collect();
                let wanted: Vec<&str> = w.iter().filter_map(|x| x.as_str()).collect();
                if got != wanted {
                    bad.push(format!("caps {got:?} (wanted {wanted:?})"));
                }
            }
        }
        Decision::Clarify { ask, cap } => {
            if let Some(a) = e["ask"].as_str() {
                if ask_name(*ask) != a {
                    bad.push(format!("ask {} (wanted {a})", ask_name(*ask)));
                }
            }
            if let Some(c) = e["cap"].as_str() {
                if cap.map(|c| c.as_str()) != Some(c) {
                    bad.push(format!("clarify cap {:?} (wanted {c})", cap.map(|c| c.as_str())));
                }
            }
        }
        Decision::NoMatch => {}
    }
    let s = &i.slots;
    if let Some(slots) = e["slots"].as_object() {
        for (k, v) in slots {
            let ok = match k.as_str() {
                "date" => s.time.as_ref().map_or(false, |t| fmt(&t.from)[..10] == *v.as_str().unwrap_or("") && t.grain == Grain::Day),
                "from" => s.time.as_ref().map_or(false, |t| fmt(&t.from).starts_with(v.as_str().unwrap_or("?"))),
                "to" => s.time.as_ref().map_or(false, |t| fmt(&t.to).starts_with(v.as_str().unwrap_or("?"))),
                "person" => s.person.as_deref() == v.as_str(),
                "sender" => s.sender.as_deref() == v.as_str(),
                "mailbox" => s.mailbox.as_deref() == v.as_str(),
                "app" => s.app.as_deref() == v.as_str(),
                "ext" => s.file_ext.as_deref() == v.as_str(),
                "expr" => s.expr.as_deref() == v.as_str(),
                "value" => s.expr.as_deref().and_then(|x| evaluate_expr(x).ok()).map_or(false, |r| (r - v.as_f64().unwrap_or(f64::NAN)).abs() < 1e-9),
                "latest" => s.latest == v.as_bool().unwrap_or(true),
                "unread" => s.unread == v.as_bool().unwrap_or(true),
                "all_mailboxes" => s.all_mailboxes == v.as_bool().unwrap_or(true),
                "follow_up" => i.follow_up == v.as_bool().unwrap_or(true),
                "limit" => s.limit.map(u64::from) == v.as_u64(),
                // every expected group is present, and there are no extra (junk) groups
                "terms" => v.as_array().map_or(false, |groups| {
                    s.terms.len() <= groups.len()
                        && groups.iter().all(|g| {
                        let want: Vec<&str> = g.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
                        s.terms.iter().any(|have| want.iter().all(|w| have.iter().any(|h| h == w)))
                    })
                }),
                other => panic!("unknown expected slot {other} in {}", case.id),
            };
            if !ok {
                bad.push(format!("slot {k}: wanted {v}, got {}", describe(s, i.follow_up)));
            }
        }
    }
    (bad.len() == decision_ok_so_far, bad)
}

fn describe(s: &Slots, follow_up: bool) -> String {
    format!(
        "time={:?} person={:?} sender={:?} terms={:?} mailbox={:?} all={} latest={} limit={:?} unread={} ext={:?} app={:?} expr={:?} follow_up={}",
        s.time.as_ref().map(|t| (fmt(&t.from), fmt(&t.to), t.grain)),
        s.person,
        s.sender,
        s.terms,
        s.mailbox,
        s.all_mailboxes,
        s.latest,
        s.limit,
        s.unread,
        s.file_ext,
        s.app,
        s.expr,
        follow_up
    )
}

fn cap_of(d: &Decision) -> String {
    match d {
        Decision::Execute { cap } | Decision::Confirm { cap } => cap.as_str().to_string(),
        Decision::Clarify { cap: Some(c), ask } => format!("clarify({}) {}", ask_name(*ask), c.as_str()),
        Decision::Clarify { cap: None, ask } => format!("clarify({})", ask_name(*ask)),
        Decision::MultiSource { .. } => "multi-source".into(),
        Decision::NoMatch => "no-match".into(),
    }
}

#[test]
fn corpus_is_well_formed() {
    let cases = load();
    assert!(cases.len() >= 120, "only {} cases", cases.len());
    let all = by_id(&cases);
    assert_eq!(all.len(), cases.len(), "duplicate ids");
    let dev = cases.iter().filter(|c| c.split == "dev").count();
    let hold = cases.iter().filter(|c| c.split == "holdout").count();
    assert_eq!(dev + hold, cases.len(), "split must be dev or holdout");
    assert!(dev >= cases.len() / 3 && hold >= cases.len() / 3, "dev {dev} / holdout {hold}");
    for c in &cases {
        if let Some(p) = &c.ctx {
            assert!(all.contains_key(p.as_str()), "{} points at missing ctx {p}", c.id);
        }
    }
    for tag in ["he", "en", "mixed", "typo", "follow-up", "refinement", "unsafe"] {
        assert!(cases.iter().any(|c| c.tags.iter().any(|t| t == tag)), "no case tagged {tag}");
    }
}

#[test]
fn corpus_gates() {
    let cases = load();
    let all = by_id(&cases);
    let mut executed = 0usize;
    let mut executed_ok = 0usize;
    let mut sensitive_exec = 0usize;
    let mut under_clarified = 0usize;
    let mut non_exec = 0usize;
    let mut recall: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut table: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut by_lang: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();

    for c in &cases {
        let i = run(c, &all);
        let (ok, why) = check(c, &i);
        let want = c.expect["decision"].as_str().unwrap_or("");
        if let Decision::Execute { cap } = &i.decision {
            executed += 1;
            if sensitivity(*cap) != Sensitivity::Read {
                sensitive_exec += 1;
            }
            if ok && want == "exec" {
                executed_ok += 1;
            }
            if want != "exec" {
                under_clarified += 1;
            }
        }
        if want == "exec" {
            let r = recall.entry(if c.split == "dev" { "dev" } else { "holdout" }).or_default();
            r.0 += 1;
            if ok {
                r.1 += 1;
            }
        } else {
            non_exec += 1;
        }
        let key = match c.expect["cap"].as_str() {
            Some(cap) => cap.to_string(),
            None if want == "multi" => "multi-source".to_string(),
            None => format!("({want})"),
        };
        let row = table.entry(key).or_default();
        row.0 += 1;
        if kind(&i.decision) == want {
            row.1 += 1;
        }
        if ok {
            row.2 += 1;
        }
        for t in &c.tags {
            if matches!(t.as_str(), "he" | "en" | "mixed" | "typo" | "follow-up" | "refinement" | "unsafe" | "niqqud" | "bidi") {
                let r = by_lang.entry(t.clone()).or_default();
                r.0 += 1;
                if ok {
                    r.1 += 1;
                }
            }
        }
        if !ok {
            failures.push(format!("{} [{}] {:?}: {} -> {} | {}", c.id, c.split, c.text, cap_of(&i.decision), why.join("; "), describe(&i.slots, i.follow_up)));
        }
    }

    let pct = |a: usize, b: usize| if b == 0 { 100.0 } else { 100.0 * a as f64 / b as f64 };
    let precision = pct(executed_ok, executed);
    let (hd, hh) = (recall.get("dev").copied().unwrap_or_default(), recall.get("holdout").copied().unwrap_or_default());
    let under = pct(under_clarified, non_exec);

    println!("\ncases: {} (dev {}, holdout {})", cases.len(), cases.iter().filter(|c| c.split == "dev").count(), cases.iter().filter(|c| c.split == "holdout").count());
    println!("{:<44} {:>4} {:>8} {:>8}", "capability (expected)", "n", "decision", "correct");
    for (k, (n, d, ok)) in &table {
        println!("{k:<44} {n:>4} {d:>8} {ok:>8}");
    }
    println!("\n{:<14} {:>4} {:>8}", "category", "n", "correct");
    for (k, (n, ok)) in &by_lang {
        println!("{k:<14} {n:>4} {ok:>8}");
    }
    println!(
        "\nexecute precision {precision:.1}% ({executed_ok}/{executed}); recall dev {:.1}% ({}/{}), holdout {:.1}% ({}/{}); under-clarification {under:.1}% ({under_clarified}/{non_exec}); sensitive executes {sensitive_exec}",
        pct(hd.1, hd.0),
        hd.1,
        hd.0,
        pct(hh.1, hh.0),
        hh.1,
        hh.0
    );
    for f in &failures {
        println!("MISS {f}");
    }

    assert_eq!(sensitive_exec, 0, "a sensitive capability was executed");
    assert!(precision >= 98.0, "execute precision {precision:.1}% < 98%");
    assert!(pct(hh.1, hh.0) >= 85.0, "holdout recall {:.1}% < 85%", pct(hh.1, hh.0));
    assert!(pct(hd.1, hd.0) >= 85.0, "dev recall {:.1}% < 85%", pct(hd.1, hd.0));
    assert!(under <= 1.0, "under-clarification {under:.1}% > 1%");
}

const FRESH: &str = include_str!("fresh.jsonl");

/// 40 phrasings written before the review fixes, from outside the findings. Not part of the
/// gates (the corpus is tuned on); this checks the engine generalises. Every one must pass.
#[test]
fn fresh_phrasings_generalise() {
    let all = HashMap::new();
    let mut pass = 0;
    let mut total = 0;
    for l in FRESH.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(l).unwrap_or_else(|e| panic!("bad fresh line {l}: {e}"));
        let case = Case {
            id: v["id"].as_str().expect("id").to_string(),
            text: v["text"].as_str().expect("text").to_string(),
            ctx: None,
            ctx_age_s: 10,
            expect: v["expect"].clone(),
            split: "dev".into(),
            tags: Vec::new(),
        };
        let i = run(&case, &all);
        let (ok, why) = check(&case, &i);
        total += 1;
        if ok {
            pass += 1;
        } else {
            println!("FRESH MISS {} {:?}: {} -> {} | {}", case.id, case.text, case.expect["decision"], why.join("; "), describe(&i.slots, i.follow_up));
        }
    }
    println!("fresh phrasings: {pass}/{total}");
    assert_eq!(total, 40);
    assert_eq!(pass, total, "fresh phrasings that fail");
}

/// 20,000 interprets of mixed corpus text: a bound loose enough for a debug build on a busy
/// machine, plus the measured percentiles.
#[test]
fn load_20000_interprets() {
    let cases = load();
    let all = by_id(&cases);
    let _ = interpret("מה יש לי היום", &Ctx::default(), fixed_now(), &known());
    let started = Instant::now();
    let mut samples = Vec::with_capacity(20_000);
    for n in 0..20_000usize {
        let c = &cases[(n * 7) % cases.len()];
        let t = Instant::now();
        let _ = run_text(&c.text, c, &all);
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let total = started.elapsed().as_secs_f64();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (p50, p95, p99) = (samples[10_000], samples[19_000], samples[19_800]);
    println!("interpret x20000: total {total:.2} s, p50 {p50:.3} ms, p95 {p95:.3} ms, p99 {p99:.3} ms");
    assert!(total < 20.0, "20,000 interprets took {total:.1} s");
}

#[test]
fn latency_p95_is_under_5ms() {
    let cases = load();
    let all = by_id(&cases);
    // warm the lexicon (built once per process) so the first call is not measured
    let _ = interpret("מה יש לי היום", &Ctx::default(), fixed_now(), &known());
    let mut samples = Vec::with_capacity(1000);
    for n in 0..1000 {
        let c = &cases[n % cases.len()];
        let t = Instant::now();
        let _ = run_text(&c.text, c, &all);
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (p50, p95, max) = (samples[499], samples[949], samples[999]);
    println!("interpret x1000 (incl. context set-up): p50 {p50:.3} ms, p95 {p95:.3} ms, max {max:.3} ms");
    assert!(p95 < 5.0, "p95 {p95:.3} ms");
}

#[test]
fn mutated_exec_cases_never_execute_something_sensitive() {
    let cases = load();
    let all = by_id(&cases);
    let mut tried = 0;
    let mut confirms = 0;
    for c in cases.iter().filter(|c| c.expect["decision"] == "exec") {
        let cs: Vec<char> = c.text.chars().collect();
        let n = cs.len();
        for j in 0..12 {
            let k = (j * n) / 12;
            // delete the letter at k
            let mut del = cs.clone();
            if k < del.len() {
                del.remove(k);
            }
            // swap the letters at k and k+1
            let mut swp = cs.clone();
            if k + 1 < swp.len() {
                swp.swap(k, k + 1);
            }
            for m in [del, swp] {
                let text: String = m.into_iter().collect();
                let i = run_text(&text, c, &all);
                tried += 1;
                if let Decision::Execute { cap } = &i.decision {
                    assert_eq!(sensitivity(*cap), Sensitivity::Read, "{:?} (from {}) executed {}", text, c.id, cap.as_str());
                }
                if matches!(i.decision, Decision::Confirm { .. }) {
                    confirms += 1;
                }
            }
        }
    }
    println!("mutations tried: {tried}, of which {confirms} became a Confirm (never an Execute)");
    assert!(tried > 1000);
}

#[test]
fn execute_is_always_read_only_on_random_text() {
    // a cheap deterministic fuzz: words from the lexicon in random order
    let words = [
        "פתח", "הפעל", "מחק", "אקסל", "מייל", "קובץ", "פתק", "מחר", "היום", "יובל", "מ", "על", "תקציב", "open", "launch", "file", "mail", "note", "app", "delete",
        "run", "אותו", "הראשון", "האחרון", "15.10", "3", "כמה", "12+3", "ביומן", "פנוי",
    ];
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..3000 {
        let n = 1 + (next() % 6) as usize;
        let text: Vec<&str> = (0..n).map(|_| words[(next() % words.len() as u64) as usize]).collect();
        let text = text.join(" ");
        let i = interpret(&text, &Ctx::default(), fixed_now(), &known());
        if let Decision::Execute { cap } = i.decision {
            assert_eq!(sensitivity(cap), Sensitivity::Read, "{text}");
        }
        if let Decision::MultiSource { caps } = i.decision {
            assert!(caps.iter().all(|c| sensitivity(*c) == Sensitivity::Read), "{text}");
        }
    }
}
