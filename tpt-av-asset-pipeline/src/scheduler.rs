//! Scheduling logic: job dependencies.
//!
//! The scheduler tracks `job depends on [jobs]` edges. A queued job becomes
//! ready only when all of its dependencies are `Completed`; a failed or
//! cancelled dependency blocks the dependent forever (it is cancelled by
//! the worker sweep rather than run).

use std::collections::{HashMap, HashSet};

use crate::job::JobId;

/// Dependency-aware readiness tracker.
#[derive(Debug, Default)]
pub struct Scheduler {
    deps: HashMap<JobId, Vec<JobId>>,
    completed: HashSet<JobId>,
    dead: HashSet<JobId>,
}

impl Scheduler {
    /// Creates an empty scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers dependency edges for a job. Calling again replaces the
    /// previous edges.
    pub fn register(&mut self, job_id: JobId, deps: Vec<JobId>) {
        self.deps.insert(job_id, deps);
    }

    /// Marks a dependency job completed.
    pub fn mark_completed(&mut self, job_id: JobId) {
        self.completed.insert(job_id);
    }

    /// Marks a dependency job failed or cancelled — its dependents are
    /// blocked forever.
    pub fn mark_dead(&mut self, job_id: JobId) {
        self.dead.insert(job_id);
        // Completion and death are mutually exclusive.
        self.completed.remove(&job_id);
    }

    /// True when every dependency of `job_id` has completed.
    pub fn is_ready(&self, job_id: JobId) -> bool {
        match self.deps.get(&job_id) {
            None => true,
            Some(deps) => deps.iter().all(|dep| self.completed.contains(dep)),
        }
    }

    /// True when at least one dependency failed or was cancelled — the job
    /// can never run.
    pub fn is_blocked_forever(&self, job_id: JobId) -> bool {
        match self.deps.get(&job_id) {
            None => false,
            Some(deps) => deps.iter().any(|dep| self.dead.contains(dep)),
        }
    }

    /// Removes all scheduling state for a job (post-terminal cleanup).
    pub fn forget(&mut self, job_id: JobId) {
        self.deps.remove(&job_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: JobId = JobId(1);
    const B: JobId = JobId(2);
    const C: JobId = JobId(3);

    #[test]
    fn readiness_follows_dependencies() {
        let mut scheduler = Scheduler::new();
        scheduler.register(C, vec![A, B]);

        assert!(!scheduler.is_ready(C));
        scheduler.mark_completed(A);
        assert!(!scheduler.is_ready(C));
        scheduler.mark_completed(B);
        assert!(scheduler.is_ready(C));
        // Independent job is always ready.
        assert!(scheduler.is_ready(B));
    }

    #[test]
    fn dead_dependency_blocks_dependents() {
        let mut scheduler = Scheduler::new();
        scheduler.register(C, vec![A, B]);
        scheduler.mark_completed(A);
        scheduler.mark_dead(B);
        assert!(scheduler.is_blocked_forever(C));
        assert!(!scheduler.is_ready(C));
    }

    #[test]
    fn re_registering_replaces_edges() {
        let mut scheduler = Scheduler::new();
        scheduler.register(C, vec![A]);
        scheduler.register(C, vec![B]);
        scheduler.mark_completed(A);
        assert!(!scheduler.is_ready(C), "A no longer a dependency");
        scheduler.mark_completed(B);
        assert!(scheduler.is_ready(C));
    }
}
