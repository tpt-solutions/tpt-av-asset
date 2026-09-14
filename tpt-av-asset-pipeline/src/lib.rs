//! Background processing orchestration for the TPT AV asset engine.
//!
//! The [`ProcessingPipeline`] runs prioritized, dependency-aware jobs on a
//! worker thread pool:
//!
//! - [`job::Job`] — the unit of work; `execute` polls the progress
//!   reporter's cancellation token between work units.
//! - [`queue::JobQueue`] — Critical-first, FIFO-within-priority dispatch.
//! - [`scheduler::Scheduler`] — inter-job dependencies; jobs whose deps
//!   fail are cancelled, never run.
//! - [`progress::ProgressTracker`] — per-job state/fraction snapshots with
//!   blocking waits.
//! - [`jobs`] — the concrete jobs (waveform, thumbnails, video/audio
//!   proxies) plus crash-recovery rebuilding from `AssetDb` records.
//! - [`importer::AssetImporter`] — probe → index → schedule → return
//!   immediately.
//!
//! Job records persist into the database's `jobs` table on every state
//! transition, so [`ProcessingPipeline::recover_interrupted`] can re-enqueue
//! unfinished work after a crash; the concrete generators resume from
//! whatever the caches already hold.

pub mod importer;
pub mod job;
pub mod jobs;
pub mod pipeline;
pub mod progress;
pub mod queue;
pub mod scheduler;
pub mod worker;

pub use importer::{probe_media_info, AssetImporter};
pub use job::{Job, JobId};
pub use jobs::{AudioProxyJob, ThumbnailJob, VideoProxyJob, WaveformJob};
pub use pipeline::ProcessingPipeline;
pub use progress::{JobProgress, ProgressTracker};
pub use queue::JobQueue;
pub use scheduler::Scheduler;
