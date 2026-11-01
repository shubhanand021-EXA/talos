//! Injection guard: cooperative cancellation + in-progress accounting.
//!
//! Every real input injection holds an [`InjectionToken`]. Dropping the
//! token releases the slot. The emergency stop latches a global stop flag
//! and reports how many injections were in flight, so the log line
//! `Emergency stop activated at [timestamp], [N] actions were in progress`
//! is exact. New injections are refused while the stop is latched.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Process-wide injection state (one per binary; the desktop shell and the
/// executor share it).
#[derive(Debug, Default)]
pub struct InjectionGuard {
    stopped: AtomicBool,
    in_progress: AtomicUsize,
}

impl InjectionGuard {
    /// Create a fresh guard (unstopped, idle).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin one injection; fails closed while the stop is latched.
    pub fn begin(&self) -> Result<InjectionToken<'_>, String> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err("emergency stop latched: injection refused".to_owned());
        }
        self.in_progress.fetch_add(1, Ordering::SeqCst);
        // Re-check after claiming the slot: a stop racing us still wins.
        if self.stopped.load(Ordering::SeqCst) {
            self.in_progress.fetch_sub(1, Ordering::SeqCst);
            return Err("emergency stop latched: injection refused".to_owned());
        }
        Ok(InjectionToken { guard: self })
    }

    /// Latch the stop and return the in-flight count at latch time.
    pub fn emergency_release(&self) -> usize {
        self.stopped.store(true, Ordering::SeqCst);
        self.in_progress.load(Ordering::SeqCst)
    }

    /// Clear the latch (explicit human reset only).
    pub fn reset(&self) {
        self.stopped.store(false, Ordering::SeqCst);
    }

    /// Whether the stop is latched.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Current in-flight injection count.
    #[must_use]
    pub fn in_progress(&self) -> usize {
        self.in_progress.load(Ordering::SeqCst)
    }
}

/// RAII slot for one injection; dropping it releases the count.
#[derive(Debug)]
pub struct InjectionToken<'a> {
    guard: &'a InjectionGuard,
}

impl Drop for InjectionToken<'_> {
    fn drop(&mut self) {
        self.guard.in_progress.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_counts_in_flight() {
        let guard = InjectionGuard::new();
        assert_eq!(guard.in_progress(), 0);
        let first = guard.begin().expect("first injection runs");
        let _second = guard.begin().expect("second injection runs");
        assert_eq!(guard.in_progress(), 2);
        drop(first);
        assert_eq!(guard.in_progress(), 1);
    }

    #[test]
    fn stop_refuses_new_injections_and_reports_count() {
        let guard = InjectionGuard::new();
        let _held = guard.begin().expect("injection runs");
        let in_flight = guard.emergency_release();
        assert_eq!(in_flight, 1);
        assert!(guard.begin().is_err());
        guard.reset();
        assert!(guard.begin().is_ok());
    }
}
