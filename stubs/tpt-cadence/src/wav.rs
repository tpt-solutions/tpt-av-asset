//! Minimal pure-Rust PCM WAV reader/writer.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{AudioInfo, AudioSpec, Error, Result};

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;

/// WAV decoder producing interleaved `f32` samples.
pub struct WavDecoder {
    reader: BufReader<File>,
    info: AudioInfo,
    data_len: u32,
    bytes_read: u32,
    bytes_per_sample: usize,
    format: u16,
}

impl WavDecoder {
    /// Opens and parses a RIFF/WAVE file.
    ///
    /// # Errors
    /// Returns [`Error::Format`] for missing/malformed chunks or unsupported
    /// bit depths.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        let mut riff = [0u8; 12];
        reader.read_exact(&mut riff)?;
        if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
            return Err(Error::Format("missing RIFF/WAVE header".into()));
        }

        let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
        let mut data: Option<(u64, u32)> = None; // (offset, len)

        loop {
            let mut header = [0u8; 8];
            match reader.read_exact(&mut header) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
            let id = &header[0..4];
            let size = u32::from_le_bytes(header[4..8].try_into().expect("8-byte slice"));
            let offset = reader.stream_position()?;

            match id {
                b"fmt " => {
                    let mut chunk = vec![0u8; size as usize];
                    reader.read_exact(&mut chunk)?;
                    if size < 16 {
                        return Err(Error::Format("fmt chunk too small".into()));
                    }
                    let format = u16::from_le_bytes(chunk[0..2].try_into().expect("sized"));
                    let channels = u16::from_le_bytes(chunk[2..4].try_into().expect("sized"));
                    let rate = u32::from_le_bytes(chunk[4..8].try_into().expect("sized"));
                    let bits = u16::from_le_bytes(chunk[14..16].try_into().expect("sized"));
                    fmt = Some((format, channels, rate, bits));
                }
                b"data" => {
                    // Clamp to the actual file length; some writers pad the size.
                    let file_len = reader.seek(SeekFrom::End(0))?;
                    let len = (offset + size as u64).min(file_len) - offset;
                    data = Some((offset, len as u32));
                    reader.seek(SeekFrom::Start(offset))?;
                    // Continue scanning in case data precedes fmt.
                    reader.seek(SeekFrom::Start(offset + size as u64))?;
                }
                _ => {
                    reader.seek(SeekFrom::Current(size as i64))?;
                }
            }
            // Chunks are word-aligned.
            if size % 2 == 1 {
                reader.seek(SeekFrom::Current(1))?;
            }
        }

        let (format, channels, rate, bits) =
            fmt.ok_or_else(|| Error::Format("missing fmt chunk".into()))?;
        let (data_start, data_len) =
            data.ok_or_else(|| Error::Format("missing data chunk".into()))?;

        if channels == 0 {
            return Err(Error::Format("zero channels".into()));
        }
        if bits % 8 != 0 || !(8..=32).contains(&bits) {
            return Err(Error::Format(format!("unsupported bit depth {bits}")));
        }
        if bits == 8 && format != FORMAT_PCM {
            return Err(Error::Format("8-bit must be unsigned PCM".into()));
        }
        if bits == 32 && format == FORMAT_FLOAT {
            // ok
        } else if format != FORMAT_PCM {
            return Err(Error::Format(format!("unsupported format tag {format}")));
        }

        let bytes_per_sample = (bits / 8) as usize;
        let block_align = bytes_per_sample * channels as usize;
        let duration_secs = data_len as f64 / (rate as f64 * block_align as f64);
        let byte_rate = rate as u64 * block_align as u64;

        let codec = if format == FORMAT_FLOAT {
            "pcm_f32le".to_string()
        } else {
            format!("pcm_s{}le", bits)
        };

        reader.seek(SeekFrom::Start(data_start))?;

        Ok(Self {
            reader,
            info: AudioInfo {
                sample_rate: rate,
                channels,
                bit_depth: bits,
                codec,
                bit_rate: Some(byte_rate * 8),
                duration_secs,
            },
            data_len,
            bytes_read: 0,
            bytes_per_sample,
            format,
        })
    }

    fn decode_sample(&self, raw: &[u8]) -> f32 {
        match (self.format, self.bytes_per_sample) {
            (FORMAT_FLOAT, 4) => f32::from_le_bytes(raw.try_into().expect("4 bytes")),
            (_, 1) => (raw[0] as f32 - 128.0) / 128.0,
            (_, 2) => i16::from_le_bytes(raw.try_into().expect("2 bytes")) as f32 / 32768.0,
            (_, 3) => {
                let v = ((raw[0] as i32) | ((raw[1] as i32) << 8) | ((raw[2] as i32) << 16)) << 8;
                v as f32 / (1u32 << 31) as f32
            }
            (_, 4) => {
                i32::from_le_bytes(raw.try_into().expect("4 bytes")) as f32 / (1u32 << 31) as f32
            }
            _ => 0.0,
        }
    }
}

impl crate::AudioDecoder for WavDecoder {
    fn info(&self) -> &AudioInfo {
        &self.info
    }

    fn read_samples(&mut self, out: &mut [f32]) -> Result<usize> {
        let bps = self.bytes_per_sample;
        let mut want = out.len().saturating_mul(bps);
        let remaining = self.data_len - self.bytes_read;
        want = want.min(remaining as usize);
        want -= want % bps.max(1);
        if want == 0 {
            return Ok(0);
        }

        let mut bytes = vec![0u8; want];
        self.reader.read_exact(&mut bytes)?;
        self.bytes_read += want as u32;

        for (i, sample) in out.iter_mut().take(want / bps).enumerate() {
            *sample = self.decode_sample(&bytes[i * bps..(i + 1) * bps]);
        }
        Ok(want / bps)
    }
}

/// 16-bit PCM WAV encoder.
pub struct WavEncoder {
    writer: BufWriter<File>,
    bytes_written: u32,
}

impl WavEncoder {
    /// Creates the file and writes a placeholder header (patched by
    /// `finish`).
    ///
    /// # Errors
    /// Returns [`Error`] if the file cannot be created or written.
    pub fn new(path: &Path, spec: AudioSpec) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        let block_align = spec.channels * 2;
        let byte_rate = spec.sample_rate * u32::from(block_align);

        writer.write_all(b"RIFF")?;
        writer.write_all(&[0u8; 4])?; // riff size (patched)
        writer.write_all(b"WAVE")?;
        writer.write_all(b"fmt ")?;
        writer.write_all(&16u32.to_le_bytes())?;
        writer.write_all(&FORMAT_PCM.to_le_bytes())?;
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
        })
    }

    fn sample_to_i16(sample: f32) -> i16 {
        let clamped = sample.clamp(-1.0, 1.0);
        (clamped * 32767.0).round() as i16
    }
}

impl crate::AudioEncoder for WavEncoder {
    fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&Self::sample_to_i16(*s).to_le_bytes());
        }
        self.bytes_written += bytes.len() as u32;
        self.writer.write_all(&bytes)?;
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<()> {
        WavEncoder::finish_impl(*self)
    }
}

impl WavEncoder {
    fn finish_impl(mut self) -> Result<()> {
        self.writer.flush()?;
        let data_len = self.bytes_written;
        let mut file = self.writer.into_inner().map_err(|e| e.into_error())?;
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&(36 + data_len).to_le_bytes())?;
        file.seek(SeekFrom::Start(40))?;
        file.write_all(&data_len.to_le_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{write_test_wav, AudioDecoder, AudioEncoder, AudioSpec};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-cadence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn wav_roundtrip() {
        let path = temp_path("roundtrip.wav");
        write_test_wav(&path, 0.5, 8_000, 2).unwrap();

        let mut dec = WavDecoder::open(&path).unwrap();
        let info = dec.info().clone();
        assert_eq!(info.sample_rate, 8_000);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bit_depth, 16);
        assert!((info.duration_secs - 0.5).abs() < 1e-6);

        let mut all = Vec::new();
        let mut buf = vec![0f32; 333];
        loop {
            let n = dec.read_samples(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        assert_eq!(all.len() as u64, info.total_samples());
        assert!(all.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        assert!(all.iter().any(|s| s.abs() > 0.5), "sine should peak");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn encoder_patches_header() {
        let path = temp_path("encode.wav");
        let mut enc = WavEncoder::new(
            &path,
            AudioSpec {
                sample_rate: 4_000,
                channels: 1,
            },
        )
        .unwrap();
        let samples: Vec<f32> = (0..4_000)
            .map(|i| ((i as f32) * 0.01).sin() * 0.25)
            .collect();
        enc.write_samples(&samples).unwrap();
        Box::new(enc).finish().unwrap();

        let mut dec = WavDecoder::open(&path).unwrap();
        assert_eq!(dec.info().sample_rate, 4_000);
        assert!((dec.info().duration_secs - 1.0).abs() < 1e-6);
        let mut buf = vec![0f32; 4096];
        let n = dec.read_samples(&mut buf).unwrap();
        assert_eq!(n, 4_000);
        std::fs::remove_file(&path).ok();
    }
}
