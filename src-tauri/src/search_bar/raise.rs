//! When the AI button re-asserts HWND_TOPMOST after the shell raised the taskbar.
//!
//! Pressing the Windows key (or opening Start / the search flyout) makes Explorer re-raise the
//! taskbar above the other topmost windows, and it does so some milliseconds AFTER the foreground
//! event. A single check right after the event sees the old order and gives up, so the button stayed
//! behind the taskbar. A short burst of checks (0, 60, 200, 500 ms) catches the raise whenever it
//! lands; bursts are rate-limited so two topmost windows can never fight in a loop.

use std::time::{Duration, Instant};

/// Offsets of the checks after a trigger.
pub const BURST_MS: [u64; 4] = [0, 60, 200, 500];
/// A new trigger inside this long after the previous burst started is absorbed by that burst.
pub const MIN_BURST_GAP: Duration = Duration::from_millis(250);

#[derive(Debug, Default)]
pub struct RaiseSchedule {
    pending: Vec<Instant>,
    last_trigger: Option<Instant>,
}

impl RaiseSchedule {
    /// A shell surface came to the front (or the tray reordered). False when absorbed.
    pub fn trigger(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_trigger {
            if now.saturating_duration_since(last) < MIN_BURST_GAP {
                return false;
            }
        }
        self.last_trigger = Some(now);
        self.pending = BURST_MS.iter().map(|ms| now + Duration::from_millis(*ms)).collect();
        true
    }

    /// Consume the checks that are due; true when at least one was.
    pub fn take_due(&mut self, now: Instant) -> bool {
        let before = self.pending.len();
        self.pending.retain(|t| *t > now);
        self.pending.len() != before
    }

    /// Time until the next check (zero when overdue), None when nothing is pending.
    pub fn next_in(&self, now: Instant) -> Option<Duration> {
        self.pending.iter().map(|t| t.saturating_duration_since(now)).min()
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trigger_schedules_the_burst() {
        let t0 = Instant::now();
        let mut s = RaiseSchedule::default();
        assert!(s.is_idle());
        assert!(s.next_in(t0).is_none());
        assert!(s.trigger(t0));
        assert_eq!(s.next_in(t0), Some(Duration::ZERO));
        // the first check is due immediately, the rest later
        assert!(s.take_due(t0));
        assert!(!s.take_due(t0));
        assert_eq!(s.next_in(t0), Some(Duration::from_millis(60)));
        assert!(s.take_due(t0 + Duration::from_millis(65)));
        assert!(s.take_due(t0 + Duration::from_millis(210)));
        assert!(!s.is_idle());
        assert!(s.take_due(t0 + Duration::from_millis(600)));
        assert!(s.is_idle());
    }

    #[test]
    fn a_late_check_consumes_every_overdue_slot_at_once() {
        let t0 = Instant::now();
        let mut s = RaiseSchedule::default();
        s.trigger(t0);
        assert!(s.take_due(t0 + Duration::from_millis(1000)));
        assert!(s.is_idle());
    }

    #[test]
    fn bursts_are_rate_limited() {
        let t0 = Instant::now();
        let mut s = RaiseSchedule::default();
        assert!(s.trigger(t0));
        assert!(!s.trigger(t0 + Duration::from_millis(100)), "absorbed by the running burst");
        assert!(!s.trigger(t0 + Duration::from_millis(249)));
        assert!(s.trigger(t0 + Duration::from_millis(250)));
        // a flood of events yields at most one burst per gap
        let mut count = 0;
        let mut s = RaiseSchedule::default();
        for i in 0..1000u64 {
            if s.trigger(t0 + Duration::from_millis(i)) {
                count += 1;
            }
        }
        assert_eq!(count, 4, "1000 ms of events: one burst per 250 ms");
    }
}
