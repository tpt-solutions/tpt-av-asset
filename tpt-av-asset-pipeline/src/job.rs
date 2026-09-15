//! Job trait and types.

use tpt_av_asset_utils::{AssetError, AssetId, Priority, ProgressReporter};

/// Unique job identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// A background processing job.
///
/// `execute` receives a [`ProgressReporter`] that doubles as the
/// cancellation token: implementations must poll
/// [`ProgressReporter::check_cancelled`] inside their loops and abort with
/// [`AssetError::Cancelled`] when requested.
pub trait Job: Send {
    /// Returns the job ID.
    fn id(&self) -> JobId;

    /// Returns the job priority.
    fn priority(&self) -> Priority;

    /// Returns the asset ID this job is for.
    fn asset_id(&self) -> AssetId;

    /// Job kind tag (`"waveform"`, `"thumbnails"`, `"video_proxy"`,
    /// `"audio_proxy"`, ...). Persisted for crash recovery.
    fn kind(&self) -> &'static str;

    /// Parameters needed to rebuild this job after a crash. Job-kind
    /// specific encoding; kept opaque by the pipeline.
    fn payload(&self) -> String;

    /// Executes the job.
    ///
    /// # Errors
    /// Returns [`AssetError::Cancelled`] when cancelled through the
    /// progress reporter, or any other [`AssetError`] on failure.
    fn execute(
        &mut self,
        progress: &ProgressReporter,
    ) -> Result<(), AssetError>;
}
