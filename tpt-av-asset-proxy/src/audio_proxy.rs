//! Audio proxy generation: decode PCM through the real `tpt-cadence`
//! readers → write a 16-bit PCM WAV.
//!
//! FLAC re-encoding starts the moment `tpt-cadence` ships its draft
//! `Encoder` trait; until then the proxy is losslessly re-containered PCM.

use std::path::Path;

use tpt_av_asset_utils::{AssetError, ProgressReporter};

use crate::profile::ProxyProfile;
use crate::wav_writer::{WavSpec, WavWriter};

/// Frames pulled from the decoder per iteration.
const READ_FRAMES: usize = 4_096;

/// Renders an audio proxy from `source_path` to `output_path`.
///
/// Decodes the source through the `tpt-cadence` stack (WAV, FLAC) and
/// re-writes it as 16-bit PCM WAV with identical rate/channels/duration.
///
/// # Errors
/// Returns [`AssetError::Cancelled`] when `progress` is cancelled,
/// [`AssetError::Validation`] for a bad profile, and
/// [`AssetError::Codec`] for decode/encode failures.
pub fn generate(
    source_path: &Path,
    output_path: &Path,
    profile: &ProxyProfile,
    progress: &ProgressReporter,
) -> Result<(), AssetError> {
    profile.validate()?;
    let mut stream = tpt_av_asset_cache::audio::open_audio(source_path)?;
    let info = stream.info().clone();

    let mut encoder = WavWriter::new(
        output_path,
        WavSpec {
            sample_rate: info.sample_rate,
            channels: info.channels,
        },
    )?;

    let total_samples = info.total_samples().unwrap_or(0).max(1) as f64;
    let mut done = 0f64;
    let mut block = vec![0f32; READ_FRAMES * usize::from(info.channels.max(1))];

    loop {
        progress.check_cancelled()?;
        let frames = stream.decode(&mut block)?;
        if frames == 0 {
            break;
        }
        encoder.write_samples(&block[..frames * usize::from(info.channels.max(1))])?;
        done += frames as f64;
        progress.report_ratio(done, total_samples);
    }

    encoder.finish()?;
    progress.report(1.0);
    Ok(())
}
