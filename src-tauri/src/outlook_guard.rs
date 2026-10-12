//! Outlook's object model guard: never wake it up, and never ask it twice.
//!
//! A few Outlook properties (the "protected properties", see Microsoft's list: `AppointmentItem.Organizer`,
//! `AppointmentItem.PropertyAccessor`, `Store.PropertyAccessor`, anything on a `Recipient`, ...) show the user a
//! security prompt ("A program is trying to access email address information stored in Outlook") when Outlook
//! does not trust the caller. Outlook trusts it only when the Windows Security Center reports a healthy antivirus
//! (or an administrator policy approves access). On a closed PC without a registered antivirus every guarded read
//! therefore opens a modal dialog in Outlook and the COM call **blocks until a person clicks**. Read on a timer
//! that is a prompt every few seconds, and the calendar watchdog (OUTLOOK-109) abandons a worker per prompt.
//!
//! The rule here: the features that need a guarded property are optional (organizer name, the Teams join link
//! read from the item's properties, the calendar colors, the free/busy of a named person). They run only when
//!
//! 1. the guard is predicted to be open ([`guard_open`]: the policy approves, or the Security Center says the
//!    antivirus is healthy), and
//! 2. they were not blocked, denied or stuck waiting for a click before ([`allowed`]). A stuck call is noticed by
//!    the next caller (a call in flight for longer than the feature's `stuck_after`) and the feature stays off for [`LATCH`], also
//!    across restarts, so the user is asked at most once and never by a timer.
//!
//! Everything else the island reads (subject, times, location, categories, busy status, folders) is not guarded.

use crate::com::{ComError, ComResult};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The optional features that touch a protected property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    /// `AppointmentItem.Organizer`.
    Organizer,
    /// `AppointmentItem.PropertyAccessor` (the Teams / Skype join link Outlook keeps in named properties).
    MeetingLink,
    /// `Store.PropertyAccessor` + `Columns.Add` on the navigation shortcuts (the pane's calendar colors).
    CalendarColors,
    /// `Recipient` (resolve a name, read its free/busy).
    FreeBusy,
    /// `Columns.Add` of the sender's name on a mail table.
    MailSender,
}

impl Feature {
    fn key(self) -> &'static str {
        match self {
            Feature::Organizer => "organizer",
            Feature::MeetingLink => "meeting-link",
            Feature::CalendarColors => "calendar-colors",
            Feature::FreeBusy => "free-busy",
            Feature::MailSender => "mail-sender",
        }
    }

    /// A call of this feature that takes longer was waiting for a click, not working.
    fn stuck_after(self) -> Duration {
        match self {
            Feature::Organizer | Feature::MeetingLink | Feature::MailSender => Duration::from_secs(2),
            // Opens the Common Views folder and a table: a little more than a property.
            Feature::CalendarColors => Duration::from_secs(4),
            // A network lookup of a colleague's free/busy is slow by itself.
            Feature::FreeBusy => Duration::from_secs(15),
        }
    }
}

/// How long a feature stays off after Outlook blocked it, denied it or made it wait for a person.
const LATCH: Duration = Duration::from_secs(6 * 3600);
/// The prediction is recomputed after this long (an antivirus can be installed or snoozed meanwhile).
const PREDICTION_TTL: Duration = Duration::from_secs(600);
const STATE_FILE: &str = "outlook_guard.json";

#[derive(Default)]
struct State {
    /// (when computed, guard open)
    prediction: Option<(Instant, bool)>,
    /// Off until (monotonic) and the wall-clock second that is persisted.
    latched: HashMap<&'static str, (Instant, u64)>,
    /// The start of the guarded call that is running (or hung) now.
    in_flight: HashMap<&'static str, Instant>,
    loaded: bool,
    logged_closed: bool,
}

static STATE: std::sync::LazyLock<Mutex<State>> = std::sync::LazyLock::new(|| Mutex::new(State::default()));

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// =============================================================================
// Will Outlook ask?
// =============================================================================

/// `PromptOOMAddressInformationAccess` in Outlook's security policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Prompt,
    Approve,
    Deny,
    /// 3, or not set: follows the antivirus status.
    AntivirusBased,
}

/// The pure decision: is a guarded property readable without a person clicking?
pub fn decide(policy: Policy, antivirus_healthy: Option<bool>) -> bool {
    match policy {
        Policy::Approve => true,
        Policy::Prompt | Policy::Deny => false,
        // Unknown (no Security Center, e.g. a server SKU) counts as not healthy: Outlook cannot see an antivirus either.
        Policy::AntivirusBased => antivirus_healthy == Some(true),
    }
}

/// Whether guarded reads are expected to go through silently on this PC (cached for [`PREDICTION_TTL`]).
pub fn guard_open() -> bool {
    {
        let s = state();
        if let Some((at, open)) = s.prediction {
            if at.elapsed() < PREDICTION_TTL {
                return open;
            }
        }
    }
    let policy = platform::policy();
    let av = platform::antivirus_healthy();
    let open = decide(policy, av);
    let mut s = state();
    s.prediction = Some((Instant::now(), open));
    if !open && !s.logged_closed {
        s.logged_closed = true;
        dlog!(
            "INFO",
            "outlook",
            "Outlook's object model guard would ask the user (policy {:?}, antivirus healthy {:?}): organizer names, the item join link, calendar colors and free/busy of named people stay off",
            policy,
            av
        );
    }
    open
}

// =============================================================================
// Latches
// =============================================================================

fn load_if_needed(s: &mut State) {
    if s.loaded {
        return;
    }
    s.loaded = true;
    if cfg!(test) {
        return;
    }
    let Ok(dir) = crate::paths::state_dir() else { return };
    let Ok(bytes) = std::fs::read(dir.join(STATE_FILE)) else { return };
    let Ok(saved) = serde_json::from_slice::<HashMap<String, u64>>(&bytes) else { return };
    let now = now_secs();
    for f in [Feature::Organizer, Feature::MeetingLink, Feature::CalendarColors, Feature::FreeBusy, Feature::MailSender] {
        if let Some(&until) = saved.get(f.key()).filter(|&&u| u > now) {
            s.latched.insert(f.key(), (Instant::now() + Duration::from_secs(until - now), until));
        }
    }
}

fn save(s: &State) {
    // Tests never touch the real profile.
    if cfg!(test) {
        return;
    }
    let Ok(dir) = crate::paths::state_dir() else { return };
    let now = now_secs();
    let saved: HashMap<&str, u64> = s.latched.iter().filter(|(_, (_, until))| *until > now).map(|(k, (_, until))| (*k, *until)).collect();
    if let Ok(bytes) = serde_json::to_vec(&saved) {
        let _ = crate::reminder_state::write_atomic(&dir.join(STATE_FILE), &bytes);
    }
}

fn latch(feature: Feature, why: &str) {
    let mut s = state();
    load_if_needed(&mut s);
    let fresh = !s.latched.get(feature.key()).is_some_and(|(until, _)| Instant::now() < *until);
    s.latched.insert(feature.key(), (Instant::now() + LATCH, now_secs() + LATCH.as_secs()));
    if fresh {
        dlog!("WARN", "outlook", "OUTLOOK-110 {} is off for {} h: {} (Outlook's object model guard)", feature.key(), LATCH.as_secs() / 3600, why);
        save(&s);
    }
}

/// Can `feature` be used now, without Outlook asking anyone? False also when an earlier call of it is still
/// waiting for a click: that call is the prompt, and nobody else may start another.
pub fn allowed(feature: Feature) -> bool {
    if !guard_open() {
        return false;
    }
    let mut s = state();
    load_if_needed(&mut s);
    if s.latched.get(feature.key()).is_some_and(|(until, _)| Instant::now() < *until) {
        return false;
    }
    let stuck = s.in_flight.get(feature.key()).is_some_and(|at| at.elapsed() >= feature.stuck_after());
    if stuck {
        drop(s);
        latch(feature, "a read is waiting for an answer");
        return false;
    }
    true
}

/// Outlook said no: E_ACCESSDENIED, a VBA-style runtime error, or E_ABORT ("operation aborted", what Deny returns).
pub fn is_denial(e: &ComError) -> bool {
    const E_ABORT: i32 = 0x8000_4004u32 as i32;
    e.is_blocked() || e.hr == E_ABORT || e.effective() == E_ABORT
}

/// A guarded read in progress. [`Probe::finish`] says how it went.
pub struct Probe {
    feature: Feature,
    started: Instant,
    done: bool,
}

/// Start a guarded read of `feature`; `None` when it must not run (see [`allowed`]).
pub fn begin(feature: Feature) -> Option<Probe> {
    if !allowed(feature) {
        return None;
    }
    state().in_flight.insert(feature.key(), Instant::now());
    Some(Probe { feature, started: Instant::now(), done: false })
}

impl Probe {
    /// The read returned. A denial, or an answer that took long enough to have waited for a click, turns the
    /// feature off; `true` when the result can be used.
    pub fn finish<T>(mut self, result: &ComResult<T>) -> bool {
        self.done = true;
        state().in_flight.remove(self.feature.key());
        match result {
            Err(e) if is_denial(e) => {
                latch(self.feature, &format!("denied ({e})"));
                false
            }
            Err(_) => false,
            Ok(_) => {
                if self.started.elapsed() >= self.feature.stuck_after() {
                    latch(self.feature, "the answer came only after a wait");
                }
                true
            }
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        // Dropped without an answer (a panic unwinding): not a prompt, just clear the marker.
        if !self.done {
            state().in_flight.remove(self.feature.key());
        }
    }
}

/// Run a guarded read. `Ok(None)`: not allowed, denied or off; `Err`: Outlook is busy or gone (the caller's
/// ordinary Outlook-level handling), `Ok(Some(v))`: the value.
pub fn guarded<T>(feature: Feature, read: impl FnOnce() -> ComResult<T>) -> ComResult<Option<T>> {
    let Some(probe) = begin(feature) else { return Ok(None) };
    let result = read();
    let hard_failure = matches!(&result, Err(e) if e.is_disconnected() || e.is_busy());
    let usable = probe.finish(&result);
    match result {
        Ok(v) if usable => Ok(Some(v)),
        Err(e) if hard_failure => Err(e),
        _ => Ok(None),
    }
}

#[cfg(test)]
pub fn reset_for_tests() -> std::sync::MutexGuard<'static, ()> {
    let serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    *state() = State { loaded: true, prediction: Some((Instant::now(), true)), ..State::default() };
    serial
}

#[cfg(test)]
static TEST_SERIAL: Mutex<()> = Mutex::new(());

// =============================================================================
// Windows: policy and Security Center
// =============================================================================

mod platform {
    use super::Policy;
    use std::ffi::c_void;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    use windows::Win32::System::SecurityCenter::{
        WscGetSecurityProviderHealth, WSC_SECURITY_PROVIDER_ANTIVIRUS, WSC_SECURITY_PROVIDER_HEALTH_GOOD,
    };

    fn dword(root: HKEY, subkey: &str, value: &str) -> Option<u32> {
        let subkey = HSTRING::from(subkey);
        let value = HSTRING::from(value);
        let mut data = 0u32;
        let mut bytes = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(root, &subkey, &value, RRF_RT_REG_DWORD, None, Some(&mut data as *mut u32 as *mut c_void), Some(&mut bytes))
        };
        (status == ERROR_SUCCESS).then_some(data)
    }

    /// Outlook's own security policy (HKCU wins over HKLM, as for Office policies). It counts only with
    /// `AdminSecurityMode` = 3 ("use the registry settings"), which is how Outlook reads these values.
    pub fn policy() -> Policy {
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            for version in ["16.0", "15.0", "14.0"] {
                let key = format!(r"Software\Policies\Microsoft\Office\{version}\Outlook\Security");
                if dword(root, &key, "AdminSecurityMode") != Some(3) {
                    continue;
                }
                match dword(root, &key, "PromptOOMAddressInformationAccess") {
                    Some(0) => return Policy::Prompt,
                    Some(1) => return Policy::Approve,
                    Some(2) => return Policy::Deny,
                    _ => {}
                }
            }
        }
        Policy::AntivirusBased
    }

    /// The Security Center's antivirus health: what Outlook looks at. `None` when it cannot be asked.
    pub fn antivirus_healthy() -> Option<bool> {
        let mut health = Default::default();
        unsafe { WscGetSecurityProviderHealth(WSC_SECURITY_PROVIDER_ANTIVIRUS.0 as u32, &mut health) }.ok()?;
        Some(health == WSC_SECURITY_PROVIDER_HEALTH_GOOD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_counts_as_open_only_when_outlook_will_not_ask() {
        assert!(decide(Policy::Approve, Some(false)));
        assert!(decide(Policy::AntivirusBased, Some(true)));
        assert!(!decide(Policy::AntivirusBased, Some(false)));
        assert!(!decide(Policy::AntivirusBased, None));
        assert!(!decide(Policy::Prompt, Some(true)));
        assert!(!decide(Policy::Deny, Some(true)));
    }

    #[test]
    fn a_denied_read_turns_the_feature_off_and_nobody_asks_again() {
        let _serial = reset_for_tests();
        let denied: ComResult<()> = Err(ComError::new("Organizer", 0x8000_4004u32 as i32));
        assert!(allowed(Feature::Organizer));
        assert_eq!(guarded(Feature::Organizer, || denied.clone()).unwrap(), None);
        assert!(!allowed(Feature::Organizer));
        assert!(allowed(Feature::MeetingLink), "the other features are not affected");
        let mut called = false;
        assert_eq!(guarded(Feature::Organizer, || { called = true; Ok(1) }).unwrap(), None);
        assert!(!called, "an off feature never reaches Outlook");
    }

    #[test]
    fn a_call_that_never_returns_is_noticed_by_the_next_caller() {
        let _serial = reset_for_tests();
        // The hung worker: marked in flight long ago and never finished.
        state().in_flight.insert(Feature::MeetingLink.key(), Instant::now() - Duration::from_secs(10));
        assert!(!allowed(Feature::MeetingLink), "the second worker must not open a second prompt");
        assert!(!allowed(Feature::MeetingLink));
    }

    #[test]
    fn a_fast_answer_keeps_the_feature_on() {
        let _serial = reset_for_tests();
        assert_eq!(guarded(Feature::CalendarColors, || Ok(7)).unwrap(), Some(7));
        assert!(allowed(Feature::CalendarColors));
    }

    #[test]
    fn busy_and_disconnected_outlook_stay_outlook_level_errors() {
        let _serial = reset_for_tests();
        let busy: ComResult<()> = Err(ComError::new("x", 0x8001_010Au32 as i32));
        assert!(guarded(Feature::Organizer, || busy.clone()).is_err());
        assert!(allowed(Feature::Organizer));
    }
}

#[cfg(test)]
mod live {
    /// `cargo test --lib prediction_on_this_pc -- --ignored --nocapture`: what the guard looks like on this PC.
    #[test]
    #[ignore]
    fn prediction_on_this_pc() {
        let policy = super::platform::policy();
        let av = super::platform::antivirus_healthy();
        println!("policy {:?}, antivirus healthy {:?}, guard open {}", policy, av, super::decide(policy, av));
    }
}
