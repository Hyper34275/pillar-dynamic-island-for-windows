use super::*;
use crate::assistant::exec::Sources;
use crate::assistant::wire::ChoiceKind;
use crate::calendar::{BusyStatus, CalendarEventDto, CalendarSourceDto, RangeRead, ResponseStatus, SourceGroup, SourceKind, SourceState};
use crate::intent::{caps, AskKind, Grain, Slots, TimeSpec};
use crate::local::{AppHit, FileHit, FileSearch, NoteHit};
use crate::outlook_mail::{
    FreeBusy, FreeBusyBlock, FreeBusyStatus, MailCursor, MailHit, MailQuery, MailSearchResult, MailboxAccess, MailboxAvailability, MailboxInfo,
    MailboxKind, MailboxOutcome,
};
use chrono::{Duration, TimeZone, Utc};
use std::collections::VecDeque;

// ----- fakes ---------------------------------------------------------------------------------

/// What the fake in-memory prefetch holds: the covered window, the calendars read in the last
/// round (empty = all) and the events.
struct Prefetch {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    read: Vec<String>,
    events: Vec<CalendarEventDto>,
}

#[derive(Default)]
struct Fake {
    prefetch: Option<Prefetch>,
    prefetch_asked: Mutex<Vec<Option<Vec<String>>>>,
    boxes: Vec<MailboxInfo>,
    mail: Mutex<VecDeque<Result<MailSearchResult, String>>>,
    queries: Mutex<Vec<(MailQuery, bool)>>,
    cals: Vec<CalendarSourceDto>,
    events: Vec<CalendarEventDto>,
    range_err: Option<String>,
    ranges: Mutex<Vec<Option<Vec<String>>>>,
    windows: Mutex<Vec<(DateTime<Utc>, DateTime<Utc>)>>,
    fb: Mutex<Option<Result<FreeBusy, String>>>,
    files: Vec<FileHit>,
    notes: Vec<NoteHit>,
    opened: Mutex<Vec<String>>,
    prefs: Mutex<Option<Prefs>>,
}

impl Sources for Fake {
    fn cached_mailboxes(&self) -> Vec<MailboxInfo> {
        self.boxes.clone()
    }
    fn discover_mailboxes(&self, _force: bool) -> Result<Vec<MailboxInfo>, String> {
        Err("OUTLOOK-101: not running".into())
    }
    fn search_mail(&self, q: &MailQuery, cursor: Option<MailCursor>) -> Result<MailSearchResult, String> {
        self.queries.lock().unwrap().push((q.clone(), cursor.is_some()));
        self.mail.lock().unwrap().pop_front().unwrap_or_else(|| Ok(MailSearchResult::default()))
    }
    fn open_mail(&self, key: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(format!("mail:{key}"));
        Ok(())
    }
    fn free_busy(&self, _name: &str, _from: DateTime<Utc>, _to: DateTime<Utc>) -> Result<FreeBusy, String> {
        self.fb.lock().unwrap().clone().unwrap_or_else(|| Err("OUTLOOK-101: not running".into()))
    }
    fn calendars(&self) -> Vec<CalendarSourceDto> {
        self.cals.clone()
    }
    fn people(&self) -> Vec<String> {
        Vec::new()
    }
    fn query_range(&self, from: DateTime<Utc>, to: DateTime<Utc>, only: Option<Vec<String>>) -> Result<RangeRead, String> {
        self.windows.lock().unwrap().push((from, to));
        self.ranges.lock().unwrap().push(only);
        if let Some(e) = &self.range_err {
            return Err(e.clone());
        }
        Ok(RangeRead { events: self.events.clone(), truncated: false, failed: Vec::new() })
    }
    fn prefetched(&self, from: DateTime<Utc>, to: DateTime<Utc>, only: Option<&[String]>) -> Option<RangeRead> {
        self.prefetch_asked.lock().unwrap().push(only.map(|o| o.to_vec()));
        let p = self.prefetch.as_ref()?;
        if from < p.from || to > p.to {
            return None;
        }
        if let Some(o) = only {
            if !p.read.is_empty() && !o.iter().all(|id| p.read.contains(id)) {
                return None;
            }
        }
        let events = p
            .events
            .iter()
            .filter(|e| e.start_utc < to && e.end_utc > from)
            .filter(|e| only.map_or(true, |o| o.iter().any(|id| *id == e.calendar_id)))
            .cloned()
            .collect();
        Some(RangeRead { events, truncated: false, failed: Vec::new() })
    }
    fn open_event(&self, _start: DateTime<Utc>) -> Result<(), String> {
        self.opened.lock().unwrap().push("event".into());
        Ok(())
    }
    fn search_files(&self, _t: &[Vec<String>], _e: Option<&str>, _l: usize, _b: u64) -> Result<FileSearch, String> {
        Ok(FileSearch { hits: self.files.clone(), partial: false, index_used: true })
    }
    fn open_file(&self, key: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(format!("file:{key}"));
        Ok(())
    }
    fn search_apps(&self, _n: &[String], _l: usize) -> Result<Vec<AppHit>, String> {
        Ok(vec![AppHit { key: "app1".into(), name: "Excel".into() }])
    }
    fn launch_app(&self, key: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(format!("app:{key}"));
        Ok(())
    }
    fn search_notes(&self, _t: &[Vec<String>], _latest: bool, _l: usize) -> Result<Vec<NoteHit>, String> {
        Ok(self.notes.clone())
    }
    fn open_note(&self, id: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(format!("note:{id}"));
        Ok(())
    }
    fn load_prefs(&self) -> Option<Prefs> {
        self.prefs.lock().unwrap().clone()
    }
    fn save_prefs(&self, p: &Prefs) -> Result<(), String> {
        *self.prefs.lock().unwrap() = Some(p.clone());
        Ok(())
    }
}

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2027, 3, 10, 12, 0, 0).unwrap()
}

fn at(day: u32, h: u32, m: u32) -> DateTime<Local> {
    Local.with_ymd_and_hms(2027, 3, day, h, m, 0).unwrap()
}

fn mailbox(id: &str, name: &str, kind: MailboxKind) -> MailboxInfo {
    MailboxInfo {
        id: id.into(),
        name: name.into(),
        kind,
        access: MailboxAccess::Ok,
        availability: MailboxAvailability::Ok,
        cached: None,
        instant_search: None,
    }
}

fn three_boxes() -> Vec<MailboxInfo> {
    vec![
        mailbox("m1", "Yuval Cohen", MailboxKind::Primary),
        mailbox("m2", "מכירות", MailboxKind::Shared),
        mailbox("m3", "תמיכה", MailboxKind::Shared),
    ]
}

fn cal(id: &str, name: &str) -> CalendarSourceDto {
    CalendarSourceDto {
        id: id.into(),
        name: name.into(),
        group: SourceGroup::Shared,
        kind: SourceKind::Shared,
        selected: false,
        active: false,
        pending_in_outlook: false,
        color: None,
        state: SourceState::NotSelected,
        error_code: None,
        event_count: 0,
        last_read_unix_ms: None,
    }
}

fn event(id: &str, subject: &str, start: DateTime<Local>, minutes: i64, status: BusyStatus) -> CalendarEventDto {
    CalendarEventDto {
        id: id.into(),
        calendar_id: "c1".into(),
        calendar_name: "איציק כהן".into(),
        source_kind: SourceKind::Shared,
        meeting_key: None,
        subject: subject.into(),
        start_utc: start.with_timezone(&Utc),
        end_utc: (start + Duration::minutes(minutes)).with_timezone(&Utc),
        all_day: false,
        location: None,
        organizer: None,
        is_recurring: false,
        meeting_url: None,
        busy_status: status,
        response_status: ResponseStatus::Accepted,
        color: Some("#112233".into()),
        calendar_color: None,
    }
}

fn hit(key: &str, mailbox: &str, subject: &str, day: u32) -> MailHit {
    MailHit {
        key: key.into(),
        mailbox_id: mailbox.into(),
        subject: subject.into(),
        from: "דנה".into(),
        received: Some(Utc.with_ymd_and_hms(2027, 3, day, 9, 0, 0).unwrap()),
        unread: false,
        folder: "Inbox".into(),
    }
}

fn done(id: &str) -> MailboxOutcome {
    MailboxOutcome { mailbox_id: id.into(), complete: true, error: None }
}

fn interp(decision: Decision, slots: Slots) -> Interpretation {
    Interpretation { decision, slots, confidence: 0.9, lang: Lang::He, follow_up: false, ranked: Vec::new() }
}

fn exec_cap(cap: intent::CapId, slots: Slots) -> Interpretation {
    interp(Decision::Execute { cap }, slots)
}

fn tomorrow() -> TimeSpec {
    TimeSpec { from: at(11, 0, 0), to: at(12, 0, 0), grain: Grain::Day }
}

fn terms(t: &str) -> Vec<Vec<String>> {
    vec![vec![t.to_string()]]
}

fn engine_run(e: &Engine, f: &Fake, id: &str, i: &Interpretation) -> AssistantCard {
    e.run_interpretation(f, "text", id, now(), i)
}

// ----- calendar ------------------------------------------------------------------------------

fn itzik_fake() -> Fake {
    Fake {
        cals: vec![cal("c-me", "היומן שלי"), cal("c-itzik", "איציק כהן")],
        events: vec![
            event("e1", "סטטוס", at(11, 9, 0), 60, BusyStatus::Busy),
            event("e2", "ראיון", at(11, 11, 30), 30, BusyStatus::Busy),
            event("e3", "סיכום", at(11, 14, 0), 60, BusyStatus::Busy),
        ],
        ..Fake::default()
    }
}

#[test]
fn the_final_scenario_about_a_shared_calendar() {
    let (e, f) = (Engine::new(), itzik_fake());
    let slots = Slots { person: Some("איציק".into()), time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&e, &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, slots));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.title, "מחר יש לאיציק 3 פגישות");
    assert_eq!(card.summary, "09:00 · 11:30 · 14:00");
    assert_eq!(card.total, 3);
    assert_eq!(card.items.len(), 3);
    assert_eq!(card.items[0].subtitle.as_deref(), Some("09:00–10:00"));
    assert_eq!(card.items[0].source.as_deref(), Some("איציק כהן"));
    assert_eq!(card.sources, vec!["calendar"]);
    // read exactly the matching calendar, even though it is not checked in Outlook
    assert_eq!(f.ranges.lock().unwrap().as_slice(), &[Some(vec!["c-itzik".to_string()])]);
    // the result is stored for the Center
    let r = e.results("q1", now().timestamp_millis()).unwrap();
    assert_eq!(r.groups.len(), 1);
    assert_eq!(r.groups[0].items.len(), 3);
}

#[test]
fn own_calendar_default_window_is_today() {
    let (e, f) = (Engine::new(), Fake { events: Vec::new(), ..Fake::default() });
    let card = engine_run(&e, &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, Slots::default()));
    assert_eq!(card.title, "היום אין לך פגישות");
    assert_eq!(f.ranges.lock().unwrap().as_slice(), &[None]);
}

#[test]
fn declined_meetings_are_not_counted() {
    let mut f = itzik_fake();
    f.events[1].response_status = ResponseStatus::Declined;
    let slots = Slots { person: Some("איציק".into()), time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, slots));
    assert_eq!(card.title, "מחר יש לאיציק 2 פגישות");
}

#[test]
fn person_without_a_calendar_falls_back_to_free_busy_without_titles() {
    let f = itzik_fake();
    *f.fb.lock().unwrap() = Some(Ok(FreeBusy {
        resolved: true,
        display_name: None,
        blocks: vec![
            FreeBusyBlock { start: at(11, 9, 0).with_timezone(&Utc), end: at(11, 10, 0).with_timezone(&Utc), status: FreeBusyStatus::Busy },
            FreeBusyBlock { start: at(11, 12, 0).with_timezone(&Utc), end: at(11, 13, 0).with_timezone(&Utc), status: FreeBusyStatus::Free },
        ],
    }));
    let slots = Slots { person: Some("דנה".into()), time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, slots));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.title, "מחר יש לדנה חלון תפוס אחד");
    assert!(card.summary.contains("לא את שמות הפגישות"));
    assert_eq!(card.items.len(), 1);
    assert_eq!(card.items[0].kind, ItemKind::Info);
    assert_eq!(card.items[0].title, "תפוס 09:00–10:00");
    assert!(!card.items[0].openable);
    assert!(f.ranges.lock().unwrap().is_empty(), "no calendar read for a person without a calendar");
}

#[test]
fn free_busy_failure_is_an_error_card_with_code_and_advice() {
    let f = Fake::default();
    let slots = Slots { person: Some("דנה".into()), time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, slots));
    assert_eq!(card.phase, CardPhase::Error);
    assert_eq!(card.error_code.as_deref(), Some("OUTLOOK-101"));
    assert_eq!(card.title, "לא מצאתי יומן של דנה — פתח אותו ב-Outlook");
}

#[test]
fn several_matching_calendars_ask_which_one() {
    let mut f = itzik_fake();
    f.cals.push(cal("c-itzik2", "איציק - פרויקטים"));
    let e = Engine::new();
    let slots = Slots { person: Some("איציק".into()), time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&e, &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, slots));
    assert_eq!(card.phase, CardPhase::Choices);
    assert_eq!(card.choices.len(), 2);
    assert!(card.choices.iter().all(|c| c.kind == ChoiceKind::Option));
    let answered = e.choose(&f, "q1", "cal:c-itzik2", false, now()).unwrap();
    assert_eq!(answered.phase, CardPhase::Answer);
    assert_eq!(answered.query_id, "q1");
    assert_eq!(f.ranges.lock().unwrap().last().unwrap(), &Some(vec!["c-itzik2".to_string()]));
    // an option that was never offered is refused
    assert!(e.choose(&f, "q1", "cal:other", false, now()).is_err());
}

#[test]
fn calendar_read_failure_is_reported() {
    let f = Fake { range_err: Some("OUTLOOK-102: attach failed".into()), ..Fake::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, Slots::default()));
    assert_eq!(card.phase, CardPhase::Error);
    assert_eq!(card.error_code.as_deref(), Some("OUTLOOK-102"));
}

#[test]
fn availability_lists_free_slots_in_working_hours() {
    let mut f = itzik_fake();
    f.events.push(event("e4", "אולי", at(11, 16, 0), 60, BusyStatus::Free));
    let slots = Slots { person: Some("איציק".into()), time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_CHECK_AVAILABILITY, slots));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.title, "מחר יש חלונות פנויים לאיציק");
    assert_eq!(card.summary, "פנוי: 08:00–09:00 · 10:00–11:30 · 12:00–14:00 · 15:00–18:00");
    assert_eq!(card.items[0].kind, ItemKind::Slot);
    assert_eq!(card.items[0].title, "פנוי 08:00–09:00");
}

#[test]
fn availability_free_and_fully_busy() {
    let e = Engine::new();
    let slots = Slots { time: Some(tomorrow()), ..Slots::default() };
    let free = engine_run(&e, &Fake::default(), "q1", &exec_cap(caps::CALENDAR_CHECK_AVAILABILITY, slots.clone()));
    assert_eq!(free.title, "היומן שלך פנוי מחר");
    let full = Fake { events: vec![event("e", "יום שלם", at(11, 7, 0), 13 * 60, BusyStatus::Oof)], ..Fake::default() };
    let busy = engine_run(&e, &full, "q2", &exec_cap(caps::CALENDAR_CHECK_AVAILABILITY, slots));
    assert_eq!(busy.title, "מחר אין לך זמן פנוי");
}

#[test]
fn english_answer() {
    let f = itzik_fake();
    let slots = Slots { person: Some("איציק".into()), time: Some(tomorrow()), ..Slots::default() };
    let mut i = exec_cap(caps::CALENDAR_LIST_EVENTS, slots);
    i.lang = Lang::En;
    let card = engine_run(&Engine::new(), &f, "q1", &i);
    assert_eq!(card.title, "איציק has 3 meetings tomorrow");
    assert_eq!(card.lang, Lang::En);
}

#[test]
fn search_events_filters_by_subject() {
    let f = itzik_fake();
    let slots = Slots { terms: terms("ראיון"), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_SEARCH_EVENTS, slots));
    assert_eq!(card.title, "נמצאה פגישה אחת");
    assert_eq!(card.items[0].title, "ראיון");
}

// ----- mail ----------------------------------------------------------------------------------

fn mail_slots() -> Slots {
    Slots { terms: terms("תקציב"), ..Slots::default() }
}

fn result(hits: Vec<MailHit>, partial: bool) -> MailSearchResult {
    let mut ids: Vec<String> = hits.iter().map(|h| h.mailbox_id.clone()).collect();
    ids.dedup();
    MailSearchResult {
        hits,
        per_mailbox: ids.iter().map(|i| done(i)).collect(),
        partial,
        cursor: partial.then(|| MailCursor { pending: vec!["x".into()] }),
    }
}

#[test]
fn several_mailboxes_ask_then_dont_know_searches_all_in_groups() {
    let f = Fake { boxes: three_boxes(), ..Fake::default() };
    let e = Engine::new();
    let asked = engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!(asked.phase, CardPhase::Choices);
    assert_eq!(asked.question.as_deref(), Some("באיזו תיבת דואר לחפש?"));
    let labels: Vec<_> = asked.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels[..3], ["Yuval Cohen", "מכירות (משותפת)", "תמיכה (משותפת)"]);
    assert_eq!(asked.choices[3].kind, ChoiceKind::AllMailboxes);
    assert_eq!(asked.choices[3].label, "אני לא יודע — חפש בכל התיבות שיש לי הרשאה אליהן.");
    assert!(f.queries.lock().unwrap().is_empty(), "nothing is searched before the user answers");

    // the user types "אני לא יודע"
    let Route::Choose { query_id, option_id } = e.route("אני לא יודע", now().timestamp_millis()) else { panic!("not a reply") };
    assert_eq!((query_id.as_str(), option_id.as_str()), ("q1", "all"));
    f.mail.lock().unwrap().push_back(Ok(result(vec![hit("k1", "m1", "תקציב 2027", 5), hit("k2", "m2", "תקציב שיווק", 8), hit("k3", "m2", "re: תקציב", 9)], false)));
    let card = e.choose(&f, &query_id, &option_id, false, now()).unwrap();
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.title, "נמצאו 3 מיילים");
    assert_eq!(card.total, 3);
    assert_eq!(f.queries.lock().unwrap()[0].0.mailboxes, vec!["m1", "m2", "m3"]);
    // newest first on the card
    assert_eq!(card.items[0].title, "re: תקציב");
    let r = e.results("q1", now().timestamp_millis()).unwrap();
    assert_eq!(r.groups.len(), 2, "one group per mailbox that has hits");
    assert_eq!(r.groups[1].mailbox.as_ref().unwrap().name, "מכירות");
    assert_eq!(r.groups[1].items.len(), 2);
}

#[test]
fn a_new_question_is_not_taken_for_the_answer() {
    let f = Fake { boxes: three_boxes(), ..Fake::default() };
    let e = Engine::new();
    engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert!(matches!(e.route("מה יש לי היום ביומן", now().timestamp_millis()), Route::New));
    assert!(matches!(e.route("מכירות", now().timestamp_millis()), Route::Choose { .. }));
}

#[test]
fn single_mailbox_named_mailbox_and_sender_do_not_ask() {
    let e = Engine::new();
    let one = Fake { boxes: vec![mailbox("m1", "Yuval", MailboxKind::Primary)], ..Fake::default() };
    assert_eq!(engine_run(&e, &one, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots())).phase, CardPhase::Answer);

    let many = Fake { boxes: three_boxes(), ..Fake::default() };
    let named = Slots { mailbox: Some("m3".into()), ..mail_slots() };
    engine_run(&e, &many, "q2", &exec_cap(caps::EMAIL_SEARCH, named));
    assert_eq!(many.queries.lock().unwrap()[0].0.mailboxes, vec!["m3"]);

    let sender = Slots { sender: Some("דנה".into()), ..Slots::default() };
    let card = engine_run(&e, &many, "q3", &exec_cap(caps::EMAIL_SEARCH, sender));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(many.queries.lock().unwrap()[1].0.mailboxes.len(), 3);
    // the typed name first, then its other spellings (intent::name_variants)
    let senders = many.queries.lock().unwrap()[1].0.sender.clone();
    assert_eq!(senders.first().map(String::as_str), Some("דנה"));
    assert!(senders.iter().any(|s| s == "Dana"), "{senders:?}");
}

#[test]
fn remembered_choice_is_applied_until_a_new_mailbox_appears() {
    let e = Engine::new();
    let f = Fake { boxes: three_boxes(), ..Fake::default() };
    engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    e.choose(&f, "q1", "mb:m2", true, now()).unwrap();
    let saved = f.prefs.lock().unwrap().clone().unwrap();
    assert_eq!(saved.chosen, vec!["m2"]);
    assert_eq!(saved.known, vec!["m1", "m2", "m3"]);

    // same mailboxes: no question
    let card = engine_run(&e, &f, "q2", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(f.queries.lock().unwrap().last().unwrap().0.mailboxes, vec!["m2"]);

    // a mailbox appeared since: ask again, the saved one is marked preferred
    let mut boxes = three_boxes();
    boxes.push(mailbox("m4", "חדשה", MailboxKind::Shared));
    let f2 = Fake { boxes, prefs: Mutex::new(Some(saved)), ..Fake::default() };
    let card = engine_run(&e, &f2, "q3", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!(card.phase, CardPhase::Choices);
    assert!(card.choices[0].preferred);
    assert_eq!(card.choices[0].id, "mb:m2");
}

#[test]
fn partial_search_shows_what_was_found_and_extend_merges() {
    let f = Fake { boxes: vec![mailbox("m1", "Yuval", MailboxKind::Primary)], ..Fake::default() };
    let e = Engine::new();
    f.mail.lock().unwrap().push_back(Ok(result(vec![hit("k1", "m1", "תקציב א", 5)], true)));
    let card = engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!(card.title, "נמצא מייל אחד עד כה");
    assert!(card.partial && card.can_extend);
    assert_eq!(f.queries.lock().unwrap()[0].0.budget_ms, 10_000);
    assert!(!f.queries.lock().unwrap()[0].1);

    f.mail.lock().unwrap().push_back(Ok(result(vec![hit("k1", "m1", "תקציב א", 5), hit("k2", "m1", "תקציב ב", 9)], false)));
    let card = e.extend(&f, "q1", now()).unwrap();
    assert_eq!(card.query_id, "q1");
    assert_eq!(card.title, "נמצאו 2 מיילים");
    assert!(!card.partial && !card.can_extend);
    assert_eq!(card.total, 2, "k1 is not counted twice");
    let q = f.queries.lock().unwrap();
    assert_eq!(q[1].0.budget_ms, 10_000);
    assert!(q[1].1, "the extension continues from the cursor");
    // nothing left to extend
    drop(q);
    assert!(e.extend(&f, "q1", now()).is_err());
}

#[test]
fn merge_dedupes_and_sorts_newest_first() {
    let merged = exec::merge_hits(vec![hit("a", "m1", "x", 5), hit("b", "m1", "y", 7)], vec![hit("b", "m1", "y", 7), hit("c", "m1", "z", 9)], 100);
    assert_eq!(merged.iter().map(|h| h.key.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);
    assert_eq!(exec::merge_hits(merged, vec![], 2).len(), 2);
}

#[test]
fn refinement_reruns_the_last_mail_search_with_the_inherited_mailboxes() {
    let f = Fake { boxes: three_boxes(), ..Fake::default() };
    let e = Engine::new();
    engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    e.choose(&f, "q1", "mb:m3", false, now()).unwrap();

    // "רק מהשבוע שעבר": a follow-up with the new time and the inherited terms, no mailbox in it
    let last_week = TimeSpec { from: at(3, 0, 0), to: at(10, 0, 0), grain: Grain::Week };
    let mut i = exec_cap(caps::EMAIL_SEARCH, Slots { time: Some(last_week.clone()), ..mail_slots() });
    i.follow_up = true;
    let card = engine_run(&e, &f, "q2", &i);
    assert_eq!(card.phase, CardPhase::Answer, "no new question");
    assert!(card.follow_up);
    let q = f.queries.lock().unwrap();
    let last = &q.last().unwrap().0;
    assert_eq!(last.mailboxes, vec!["m3"]);
    assert_eq!(last.since, Some(last_week.from.with_timezone(&Utc)));
    assert_eq!(last.until, Some(last_week.to.with_timezone(&Utc)));
    assert_eq!(last.terms, terms("תקציב"));
}

#[test]
fn a_fresh_question_without_follow_up_does_not_inherit() {
    let f = Fake { boxes: three_boxes(), ..Fake::default() };
    let e = Engine::new();
    engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    e.choose(&f, "q1", "mb:m3", false, now()).unwrap();
    let card = engine_run(&e, &f, "q2", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!(card.phase, CardPhase::Choices);
}

#[test]
fn mailbox_failures_are_per_mailbox() {
    let f = Fake { boxes: three_boxes(), ..Fake::default() };
    let e = Engine::new();
    let mut res = result(vec![hit("k1", "m1", "תקציב", 5)], false);
    res.per_mailbox.push(MailboxOutcome { mailbox_id: "m2".into(), complete: false, error: Some("MAIL-101: no permission".into()) });
    f.mail.lock().unwrap().push_back(Ok(res));
    let all = Slots { all_mailboxes: true, ..mail_slots() };
    let card = engine_run(&e, &f, "q1", &exec_cap(caps::EMAIL_SEARCH, all.clone()));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.error_code.as_deref(), Some("MAIL-101"));
    let r = e.results("q1", now().timestamp_millis()).unwrap();
    assert_eq!(r.groups.iter().find(|g| g.mailbox.as_ref().unwrap().id == "m2").unwrap().error_code.as_deref(), Some("MAIL-101"));

    // every mailbox failed and nothing was found: an error card
    f.mail.lock().unwrap().push_back(Ok(MailSearchResult {
        hits: vec![],
        per_mailbox: vec![MailboxOutcome { mailbox_id: "m1".into(), complete: false, error: Some("MAIL-105: timeout".into()) }],
        partial: false,
        cursor: None,
    }));
    let card = engine_run(&e, &f, "q2", &exec_cap(caps::EMAIL_SEARCH, all));
    assert_eq!(card.phase, CardPhase::Error);
    assert_eq!(card.error_code.as_deref(), Some("MAIL-105"));
}

#[test]
fn nothing_searchable_and_outlook_down() {
    let e = Engine::new();
    let mut b = mailbox("m1", "x", MailboxKind::Shared);
    b.access = MailboxAccess::Denied;
    let denied = Fake { boxes: vec![b], ..Fake::default() };
    let card = engine_run(&e, &denied, "q1", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!((card.phase, card.error_code.as_deref()), (CardPhase::Error, Some("MAIL-101")));
    // no cached mailboxes and discovery fails
    let none = Fake::default();
    let card = engine_run(&e, &none, "q2", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    assert_eq!((card.phase, card.error_code.as_deref()), (CardPhase::Error, Some("OUTLOOK-101")));
}

// ----- other sources, decisions --------------------------------------------------------------

fn file_hit() -> FileHit {
    FileHit { key: "fk".into(), name: "תקציב.xlsx".into(), place: "Documents".into(), modified: None, size: None, is_dir: false, risky: false }
}

#[test]
fn confirm_never_opens_it_offers_candidates() {
    let f = Fake { files: vec![file_hit()], ..Fake::default() };
    let e = Engine::new();
    let card = engine_run(&e, &f, "q1", &interp(Decision::Confirm { cap: caps::FILES_OPEN }, mail_slots()));
    assert_eq!(card.phase, CardPhase::Answer);
    assert!(card.summary.starts_with("לחץ כדי לפתוח"));
    assert!(card.items[0].openable);
    assert!(f.opened.lock().unwrap().is_empty(), "nothing is opened from text");

    let app = engine_run(&e, &f, "q2", &interp(Decision::Confirm { cap: caps::APPS_LAUNCH }, Slots { app: Some("אקסל".into()), ..Slots::default() }));
    assert!(app.summary.starts_with("לחץ כדי להפעיל"));
    assert!(f.opened.lock().unwrap().is_empty(), "nothing is launched from text");

    // only an explicit click on the minted id opens it
    e.open(&f, "q1", &card.items[0].id, now().timestamp_millis()).unwrap();
    assert_eq!(f.opened.lock().unwrap().as_slice(), ["file:fk"]);
}

#[test]
fn open_item_routes_by_kind_and_refuses_unknown_ids() {
    let f = Fake {
        boxes: vec![mailbox("m1", "Y", MailboxKind::Primary)],
        notes: vec![NoteHit { id: "n1".into(), title: "רעיון".into(), snippet: "s".into(), updated_at: 5, pinned: false }],
        events: vec![event("e1", "פגישה", at(10, 15, 0), 30, BusyStatus::Busy)],
        ..Fake::default()
    };
    let e = Engine::new();
    let ms = now().timestamp_millis();
    f.mail.lock().unwrap().push_back(Ok(result(vec![hit("mk", "m1", "תקציב", 5)], false)));
    let mail = engine_run(&e, &f, "qm", &exec_cap(caps::EMAIL_SEARCH, mail_slots()));
    let note = engine_run(&e, &f, "qn", &exec_cap(caps::NOTES_SEARCH, mail_slots()));
    let ev = engine_run(&e, &f, "qe", &exec_cap(caps::CALENDAR_LIST_EVENTS, Slots::default()));
    e.open(&f, "qm", &mail.items[0].id, ms).unwrap();
    e.open(&f, "qn", &note.items[0].id, ms).unwrap();
    e.open(&f, "qe", &ev.items[0].id, ms).unwrap();
    assert_eq!(f.opened.lock().unwrap().as_slice(), ["mail:mk", "note:n1", "event"]);
    // ids are opaque and bound to their query
    assert!(!mail.items[0].id.contains("mk"));
    assert!(e.open(&f, "qn", &mail.items[0].id, ms).is_err());
    assert!(e.open(&f, "qm", "nope", ms).is_err());
    assert_eq!(e.open(&f, "missing", "x", ms).unwrap_err().split(':').next(), Some("APP-041"));
    // expired after 30 minutes
    assert!(e.open(&f, "qm", &mail.items[0].id, ms + 31 * 60_000).is_err());
}

#[test]
fn no_match_is_an_answer_with_examples() {
    let card = engine_run(&Engine::new(), &Fake::default(), "q1", &interp(Decision::NoMatch, Slots::default()));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.title, "אפשר לשאול למשל: מה יש לי היום?");
}

#[test]
fn clarify_asks_in_the_card() {
    let card = engine_run(&Engine::new(), &Fake::default(), "q1", &interp(Decision::Clarify { ask: AskKind::Date, cap: None }, Slots::default()));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.question.as_deref(), Some("לאיזה תאריך התכוונת?"));
}

#[test]
fn multi_source_groups_per_source_and_never_asks() {
    let f = Fake {
        boxes: three_boxes(),
        files: vec![file_hit()],
        notes: vec![NoteHit { id: "n1".into(), title: "תקציב".into(), snippet: String::new(), updated_at: 1, pinned: false }],
        ..Fake::default()
    };
    f.mail.lock().unwrap().push_back(Ok(result(vec![hit("k1", "m1", "תקציב", 5)], false)));
    let e = Engine::new();
    let card = engine_run(&e, &f, "q1", &interp(Decision::MultiSource { caps: vec![caps::EMAIL_SEARCH, caps::FILES_SEARCH, caps::NOTES_SEARCH] }, mail_slots()));
    assert_eq!(card.phase, CardPhase::Answer);
    assert_eq!(card.title, "נמצאו 3 תוצאות");
    assert_eq!(card.sources, vec!["mail", "files", "notes"]);
    assert_eq!(card.items.len(), 3);
    assert_eq!(card.items[0].kind, ItemKind::Mail);
    assert_eq!(card.items[1].kind, ItemKind::File);
    assert_eq!(card.summary, "מייל אחד · קובץ אחד · פתק אחד");
}

#[test]
fn calculator_failure_is_an_answer() {
    let card = engine_run(&Engine::new(), &Fake::default(), "q1", &exec_cap(caps::CALCULATOR_EVALUATE, Slots { expr: Some("2+2".into()), ..Slots::default() }));
    assert_eq!(card.phase, CardPhase::Answer);
    let none = engine_run(&Engine::new(), &Fake::default(), "q2", &exec_cap(caps::CALCULATOR_EVALUATE, Slots::default()));
    assert_eq!(none.question.as_deref(), Some("מה לחפש?"));
}

#[test]
fn history_and_dismissal() {
    let e = Engine::new();
    let f = Fake::default();
    engine_run(&e, &f, "q1", &interp(Decision::NoMatch, Slots::default()));
    engine_run(&e, &f, "q2", &interp(Decision::NoMatch, Slots::default()));
    assert_eq!(e.history(now().timestamp_millis()).len(), 2);
    assert!(!e.is_dismissed("q1"));
    e.dismiss("q1");
    assert!(e.is_dismissed("q1"));
    // a dismissed query is still stored for the Center
    assert!(e.results("q1", now().timestamp_millis()).is_ok());
    assert_eq!(e.results("zzz", 0).unwrap_err().split(':').next(), Some("APP-041"));
}

// ----- privacy -------------------------------------------------------------------------------

#[test]
fn the_log_line_has_no_query_text_names_or_subjects() {
    let f = Fake { boxes: vec![mailbox("m1", "תיבה סודית", MailboxKind::Primary)], ..Fake::default() };
    f.mail.lock().unwrap().push_back(Ok(result(vec![hit("k1", "m1", "נושא סודי", 5)], false)));
    let e = Engine::new();
    let slots = Slots { terms: terms("תקציב-סודי"), sender: Some("דנה-סודית".into()), ..Slots::default() };
    let card = e.run_interpretation(&f, "מה התקציב הסודי של דנה", "abcd1234abcd1234", now(), &exec_cap(caps::EMAIL_SEARCH, slots));
    let line = log_line(&card, "execute:email.search", 12);
    for secret in ["תקציב", "סודי", "דנה", "נושא", "תיבה", "מייל"] {
        assert!(!line.contains(secret), "{line}");
    }
    assert!(line.contains("abcd1234abcd1234") && line.contains("execute:email.search") && line.contains("total=1"));
    let err = error_card("q", "secret text", Lang::He, "MAIL-101", 0);
    let line = log_line(&err, "execute:email.search", 3);
    assert!(line.contains("code=MAIL-101") && !line.contains("secret"));
}

// ----- end to end: the real intent engine in front of the orchestrator ------------------------

/// The spec's final scenario, typed text all the way (intent::interpret is real here).
#[test]
fn end_to_end_final_scenario_with_real_understanding() {
    let e = Engine::new();
    let mut f = itzik_fake();
    f.boxes = three_boxes();

    // 1. "מה יש לאיציק ביומן מחר?" -> Itzik's shared calendar, tomorrow, count + times
    let card = e.submit(&f, "מה יש לאיציק ביומן מחר?", "q1", now());
    assert_eq!(card.phase, CardPhase::Answer, "{card:?}");
    assert_eq!(card.title, "מחר יש לאיציק 3 פגישות");
    assert_eq!(card.summary, "09:00 · 11:30 · 14:00");
    assert_eq!(f.ranges.lock().unwrap().as_slice(), &[Some(vec!["c-itzik".to_string()])]);

    // 2. "תמצא את המייל עם המילה תקציב." with three mailboxes -> which mailbox?
    let partial = MailSearchResult {
        hits: vec![hit("k1", "m1", "תקציב 2027", 9), hit("k2", "m2", "re: תקציב", 8)],
        per_mailbox: vec![done("m1"), MailboxOutcome { mailbox_id: "m2".into(), complete: false, error: None }],
        partial: true,
        cursor: Some(MailCursor { pending: vec!["rest".into()] }),
    };
    f.mail.lock().unwrap().push_back(Ok(partial));
    let card = e.submit(&f, "תמצא את המייל עם המילה תקציב.", "q2", now());
    assert_eq!(card.phase, CardPhase::Choices, "{card:?}");
    assert_eq!(card.question.as_deref(), Some("באיזו תיבת דואר לחפש?"));
    assert_eq!(card.choices.len(), 4);
    assert_eq!(card.choices.last().unwrap().kind, ChoiceKind::AllMailboxes);
    assert!(f.queries.lock().unwrap().is_empty(), "nothing is searched before the answer");

    // 3. typed answer "אני לא יודע" -> every permitted mailbox, 10 s budget, partial results shown
    let Route::Choose { query_id, option_id } = e.route("אני לא יודע", now().timestamp_millis()) else {
        panic!("the typed reply must answer the pending question")
    };
    assert_eq!(query_id, "q2");
    let card = e.choose(&f, &query_id, &option_id, false, now()).unwrap();
    assert_eq!(card.phase, CardPhase::Answer);
    assert!(card.partial && card.can_extend, "{card:?}");
    assert_eq!(card.total, 2);
    {
        let q = f.queries.lock().unwrap();
        assert_eq!(q[0].0.mailboxes.len(), 3);
        assert_eq!(q[0].0.budget_ms, 10_000);
        assert!(q[0].0.terms.iter().flatten().any(|t| t == "תקציב"), "{:?}", q[0].0.terms);
    }

    // 4. "search 10 more seconds" continues from the cursor
    let card = e.extend(&f, "q2", now()).unwrap();
    assert_eq!(card.phase, CardPhase::Answer);
    assert!(f.queries.lock().unwrap()[1].1, "the extension passes the cursor back");

    // 5. "רק מהשבוע שעבר" refines the last mail search (same mailboxes, same term, last week)
    let card = e.submit(&f, "רק מהשבוע שעבר", "q3", now());
    assert!(card.follow_up, "{card:?}");
    let q = f.queries.lock().unwrap();
    let last = &q.last().unwrap().0;
    assert_eq!(last.mailboxes.len(), 3, "the mailbox plan is inherited");
    assert!(last.terms.iter().flatten().any(|t| t == "תקציב"));
    // now = Wed 10.3.2027: last week = Sun 28.2 .. Sun 7.3 (exclusive)
    assert_eq!(last.since, Some(Local.with_ymd_and_hms(2027, 2, 28, 0, 0, 0).unwrap().with_timezone(&Utc)));
    assert_eq!(last.until, Some(Local.with_ymd_and_hms(2027, 3, 7, 0, 0, 0).unwrap().with_timezone(&Utc)));
}

// ----- review fixes --------------------------------------------------------------------------

#[test]
fn a_window_longer_than_the_calendar_limit_is_cut_and_says_so() {
    let f = Fake { events: Vec::new(), ..Fake::default() };
    let year = TimeSpec { from: at(1, 0, 0), to: at(1, 0, 0) + Duration::days(365), grain: Grain::Range };
    let slots = Slots { time: Some(year), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, slots.clone()));
    // the read is cut to the first MAX_QUERY_DAYS days (not rejected with OUTLOOK-108)
    let (from, to) = f.windows.lock().unwrap()[0];
    assert_eq!(to - from, Duration::days(crate::calendar::MAX_QUERY_DAYS));
    assert_eq!(card.phase, CardPhase::Answer);
    assert!(card.error_code.is_none());
    assert!(card.summary.contains("31 הימים הראשונים"), "{}", card.summary);
    let mut en = exec_cap(caps::CALENDAR_LIST_EVENTS, slots);
    en.lang = Lang::En;
    let card = engine_run(&Engine::new(), &f, "q2", &en);
    assert!(card.summary.contains("first 31 days"), "{}", card.summary);
}

#[test]
fn a_month_across_the_end_of_daylight_saving_is_cut_without_a_note() {
    use super::exec::clamp_window;
    let from = at(1, 0, 0);
    // 31 days + 1 h (October in Israel): cut to the limit the calendar accepts, nothing announced
    let (to, announced) = clamp_window(from, from + Duration::days(31) + Duration::hours(1));
    assert_eq!(to, from + Duration::days(31));
    assert!(!announced);
    assert_eq!(clamp_window(from, from + Duration::days(30)), (from + Duration::days(30), false));
    assert_eq!(clamp_window(from, from + Duration::days(31)), (from + Duration::days(31), false));
    assert!(clamp_window(from, from + Duration::days(34)).1);
}

#[test]
fn answering_a_closed_card_shows_the_query_again() {
    let e = Engine::new();
    let f = Fake::default();
    engine_run(&e, &f, "q1", &interp(Decision::NoMatch, Slots::default()));
    let ms = now().timestamp_millis();
    e.dismiss("q1");
    assert!(e.is_dismissed("q1"));
    assert!(e.resume("q1", ms).is_some());
    assert!(!e.is_dismissed("q1"));
    // an unknown or expired id changes nothing
    e.dismiss("gone");
    assert!(e.resume("gone", ms).is_none());
    assert!(e.is_dismissed("gone"));
}

#[test]
fn switching_off_forgets_the_queries_and_the_conversation() {
    let e = Engine::new();
    let f = Fake { events: Vec::new(), ..Fake::default() };
    engine_run(&e, &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, Slots::default()));
    e.dismiss("q1");
    let ms = now().timestamp_millis();
    assert_eq!(e.history(ms).len(), 1);
    assert!(lock(&e.session).ctx.last(ms).is_some());
    e.clear();
    assert!(e.history(ms).is_empty());
    assert!(e.results("q1", ms).is_err());
    assert!(e.query_of("q1", ms).is_none());
    assert!(!e.is_dismissed("q1"));
    assert!(lock(&e.session).ctx.last(ms).is_none());
    assert!(lock(&e.session).last_plan.is_none());
}

#[test]
fn an_expired_mail_key_is_a_clear_card_text() {
    assert_eq!(answer::code_of("MAIL-104: mail no longer available", "APP-001"), "MAIL-104");
    assert_eq!(answer::error_text("MAIL-104", Lang::He), "המייל כבר לא זמין");
    assert_eq!(answer::error_text("MAIL-104", Lang::En), "The mail is no longer available");
    let card = error_card("q1", "x", Lang::He, "MAIL-104", 0);
    assert_eq!(card.phase, CardPhase::Error);
    assert_eq!(card.title, "המייל כבר לא זמין");
}

#[test]
fn fifty_concurrent_submits_do_not_deadlock_and_each_gets_a_final_card() {
    use super::super::store::MAX_QUERIES;
    let e = Engine::new();
    let f = Fake { cals: vec![cal("c-me", "היומן שלי")], events: vec![event("e1", "סטטוס", at(10, 9, 0), 60, BusyStatus::Busy)], ..Fake::default() };
    let started = std::time::Instant::now();
    let cards: Vec<AssistantCard> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..50)
            .map(|i| {
                let (e, f) = (&e, &f);
                s.spawn(move || {
                    let text = if i % 2 == 0 { "מה יש לי היום ביומן" } else { "what is on my calendar today" };
                    let card = e.submit(f, text, &format!("load{i:02}"), now());
                    // readers run alongside the writers
                    let _ = e.history(now().timestamp_millis());
                    let _ = e.results(&card.query_id, now().timestamp_millis());
                    card
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("a submit panicked")).collect()
    });
    let elapsed = started.elapsed();
    assert_eq!(cards.len(), 50);
    for c in &cards {
        assert_ne!(c.phase, CardPhase::Processing, "every query ends in a final card");
    }
    let ids: std::collections::HashSet<_> = cards.iter().map(|c| c.query_id.clone()).collect();
    assert_eq!(ids.len(), 50);
    assert_eq!(lock(&e.store).len(), MAX_QUERIES);
    println!("LOAD50 elapsed_ms={}", elapsed.as_millis());
    assert!(elapsed < std::time::Duration::from_secs(10));
}

// ----- prefetch ------------------------------------------------------------------------------

/// Moves the three meetings into the prefetch (calendar "c-itzik", window day `from_day`..`to_day`)
/// and leaves one different meeting for the live read, so the two paths tell apart.
fn itzik_prefetch(f: &mut Fake, from_day: u32, to_day: u32, read: Vec<String>) {
    let mut events = f.events.clone();
    events.iter_mut().for_each(|e| e.calendar_id = "c-itzik".into());
    f.prefetch = Some(Prefetch { from: at(from_day, 0, 0).with_timezone(&Utc), to: at(to_day, 0, 0).with_timezone(&Utc), read, events });
    f.events = vec![event("live", "חי", at(11, 8, 0), 30, BusyStatus::Busy)];
}

fn itzik_slots() -> Slots {
    Slots { person: Some("איציק".into()), time: Some(tomorrow()), ..Slots::default() }
}

#[test]
fn a_prefetch_hit_answers_without_reading_outlook() {
    let mut f = itzik_fake();
    itzik_prefetch(&mut f, 9, 20, Vec::new());
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, itzik_slots()));
    assert_eq!(card.title, "מחר יש לאיציק 3 פגישות");
    assert!(f.ranges.lock().unwrap().is_empty(), "query_range must not run on a hit");
    assert_eq!(f.prefetch_asked.lock().unwrap().as_slice(), &[Some(vec!["c-itzik".to_string()])]);
}

#[test]
fn a_prefetch_hit_serves_search_and_availability_too() {
    let mut f = itzik_fake();
    itzik_prefetch(&mut f, 9, 20, Vec::new());
    let slots = Slots { terms: terms("ראיון"), ..itzik_slots() };
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_SEARCH_EVENTS, slots));
    assert_eq!(card.total, 1);
    engine_run(&Engine::new(), &f, "q2", &exec_cap(caps::CALENDAR_CHECK_AVAILABILITY, itzik_slots()));
    assert!(f.ranges.lock().unwrap().is_empty());
}

#[test]
fn a_window_outside_the_prefetch_reads_live() {
    let mut f = itzik_fake();
    itzik_prefetch(&mut f, 1, 5, Vec::new());
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, itzik_slots()));
    assert_eq!(card.total, 1, "the live meeting, not the prefetched three");
    assert_eq!(f.ranges.lock().unwrap().as_slice(), &[Some(vec!["c-itzik".to_string()])]);
}

#[test]
fn a_stale_prefetch_or_an_unread_calendar_reads_live() {
    // no fresh prefetch: prefetched() answers None
    let f = itzik_fake();
    engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, itzik_slots()));
    assert_eq!(f.ranges.lock().unwrap().len(), 1);
    // the calendar was not read in the last round
    let mut g = itzik_fake();
    itzik_prefetch(&mut g, 9, 20, vec!["c-me".to_string()]);
    engine_run(&Engine::new(), &g, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, itzik_slots()));
    assert_eq!(g.ranges.lock().unwrap().len(), 1);
}

#[test]
fn the_prefetch_only_filter_is_respected() {
    let mut f = itzik_fake();
    itzik_prefetch(&mut f, 9, 20, Vec::new());
    let mut other = event("o1", "אחר", at(11, 10, 0), 30, BusyStatus::Busy);
    other.calendar_id = "c-me".into();
    f.prefetch.as_mut().unwrap().events.push(other);
    let card = engine_run(&Engine::new(), &f, "q1", &exec_cap(caps::CALENDAR_LIST_EVENTS, itzik_slots()));
    assert_eq!(card.total, 3, "the other calendar's meeting must not leak into a person's answer");
    let own = Slots { time: Some(tomorrow()), ..Slots::default() };
    let card = engine_run(&Engine::new(), &f, "q2", &exec_cap(caps::CALENDAR_LIST_EVENTS, own));
    assert_eq!(card.total, 4);
}

#[test]
fn a_sticky_note_hit_says_where_it_is_from_and_is_openable() {
    let f = Fake {
        notes: vec![
            NoteHit { id: "sticky:3f2a-guid".into(), title: "רשימת קניות".into(), snippet: "חלב".into(), updated_at: 9, pinned: false },
            NoteHit { id: "n1".into(), title: "רעיון".into(), snippet: String::new(), updated_at: 5, pinned: false },
        ],
        ..Fake::default()
    };
    let e = Engine::new();
    let card = engine_run(&e, &f, "qs", &exec_cap(caps::NOTES_SEARCH, mail_slots()));
    assert_eq!(card.items.len(), 2);
    assert_eq!(card.items[0].kind, ItemKind::Note);
    assert_eq!(card.items[0].source.as_deref(), Some("Sticky Notes"));
    assert!(card.items[0].openable);
    assert_eq!(card.items[1].source.as_deref(), Some("פתקים"), "the island's own notes keep their label");
    // a click reaches open_note with the hit id (the live source launches Sticky Notes for the prefix)
    e.open(&f, "qs", &card.items[0].id, now().timestamp_millis()).unwrap();
    assert_eq!(f.opened.lock().unwrap().as_slice(), ["note:sticky:3f2a-guid"]);
}
