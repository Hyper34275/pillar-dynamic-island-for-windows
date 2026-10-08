//! Mailbox plan and typed-reply matching (pure).

use super::prefs::Prefs;
use super::wire::{Choice, ChoiceKind};
use crate::outlook_mail::{MailboxAccess, MailboxAvailability, MailboxInfo, MailboxKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailPlan {
    /// Search exactly these mailbox ids.
    Search(Vec<String>),
    /// Several mailboxes and nothing decides: ask the user.
    Ask,
    /// No mailbox can be searched at all.
    NoneSearchable,
    /// The mailbox the user named exists but cannot be searched now.
    NamedUnavailable(String),
    /// "בתיבה המשותפת", several shared mailboxes can be searched and nothing decides: ask which of
    /// these (their ids).
    AskShared(Vec<String>),
    /// "בתיבה המשותפת", but the profile has no shared mailbox.
    NoShared,
    /// "בתיבה המשותפת": there are shared mailboxes, none can be searched now.
    SharedUnavailable,
}

pub struct PlanInput<'a> {
    pub mailboxes: &'a [MailboxInfo],
    /// `Slots::mailbox`: a mailbox the user named (id).
    pub named: Option<&'a str>,
    /// `Slots::all_mailboxes`.
    pub all: bool,
    /// A sender was named: the mail could be in any mailbox.
    pub has_sender: bool,
    /// The plan of the previous mail search, for a follow-up / refinement.
    pub inherited: Option<&'a [String]>,
    pub prefs: Option<&'a Prefs>,
    /// False inside a multi-source answer: never stop to ask there.
    pub allow_ask: bool,
    /// "בתיבה המשותפת" (`Signals::shared_mailbox`): only the shared mailboxes are in play.
    pub shared_only: bool,
}

pub fn searchable_ids(mailboxes: &[MailboxInfo]) -> Vec<String> {
    mailboxes.iter().filter(|m| m.searchable()).map(|m| m.id.clone()).collect()
}

/// A mailbox that is somebody else's or a team's: a delegate or auto-mapped shared mailbox, or one
/// added by hand under "open these additional mailboxes". Not the user's own (`Primary`), an
/// archive or a data file.
pub fn is_shared(m: &MailboxInfo) -> bool {
    matches!(m.kind, MailboxKind::Shared | MailboxKind::Additional)
}

/// The shared mailboxes, whether they can be searched or not.
pub fn shared_boxes(mailboxes: &[MailboxInfo]) -> Vec<MailboxInfo> {
    mailboxes.iter().filter(|m| is_shared(m)).cloned().collect()
}

/// The shared mailboxes that can be searched now.
pub fn shared_searchable_ids(mailboxes: &[MailboxInfo]) -> Vec<String> {
    mailboxes.iter().filter(|m| is_shared(m) && m.searchable()).map(|m| m.id.clone()).collect()
}

/// The plan for "בתיבה המשותפת": one shared mailbox is used, several are asked about (after what a
/// sender, "all", an earlier plan or a saved choice already decide), none is said so.
fn plan_shared(input: &PlanInput, searchable: &[String]) -> MailPlan {
    let shared = shared_boxes(input.mailboxes);
    if shared.is_empty() {
        return MailPlan::NoShared;
    }
    let ok = shared_searchable_ids(input.mailboxes);
    if ok.is_empty() {
        return MailPlan::SharedUnavailable;
    }
    if ok.len() == 1 || input.all || input.has_sender {
        return MailPlan::Search(ok);
    }
    if let Some(inherited) = input.inherited {
        let still: Vec<String> = ok.iter().filter(|id| inherited.contains(id)).cloned().collect();
        if !still.is_empty() {
            return MailPlan::Search(still);
        }
    }
    // a saved choice counts only for the shared mailboxes it covers (and only while no mailbox is new)
    if let Some(ids) = input.prefs.and_then(|p| p.applies(searchable)) {
        let own: Vec<String> = ids.into_iter().filter(|id| ok.contains(id)).collect();
        if !own.is_empty() {
            return MailPlan::Search(own);
        }
    }
    if input.allow_ask {
        MailPlan::AskShared(ok)
    } else {
        MailPlan::Search(ok)
    }
}

pub fn plan(input: &PlanInput) -> MailPlan {
    let searchable = searchable_ids(input.mailboxes);
    if let Some(named) = input.named {
        if searchable.iter().any(|id| id == named) {
            return MailPlan::Search(vec![named.to_string()]);
        }
        if input.mailboxes.iter().any(|m| m.id == named) {
            return MailPlan::NamedUnavailable(named.to_string());
        }
    }
    if input.shared_only {
        return plan_shared(input, &searchable);
    }
    if searchable.is_empty() {
        return MailPlan::NoneSearchable;
    }
    if input.all || input.has_sender {
        return MailPlan::Search(searchable);
    }
    if let Some(inherited) = input.inherited {
        let still: Vec<String> = searchable.iter().filter(|id| inherited.contains(id)).cloned().collect();
        if !still.is_empty() {
            return MailPlan::Search(still);
        }
    }
    if searchable.len() == 1 {
        return MailPlan::Search(searchable);
    }
    if let Some(ids) = input.prefs.and_then(|p| p.applies(&searchable)) {
        return MailPlan::Search(ids);
    }
    if input.allow_ask {
        MailPlan::Ask
    } else {
        MailPlan::Search(searchable)
    }
}

/// The error code that explains why this mailbox cannot be searched.
pub fn unavailable_code(m: &MailboxInfo) -> &'static str {
    if m.access == MailboxAccess::Denied {
        "MAIL-101"
    } else {
        match m.availability {
            MailboxAvailability::Gone => "MAIL-102",
            MailboxAvailability::Offline => "MAIL-103",
            MailboxAvailability::Timeout => "MAIL-105",
            MailboxAvailability::Ok => "MAIL-109",
        }
    }
}

/// The code when nothing can be searched: permission first, then offline, then gone.
pub fn none_searchable_code(mailboxes: &[MailboxInfo]) -> &'static str {
    if mailboxes.is_empty() {
        return "MAIL-102";
    }
    for want in ["MAIL-101", "MAIL-103", "MAIL-105"] {
        if let Some(m) = mailboxes.iter().find(|m| unavailable_code(m) == want) {
            return unavailable_code(m);
        }
    }
    "MAIL-102"
}

// ----- typed replies -------------------------------------------------------------------------

fn norm(text: &str) -> String {
    let folded = crate::intent::fold(text);
    let cleaned: String = folded.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

const ALL_PHRASES: [&str; 17] = [
    "אני לא יודע",
    "אני לא יודעת",
    "לא יודע",
    "לא יודעת",
    "כולן",
    "כולם",
    "בכולן",
    "בכל התיבות",
    "כל התיבות",
    "חפש בכל התיבות",
    "הכל",
    "all",
    "any",
    "dont know",
    "i dont know",
    "every mailbox",
    "all mailboxes",
];

fn label_base(label: &str) -> &str {
    label.split(" (").next().unwrap_or(label)
}

/// Which offered choice a typed reply selects, if any. Order: the "I don't know / all" phrases,
/// a number ("2"), the full label, then a unique partial match of a short reply.
pub fn match_reply(text: &str, choices: &[Choice]) -> Option<usize> {
    let t = norm(text);
    if t.is_empty() || choices.is_empty() {
        return None;
    }
    // `fold` may map final letters; compare the phrases in the same form
    if ALL_PHRASES.iter().any(|p| norm(p) == t) {
        return choices.iter().position(|c| c.kind == ChoiceKind::AllMailboxes);
    }
    if let Ok(n) = t.parse::<usize>() {
        return (1..=choices.len()).contains(&n).then(|| n - 1);
    }
    let bases: Vec<String> = choices.iter().map(|c| norm(label_base(&c.label))).collect();
    if let Some(i) = choices.iter().enumerate().position(|(i, c)| norm(&c.label) == t || bases[i] == t) {
        return Some(i);
    }
    if t.chars().count() >= 2 && t.split(' ').count() <= 4 {
        let hits: Vec<usize> = choices
            .iter()
            .enumerate()
            .filter(|(i, c)| c.kind != ChoiceKind::AllMailboxes && bases[*i].contains(&t))
            .map(|(i, _)| i)
            .collect();
        if hits.len() == 1 {
            return Some(hits[0]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mb(id: &str, access: MailboxAccess, av: MailboxAvailability) -> MailboxInfo {
        MailboxInfo {
            id: id.into(),
            name: id.into(),
            kind: MailboxKind::Shared,
            access,
            availability: av,
            cached: None,
            instant_search: None,
        }
    }
    fn ok(id: &str) -> MailboxInfo {
        mb(id, MailboxAccess::Ok, MailboxAvailability::Ok)
    }
    fn input<'a>(boxes: &'a [MailboxInfo]) -> PlanInput<'a> {
        PlanInput { mailboxes: boxes, named: None, all: false, has_sender: false, inherited: None, prefs: None, allow_ask: true, shared_only: false }
    }
    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn kind(id: &str, kind: MailboxKind) -> MailboxInfo {
        MailboxInfo { kind, ..ok(id) }
    }
    fn shared_input<'a>(boxes: &'a [MailboxInfo]) -> PlanInput<'a> {
        PlanInput { shared_only: true, ..input(boxes) }
    }

    #[test]
    fn shared_means_a_delegate_or_added_mailbox_only() {
        assert!(is_shared(&kind("a", MailboxKind::Shared)));
        assert!(is_shared(&kind("a", MailboxKind::Additional)));
        for k in [MailboxKind::Primary, MailboxKind::Archive, MailboxKind::DataFile, MailboxKind::Other] {
            assert!(!is_shared(&kind("a", k)), "{k:?}");
        }
        let boxes = [kind("me", MailboxKind::Primary), kind("s", MailboxKind::Shared), mb("d", MailboxAccess::Denied, MailboxAvailability::Ok)];
        assert_eq!(shared_searchable_ids(&boxes), ids(&["s"]));
        assert_eq!(shared_boxes(&boxes).len(), 2);
    }

    #[test]
    fn the_shared_mailbox_plan() {
        let me = kind("me", MailboxKind::Primary);
        // none: said so, the primary mailbox is not used instead
        assert_eq!(plan(&shared_input(&[me.clone()])), MailPlan::NoShared);
        assert_eq!(plan(&shared_input(&[])), MailPlan::NoShared);
        // one: used without asking
        let boxes = [me.clone(), kind("s1", MailboxKind::Shared)];
        assert_eq!(plan(&shared_input(&boxes)), MailPlan::Search(ids(&["s1"])));
        // several: ask among them, or take them all where asking is not allowed
        let boxes = [me.clone(), kind("s1", MailboxKind::Shared), kind("s2", MailboxKind::Additional), kind("s3", MailboxKind::Shared)];
        assert_eq!(plan(&shared_input(&boxes)), MailPlan::AskShared(ids(&["s1", "s2", "s3"])));
        let mut i = shared_input(&boxes);
        i.allow_ask = false;
        assert_eq!(plan(&i), MailPlan::Search(ids(&["s1", "s2", "s3"])));
        // a sender or "all" decides without asking
        let mut i = shared_input(&boxes);
        i.has_sender = true;
        assert_eq!(plan(&i), MailPlan::Search(ids(&["s1", "s2", "s3"])));
        let mut i = shared_input(&boxes);
        i.all = true;
        assert_eq!(plan(&i), MailPlan::Search(ids(&["s1", "s2", "s3"])));
        // a named mailbox is still the one named
        let mut i = shared_input(&boxes);
        i.named = Some("me");
        assert_eq!(plan(&i), MailPlan::Search(ids(&["me"])));
    }

    #[test]
    fn the_shared_plan_uses_what_is_inherited_or_saved_only_inside_the_shared_mailboxes() {
        let boxes = [kind("me", MailboxKind::Primary), kind("s1", MailboxKind::Shared), kind("s2", MailboxKind::Shared)];
        let inherited = ids(&["s2", "me"]);
        let mut i = shared_input(&boxes);
        i.inherited = Some(&inherited);
        assert_eq!(plan(&i), MailPlan::Search(ids(&["s2"])));
        let primary_only = ids(&["me"]);
        i.inherited = Some(&primary_only);
        assert_eq!(plan(&i), MailPlan::AskShared(ids(&["s1", "s2"])));
        // a saved choice
        let prefs = Prefs::new(ids(&["s1", "me"]), ids(&["me", "s1", "s2"]));
        let mut i = shared_input(&boxes);
        i.prefs = Some(&prefs);
        assert_eq!(plan(&i), MailPlan::Search(ids(&["s1"])));
        // ... that no longer applies once a mailbox is new
        let stale = Prefs::new(ids(&["s1"]), ids(&["me", "s1"]));
        i.prefs = Some(&stale);
        assert_eq!(plan(&i), MailPlan::AskShared(ids(&["s1", "s2"])));
    }

    #[test]
    fn shared_mailboxes_that_cannot_be_searched() {
        let boxes = [kind("me", MailboxKind::Primary), mb("s1", MailboxAccess::Denied, MailboxAvailability::Ok)];
        assert_eq!(plan(&shared_input(&boxes)), MailPlan::SharedUnavailable);
        // one that works is enough
        let boxes = [kind("me", MailboxKind::Primary), mb("s1", MailboxAccess::Denied, MailboxAvailability::Ok), ok("s2")];
        assert_eq!(plan(&shared_input(&boxes)), MailPlan::Search(ids(&["s2"])));
    }

    #[test]
    fn named_mailbox_wins() {
        let boxes = [ok("a"), ok("b")];
        let mut i = input(&boxes);
        i.named = Some("b");
        assert_eq!(plan(&i), MailPlan::Search(ids(&["b"])));
    }

    #[test]
    fn named_mailbox_that_is_denied_is_reported() {
        let boxes = [ok("a"), mb("b", MailboxAccess::Denied, MailboxAvailability::Ok)];
        let mut i = input(&boxes);
        i.named = Some("b");
        assert_eq!(plan(&i), MailPlan::NamedUnavailable("b".into()));
        assert_eq!(unavailable_code(&boxes[1]), "MAIL-101");
    }

    #[test]
    fn all_and_sender_search_every_searchable_mailbox() {
        let boxes = [ok("a"), ok("b"), mb("c", MailboxAccess::Denied, MailboxAvailability::Ok)];
        let mut i = input(&boxes);
        i.all = true;
        assert_eq!(plan(&i), MailPlan::Search(ids(&["a", "b"])));
        let mut i = input(&boxes);
        i.has_sender = true;
        assert_eq!(plan(&i), MailPlan::Search(ids(&["a", "b"])));
    }

    #[test]
    fn a_single_searchable_mailbox_is_used_without_asking() {
        let boxes = [ok("a"), mb("c", MailboxAccess::Ok, MailboxAvailability::Offline)];
        assert_eq!(plan(&input(&boxes)), MailPlan::Search(ids(&["a"])));
    }

    #[test]
    fn several_mailboxes_ask() {
        let boxes = [ok("a"), ok("b"), ok("c")];
        assert_eq!(plan(&input(&boxes)), MailPlan::Ask);
        let mut i = input(&boxes);
        i.allow_ask = false;
        assert_eq!(plan(&i), MailPlan::Search(ids(&["a", "b", "c"])));
    }

    #[test]
    fn unchanged_preference_is_applied() {
        let boxes = [ok("a"), ok("b")];
        let prefs = Prefs::new(ids(&["b"]), ids(&["a", "b"]));
        let mut i = input(&boxes);
        i.prefs = Some(&prefs);
        assert_eq!(plan(&i), MailPlan::Search(ids(&["b"])));
    }

    #[test]
    fn preference_plus_new_mailbox_asks_again() {
        let boxes = [ok("a"), ok("b"), ok("new")];
        let prefs = Prefs::new(ids(&["b"]), ids(&["a", "b"]));
        let mut i = input(&boxes);
        i.prefs = Some(&prefs);
        assert_eq!(plan(&i), MailPlan::Ask);
    }

    #[test]
    fn follow_up_inherits_the_previous_plan() {
        let boxes = [ok("a"), ok("b"), ok("c")];
        let inherited = ids(&["b", "gone"]);
        let mut i = input(&boxes);
        i.inherited = Some(&inherited);
        assert_eq!(plan(&i), MailPlan::Search(ids(&["b"])));
        // an inherited plan whose mailboxes are all gone falls back to asking
        let inherited = ids(&["gone"]);
        i.inherited = Some(&inherited);
        assert_eq!(plan(&i), MailPlan::Ask);
    }

    #[test]
    fn nothing_searchable() {
        let boxes = [mb("a", MailboxAccess::Denied, MailboxAvailability::Ok), mb("b", MailboxAccess::Ok, MailboxAvailability::Offline)];
        assert_eq!(plan(&input(&boxes)), MailPlan::NoneSearchable);
        assert_eq!(none_searchable_code(&boxes), "MAIL-101");
        assert_eq!(none_searchable_code(&boxes[1..]), "MAIL-103");
        assert_eq!(none_searchable_code(&[]), "MAIL-102");
    }

    fn choices() -> Vec<Choice> {
        let c = |id: &str, label: &str, kind| Choice { id: id.into(), label: label.into(), kind, preferred: false };
        vec![
            c("mb:a", "תיבת מכירות (משותפת)", ChoiceKind::Mailbox),
            c("mb:b", "Yuval Cohen", ChoiceKind::Mailbox),
            c("all", "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן.", ChoiceKind::AllMailboxes),
        ]
    }

    #[test]
    fn typed_dont_know_selects_all() {
        let c = choices();
        for t in ["אני לא יודע", "לא יודע.", "כולן", "בכל התיבות", "All", "don't know", "I don't know"] {
            assert_eq!(match_reply(t, &c), Some(2), "{t}");
        }
    }

    #[test]
    fn typed_number_and_label() {
        let c = choices();
        assert_eq!(match_reply("1", &c), Some(0));
        assert_eq!(match_reply("2.", &c), Some(1));
        assert_eq!(match_reply("4", &c), None);
        assert_eq!(match_reply("תיבת מכירות", &c), Some(0));
        assert_eq!(match_reply("yuval cohen", &c), Some(1));
        assert_eq!(match_reply("מכירות", &c), Some(0));
    }

    #[test]
    fn a_new_question_is_not_a_reply() {
        let c = choices();
        assert_eq!(match_reply("מה יש לי היום ביומן", &c), None);
        assert_eq!(match_reply("", &c), None);
        assert_eq!(match_reply("אני לא יודע", &[]), None);
    }
}
