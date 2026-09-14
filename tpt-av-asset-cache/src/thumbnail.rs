//! Video thumbnail cache.
//!
//! Layout: `thumbnails/{asset_hash}/NNNNNN.jpg` (one JPEG per interval) plus
//! a `meta` sidecar recording interval and resolution, written when the
//! cache is created. Indices map to time via `time_secs = index × interval`.

use image::GenericImageView as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use tpt_av_asset_utils::{AssetError, AssetId};

use crate::storage::CacheStorage;

const META_MAGIC: &[u8; 8] = b"TPTTHMB1";
const META_LEN: usize = 24;

/// A cached video thumbnail.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    /// Time position in seconds.
    pub time_secs: f64,
    /// Thumbnail pixel data (RGBA8), `width * height * 4` bytes.
    pub data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Video thumbnail cache for one asset.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    asset_id: AssetId,
    dir: PathBuf,
    interval_secs: f64,
    resolution: (u32, u32),
}

impl ThumbnailCache {
    /// Creates a fresh thumbnail cache with the given interval and
    /// resolution, writing the `meta` sidecar.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] if the directory/meta file cannot be
    /// created.
    pub fn create(
        asset_id: AssetId,
        storage: &CacheStorage,
        interval_secs: f64,
        resolution: (u32, u32),
    ) -> Result<Self, AssetError> {
        if !(interval_secs.is_finite() && interval_secs > 0.0) {
            return Err(AssetError::validation(format!(
                "invalid thumbnail interval {interval_secs}"
            )));
        }
        if resolution.0 == 0 || resolution.1 == 0 {
            return Err(AssetError::validation("invalid thumbnail resolution"));
        }
        let dir = storage.thumbnail_dir(asset_id);
        std::fs::create_dir_all(&dir)?;
        let cache = Self {
            asset_id,
            dir,
            interval_secs,
            resolution,
        };
        cache.write_meta()?;
        Ok(cache)
    }

    /// Creates or opens a thumbnail cache for an asset, recovering interval
    /// and resolution from the `meta` sidecar.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] if no meta sidecar exists (create
    /// one with [`ThumbnailCache::create`]).
    pub fn open(asset_id: AssetId, storage: &CacheStorage) -> Result<Self, AssetError> {
        let dir = storage.thumbnail_dir(asset_id);
        let meta_path = dir.join("meta");
        let raw = std::fs::read(&meta_path).map_err(|e| {
            AssetError::validation(format!(
                "no thumbnail meta for asset at {} ({e}); create the cache first",
                meta_path.display()
            ))
        })?;
        if raw.len() < META_LEN || &raw[0..8] != META_MAGIC {
            return Err(AssetError::validation("corrupt thumbnail meta"));
        }
        Ok(Self {
            asset_id,
            dir,
            interval_secs: f64::from_le_bytes(raw[8..16].try_into().expect("sized")),
            resolution: (
                u32::from_le_bytes(raw[16..20].try_into().expect("sized")),
                u32::from_le_bytes(raw[20..24].try_into().expect("sized")),
            ),
        })
    }

    /// The asset this cache belongs to.
    pub fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    /// Interval between thumbnails in seconds.
    pub fn interval_secs(&self) -> f64 {
        self.interval_secs
    }

    /// Thumbnail resolution `(width, height)`.
    pub fn resolution(&self) -> (u32, u32) {
        self.resolution
    }

    fn file_for_index(dir: &Path, index: u64) -> PathBuf {
        dir.join(format!("{index:06}.jpg"))
    }

    fn index_for_time(&self, time_secs: f64) -> u64 {
        ((time_secs.max(0.0) / self.interval_secs).round()) as u64
    }

    fn write_meta(&self) -> Result<(), AssetError> {
        let mut meta = [0u8; META_LEN];
        meta[0..8].copy_from_slice(META_MAGIC);
        meta[8..16].copy_from_slice(&self.interval_secs.to_le_bytes());
        meta[16..20].copy_from_slice(&self.resolution.0.to_le_bytes());
        meta[20..24].copy_from_slice(&self.resolution.1.to_le_bytes());
        let mut file = std::fs::File::create(self.dir.join("meta"))?;
        file.write_all(&meta)?;
        Ok(())
    }

    /// Encodes and writes one thumbnail. The storage index is derived from
    /// `thumbnail.time_secs`.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] on bad pixel data and I/O/codec
    /// errors on write.
    pub fn write_thumbnail(&mut self, thumbnail: &Thumbnail) -> Result<(), AssetError> {
        let index = self.index_for_time(thumbnail.time_secs);
        let img = image::RgbaImage::from_raw(
            thumbnail.width,
            thumbnail.height,
            thumbnail.data.clone(),
        )
        .ok_or_else(|| {
            AssetError::validation(format!(
                "thumbnail data length {} does not match {}x{} RGBA",
                thumbnail.data.len(),
                thumbnail.width,
                thumbnail.height
            ))
        })?;
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .map_err(|e| AssetError::codec(format!("jpeg encode failed: {e}")))?;

        let path = Self::file_for_index(&self.dir, index);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &jpeg)?;
        if !self.dir.join("meta").exists() {
            self.write_meta()?;
        }
        Ok(())
    }

    fn load(&self, index: u64) -> Result<Option<Thumbnail>, AssetError> {
        let path = Self::file_for_index(&self.dir, index);
        let jpeg = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let img = image::load_from_memory(&jpeg)
            .map_err(|e| AssetError::codec(format!("jpeg decode failed: {e}")))?
            .to_rgba8();
        let (width, height) = img.dimensions();
        Ok(Some(Thumbnail {
            time_secs: index as f64 * self.interval_secs,
            data: img.into_raw(),
            width,
            height,
        }))
    }

    /// Reads the thumbnail whose slot is exactly `time_secs` (rounded to the
    /// interval grid).
    ///
    /// # Errors
    /// Returns [`AssetError::Io`]/[`AssetError::Codec`] on failure.
    pub fn read_thumbnail(&self, time_secs: f64) -> Result<Option<Thumbnail>, AssetError> {
        self.load(self.index_for_time(time_secs))
    }

    /// Reads the nearest cached thumbnail to `time_secs`.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`]/[`AssetError::Codec`] on failure.
    pub fn read_nearest(&self, time_secs: f64) -> Result<Option<Thumbnail>, AssetError> {
        let indices = self.indices()?;
        let index = indices.into_iter().min_by(|a, b| {
            let da = (*a as f64 * self.interval_secs - time_secs).abs();
            let db = (*b as f64 * self.interval_secs - time_secs).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        match index {
            Some(index) => self.load(index),
            None => Ok(None),
        }
    }

    /// Returns the total number of thumbnails in the cache.
    pub fn thumbnail_count(&self) -> usize {
        self.indices().map(|v| v.len()).unwrap_or(0)
    }

    /// Sorted list of cached thumbnail times in seconds.
    pub fn times(&self) -> Vec<f64> {
        self.indices()
            .unwrap_or_default()
            .into_iter()
            .map(|i| i as f64 * self.interval_secs)
            .collect()
    }

    fn indices(&self) -> Result<Vec<u64>, AssetError> {
        let mut indices = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(stem) = name.strip_suffix(".jpg") {
                if let Ok(index) = stem.parse::<u64>() {
                    indices.push(index);
                }
            }
        }
        indices.sort_unstable();
        Ok(indices)
    }
}

/// Generates video thumbnails from a video file.
///
/// Seeks the `tpt-kinetix` decoder at every interval boundary, resizes the
/// frame to the configured resolution, and writes JPEG thumbnails.
/// Reports progress per thumbnail and aborts with [`AssetError::Cancelled`]
/// when the reporter is cancelled.
#[derive(Debug, Clone)]
pub struct ThumbnailGenerator {
    /// Interval between thumbnails in seconds.
    interval_secs: f64,
    /// Thumbnail resolution.
    resolution: (u32, u32),
}

impl ThumbnailGenerator {
    /// Creates a new thumbnail generator.
    pub fn new(interval_secs: f64, resolution: (u32, u32)) -> Self {
        Self {
            interval_secs,
            resolution,
        }
    }

    /// Generates thumbnails from a video file into `cache`.
    ///
    /// # Errors
    /// Returns [`AssetError::Codec`] when the video cannot be decoded,
    /// [`AssetError::Cancelled`] when cancelled, and I/O errors from cache
    /// writes.
    pub fn generate(
        &self,
        video_path: &Path,
        cache: &mut ThumbnailCache,
        progress: &tpt_av_asset_utils::ProgressReporter,
    ) -> Result<(), AssetError> {
        let mut decoder =
            tpt_kinetix::open(video_path).map_err(|e| AssetError::codec(e.to_string()))?;
        let info = decoder.info().clone();
        let count = ((info.duration_secs / self.interval_secs).ceil() as u64).max(1);

        for i in 0..count {
            progress.check_cancelled()?;
            let time_secs = i as f64 * self.interval_secs;
            decoder.seek_to(time_secs);
            let frame = decoder
                .next_frame()
                .map_err(|e| AssetError::codec(e.to_string()))?
                .ok_or_else(|| AssetError::codec("video ended before expected duration"))?;

            let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.data)
                .ok_or_else(|| AssetError::codec("decoder returned malformed frame"))?;
            let resized = image::DynamicImage::ImageRgba8(img).resize_exact(
                self.resolution.0,
                self.resolution.1,
                image::imageops::FilterType::Triangle,
            );
            let (width, height) = resized.dimensions();
            cache.write_thumbnail(&Thumbnail {
                time_secs,
                data: resized.to_rgba8().into_raw(),
                width,
                height,
            })?;
            progress.report_ratio((i + 1) as f64, count as f64);
        }
        progress.report(1.0);
        Ok(())
    }
}
