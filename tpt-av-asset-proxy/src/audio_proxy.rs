//! Audio proxy generation: decode PCM → re-encode → write.

use std::path::Path;

use tpt_av_asset_utils::{AssetError, ProgressReporter};

use crate::profile::ProxyProfile;

/// Samples pulled from the decoder per iteration.
const READ_BLOCK: usize = 8_192;

/// Renders an audio proxy from `source_path` to `output_path`.
///
/// Decodes the source through the `tpt-cadence` trait and re-encodes it.
/// With the stand-in codec the output is 16-bit PCM WAV; the real
/// `tpt-cadence` will produce the profile's codec (e.g. FLAC).
///
/// # Errors
/// Returns [`AssetError::Cancelled`] when `progress` is cancelled,
/// [`AssetError::Codec`] for decode/encode failures.
pub fn generate(
    source_path: &Path,
    output_path: &Path,
    profile: &ProxyProfile,
    progress: &ProgressReporter,
) -> Result<(), AssetError> {
    profile.validate()?;
    let mut decoder =
        tpt_cadence::open(source_path).map_err(|e| AssetError::codec(e.to_string()))?;
    let info = decoder.info().clone();

    let mut encoder = tpt_cadence::open_encoder(
        output_path,
        &tpt_cadence::AudioSpec {
            sample_rate: info.sample_rate,
            channels: info.channels,
        },
    )
    .map_err(|e| AssetError::codec(e.to_string()))?;

    let total = info.total_samples().max(1) as f64;
    let mut done = 0f64;
    let mut block = vec![0f32; READ_BLOCK];

    loop {
        progress.check_cancelled()?;
        let n = decoder
            .read_samples(&mut block)
            .map_err(|e| AssetError::codec(e.to_string()))?;
        if n == 0 {
            break;
        }
        encoder
            .write_samples(&block[..n])
            .map_err(|e| AssetError::codec(e.to_string()))?;
        done += n as f64;
        progress.report_ratio(done, total);
    }

    encoder.finish().map_err(|e| AssetError::codec(e.to_string()))?;
    progress.report(1.0);
    Ok(())
}
