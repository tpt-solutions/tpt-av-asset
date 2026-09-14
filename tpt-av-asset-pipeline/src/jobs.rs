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

/// Generates waveform peaks for an audio asset.
pub struct WaveformJob {
    pub(crate) job_id: JobId,
    pub(crate) asset_id: AssetId,
    pub(crate) audio_path: PathBuf,
    pub(crate) chunk_size: u32,
    pub(crate) sample_rate: u32,
    pub(crate) storage: CacheStorage,
    pub(crate) db: AssetDb,
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
    pub(crate) job_id: JobId,
    pub(crate) asset_id: AssetId,
    pub(crate) video_path: PathBuf,
    pub(crate) interval_secs: f64,
    pub(crate) resolution: (u32, u32),
    pub(crate) storage: CacheStorage,
    pub(crate) db: AssetDb,
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
    pub(crate) job_id: JobId,
    pub(crate) asset_id: AssetId,
    pub(crate) video_path: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: ProxyProfile,
    pub(crate) db: AssetDb,
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
    pub(crate) job_id: JobId,
    pub(crate) asset_id: AssetId,
    pub(crate) audio_path: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) profile: ProxyProfile,
    pub(crate) db: AssetDb,
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
        "waveform" => Some(Box::new(WaveformJob {
            job_id,
            asset_id,
            audio_path: PathBuf::from(field!(0)),
            chunk_size: field!(1).parse().ok()?,
            sample_rate: field!(2).parse().ok()?,
            storage: storage.clone(),
            db: db.clone(),
        })),
        "thumbnails" => Some(Box::new(ThumbnailJob {
            job_id,
            asset_id,
            video_path: PathBuf::from(field!(0)),
            interval_secs: field!(1).parse().ok()?,
            resolution: (field!(2).parse().ok()?, field!(3).parse().ok()?),
            storage: storage.clone(),
            db: db.clone(),
        })),
        "video_proxy" => Some(Box::new(VideoProxyJob {
            job_id,
            asset_id,
            video_path: PathBuf::from(field!(0)),
            output: PathBuf::from(field!(1)),
            profile: ProxyProfile::proxy_1080p_low(),
            db: db.clone(),
        })),
        "audio_proxy" => Some(Box::new(AudioProxyJob {
            job_id,
            asset_id,
            audio_path: PathBuf::from(field!(0)),
            output: PathBuf::from(field!(1)),
            profile: ProxyProfile::audio_proxy_flac(),
            db: db.clone(),
        })),
        _ => None,
    }
}
