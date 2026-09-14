//! Placeholder stand-in for the real [`tpt-kinetix`] media container/video
//! codec crate.
//!
//! The real `tpt-kinetix` / `tpt-cadence` git dependencies do not exist yet
//! (see `todo.md`, Phase 7). This crate provides the trait shapes the real
//! crate is expected to expose — [`VideoDecoder`] and [`VideoEncoder`] —
//! backed by a trivial synthetic container (`TKV1`: a small header followed
//! by raw RGBA8 frames) plus test-media helpers, so the rest of the
//! `tpt-av-asset` workspace builds, runs, and is fully testable today.
//!
//! When the real crate lands, swap the workspace dependency from
//! `path = "stubs/tpt-kinetix"` to its git source; downstream code only
//! touches these traits.

mod synth;
mod tkv;

use std::fmt;
use std::path::Path;

pub use synth::{gradient_painter, write_test_video};
pub use tkv::{TkvDecoder, TkvEncoder};

/// Metadata describing a video stream.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frame rate in frames per second.
    pub frame_rate: f64,
    /// Codec name (e.g. `"tkv_rgba8"` for this stand-in's container).
    pub codec: String,
    /// Pixel format (e.g. `"rgba8"`).
    pub pixel_format: String,
    /// Bit rate in bits per second, if known.
    pub bit_rate: Option<u64>,
    /// Total frame count.
    pub frame_count: u32,
    /// Duration in seconds.
    pub duration_secs: f64,
}

/// A decoded video frame (RGBA8).
#[derive(Debug, Clone)]
pub struct Frame {
    /// Zero-based frame index.
    pub index: u32,
    /// Presentation timestamp in seconds.
    pub time_secs: f64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixel data, `width * height * 4` bytes.
    pub data: Vec<u8>,
}

/// Decode/encode failures.
#[derive(Debug)]
pub enum Error {
    /// Underlying I/O failure.
    Io(std::io::Error),
    /// The file is not a supported video container.
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "video I/O error: {e}"),
            Error::Format(msg) => write!(f, "unsupported video format: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Format(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Convenience result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Decodes video frames sequentially with random access via [`VideoDecoder::seek_to`].
pub trait VideoDecoder: Send {
    /// Stream metadata.
    fn info(&self) -> &VideoInfo;

    /// Positions the decoder at the first frame at or after `time_secs`.
    fn seek_to(&mut self, time_secs: f64);

    /// Decodes and returns the next frame, or `None` at end of stream.
    ///
    /// # Errors
    /// Returns [`Error`] on I/O failure.
    fn next_frame(&mut self) -> Result<Option<Frame>>;
}

/// Opens a video file for decoding (TKV for this stand-in).
///
/// # Errors
/// Returns [`Error::Format`] if the file is not a supported container.
pub fn open(path: &Path) -> Result<Box<dyn VideoDecoder>> {
    Ok(Box::new(tkv::TkvDecoder::open(path)?))
}

/// Encodes RGBA8 frames.
pub trait VideoEncoder: Send {
    /// Appends a frame. Frame dimensions must match the encoder spec.
    ///
    /// # Errors
    /// Returns [`Error`] on I/O or dimension mismatch.
    fn write_frame(&mut self, frame: &Frame) -> Result<()>;

    /// Flushes and finalizes the container.
    ///
    /// # Errors
    /// Returns [`Error`] on I/O failure.
    fn finish(self: Box<Self>) -> Result<()>;
}

/// Opens an encoder writing the TKV container at `path`.
///
/// # Errors
/// Returns [`Error`] if the file cannot be created.
pub fn open_encoder(path: &Path, width: u32, height: u32, frame_rate: f64) -> Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(tkv::TkvEncoder::new(path, width, height, frame_rate)?))
}
