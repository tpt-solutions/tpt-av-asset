//! Real-time safe waveform reading.
//!
//! # Real-Time Safety contract
//!
//! [`WaveformReader::read_chunk`] and [`WaveformReader::read_range_into`]
//! are **allocation-free and free of userspace locks**, so they are safe to
//! call from the audio/render thread:
//!
//! - Each chunk is fetched with a single positioned read
//!   (`pread` on Unix, `ReadFileScatter`-style `seek_read` on Windows)
//!   into a caller-provided stack buffer; the chunk is returned by value as
//!   a [`Copy`] struct. No heap allocation occurs.
//! - No `Mutex`/`RwLock` is taken: positioned reads do not mutate shared
//!   seek state, and the header fields are cached in the reader.
//! - File reads do block on I/O — for the audio thread the intended pattern
//!   is a pre-warmed in-process buffer, with `read_chunk` used to fill it
//!   off the first miss. See the allocation-counting test in
//!   `tests/alloc_free.rs`, which proves zero heap allocations.

use std::fs::File;
use std::io;
use std::path::Path;

use crate::waveform::{WaveformChunk, HEADER_LEN, RECORD_LEN};

/// Read-only, random-access view over a `.peaks` file.
///
/// Duplicate via [`WaveformReader::try_clone`] to hand a reader to the
/// audio thread (`File` handles are not `Clone`).
#[derive(Debug)]
pub struct WaveformReader {
    pub(crate) file: File,
    pub(crate) chunk_size: u32,
    pub(crate) sample_rate: u32,
    pub(crate) chunk_count: u64,
}

impl WaveformReader {
    /// Opens and validates a `.peaks` file.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`](tpt_av_asset_utils::AssetError::Io) for I/O failures and
    /// [`AssetError::Validation`](tpt_av_asset_utils::AssetError::Validation) for a bad header.
    pub fn open(path: &Path) -> Result<Self, tpt_av_asset_utils::AssetError> {
        let file = File::open(path)?;
        let mut header = [0u8; HEADER_LEN as usize];
        read_exact_at(&file, &mut header, 0)?;
        if &header[0..8] != crate::waveform::MAGIC {
            return Err(tpt_av_asset_utils::AssetError::validation(format!(
                "{} is not a waveform peaks file",
                path.display()
            )));
        }
        Ok(Self {
            file,
            chunk_size: u32::from_le_bytes(header[8..12].try_into().expect("sized")),
            sample_rate: u32::from_le_bytes(header[12..16].try_into().expect("sized")),
            chunk_count: u64::from_le_bytes(header[16..24].try_into().expect("sized")),
        })
    }

    /// Samples per chunk (frames).
    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    /// Sample rate the peaks were computed at.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Total number of chunks stored.
    pub fn chunk_count(&self) -> u64 {
        self.chunk_count
    }

    /// Duplicates the reader (same open file handle semantics).
    ///
    /// # Errors
    /// Returns [`AssetError::Io`](tpt_av_asset_utils::AssetError::Io) if the handle cannot be cloned.
    pub fn try_clone(&self) -> Result<Self, tpt_av_asset_utils::AssetError> {
        Ok(Self {
            file: self.file.try_clone()?,
            chunk_size: self.chunk_size,
            sample_rate: self.sample_rate,
            chunk_count: self.chunk_count,
        })
    }

    /// Reads one chunk of peaks.
    ///
    /// # Real-Time Safety
    /// Allocation-free and lock-free; see the [module docs](self).
    ///
    /// # Errors
    /// Returns [`AssetError::Io`](tpt_av_asset_utils::AssetError::Io) on read failure.
    pub fn read_chunk(
        &self,
        index: u64,
    ) -> Result<Option<WaveformChunk>, tpt_av_asset_utils::AssetError> {
        if index >= self.chunk_count {
            return Ok(None);
        }
        let mut raw = [0u8; RECORD_LEN];
        read_exact_at(&self.file, &mut raw, HEADER_LEN + index * RECORD_LEN as u64)?;
        Ok(Some(decode_record(index, &raw)))
    }

    /// Fills `out` with chunks starting at `start_index`, returning how many
    /// were read (fewer at the end of the file).
    ///
    /// # Real-Time Safety
    /// Allocation-free and lock-free; `out` must be pre-allocated.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`](tpt_av_asset_utils::AssetError::Io) on read failure.
    pub fn read_range_into(
        &self,
        start_index: u64,
        out: &mut [WaveformChunk],
    ) -> Result<usize, tpt_av_asset_utils::AssetError> {
        let mut read = 0usize;
        for (i, slot) in out.iter_mut().enumerate() {
            match self.read_chunk(start_index + i as u64)? {
                Some(chunk) => *slot = chunk,
                None => break,
            }
            read += 1;
        }
        Ok(read)
    }
}

pub(crate) fn decode_record(index: u64, raw: &[u8; RECORD_LEN]) -> WaveformChunk {
    WaveformChunk {
        index: index as usize,
        min: f32::from_le_bytes(raw[0..4].try_into().expect("sized")),
        max: f32::from_le_bytes(raw[4..8].try_into().expect("sized")),
        rms: f32::from_le_bytes(raw[8..12].try_into().expect("sized")),
    }
}

pub(crate) fn encode_record(chunk: &WaveformChunk) -> [u8; RECORD_LEN] {
    let mut raw = [0u8; RECORD_LEN];
    raw[0..4].copy_from_slice(&chunk.min.to_le_bytes());
    raw[4..8].copy_from_slice(&chunk.max.to_le_bytes());
    raw[8..12].copy_from_slice(&chunk.rms.to_le_bytes());
    raw
}

/// Positioned read that never allocates and never takes a userspace lock.
pub(crate) fn read_exact_at(file: &File, buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = file.seek_read(&mut buf[filled..], offset)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected end of file in positioned read",
                ));
            }
            offset += n as u64;
            filled += n;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (&file, &mut buf, &mut offset);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no positioned read on this platform",
        ))
    }
}
