//! High-level proxy generation API.

use std::path::Path;

use tpt_av_asset_utils::{AssetError, ProgressReporter};

use crate::profile::ProxyProfile;

/// Proxy generation engine bound to one [`ProxyProfile`].
#[derive(Debug, Clone)]
pub struct ProxyGenerator {
    profile: ProxyProfile,
}

impl ProxyGenerator {
    /// Creates a new proxy generator.
    pub fn new(profile: ProxyProfile) -> Self {
        Self { profile }
    }

    /// The configured profile.
    pub fn profile(&self) -> &ProxyProfile {
        &self.profile
    }

    /// Generates a proxy for a video file: decode → resize → re-encode →
    /// write. A cancelled or failed run deletes its partial output file, so
    /// `output_path` only ever holds a complete proxy.
    ///
    /// # Errors
    /// Returns [`AssetError::Cancelled`] when cancelled,
    /// [`AssetError::Codec`] on decode/encode failure.
    pub fn generate_video_proxy(
        &self,
        source_path: &Path,
        output_path: &Path,
        progress: &ProgressReporter,
    ) -> Result<(), AssetError> {
        match crate::video_proxy::generate(source_path, output_path, &self.profile, progress) {
            Ok(()) => Ok(()),
            Err(e) => {
                remove_partial(output_path);
                Err(e)
            }
        }
    }

    /// Generates a proxy for an audio file: decode PCM → re-encode → write.
    /// Partial outputs are removed on failure or cancellation.
    ///
    /// # Errors
    /// Returns [`AssetError::Cancelled`] when cancelled,
    /// [`AssetError::Codec`] on decode/encode failure.
    pub fn generate_audio_proxy(
        &self,
        source_path: &Path,
        output_path: &Path,
        progress: &ProgressReporter,
    ) -> Result<(), AssetError> {
        match crate::audio_proxy::generate(source_path, output_path, &self.profile, progress) {
            Ok(()) => Ok(()),
            Err(e) => {
                remove_partial(output_path);
                Err(e)
            }
        }
    }
}

fn remove_partial(output_path: &Path) {
    match std::fs::remove_file(output_path) {
        Ok(()) => log::debug!("proxy: removed partial output {}", output_path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!(
            "proxy: could not remove partial output {}: {e}",
            output_path.display()
        ),
    }
}
