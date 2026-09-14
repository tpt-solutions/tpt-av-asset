//! Synthetic test-media generation helpers.

use std::path::Path;

use crate::wav::WavEncoder;
use crate::{AudioEncoder, AudioSpec, Result};

/// Writes a synthetic PCM WAV file: a 440 Hz sine with a slow amplitude
/// envelope, so waveform chunks have meaningful, varying peaks.
///
/// # Errors
/// Returns [`Error`] if the file cannot be created or written.
pub fn write_test_wav(
    path: &Path,
    duration_secs: f64,
    sample_rate: u32,
    channels: u16,
) -> Result<()> {
    let mut enc: Box<dyn AudioEncoder> =
        Box::new(WavEncoder::new(path, AudioSpec { sample_rate, channels })?);
    let total = (duration_secs * sample_rate as f64) as u64;
    const BLOCK: usize = 4_096;
    let mut block = Vec::with_capacity(BLOCK);
    let mut i = 0u64;
    while i < total {
        block.clear();
        let end = (i + BLOCK as u64).min(total);
        for n in i..end {
            let t = n as f64 / sample_rate as f64;
            // Sine with a slow tremolo so peak values vary between chunks.
            let envelope = 0.4 + 0.2 * (2.0 * std::f64::consts::PI * 0.7 * t).sin();
            let sample = (2.0 * std::f64::consts::PI * 440.0 * t).sin() * envelope;
            for _ in 0..channels {
                block.push(sample as f32);
            }
        }
        enc.write_samples(&block)?;
        i = end;
    }
    enc.finish()?;
    Ok(())
}
