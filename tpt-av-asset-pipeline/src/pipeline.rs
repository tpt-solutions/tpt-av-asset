//! The high-level [`ProcessingPipeline`] API and shared worker state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tpt_av_asset_db::{AssetDb, JobRecord, JobState};
use tpt_av_asset_utils::{AssetError, AssetId, Priority, ProgressReporter};

use crate::job::JobId;
use crate::progress::ProgressTracker;
use crate::queue::JobQueue;
use crate::scheduler::Scheduler;
use crate::worker;

/// How often idle workers re-check the queue (also the dependency-wait
/// granularity).
const IDLE_POLL: Duration = Duration::from_millis(25);

/// Poll cadence for worker wait loops.
pub(crate) fn pipeline_poll_interval() -> Duration {
    IDLE_POLL
}

/// State shared between the pipeline handle and its worker threads.
#[derive(Default)]
pub(crate) struct Shared {
    pub queue: Mutex<JobQueue>,
    pub queue_signal: Condvar,
    pub shutdown: AtomicBool,
    pub tracker: ProgressTracker,
    pub scheduler: Mutex<Scheduler>,
    /// Reporters for in-flight jobs — cancellation flips their token.
    pub running: Mutex<HashMap<u64, ProgressReporter>>,
    /// Metadata captured at submit time, needed to persist job records on
    /// every state transition.
    pub submitted: Mutex<HashMap<u64, SubmittedMeta>>,
    pub db: Mutex<Option<AssetDb>>,
    pub next_job_id: AtomicU64,
}

/// Submit-time metadata for job persistence.
#[derive(Debug, Clone)]
pub(crate) struct SubmittedMeta {
    pub asset_id: AssetId,
    pub priority: Priority,
    pub kind: &'static str,
    pub payload: String,
    pub created_ms: u64,
}

impl Shared {
    pub fn persist(&self, job_id: JobId, state: JobState, error: Option<String>) {
        let db_guard = self.db.lock().expect("db slot poisoned");
        let Some(db) = db_guard.as_ref() else { return };
        let (meta, progress) = {
            let submitted = self.submitted.lock().expect("submitted poisoned");
            let Some(meta) = submitted.get(&job_id.0).cloned() else { return };
            let progress = self
                .tracker
                .snapshot(job_id)
                .expect("tracked at submit time");
            (meta, progress)
        };
        let record = JobRecord {
            job_id: job_id.0,
            asset_id: meta.asset_id,
            priority: meta.priority,
            state,
            kind: meta.kind.to_string(),
            progress: progress.fraction,
            resume_hint: 0,
            payload: meta.payload,
            created_ms: meta.created_ms,
            updated_ms: now_ms(),
            error,
        };
        if let Err(e) = db.upsert_job(&record) {
            log::warn!("pipeline: could not persist job {}: {e}", job_id.0);
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The background processing pipeline: a prioritized, dependency-aware job
/// queue executed by a worker thread pool, with progress tracking,
/// cancellation, and (optionally) crash-recovery persistence in
/// [`AssetDb`].
pub struct ProcessingPipeline {
    shared: Arc<Shared>,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
    num_workers: usize,
    started: AtomicBool,
}

impl Clone for ProcessingPipeline {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            workers: Mutex::new(Vec::new()),
            num_workers: self.num_workers,
            started: AtomicBool::new(self.started.load(Ordering::Acquire)),
        }
    }
}

impl ProcessingPipeline {
    /// Creates a pipeline with `num_workers` background threads (not yet
    /// started — see [`ProcessingPipeline::start`]).
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] when `num_workers` is zero.
    pub fn new(num_workers: usize) -> Result<Self, AssetError> {
        if num_workers == 0 {
            return Err(AssetError::validation("pipeline needs at least one worker"));
        }
        Ok(Self {
            shared: Arc::new(Shared::default()),
            workers: Mutex::new(Vec::new()),
            num_workers,
            started: AtomicBool::new(false),
        })
    }

    /// Creates a pipeline that persists job records into `db` for crash
    /// recovery.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] when `num_workers` is zero.
    pub fn with_db(num_workers: usize, db: AssetDb) -> Result<Self, AssetError> {
        let pipeline = Self::new(num_workers)?;
        *pipeline.shared.db.lock().expect("db slot poisoned") = Some(db);
        Ok(pipeline)
    }

    /// Attaches (or replaces) the job-persistence database.
    pub fn attach_db(&self, db: AssetDb) {
        *self.shared.db.lock().expect("db slot poisoned") = Some(db);
    }

    /// Submits a job to the pipeline.
    ///
    /// # Errors
    /// Returns [`AssetError::ChannelClosed`] only if the pipeline is
    /// shutting down.
    pub fn submit(&self, job: Box<dyn crate::job::Job>) -> Result<JobId, AssetError> {
        self.submit_with_deps(job, Vec::new())
    }

    /// Submits a job that must wait for `deps` to complete first.
    ///
    /// # Errors
    /// Returns [`AssetError::ChannelClosed`] only if the pipeline is
    /// shutting down.
    pub fn submit_with_deps(
        &self,
        job: Box<dyn crate::job::Job>,
        deps: Vec<JobId>,
    ) -> Result<JobId, AssetError> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(AssetError::ChannelClosed);
        }
        let id = job.id();
        self.shared.scheduler.lock().expect("scheduler poisoned").register(id, deps);
        self.shared.tracker.register(id);
        self.shared.submitted.lock().expect("submitted poisoned").insert(
            id.0,
            SubmittedMeta {
                asset_id: job.asset_id(),
                priority: job.priority(),
                kind: job.kind(),
                payload: job.payload(),
                created_ms: now_ms(),
            },
        );
        self.shared.persist(id, JobState::Pending, None);

        {
            let mut queue = self.shared.queue.lock().expect("queue poisoned");
            queue.push(job);
        }
        self.shared.queue_signal.notify_all();
        Ok(id)
    }

    /// Allocates a fresh job id (for callers constructing jobs lazily).
    pub fn next_job_id(&self) -> JobId {
        JobId(self.shared.next_job_id.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Cancels a job. Queued jobs are removed; running jobs are cancelled
    /// cooperatively (the job observes the token and aborts). Cancelling an
    /// already-terminal job is a no-op.
    ///
    /// # Errors
    /// Returns [`AssetError::JobNotFound`] for unknown job ids.
    pub fn cancel(&self, job_id: JobId) -> Result<(), AssetError> {
        if self.shared.tracker.snapshot(job_id).is_none() {
            return Err(AssetError::JobNotFound(job_id.0));
        }

        // Queued (not yet running): remove outright.
        {
            let mut queue = self.shared.queue.lock().expect("queue poisoned");
            if queue.remove(job_id).is_some() {
                drop(queue);
                self.shared.tracker.update_state(job_id, JobState::Cancelled);
                self.shared.scheduler.lock().expect("scheduler poisoned").mark_dead(job_id);
                self.shared.persist(job_id, JobState::Cancelled, None);
                return Ok(());
            }
        }

        // Running: flip the cooperative token; the worker finalizes.
        if let Some(reporter) = self.shared.running.lock().expect("running poisoned").get(&job_id.0) {
            reporter.cancel();
        }
        Ok(())
    }

    /// Returns the progress of a job.
    ///
    /// # Errors
    /// Returns [`AssetError::JobNotFound`] for unknown job ids.
    pub fn get_progress(&self, job_id: JobId) -> Result<Option<crate::progress::JobProgress>, AssetError> {
        match self.shared.tracker.snapshot(job_id) {
            Some(progress) => Ok(Some(progress)),
            None => Err(AssetError::JobNotFound(job_id.0)),
        }
    }

    /// Waits for a job to complete (blocking).
    ///
    /// # Errors
    /// Returns [`AssetError::JobNotFound`] for unknown job ids.
    pub fn wait_for(&self, job_id: JobId) -> Result<(), AssetError> {
        self.shared.tracker.wait_for(job_id)?;
        Ok(())
    }

    /// [`ProcessingPipeline::wait_for`] with a timeout; returns the last
    /// snapshot instead of blocking forever.
    ///
    /// # Errors
    /// Returns [`AssetError::JobNotFound`] for unknown job ids.
    pub fn wait_for_timeout(
        &self,
        job_id: JobId,
        timeout: Duration,
    ) -> Result<crate::progress::JobProgress, AssetError> {
        self.shared.tracker.wait_for_timeout(job_id, timeout)
    }

    /// Waits until every tracked job is terminal or the timeout elapses.
    /// Returns false on timeout.
    pub fn wait_for_all(&self, timeout: Duration) -> bool {
        self.shared.tracker.wait_all(timeout)
    }

    /// The tracker (for progress dashboards).
    pub fn tracker(&self) -> &ProgressTracker {
        &self.shared.tracker
    }

    /// Number of jobs waiting to run.
    pub fn queued_count(&self) -> usize {
        self.shared.queue.lock().expect("queue poisoned").len()
    }

    /// Starts the pipeline (spawns worker threads). Idempotent.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] if worker threads cannot spawn.
    pub fn start(&mut self) -> Result<(), AssetError> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let mut workers = self.workers.lock().expect("workers poisoned");
        for index in 0..self.num_workers {
            let shared = Arc::clone(&self.shared);
            let handle = std::thread::Builder::new()
                .name(format!("tpt-av-asset-worker-{index}"))
                .spawn(move || worker::run(shared))
                .map_err(|e| AssetError::validation(format!("failed to spawn worker: {e}")))?;
            workers.push(handle);
        }
        Ok(())
    }

    /// Stops the pipeline: signals shutdown, then joins the workers. Queued
    /// jobs remain queued (and persisted, if a db is attached) for later
    /// recovery. Idempotent.
    ///
    /// # Errors
    /// Never fails currently; the signature reserves room for future joins.
    pub fn stop(&mut self) -> Result<(), AssetError> {
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.queue_signal.notify_all();
        let mut workers = self.workers.lock().expect("workers poisoned");
        for handle in workers.drain(..) {
            let _ = handle.join();
        }
        Ok(())
    }

    /// Re-enqueues jobs persisted as Pending/Running by a previous pipeline
    /// (crash recovery). Returns how many jobs were re-enqueued. Old
    /// records are removed; the new submissions write fresh ones.
    ///
    /// # Errors
    /// Returns [`AssetError`] on database failure.
    pub fn recover_interrupted(
        &self,
        db: &AssetDb,
        storage: &tpt_av_asset_cache::CacheStorage,
    ) -> Result<usize, AssetError> {
        let mut recovered = 0;
        for state in [JobState::Pending, JobState::Running] {
            for record in db.jobs_in_state(state)? {
                let Some(job) = crate::jobs::rebuild_job(&record, storage, db) else {
                    log::warn!(
                        "recovery: cannot rebuild job {} of kind {}",
                        record.job_id,
                        record.kind
                    );
                    continue;
                };
                db.delete_job(record.job_id)?;
                self.submit(job)?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

}
