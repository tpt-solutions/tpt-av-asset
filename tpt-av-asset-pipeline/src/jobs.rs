//! Concrete cache/proxy jobs and crash-recovery rebuilding.

use std::path::PathBuf;

use tpt_av_asset_cache::{CacheStorage, ThumbnailCache, ThumbnailGenerator, WaveformCache, WaveformGenerator};
use tpt_av_asset_db::{AssetDb, CacheType, JobRecord};
use tpt_av_asset_proxy::{ProxyGenerator, ProxyProfile};
use tpt_av_asset_utils::{AssetError, AssetId, ProgressReporter};

use crate::job::{Job, JobId};

/// Field separator for persisted job payloads (unit separator: never
/// appears in sane paths).
const SEP: char = '\u{1}';

impl WaveformJob {
    /// Creates a waveform generation job.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: JobId,
        asset_id: AssetId,
        audio_path: PathBuf,
        chunk_size: u32,
        sample_rate: u32,
        storage: CacheStorage,
        db: AssetDb,
    ) -> Self {
        Self { job_id, asset_id, audio_path, chunk_size, sample_rate, storage, db }
    }
}

impl ThumbnailJob {
    /// Creates a thumbnail extraction job.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: JobId,
        asset_id: AssetId,
        video_path: PathBuf,
        interval_secs: f64,
        resolution: (u32, u32),
        storage: CacheStorage,
        db: AssetDb,
    ) -> Self {
        Self { job_id, asset_id, video_path, interval_secs, resolution, storage, db }
    }
}

impl VideoProxyJob {
    /// Creates a video proxy job.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: JobId,
        asset_id: AssetId,
        video_path: PathBuf,
        output: PathBuf,
        profile: ProxyProfile,
        db: AssetDb,
    ) -> Self {
        Self { job_id, asset_id, video_path, output, profile, db }
    }
}

impl AudioProxyJob {
    /// Creates an audio proxy job.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: JobId,
        asset_id: AssetId,
        audio_path: PathBuf,
        output: PathBuf,
        profile: ProxyProfile,
        db: AssetDb,
    ) -> Self {
        Self { job_id, asset_id, audio_path, output, profile, db }
    }
}

/// Generates waveform peaks for an audio asset.
pub struct WaveformJob {
    job_id: JobId,
    asset_id: AssetId,
    audio_path: PathBuf,
    chunk_size: u32,
    sample_rate: u32,
    storage: CacheStorage,
    db: AssetDb,
}

impl Job for WaveformJob {
    fn id(&self) -> JobId {
        self.job_id
    }

    fn priority(&self) -> tpt_av_asset_utils::Priority {
        tpt_av_asset_utils::Priority::High
    }

    fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    fn kind(&self) -> &'static str {
        "waveform"
    }

    fn payload(&self) -> String {
        format!(
            "{0}{sep}{1}{sep}{2}",
            self.audio_path.to_string_lossy(),
            self.chunk_size,
            self.sample_rate,
            sep = SEP
        )
    }

    fn execute(&mut self, progress: &ProgressReporter) -> Result<(), AssetError> {
        let mut cache = WaveformCache::open(self.asset_id, &self.storage)?;
        WaveformGenerator::new(self.chunk_size, self.sample_rate).generate(
            &self.audio_path,
            &mut cache,
            progress,
        )?;
        self.db
            .record_cache_entry(self.asset_id, CacheType::WaveformPeaks, cache.path())
    }
}

/// Extracts video thumbnails for a video asset.
pub struct ThumbnailJob {
    job_id: JobId,
    asset_id: AssetId,
    video_path: PathBuf,
    interval_secs: f64,
    resolution: (u32, u32),
    storage: CacheStorage,
    db: AssetDb,
}

impl Job for ThumbnailJob {
    fn id(&self) -> JobId {
        self.job_id
    }

    fn priority(&self) -> tpt_av_asset_utils::Priority {
        tpt_av_asset_utils::Priority::Normal
    }

    fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    fn kind(&self) -> &'static str {
        "thumbnails"
    }

    fn payload(&self) -> String {
        format!(
            "{0}{sep}{1}{sep}{2}{sep}{3}",
            self.video_path.to_string_lossy(),
            self.interval_secs,
            self.resolution.0,
            self.resolution.1,
            sep = SEP
        )
    }

    fn execute(&mut self, progress: &ProgressReporter) -> Result<(), AssetError> {
        let mut cache = ThumbnailCache::create(
            self.asset_id,
            &self.storage,
            self.interval_secs,
            self.resolution,
        )?;
        ThumbnailGenerator::new(self.interval_secs, self.resolution).generate(
            &self.video_path,
            &mut cache,
            progress,
        )?;
        let dir = self.storage.thumbnail_dir(self.asset_id);
        self.db
            .record_cache_entry(self.asset_id, CacheType::VideoThumbnails, &dir)
    }
}

/// Renders a video proxy.
pub struct VideoProxyJob {
    job_id: JobId,
    asset_id: AssetId,
    video_path: PathBuf,
    output: PathBuf,
    profile: ProxyProfile,
    db: AssetDb,
}

impl Job for VideoProxyJob {
    fn id(&self) -> JobId {
        self.job_id
    }

    fn priority(&self) -> tpt_av_asset_utils::Priority {
        tpt_av_asset_utils::Priority::Normal
    }

    fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    fn kind(&self) -> &'static str {
        "video_proxy"
    }

    fn payload(&self) -> String {
        format!(
            "{0}{sep}{1}{sep}{2}",
            self.video_path.to_string_lossy(),
            self.output.to_string_lossy(),
            self.profile.name,
            sep = SEP
        )
    }

    fn execute(&mut self, progress: &ProgressReporter) -> Result<(), AssetError> {
        ProxyGenerator::new(self.profile.clone()).generate_video_proxy(
            &self.video_path,
            &self.output,
            progress,
        )?;
        self.db
            .record_cache_entry(self.asset_id, CacheType::VideoProxy, &self.output)
    }
}

/// Renders an audio proxy.
pub struct AudioProxyJob {
    job_id: JobId,
    asset_id: AssetId,
    audio_path: PathBuf,
    output: PathBuf,
    profile: ProxyProfile,
    db: AssetDb,
}

impl Job for AudioProxyJob {
    fn id(&self) -> JobId {
        self.job_id
    }

    fn priority(&self) -> tpt_av_asset_utils::Priority {
        tpt_av_asset_utils::Priority::Normal
    }

    fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    fn kind(&self) -> &'static str {
        "audio_proxy"
    }

    fn payload(&self) -> String {
        format!(
            "{0}{sep}{1}{sep}{2}",
            self.audio_path.to_string_lossy(),
            self.output.to_string_lossy(),
            self.profile.name,
            sep = SEP
        )
    }

    fn execute(&mut self, progress: &ProgressReporter) -> Result<(), AssetError> {
        ProxyGenerator::new(self.profile.clone()).generate_audio_proxy(
            &self.audio_path,
            &self.output,
            progress,
        )?;
        self.db
            .record_cache_entry(self.asset_id, CacheType::AudioProxy, &self.output)
    }
}

/// Rebuilds a boxed [`Job`] from a persisted [`JobRecord`] — the crash
/// recovery path. The db/storage handles come from the recovery context,
/// not the record. Returns `None` for unknown kinds or corrupt payloads.
pub fn rebuild_job(
    record: &JobRecord,
    storage: &CacheStorage,
    db: &AssetDb,
) -> Option<Box<dyn Job>> {
    let fields: Vec<&str> = record.payload.split(SEP).collect();
    let job_id = JobId(record.job_id);
    let asset_id = record.asset_id;

    macro_rules! field {
        ($i:expr) => {
            fields.get($i).copied().unwrap_or_default()
        };
    }

    match record.kind.as_str() {
        "waveform" => Some(Box::new(WaveformJob::new(
            job_id,
            asset_id,
            PathBuf::from(field!(0)),
            field!(1).parse().ok()?,
            field!(2).parse().ok()?,
            storage.clone(),
            db.clone(),
        ))),
        "thumbnails" => Some(Box::new(ThumbnailJob::new(
            job_id,
            asset_id,
            PathBuf::from(field!(0)),
            field!(1).parse().ok()?,
            (field!(2).parse().ok()?, field!(3).parse().ok()?),
            storage.clone(),
            db.clone(),
        ))),
        "video_proxy" => Some(Box::new(VideoProxyJob::new(
            job_id,
            asset_id,
            PathBuf::from(field!(0)),
            PathBuf::from(field!(1)),
            ProxyProfile::proxy_1080p_low(),
            db.clone(),
        ))),
        "audio_proxy" => Some(Box::new(AudioProxyJob::new(
            job_id,
            asset_id,
            PathBuf::from(field!(0)),
            PathBuf::from(field!(1)),
            ProxyProfile::audio_proxy_flac(),
            db.clone(),
        ))),
        _ => None,
    }
}
