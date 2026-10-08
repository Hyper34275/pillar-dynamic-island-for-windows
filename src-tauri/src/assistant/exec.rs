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
use crate::calendar::{BusyStatus, CalendarEventDto, CalendarSourceDto, RangeRead, ResponseStatus};
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
            _ => {
                let q = answer::clarify_text(*ask, r.lang);
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
        _ => execute(r, &Interpretation { decision: Decision::NoMatch, ..interp.clone() }),
    }
}

/// Open/launch capabilities never run from text: search for the candidates and offer them.
fn confirm(r: &Run, interp: &Interpretation, cap: CapId) -> Outcome {
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

/// Calendars whose name contains the person (any spelling of the name), token-wise.
pub fn match_calendars<'a>(person: &str, calendars: &'a [CalendarSourceDto]) -> Vec<&'a CalendarSourceDto> {
    let variants: Vec<Vec<String>> = intent::name_variants(person).iter().map(|v| tokens(&intent::fold(v))).filter(|t| !t.is_empty()).collect();
    calendars
        .iter()
        .filter(|c| {
            let name = tokens(&intent::fold(&c.name));
            variants.iter().any(|v| v.iter().all(|t| t.chars().count() >= 2 && name.iter().any(|n| n.contains(t.as_str()))))
        })
        .collect()
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

    if let Some(id) = chosen {
        only = Some(vec![id.to_string()]);
        calendar_name = cals.iter().find(|c| c.id == id).map(|c| c.name.clone());
    } else if let Some(p) = person {
        let matched = match_calendars(p, &cals);
        if cap == caps::CALENDAR_RESOLVE_SHARED {
            return resolve_shared(r, p, &matched);
        }
        match matched.len() {
            0 => return free_busy_path(r, cap, p, (from, to, grain)),
            1 => {
                only = Some(vec![matched[0].id.clone()]);
                calendar_name = Some(matched[0].name.clone());
            }
            _ => return calendar_choices(r, interp, &matched),
        }
    } else if cap == caps::CALENDAR_RESOLVE_SHARED {
        let q = answer::clarify_text(intent::AskKind::Person, lang);
        let mut o = Outcome::answer(q, "");
        o.question = Some(q.to_string());
        return o;
    }

    let live = match r.src.prefetched(from_u, to_u, only.as_deref()) {
        Some(read) => Ok(read),
        None => r.src.query_range(from_u, to_u, only.clone()),
    };
    let read = match live {
        Ok(read) => read,
        Err(e) => {
            let code = answer::code_of(&e, "OUTLOOK-102");
            return match person {
                Some(p) if only.is_some() => Outcome::error_text(&code, answer::calendar_not_found(p, lang)),
                _ => Outcome::error(&code, lang),
            };
        }
    };
    if only.is_some() && read.events.is_empty() {
        if let Some((_, code)) = read.failed.first() {
            return match person {
                Some(p) => Outcome::error_text(code, answer::calendar_not_found(p, lang)),
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
        availability(r, person, &day, (from, to, grain), &busy, busy_events.iter().map(|e| event_item(e, lang)).collect())
    } else {
        let n = events.len() as u32;
        let multi_day = events.first().zip(events.last()).is_some_and(|(a, b)| a.start_utc.with_timezone(&Local).date_naive() != b.start_utc.with_timezone(&Local).date_naive());
        let times: Vec<(DateTime<Utc>, bool)> = events.iter().map(|e| (e.start_utc, e.all_day)).collect();
        let title = if cap == caps::CALENDAR_SEARCH_EVENTS {
            answer::found_text(n, Noun::Meeting, lang)
        } else {
            answer::meetings_title(n, &day, person, lang)
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

fn resolve_shared(r: &Run, person: &str, matched: &[&CalendarSourceDto]) -> Outcome {
    let lang = r.lang;
    if matched.is_empty() {
        return Outcome::error_text("OUTLOOK-107", answer::calendar_not_found(person, lang));
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

fn calendar_choices(r: &Run, interp: &Interpretation, matched: &[&CalendarSourceDto]) -> Outcome {
    let q = answer::calendar_question(r.lang);
    let mut o = Outcome::new(CardPhase::Choices, q.to_string());
    o.question = Some(q.to_string());
    o.choices = matched
        .iter()
        .map(|c| Choice { id: format!("cal:{}", c.id), label: c.name.clone(), kind: ChoiceKind::Option, preferred: false })
        .collect();
    o.pending = Some(Pending::Calendar { interp: interp.clone(), offered: matched.iter().map(|c| c.id.clone()).collect() });
    o
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
    let busy_now = avail::overlaps(busy, from.with_timezone(&Utc), to.with_timezone(&Utc));
    let slots = if grain == Grain::Instant { Vec::new() } else { avail::free_slots(busy, from, to) };
    let multi_day = to - from > Duration::hours(30);
    let (title, summary) = if !busy_now {
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

/// The person has no calendar in the user's Outlook: ask the address book for free/busy.
fn free_busy_path(r: &Run, cap: CapId, person: &str, (from, to, grain): (DateTime<Local>, DateTime<Local>, Grain)) -> Outcome {
    let lang = r.lang;
    let (from_u, to_u) = (from.with_timezone(&Utc), to.with_timezone(&Utc));
    let name = intent::name_variants(person).into_iter().next().unwrap_or_else(|| person.to_string());
    let fb = match r.src.free_busy(&name, from_u, to_u) {
        Ok(fb) => fb,
        Err(e) => return Outcome::error_text(&answer::code_of(&e, "OUTLOOK-107"), answer::calendar_not_found(person, lang)),
    };
    if !fb.resolved {
        return Outcome::error_text("OUTLOOK-107", answer::calendar_not_found(person, lang));
    }
    let blocks: Vec<_> = fb.blocks.iter().filter(|b| matches!(b.status, FreeBusyStatus::Busy | FreeBusyStatus::Oof | FreeBusyStatus::Tentative)).collect();
    let items: Vec<(AssistantItem, Target)> = blocks
        .iter()
        .map(|b| {
            let kind = match b.status {
                FreeBusyStatus::Oof => BusyKind::Oof,
                FreeBusyStatus::Tentative => BusyKind::Tentative,
                _ => BusyKind::Busy,
            };
            let mut it = item(ItemKind::Info, format!("{} {}", answer::busy_label(kind, lang), answer::range_hm(b.start, b.end)));
            it.time = Some(ms(b.start));
            it.end_time = Some(ms(b.end));
            (it, Target::None)
        })
        .collect();
    let day = answer::day_label(from, to, grain, r.now, lang);
    let busy: Vec<avail::Span> = blocks.iter().map(|b| (b.start, b.end)).collect();
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
        Outcome::answer(title, "")
    };
    o.summary = join_lines(&o.summary, answer::busy_only_note(lang));
    o.groups.push(group("availability", if lang == Lang::He { "זמנים תפוסים" } else { "Busy times" }, items));
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
