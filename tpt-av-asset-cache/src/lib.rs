//! Waveform and thumbnail caching for the TPT AV asset engine.
//!
//! Two on-disk caches built on [`CacheStorage`]:
//!
//! - [`WaveformCache`] — fixed-size min/max/RMS peak chunks (`.peaks`
//!   files). Reading is **real-time safe**: [`WaveformReader::read_chunk`]
//!   and [`WaveformReader::read_range_into`] perform positioned reads into
//!   caller-provided buffers and return `Copy` values — allocation-free and
//!   lock-free, safe for the audio/render thread. An allocation-counting
//!   test guards this guarantee.
//! - [`ThumbnailCache`] — one JPEG per interval for fast timeline scrubbing.
//!
//! [`WaveformGenerator`] and [`ThumbnailGenerator`] populate the caches
//! through the `tpt-cadence` / `tpt-kinetix` decoder traits, with progress
//! reporting, cooperative cancellation, and resumable partial progress.
//! [`invalidate_asset`] ties invalidation into `tpt-av-asset-db`.

pub mod audio;
pub mod container;
pub use container::{planes_to_rgba, rgba_to_planes};
pub mod error_map;
pub mod invalidation;
pub mod reader;
pub mod storage;
pub mod thumbnail;
pub mod video;
pub mod waveform;

pub use audio::{open_audio, AudioStream, AudioStreamInfo};
pub use invalidation::invalidate_asset;
pub use reader::WaveformReader;
pub use storage::CacheStorage;
pub use thumbnail::{Thumbnail, ThumbnailCache, ThumbnailGenerator};
pub use video::{open_video, probe_video, RgbaFrame, VideoSource};
pub use waveform::{WaveformCache, WaveformChunk, WaveformGenerator};
