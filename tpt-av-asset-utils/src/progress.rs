//! Progress reporting and cooperative cancellation.
//!
//! Every long-running generator (waveforms, thumbnails, proxies, pipeline
//! jobs) receives a [`ProgressReporter`]. The reporter carries two things:
//!
//! 1. an optional progress callback invoked with `0.0..=1.0` fractions, and
//! 2. a shared cancellation flag that generators must poll inside their
//!    loops via [`ProgressReporter::is_cancelled`] or
//!    [`ProgressReporter::check_cancelled`].
//!
//! Cloning a reporter shares the same underlying state, which is how the
//! pipeline cancels a running job from another thread.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::AssetError;

/// A progress update delivered to a [`ProgressReporter`] callback.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressEvent {
    /// Completion fraction in `0.0..=1.0`.
    pub fraction: f64,
    /// Optional human-readable status line.
    pub message: Option<Arc<str>>,
}

impl fmt::Display for ProgressEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(msg) => write!(f, "{:5.1}% — {msg}", self.fraction * 100.0),
            None => write!(f, "{:5.1}%", self.fraction * 100.0),
        }
    }
}

type Callback = Box<dyn Fn(ProgressEvent) + Send + Sync>;

struct Inner {
    cancelled: AtomicBool,
    // `Fn` callbacks are callable through a shared reference, so no lock is
    // needed: concurrent reporters can invoke the callback simultaneously,
    // and a callback may safely re-enter `report`.
    callback: Option<Callback>,
}

/// Progress callback + cancellation token, cheap to clone and share across
/// threads.
#[derive(Clone)]
pub struct ProgressReporter {
    inner: Arc<Inner>,
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                callback: None,
            }),
        }
    }
}

impl fmt::Debug for ProgressReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressReporter")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl ProgressReporter {
    /// Creates a reporter that silently ignores progress and is never
    /// cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a reporter invoking `callback` on every progress report.
    pub fn with_callback(callback: impl Fn(ProgressEvent) + Send + Sync + 'static) -> Self {
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                callback: Some(Box::new(callback)),
            }),
        }
    }

    /// Reports progress as a `0.0..=1.0` fraction (values outside the range
    /// or NaN are ignored).
    pub fn report(&self, fraction: f64) {
        if !(fraction.is_finite()) {
            return;
        }
        self.report_event(ProgressEvent {
            fraction: fraction.clamp(0.0, 1.0),
            message: None,
        });
    }

    /// Reports progress with a status message.
    pub fn report_with_message(&self, fraction: f64, message: impl Into<Arc<str>>) {
        if !(fraction.is_finite()) {
            return;
        }
        self.report_event(ProgressEvent {
            fraction: fraction.clamp(0.0, 1.0),
            message: Some(message.into()),
        });
    }

    /// Reports progress as `done / total` (total <= 0 is ignored).
    pub fn report_ratio(&self, done: f64, total: f64) {
        if total <= 0.0 {
            return;
        }
        self.report(done / total);
    }

    /// Requests cancellation. Generators observe this between work units
    /// and abort with [`AssetError::Cancelled`].
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
    }

    /// True once [`ProgressReporter::cancel`] has been called.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Returns [`AssetError::Cancelled`] if cancellation was requested —
    /// the idiomatic per-iteration check inside generator loops.
    ///
    /// # Errors
    /// Returns [`AssetError::Cancelled`] when cancelled.
    pub fn check_cancelled(&self) -> Result<(), AssetError> {
        if self.is_cancelled() {
            Err(AssetError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn report_event(&self, event: ProgressEvent) {
        if let Some(callback) = &self.inner.callback {
            callback(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    #[test]
    fn callback_receives_clamped_fractions() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let reporter = ProgressReporter::with_callback(move |e| {
            sink.lock().unwrap().push(e.fraction);
        });

        reporter.report(0.25);
        reporter.report(2.5); // clamped to 1.0
        reporter.report(-1.0); // clamped to 0.0
        reporter.report(f64::NAN); // ignored

        let seen = seen.lock().unwrap();
        assert_eq!(*seen, vec![0.25, 1.0, 0.0]);
    }

    #[test]
    fn ratio_reporting() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let reporter = ProgressReporter::with_callback(move |e| {
            if e.fraction >= 0.5 {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        reporter.report_ratio(1.0, 4.0);
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        reporter.report_ratio(2.0, 4.0);
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        reporter.report_ratio(1.0, 0.0); // ignored, no panic
    }

    #[test]
    fn cancellation_flows_through_clones() {
        let reporter = ProgressReporter::new();
        let clone = reporter.clone();
        assert!(!reporter.is_cancelled());
        assert!(clone.check_cancelled().is_ok());

        clone.cancel();
        assert!(reporter.is_cancelled());
        assert!(matches!(clone.check_cancelled(), Err(AssetError::Cancelled)));
    }

    #[test]
    fn reentrant_callback_is_safe() {
        // A callback that reports again must not deadlock.
        let reporter = ProgressReporter::with_callback(|_e| {});
        let deeper = reporter.clone();
        let reporter2 = ProgressReporter::with_callback(move |e| {
            if e.fraction < 0.5 {
                deeper.report(e.fraction + 0.5);
            }
        });
        reporter2.report(0.1); // would deadlock if the lock were held across callbacks
    }
}
