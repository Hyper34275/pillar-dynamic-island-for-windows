//! The Hebrew evaluation set (`eval/`, labelling rules in `eval/GUIDE.md`):
//! - `must.jsonl`: the sentences the request names. Every one must pass.
//! - `dev.jsonl`, `dev2.jsonl`: may be used to tune the engine (two authors).
//! - `test.jsonl`: written by another author from the guide alone, never used for tuning. Its
//!   numbers are reported, not tuned to; label fixes are listed in `eval/LABEL_FIXES.md`.
//!
//! The world is fixed (Thursday 2026-10-08 09:00, three mailboxes, four calendars). Metrics:
//! - success: decision, capability and every expected slot right;
//! - intent accuracy: decision and capability (or ask) right;
//! - slot accuracy: of the cases with the intent right, those whose slots are all right, plus a
//!   per-slot rate;
//! - execute precision and recall;
//! - false clarification: an `exec` case that asked a question instead;
//! - under-clarification: executed where it should have asked, confirmed or refused;
//! - latency of `analyze`.
//!
//! `cargo test --lib intent::eval -- --nocapture` prints the report.

use super::*;
use chrono::{NaiveDateTime, TimeZone};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

const MUST: &str = include_str!("eval/must.jsonl");
const DEV: &str = include_str!("eval/dev.jsonl");
const DEV2: &str = include_str!("eval/dev2.jsonl");
const TEST: &str = include_str!("eval/test.jsonl");

fn fixed_now() -> DateTime<Local> {
    let ndt = NaiveDateTime::parse_from_str("2026-10-08T09:00", "%Y-%m-%dT%H:%M").unwrap();
    Local.from_local_datetime(&ndt).earliest().unwrap()
}

/// The world of `eval/GUIDE.md`.
fn known() -> Known {
    let n = |id: &str, name: &str| KnownName { id: id.into(), name: name.into() };
    Known {
        mailboxes: vec![n("mb1", "Yuval Cohen"), n("mb2", "מכירות"), n("mb3", "תמיכה טכנית")],
        calendars: vec![n("cal1", "איציק לוי"), n("cal2", "דנה כהן"), n("cal3", "חדר ישיבות"), n("cal4", "משה פרץ")],
        people: vec!["יובל כהן".into(), "דני רוזן".into(), "שרון לוי".into(), "מירי אברהם".into()],
    }
}

struct Case {
    id: String,
    text: String,
    ctx: Option<String>,
    expect: Value,
    tags: Vec<String>,
}

fn load(src: &str) -> Vec<Case> {
    src.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap_or_else(|e| panic!("bad eval line {l}: {e}"));
            Case {
                id: v["id"].as_str().expect("id").to_string(),
                text: v["text"].as_str().expect("text").to_string(),
                ctx: v["ctx"].as_str().map(String::from),
                expect: v["expect"].clone(),
                tags: v["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
            }
        })
        .collect()
}

/// Run a case, first running (and remembering) the case it continues.
fn run(case: &Case, all: &HashMap<&str, &Case>) -> (Interpretation, Analysis) {
    let mut ctx = Ctx::default();
    let now = fixed_now();
    if let Some(prior) = case.ctx.as_deref().and_then(|id| all.get(id)) {
        let first = run(prior, all).0;
        ctx.remember(&first, now.timestamp_millis());
    }
    analyze(&case.text, &ctx, now + Duration::seconds(10), &known())
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

fn fmt(t: &DateTime<Local>) -> String {
    t.format("%Y-%m-%dT%H:%M").to_string()
}

/// Decision and capability (or ask) as labelled.
fn intent_ok(e: &Value, i: &Interpretation) -> bool {
    if kind(&i.decision) != e["decision"].as_str().unwrap_or("") {
        return false;
    }
    match &i.decision {
        Decision::Execute { cap } | Decision::Confirm { cap } => e["cap"].as_str().map_or(true, |c| c == cap.as_str()),
        Decision::MultiSource { caps } => e["caps"].as_array().map_or(true, |w| {
            let got: Vec<&str> = caps.iter().map(|c| c.as_str()).collect();
            let wanted: Vec<&str> = w.iter().filter_map(|x| x.as_str()).collect();
            got == wanted
        }),
        Decision::Clarify { ask, cap } => {
            e["ask"].as_str().map_or(true, |a| a == ask_name(*ask)) && e["cap"].as_str().map_or(true, |c| cap.map(|x| x.as_str()) == Some(c))
        }
        Decision::NoMatch => true,
    }
}

/// One expected slot.
fn slot_ok(key: &str, v: &Value, i: &Interpretation, an: &Analysis) -> bool {
    let s = &i.slots;
    let b = |x: bool| x == v.as_bool().unwrap_or(true);
    match key {
        "date" => s.time.as_ref().map_or(false, |t| fmt(&t.from)[..10] == *v.as_str().unwrap_or("") && t.grain == Grain::Day),
        "from" => s.time.as_ref().map_or(false, |t| fmt(&t.from).starts_with(v.as_str().unwrap_or("?"))),
        "to" => s.time.as_ref().map_or(false, |t| fmt(&t.to).starts_with(v.as_str().unwrap_or("?"))),
        "person" => s.person.as_deref().map(fold) == v.as_str().map(fold),
        "sender" => s.sender.as_deref().map(fold) == v.as_str().map(fold),
        "mailbox" => s.mailbox.as_deref() == v.as_str(),
        "app" => s.app.as_deref().map(fold) == v.as_str().map(fold),
        "ext" => s.file_ext.as_deref() == v.as_str(),
        "value" => s.expr.as_deref().and_then(|x| evaluate_expr(x).ok()).map_or(false, |r| (r - v.as_f64().unwrap_or(f64::NAN)).abs() < 1e-6),
        "latest" => b(s.latest),
        "unread" => b(s.unread),
        "all_mailboxes" => b(s.all_mailboxes),
        "shared_mailbox" => b(an.shared_mailbox),
        "exact" => b(an.exact_terms),
        "follow_up" => b(i.follow_up),
        "limit" => s.limit.map(u64::from) == v.as_u64(),
        // every expected word is one of the alternatives of some group, and no extra groups
        "terms" => v.as_array().map_or(false, |groups| {
            s.terms.len() <= groups.len()
                && groups.iter().all(|g| {
                    let want: Vec<String> = g.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(fold).collect()).unwrap_or_default();
                    s.terms.iter().any(|have| want.iter().all(|w| have.iter().any(|h| fold(h) == *w)))
                })
        }),
        _ => true,
    }
}

fn describe(i: &Interpretation, an: &Analysis) -> String {
    let s = &i.slots;
    format!(
        "{:?} time={:?} person={:?} sender={:?} terms={:?} mailbox={:?} shared={} all={} latest={} limit={:?} unread={} ext={:?} app={:?} exact={} follow={} reasons={:?}",
        i.decision,
        s.time.as_ref().map(|t| (fmt(&t.from), fmt(&t.to), t.grain)),
        s.person,
        s.sender,
        s.terms,
        s.mailbox,
        an.shared_mailbox,
        s.all_mailboxes,
        s.latest,
        s.limit,
        s.unread,
        s.file_ext,
        s.app,
        an.exact_terms,
        i.follow_up,
        an.reasons
    )
}

#[derive(Default)]
struct Report {
    n: usize,
    success: usize,
    intent: usize,
    slot_cases: usize,
    slot_cases_ok: usize,
    slots: usize,
    slots_ok: usize,
    executed: usize,
    executed_ok: usize,
    want_exec: usize,
    recall_ok: usize,
    false_clarify: usize,
    non_exec: usize,
    under: usize,
    sensitive_exec: usize,
    tags: BTreeMap<String, (usize, usize)>,
    latency_ms: Vec<f64>,
    misses: Vec<String>,
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 {
        100.0
    } else {
        100.0 * a as f64 / b as f64
    }
}

impl Report {
    fn success_rate(&self) -> f64 {
        pct(self.success, self.n)
    }

    fn print(&mut self, name: &str) {
        self.latency_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let at = |q: f64| self.latency_ms.get(((self.latency_ms.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(0.0);
        println!("\n==== {name}: {} cases ====", self.n);
        println!("success (decision + capability + slots) {:.1}% ({}/{})", self.success_rate(), self.success, self.n);
        println!("intent accuracy {:.1}% ({}/{})", pct(self.intent, self.n), self.intent, self.n);
        println!(
            "slot accuracy: cases {:.1}% ({}/{}), slots {:.1}% ({}/{})",
            pct(self.slot_cases_ok, self.slot_cases),
            self.slot_cases_ok,
            self.slot_cases,
            pct(self.slots_ok, self.slots),
            self.slots_ok,
            self.slots
        );
        println!(
            "execute precision {:.1}% ({}/{}), recall {:.1}% ({}/{})",
            pct(self.executed_ok, self.executed),
            self.executed_ok,
            self.executed,
            pct(self.recall_ok, self.want_exec),
            self.recall_ok,
            self.want_exec
        );
        println!(
            "false clarification {:.1}% ({}/{}), under-clarification {:.1}% ({}/{}), sensitive executes {}",
            pct(self.false_clarify, self.want_exec),
            self.false_clarify,
            self.want_exec,
            pct(self.under, self.non_exec),
            self.under,
            self.non_exec,
            self.sensitive_exec
        );
        println!("latency per question: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms", at(0.5), at(0.95), at(1.0));
        println!("{:<18} {:>4} {:>7}", "tag", "n", "success");
        for (t, (n, ok)) in &self.tags {
            println!("{t:<18} {n:>4} {:>6.1}%", pct(*ok, *n));
        }
        for m in &self.misses {
            println!("MISS {m}");
        }
    }
}

fn evaluate(src: &str) -> Report {
    let cases = load(src);
    let all: HashMap<&str, &Case> = cases.iter().map(|c| (c.id.as_str(), c)).collect();
    assert_eq!(all.len(), cases.len(), "duplicate ids");
    // warm the lexicon so the first case is not measured with it
    let _ = analyze("מה יש לי היום", &Ctx::default(), fixed_now(), &known());
    let mut r = Report::default();
    for c in &cases {
        let t = Instant::now();
        let (i, an) = run(c, &all);
        r.latency_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let e = &c.expect;
        let want = e["decision"].as_str().unwrap_or("");
        let intent = intent_ok(e, &i);
        let mut bad = Vec::new();
        if let Some(slots) = e["slots"].as_object().filter(|s| !s.is_empty()) {
            if intent {
                r.slot_cases += 1;
            }
            for (k, v) in slots {
                let ok = slot_ok(k, v, &i, &an);
                if intent {
                    r.slots += 1;
                    if ok {
                        r.slots_ok += 1;
                    }
                }
                if !ok {
                    bad.push(format!("{k}={v}"));
                }
            }
            if intent && bad.is_empty() {
                r.slot_cases_ok += 1;
            }
        }
        let ok = intent && bad.is_empty();
        r.n += 1;
        r.intent += usize::from(intent);
        r.success += usize::from(ok);
        if let Decision::Execute { cap } = &i.decision {
            r.executed += 1;
            r.executed_ok += usize::from(ok && want == "exec");
            r.under += usize::from(want != "exec" && want != "multi");
            r.sensitive_exec += usize::from(sensitivity(*cap) != Sensitivity::Read);
        }
        if want == "exec" {
            r.want_exec += 1;
            r.recall_ok += usize::from(ok);
            r.false_clarify += usize::from(matches!(i.decision, Decision::Clarify { .. }));
        } else if want != "multi" {
            r.non_exec += 1;
        }
        for t in &c.tags {
            let row = r.tags.entry(t.clone()).or_default();
            row.0 += 1;
            row.1 += usize::from(ok);
        }
        if !ok {
            let what = if intent { format!("slots {}", bad.join(", ")) } else { format!("wanted {want} {}", e["cap"].as_str().or(e["ask"].as_str()).unwrap_or("")) };
            r.misses.push(format!("{} {:?}: {what} | got {}", c.id, c.text, describe(&i, &an)));
        }
    }
    r
}

#[test]
fn must_pass_sentences() {
    let mut r = evaluate(MUST);
    r.print("must");
    assert_eq!(r.success, r.n, "must-pass sentences that fail");
}

#[test]
fn dev_set() {
    let mut r = evaluate(DEV);
    r.print("dev");
    assert!(r.n >= 300);
    assert_eq!(r.sensitive_exec, 0);
}

#[test]
fn dev2_set() {
    let mut r = evaluate(DEV2);
    r.print("dev2");
    assert!(r.n >= 300);
    assert_eq!(r.sensitive_exec, 0);
}

/// Reported, never tuned to. The floor only catches a collapse; the measured number is what counts.
#[test]
fn held_out_test_set() {
    let mut r = evaluate(TEST);
    r.print("test");
    assert!(r.n >= 300);
    assert_eq!(r.sensitive_exec, 0);
}

/// Tuning aid: `INTENT_DEBUG="שאלה|שאלה" cargo test --lib intent::eval::debug -- --ignored --nocapture`
/// prints the annotation and the analysis of each question.
#[test]
#[ignore]
fn debug() {
    let texts = std::env::var("INTENT_DEBUG").unwrap_or_default();
    for t in texts.split('|').filter(|t| !t.trim().is_empty()) {
        let a = lexicon::annotate(&normalize::tokenize(t));
        let ann: Vec<String> = a.iter().map(|x| format!("{}={}{}", x.raw(), x.concept(), if x.hit.as_ref().map_or(false, |h| h.typo) { "~" } else { "" })).collect();
        let (i, an) = analyze(t, &Ctx::default(), fixed_now(), &known());
        println!("{t}\n  {}\n  {}", ann.join(" "), describe(&i, &an));
    }
}
