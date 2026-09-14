//! Video proxy generation: decode → resize → re-encode → write.

use std::path::Path;

use tpt_av_asset_utils::{AssetError, ProgressReporter};

use crate::encoder::VideoEncoderSink;
use crate::profile::ProxyProfile;

/// Renders a video proxy from `source_path` to `output_path`.
///
/// Every source frame is decoded, aspect-fit into the profile's target
/// resolution, and re-encoded. When the profile sets a target frame rate,
/// frames are decimated (never duplicated) to approach it.
///
/// # Errors
/// Returns [`AssetError::Cancelled`] when `progress` is cancelled (partial
/// output cleanup is the caller's responsibility — see
/// [`crate::generator::ProxyGenerator`]), [`AssetError::Codec`] for decode/
/// encode failures, and [`AssetError::Validation`] for a bad profile.
pub fn generate(
    source_path: &Path,
    output_path: &Path,
    profile: &ProxyProfile,
    progress: &ProgressReporter,
) -> Result<(), AssetError> {
    profile.validate()?;

    let mut decoder =
        tpt_kinetix::open(source_path).map_err(|e| AssetError::codec(e.to_string()))?;
    let info = decoder.info().clone();

    let source_fps = if info.frame_rate > 0.0 {
        info.frame_rate
    } else {
        30.0
    };
    let mut sink = VideoEncoderSink::open(output_path, profile, info.width, info.height, source_fps)?;
    let target_fps = profile.frame_rate.unwrap_or(source_fps);

    let total = u64::from(info.frame_count.max(1));
    let mut written = 0u64;
    let mut index = 0u64;
    let mut last_bucket: i64 = -1;

    loop {
        progress.check_cancelled()?;
        match decoder
            .next_frame()
            .map_err(|e| AssetError::codec(e.to_string()))?
        {
            Some(frame) => {
                // Frame decimation: keep the first source frame of each
                // target-timestamp bucket (never duplicates, never reorders).
                let keep = if profile.frame_rate.is_some() {
                    let bucket = (index as f64 * target_fps / source_fps).floor() as i64;
                    if bucket != last_bucket {
                        last_bucket = bucket;
                        true
                    } else {
                        false
                    }
                } else {
                    true
                };
                if keep {
                    sink.write_resized(&frame)?;
                    written += 1;
                    progress.report_ratio(written.min(total) as f64, total as f64);
                }
                index += 1;
            }
            None => break,
        }
    }

    sink.finish()?;
    progress.report(1.0);
    Ok(())
}
