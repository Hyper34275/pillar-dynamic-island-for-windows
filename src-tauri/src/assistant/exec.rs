//! Executing a decided intent against the data sources (behind [`Sources`], so tests use fakes).
//! Everything here is blocking: the caller runs it on a blocking thread.
//!
//! Nothing here opens, launches or runs anything: `Confirm` decisions run the matching read-only
//! search and offer the candidates for a click (see `flow::Engine::open`).

use super::answer::{self, BusyKind, Noun};
use super::avail;
use super::policy::{self, MailPlan, PlanInput};
use super::prefs::Prefs;
use super::store::{MailRun, Pending, Target};
use super::wire::{AssistantItem, CardPhase, Choice, ChoiceKind, ItemKind, MailboxRef};
use crate::calendar::{BusyStatus, CalendarEventDto, CalendarSourceDto, RangeRead, ResponseStatus, SourceGroup, SourceKind};
use crate::intent::{self, caps, CapId, Decision, Grain, Interpretation, Lang};
use crate::local::{AppHit, FileSearch, NoteHit};
use crate::outlook_mail::{
    FreeBusy, FreeBusyStatus, MailCursor, MailHit, MailQuery, MailSearchResult, MailboxInfo,
};
use chrono::{DateTime, Duration, Local, TimeZone, Utc};

pub const MAIL_BUDGET_MS: u64 = 10_000;
pub const FILES_BUDGET_MS: u64 = 1_500;
const MAX_MAIL_HITS: usize = 100;
const MAX_LOCAL_HITS: usize = 20;
const MAX_APP_HITS: usize = 8;
const SEARCH_EVENTS_DAYS: i64 = 30;
/// Calendars offered as buttons when a person's own calendar cannot be found.
const MAX_CAL_CHOICES: usize = 8;
/// Names handed to the intent engine as people.
const MAX_KNOWN_PEOPLE: usize = 300;
/// Spellings of a name tried against the address book (free/busy), at most.
const MAX_NAME_TRIES: usize = 3;
/// No new spelling is tried after the address book has taken this long.
const NAME_TRIES_BUDGET_MS: u128 = 20_000;

/// Everything the executors read or do outside the process. The real implementation calls the
/// Outlook/Windows modules; tests supply fakes.
pub trait Sources: Send + Sync {
    fn cached_mailboxes(&self) -> Vec<MailboxInfo>;
    fn discover_mailboxes(&self, force: bool) -> Result<Vec<MailboxInfo>, String>;
    fn search_mail(&self, query: &MailQuery, cursor: Option<MailCursor>) -> Result<MailSearchResult, String>;
    fn open_mail(&self, key: &str) -> Result<(), String>;
    fn free_busy(&self, name: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<FreeBusy, String>;
    fn calendars(&self) -> Vec<CalendarSourceDto>;
    /// Organizers seen in the calendar snapshot (names only, for parsing).
    fn people(&self) -> Vec<String>;
    fn query_range(&self, from: DateTime<Utc>, to: DateTime<Utc>, only: Option<Vec<String>>) -> Result<RangeRead, String>;
    /// The in-memory prefetch, when it is fresh, covers `[from, to)` and (given `only`) has read every
    /// requested calendar. `None` means: read live with `query_range`.
    fn prefetched(&self, from: DateTime<Utc>, to: DateTime<Utc>, only: Option<&[String]>) -> Option<RangeRead>;
    fn open_event(&self, start: DateTime<Utc>) -> Result<(), String>;
    fn search_files(&self, terms: &[Vec<String>], ext: Option<&str>, limit: usize, budget_ms: u64) -> Result<FileSearch, String>;
    fn open_file(&self, key: &str) -> Result<(), String>;
    fn search_apps(&self, names: &[String], limit: usize) -> Result<Vec<AppHit>, String>;
    fn launch_app(&self, key: &str) -> Result<(), String>;
    fn search_notes(&self, terms: &[Vec<String>], latest: bool, limit: usize) -> Result<Vec<NoteHit>, String>;
    fn open_note(&self, id: &str) -> Result<(), String>;
    fn load_prefs(&self) -> Option<Prefs>;
    fn save_prefs(&self, prefs: &Prefs) -> Result<(), String>;
    /// Do what a clicked command item stands for (web search, site, settings, folder, note, new mail,
    /// lock). Only ever called from `flow::Engine::open`, i.e. after an explicit click.
    fn run_action(&self, _action: &super::actions::Action) -> Result<(), String> {
        Err("APP-042: nothing to open".into())
    }
}

/// One group of results with the targets behind its items (ids are minted when the card is built).
#[derive(Clone, Debug)]
pub struct Group {
    pub kind: &'static str,
    pub title: String,
    pub mailbox: Option<MailboxRef>,
    pub items: Vec<(AssistantItem, Target)>,
    pub truncated: bool,
    pub error_code: Option<String>,
}

/// The result of executing one interpretation, before ids are minted.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub phase: CardPhase,
    pub title: String,
    pub summary: String,
    pub question: Option<String>,
    pub choices: Vec<Choice>,
    pub groups: Vec<Group>,
    pub partial: bool,
    pub can_extend: bool,
    pub error_code: Option<String>,
    pub pending: Option<Pending>,
    pub mail: Option<MailRun>,
    /// The mailboxes a mail search used (for follow-ups and "remember").
    pub used_plan: Option<Vec<String>>,
    /// Every searchable mailbox at that time (saved with a remembered choice).
    pub searchable: Vec<String>,
    /// Show the first hits of each group in turn (multi-source answers).
    pub interleave: bool,
}

impl Outcome {
    fn new(phase: CardPhase, title: String) -> Self {
        Outcome {
            phase,
            title,
            summary: String::new(),
            question: None,
            choices: Vec::new(),
            groups: Vec::new(),
            partial: false,
            can_extend: false,
            error_code: None,
            pending: None,
            mail: None,
            used_plan: None,
            searchable: Vec::new(),
            interleave: false,
        }
    }

    pub fn answer(title: impl Into<String>, summary: impl Into<String>) -> Self {
        let mut o = Outcome::new(CardPhase::Answer, title.into());
        o.summary = summary.into();
        o
    }

    pub fn error_text(code: &str, text: impl Into<String>) -> Self {
        let mut o = Outcome::new(CardPhase::Error, text.into());
        o.error_code = Some(code.to_string());
        o
    }

    pub fn error(code: &str, lang: Lang) -> Self {
        Outcome::error_text(code, answer::error_text(code, lang))
    }

    pub fn total(&self) -> usize {
        self.groups.iter().map(|g| g.items.len()).sum()
    }
}

/// What one execution may use.
#[derive(Clone, Copy)]
pub struct Run<'a> {
    pub src: &'a dyn Sources,
    pub now: DateTime<Local>,
    pub lang: Lang,
    pub prefs: Option<&'a Prefs>,
    /// The mailbox plan of the previous mail search (a follow-up / refinement inherits it).
    pub inherited: Option<&'a [String]>,
    /// False inside a multi-source answer: no question may stop it.
    pub allow_ask: bool,
}

fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}

fn item(kind: ItemKind, title: String) -> AssistantItem {
    AssistantItem {
        id: String::new(),
        kind,
        title,
        subtitle: None,
        time: None,
        end_time: None,
        accent: None,
        openable: false,
        unread: false,
        source: None,
    }
}

fn group(kind: &'static str, title: impl Into<String>, items: Vec<(AssistantItem, Target)>) -> Group {
    Group { kind, title: title.into(), mailbox: None, items, truncated: false, error_code: None }
}

/// The capability a decision is mainly about (for choices that resume it).
pub fn decision_cap(d: &Decision) -> CapId {
    match d {
        Decision::Execute { cap } | Decision::Confirm { cap } => *cap,
        Decision::Clarify { cap: Some(cap), .. } => *cap,
        Decision::MultiSource { caps } => caps.first().copied().unwrap_or(caps::CALENDAR_LIST_EVENTS),
        _ => caps::CALENDAR_LIST_EVENTS,
    }
}

/// Run an interpretation.
pub fn execute(r: &Run, interp: &Interpretation) -> Outcome {
    match &interp.decision {
        Decision::Execute { cap } => run_cap(r, interp, *cap),
        Decision::MultiSource { caps } => multi(r, interp, caps),
        Decision::Confirm { cap } => confirm(r, interp, *cap),
        Decision::Clarify { ask, cap } => match ask {
            intent::AskKind::Mailbox => run_cap(r, interp, cap.unwrap_or(caps::EMAIL_SEARCH)),
            // "whose calendar?" is answered with the calendars that fit, never with a bare question
            intent::AskKind::Person if cap.map_or(true, |c| c.as_str().starts_with("calendar.")) => person_clarify(r, interp),
            _ => {
                // "מה לחפש?" / "מה לרשום בפתק?" / "מה לתרגם?" for the commands that lack their text
                let q = super::actions::clarify_question(interp, r.lang).unwrap_or_else(|| answer::clarify_text(*ask, r.lang));
                let mut o = Outcome::answer(q, "");
                o.question = Some(q.to_string());
                o
            }
        },
        Decision::NoMatch => {
            let (title, examples) = answer::no_match(r.lang);
            Outcome::answer(title, examples)
        }
    }
}

/// Continue a pending question with the chosen option id (`mb:<id>`, `all`, `cal:<id>`).
pub fn resume(r: &Run, pending: &Pending, option_id: &str) -> Result<Outcome, String> {
    match pending {
        Pending::Mailbox { interp, offered } => {
            let ids = if option_id == "all" {
                policy::searchable_ids(&r.src.cached_mailboxes())
            } else if let Some(id) = option_id.strip_prefix("mb:") {
                if !offered.iter().any(|o| o == id) {
                    return Err("APP-042: invalid choice".into());
                }
                vec![id.to_string()]
            } else {
                return Err("APP-042: invalid choice".into());
            };
            Ok(mail(r, interp, Some(ids)))
        }
        Pending::Calendar { interp, offered } => {
            let id = option_id.strip_prefix("cal:").filter(|id| offered.iter().any(|o| o == id));
            let Some(id) = id else { return Err("APP-042: invalid choice".into()) };
            Ok(calendar(r, interp, decision_cap(&interp.decision), Some(id)))
        }
    }
}

fn run_cap(r: &Run, interp: &Interpretation, cap: CapId) -> Outcome {
    match cap {
        caps::CALENDAR_LIST_EVENTS | caps::CALENDAR_SEARCH_EVENTS | caps::CALENDAR_CHECK_AVAILABILITY | caps::CALENDAR_RESOLVE_SHARED => {
            calendar(r, interp, cap, None)
        }
        caps::EMAIL_SEARCH => mail(r, interp, None),
        caps::EMAIL_DISCOVER_MAILBOXES => list_mailboxes(r),
        caps::NOTES_SEARCH => notes(r, interp),
        caps::FILES_SEARCH => files(r, interp),
        caps::APPS_SEARCH => apps(r, interp),
        caps::CALCULATOR_EVALUATE => calc(r, interp),
        caps::EMAIL_OPEN | caps::NOTES_OPEN | caps::FILES_OPEN | caps::APPS_LAUNCH => confirm(r, interp, cap),
        c if super::actions::handles(c) => super::actions::build(r, interp, c),
        _ => execute(r, &Interpretation { decision: Decision::NoMatch, ..interp.clone() }),
    }
}

/// Open/launch capabilities never run from text: search for the candidates and offer them.
fn confirm(r: &Run, interp: &Interpretation, cap: CapId) -> Outcome {
    // explicit commands: the card describes the action and offers it as the one click
    if super::actions::handles(cap) {
        return super::actions::build(r, interp, cap);
    }
    let read = match cap {
        caps::EMAIL_OPEN => caps::EMAIL_SEARCH,
        caps::NOTES_OPEN => caps::NOTES_SEARCH,
        caps::FILES_OPEN => caps::FILES_SEARCH,
        caps::APPS_LAUNCH => caps::APPS_SEARCH,
        other => other,
    };
    let mut o = run_cap(r, interp, read);
    if o.phase == CardPhase::Answer && o.total() > 0 {
        let tap = if cap == caps::APPS_LAUNCH { answer::tap_to_launch(r.lang) } else { answer::tap_to_open(r.lang) };
        o.summary = if o.summary.is_empty() { tap.to_string() } else { format!("{tap}\n{}", o.summary) };
    }
    o
}

fn multi(r: &Run, interp: &Interpretation, cap_list: &[CapId]) -> Outcome {
    let sub = Run { allow_ask: false, ..*r };
    let mut outs = Vec::new();
    for cap in cap_list {
        let o = run_cap(&sub, interp, *cap);
        if o.phase != CardPhase::Choices {
            outs.push(o);
        }
    }
    if outs.is_empty() || outs.iter().all(|o| o.phase == CardPhase::Error) {
        return outs.into_iter().next().unwrap_or_else(|| Outcome::error("APP-042", r.lang));
    }
    let error_code = outs.iter().find_map(|o| o.error_code.clone());
    let partial = outs.iter().any(|o| o.partial);
    let used_plan = outs.iter().find_map(|o| o.used_plan.clone());
    let searchable = outs.iter().find(|o| !o.searchable.is_empty()).map(|o| o.searchable.clone()).unwrap_or_default();
    let groups: Vec<Group> = outs.into_iter().filter(|o| o.phase == CardPhase::Answer).flat_map(|o| o.groups).collect();
    let total: usize = groups.iter().map(|g| g.items.len()).sum();
    let mut o = Outcome::new(
        CardPhase::Answer,
        if partial { answer::found_so_far(total as u32, Noun::Result, r.lang) } else { answer::found_text(total as u32, Noun::Result, r.lang) },
    );
    o.summary = kind_counts(&groups, r.lang);
    o.groups = groups;
    o.partial = partial;
    o.error_code = error_code;
    o.interleave = true;
    o.used_plan = used_plan;
    o.searchable = searchable;
    o
}

/// "3 מיילים · 2 קבצים".
fn kind_counts(groups: &[Group], lang: Lang) -> String {
    let mut order: Vec<(&'static str, u32)> = Vec::new();
    for g in groups {
        match order.iter_mut().find(|(k, _)| *k == g.kind) {
            Some((_, n)) => *n += g.items.len() as u32,
            None => order.push((g.kind, g.items.len() as u32)),
        }
    }
    order
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| {
            let noun = match k {
                "mail" => Noun::Mail,
                "files" => Noun::File,
                "notes" => Noun::Note,
                "apps" => Noun::App,
                "calendar" | "availability" => Noun::Meeting,
                _ => Noun::Result,
            };
            answer::count_text(n, noun, lang)
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

// =============================================================================
// Calendar
// =============================================================================

fn local_midnight(day: chrono::NaiveDate) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&day.and_hms_opt(0, 0, 0)?).earliest()
}

/// The asked window; default is today (or the next 30 days for a search by title).
fn window(interp: &Interpretation, cap: CapId, now: DateTime<Local>) -> (DateTime<Local>, DateTime<Local>, Grain) {
    if let Some(t) = &interp.slots.time {
        return (t.from, t.to, t.grain);
    }
    let today = local_midnight(now.date_naive()).unwrap_or(now);
    if cap == caps::CALENDAR_SEARCH_EVENTS {
        return (today, today + Duration::days(SEARCH_EVENTS_DAYS), Grain::Range);
    }
    let tomorrow = now.date_naive().succ_opt().and_then(local_midnight).unwrap_or(today + Duration::days(1));
    (today, tomorrow, Grain::Day)
}

/// Cut a window longer than the calendar's read limit to its first `MAX_QUERY_DAYS` days. The
/// flag is true when days were really dropped; an excess of up to two hours (a month across the
/// end of daylight saving time is 31 days + 1 h) is not announced.
pub(super) fn clamp_window(from: DateTime<Local>, to: DateTime<Local>) -> (DateTime<Local>, bool) {
    let limit = Duration::days(crate::calendar::MAX_QUERY_DAYS);
    if to - from <= limit {
        return (to, false);
    }
    (from + limit, to - from > limit + Duration::hours(2))
}

fn tokens(folded: &str) -> Vec<String> {
    folded.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// Calendars whose name contains the person (any spelling of the name), token-wise. A calendar
/// whose whole name is the person's (so "איציק כהן" after "איציק כהן - פרויקטים" was ruled out by
/// the user) wins over the ones that merely contain it.
pub fn match_calendars<'a>(person: &str, calendars: &'a [CalendarSourceDto]) -> Vec<&'a CalendarSourceDto> {
    let variants: Vec<Vec<String>> = intent::name_variants(person).iter().map(|v| tokens(&intent::fold(v))).filter(|t| !t.is_empty()).collect();
    let found: Vec<&CalendarSourceDto> = calendars
        .iter()
        .filter(|c| {
            let name = tokens(&intent::fold(&c.name));
            variants.iter().any(|v| v.iter().all(|t| t.chars().count() >= 2 && name.iter().any(|n| n.contains(t.as_str()))))
        })
        .collect();
    if found.len() > 1 {
        let whole: Vec<&CalendarSourceDto> = found
            .iter()
            .copied()
            .filter(|c| {
                let name = tokens(&intent::fold(&c.name));
                let owner = tokens(&intent::fold(&owner_name(&c.name)));
                variants.iter().any(|v| *v == name || *v == owner)
            })
            .collect();
        if whole.len() == 1 {
            return whole;
        }
    }
    found
}

/// The person (or team) a calendar belongs to, from its display name: Outlook may call it
/// "יומן - איציק כהן" or "Calendar - Dana Levi". The generic words and separators go; a name that
/// is nothing else stays as it is.
pub fn owner_name(calendar_name: &str) -> String {
    // `fold` turns final letters into plain ones, so the generic words are folded the same way
    let generic = [intent::fold("יומן"), intent::fold("calendar")];
    let kept: Vec<&str> = calendar_name
        .split_whitespace()
        .filter(|w| !generic.contains(&intent::fold(w.trim_matches(|c: char| !c.is_alphanumeric()))))
        .collect();
    let joined = kept.join(" ");
    let joined = joined.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | '|' | ':')).to_string();
    if joined.is_empty() { calendar_name.trim().to_string() } else { joined }
}

/// The names the intent engine may take for a person: the owners of the shared calendars the user
/// has (first: those are the people they ask about) and then the organizers seen lately.
/// Distinct, at most [`MAX_KNOWN_PEOPLE`].
pub fn known_people(calendars: &[CalendarSourceDto], organizers: Vec<String>) -> Vec<String> {
    let owners = calendars.iter().filter(|c| c.kind == SourceKind::Shared || c.group == SourceGroup::Shared).map(|c| owner_name(&c.name));
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in owners.chain(organizers) {
        let name = name.trim().to_string();
        if !name.is_empty() && seen.insert(intent::fold(&name)) {
            out.push(name);
        }
    }
    out.truncate(MAX_KNOWN_PEOPLE);
    out
}

/// What the conversation remembers once a calendar was picked from a question: the same request on
/// that calendar, so "ומה מחר?" goes on there instead of asking again.
pub fn after_calendar_choice(calendars: &[CalendarSourceDto], interp: &Interpretation, option_id: &str) -> Interpretation {
    let mut out = interp.clone();
    out.decision = Decision::Execute { cap: decision_cap(&interp.decision) };
    if let Some(picked) = option_id.strip_prefix("cal:").and_then(|id| calendars.iter().find(|c| c.id == id)) {
        out.slots.person = (picked.kind != SourceKind::Primary).then(|| owner_name(&picked.name));
    }
    out
}

/// Calendars that are not the user's own, the people's first: what to offer when the asked
/// person's calendar cannot be found or the name fits none.
fn other_calendars(calendars: &[CalendarSourceDto]) -> Vec<&CalendarSourceDto> {
    let rank = |c: &CalendarSourceDto| match (c.kind, c.group) {
        (SourceKind::Shared, _) | (_, SourceGroup::Shared) => 0,
        (SourceKind::Personal, _) => 1,
        _ => 2,
    };
    let mut out: Vec<&CalendarSourceDto> = calendars.iter().filter(|c| c.kind != SourceKind::Primary && !c.name.trim().is_empty()).collect();
    out.sort_by_key(|c| rank(c));
    out.truncate(MAX_CAL_CHOICES);
    out
}

fn is_busy(status: BusyStatus) -> bool {
    matches!(status, BusyStatus::Busy | BusyStatus::Oof | BusyStatus::Tentative)
}

fn event_item(ev: &CalendarEventDto, lang: Lang) -> (AssistantItem, Target) {
    let title = if ev.subject.trim().is_empty() { answer::no_title(lang).to_string() } else { ev.subject.clone() };
    let mut it = item(ItemKind::Event, title);
    it.subtitle = Some(if ev.all_day {
        if lang == Lang::He { "כל היום".to_string() } else { "All day".to_string() }
    } else {
        answer::range_hm(ev.start_utc, ev.end_utc)
    });
    it.time = Some(ms(ev.start_utc));
    it.end_time = Some(ms(ev.end_utc));
    it.accent = ev.color.clone().or_else(|| ev.calendar_color.clone());
    it.openable = true;
    it.source = Some(ev.calendar_name.clone());
    (it, Target::Event(ev.start_utc))
}

fn slot_items(slots: &[(DateTime<Utc>, DateTime<Utc>)], lang: Lang) -> Vec<(AssistantItem, Target)> {
    slots
        .iter()
        .map(|(a, b)| {
            let label = if lang == Lang::He { "פנוי" } else { "Free" };
            let mut it = item(ItemKind::Slot, format!("{label} {}", answer::range_hm(*a, *b)));
            it.time = Some(ms(*a));
            it.end_time = Some(ms(*b));
            (it, Target::None)
        })
        .collect()
}

fn person_of(interp: &Interpretation) -> Option<&str> {
    interp.slots.person.as_deref().map(str::trim).filter(|p| !p.is_empty())
}

fn calendar(r: &Run, interp: &Interpretation, cap: CapId, chosen: Option<&str>) -> Outcome {
    let lang = r.lang;
    let (from, wanted_to, grain) = window(interp, cap, r.now);
    // The calendar reads at most MAX_QUERY_DAYS at once: a longer question gets the first days, and says so.
    let (to, clamped) = clamp_window(from, wanted_to);
    let (from_u, to_u) = (from.with_timezone(&Utc), to.with_timezone(&Utc));
    let person = person_of(interp);
    let cals = r.src.calendars();
    let mut only: Option<Vec<String>> = None;
    let mut calendar_name: Option<String> = None;
    // Whose calendar the answer is about, in the user's words (a chosen calendar names its owner).
    let mut who: Option<String> = person.map(str::to_string);

    if let Some(id) = chosen {
        only = Some(vec![id.to_string()]);
        let picked = cals.iter().find(|c| c.id == id);
        calendar_name = picked.map(|c| c.name.clone());
        if let Some(c) = picked {
            who = (c.kind != SourceKind::Primary).then(|| owner_name(&c.name));
        }
    } else if let Some(p) = person {
        let matched = match_calendars(p, &cals);
        if cap == caps::CALENDAR_RESOLVE_SHARED {
            return resolve_shared(r, interp, p, &matched, &cals);
        }
        match matched.len() {
            0 => return free_busy_path(r, interp, cap, p, (from, to, grain), &cals),
            1 => {
                only = Some(vec![matched[0].id.clone()]);
                calendar_name = Some(matched[0].name.clone());
            }
            _ => return calendar_choices(r, interp, p, &matched),
        }
    } else if cap == caps::CALENDAR_RESOLVE_SHARED {
        return person_clarify(r, interp);
    }

    let live = match r.src.prefetched(from_u, to_u, only.as_deref()) {
        Some(read) => Ok(read),
        None => r.src.query_range(from_u, to_u, only.clone()),
    };
    let read = match live {
        Ok(read) => read,
        Err(e) => {
            let code = answer::code_of(&e, "OUTLOOK-102");
            return match who.as_deref() {
                Some(w) if only.is_some() => no_access(r, interp, w, &code, &cals),
                _ => Outcome::error(&code, lang),
            };
        }
    };
    if only.is_some() && read.events.is_empty() {
        if let Some((_, code)) = read.failed.first() {
            // The calendar is there but cannot be read: the address book may still know when they are busy.
            if let Some(w) = who.as_deref().filter(|_| code.starts_with("CAL-SHARED-")) {
                if let FbLookup::Found(fb) = free_busy_lookup(r, w, from_u, to_u) {
                    return free_busy_answer(r, cap, w, &fb, (from, to, grain));
                }
            }
            return match who.as_deref() {
                Some(w) => no_access(r, interp, w, code, &cals),
                None => Outcome::error(code, lang),
            };
        }
    }

    let mut events: Vec<&CalendarEventDto> = read.events.iter().filter(|e| e.response_status != ResponseStatus::Declined).collect();
    if cap == caps::CALENDAR_SEARCH_EVENTS && !interp.slots.terms.is_empty() {
        events.retain(|e| subject_matches(&e.subject, &interp.slots.terms));
    }
    events.sort_by_key(|e| e.start_utc);
    let day = answer::day_label(from, to, grain, r.now, lang);
    let failed_code = read.failed.first().map(|(_, c)| c.clone());

    let mut o = if cap == caps::CALENDAR_CHECK_AVAILABILITY {
        let busy: Vec<avail::Span> = events.iter().filter(|e| is_busy(e.busy_status)).map(|e| (e.start_utc, e.end_utc)).collect();
        let busy_events: Vec<&CalendarEventDto> = events.iter().copied().filter(|e| is_busy(e.busy_status)).collect();
        availability(r, who.as_deref(), &day, (from, to, grain), &busy, busy_events.iter().map(|e| event_item(e, lang)).collect())
    } else {
        let n = events.len() as u32;
        // the day goes with each time when the answer spans days ("השבוע"), even if one day has them all
        let multi_day = to - from > Duration::hours(30)
            || events.first().zip(events.last()).is_some_and(|(a, b)| a.start_utc.with_timezone(&Local).date_naive() != b.start_utc.with_timezone(&Local).date_naive());
        let times: Vec<(DateTime<Utc>, bool)> = events.iter().map(|e| (e.start_utc, e.all_day)).collect();
        let title = if cap == caps::CALENDAR_SEARCH_EVENTS {
            answer::found_text(n, Noun::Meeting, lang)
        } else {
            answer::meetings_title(n, &day, who.as_deref(), lang)
        };
        let mut o = Outcome::answer(title, answer::times_line(&times, multi_day, lang));
        let title = calendar_name.clone().unwrap_or_else(|| if lang == Lang::He { "יומן".into() } else { "Calendar".into() });
        o.groups.push(group("calendar", title, events.iter().map(|e| event_item(e, lang)).collect()));
        o
    };
    if read.truncated {
        o.partial = true;
        o.summary = join_lines(&o.summary, answer::partial_note(lang));
    }
    if clamped {
        o.summary = join_lines(&o.summary, &answer::range_clamped_note(crate::calendar::MAX_QUERY_DAYS, lang));
    }
    if let Some(code) = failed_code {
        o.error_code = Some(code.clone());
        if let Some(g) = o.groups.first_mut() {
            g.error_code = Some(code);
        }
    }
    o
}

fn join_lines(a: &str, b: &str) -> String {
    if a.is_empty() { b.to_string() } else { format!("{a}\n{b}") }
}

fn subject_matches(subject: &str, terms: &[Vec<String>]) -> bool {
    let s = intent::fold(subject);
    terms.iter().all(|alts| alts.iter().any(|a| !a.trim().is_empty() && s.contains(&intent::fold(a))))
}

fn resolve_shared(r: &Run, interp: &Interpretation, person: &str, matched: &[&CalendarSourceDto], cals: &[CalendarSourceDto]) -> Outcome {
    let lang = r.lang;
    if matched.is_empty() {
        return no_access(r, interp, person, "OUTLOOK-107", cals);
    }
    let items: Vec<(AssistantItem, Target)> = matched
        .iter()
        .map(|c| {
            let mut it = item(ItemKind::Info, c.name.clone());
            it.source = Some(c.name.clone());
            (it, Target::None)
        })
        .collect();
    let title = if lang == Lang::He {
        format!("מצאתי את היומן {}", answer::he_of(Some(person)))
    } else {
        format!("Found {person}'s calendar")
    };
    let mut o = Outcome::answer(title, "");
    o.groups.push(group("calendar", if lang == Lang::He { "יומנים" } else { "Calendars" }, items));
    o
}

/// Several calendars fit the name: which one?
fn calendar_choices(r: &Run, interp: &Interpretation, person: &str, matched: &[&CalendarSourceDto]) -> Outcome {
    let q = answer::which_calendar_of(person, r.lang);
    calendar_question(q, matched, interp)
}

/// A choices card over calendars; each button resumes the question on that calendar.
fn calendar_question(question: String, calendars: &[&CalendarSourceDto], interp: &Interpretation) -> Outcome {
    let mut o = Outcome::new(CardPhase::Choices, question.clone());
    o.question = Some(question);
    o.choices = calendars
        .iter()
        .map(|c| Choice { id: format!("cal:{}", c.id), label: c.name.clone(), kind: ChoiceKind::Option, preferred: false })
        .collect();
    o.pending = Some(Pending::Calendar { interp: interp.clone(), offered: calendars.iter().map(|c| c.id.clone()).collect() });
    o
}

/// The intent could not tell whose calendar is meant (several fit the name, or several people were
/// named): offer the calendars that fit, else the other calendars the user has, else just ask.
fn person_clarify(r: &Run, interp: &Interpretation) -> Outcome {
    let cals = r.src.calendars();
    if let Some(p) = person_of(interp) {
        let matched = match_calendars(p, &cals);
        if matched.len() > 1 {
            return calendar_choices(r, interp, p, &matched);
        }
    }
    let q = answer::clarify_text(intent::AskKind::Person, r.lang);
    let offered = other_calendars(&cals);
    if offered.is_empty() || !r.allow_ask {
        let mut o = Outcome::answer(q, "");
        o.question = Some(q.to_string());
        return o;
    }
    calendar_question(q.to_string(), &offered, interp)
}

/// The honest answer when a person's calendar cannot be read: names the person and says in one
/// short line what to do about it (see [`answer::NoAccess`]). When the name simply fits no calendar,
/// the user's other calendars are offered, in case the one meant is called something else.
fn no_access(r: &Run, interp: &Interpretation, who: &str, code: &str, cals: &[CalendarSourceDto]) -> Outcome {
    let lang = r.lang;
    let kind = answer::NoAccess::from_code(code);
    let hint = answer::no_access_hint(kind, code, lang);
    let offered = other_calendars(cals);
    if kind == answer::NoAccess::NotShared && r.allow_ask && !offered.is_empty() {
        let mut o = calendar_question(answer::no_access_choose(who, lang), &offered, interp);
        o.summary = hint;
        o.error_code = Some(code.to_string());
        return o;
    }
    let mut o = Outcome::error_text(code, answer::no_access_title(who, lang));
    o.summary = hint;
    o
}

/// The part of the asked window that is still ahead: the hours that have passed are neither free
/// nor busy any more. A moment, a past day and a future day keep their window.
fn ahead(from: DateTime<Local>, to: DateTime<Local>, grain: Grain, now: DateTime<Local>) -> DateTime<Local> {
    if grain == Grain::Instant || now <= from || now >= to {
        return from;
    }
    // to the next full five minutes, so a slot never starts at "12:07"
    let secs = now.timestamp();
    let rounded = secs - secs.rem_euclid(300) + if secs.rem_euclid(300) == 0 { 0 } else { 300 };
    match Local.timestamp_opt(rounded, 0).single() {
        Some(t) if t < to => t,
        _ => from,
    }
}

/// Availability: free when no Busy/OOF/Tentative event overlaps the window; free slots in 08-18.
fn availability(
    r: &Run,
    person: Option<&str>,
    day: &str,
    (from, to, grain): (DateTime<Local>, DateTime<Local>, Grain),
    busy: &[avail::Span],
    busy_items: Vec<(AssistantItem, Target)>,
) -> Outcome {
    let lang = r.lang;
    let multi_day = to - from > Duration::hours(30);
    // Decided on the window as asked: "היום אחר הצהריים" asked at 17:15 has less than an hour left,
    // but it is still a "when", not a yes or no about one hour.
    let specific_time = grain == Grain::Instant || (grain == Grain::Range && to - from <= Duration::hours(1));
    let from = ahead(from, to, grain, r.now);
    let busy_now = avail::overlaps(busy, from.with_timezone(&Utc), to.with_timezone(&Utc));
    let mut slots = if grain == Grain::Instant { Vec::new() } else { avail::free_slots(busy, from, to) };
    if multi_day {
        slots = avail::skip_weekend(slots, from, to);
    }
    let (title, summary) = if specific_time {
        // "האם איציק פנוי מחר ב-15:00": a plain yes or no, what is in the way and what is left of the hour
        let in_the_way: Vec<_> = avail::merge(busy.to_vec())
            .into_iter()
            .filter(|(s, e)| *s < to.with_timezone(&Utc) && *e > from.with_timezone(&Utc))
            .map(|(s, e)| (s, e, BusyKind::Busy))
            .collect();
        let mut summary = if busy_now { answer::busy_summary(&in_the_way, false, lang) } else { String::new() };
        if busy_now && !slots.is_empty() {
            summary = join_lines(&summary, &answer::slots_line(&slots, false, lang));
        }
        (answer::calendar_state(person, busy_now, day, lang), summary)
    } else if !busy_now {
        let t = if lang == Lang::He {
            format!("היומן {} פנוי {day}", answer::he_of(person))
        } else {
            format!("{} {day}", person.map_or("Your calendar is free".to_string(), |p| format!("{p}'s calendar is free")))
        };
        (t, if slots.is_empty() { String::new() } else { answer::slots_line(&slots, multi_day, lang) })
    } else if !slots.is_empty() {
        let t = if lang == Lang::He {
            format!("{day} יש חלונות פנויים {}", answer::he_to(person))
        } else {
            format!("{} free time {day}", person.map_or("You have".to_string(), |p| format!("{p} has")))
        };
        (t, answer::slots_line(&slots, multi_day, lang))
    } else {
        let t = if lang == Lang::He {
            format!("{day} אין {} זמן פנוי", answer::he_to(person))
        } else {
            format!("{} no free time {day}", person.map_or("You have".to_string(), |p| format!("{p} has")))
        };
        (t, String::new())
    };
    let mut o = Outcome::answer(title, summary);
    if !slots.is_empty() {
        o.groups.push(group("availability", if lang == Lang::He { "זמנים פנויים" } else { "Free time" }, slot_items(&slots, lang)));
    }
    if !busy_items.is_empty() {
        o.groups.push(group("calendar", if lang == Lang::He { "פגישות" } else { "Meetings" }, busy_items));
    }
    o
}

/// What the address book said about a name.
enum FbLookup {
    Found(FreeBusy),
    /// No spelling of the name resolved to one person.
    Unresolved,
    /// The lookup itself failed (code of the failure).
    Failed(String),
}

/// The spellings of a name worth asking the address book, best first: as typed, the first other
/// Hebrew spelling (a nickname: איציק -> יצחק) and the first Latin one (Itzik). Each costs a
/// round trip, so there are few.
fn name_tries(person: &str) -> Vec<String> {
    let is_he = |s: &str| s.chars().any(|c| ('\u{05D0}'..='\u{05EA}').contains(&c));
    let variants = intent::name_variants(person);
    let Some(first) = variants.first().cloned() else { return Vec::new() };
    let first_key = intent::fold(&first);
    let other = |hebrew: bool| variants.iter().skip(1).find(|v| is_he(v) == hebrew && intent::fold(v) != first_key);
    let mut out = vec![first.clone()];
    out.extend(other(true).cloned());
    out.extend(other(false).cloned());
    out.truncate(MAX_NAME_TRIES);
    out
}

/// Free/busy from the address book: the name as typed, then other spellings of it while none
/// resolves (a failure of Outlook itself ends the search).
fn free_busy_lookup(r: &Run, person: &str, from_u: DateTime<Utc>, to_u: DateTime<Utc>) -> FbLookup {
    let started = std::time::Instant::now();
    for (i, name) in name_tries(person).iter().enumerate() {
        if i > 0 && started.elapsed().as_millis() > NAME_TRIES_BUDGET_MS {
            break;
        }
        match r.src.free_busy(name, from_u, to_u) {
            Ok(fb) if fb.resolved => return FbLookup::Found(fb),
            Ok(_) => {}
            Err(e) => return FbLookup::Failed(answer::code_of(&e, "OUTLOOK-107")),
        }
    }
    FbLookup::Unresolved
}

/// The person has no calendar in the user's Outlook: ask the address book for free/busy, and when
/// that gives nothing, say so (see [`no_access`]).
fn free_busy_path(
    r: &Run,
    interp: &Interpretation,
    cap: CapId,
    person: &str,
    window: (DateTime<Local>, DateTime<Local>, Grain),
    cals: &[CalendarSourceDto],
) -> Outcome {
    let (from_u, to_u) = (window.0.with_timezone(&Utc), window.1.with_timezone(&Utc));
    match free_busy_lookup(r, person, from_u, to_u) {
        FbLookup::Found(fb) => free_busy_answer(r, cap, person, &fb, window),
        FbLookup::Unresolved => no_access(r, interp, person, "OUTLOOK-107", cals),
        FbLookup::Failed(code) => no_access(r, interp, person, &code, cals),
    }
}

/// The answer from a colleague's free/busy: busy times without titles, labelled as such.
fn free_busy_answer(r: &Run, cap: CapId, person: &str, fb: &FreeBusy, (from, to, grain): (DateTime<Local>, DateTime<Local>, Grain)) -> Outcome {
    let lang = r.lang;
    let blocks: Vec<(DateTime<Utc>, DateTime<Utc>, BusyKind)> = fb
        .blocks
        .iter()
        .filter_map(|b| {
            let kind = match b.status {
                FreeBusyStatus::Busy => BusyKind::Busy,
                FreeBusyStatus::Oof => BusyKind::Oof,
                FreeBusyStatus::Tentative => BusyKind::Tentative,
                _ => return None,
            };
            Some((b.start, b.end, kind))
        })
        .collect();
    let items: Vec<(AssistantItem, Target)> = blocks
        .iter()
        .map(|(start, end, kind)| {
            let mut it = item(ItemKind::Info, format!("{} {}", answer::busy_label(*kind, lang), answer::range_hm(*start, *end)));
            it.time = Some(ms(*start));
            it.end_time = Some(ms(*end));
            (it, Target::None)
        })
        .collect();
    let day = answer::day_label(from, to, grain, r.now, lang);
    let busy: Vec<avail::Span> = blocks.iter().map(|(a, b, _)| (*a, *b)).collect();
    let multi_day = to - from > Duration::hours(30);
    let mut o = if cap == caps::CALENDAR_CHECK_AVAILABILITY {
        availability(r, Some(person), &day, (from, to, grain), &busy, Vec::new())
    } else {
        let n = blocks.len() as u32;
        let title = if lang == Lang::He {
            match n {
                0 => format!("{day} היומן {} פנוי", answer::he_of(Some(person))),
                1 => format!("{day} יש {} חלון תפוס אחד", answer::he_to(Some(person))),
                _ => format!("{day} יש {} {n} חלונות תפוסים", answer::he_to(Some(person))),
            }
        } else {
            match n {
                0 => format!("{person}'s calendar is free {day}"),
                1 => format!("{person} has 1 busy block {day}"),
                _ => format!("{person} has {n} busy blocks {day}"),
            }
        };
        let times = if blocks.is_empty() { String::new() } else { answer::busy_summary(&blocks, multi_day, lang) };
        Outcome::answer(title, times)
    };
    o.summary = join_lines(&o.summary, answer::busy_only_note(lang));
    if !items.is_empty() {
        o.groups.push(group("availability", if lang == Lang::He { "זמנים תפוסים" } else { "Busy times" }, items));
    }
    o
}

// =============================================================================
// Mail
// =============================================================================

fn list_mailboxes(r: &Run) -> Outcome {
    let lang = r.lang;
    let mut boxes = r.src.cached_mailboxes();
    if boxes.is_empty() {
        match r.src.discover_mailboxes(false) {
            Ok(b) => boxes = b,
            Err(e) => return Outcome::error(&answer::code_of(&e, "MAIL-109"), lang),
        }
    }
    let items: Vec<(AssistantItem, Target)> = boxes
        .iter()
        .map(|m| {
            let mut it = item(ItemKind::Info, format!("{}{}", m.name, answer::mailbox_suffix(m.kind, lang)));
            if !m.searchable() {
                it.subtitle = Some(answer::error_text(policy::unavailable_code(m), lang).to_string());
            }
            (it, Target::None)
        })
        .collect();
    let n = items.len() as u32;
    let title = if lang == Lang::He { format!("יש לך {n} תיבות דואר") } else { format!("You have {n} mailboxes") };
    let mut o = Outcome::answer(title, "");
    o.groups.push(group("mail", if lang == Lang::He { "תיבות דואר" } else { "Mailboxes" }, items));
    o.searchable = policy::searchable_ids(&boxes);
    o
}

fn mail(r: &Run, interp: &Interpretation, forced: Option<Vec<String>>) -> Outcome {
    let lang = r.lang;
    let mut boxes = r.src.cached_mailboxes();
    if boxes.is_empty() {
        match r.src.discover_mailboxes(false) {
            Ok(b) => boxes = b,
            Err(e) => return Outcome::error(&answer::code_of(&e, "MAIL-109"), lang),
        }
    }
    let s = &interp.slots;
    let plan = match forced {
        Some(ids) => MailPlan::Search(ids),
        None => policy::plan(&PlanInput {
            mailboxes: &boxes,
            named: s.mailbox.as_deref(),
            all: s.all_mailboxes,
            has_sender: s.sender.as_deref().is_some_and(|x| !x.trim().is_empty()),
            inherited: if interp.follow_up { r.inherited } else { None },
            prefs: r.prefs,
            allow_ask: r.allow_ask,
        }),
    };
    let searchable = policy::searchable_ids(&boxes);
    match plan {
        MailPlan::NoneSearchable => Outcome::error(policy::none_searchable_code(&boxes), lang),
        MailPlan::NamedUnavailable(id) => {
            let code = boxes.iter().find(|m| m.id == id).map_or("MAIL-102", policy::unavailable_code);
            Outcome::error(code, lang)
        }
        MailPlan::Ask => mailbox_choices(r, interp, &boxes),
        MailPlan::Search(ids) => {
            let query = MailQuery {
                mailboxes: ids.clone(),
                terms: s.terms.clone(),
                sender: s.sender.as_deref().map(str::trim).filter(|x| !x.is_empty()).map(intent::name_variants).unwrap_or_default(),
                since: s.time.as_ref().map(|t| t.from.with_timezone(&Utc)),
                until: s.time.as_ref().map(|t| t.to.with_timezone(&Utc)),
                unread_only: s.unread,
                limit: s.limit.map_or(MAX_MAIL_HITS, |l| (l as usize).clamp(1, MAX_MAIL_HITS)),
                budget_ms: MAIL_BUDGET_MS,
            };
            let res = match r.src.search_mail(&query, None) {
                Ok(res) => res,
                Err(e) => return Outcome::error(&answer::code_of(&e, "MAIL-109"), lang),
            };
            let run = MailRun {
                query,
                cursor: res.cursor,
                hits: merge_hits(Vec::new(), res.hits, MAX_MAIL_HITS),
                boxes,
                outcomes: res.per_mailbox,
                partial: res.partial,
                latest: s.latest,
            };
            let mut o = mail_outcome(&run, lang);
            o.mail = Some(run);
            o.used_plan = Some(ids);
            o.searchable = searchable;
            o
        }
    }
}

fn mailbox_choices(r: &Run, interp: &Interpretation, boxes: &[MailboxInfo]) -> Outcome {
    let lang = r.lang;
    let q = answer::mailbox_question(lang);
    let searchable: Vec<&MailboxInfo> = boxes.iter().filter(|m| m.searchable()).collect();
    let preferred = |id: &str| r.prefs.is_some_and(|p| p.chosen.iter().any(|c| c == id));
    let mut choices: Vec<Choice> = searchable
        .iter()
        .map(|m| Choice {
            id: format!("mb:{}", m.id),
            label: format!("{}{}", m.name, answer::mailbox_suffix(m.kind, lang)),
            kind: ChoiceKind::Mailbox,
            preferred: preferred(&m.id),
        })
        .collect();
    choices.sort_by_key(|c| !c.preferred);
    choices.push(Choice { id: "all".into(), label: answer::all_mailboxes_label(lang).into(), kind: ChoiceKind::AllMailboxes, preferred: false });
    let mut o = Outcome::new(CardPhase::Choices, q.to_string());
    o.question = Some(q.to_string());
    o.choices = choices;
    o.pending = Some(Pending::Mailbox { interp: interp.clone(), offered: searchable.iter().map(|m| m.id.clone()).collect() });
    o.searchable = searchable.iter().map(|m| m.id.clone()).collect();
    o
}

/// Newest first, one entry per key, at most `limit`.
pub fn merge_hits(mut existing: Vec<MailHit>, new: Vec<MailHit>, limit: usize) -> Vec<MailHit> {
    for hit in new {
        if !existing.iter().any(|h| h.key == hit.key) {
            existing.push(hit);
        }
    }
    existing.sort_by(|a, b| b.received.cmp(&a.received));
    existing.truncate(limit);
    existing
}

/// Continue a partial search for another budget and merge what it finds.
pub fn extend_mail(src: &dyn Sources, run: &MailRun) -> Result<MailRun, String> {
    let mut query = run.query.clone();
    query.budget_ms = MAIL_BUDGET_MS;
    let res = src.search_mail(&query, run.cursor.clone())?;
    let mut outcomes = run.outcomes.clone();
    for o in res.per_mailbox {
        match outcomes.iter_mut().find(|x| x.mailbox_id == o.mailbox_id) {
            Some(x) => *x = o,
            None => outcomes.push(o),
        }
    }
    Ok(MailRun {
        query,
        cursor: res.cursor,
        hits: merge_hits(run.hits.clone(), res.hits, MAX_MAIL_HITS),
        boxes: run.boxes.clone(),
        outcomes,
        partial: res.partial,
        latest: run.latest,
    })
}

/// The card for a (possibly merged) mail run.
pub fn mail_outcome(run: &MailRun, lang: Lang) -> Outcome {
    let n = run.hits.len() as u32;
    let mut groups = Vec::new();
    for id in &run.query.mailboxes {
        let outcome = run.outcomes.iter().find(|o| &o.mailbox_id == id);
        let items: Vec<(AssistantItem, Target)> = run
            .hits
            .iter()
            .filter(|h| &h.mailbox_id == id)
            .map(|h| {
                let mailbox = run.boxes.iter().find(|m| &m.id == id).map(|m| m.name.clone());
                let mut it = item(ItemKind::Mail, if h.subject.trim().is_empty() { answer::no_title(lang).to_string() } else { h.subject.clone() });
                it.subtitle = Some(h.from.clone()).filter(|f| !f.is_empty());
                it.time = h.received.map(ms);
                it.openable = true;
                it.unread = h.unread;
                it.source = mailbox;
                (it, Target::Mail(h.key.clone()))
            })
            .collect();
        let error = outcome.and_then(|o| o.error.as_deref()).map(|e| answer::code_of(e, "MAIL-109"));
        if items.is_empty() && error.is_none() {
            continue;
        }
        let name = run.boxes.iter().find(|m| &m.id == id).map(|m| m.name.clone()).unwrap_or_default();
        groups.push(Group {
            kind: "mail",
            title: name.clone(),
            mailbox: Some(MailboxRef { id: id.clone(), name }),
            items,
            truncated: run.partial && outcome.is_some_and(|o| !o.complete),
            error_code: error,
        });
    }
    let first_error = groups.iter().find_map(|g| g.error_code.clone());
    if n == 0 && !run.outcomes.is_empty() && run.outcomes.iter().all(|o| o.error.is_some()) {
        if let Some(code) = first_error.clone() {
            let mut o = Outcome::error(&code, lang);
            o.groups = groups;
            return o;
        }
    }
    let mut o = Outcome::answer(if run.partial { answer::found_so_far(n, Noun::Mail, lang) } else { answer::found_text(n, Noun::Mail, lang) }, "");
    if run.latest {
        if let Some(h) = run.hits.first() {
            let subject = if h.subject.trim().is_empty() { answer::no_title(lang) } else { h.subject.as_str() };
            let line = if h.from.is_empty() { subject.to_string() } else if lang == Lang::He { format!("{subject} · מאת {}", h.from) } else { format!("{subject} · from {}", h.from) };
            o.summary = line;
        }
    }
    if run.partial {
        o.summary = join_lines(&o.summary, answer::partial_note(lang));
    }
    let searched = run.query.mailboxes.len();
    if searched > 1 {
        let line = if lang == Lang::He { format!("חיפשתי ב-{searched} תיבות") } else { format!("Searched {searched} mailboxes") };
        o.summary = join_lines(&o.summary, &line);
    }
    o.groups = groups;
    o.partial = run.partial;
    o.can_extend = run.partial && run.cursor.is_some();
    o.error_code = first_error;
    o
}

// =============================================================================
// Notes, files, apps, calculator
// =============================================================================

fn notes(r: &Run, interp: &Interpretation) -> Outcome {
    let lang = r.lang;
    let s = &interp.slots;
    let hits = match r.src.search_notes(&s.terms, s.latest, MAX_LOCAL_HITS) {
        Ok(h) => h,
        Err(e) => return Outcome::error(&answer::code_of(&e, "APP-001"), lang),
    };
    let items: Vec<(AssistantItem, Target)> = hits
        .iter()
        .map(|h| {
            let mut it = item(ItemKind::Note, if h.title.trim().is_empty() { answer::no_title(lang).to_string() } else { h.title.clone() });
            it.subtitle = Some(h.snippet.clone()).filter(|x| !x.is_empty());
            it.time = Some(h.updated_at);
            it.openable = true;
            // A Windows Sticky Notes hit (`sticky:<id>`) says so; it opens the Sticky Notes app.
            it.source = Some(if crate::sticky_notes::is_hit_id(&h.id) {
                crate::sticky_notes::SOURCE_LABEL.into()
            } else if lang == Lang::He {
                "פתקים".into()
            } else {
                "Notes".into()
            });
            (it, Target::Note(h.id.clone()))
        })
        .collect();
    let mut o = Outcome::answer(answer::found_text(items.len() as u32, Noun::Note, lang), "");
    o.groups.push(group("notes", if lang == Lang::He { "פתקים" } else { "Notes" }, items));
    o
}

fn files(r: &Run, interp: &Interpretation) -> Outcome {
    let lang = r.lang;
    let s = &interp.slots;
    let found = match r.src.search_files(&s.terms, s.file_ext.as_deref(), MAX_LOCAL_HITS, FILES_BUDGET_MS) {
        Ok(f) => f,
        Err(e) => return Outcome::error(&answer::code_of(&e, "FILES-101"), lang),
    };
    let items: Vec<(AssistantItem, Target)> = found
        .hits
        .iter()
        .map(|h| {
            let mut it = item(ItemKind::File, h.name.clone());
            it.subtitle = Some(h.place.clone()).filter(|x| !x.is_empty());
            it.time = h.modified.map(ms);
            it.openable = true;
            it.source = Some(h.place.clone()).filter(|x| !x.is_empty());
            (it, Target::File(h.key.clone()))
        })
        .collect();
    let n = items.len() as u32;
    let mut o = Outcome::answer(if found.partial { answer::found_so_far(n, Noun::File, lang) } else { answer::found_text(n, Noun::File, lang) }, "");
    if found.partial {
        o.summary = answer::partial_note(lang).to_string();
    }
    let mut g = group("files", if lang == Lang::He { "קבצים" } else { "Files" }, items);
    g.truncated = found.partial;
    o.groups.push(g);
    o.partial = found.partial;
    o
}

fn apps(r: &Run, interp: &Interpretation) -> Outcome {
    let lang = r.lang;
    let s = &interp.slots;
    let names: Vec<String> = match &s.app {
        Some(a) if !a.trim().is_empty() => vec![a.trim().to_string()],
        _ => s.terms.iter().flatten().filter(|t| !t.trim().is_empty()).cloned().collect(),
    };
    if names.is_empty() {
        let q = answer::clarify_text(intent::AskKind::Content, lang);
        let mut o = Outcome::answer(q, "");
        o.question = Some(q.to_string());
        return o;
    }
    let hits = match r.src.search_apps(&names, MAX_APP_HITS) {
        Ok(h) => h,
        Err(e) => return Outcome::error(&answer::code_of(&e, "APPS-104"), lang),
    };
    let items: Vec<(AssistantItem, Target)> = hits
        .iter()
        .map(|h| {
            let mut it = item(ItemKind::App, h.name.clone());
            it.subtitle = Some(if lang == Lang::He { "אפליקציה".to_string() } else { "App".to_string() });
            it.openable = true;
            (it, Target::App(h.key.clone()))
        })
        .collect();
    let mut o = Outcome::answer(answer::found_text(items.len() as u32, Noun::App, lang), "");
    o.groups.push(group("apps", if lang == Lang::He { "אפליקציות" } else { "Apps" }, items));
    o
}

fn calc(r: &Run, interp: &Interpretation) -> Outcome {
    let lang = r.lang;
    let Some(expr) = interp.slots.expr.as_deref().filter(|e| !e.trim().is_empty()) else {
        let q = answer::clarify_text(intent::AskKind::Content, lang);
        let mut o = Outcome::answer(q, "");
        o.question = Some(q.to_string());
        return o;
    };
    match intent::evaluate_expr(expr) {
        Ok(v) => {
            let value = answer::format_number(v);
            let mut o = Outcome::answer(value.clone(), format!("{expr} = {value}"));
            let it = item(ItemKind::Calc, value);
            o.groups.push(group("calc", if lang == Lang::He { "חישוב" } else { "Calculation" }, vec![(it, Target::None)]));
            o
        }
        Err(intent::CalcError::DivideByZero) => Outcome::answer(answer::calc_by_zero(lang), ""),
        Err(_) => Outcome::answer(answer::calc_failed(lang), ""),
    }
}
