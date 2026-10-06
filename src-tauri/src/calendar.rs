//! Calendar service: contract types, the connection state machine and the supervisor.
//!
//! The supervisor thread (`companyisland-calendar`) owns the [`Machine`], does the cheap
//! process discovery itself and sends Outlook reads to a dedicated STA worker
//! (`outlook::Worker`). It doubles as the watchdog: a read that does not answer within
//! [`WATCHDOG_MS`] makes it abandon the (hung) worker and start a fresh one later.
//!
//! The machine is pure (no clock, no IO): every input carries a [`Now`], so schedules
//! and backoff are unit-tested with a simulated clock and a fake source.
//!
//! Privacy: events live only in memory. Logs carry counts, status names and error codes.

use crate::calendar_diag::{self, OutlookDiag};
use crate::outlook::{self, Discovery};
use crate::settings::SettingsStore;
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, Utc};
use serde::{Serialize, Serializer};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

// =============================================================================
// Contract types (camelCase over IPC, see docs/ENTERPRISE_DESIGN.md section 1)
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CalendarStatus {
    Waiting,
    Connecting,
    Connected,
    NewOutlookOnly,
    ElevationMismatch,
    Unresponsive,
    Failed,
}

impl CalendarStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CalendarStatus::Waiting => "waiting",
            CalendarStatus::Connecting => "connecting",
            CalendarStatus::Connected => "connected",
            CalendarStatus::NewOutlookOnly => "newOutlookOnly",
            CalendarStatus::ElevationMismatch => "elevationMismatch",
            CalendarStatus::Unresponsive => "unresponsive",
            CalendarStatus::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BusyStatus {
    Free,
    Tentative,
    Busy,
    Oof,
    WorkingElsewhere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseStatus {
    None,
    Organized,
    Tentative,
    Accepted,
    Declined,
    NotResponded,
}

fn serialize_iso<S: Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

fn serialize_iso_opt<S: Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match t {
        Some(t) => serialize_iso(t, s),
        None => s.serialize_none(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventDto {
    pub id: String,
    pub calendar_id: String,
    pub subject: String,
    #[serde(serialize_with = "serialize_iso")]
    pub start_utc: DateTime<Utc>,
    #[serde(serialize_with = "serialize_iso")]
    pub end_utc: DateTime<Utc>,
    pub all_day: bool,
    pub location: Option<String>,
    pub organizer: Option<String>,
    pub is_recurring: bool,
    pub meeting_url: Option<String>,
    pub busy_status: BusyStatus,
    pub response_status: ResponseStatus,
    /// `#RRGGBB` of the first of the item's Outlook categories that has a color.
    pub color: Option<String>,
}

/// An unread meeting request in the default Inbox. Memory only, like events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingInviteDto {
    /// Hash of the request's EntryID; the EntryID itself never leaves the COM worker.
    pub id: String,
    pub subject: String,
    pub organizer: Option<String>,
    /// The requested meeting, when Outlook can tell (its tentative calendar entry).
    #[serde(serialize_with = "serialize_iso_opt")]
    pub start_utc: Option<DateTime<Utc>>,
    #[serde(serialize_with = "serialize_iso_opt")]
    pub end_utc: Option<DateTime<Utc>>,
    pub location: Option<String>,
    #[serde(serialize_with = "serialize_iso")]
    pub received_utc: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    pub status: CalendarStatus,
    pub error_code: Option<String>,
    pub last_sync_unix_ms: Option<i64>,
    pub cached_count: usize,
    pub next_retry_unix_ms: Option<i64>,
    pub events: Vec<CalendarEventDto>,
    /// Newest first, at most [`MAX_INVITES`]; empty while invites are switched off.
    pub invites: Vec<MeetingInviteDto>,
}

impl Default for CalendarSnapshot {
    fn default() -> Self {
        CalendarSnapshot {
            status: CalendarStatus::Waiting,
            error_code: None,
            last_sync_unix_ms: None,
            cached_count: 0,
            next_retry_unix_ms: None,
            events: Vec::new(),
            invites: Vec::new(),
        }
    }
}

/// Managed Tauri state: the latest published snapshot.
#[derive(Default)]
pub struct CalendarState(Mutex<CalendarSnapshot>);

impl CalendarState {
    /// Store `next`; true when it differs from what was stored (and so must be emitted).
    fn replace_if_changed(&self, next: &CalendarSnapshot) -> bool {
        let mut current = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if *current == *next {
            return false;
        }
        *current = next.clone();
        true
    }

    fn get(&self) -> CalendarSnapshot {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// =============================================================================
// Source abstraction
// =============================================================================

pub const MAX_EVENTS: usize = 50;
pub const MAX_INVITES: usize = 10;
pub const HORIZON_HOURS: i64 = 48;

#[derive(Clone, Debug)]
pub struct FetchWindow {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Also read the unread meeting requests (the `meetingInvitesEnabled` setting).
    pub invites: bool,
}

impl FetchWindow {
    pub fn starting_at(now: DateTime<Utc>) -> Self {
        FetchWindow { from: now, to: now + ChronoDuration::hours(HORIZON_HOURS), invites: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    /// Outlook runs but is not (yet) in the running object table.
    NotInRot,
    /// Call rejected: the server is busy (modal dialog, startup).
    Busy,
    /// The server went away; every cached object must be dropped.
    Disconnected,
    /// Object model guard or policy blocked the read.
    Blocked,
    Failed,
}

#[derive(Clone, Debug)]
pub struct SourceError {
    pub kind: ErrKind,
    pub code: &'static str,
    /// HRESULT/stage text only; never item content.
    pub detail: String,
}

impl SourceError {
    pub fn new(kind: ErrKind, code: &'static str, detail: impl Into<String>) -> Self {
        SourceError { kind, code, detail: detail.into() }
    }
}

/// What one read returns: the events inside the window and, when asked for, the invites.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    pub events: Vec<CalendarEventDto>,
    pub invites: Vec<MeetingInviteDto>,
}

impl From<Vec<CalendarEventDto>> for Fetched {
    fn from(events: Vec<CalendarEventDto>) -> Self {
        Fetched { events, invites: Vec::new() }
    }
}

pub type FetchResult = Result<Fetched, SourceError>;

/// A calendar backend. `fetch` returns the events (all calendars it covers) inside the window.
pub trait CalendarSource {
    fn fetch(&mut self, window: &FetchWindow) -> FetchResult;
}

/// Sort by start, drop duplicate ids, cap at [`MAX_EVENTS`].
pub fn normalize_events(mut events: Vec<CalendarEventDto>) -> Vec<CalendarEventDto> {
    events.sort_by(|a, b| a.start_utc.cmp(&b.start_utc).then(a.end_utc.cmp(&b.end_utc)).then_with(|| a.id.cmp(&b.id)));
    events.dedup_by(|a, b| a.id == b.id);
    events.truncate(MAX_EVENTS);
    events
}

// =============================================================================
// State machine
// =============================================================================

const DISCOVER_POLL_MS: u64 = 15_000;
const SYNC_INTERVAL_MS: u64 = 60_000;
const CONNECT_SCHEDULE_MS: [u64; 6] = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
const CONNECT_MAX_ATTEMPTS: u32 = 8;
const FAIL_SCHEDULE_MS: [u64; 7] = [5_000, 10_000, 20_000, 40_000, 60_000, 120_000, 300_000];
const BUSY_UNRESPONSIVE_AFTER: u32 = 3;
const ABANDONED_RETRY_MS: u64 = 300_000;
const MIN_REFRESH_GAP_MS: u64 = 2_000;
/// Wall clock running this far ahead of the monotonic clock means the PC slept.
const RESUME_SLACK_MS: i64 = 20_000;
pub const WATCHDOG_MS: u64 = 10_000;
/// Abandoned (hung) workers tolerated before the machine settles on `failed`.
pub const MAX_ABANDONED: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Cheap process-list check; decides whether an attach is worth trying.
    Discover,
    /// Attach (if needed), read the calendar, release.
    Fetch,
}

#[derive(Clone, Copy, Debug)]
pub struct Now {
    pub mono_ms: u64,
    pub unix_ms: i64,
    /// Local calendar day (days from CE), to resync at midnight.
    pub day: i32,
}

pub struct Machine {
    status: CalendarStatus,
    error_code: Option<&'static str>,
    events: Vec<CalendarEventDto>,
    invites: Vec<MeetingInviteDto>,
    last_sync_unix_ms: Option<i64>,
    next_retry_unix_ms: Option<i64>,
    action: Action,
    at: u64,
    /// No fetch before this instant (sync interval or backoff).
    fetch_not_before: u64,
    last_fetch_at: Option<u64>,
    connect_attempts: u32,
    fail_attempts: u32,
    busy_streak: u32,
    abandoned: u32,
    synced_day: Option<i32>,
    /// The Outlook process the current attempts and backoff belong to.
    outlook_pid: Option<u32>,
    last_tick: Option<(u64, i64)>,
    /// xorshift state for backoff jitter; `None` disables jitter (tests).
    rng: Option<u64>,
}

impl Machine {
    pub fn new(jitter_seed: Option<u64>) -> Self {
        Machine {
            status: CalendarStatus::Waiting,
            error_code: None,
            events: Vec::new(),
            invites: Vec::new(),
            last_sync_unix_ms: None,
            next_retry_unix_ms: None,
            action: Action::Discover,
            at: 0,
            fetch_not_before: 0,
            last_fetch_at: None,
            connect_attempts: 0,
            fail_attempts: 0,
            busy_streak: 0,
            abandoned: 0,
            synced_day: None,
            outlook_pid: None,
            last_tick: None,
            rng: jitter_seed.map(|s| s | 1),
        }
    }

    /// The next thing to do and the monotonic instant it is due.
    pub fn next(&self) -> (Action, u64) {
        (self.action, self.at)
    }

    pub fn status(&self) -> CalendarStatus {
        self.status
    }

    pub fn error_code(&self) -> Option<&'static str> {
        self.error_code
    }

    pub fn abandoned(&self) -> u32 {
        self.abandoned
    }

    pub fn snapshot(&self, now_unix_ms: i64) -> CalendarSnapshot {
        let events: Vec<CalendarEventDto> =
            self.events.iter().filter(|e| e.end_utc.timestamp_millis() > now_unix_ms).cloned().collect();
        CalendarSnapshot {
            status: self.status,
            error_code: self.error_code.map(str::to_string),
            last_sync_unix_ms: self.last_sync_unix_ms,
            cached_count: events.len(),
            next_retry_unix_ms: self.next_retry_unix_ms,
            events,
            invites: self.invites.clone(),
        }
    }

    /// The invites of the latest successful read (replaced as a whole, like events).
    pub fn set_invites(&mut self, mut invites: Vec<MeetingInviteDto>) {
        invites.truncate(MAX_INVITES);
        self.invites = invites;
    }

    fn jittered(&mut self, base_ms: u64) -> u64 {
        let Some(state) = self.rng.as_mut() else { return base_ms };
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        // +-20 %
        base_ms * (800 + x % 401) / 1000
    }

    fn fail_delay(&mut self) -> u64 {
        let idx = (self.fail_attempts.max(1) as usize - 1).min(FAIL_SCHEDULE_MS.len() - 1);
        self.jittered(FAIL_SCHEDULE_MS[idx])
    }

    fn connect_delay(&self) -> u64 {
        let idx = (self.connect_attempts.max(1) as usize - 1).min(CONNECT_SCHEDULE_MS.len() - 1);
        CONNECT_SCHEDULE_MS[idx]
    }

    /// Poll the process list soon, but never past the fetch gate.
    fn schedule_poll(&mut self, now: &Now) {
        self.action = Action::Discover;
        self.at = self.fetch_not_before.min(now.mono_ms + DISCOVER_POLL_MS).max(now.mono_ms);
    }

    fn retry_in(&mut self, now: &Now, delay_ms: u64) {
        self.fetch_not_before = now.mono_ms + delay_ms;
        self.next_retry_unix_ms = Some(now.unix_ms + delay_ms as i64);
        self.schedule_poll(now);
    }

    fn set_idle(&mut self, now: &Now, status: CalendarStatus, code: &'static str) {
        self.status = status;
        self.error_code = Some(code);
        self.next_retry_unix_ms = None;
        self.connect_attempts = 0;
        self.fail_attempts = 0;
        self.busy_streak = 0;
        // A different Outlook process is a fresh chance for a previously hung one.
        self.abandoned = 0;
        self.outlook_pid = None;
        self.fetch_not_before = now.mono_ms;
        self.action = Action::Discover;
        self.at = now.mono_ms + DISCOVER_POLL_MS;
    }

    fn fail(&mut self, now: &Now, status: CalendarStatus, code: &'static str, delay_ms: u64) {
        self.status = status;
        self.error_code = Some(code);
        self.retry_in(now, delay_ms);
    }

    /// Call before choosing the next action. Detects resume-from-sleep (wall clock ran ahead
    /// of the monotonic one) and the local date change. True when a resume was detected.
    pub fn on_tick(&mut self, now: &Now) -> bool {
        let resumed = self
            .last_tick
            .is_some_and(|(mono, unix)| (now.unix_ms - unix) - (now.mono_ms.saturating_sub(mono) as i64) > RESUME_SLACK_MS);
        self.last_tick = Some((now.mono_ms, now.unix_ms));
        if resumed {
            self.connect_attempts = 0;
            self.fail_attempts = 0;
            self.busy_streak = 0;
            self.fetch_not_before = now.mono_ms;
            self.action = Action::Discover;
            self.at = now.mono_ms;
            if self.next_retry_unix_ms.is_some() {
                self.next_retry_unix_ms = Some(now.unix_ms);
            }
        } else if self.status == CalendarStatus::Connected && self.synced_day.is_some_and(|d| d != now.day) {
            // One-shot: if the read answers "busy" the status stays connected, and the
            // backoff must not be overridden again on every tick.
            self.synced_day = None;
            self.fetch_not_before = self.fetch_not_before.min(now.mono_ms);
            if self.action == Action::Discover {
                self.at = self.at.min(now.mono_ms);
            }
        }
        resumed
    }

    /// Explicit user refresh: try now (rate-limited), unless the machine has given up on
    /// a hung Outlook.
    pub fn on_refresh(&mut self, now: &Now) {
        if self.abandoned >= MAX_ABANDONED {
            return;
        }
        self.connect_attempts = 0;
        self.fail_attempts = 0;
        self.busy_streak = 0;
        let earliest = self.last_fetch_at.map_or(now.mono_ms, |t| t + MIN_REFRESH_GAP_MS).max(now.mono_ms);
        self.fetch_not_before = earliest;
        if self.next_retry_unix_ms.is_some() {
            self.next_retry_unix_ms = Some(now.unix_ms + (earliest - now.mono_ms) as i64);
        }
        self.action = Action::Discover;
        self.at = now.mono_ms;
    }

    pub fn on_discovery(&mut self, now: &Now, discovery: Discovery) {
        match discovery {
            Discovery::Waiting => self.set_idle(now, CalendarStatus::Waiting, "OUTLOOK-101"),
            Discovery::NewOutlookOnly => self.set_idle(now, CalendarStatus::NewOutlookOnly, "OUTLOOK-104"),
            Discovery::ElevationMismatch => self.set_idle(now, CalendarStatus::ElevationMismatch, "OUTLOOK-103"),
            Discovery::Classic(pid) => {
                if self.outlook_pid.is_some_and(|known| known != pid) {
                    // Outlook was restarted: whatever the old process earned (backoff,
                    // hung-worker verdict) does not apply to the new one.
                    self.connect_attempts = 0;
                    self.fail_attempts = 0;
                    self.busy_streak = 0;
                    self.abandoned = 0;
                    self.fetch_not_before = now.mono_ms;
                    if matches!(self.status, CalendarStatus::Failed | CalendarStatus::Unresponsive) {
                        self.status = CalendarStatus::Connecting;
                        self.error_code = None;
                        self.next_retry_unix_ms = None;
                    }
                }
                self.outlook_pid = Some(pid);
                if matches!(
                    self.status,
                    CalendarStatus::Waiting | CalendarStatus::NewOutlookOnly | CalendarStatus::ElevationMismatch
                ) {
                    self.status = CalendarStatus::Connecting;
                    self.error_code = None;
                    self.next_retry_unix_ms = None;
                }
                if now.mono_ms >= self.fetch_not_before {
                    self.action = Action::Fetch;
                    self.at = now.mono_ms;
                } else {
                    self.schedule_poll(now);
                }
            }
        }
    }

    /// The process list could not be read (or our own identity could not be established).
    pub fn on_discovery_error(&mut self, now: &Now) {
        self.fail_attempts += 1;
        let delay = self.fail_delay();
        self.fail(now, CalendarStatus::Failed, "OUTLOOK-102", delay);
    }

    pub fn on_fetch_started(&mut self, now: &Now) {
        self.last_fetch_at = Some(now.mono_ms);
    }

    pub fn on_fetch_ok(&mut self, now: &Now, events: Vec<CalendarEventDto>) {
        self.events = normalize_events(events);
        self.status = CalendarStatus::Connected;
        self.error_code = None;
        self.next_retry_unix_ms = None;
        self.last_sync_unix_ms = Some(now.unix_ms);
        self.synced_day = Some(now.day);
        self.connect_attempts = 0;
        self.fail_attempts = 0;
        self.busy_streak = 0;
        self.abandoned = 0;
        self.fetch_not_before = now.mono_ms + SYNC_INTERVAL_MS;
        self.schedule_poll(now);
    }

    pub fn on_fetch_err(&mut self, now: &Now, err: &SourceError) {
        match err.kind {
            ErrKind::NotInRot | ErrKind::Disconnected => {
                self.connect_attempts += 1;
                if self.connect_attempts > CONNECT_MAX_ATTEMPTS {
                    self.fail_attempts += 1;
                    let delay = self.fail_delay();
                    self.fail(now, CalendarStatus::Failed, "OUTLOOK-102", delay);
                } else {
                    let delay = self.connect_delay();
                    self.status = CalendarStatus::Connecting;
                    self.error_code = None;
                    self.retry_in(now, delay);
                }
            }
            ErrKind::Busy => {
                self.busy_streak += 1;
                self.fail_attempts += 1;
                let delay = self.fail_delay();
                self.status = if self.busy_streak >= BUSY_UNRESPONSIVE_AFTER {
                    CalendarStatus::Unresponsive
                } else if self.status == CalendarStatus::Connected {
                    CalendarStatus::Connected
                } else {
                    CalendarStatus::Connecting
                };
                self.error_code = Some("OUTLOOK-105");
                self.retry_in(now, delay);
            }
            ErrKind::Blocked => {
                self.fail_attempts += 1;
                let delay = self.fail_delay();
                self.fail(now, CalendarStatus::Failed, "OUTLOOK-110", delay);
            }
            ErrKind::Failed => {
                self.fail_attempts += 1;
                let delay = self.fail_delay();
                self.fail(now, CalendarStatus::Failed, err.code, delay);
            }
        }
    }

    /// The watchdog abandoned a worker that did not answer in time.
    pub fn on_timeout(&mut self, now: &Now) {
        self.abandoned += 1;
        self.fail_attempts += 1;
        let mut delay = self.fail_delay();
        let status = if self.abandoned >= MAX_ABANDONED {
            delay = delay.max(ABANDONED_RETRY_MS);
            CalendarStatus::Failed
        } else {
            CalendarStatus::Unresponsive
        };
        self.fail(now, status, "OUTLOOK-109", delay);
    }
}

// =============================================================================
// Supervisor
// =============================================================================

enum Msg {
    Refresh,
    Shutdown,
    Reply { generation: u64, result: FetchResult },
    /// Read another stretch of the calendar (a day the user browses to) through the same worker.
    /// It changes nothing in the machine: the regular 48 h sync stays what reminders follow.
    Range { window: FetchWindow, reply: Sender<FetchResult> },
}

/// Longest stretch one `calendar_get_range` may read.
const MAX_RANGE_DAYS: i64 = 7;

struct Control {
    tx: Sender<Msg>,
    done: Receiver<()>,
}

static CONTROL: Mutex<Option<Control>> = Mutex::new(None);

/// Longest the supervisor sleeps in one go, so sleep/resume and midnight are noticed.
const MAX_IDLE_WAIT_MS: u64 = 30_000;

enum Outcome {
    Done(FetchResult),
    TimedOut,
    Shutdown,
}

struct Supervisor {
    app: AppHandle,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    machine: Machine,
    worker: Option<outlook::Worker>,
    generation: u64,
    started: Instant,
    last_logged: (CalendarStatus, Option<&'static str>),
    /// Range reads asked for while the worker was busy; served before the next scheduled step.
    pending_ranges: Vec<(FetchWindow, Sender<FetchResult>)>,
}

impl Supervisor {
    fn now(&self) -> Now {
        let local = Local::now();
        Now {
            mono_ms: self.started.elapsed().as_millis() as u64,
            unix_ms: local.timestamp_millis(),
            day: local.date_naive().num_days_from_ce(),
        }
    }

    fn publish(&mut self, now: &Now) {
        let snapshot = self.machine.snapshot(now.unix_ms);
        let state = self.app.state::<CalendarState>();
        if state.replace_if_changed(&snapshot) {
            let _ = self.app.emit("calendar-snapshot", &snapshot);
        }
        calendar_diag::set(OutlookDiag {
            status: snapshot.status.as_str().into(),
            error_code: snapshot.error_code.clone(),
            mode: match snapshot.status {
                CalendarStatus::Waiting => "none",
                CalendarStatus::NewOutlookOnly => "new",
                _ => "classic",
            }
            .into(),
            last_sync_unix_ms: snapshot.last_sync_unix_ms,
            cached_count: snapshot.cached_count,
        });
        let logged = (self.machine.status(), self.machine.error_code());
        if logged != self.last_logged {
            dlog!(
                "INFO",
                "calendar",
                "status {} -> {} code={} events={}",
                self.last_logged.0.as_str(),
                logged.0.as_str(),
                logged.1.unwrap_or("-"),
                snapshot.cached_count
            );
            self.last_logged = logged;
        }
    }

    fn drop_worker(&mut self, graceful: bool) {
        if let Some(worker) = self.worker.take() {
            if graceful {
                worker.shutdown();
            }
        }
    }

    fn discover(&mut self) {
        let now = self.now();
        match outlook::discover() {
            Ok(d) => {
                if !matches!(d, Discovery::Classic(_)) {
                    // Nothing to read: let the STA worker (and its apartment) go.
                    self.drop_worker(true);
                }
                self.machine.on_discovery(&now, d);
            }
            Err(e) => {
                dlog!("WARN", "calendar", "OUTLOOK-102 process discovery failed: {}", e);
                self.machine.on_discovery_error(&now);
            }
        }
    }

    fn reply_fn(&self, generation: u64) -> outlook::ReplyFn {
        let tx = self.tx.clone();
        Box::new(move |result| {
            let _ = tx.send(Msg::Reply { generation, result });
        })
    }

    /// Hand the window to the worker and wait up to the watchdog limit for its answer.
    fn run_fetch(&mut self, window: FetchWindow) -> Outcome {
        if self.worker.is_none() {
            self.generation += 1;
            let generation = self.generation;
            match outlook::Worker::spawn_outlook(generation, self.reply_fn(generation)) {
                Ok(w) => self.worker = Some(w),
                Err(e) => {
                    return Outcome::Done(Err(SourceError::new(ErrKind::Failed, "OUTLOOK-102", format!("worker spawn failed: {e}"))));
                }
            }
        }
        let generation = self.generation;
        if !self.worker.as_ref().is_some_and(|w| w.fetch(window)) {
            self.worker = None;
            return Outcome::Done(Err(SourceError::new(ErrKind::Disconnected, "OUTLOOK-102", "worker is gone")));
        }
        let deadline = Instant::now() + Duration::from_millis(WATCHDOG_MS);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(remaining) {
                Ok(Msg::Reply { generation: g, result }) if g == generation => return Outcome::Done(result),
                Ok(Msg::Reply { .. }) => {}
                Ok(Msg::Refresh) => {
                    // Already reading; the refresh is satisfied by this read.
                }
                Ok(Msg::Range { window, reply }) => self.pending_ranges.push((window, reply)),
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => return Outcome::Shutdown,
                Err(RecvTimeoutError::Timeout) => return Outcome::TimedOut,
            }
        }
    }

    /// Serve one range read. Only while connected: otherwise the answer is the current status code.
    fn fetch_range(&mut self, window: FetchWindow, reply: Sender<FetchResult>) -> bool {
        if self.machine.status() != CalendarStatus::Connected {
            let code = self.machine.error_code().unwrap_or("OUTLOOK-101");
            let _ = reply.send(Err(SourceError::new(ErrKind::Failed, code, "calendar not connected")));
            return true;
        }
        match self.run_fetch(window) {
            Outcome::Shutdown => return false,
            Outcome::Done(result) => {
                let _ = reply.send(result);
            }
            Outcome::TimedOut => {
                // Same as a hung sync: abandon that worker, the machine counts it.
                self.drop_worker(false);
                let now = self.now();
                self.machine.on_timeout(&now);
                dlog!("ERROR", "calendar", "OUTLOOK-109 watchdog: range read did not answer in {}ms", WATCHDOG_MS);
                let _ = reply.send(Err(SourceError::new(ErrKind::Busy, "OUTLOOK-109", "watchdog")));
            }
        }
        true
    }

    fn fetch(&mut self) -> bool {
        let started = self.now();
        self.machine.on_fetch_started(&started);
        let invites = self.app.state::<SettingsStore>().get().meeting_invites_enabled;
        let outcome = self.run_fetch(FetchWindow { invites, ..FetchWindow::starting_at(Utc::now()) });
        let now = self.now();
        match outcome {
            Outcome::Shutdown => return false,
            Outcome::Done(Ok(fetched)) => {
                dlog!(
                    "DEBUG",
                    "calendar",
                    "sync ok: {} events, {} invites in {}ms",
                    fetched.events.len(),
                    fetched.invites.len(),
                    now.mono_ms.saturating_sub(started.mono_ms)
                );
                self.machine.on_fetch_ok(&now, fetched.events);
                self.machine.set_invites(fetched.invites);
            }
            Outcome::Done(Err(err)) => {
                dlog!("WARN", "calendar", "{} read failed ({:?}): {}", err.code, err.kind, err.detail);
                self.machine.on_fetch_err(&now, &err);
            }
            Outcome::TimedOut => {
                // A hung STA call cannot be cancelled: leave that thread (and its COM
                // references) alone and use a fresh worker for the next attempt.
                self.drop_worker(false);
                self.machine.on_timeout(&now);
                dlog!(
                    "ERROR",
                    "calendar",
                    "OUTLOOK-109 watchdog: no answer in {}ms, abandoned worker generation {} (abandoned: {})",
                    WATCHDOG_MS,
                    self.generation,
                    self.machine.abandoned()
                );
            }
        }
        true
    }

    /// One scheduling step. False when the supervisor should stop.
    fn step(&mut self) -> bool {
        let now = self.now();
        if self.machine.on_tick(&now) {
            dlog!("INFO", "calendar", "resume from sleep detected, rechecking now");
        }
        self.publish(&now);
        if !self.pending_ranges.is_empty() {
            let (window, reply) = self.pending_ranges.remove(0);
            return self.fetch_range(window, reply);
        }
        let (action, at) = self.machine.next();
        if at > now.mono_ms {
            let wait = (at - now.mono_ms).min(MAX_IDLE_WAIT_MS);
            return match self.rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(Msg::Refresh) => {
                    let now = self.now();
                    self.machine.on_refresh(&now);
                    true
                }
                Ok(Msg::Range { window, reply }) => {
                    self.pending_ranges.push((window, reply));
                    true
                }
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => false,
                Ok(Msg::Reply { .. }) | Err(RecvTimeoutError::Timeout) => true,
            };
        }
        match action {
            Action::Discover => {
                self.discover();
                true
            }
            Action::Fetch => self.fetch(),
        }
    }

    fn run(&mut self) {
        loop {
            match crate::debug_log::catch("calendar", || self.step()) {
                Some(true) => {}
                Some(false) => break,
                None => {
                    let now = self.now();
                    self.drop_worker(false);
                    self.machine.on_discovery_error(&now);
                }
            }
        }
        self.drop_worker(true);
    }
}

/// Start the calendar service (idempotent).
pub fn start(app: AppHandle) {
    let mut control = CONTROL.lock().unwrap_or_else(|e| e.into_inner());
    if control.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let mut supervisor = Supervisor {
        app,
        tx: tx.clone(),
        rx,
        machine: Machine::new(Some(Utc::now().timestamp_nanos_opt().unwrap_or(1) as u64)),
        worker: None,
        generation: 0,
        started: Instant::now(),
        last_logged: (CalendarStatus::Waiting, None),
        pending_ranges: Vec::new(),
    };
    let spawned = std::thread::Builder::new().name("companyisland-calendar".into()).spawn(move || {
        supervisor.run();
        let _ = done_tx.send(());
    });
    match spawned {
        Ok(_) => *control = Some(Control { tx, done: done_rx }),
        Err(e) => dlog!("ERROR", "calendar", "OUTLOOK-102 cannot start calendar thread: {}", e),
    }
}

/// Stop the service and give the worker a moment to release COM cleanly.
pub fn stop() {
    let control = CONTROL.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(c) = control {
        let _ = c.tx.send(Msg::Shutdown);
        let _ = c.done.recv_timeout(Duration::from_secs(2));
    }
}

#[tauri::command]
pub fn calendar_get_snapshot(state: State<'_, CalendarState>) -> CalendarSnapshot {
    state.get()
}

#[tauri::command]
pub fn calendar_refresh() -> Result<(), String> {
    if let Some(c) = CONTROL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = c.tx.send(Msg::Refresh);
    }
    Ok(())
}

/// `[from, to]` checked for a range read: both ISO instants, `to` after `from`, at most
/// [`MAX_RANGE_DAYS`] apart.
pub fn range_window(from_utc: &str, to_utc: &str) -> Result<FetchWindow, String> {
    let parse = |s: &str| {
        DateTime::parse_from_rfc3339(s).map(|t| t.with_timezone(&Utc)).map_err(|_| "OUTLOOK-108: invalid date".to_string())
    };
    let (from, to) = (parse(from_utc)?, parse(to_utc)?);
    if to <= from || to - from > ChronoDuration::days(MAX_RANGE_DAYS) {
        return Err("OUTLOOK-108: invalid range".into());
    }
    Ok(FetchWindow { from, to, invites: false })
}

/// The events of another stretch of the calendar (a day the user browses to), read on demand
/// through the same worker and watchdog as the regular sync. Nothing is cached in Rust.
#[tauri::command]
pub async fn calendar_get_range(from_utc: String, to_utc: String) -> Result<Vec<CalendarEventDto>, String> {
    let window = range_window(&from_utc, &to_utc)?;
    let (tx, rx) = mpsc::channel();
    {
        let control = CONTROL.lock().unwrap_or_else(|e| e.into_inner());
        let c = control.as_ref().ok_or_else(|| "OUTLOOK-102: calendar service not running".to_string())?;
        c.tx.send(Msg::Range { window, reply: tx }).map_err(|_| "OUTLOOK-102: calendar service not running".to_string())?;
    }
    crate::rt::run_blocking("calendar_get_range", move || {
        // A sync in progress, then this read: each has the watchdog's 10 s.
        match rx.recv_timeout(Duration::from_millis(WATCHDOG_MS * 2 + 5_000)) {
            Ok(Ok(fetched)) => Ok(fetched.events),
            Ok(Err(e)) => Err(format!("{}: range read failed", e.code)),
            Err(_) => Err("OUTLOOK-109: no answer".into()),
        }
    })
    .await
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    fn at(mono: u64) -> Now {
        Now { mono_ms: mono, unix_ms: 1_800_000_000_000 + mono as i64, day: 740_000 }
    }

    fn ev(id: &str, start_min: i64, len_min: i64) -> CalendarEventDto {
        let base = Utc.with_ymd_and_hms(2027, 1, 15, 8, 0, 0).unwrap();
        CalendarEventDto {
            id: id.into(),
            calendar_id: "cal".into(),
            subject: "s".into(),
            start_utc: base + ChronoDuration::minutes(start_min),
            end_utc: base + ChronoDuration::minutes(start_min + len_min),
            all_day: false,
            location: None,
            organizer: None,
            is_recurring: false,
            meeting_url: None,
            busy_status: BusyStatus::Busy,
            response_status: ResponseStatus::Accepted,
            color: None,
        }
    }

    fn at_utc(min: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 1, 15, 8, 0, 0).unwrap() + ChronoDuration::minutes(min)
    }

    #[test]
    fn range_reads_are_bounded_iso_windows() {
        let w = range_window("2026-10-11T21:00:00Z", "2026-10-12T21:00:00Z").unwrap();
        assert_eq!(w.to - w.from, ChronoDuration::hours(24));
        assert!(!w.invites);
        assert!(range_window("2026-10-11T21:00:00+03:00", "2026-10-18T21:00:00+03:00").is_ok());
        for (from, to) in [
            ("2026-10-12T00:00:00Z", "2026-10-11T00:00:00Z"),
            ("2026-10-12T00:00:00Z", "2026-10-12T00:00:00Z"),
            ("2026-10-01T00:00:00Z", "2026-10-09T00:00:00Z"),
            ("tomorrow", "2026-10-12T00:00:00Z"),
        ] {
            assert!(range_window(from, to).unwrap_err().starts_with("OUTLOOK-108"), "{from} {to}");
        }
    }

    #[test]
    fn invites_of_the_latest_read_reach_the_snapshot_capped() {
        let invite = |i: usize| MeetingInviteDto {
            id: format!("inv{i}"),
            subject: "s".into(),
            organizer: None,
            start_utc: None,
            end_utc: None,
            location: None,
            received_utc: at_utc(0),
        };
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 0, 30)]);
        m.set_invites((0..15).map(invite).collect());
        let snapshot = m.snapshot(0);
        assert_eq!(snapshot.invites.len(), MAX_INVITES);
        assert_eq!(snapshot.invites[0].id, "inv0");
        m.set_invites(Vec::new());
        assert!(m.snapshot(0).invites.is_empty());
    }

    fn err(kind: ErrKind, code: &'static str) -> SourceError {
        SourceError::new(kind, code, "test")
    }

    /// Scripted source for driving the machine without Outlook.
    struct FakeSource {
        script: VecDeque<FetchResult>,
        calls: u32,
    }

    impl CalendarSource for FakeSource {
        fn fetch(&mut self, _window: &FetchWindow) -> FetchResult {
            self.calls += 1;
            self.script.pop_front().unwrap_or_else(|| Ok(Fetched::default()))
        }
    }

    /// Runs the machine's own schedule against a fake source and a simulated clock.
    struct Sim {
        machine: Machine,
        source: FakeSource,
        discovery: Discovery,
        clock: u64,
        trace: Vec<(u64, Action)>,
    }

    impl Sim {
        fn new(script: Vec<FetchResult>) -> Self {
            Sim {
                machine: Machine::new(None),
                source: FakeSource { script: script.into(), calls: 0 },
                discovery: Discovery::Classic(1),
                clock: 0,
                trace: Vec::new(),
            }
        }

        /// Execute every action due at or before `until`.
        fn run_until(&mut self, until: u64) {
            for _ in 0..10_000 {
                let (action, due) = self.machine.next();
                if due > until {
                    self.clock = until;
                    return;
                }
                self.clock = self.clock.max(due);
                let now = at(self.clock);
                self.trace.push((self.clock, action));
                match action {
                    Action::Discover => self.machine.on_discovery(&now, self.discovery),
                    Action::Fetch => {
                        self.machine.on_fetch_started(&now);
                        match self.source.fetch(&FetchWindow::starting_at(Utc::now())) {
                            Ok(fetched) => self.machine.on_fetch_ok(&now, fetched.events),
                            Err(e) => self.machine.on_fetch_err(&now, &e),
                        }
                    }
                }
            }
            panic!("machine did not settle (tight loop?)");
        }

        fn fetch_times(&self) -> Vec<u64> {
            self.trace.iter().filter(|(_, a)| *a == Action::Fetch).map(|(t, _)| *t).collect()
        }
    }

    #[test]
    fn serializes_to_the_contract_shape() {
        let snapshot = CalendarSnapshot {
            status: CalendarStatus::NewOutlookOnly,
            error_code: Some("OUTLOOK-104".into()),
            last_sync_unix_ms: Some(5),
            cached_count: 1,
            next_retry_unix_ms: None,
            events: vec![CalendarEventDto {
                busy_status: BusyStatus::WorkingElsewhere,
                response_status: ResponseStatus::NotResponded,
                color: Some("#3267B8".into()),
                ..ev("a", 0, 30)
            }],
            invites: vec![MeetingInviteDto {
                id: "0123456789abcdef".into(),
                subject: "Review".into(),
                organizer: None,
                start_utc: Some(at_utc(60)),
                end_utc: None,
                location: None,
                received_utc: at_utc(0),
            }],
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["status"], "newOutlookOnly");
        assert_eq!(json["errorCode"], "OUTLOOK-104");
        assert_eq!(json["lastSyncUnixMs"], 5);
        assert_eq!(json["cachedCount"], 1);
        assert!(json["nextRetryUnixMs"].is_null());
        let e = &json["events"][0];
        assert_eq!(e["startUtc"], "2027-01-15T08:00:00Z");
        assert_eq!(e["endUtc"], "2027-01-15T08:30:00Z");
        assert_eq!(e["calendarId"], "cal");
        assert_eq!(e["allDay"], false);
        assert_eq!(e["isRecurring"], false);
        assert_eq!(e["busyStatus"], "workingElsewhere");
        assert_eq!(e["responseStatus"], "notResponded");
        assert!(e["meetingUrl"].is_null());
        assert_eq!(e["color"], "#3267B8");
        let i = &json["invites"][0];
        assert_eq!(i["startUtc"], "2027-01-15T09:00:00Z");
        assert!(i["endUtc"].is_null());
        assert_eq!(i["receivedUtc"], "2027-01-15T08:00:00Z");
        assert!(i.get("entryId").is_none());
        for s in [CalendarStatus::Waiting, CalendarStatus::ElevationMismatch, CalendarStatus::Unresponsive] {
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
        }
    }

    #[test]
    fn normalize_sorts_dedups_and_caps() {
        let out = normalize_events(vec![ev("b", 30, 10), ev("a", 10, 10), ev("a", 10, 10)]);
        assert_eq!(out.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        let many: Vec<_> = (0..80).map(|i| ev(&format!("e{i:03}"), i, 5)).collect();
        assert_eq!(normalize_events(many).len(), MAX_EVENTS);
    }

    #[test]
    fn snapshot_diffing_only_reports_real_changes() {
        let state = CalendarState::default();
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 0, 30)]);
        let s1 = m.snapshot(at(0).unix_ms);
        assert!(state.replace_if_changed(&s1));
        assert!(!state.replace_if_changed(&m.snapshot(at(0).unix_ms)));
        // Same events, same status: nothing to emit even though time passed.
        assert!(!state.replace_if_changed(&m.snapshot(at(5_000).unix_ms)));
        // A new sync timestamp is a change (contract: emitted after each successful sync).
        m.on_fetch_ok(&at(60_000), vec![ev("a", 0, 30)]);
        assert!(state.replace_if_changed(&m.snapshot(at(60_000).unix_ms)));
        // Events list changes.
        m.on_fetch_ok(&at(60_000), vec![ev("a", 0, 30), ev("b", 5, 5)]);
        assert!(state.replace_if_changed(&m.snapshot(at(60_000).unix_ms)));
        // Status change.
        m.on_discovery(&at(70_000), Discovery::Waiting);
        assert!(state.replace_if_changed(&m.snapshot(at(70_000).unix_ms)));
        assert_eq!(state.get().status, CalendarStatus::Waiting);
    }

    #[test]
    fn expired_events_leave_the_snapshot_but_stay_cached_until_next_sync() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("short", 0, 10), ev("long", 0, 600)]);
        let base = ev("x", 0, 0).start_utc.timestamp_millis();
        let snap = m.snapshot(base + 5 * 60_000);
        assert_eq!(snap.cached_count, 2);
        let snap = m.snapshot(base + 20 * 60_000);
        assert_eq!(snap.events.len(), 1);
        assert_eq!(snap.cached_count, 1);
        assert_eq!(snap.events[0].id, "long");
    }

    #[test]
    fn waiting_polls_every_15_seconds_and_never_fetches() {
        let mut sim = Sim::new(vec![]);
        sim.discovery = Discovery::Waiting;
        sim.run_until(60_000);
        assert!(sim.fetch_times().is_empty());
        let polls: Vec<u64> = sim.trace.iter().map(|(t, _)| *t).collect();
        assert_eq!(polls, [0, 15_000, 30_000, 45_000, 60_000]);
        assert_eq!(sim.machine.status(), CalendarStatus::Waiting);
        assert_eq!(sim.machine.error_code(), Some("OUTLOOK-101"));
    }

    #[test]
    fn idle_discoveries_map_to_contract_codes() {
        let mut m = Machine::new(None);
        m.on_discovery(&at(0), Discovery::NewOutlookOnly);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::NewOutlookOnly, Some("OUTLOOK-104")));
        m.on_discovery(&at(0), Discovery::ElevationMismatch);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::ElevationMismatch, Some("OUTLOOK-103")));
    }

    #[test]
    fn connected_resyncs_every_60_seconds_and_discovers_every_15() {
        let mut sim = Sim::new(vec![Ok(vec![ev("a", 0, 30)].into()), Ok(vec![ev("a", 0, 30)].into()), Ok(vec![].into())]);
        sim.run_until(125_000);
        assert_eq!(sim.fetch_times(), [0, 60_000, 120_000]);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
        let discovers = sim.trace.iter().filter(|(_, a)| *a == Action::Discover).count();
        assert!(discovers >= 8, "expected a process check every 15 s, got {discovers}");
    }

    #[test]
    fn outlook_closing_is_noticed_within_one_poll() {
        let mut sim = Sim::new(vec![Ok(vec![ev("a", 0, 30)].into())]);
        sim.run_until(10_000);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
        sim.discovery = Discovery::Waiting;
        sim.run_until(10_000 + DISCOVER_POLL_MS);
        assert_eq!(sim.machine.status(), CalendarStatus::Waiting);
        // The cache survives so reminders for already-known events keep working.
        assert_eq!(sim.machine.snapshot(at(0).unix_ms).events.len(), 1);
    }

    #[test]
    fn not_in_rot_retries_1_2_4_8_16_30_then_gives_up_to_failed() {
        let script = (0..20).map(|_| Err(err(ErrKind::NotInRot, "OUTLOOK-102"))).collect();
        let mut sim = Sim::new(script);
        sim.run_until(400_000);
        let t = sim.fetch_times();
        let gaps: Vec<u64> = t.windows(2).map(|w| w[1] - w[0]).take(9).collect();
        // 8 connecting retries (1,2,4,8,16,30,30,30 s); the 9th failure starts the failure schedule.
        assert_eq!(gaps[..8], [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000]);
        assert_eq!(gaps[8], 5_000);
        assert_eq!(sim.machine.status(), CalendarStatus::Failed);
        assert_eq!(sim.machine.error_code(), Some("OUTLOOK-102"));
    }

    #[test]
    fn failure_backoff_schedule_and_reset_on_success() {
        let mut script: Vec<FetchResult> = (0..9).map(|_| Err(err(ErrKind::Failed, "OUTLOOK-108"))).collect();
        script.push(Ok(vec![].into()));
        script.push(Err(err(ErrKind::Failed, "OUTLOOK-108")));
        let mut sim = Sim::new(script);
        sim.run_until(2_000_000);
        let t = sim.fetch_times();
        let gaps: Vec<u64> = t.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(gaps[..8], [5_000, 10_000, 20_000, 40_000, 60_000, 120_000, 300_000, 300_000]);
        // Fetch #10 succeeds, #11 (one sync interval later) fails: the backoff restarts at 5 s.
        assert_eq!(gaps[8..11], [300_000, 60_000, 5_000]);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
    }

    #[test]
    fn failure_counter_resets_after_success() {
        let mut m = Machine::new(None);
        for _ in 0..4 {
            m.on_fetch_err(&at(0), &err(ErrKind::Failed, "OUTLOOK-108"));
        }
        assert_eq!(m.fetch_not_before, 40_000);
        m.on_fetch_ok(&at(100_000), vec![]);
        m.on_fetch_err(&at(200_000), &err(ErrKind::Failed, "OUTLOOK-107"));
        assert_eq!(m.fetch_not_before, 205_000);
        assert_eq!(m.error_code(), Some("OUTLOOK-107"));
        assert_eq!(m.snapshot(0).next_retry_unix_ms, Some(at(200_000).unix_ms + 5_000));
    }

    #[test]
    fn jitter_stays_within_20_percent() {
        let mut m = Machine::new(Some(0x1234_5678));
        for _ in 0..200 {
            let d = m.jittered(10_000);
            assert!((8_000..=12_000).contains(&d), "{d}");
        }
    }

    #[test]
    fn busy_shows_unresponsive_only_when_it_persists() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![]);
        m.on_fetch_err(&at(1_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connected, Some("OUTLOOK-105")));
        m.on_fetch_err(&at(7_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!(m.status(), CalendarStatus::Connected);
        m.on_fetch_err(&at(20_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Unresponsive, Some("OUTLOOK-105")));
        m.on_fetch_ok(&at(40_000), vec![]);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connected, None));
    }

    #[test]
    fn blocked_is_failed_110() {
        let mut m = Machine::new(None);
        m.on_fetch_err(&at(0), &err(ErrKind::Blocked, "OUTLOOK-110"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Failed, Some("OUTLOOK-110")));
    }

    #[test]
    fn disconnect_reattaches_through_connecting() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![ev("a", 0, 30)]);
        m.on_fetch_err(&at(10_000), &err(ErrKind::Disconnected, "OUTLOOK-102"));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connecting, None));
        assert_eq!(m.fetch_not_before, 11_000);
        assert_eq!(m.snapshot(0).events.len(), 1);
    }

    #[test]
    fn watchdog_timeouts_escalate_then_settle_on_failed() {
        let mut m = Machine::new(None);
        for i in 1..MAX_ABANDONED {
            m.on_timeout(&at(i as u64 * 1_000));
            assert_eq!((m.status(), m.error_code()), (CalendarStatus::Unresponsive, Some("OUTLOOK-109")));
        }
        let now = at(100_000);
        m.on_timeout(&now);
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Failed, Some("OUTLOOK-109")));
        assert!(m.fetch_not_before >= now.mono_ms + ABANDONED_RETRY_MS);
        // A refresh must not hammer a hung Outlook.
        let before = m.fetch_not_before;
        m.on_refresh(&at(101_000));
        assert_eq!(m.fetch_not_before, before);
        // A new Outlook process (process list says waiting) clears the verdict.
        m.on_discovery(&at(102_000), Discovery::Waiting);
        assert_eq!(m.abandoned(), 0);
    }

    #[test]
    fn refresh_fetches_now_but_is_rate_limited() {
        let mut m = Machine::new(None);
        m.on_fetch_started(&at(0));
        m.on_fetch_ok(&at(0), vec![]);
        m.on_refresh(&at(500));
        m.on_discovery(&at(500), Discovery::Classic(1));
        // Within the 2 s gap the fetch waits for the gate.
        assert_eq!(m.next(), (Action::Discover, 2_000));
        m.on_discovery(&at(2_000), Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 2_000));
    }

    #[test]
    fn refresh_cuts_a_backoff_short() {
        let mut m = Machine::new(None);
        m.on_fetch_started(&at(0));
        for _ in 0..5 {
            m.on_fetch_err(&at(0), &err(ErrKind::Failed, "OUTLOOK-108"));
        }
        assert_eq!(m.fetch_not_before, 60_000);
        m.on_refresh(&at(10_000));
        assert_eq!(m.fetch_not_before, 10_000);
        m.on_discovery(&at(10_000), Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 10_000));
    }

    #[test]
    fn resume_from_sleep_rechecks_immediately() {
        let mut m = Machine::new(None);
        m.on_tick(&at(0));
        m.on_fetch_ok(&at(0), vec![]);
        m.on_fetch_err(&at(1_000), &err(ErrKind::Failed, "OUTLOOK-108"));
        assert!(m.fetch_not_before > 1_000);
        // 10 minutes of wall clock passed while the monotonic clock advanced 1 s.
        let woke = Now { mono_ms: 2_000, unix_ms: at(0).unix_ms + 600_000, day: 740_000 };
        assert!(m.on_tick(&woke));
        assert_eq!(m.next(), (Action::Discover, 2_000));
        assert_eq!(m.fetch_not_before, 2_000);
        // Ordinary ticks are not resumes.
        assert!(!m.on_tick(&Now { mono_ms: 17_000, unix_ms: woke.unix_ms + 15_000, day: 740_000 }));
    }

    #[test]
    fn local_date_change_triggers_a_resync() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![]);
        assert_eq!(m.next().1, 15_000);
        let next_day = Now { mono_ms: 5_000, unix_ms: at(5_000).unix_ms, day: 740_001 };
        m.on_tick(&next_day);
        assert_eq!(m.next(), (Action::Discover, 5_000));
        m.on_discovery(&next_day, Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 5_000));
    }

    #[test]
    fn midnight_resync_is_one_shot_so_a_busy_answer_keeps_its_backoff() {
        let mut m = Machine::new(None);
        m.on_fetch_ok(&at(0), vec![]);
        let next_day = |mono: u64| Now { mono_ms: mono, unix_ms: at(mono).unix_ms, day: 740_001 };
        m.on_tick(&next_day(5_000));
        m.on_discovery(&next_day(5_000), Discovery::Classic(1));
        assert_eq!(m.next(), (Action::Fetch, 5_000));
        // The read is rejected as busy: status stays connected, retry is scheduled.
        m.on_fetch_err(&next_day(5_000), &err(ErrKind::Busy, "OUTLOOK-105"));
        assert_eq!(m.status(), CalendarStatus::Connected);
        let gate = m.fetch_not_before;
        assert!(gate > 5_000);
        m.on_tick(&next_day(6_000));
        assert_eq!(m.fetch_not_before, gate);
        assert!(m.next().1 > 6_000);
    }

    #[test]
    fn a_restarted_outlook_clears_backoff_and_the_hung_verdict() {
        let mut m = Machine::new(None);
        m.on_discovery(&at(0), Discovery::Classic(10));
        for i in 0..MAX_ABANDONED {
            m.on_timeout(&at(1_000 * (i as u64 + 1)));
        }
        assert_eq!(m.status(), CalendarStatus::Failed);
        // Same process: the verdict stands and the gate holds.
        m.on_discovery(&at(20_000), Discovery::Classic(10));
        assert_eq!(m.status(), CalendarStatus::Failed);
        assert!(m.fetch_not_before > 20_000);
        // Quick restart (the process list never showed "waiting"): new pid, try again now.
        m.on_discovery(&at(20_000), Discovery::Classic(11));
        assert_eq!((m.status(), m.error_code()), (CalendarStatus::Connecting, None));
        assert_eq!(m.abandoned(), 0);
        assert_eq!(m.next(), (Action::Fetch, 20_000));
        m.on_refresh(&at(20_000));
        assert_eq!(m.fetch_not_before, 20_000);
    }

    #[test]
    fn simulated_run_with_fake_source_recovers_after_errors() {
        let mut sim = Sim::new(vec![
            Err(err(ErrKind::NotInRot, "OUTLOOK-102")),
            Err(err(ErrKind::Busy, "OUTLOOK-105")),
            Ok(vec![ev("a", 0, 30)].into()),
        ]);
        sim.run_until(40_000);
        assert_eq!(sim.machine.status(), CalendarStatus::Connected);
        assert_eq!(sim.source.calls, 3);
        assert_eq!(sim.machine.snapshot(0).events.len(), 1);
        assert!(sim.machine.snapshot(0).next_retry_unix_ms.is_none());
    }
}
