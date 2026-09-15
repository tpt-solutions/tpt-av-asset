//! Lightweight video encoder wrapper.
//!
//! Wraps the proxy-stream container writer
//! ([`tpt_av_asset_cache::container::Writer`]), which encodes every
//! frame with `tpt-kinetix-lossless` — the one video encoder in the kinetix
//! stack today — and adds the resize step proxies need: every source frame
//! is aspect-fit into the profile's target resolution before encoding.

use std::path::Path;

use tpt_av_asset_cache::container::Writer as ContainerWriter;
use tpt_av_asset_utils::AssetError;

use crate::profile::ProxyProfile;

/// Frame sink that resizes incoming frames to the target resolution and
/// encodes them into the proxy stream.
pub struct VideoEncoderSink {
    inner: ContainerWriter,
    target: (u32, u32),
}

impl VideoEncoderSink {
    /// Opens an encoder at `path` for a source of the given resolution.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for a bad profile and I/O errors
    /// when the output cannot be created.
    pub fn open(
        path: &Path,
        profile: &ProxyProfile,
        source_width: u32,
        source_height: u32,
        effective_frame_rate: f64,
    ) -> Result<Self, AssetError> {
        profile.validate()?;
        let target = profile.target_size(source_width, source_height);
        let frame_rate = profile.frame_rate.unwrap_or(effective_frame_rate);
        let inner = ContainerWriter::create(path, target.0, target.1, frame_rate)?;
        Ok(Self { inner, target })
    }

    /// The resolution frames are encoded at.
    pub fn target(&self) -> (u32, u32) {
        self.target
    }

    /// Frames encoded so far.
    pub fn frame_count(&self) -> u32 {
        self.inner.frame_count()
    }

    /// Resizes `frame` (RGBA8) to the target and encodes it.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for malformed frames and
    /// [`AssetError::Codec`]/[`AssetError::Io`] on encode failure.
    pub fn write_resized(
        &mut self,
        frame: &tpt_av_asset_cache::video::RgbaFrame,
    ) -> Result<(), AssetError> {
        let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())
            .ok_or_else(|| AssetError::codec("decoder returned malformed frame"))?;
        let resized = image::DynamicImage::ImageRgba8(img).resize_exact(
            self.target.0,
            self.target.1,
            image::imageops::FilterType::Triangle,
        );
        self.inner.write_frame(&resized.to_rgba8().into_raw())
    }

    /// Finalizes the output file (patches the frame count).
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] on failure.
    pub fn finish(self) -> Result<(), AssetError> {
        self.inner.finish()
    }
}
