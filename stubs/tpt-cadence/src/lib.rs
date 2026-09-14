//! Placeholder stand-in for the real [`tpt-cadence`] audio codec crate.
//!
//! The real `tpt-kinetix` / `tpt-cadence` git dependencies do not exist yet
//! (see `todo.md`, Phase 7). This crate provides the trait shapes the real
//! crate is expected to expose — [`AudioDecoder`] and [`AudioEncoder`] —
//! backed by a pure-Rust PCM WAV implementation plus synthetic test-media
//! helpers, so the rest of the `tpt-av-asset` workspace builds, runs, and is
//! fully testable today.
//!
//! When the real crate lands, swap the workspace dependency from
//! `path = "stubs/tpt-cadence"` to its git source; downstream code only
//! touches these traits.
//!
//! Supported formats: 8-bit unsigned PCM, 16-bit PCM, 24-bit PCM, 32-bit
//! integer PCM, and 32-bit IEEE float WAV. Encoding produces 16-bit PCM
//! (real FLAC encoding arrives with the real `tpt-cadence`).

mod synth;
mod wav;

use std::fmt;
use std::path::Path;

pub use synth::write_test_wav;
pub use wav::{WavDecoder, WavEncoder};

/// Metadata describing an audio stream.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
    /// Bits per sample.
    pub bit_depth: u16,
    /// Codec name (e.g. `"pcm_s16le"`).
    pub codec: String,
    /// Bit rate in bits per second, if known.
    pub bit_rate: Option<u64>,
    /// Duration in seconds.
    pub duration_secs: f64,
}

impl AudioInfo {
    /// Total number of interleaved samples in the stream.
    pub fn total_samples(&self) -> u64 {
        (self.duration_secs * self.sample_rate as f64) as u64 * self.channels as u64
    }
}

/// Sample-rate/channel spec for opening an encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSpec {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
}

/// Decode/encode failures.
#[derive(Debug)]
pub enum Error {
    /// Underlying I/O failure.
    Io(std::io::Error),
    /// The file is not a supported audio container.
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "audio I/O error: {e}"),
            Error::Format(msg) => write!(f, "unsupported audio format: {msg}"),
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

/// Decodes audio into interleaved `f32` samples in `[-1.0, 1.0]`.
pub trait AudioDecoder: Send {
    /// Stream metadata.
    fn info(&self) -> &AudioInfo;

    /// Reads up to `out.len()` interleaved samples into `out`, returning how
    /// many were written. Zero means end of stream.
    ///
    /// # Errors
    /// Returns [`Error`] on I/O failure.
    fn read_samples(&mut self, out: &mut [f32]) -> Result<usize>;
}

/// Opens an audio file for decoding (WAV for this stand-in).
///
/// # Errors
/// Returns [`Error::Format`] if the file is not a supported WAV.
pub fn open(path: &Path) -> Result<Box<dyn AudioDecoder>> {
    Ok(Box::new(wav::WavDecoder::open(path)?))
}

/// Encodes interleaved `f32` samples.
pub trait AudioEncoder: Send {
    /// Appends interleaved samples.
    ///
    /// # Errors
    /// Returns [`Error`] on I/O failure.
    fn write_samples(&mut self, samples: &[f32]) -> Result<()>;

    /// Flushes and finalizes the container (patches size fields).
    ///
    /// # Errors
    /// Returns [`Error`] on I/O failure.
    fn finish(self: Box<Self>) -> Result<()>;
}

/// Opens an encoder writing 16-bit PCM WAV at `path`.
///
/// # Errors
/// Returns [`Error`] if the file cannot be created.
pub fn open_encoder(path: &Path, spec: &AudioSpec) -> Result<Box<dyn AudioEncoder>> {
    Ok(Box::new(wav::WavEncoder::new(path, *spec)?))
}
