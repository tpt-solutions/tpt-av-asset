//! The stand-in `TKV1` container: a fixed 24-byte header followed by raw
//! RGBA8 frames. Random access is trivial because every frame has a fixed
//! size.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{Error, Frame, Result, VideoInfo};

const MAGIC: &[u8; 4] = b"TKV1";
const HEADER_LEN: u64 = 24;

fn frame_bytes(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height) * 4
}

/// TKV decoder.
pub struct TkvDecoder {
    reader: BufReader<File>,
    info: VideoInfo,
    next_index: u32,
}

impl TkvDecoder {
    /// Opens and validates a TKV file.
    ///
    /// # Errors
    /// Returns [`Error::Format`] for a bad header.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        let mut header = [0u8; HEADER_LEN as usize];
        reader.read_exact(&mut header).map_err(|e| {
            Error::Format(format!("truncated TKV header: {e}"))
        })?;
        if &header[0..4] != MAGIC {
            return Err(Error::Format("missing TKV1 magic".into()));
        }
        let width = u32::from_le_bytes(header[4..8].try_into().expect("sized"));
        let height = u32::from_le_bytes(header[8..12].try_into().expect("sized"));
        let frame_rate = f64::from_le_bytes(header[12..20].try_into().expect("sized"));
        let frame_count = u32::from_le_bytes(header[20..24].try_into().expect("sized"));

        if width == 0 || height == 0 {
            return Err(Error::Format("zero-sized frame".into()));
        }
        if !frame_rate.is_finite() || frame_rate <= 0.0 {
            return Err(Error::Format(format!("invalid frame rate {frame_rate}")));
        }

        Ok(Self {
            info: VideoInfo {
                width,
                height,
                frame_rate,
                codec: "tkv_rgba8".to_string(),
                pixel_format: "rgba8".to_string(),
                bit_rate: Some(
                    ((frame_bytes(width, height) as f64 * frame_rate * 8.0) as u64).max(1),
                ),
                frame_count,
                duration_secs: f64::from(frame_count) / frame_rate,
            },
            reader,
            next_index: 0,
        })
    }

    fn frame_offset(&self, index: u32) -> u64 {
        HEADER_LEN + u64::from(index) * frame_bytes(self.info.width, self.info.height)
    }
}

impl crate::VideoDecoder for TkvDecoder {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn seek_to(&mut self, time_secs: f64) {
        let clamped = time_secs.max(0.0);
        let index = (clamped * self.info.frame_rate).round() as u64;
        let index = index.min(u64::from(self.info.frame_count));
        self.next_index = index as u32;
        let offset = self.frame_offset(self.next_index);
        // Seeking past EOF is fine; the next read returns None.
        let _ = self.reader.seek(SeekFrom::Start(offset));
    }

    fn next_frame(&mut self) -> Result<Option<Frame>> {
        if self.next_index >= self.info.frame_count {
            return Ok(None);
        }
        let w = self.info.width;
        let h = self.info.height;
        let mut data = vec![0u8; frame_bytes(w, h) as usize];
        if self.reader.read_exact(&mut data).is_err() {
            // Truncated stream: stop decoding rather than fail hard.
            return Ok(None);
        }
        let frame = Frame {
            index: self.next_index,
            time_secs: f64::from(self.next_index) / self.info.frame_rate,
            width: w,
            height: h,
            data,
        };
        self.next_index += 1;
        Ok(Some(frame))
    }
}

/// TKV encoder.
pub struct TkvEncoder {
    writer: BufWriter<File>,
    width: u32,
    height: u32,
    frame_count: u32,
}

impl TkvEncoder {
    /// Creates the file and writes a placeholder header (frame count is
    /// patched by [`TkvEncoder::finish`]).
    ///
    /// # Errors
    /// Returns [`Error`] if the file cannot be created.
    pub fn new(path: &Path, width: u32, height: u32, frame_rate: f64) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::Format("zero-sized frame".into()));
        }
        if !frame_rate.is_finite() || frame_rate <= 0.0 {
            return Err(Error::Format(format!("invalid frame rate {frame_rate}")));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(MAGIC)?;
        writer.write_all(&width.to_le_bytes())?;
        writer.write_all(&height.to_le_bytes())?;
        writer.write_all(&frame_rate.to_le_bytes())?;
        writer.write_all(&0u32.to_le_bytes())?; // frame count (patched)

        Ok(Self {
            writer,
            width,
            height,
            frame_count: 0,
        })
    }
}

impl TkvEncoder {
    fn finish_impl(mut self) -> Result<()> {
        self.writer.flush()?;
        let mut file = self.writer.into_inner().map_err(|e| e.into_error())?;
        file.seek(SeekFrom::Start(20))?;
        file.write_all(&self.frame_count.to_le_bytes())?;
        Ok(())
    }
}

impl crate::VideoEncoder for TkvEncoder {
    fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        if frame.width != self.width || frame.height != self.height {
            return Err(Error::Format(format!(
                "frame {}x{} does not match encoder {}x{}",
                frame.width, frame.height, self.width, self.height
            )));
        }
        let expected = frame_bytes(self.width, self.height) as usize;
        if frame.data.len() != expected {
            return Err(Error::Format(format!(
                "frame data length {} does not match {}x{} RGBA8",
                frame.data.len(),
                self.width,
                self.height
            )));
        }
        self.writer.write_all(&frame.data)?;
        self.frame_count += 1;
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<()> {
        TkvEncoder::finish_impl(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gradient_painter, write_test_video, VideoDecoder};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-kinetix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn tkv_roundtrip() {
        let path = temp_path("roundtrip.tkv");
        write_test_video(&path, 16, 8, 10.0, 0.5, gradient_painter).unwrap();

        let mut dec = TkvDecoder::open(&path).unwrap();
        let info = dec.info().clone();
        assert_eq!((info.width, info.height), (16, 8));
        assert_eq!(info.frame_rate, 10.0);
        assert_eq!(info.frame_count, 5);
        assert!((info.duration_secs - 0.5).abs() < 1e-9);

        // Seek into the middle and read to the end.
        dec.seek_to(0.3);
        let mut count = 0;
        let mut first_time = None;
        while let Some(frame) = dec.next_frame().unwrap() {
            if first_time.is_none() {
                first_time = Some(frame.time_secs);
            }
            assert_eq!(frame.data.len(), 16 * 8 * 4);
            count += 1;
        }
        assert_eq!(count, 2);
        assert!(first_time.unwrap() >= 0.3 - 1e-9);

        // Frames differ over time (moving gradient).
        dec.seek_to(0.0);
        let f0 = dec.next_frame().unwrap().unwrap();
        dec.seek_to(0.4);
        let f4 = dec.next_frame().unwrap().unwrap();
        assert_ne!(f0.data, f4.data);
        std::fs::remove_file(&path).ok();
    }
}
