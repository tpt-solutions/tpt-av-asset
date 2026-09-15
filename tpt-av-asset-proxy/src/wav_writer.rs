//! Minimal PCM WAV container writer for audio proxies.
//!
//! `tpt-cadence` currently ships decoders only (its `Encoder` trait is a
//! draft), so audio proxies are stored as 16-bit PCM WAV today. When the
//! cadence encoder lands — FLAC first per its roadmap — this module's call
//! sites switch to it and the profile's `.flac` output returns.

use std::fs::File;
use std::io::{BufWriter, Seek as _, Write as _};
use std::path::Path;

use tpt_av_asset_utils::AssetError;

/// Streaming 16-bit PCM WAV writer with header patching.
pub struct WavWriter {
    writer: BufWriter<File>,
    bytes_written: u32,
    path: std::path::PathBuf,
}

/// Sample-rate/channel spec for opening a [`WavWriter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavSpec {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
}

impl WavWriter {
    /// Creates the file and writes the 44-byte header placeholder (patched
    /// by [`WavWriter::finish`]).
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] if the file cannot be created or written.
    pub fn new(path: &Path, spec: WavSpec) -> Result<Self, AssetError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut writer = BufWriter::new(File::create(path)?);

        let block_align = spec.channels * 2;
        let byte_rate = spec.sample_rate * u32::from(block_align);

        writer.write_all(b"RIFF")?;
        writer.write_all(&[0u8; 4])?; // riff size (patched)
        writer.write_all(b"WAVE")?;
        writer.write_all(b"fmt ")?;
        writer.write_all(&16u32.to_le_bytes())?;
        writer.write_all(&1u16.to_le_bytes())?; // PCM
        writer.write_all(&spec.channels.to_le_bytes())?;
        writer.write_all(&spec.sample_rate.to_le_bytes())?;
        writer.write_all(&byte_rate.to_le_bytes())?;
        writer.write_all(&block_align.to_le_bytes())?;
        writer.write_all(&16u16.to_le_bytes())?;
        writer.write_all(b"data")?;
        writer.write_all(&[0u8; 4])?; // data size (patched)

        Ok(Self {
            writer,
            bytes_written: 0,
            path: path.to_path_buf(),
        })
    }

    /// The file being written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends interleaved `f32` samples (clamped to `[-1.0, 1.0]`, rounded
    /// to 16-bit).
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] on failure.
    pub fn write_samples(&mut self, samples: &[f32]) -> Result<(), AssetError> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for &sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.bytes_written += bytes.len() as u32;
        self.writer.write_all(&bytes)?;
        Ok(())
    }

    /// Flushes and patches the RIFF/data size fields.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] on failure.
    pub fn finish(mut self) -> Result<(), AssetError> {
        self.writer.flush()?;
        let mut file = self.writer.into_inner().map_err(|e| e.into_error())?;
        let data_len = self.bytes_written;
        file.seek(std::io::SeekFrom::Start(4))?;
        file.write_all(&(36 + data_len).to_le_bytes())?;
        file.seek(std::io::SeekFrom::Start(40))?;
        file.write_all(&data_len.to_le_bytes())?;
        Ok(())
    }
}
