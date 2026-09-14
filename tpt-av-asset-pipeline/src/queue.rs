//! Prioritized job queue.
//!
//! Entries are kept sorted by `(priority descending, submission order)` —
//! Critical first, FIFO within a priority class. Popping consults a
//! readiness predicate so the scheduler can hold jobs back until their
//! dependencies have completed.

use std::collections::HashMap;

use tpt_av_asset_utils::Priority;

use crate::job::{Job, JobId};

struct QueueEntry {
    job: Box<dyn Job>,
    seq: u64,
}

/// Priority-sorted job queue.
#[derive(Default)]
pub struct JobQueue {
    entries: Vec<QueueEntry>,
    next_seq: u64,
}

impl JobQueue {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    fn entry_order(entry: &QueueEntry) -> (std::cmp::Reverse<Priority>, u64) {
        (std::cmp::Reverse(entry.job.priority()), entry.seq)
    }

    /// Enqueues a job, keeping priority order.
    pub fn push(&mut self, job: Box<dyn Job>) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let entry = QueueEntry { job, seq };
        let index = self
            .entries
            .partition_point(|existing| Self::entry_order(existing) <= Self::entry_order(&entry));
        self.entries.insert(index, entry);
    }

    /// Pops the highest-priority job whose readiness check passes. Jobs
    /// ahead of it that aren't ready stay queued.
    pub fn pop_ready(&mut self, mut is_ready: impl FnMut(JobId) -> bool) -> Option<Box<dyn Job>> {
        let index = self
            .entries
            .iter()
            .position(|entry| is_ready(entry.job.id()))?;
        Some(self.entries.remove(index).job)
    }

    /// Removes a queued job by id (for cancellation). Returns `None` if the
    /// job is not queued (it may be running or already finished).
    pub fn remove(&mut self, job_id: JobId) -> Option<Box<dyn Job>> {
        let index = self.entries.iter().position(|e| e.job.id() == job_id)?;
        Some(self.entries.remove(index).job)
    }

    /// All queued job ids in dispatch order.
    pub fn ids(&self) -> Vec<JobId> {
        self.entries.iter().map(|e| e.job.id()).collect()
    }

    /// Number of queued jobs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the queue holds no jobs.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Priorities of queued jobs in dispatch order (tests/diagnostics).
    pub fn priorities(&self) -> Vec<Priority> {
        self.entries.iter().map(|e| e.job.priority()).collect()
    }
}

/// Bookkeeping the queue itself doesn't own but tests may find handy:
/// a map from job id to queue position.
#[allow(dead_code)]
pub(crate) type QueueIndex = HashMap<JobId, usize>;

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_av_asset_utils::{AssetId, ProgressReporter};

    struct Dummy {
        id: JobId,
        priority: Priority,
    }

    impl Job for Dummy {
        fn id(&self) -> JobId {
            self.id
        }
        fn priority(&self) -> Priority {
            self.priority
        }
        fn asset_id(&self) -> AssetId {
            AssetId::from_parts(1, 2, 3)
        }
        fn kind(&self) -> &'static str {
            "dummy"
        }
        fn payload(&self) -> String {
            String::new()
        }
        fn execute(&mut self, _progress: &ProgressReporter) -> Result<(), AssetError> {
            Ok(())
        }
    }

    fn job(n: u64, priority: Priority) -> Box<dyn Job> {
        Box::new(Dummy { id: JobId(n), priority })
    }

    #[test]
    fn dispatch_order_is_priority_then_fifo() {
        let mut queue = JobQueue::new();
        queue.push(job(1, Priority::Normal));
        queue.push(job(2, Priority::Low));
        queue.push(job(3, Priority::Critical));
        queue.push(job(4, Priority::Normal));
        assert_eq!(queue.priorities(), vec![Priority::Critical, Priority::Normal, Priority::Normal, Priority::Low]);
        assert_eq!(
            queue.pop_ready(|_| true).unwrap().id(),
            JobId(3),
            "critical jumps the queue"
        );
        assert_eq!(queue.pop_ready(|_| true).unwrap().id(), JobId(1), "FIFO within priority");
        assert_eq!(queue.pop_ready(|_| true).unwrap().id(), JobId(4));
        assert_eq!(queue.pop_ready(|_| true).unwrap().id(), JobId(2));
        assert!(queue.is_empty());
    }

    #[test]
    fn pop_ready_skips_unready_jobs() {
        let mut queue = JobQueue::new();
        queue.push(job(1, Priority::Normal));
        queue.push(job(2, Priority::Normal));
        let popped = queue.pop_ready(|id| id != JobId(1));
        assert_eq!(popped.unwrap().id(), JobId(2), "skips job 1, takes job 2");
        assert_eq!(queue.ids(), vec![JobId(1)]);
    }

    #[test]
    fn remove_takes_queued_job_out() {
        let mut queue = JobQueue::new();
        queue.push(job(1, Priority::High));
        queue.push(job(2, Priority::Low));
        let removed = queue.remove(JobId(1)).unwrap();
        assert_eq!(removed.id(), JobId(1));
        assert!(queue.remove(JobId(99)).is_none());
        assert_eq!(queue.len(), 1);
    }
}
