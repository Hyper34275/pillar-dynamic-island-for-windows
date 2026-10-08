//! The in-memory query store: the last 20 queries for 30 minutes. Item ids are opaque and resolve
//! to the real target (EntryID key, path key, AUMID key, note id, event start) only here. Nothing is
//! persisted and nothing in here is ever logged.

use super::wire::{AssistantCard, CardPhase, ResultGroup, SearchResults};
use crate::intent::Interpretation;
use crate::outlook_mail::{MailCursor, MailHit, MailQuery, MailboxInfo, MailboxOutcome};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};

pub const MAX_QUERIES: usize = 20;
pub const TTL_MS: i64 = 30 * 60_000;
/// A clarification can be answered by typing for this long.
pub const PENDING_TTL_MS: i64 = 10 * 60_000;
const MAX_DISMISSED: usize = 64;

/// What an item opens. Keys come from the sources and mean nothing outside them.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Mail(String),
    Event(DateTime<Utc>),
    Note(String),
    File(String),
    App(String),
    /// Information only.
    None,
}

/// A question waiting for the user's answer.
#[derive(Clone, Debug)]
pub enum Pending {
    /// Which mailbox: `offered` are the mailbox ids behind the choices.
    Mailbox { interp: Interpretation, offered: Vec<String> },
    /// Which calendar of several that match the person.
    Calendar { interp: Interpretation, offered: Vec<String> },
}

/// A mail search that can be continued.
#[derive(Clone, Debug)]
pub struct MailRun {
    pub query: MailQuery,
    pub cursor: Option<MailCursor>,
    /// Newest first, deduplicated by key.
    pub hits: Vec<MailHit>,
    pub boxes: Vec<MailboxInfo>,
    pub outcomes: Vec<MailboxOutcome>,
    pub partial: bool,
    /// The question asked for the newest mail ("האחרון"): the summary names the first hit.
    pub latest: bool,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub card: AssistantCard,
    pub groups: Vec<ResultGroup>,
    pub targets: HashMap<String, Target>,
    pub pending: Option<Pending>,
    pub mail: Option<MailRun>,
    pub touched_ms: i64,
}

impl Entry {
    pub fn id(&self) -> &str {
        &self.card.query_id
    }
}

#[derive(Default)]
pub struct Store {
    /// Oldest first.
    entries: VecDeque<Entry>,
    dismissed: VecDeque<String>,
}

impl Store {
    pub const fn new() -> Self {
        Store { entries: VecDeque::new(), dismissed: VecDeque::new() }
    }

    fn prune(&mut self, now_ms: i64) {
        self.entries.retain(|e| now_ms - e.touched_ms <= TTL_MS);
    }

    /// Add or replace (same query id) an entry; it becomes the newest.
    pub fn insert(&mut self, mut entry: Entry, now_ms: i64) {
        entry.touched_ms = now_ms;
        self.prune(now_ms);
        self.entries.retain(|e| e.id() != entry.id());
        self.entries.push_back(entry);
        while self.entries.len() > MAX_QUERIES {
            self.entries.pop_front();
        }
    }

    pub fn get(&mut self, id: &str, now_ms: i64) -> Option<&Entry> {
        self.prune(now_ms);
        self.entries.iter().find(|e| e.id() == id)
    }

    /// The newest unanswered question that is still fresh.
    pub fn latest_pending(&mut self, now_ms: i64) -> Option<&Entry> {
        self.prune(now_ms);
        self.entries
            .iter()
            .rev()
            .take(3)
            .find(|e| e.pending.is_some() && e.card.phase == CardPhase::Choices && now_ms - e.touched_ms <= PENDING_TTL_MS)
    }

    /// Oldest first, for the Center's chat.
    pub fn history(&mut self, now_ms: i64) -> Vec<SearchResults> {
        self.prune(now_ms);
        self.entries.iter().map(|e| SearchResults { card: e.card.clone(), groups: e.groups.clone() }).collect()
    }

    pub fn dismiss(&mut self, id: &str) {
        if !self.dismissed.iter().any(|d| d == id) {
            if self.dismissed.len() == MAX_DISMISSED {
                self.dismissed.pop_front();
            }
            self.dismissed.push_back(id.to_string());
        }
    }

    pub fn is_dismissed(&self, id: &str) -> bool {
        self.dismissed.iter().any(|d| d == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::Lang;

    pub fn card(id: &str, phase: CardPhase) -> AssistantCard {
        AssistantCard {
            query_id: id.into(),
            query: String::new(),
            phase,
            lang: Lang::He,
            title: String::new(),
            summary: String::new(),
            question: None,
            choices: Vec::new(),
            items: Vec::new(),
            total: 0,
            partial: false,
            can_extend: false,
            error_code: None,
            sources: Vec::new(),
            created_at: 0,
            follow_up: false,
        }
    }

    pub fn entry(id: &str, phase: CardPhase) -> Entry {
        Entry { card: card(id, phase), groups: Vec::new(), targets: HashMap::new(), pending: None, mail: None, touched_ms: 0 }
    }

    #[test]
    fn keeps_the_last_twenty() {
        let mut s = Store::new();
        for i in 0..25 {
            s.insert(entry(&format!("q{i}"), CardPhase::Answer), 1000 + i);
        }
        assert_eq!(s.len(), MAX_QUERIES);
        assert!(s.get("q0", 2000).is_none());
        assert!(s.get("q4", 2000).is_none());
        assert!(s.get("q5", 2000).is_some());
        assert_eq!(s.history(2000).first().unwrap().card.query_id, "q5");
        assert_eq!(s.history(2000).last().unwrap().card.query_id, "q24");
    }

    #[test]
    fn expires_after_thirty_minutes() {
        let mut s = Store::new();
        s.insert(entry("a", CardPhase::Answer), 0);
        assert!(s.get("a", TTL_MS).is_some());
        assert!(s.get("a", TTL_MS + 1).is_none());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn replacing_refreshes_and_moves_to_newest() {
        let mut s = Store::new();
        s.insert(entry("a", CardPhase::Choices), 0);
        s.insert(entry("b", CardPhase::Answer), 10);
        s.insert(entry("a", CardPhase::Answer), TTL_MS);
        assert_eq!(s.len(), 2);
        let h = s.history(TTL_MS);
        assert_eq!(h.last().unwrap().card.query_id, "a");
        assert_eq!(h.last().unwrap().card.phase, CardPhase::Answer);
        assert!(s.get("a", TTL_MS + TTL_MS - 1).is_some());
    }

    #[test]
    fn pending_needs_choices_and_freshness() {
        let mut s = Store::new();
        let mut e = entry("a", CardPhase::Choices);
        e.pending = Some(Pending::Mailbox { interp: dummy_interp(), offered: vec![] });
        s.insert(e, 0);
        assert!(s.latest_pending(1000).is_some());
        assert!(s.latest_pending(PENDING_TTL_MS + 1).is_none());
        // answered (no pending any more) -> not offered again
        s.insert(entry("a", CardPhase::Answer), 2000);
        assert!(s.latest_pending(3000).is_none());
    }

    #[test]
    fn dismissed_ids_are_remembered_and_bounded() {
        let mut s = Store::new();
        s.dismiss("x");
        s.dismiss("x");
        assert!(s.is_dismissed("x"));
        assert!(!s.is_dismissed("y"));
        for i in 0..80 {
            s.dismiss(&format!("d{i}"));
        }
        assert!(s.dismissed.len() <= MAX_DISMISSED);
        assert!(s.is_dismissed("d79"));
    }

    pub fn dummy_interp() -> Interpretation {
        Interpretation {
            decision: crate::intent::Decision::NoMatch,
            slots: Default::default(),
            confidence: 0.0,
            lang: Lang::He,
            follow_up: false,
            ranked: Vec::new(),
        }
    }
}
