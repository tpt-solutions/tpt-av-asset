//! Proxy profiles: quality presets describing the target of a proxy render.

use tpt_av_asset_utils::AssetError;

/// The video codec video proxies are rendered with today (the only encoder
/// in the kinetix stack).
pub const VIDEO_CODEC: &str = "kinetix-lossless";
/// The audio codec audio proxies target (written as PCM WAV until
/// `tpt-cadence` ships its draft `Encoder`).
pub const AUDIO_CODEC: &str = "flac";

/// Proxy generation profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyProfile {
    /// Profile name (e.g., "1080p Low", "720p Medium").
    pub name: String,
    /// Target resolution. `(0, 0)` for audio-only profiles.
    pub resolution: (u32, u32),
    /// Target frame rate (`None` = same as source).
    pub frame_rate: Option<f64>,
    /// Video codec. The only encoder in the kinetix stack today is
    /// `tpt-kinetix-lossless`, so video proxies target it.
    pub video_codec: String,
    /// Video bit rate in bits per second (informational for the lossless
    /// codec, which has no rate control).
    pub video_bit_rate: u64,
    /// Audio codec. FLAC is the intended target; `tpt-cadence` has not
    /// shipped its draft `Encoder` yet, so audio proxies are currently
    /// written as PCM WAV (see `audio_proxy`).
    pub audio_codec: String,
    /// Audio bit rate in bits per second (0 = lossless).
    pub audio_bit_rate: u64,
}

impl ProxyProfile {
    /// Preset: 1080p low-bitrate proxy — the default for smooth playback of
    /// heavy 4K/8K sources.
    pub fn proxy_1080p_low() -> Self {
        Self {
            name: "1080p Low".to_string(),
            resolution: (1920, 1080),
            frame_rate: None,
            video_codec: VIDEO_CODEC.to_string(),
            video_bit_rate: 2_000_000,
            audio_codec: "aac".to_string(),
            audio_bit_rate: 128_000,
        }
    }

    /// Preset: 720p medium-bitrate proxy.
    pub fn proxy_720p_medium() -> Self {
        Self {
            name: "720p Medium".to_string(),
            resolution: (1280, 720),
            frame_rate: None,
            video_codec: VIDEO_CODEC.to_string(),
            video_bit_rate: 5_000_000,
            audio_codec: "aac".to_string(),
            audio_bit_rate: 192_000,
        }
    }

    /// Preset: audio-only proxy (WAV → FLAC once `tpt-cadence` ships its
    /// draft `Encoder`; currently written as PCM WAV).
    pub fn audio_proxy_flac() -> Self {
        Self {
            name: "Audio Proxy (FLAC)".to_string(),
            resolution: (0, 0),
            frame_rate: None,
            video_codec: String::new(),
            video_bit_rate: 0,
            audio_codec: AUDIO_CODEC.to_string(),
            audio_bit_rate: 0,
        }
    }

    /// True if this profile produces an audio-only proxy.
    pub fn is_audio_only(&self) -> bool {
        self.resolution == (0, 0) || self.video_codec.is_empty()
    }

    /// Canonical output file extension for this profile. Video proxies use
    /// the TPT proxy stream (`.tkvp`); audio proxies use `.wav` until the
    /// cadence FLAC encoder exists.
    pub fn output_extension(&self) -> &'static str {
        if self.is_audio_only() {
            "wav"
        } else {
            "tkvp"
        }
    }

    /// Aspect-preserving target size for a source resolution: the largest
    /// `(w, h)` with even dimensions that fits inside `self.resolution`.
    /// Never upscales.
    pub fn target_size(&self, source_width: u32, source_height: u32) -> (u32, u32) {
        let (max_w, max_h) = (
            u64::from(self.resolution.0.max(1)),
            u64::from(self.resolution.1.max(1)),
        );
        let (src_w, src_h) = (
            u64::from(source_width.max(1)),
            u64::from(source_height.max(1)),
        );
        let scale = (max_w as f64 / src_w as f64)
            .min(max_h as f64 / src_h as f64)
            .min(1.0);
        let w = ((src_w as f64 * scale).floor() as u64).max(2) & !1;
        let h = ((src_h as f64 * scale).floor() as u64).max(2) & !1;
        (w as u32, h as u32)
    }

    /// Validates the profile.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for a non-positive resolution on
    /// video profiles, non-positive bit rates, or an invalid frame rate.
    pub fn validate(&self) -> Result<(), AssetError> {
        if self.is_audio_only() {
            if self.audio_codec.is_empty() {
                return Err(AssetError::validation(
                    "audio proxy profile needs an audio codec",
                ));
            }
        } else {
            if self.resolution.0 == 0 || self.resolution.1 == 0 {
                return Err(AssetError::validation(
                    "video proxy resolution must be positive",
                ));
            }
            if self.video_codec.is_empty() {
                return Err(AssetError::validation(
                    "video proxy profile needs a video codec",
                ));
            }
            if self.video_codec != VIDEO_CODEC {
                return Err(AssetError::validation(format!(
                    "unsupported video codec {}; only {VIDEO_CODEC} is encodable today",
                    self.video_codec
                )));
            }
            if self.video_bit_rate == 0 {
                return Err(AssetError::validation(
                    "video proxy bit rate must be positive",
                ));
            }
        }
        if let Some(fps) = self.frame_rate {
            if !fps.is_finite() || fps <= 0.0 {
                return Err(AssetError::validation(format!("invalid frame rate {fps}")));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_match_spec() {
        let low = ProxyProfile::proxy_1080p_low();
        assert_eq!(low.resolution, (1920, 1080));
        assert_eq!(low.video_bit_rate, 2_000_000);
        assert_eq!(low.audio_bit_rate, 128_000);
        assert!(!low.is_audio_only());
        assert_eq!(low.output_extension(), "tkvp");

        let medium = ProxyProfile::proxy_720p_medium();
        assert_eq!(medium.resolution, (1280, 720));
        assert_eq!(medium.video_bit_rate, 5_000_000);

        let flac = ProxyProfile::audio_proxy_flac();
        assert!(flac.is_audio_only());
        assert_eq!(flac.audio_codec, "flac");
        assert_eq!(flac.output_extension(), "wav");
        assert!(flac.validate().is_ok());
        assert!(low.validate().is_ok());
        assert!(medium.validate().is_ok());
    }

    #[test]
    fn target_size_preserves_aspect_ratio() {
        let profile = ProxyProfile::proxy_1080p_low();

        // 16:9 4K → exactly 1080p.
        assert_eq!(profile.target_size(3840, 2160), (1920, 1080));
        // Already smaller than target: never upscale.
        assert_eq!(profile.target_size(1280, 720), (1280, 720));
        // 4:3 source letterboxes inside 16:9 target.
        assert_eq!(profile.target_size(1440, 1080), (1440, 1080));
        // Vertical video: fit by height (607.5 floors to 607, evened to 606).
        let (w, h) = profile.target_size(2160, 3840);
        assert_eq!(h, 1080);
        assert_eq!(w, 606);
    }

    #[test]
    fn validation_rejects_bad_profiles() {
        // (0, 1080) stays a video profile but has an invalid width; a full
        // (0, 0) resolution would reclassify the profile as audio-only.
        let mut bad = ProxyProfile::proxy_1080p_low();
        bad.resolution = (0, 1080);
        assert!(bad.validate().is_err());
        assert!(!bad.is_audio_only());

        let mut bad = ProxyProfile::proxy_1080p_low();
        bad.frame_rate = Some(-1.0);
        assert!(bad.validate().is_err());

        let mut bad = ProxyProfile::proxy_1080p_low();
        bad.video_bit_rate = 0;
        assert!(bad.validate().is_err());

        let mut bad = ProxyProfile::proxy_1080p_low();
        bad.video_codec = "h264".into(); // no H.264 encoder exists yet
        assert!(bad.validate().is_err());
    }
}
