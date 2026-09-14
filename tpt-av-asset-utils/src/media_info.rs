//! Media metadata types.

use std::path::{Path, PathBuf};

use crate::asset_id::AssetId;

/// The kind of media a file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    /// Audio-only file (music, podcast, stem).
    Audio,
    /// Video file (possibly with an audio track).
    Video,
    /// Still image.
    Image,
}

impl MediaType {
    /// Stable wire tag (used by the database encoding).
    pub fn as_u8(self) -> u8 {
        match self {
            MediaType::Audio => 0,
            MediaType::Video => 1,
            MediaType::Image => 2,
        }
    }

    /// Inverse of [`MediaType::as_u8`]; returns `None` for unknown tags.
    pub fn from_u8(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(MediaType::Audio),
            1 => Some(MediaType::Video),
            2 => Some(MediaType::Image),
            _ => None,
        }
    }
}

/// Media metadata extracted from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    /// Asset identifier.
    pub id: AssetId,
    /// File path.
    pub path: PathBuf,
    /// File name.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// Media type (audio, video, image).
    pub media_type: MediaType,
    /// Duration in seconds (if applicable).
    pub duration_secs: Option<f64>,
    /// Video-specific info.
    pub video: Option<VideoInfo>,
    /// Audio-specific info.
    pub audio: Option<AudioInfo>,
}

impl MediaInfo {
    /// Builds the file-independent parts of a [`MediaInfo`] for `path`.
    pub fn new(id: AssetId, path: &Path, media_type: MediaType) -> Self {
        Self {
            id,
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: id.size(),
            media_type,
            duration_secs: None,
            video: None,
            audio: None,
        }
    }

    /// True for [`MediaType::Video`].
    pub fn is_video(&self) -> bool {
        self.media_type == MediaType::Video
    }

    /// True for [`MediaType::Audio`].
    pub fn is_audio(&self) -> bool {
        self.media_type == MediaType::Audio
    }
}

/// Video-specific metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frame rate (e.g., 24.0, 29.97, 30.0, 60.0).
    pub frame_rate: f64,
    /// Codec name (e.g., "h264", "av1", "prores").
    pub codec: String,
    /// Pixel format (e.g., "yuv420p", "rgb24").
    pub pixel_format: String,
    /// Bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// Total frame count.
    pub frame_count: u32,
    /// Duration in seconds.
    pub duration_secs: f64,
}

impl VideoInfo {
    /// True if the other video is bit-identical to this one.
    pub fn equivalent(&self, other: &VideoInfo) -> bool {
        self == other
    }
}

/// Audio-specific metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Bit depth.
    pub bit_depth: u16,
    /// Codec name (e.g., "pcm", "aac", "opus").
    pub codec: String,
    /// Bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// Duration in seconds.
    pub duration_secs: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_tags_roundtrip() {
        for tag in 0..3u8 {
            let t = MediaType::from_u8(tag).unwrap();
            assert_eq!(t.as_u8(), tag);
        }
        assert!(MediaType::from_u8(3).is_none());
    }

    #[test]
    fn media_info_new_extracts_name() {
        let id = AssetId::from_parts(1, 2, 3);
        let info = MediaInfo::new(id, Path::new("/media/clip.wav"), MediaType::Audio);
        assert_eq!(info.name, "clip.wav");
        assert_eq!(info.size, 3);
        assert!(info.is_audio() && !info.is_video());
        assert!(info.duration_secs.is_none());
        assert!(info.video.is_none() && info.audio.is_none());
    }
}
