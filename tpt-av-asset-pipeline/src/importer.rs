//! High-level asset import API.

use std::path::Path;

use tpt_av_asset_cache::CacheStorage;
use tpt_av_asset_db::AssetDb;
use tpt_av_asset_utils::{AssetError, AssetId, MediaInfo, MediaType};

use crate::jobs::{AudioProxyJob, ThumbnailJob, VideoProxyJob, WaveformJob};
use crate::pipeline::ProcessingPipeline;

/// Default thumbnail cadence used by imports.
const DEFAULT_THUMBNAIL_INTERVAL: f64 = 1.0;
/// Default thumbnail resolution.
const DEFAULT_THUMBNAIL_RESOLUTION: (u32, u32) = (320, 180);
/// Default waveform chunk size (frames).
const DEFAULT_CHUNK_SIZE: u32 = 1024;

/// High-level asset import API: probe → index → schedule.
///
/// `import` computes the asset id, extracts media info through the decoder
/// traits, inserts the asset into the database, schedules background jobs
/// for waveform/thumbnails/proxies, and returns immediately.
pub struct AssetImporter {
    pipeline: ProcessingPipeline,
    db: AssetDb,
    storage: CacheStorage,
    thumbnail_interval: f64,
    thumbnail_resolution: (u32, u32),
    chunk_size: u32,
}

impl AssetImporter {
    /// Creates a new asset importer.
    pub fn new(pipeline: ProcessingPipeline, db: AssetDb, storage: CacheStorage) -> Self {
        Self {
            pipeline,
            db,
            storage,
            thumbnail_interval: DEFAULT_THUMBNAIL_INTERVAL,
            thumbnail_resolution: DEFAULT_THUMBNAIL_RESOLUTION,
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }

    /// Overrides the thumbnail cadence and resolution for future imports.
    pub fn with_thumbnails(mut self, interval_secs: f64, resolution: (u32, u32)) -> Self {
        self.thumbnail_interval = interval_secs;
        self.thumbnail_resolution = resolution;
        self
    }

    /// Overrides the waveform chunk size for future imports.
    pub fn with_chunk_size(mut self, chunk_size: u32) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// The pipeline this importer schedules into.
    pub fn pipeline(&self) -> &ProcessingPipeline {
        &self.pipeline
    }

    /// The media database.
    pub fn db(&self) -> &AssetDb {
        &self.db
    }

    /// The cache storage root.
    pub fn storage(&self) -> &CacheStorage {
        &self.storage
    }

    /// Imports a media file and generates all caches, returning the asset
    /// id immediately (jobs run in the background).
    ///
    /// # Errors
    /// Returns [`AssetError`] for missing files, unsupported formats, and
    /// database/submit failures.
    pub fn import(&self, path: &Path) -> Result<AssetId, AssetError> {
        self.import_with_jobs(path).map(|(id, _)| id)
    }

    /// [`AssetImporter::import`] plus the scheduled job ids (tests and
    /// progress dashboards).
    ///
    /// # Errors
    /// Same as [`AssetImporter::import`].
    pub fn import_with_jobs(&self, path: &Path) -> Result<(AssetId, Vec<crate::job::JobId>), AssetError> {
        if std::fs::metadata(path).is_err() {
            return Err(AssetError::NotFound(path.to_path_buf()));
        }

        // 1–2. Compute asset ID + extract media info.
        let info = probe_media_info(path)?;
        // 3. Insert into the database.
        self.db.upsert_asset(&info)?;

        // 4–6. Schedule background jobs.
        let mut jobs: Vec<Box<dyn crate::job::Job>> = Vec::new();
        match info.media_type {
            MediaType::Audio => {
                jobs.push(Box::new(WaveformJob::new(
                    self.pipeline.next_job_id(),
                    info.id,
                    info.path.clone(),
                    self.chunk_size,
                    info.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48_000),
                    self.storage.clone(),
                    self.db.clone(),
                )));
                jobs.push(Box::new(AudioProxyJob::new(
                    self.pipeline.next_job_id(),
                    info.id,
                    info.path.clone(),
                    self.storage.proxy_path(info.id, "flac"),
                    tpt_av_asset_proxy::ProxyProfile::audio_proxy_flac(),
                    self.db.clone(),
                )));
            }
            MediaType::Video => {
                jobs.push(Box::new(ThumbnailJob::new(
                    self.pipeline.next_job_id(),
                    info.id,
                    info.path.clone(),
                    self.thumbnail_interval,
                    self.thumbnail_resolution,
                    self.storage.clone(),
                    self.db.clone(),
                )));
                jobs.push(Box::new(VideoProxyJob::new(
                    self.pipeline.next_job_id(),
                    info.id,
                    info.path.clone(),
                    self.storage.proxy_path(info.id, "mp4"),
                    tpt_av_asset_proxy::ProxyProfile::proxy_1080p_low(),
                    self.db.clone(),
                )));
            }
            MediaType::Image => {
                // No background processing for stills today.
            }
        }

        let mut ids = Vec::with_capacity(jobs.len());
        for job in jobs {
            ids.push(self.pipeline.submit(job)?);
        }

        // 7. Return the asset ID immediately.
        Ok((info.id, ids))
    }
}

/// Probes a media file's identity and metadata through the decoder traits,
/// falling back to extension sniffing for still images.
///
/// # Errors
/// Returns [`AssetError::UnsupportedFormat`] when no decoder recognizes the
/// file.
pub fn probe_media_info(path: &Path) -> Result<MediaInfo, AssetError> {
    let id = AssetId::from_path(path)?;

    if let Ok(decoder) = tpt_kinetix::open(path) {
        let video = decoder.info().clone();
        let mut info = MediaInfo::new(id, path, MediaType::Video);
        info.duration_secs = Some(video.duration_secs);
        info.video = Some(tpt_av_asset_utils::VideoInfo {
            width: video.width,
            height: video.height,
            frame_rate: video.frame_rate,
            codec: video.codec,
            pixel_format: video.pixel_format,
            bit_rate: video.bit_rate,
            frame_count: video.frame_count,
            duration_secs: video.duration_secs,
        });
        return Ok(info);
    }

    if let Ok(decoder) = tpt_cadence::open(path) {
        let audio = decoder.info().clone();
        let mut info = MediaInfo::new(id, path, MediaType::Audio);
        info.duration_secs = Some(audio.duration_secs);
        info.audio = Some(tpt_av_asset_utils::AudioInfo {
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            bit_depth: audio.bit_depth,
            codec: audio.codec,
            bit_rate: audio.bit_rate,
            duration_secs: audio.duration_secs,
        });
        return Ok(info);
    }

    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        const IMAGE_EXTS: [&str; 6] = ["jpg", "jpeg", "png", "bmp", "gif", "tiff"];
        if IMAGE_EXTS.contains(&extension.to_ascii_lowercase().as_str()) {
            return Ok(MediaInfo::new(id, path, MediaType::Image));
        }
    }

    Err(AssetError::UnsupportedFormat(path.to_path_buf()))
}
