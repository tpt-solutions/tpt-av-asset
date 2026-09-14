//! Shared types, identifiers, progress reporting, and error handling for the
//! TPT AV asset engine.
//!
//! This crate has no dependencies beyond `std`, so every other crate in the
//! workspace can build on it. The main types:
//!
//! - [`AssetId`] — content-addressed asset identifier (path + mtime + size).
//! - [`MediaInfo`], [`MediaType`], [`VideoInfo`], [`AudioInfo`] — media
//!   metadata.
//! - [`TimeRange`] — half-open time range used across cache/proxy APIs.
//! - [`Priority`] — job priority levels (Low < Normal < High < Critical).
//! - [`ProgressReporter`] — progress callback + cooperative cancellation
//!   token shared by every long-running generator.
//! - [`AssetError`] — the single error type for the whole engine.

pub mod asset_id;
pub mod error;
pub mod media_info;
pub mod priority;
pub mod progress;
pub mod time_range;

pub use asset_id::AssetId;
pub use error::AssetError;
pub use media_info::{AudioInfo, MediaInfo, MediaType, VideoInfo};
pub use priority::Priority;
pub use progress::{ProgressEvent, ProgressReporter};
pub use time_range::TimeRange;
