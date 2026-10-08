//! The orchestrator core: conversation state, the query store and the steps of one question.
//! Synchronous and blocking (the caller runs it on a blocking thread); every outside effect goes
//! through [`Sources`], so the whole flow is tested with fakes.
//!
//! Privacy: the only things logged are the query id (random), the decision label, counts, the
//! duration and error codes (see [`log_line`]).

use super::answer;
use super::exec::{self, Group, Outcome, Run, Sources};
use super::policy;
use super::prefs::Prefs;
use super::store::{Entry, Store, Target};
use super::wire::{AssistantCard, AssistantItem, CardPhase, ItemKind, ResultGroup, SearchResults};
use crate::intent::{self, Ctx, Decision, Interpretation, Known, KnownName, Lang};
use chrono::{DateTime, Local};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// The island shows at most this many items; the Center shows all.
const CARD_ITEMS: usize = 3;
/// A follow-up inherits the previous mailbox plan for as long as the intent context lives.
const PLAN_TTL_MS: i64 = 120_000;

/// What the conversation remembers between questions (memory only).
#[derive(Default)]
pub struct Session {
    pub ctx: Ctx,
    pub last_plan: Option<(Vec<String>, i64)>,
}

pub enum Route {
    /// The text answers the pending question: continue that query.
    Choose { query_id: String, option_id: String },
    /// A new question.
    New,
}

pub struct Engine {
    session: Mutex<Session>,
    store: Mutex<Store>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A random-looking query id: 16 hex of sha256(counter, time, pid).
pub fn mint_query_id(counter: u64, unix_nanos: u128) -> String {
    let mut h = Sha256::new();
    h.update(counter.to_le_bytes());
    h.update(unix_nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    hex(&h.finalize()[..8])
}

fn item_id(query_id: &str, n: usize) -> String {
    let mut h = Sha256::new();
    h.update(query_id.as_bytes());
    h.update(n.to_le_bytes());
    hex(&h.finalize()[..6])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The structured log line of a finished query: ids, labels, counts and codes only.
pub fn log_line(card: &AssistantCard, decision: &str, elapsed_ms: u128) -> String {
    format!(
        "query {} {} phase={:?} total={} partial={} ms={}{}",
        card.query_id,
        decision,
        card.phase,
        card.total,
        card.partial,
        elapsed_ms,
        card.error_code.as_deref().map(|c| format!(" code={c}")).unwrap_or_default()
    )
}

fn decision_label(d: &Decision) -> String {
    match d {
        Decision::Execute { cap } => format!("execute:{}", cap.as_str()),
        Decision::MultiSource { caps } => format!("multi:{}", caps.len()),
        Decision::Clarify { ask, .. } => format!("clarify:{ask:?}").to_lowercase(),
        Decision::Confirm { cap } => format!("confirm:{}", cap.as_str()),
        Decision::NoMatch => "nomatch".into(),
    }
}

/// What the machine knows, from caches only. The people are the owners of the shared calendars
/// first, then the organizers seen lately, so a colleague whose calendar is open in Outlook is
/// recognised as a name.
fn known_from(src: &dyn Sources) -> Known {
    let calendars = src.calendars();
    let people = exec::known_people(&calendars, src.people());
    Known {
        mailboxes: src.cached_mailboxes().into_iter().map(|m| KnownName { id: m.id, name: m.name }).collect(),
        calendars: calendars.into_iter().map(|c| KnownName { id: c.id, name: c.name }).collect(),
        people,
    }
}

/// The first items of the answer: newest mail first, otherwise the groups' own order (taken in
/// turn for a multi-source answer).
fn top_items(groups: &[ResultGroup], interleave: bool) -> Vec<AssistantItem> {
    let flat: Vec<&AssistantItem> = groups.iter().flat_map(|g| g.items.iter()).collect();
    if !flat.is_empty() && flat.iter().all(|i| i.kind == ItemKind::Mail) {
        let mut mail = flat;
        mail.sort_by(|a, b| b.time.cmp(&a.time));
        return mail.into_iter().take(CARD_ITEMS).cloned().collect();
    }
    if interleave {
        let mut out = Vec::new();
        let mut round = 0;
        while out.len() < CARD_ITEMS && groups.iter().any(|g| g.items.len() > round) {
            for g in groups {
                if let Some(i) = g.items.get(round) {
                    if out.len() < CARD_ITEMS {
                        out.push(i.clone());
                    }
                }
            }
            round += 1;
        }
        return out;
    }
    flat.into_iter().take(CARD_ITEMS).cloned().collect()
}

fn source_of(kind: &str) -> &str {
    match kind {
        "availability" => "calendar",
        k => k,
    }
}

/// Mint ids and build the card and the stored entry from an outcome.
#[allow(clippy::too_many_arguments)]
fn build_entry(query_id: &str, query: &str, lang: Lang, follow_up: bool, created_at: i64, now_ms: i64, o: Outcome) -> Entry {
    let mut targets = HashMap::new();
    let mut n = 0usize;
    let mut groups = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    for g in o.groups {
        let Group { kind, title, mailbox, items, truncated, error_code } = g;
        let mut out_items = Vec::with_capacity(items.len());
        for (mut item, target) in items {
            item.id = item_id(query_id, n);
            n += 1;
            if target != Target::None {
                targets.insert(item.id.clone(), target);
            }
            out_items.push(item);
        }
        let s = source_of(kind).to_string();
        if !sources.contains(&s) {
            sources.push(s);
        }
        groups.push(ResultGroup { kind: kind.to_string(), title, mailbox, items: out_items, truncated, error_code });
    }
    let total = groups.iter().map(|g| g.items.len()).sum::<usize>() as u32;
    let items = if o.phase == CardPhase::Answer { top_items(&groups, o.interleave) } else { Vec::new() };
    let card = AssistantCard {
        query_id: query_id.to_string(),
        query: query.to_string(),
        phase: o.phase,
        lang,
        title: o.title,
        summary: o.summary,
        question: o.question,
        choices: o.choices,
        items,
        total,
        partial: o.partial,
        can_extend: o.can_extend,
        error_code: o.error_code,
        sources,
        created_at,
        follow_up,
    };
    Entry { card, groups, targets, pending: o.pending, mail: o.mail, touched_ms: now_ms }
}

impl Engine {
    pub fn new() -> Self {
        Engine { session: Mutex::new(Session::default()), store: Mutex::new(Store::new()) }
    }

    /// Does the text answer the question that is waiting? ("אני לא יודע", "2", a mailbox name.)
    pub fn route(&self, text: &str, now_ms: i64) -> Route {
        let mut store = lock(&self.store);
        if let Some(e) = store.latest_pending(now_ms) {
            if let Some(i) = policy::match_reply(text, &e.card.choices) {
                return Route::Choose { query_id: e.id().to_string(), option_id: e.card.choices[i].id.clone() };
            }
        }
        Route::New
    }

    /// The text and language of a stored query (for its processing card).
    pub fn query_of(&self, query_id: &str, now_ms: i64) -> Option<(String, Lang)> {
        lock(&self.store).get(query_id, now_ms).map(|e| (e.card.query.clone(), e.card.lang))
    }

    /// Understand and run a new question.
    pub fn submit(&self, src: &dyn Sources, text: &str, query_id: &str, now: DateTime<Local>) -> AssistantCard {
        let known = known_from(src);
        let interp = {
            let session = lock(&self.session);
            intent::interpret(text, &session.ctx, now, &known)
        };
        self.run_interpretation(src, text, query_id, now, &interp)
    }

    /// Run an interpretation (the part of `submit` after understanding).
    pub fn run_interpretation(&self, src: &dyn Sources, text: &str, query_id: &str, now: DateTime<Local>, interp: &Interpretation) -> AssistantCard {
        let started = Instant::now();
        let now_ms = now.timestamp_millis();
        let inherited = lock(&self.session).last_plan.clone().filter(|(_, at)| now_ms - at <= PLAN_TTL_MS).map(|(ids, _)| ids);
        let prefs = src.load_prefs();
        let run = Run { src, now, lang: interp.lang, prefs: prefs.as_ref(), inherited: inherited.as_deref(), allow_ask: true };
        let outcome = exec::execute(&run, interp);
        self.finish(query_id, text, interp.lang, interp.follow_up, Some(interp), outcome, now_ms, now_ms, started)
    }

    /// Continue a pending question with an option (clicked, or typed and matched by `route`).
    pub fn choose(&self, src: &dyn Sources, query_id: &str, option_id: &str, remember: bool, now: DateTime<Local>) -> Result<AssistantCard, String> {
        let started = Instant::now();
        let now_ms = now.timestamp_millis();
        let (pending, query, lang, created_at) = {
            let mut store = lock(&self.store);
            let e = store.get(query_id, now_ms).ok_or("APP-041: search expired")?;
            (e.pending.clone().ok_or("APP-041: search expired")?, e.card.query.clone(), e.card.lang, e.card.created_at)
        };
        let prefs = src.load_prefs();
        let run = Run { src, now, lang, prefs: prefs.as_ref(), inherited: None, allow_ask: true };
        let outcome = exec::resume(&run, &pending, option_id)?;
        if remember && outcome.phase != CardPhase::Choices {
            if let Some(chosen) = outcome.used_plan.clone().filter(|c| !c.is_empty()) {
                if let Err(e) = src.save_prefs(&Prefs::new(chosen, outcome.searchable.clone())) {
                    crate::dlog!("WARN", "assistant", "{}", answer::code_of(&e, "APP-043"));
                }
            }
        }
        let interp = match &pending {
            super::store::Pending::Mailbox { interp, .. } => interp.clone(),
            // what is remembered is the request on the calendar picked, so a follow-up does not ask again
            super::store::Pending::Calendar { interp, .. } if outcome.phase != CardPhase::Choices => exec::after_calendar_choice(&src.calendars(), interp, option_id),
            super::store::Pending::Calendar { interp, .. } => interp.clone(),
        };
        Ok(self.finish(query_id, &query, lang, false, Some(&interp), outcome, created_at, now_ms, started))
    }

    /// Continue a partial mail search for another 10 s and merge the hits.
    pub fn extend(&self, src: &dyn Sources, query_id: &str, now: DateTime<Local>) -> Result<AssistantCard, String> {
        let started = Instant::now();
        let now_ms = now.timestamp_millis();
        let (run, query, lang, follow_up, created_at) = {
            let mut store = lock(&self.store);
            let e = store.get(query_id, now_ms).ok_or("APP-041: search expired")?;
            (e.mail.clone().filter(|m| m.cursor.is_some()).ok_or("APP-041: search expired")?, e.card.query.clone(), e.card.lang, e.card.follow_up, e.card.created_at)
        };
        let next = exec::extend_mail(src, &run)?;
        let mut outcome = exec::mail_outcome(&next, lang);
        outcome.used_plan = Some(next.query.mailboxes.clone());
        outcome.mail = Some(next);
        Ok(self.finish(query_id, &query, lang, follow_up, None, outcome, created_at, now_ms, started))
    }

    /// Open one result after an explicit click. The id must belong to the query.
    pub fn open(&self, src: &dyn Sources, query_id: &str, item_id: &str, now_ms: i64) -> Result<(), String> {
        let target = {
            let mut store = lock(&self.store);
            let e = store.get(query_id, now_ms).ok_or("APP-041: search expired")?;
            e.targets.get(item_id).cloned().ok_or("APP-042: nothing to open")?
        };
        match target {
            Target::Mail(key) => src.open_mail(&key),
            Target::Event(start) => src.open_event(start),
            Target::Note(id) => src.open_note(&id),
            Target::File(key) => src.open_file(&key),
            Target::App(key) => src.launch_app(&key),
            Target::None => Err("APP-042: nothing to open".into()),
        }
    }

    pub fn results(&self, query_id: &str, now_ms: i64) -> Result<SearchResults, String> {
        let mut store = lock(&self.store);
        store
            .get(query_id, now_ms)
            .map(|e| SearchResults { card: e.card.clone(), groups: e.groups.clone() })
            .ok_or_else(|| "APP-041: search expired".to_string())
    }

    pub fn history(&self, now_ms: i64) -> Vec<SearchResults> {
        lock(&self.store).history(now_ms)
    }

    pub fn dismiss(&self, query_id: &str) {
        lock(&self.store).dismiss(query_id);
    }

    /// The user acts on a stored query again (a typed reply, a choice, extend): a card they closed
    /// earlier is shown again. Returns the text and language, like [`Engine::query_of`].
    pub fn resume(&self, query_id: &str, now_ms: i64) -> Option<(String, Lang)> {
        let mut store = lock(&self.store);
        let found = store.get(query_id, now_ms).map(|e| (e.card.query.clone(), e.card.lang));
        if found.is_some() {
            store.undismiss(query_id);
        }
        found
    }

    /// Forget the stored queries and the conversation (smart search was switched off).
    pub fn clear(&self) {
        lock(&self.store).clear();
        let mut session = lock(&self.session);
        session.ctx.clear();
        session.last_plan = None;
    }

    pub fn is_dismissed(&self, query_id: &str) -> bool {
        lock(&self.store).is_dismissed(query_id)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        query_id: &str,
        query: &str,
        lang: Lang,
        follow_up: bool,
        interp: Option<&Interpretation>,
        outcome: Outcome,
        created_at: i64,
        now_ms: i64,
        started: Instant,
    ) -> AssistantCard {
        {
            let mut session = lock(&self.session);
            if let Some(i) = interp.filter(|i| i.decision != Decision::NoMatch) {
                session.ctx.remember(i, now_ms);
            }
            if let Some(plan) = outcome.used_plan.clone() {
                session.last_plan = Some((plan, now_ms));
            }
        }
        let entry = build_entry(query_id, query, lang, follow_up, created_at, now_ms, outcome);
        let card = entry.card.clone();
        let label = interp.map_or_else(|| "extend".to_string(), |i| decision_label(&i.decision));
        crate::dlog!("INFO", "assistant", "{}", log_line(&card, &label, started.elapsed().as_millis()));
        lock(&self.store).insert(entry, now_ms);
        card
    }
}

/// The card shown while a question is being worked on.
pub fn processing_card(query_id: &str, query: &str, lang: Lang, created_at: i64) -> AssistantCard {
    AssistantCard {
        query_id: query_id.to_string(),
        query: query.to_string(),
        phase: CardPhase::Processing,
        lang,
        title: answer::processing_title(lang).to_string(),
        summary: String::new(),
        question: None,
        choices: Vec::new(),
        items: Vec::new(),
        total: 0,
        partial: false,
        can_extend: false,
        error_code: None,
        sources: Vec::new(),
        created_at,
        follow_up: false,
    }
}

/// An error card for a failure outside the flow (a task that did not run).
pub fn error_card(query_id: &str, query: &str, lang: Lang, code: &str, created_at: i64) -> AssistantCard {
    let mut c = processing_card(query_id, query, lang, created_at);
    c.phase = CardPhase::Error;
    c.title = answer::error_text(code, lang).to_string();
    c.error_code = Some(code.to_string());
    c
}

#[cfg(test)]
#[path = "flow_tests.rs"]
mod tests;
