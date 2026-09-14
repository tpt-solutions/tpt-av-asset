//! Lightweight encoder wrapper.
//!
//! Wraps the `tpt-kinetix` encoder trait (the stand-in's TKV container
//! today; a real lightweight codec once the real crate lands) and adds the
//! resize step proxies need: every source frame is aspect-fit into the
//! profile's target resolution before encoding.

use std::path::Path;

use tpt_av_asset_utils::AssetError;

use crate::profile::ProxyProfile;

/// Frame sink that resizes incoming frames to the target resolution and
/// forwards them to the underlying encoder.
pub struct VideoEncoderSink {
    inner: Box<dyn tpt_kinetix::VideoEncoder>,
    target: (u32, u32),
}

impl VideoEncoderSink {
    /// Opens an encoder at `path` for a source of the given resolution.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for a bad profile and
    /// [`AssetError::Codec`]/[`AssetError::Io`] when the encoder cannot be
    /// created.
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
        let inner = tpt_kinetix::open_encoder(path, target.0, target.1, frame_rate)
            .map_err(|e| AssetError::codec(e.to_string()))?;
        Ok(Self { inner, target })
    }

    /// The resolution frames are encoded at.
    pub fn target(&self) -> (u32, u32) {
        self.target
    }

    /// Resizes `frame` (RGBA8) to the target and encodes it.
    ///
    /// # Errors
    /// Returns [`AssetError::Codec`] for malformed frames or encoder
    /// failures.
    pub fn write_resized(
        &mut self,
        frame: &tpt_kinetix::Frame,
    ) -> Result<(), AssetError> {
        let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())
            .ok_or_else(|| AssetError::codec("decoder returned malformed frame"))?;
        let resized = image::DynamicImage::ImageRgba8(img)
            .resize_exact(self.target.0, self.target.1, image::imageops::FilterType::Triangle);
        let encoded = tpt_kinetix::Frame {
            index: frame.index,
            time_secs: frame.time_secs,
            width: self.target.0,
            height: self.target.1,
            data: resized.to_rgba8().into_raw(),
        };
        self.inner
            .write_frame(&encoded)
            .map_err(|e| AssetError::codec(e.to_string()))
    }

    /// Finalizes the output file.
    ///
    /// # Errors
    /// Returns [`AssetError::Codec`] on encoder failure.
    pub fn finish(self) -> Result<(), AssetError> {
        self.inner
            .finish()
            .map_err(|e| AssetError::codec(e.to_string()))
    }
}
