//! Progress tracking and reporting for pipeline jobs.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tpt_av_asset_db::JobState;
use tpt_av_asset_utils::{AssetError, ProgressReporter};

use crate::job::JobId;

/// A snapshot of a job's progress.
#[derive(Debug, Clone, PartialEq)]
pub struct JobProgress {
    /// Job this snapshot belongs to.
    pub job_id: JobId,
    /// Lifecycle state.
    pub state: JobState,
    /// Last reported completion fraction (`0.0..=1.0`).
    pub fraction: f64,
    /// Last reported status message.
    pub message: Option<String>,
    /// Last update (Unix ms).
    pub updated_ms: u64,
}

impl JobProgress {
    /// True once the job reached Completed/Failed/Cancelled.
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }
}

#[derive(Debug, Default)]
struct TrackerInner {
    jobs: Mutex<HashMap<u64, JobProgress>>,
    signal: Condvar,
}

/// Shared per-job progress registry.
///
/// The pipeline holds one tracker; workers push updates through
/// [`ProgressTracker::reporter_for`] reporters, waiters block on
/// [`ProgressTracker::wait_for`] until a job reaches a terminal state.
#[derive(Debug, Clone, Default)]
pub struct ProgressTracker {
    inner: Arc<TrackerInner>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ProgressTracker {
    /// Creates an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a job in the `Pending` state.
    pub fn register(&self, job_id: JobId) {
        let mut jobs = self.inner.jobs.lock().expect("tracker poisoned");
        jobs.insert(
            job_id.0,
            JobProgress {
                job_id,
                state: JobState::Pending,
                fraction: 0.0,
                message: None,
                updated_ms: now_ms(),
            },
        );
    }

    /// Updates the progress fraction/message without touching the state.
    pub fn update_fraction(&self, job_id: JobId, fraction: f64, message: Option<String>) {
        let mut jobs = self.inner.jobs.lock().expect("tracker poisoned");
        if let Some(progress) = jobs.get_mut(&job_id.0) {
            progress.fraction = fraction.clamp(0.0, 1.0);
            progress.message = message;
            progress.updated_ms = now_ms();
        }
        drop(jobs);
        self.inner.signal.notify_all();
    }

    /// Transitions a job's state (workers and cancellation paths).
    pub fn update_state(&self, job_id: JobId, state: JobState) {
        let mut jobs = self.inner.jobs.lock().expect("tracker poisoned");
        if let Some(progress) = jobs.get_mut(&job_id.0) {
            progress.state = state;
            progress.updated_ms = now_ms();
            if state.is_terminal() {
                progress.fraction = match state {
                    JobState::Completed => 1.0,
                    _ => progress.fraction,
                };
            }
        }
        drop(jobs);
        self.inner.signal.notify_all();
    }

    /// A reporter wired to this tracker: every progress report updates the
    /// job's fraction/message. The reporter's cancellation flag is private
    /// to the job run — the pipeline cancels through it.
    pub fn reporter_for(&self, job_id: JobId) -> ProgressReporter {
        let tracker = self.clone();
        ProgressReporter::with_callback(move |event| {
            tracker.update_fraction(job_id, event.fraction, event.message.map(|m| m.to_string()));
        })
    }

    /// Snapshot of one job's progress, if known.
    pub fn snapshot(&self, job_id: JobId) -> Option<JobProgress> {
        self.inner
            .jobs
            .lock()
            .expect("tracker poisoned")
            .get(&job_id.0)
            .cloned()
    }

    /// Snapshots for all tracked jobs.
    pub fn snapshots(&self) -> Vec<JobProgress> {
        self.inner
            .jobs
            .lock()
            .expect("tracker poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// Blocks until the job reaches a terminal state.
    ///
    /// # Errors
    /// Returns [`AssetError::JobNotFound`] for unknown jobs.
    pub fn wait_for(&self, job_id: JobId) -> Result<JobProgress, AssetError> {
        let mut jobs = self.inner.jobs.lock().expect("tracker poisoned");
        loop {
            match jobs.get(&job_id.0) {
                None => return Err(AssetError::JobNotFound(job_id.0)),
                Some(progress) if progress.is_terminal() => return Ok(progress.clone()),
                Some(_) => {
                    jobs = self
                        .inner
                        .signal
                        .wait(jobs)
                        .expect("tracker mutex poisoned");
                }
            }
        }
    }

    /// [`ProgressTracker::wait_for`] with a timeout; returns the last known
    /// snapshot on timeout.
    ///
    /// # Errors
    /// Returns [`AssetError::JobNotFound`] for unknown jobs.
    pub fn wait_for_timeout(
        &self,
        job_id: JobId,
        timeout: Duration,
    ) -> Result<JobProgress, AssetError> {
        let deadline = std::time::Instant::now() + timeout;
        let mut jobs = self.inner.jobs.lock().expect("tracker poisoned");
        loop {
            match jobs.get(&job_id.0) {
                None => return Err(AssetError::JobNotFound(job_id.0)),
                Some(progress) if progress.is_terminal() => return Ok(progress.clone()),
                Some(_) => {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Ok(jobs.get(&job_id.0).cloned().expect("checked above"));
                    }
                    let (guard, _timeout) = self
                        .inner
                        .signal
                        .wait_timeout(jobs, remaining)
                        .expect("tracker mutex poisoned");
                    jobs = guard;
                }
            }
        }
    }

    /// Blocks until every tracked job is terminal or the timeout elapses.
    /// Returns false on timeout.
    pub fn wait_all(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut jobs = self.inner.jobs.lock().expect("tracker poisoned");
        loop {
            if jobs.values().all(|p| p.is_terminal()) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (guard, _) = self
                .inner
                .signal
                .wait_timeout(jobs, remaining)
                .expect("tracker mutex poisoned");
            jobs = guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn lifecycle_and_reporting() {
        let tracker = ProgressTracker::new();
        let id = JobId(7);
        tracker.register(id);
        assert_eq!(tracker.snapshot(id).unwrap().state, JobState::Pending);

        let reporter = tracker.reporter_for(id);
        reporter.report(0.5);
        let snap = tracker.snapshot(id).unwrap();
        assert_eq!(snap.state, JobState::Pending);
        assert!((snap.fraction - 0.5).abs() < f64::EPSILON);

        tracker.update_state(id, JobState::Completed);
        let done = tracker.wait_for(id).unwrap();
        assert_eq!(done.state, JobState::Completed);
        assert!(
            (done.fraction - 1.0).abs() < f64::EPSILON,
            "completion saturates"
        );
    }

    #[test]
    fn wait_for_unknown_job_errors() {
        let tracker = ProgressTracker::new();
        assert!(matches!(
            tracker.wait_for(JobId(404)),
            Err(AssetError::JobNotFound(404))
        ));
    }

    #[test]
    fn wait_for_timeout_returns_snapshot() {
        let tracker = ProgressTracker::new();
        let id = JobId(9);
        tracker.register(id);
        let snap = tracker
            .wait_for_timeout(id, Duration::from_millis(50))
            .unwrap();
        assert_eq!(snap.state, JobState::Pending);
    }

    #[test]
    fn wait_all_waits_for_every_job() {
        let tracker = ProgressTracker::new();
        let a = JobId(1);
        let b = JobId(2);
        tracker.register(a);
        tracker.register(b);

        // Complete b from a callback thread-ish path (same thread is fine:
        // wait_all would deadlock, so complete before waiting).
        let reporter = tracker.reporter_for(a);
        reporter.report(1.0);
        tracker.update_state(a, JobState::Running);
        tracker.update_state(a, JobState::Completed);
        tracker.update_state(b, JobState::Cancelled);
        assert!(tracker.wait_all(Duration::from_secs(1)));

        let counter = Arc::new(AtomicUsize::new(0));
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}
