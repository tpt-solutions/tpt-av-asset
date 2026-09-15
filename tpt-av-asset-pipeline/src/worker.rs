//! Worker thread pool: the loop that turns queued jobs into completions.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tpt_av_asset_db::JobState;
use tpt_av_asset_utils::AssetError;

use crate::job::Job;
use crate::pipeline::Shared;

/// Worker main loop. Exits when the pipeline shuts down and the queue is
/// drained of runnable work.
pub(crate) fn run(shared: Arc<Shared>) {
    while !shared.shutdown.load(Ordering::Acquire) {
        match try_take(&shared) {
            Take::Job(job) => run_job(&shared, job),
            Take::Empty => {
                let queue = shared.queue.lock().expect("queue poisoned");
                if shared.shutdown.load(Ordering::Acquire) && queue.is_empty() {
                    return;
                }
                let _ = shared
                    .queue_signal
                    .wait_timeout(queue, crate::pipeline::pipeline_poll_interval())
                    .expect("queue mutex poisoned");
            }
            Take::Shutdown => return,
        }
    }
}

enum Take {
    Job(Box<dyn Job>),
    Empty,
    Shutdown,
}

/// Sweeps permanently blocked jobs out of the queue, then pops the first
/// ready job (highest priority first).
fn try_take(shared: &Arc<Shared>) -> Take {
    let mut queue = shared.queue.lock().expect("queue poisoned");
    let mut scheduler = shared.scheduler.lock().expect("scheduler poisoned");

    // Cancel jobs whose dependencies can never complete.
    let blocked: Vec<_> = queue
        .ids()
        .into_iter()
        .filter(|id| scheduler.is_blocked_forever(*id))
        .collect();
    for id in blocked {
        if queue.remove(id).is_some() {
            shared.tracker.update_state(id, JobState::Cancelled);
            scheduler.mark_dead(id);
            drop(queue);
            drop(scheduler);
            shared.persist(id, JobState::Cancelled, Some("dependency failed".into()));
            queue = shared.queue.lock().expect("queue poisoned");
            scheduler = shared.scheduler.lock().expect("scheduler poisoned");
        }
    }

    if shared.shutdown.load(Ordering::Acquire) && queue.is_empty() {
        return Take::Shutdown;
    }

    match queue.pop_ready(|id| scheduler.is_ready(id)) {
        Some(job) => Take::Job(job),
        None => Take::Empty,
    }
}

/// Executes one job and records its terminal state.
fn run_job(shared: &Arc<Shared>, mut job: Box<dyn Job>) {
    let id = job.id();
    shared.tracker.update_state(id, JobState::Running);
    shared.persist(id, JobState::Running, None);

    let reporter = shared.tracker.reporter_for(id);
    shared
        .running
        .lock()
        .expect("running poisoned")
        .insert(id.0, reporter.clone());

    // A panicking job must fail instead of taking the worker down.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.execute(&reporter)));

    shared.running.lock().expect("running poisoned").remove(&id.0);

    let (state, error) = match result {
        Ok(Ok(())) => (JobState::Completed, None),
        Ok(Err(AssetError::Cancelled)) => (JobState::Cancelled, Some("cancelled".to_string())),
        Ok(Err(e)) => (JobState::Failed, Some(e.to_string())),
        Err(_) => (JobState::Failed, Some("job panicked".to_string())),
    };
    shared.tracker.update_state(id, state);
    {
        let mut scheduler = shared.scheduler.lock().expect("scheduler poisoned");
        match state {
            JobState::Completed => scheduler.mark_completed(id),
            _ => scheduler.mark_dead(id),
        }
    }
    if let Some(message) = &error {
        log::debug!("pipeline: job {} finished with: {message}", id.0);
    }
    shared.persist(id, state, error);
    shared.queue_signal.notify_all();
}
