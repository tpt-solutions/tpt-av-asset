//! Audio decoding through the real `tpt-cadence` stack.
//!
//! [`open_audio`] probes a file with the available cadence `FormatReader`s
//! (WAV, then FLAC) and hands back a uniform frame-decoding interface. The
//! cadence decoders are real-time-safe after init: `decode` is
//! allocation-free and lock-free — the same contract the waveform cache's
//! read path honors.

use std::path::Path;

use tpt_av_asset_utils::AssetError;
use tpt_av_cadence_core::FormatReader;

use crate::error_map;

/// Metadata about an open audio stream.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioStreamInfo {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
    /// Source bit depth.
    pub bit_depth: u16,
    /// Container/codec name (`"wav"` or `"flac"`).
    pub codec: String,
    /// Total sample frames, when the container declares them.
    pub total_frames: Option<u64>,
    /// Duration in seconds, when derivable.
    pub duration_secs: Option<f64>,
}

impl AudioStreamInfo {
    /// Total interleaved samples, when the stream length is known.
    pub fn total_samples(&self) -> Option<u64> {
        self.total_frames
            .map(|frames| frames * u64::from(self.channels))
    }
}

/// A frame-decoding handle over an open audio file.
///
/// `decode` writes interleaved `f32` samples (the buffer length must be a
/// multiple of the channel count) and returns the number of **frames**
/// written; zero means end of stream.
pub trait AudioStream: Send {
    /// Stream metadata.
    fn info(&self) -> &AudioStreamInfo;

    /// Decodes the next block of frames.
    ///
    /// # Errors
    /// Returns [`AssetError::Codec`] on decode failure.
    fn decode(&mut self, buffer: &mut [f32]) -> Result<usize, AssetError>;
}

struct WavStream {
    reader: tpt_av_cadence_wav::WavReader,
    info: AudioStreamInfo,
}

struct FlacStream {
    reader: tpt_av_cadence_flac::FlacReader,
    info: AudioStreamInfo,
}

fn stream_info(codec: &str, raw: &tpt_av_cadence_core::StreamInfo) -> AudioStreamInfo {
    AudioStreamInfo {
        sample_rate: raw.sample_rate,
        channels: raw.channels,
        bit_depth: raw.bit_depth,
        codec: codec.to_string(),
        total_frames: raw.total_frames,
        duration_secs: raw
            .total_frames
            .map(|frames| frames as f64 / f64::from(raw.sample_rate.max(1))),
    }
}

/// Opens an audio file for decoding, trying WAV first and then FLAC.
///
/// # Errors
/// Returns [`AssetError::UnsupportedFormat`] when no reader accepts the
/// file, and [`AssetError::Io`] for filesystem failures.
pub fn open_audio(path: &Path) -> Result<Box<dyn AudioStream>, AssetError> {
    if let Ok(reader) = tpt_av_cadence_wav::WavReader::open(Box::new(std::fs::File::open(path)?)) {
        let info = stream_info("wav", reader.info());
        return Ok(Box::new(WavStream { reader, info }));
    }
    if let Ok(reader) = tpt_av_cadence_flac::FlacReader::open(Box::new(std::fs::File::open(path)?))
    {
        let info = stream_info("flac", reader.info());
        return Ok(Box::new(FlacStream { reader, info }));
    }
    Err(AssetError::UnsupportedFormat(path.to_path_buf()))
}

impl AudioStream for WavStream {
    fn info(&self) -> &AudioStreamInfo {
        &self.info
    }

    fn decode(&mut self, buffer: &mut [f32]) -> Result<usize, AssetError> {
        self.reader
            .decoder()
            .decode(buffer)
            .map_err(error_map::codec)
    }
}

impl AudioStream for FlacStream {
    fn info(&self) -> &AudioStreamInfo {
        &self.info
    }

    fn decode(&mut self, buffer: &mut [f32]) -> Result<usize, AssetError> {
        self.reader
            .decoder()
            .decode(buffer)
            .map_err(error_map::codec)
    }
}

/// Probes an audio file's metadata without keeping the stream open.
///
/// # Errors
/// Returns [`AssetError::UnsupportedFormat`] when no reader accepts the
/// file.
pub fn probe_audio(path: &Path) -> Result<AudioStreamInfo, AssetError> {
    Ok(open_audio(path)?.info().clone())
}
