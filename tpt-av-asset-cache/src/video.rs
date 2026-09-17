//! Video decoding: the [`VideoSource`] abstraction and its two backends.
//!
//! - **MP4/H.264** through the real `tpt-kinetix` stack
//!   ([`tpt_kinetix_demux::Mp4Demuxer`] + [`tpt_kinetix_h264::H264Decoder`]),
//!   with AVCC→Annex-B conversion and BT.601 YUV420p→RGBA in between.
//!   Limitation: the kinetix MP4 demuxer does not yet surface `avcC`
//!   extradata, so SPS/PPS must be carried in-band (as muxed by
//!   `tpt-av-asset-test-media::mux_annexb_to_mp4` and many muxers that
//!   repeat parameter sets per keyframe).
//! - **TPT proxy stream** (`.tkvp`): RGBA frames losslessly encoded with
//!   `tpt-kinetix-lossless` — the engine's own proxy format, fully
//!   supported for decode today (see [`container`]).
//!
//! [`open_video`] sniffs the container and returns the matching source.

use std::collections::VecDeque;

use crate::container;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

use tpt_av_asset_utils::{AssetError, VideoInfo};
use tpt_kinetix_demux::Demuxer as _;

use crate::error_map;

/// A decoded video frame in RGBA8.
#[derive(Debug, Clone)]
pub struct RgbaFrame {
    /// Zero-based display index.
    pub index: u64,
    /// Presentation timestamp in seconds.
    pub time_secs: f64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixels, `width * height * 4` bytes.
    pub data: Vec<u8>,
}

/// Random-access video frame source.
///
/// `seek_to` positions at the first frame at or after `time_secs`;
/// `next_frame` returns `None` at end of stream. Implementations must be
/// [`Send`] so generators can run on pipeline workers.
pub trait VideoSource: Send {
    /// Stream metadata.
    fn info(&self) -> &VideoInfo;

    /// Positions the decoder at the first frame at or after `time_secs`.
    fn seek_to(&mut self, time_secs: f64);

    /// Decodes and returns the next frame, or `None` at end of stream.
    ///
    /// # Errors
    /// Returns [`AssetError::Codec`] on decode failure.
    fn next_frame(&mut self) -> Result<Option<RgbaFrame>, AssetError>;
}

/// Opens a video file, sniffing for MP4 (ftyp) and the TPT proxy stream
/// (TPTVPR1) containers.
///
/// # Errors
/// Returns [`AssetError::UnsupportedFormat`] for unrecognized containers
/// and [`AssetError::Validation`] for unsupported codecs inside a
/// recognized container.
pub fn open_video(path: &Path) -> Result<Box<dyn VideoSource>, AssetError> {
    let mut magic = [0u8; 12];
    let mut file = std::fs::File::open(path)?;
    let read = file.read(&mut magic)?;
    let recognized = if read >= 7 && &magic[0..7] == container::MAGIC {
        Some(SourceKind::ProxyStream)
    } else if read >= 12 && &magic[4..8] == b"ftyp" {
        Some(SourceKind::Mp4H264)
    } else {
        None
    };
    drop(file);

    match recognized {
        Some(SourceKind::ProxyStream) => Ok(Box::new(ProxyStreamSource::open(path)?)),
        Some(SourceKind::Mp4H264) => Ok(Box::new(Mp4H264Source::open(path)?)),
        None => Err(AssetError::UnsupportedFormat(path.to_path_buf())),
    }
}

enum SourceKind {
    Mp4H264,
    ProxyStream,
}

/// Probes a video file's metadata.
///
/// # Errors
/// Same as [`open_video`].
pub fn probe_video(path: &Path) -> Result<VideoInfo, AssetError> {
    Ok(open_video(path)?.info().clone())
}

// ---------------------------------------------------------------------------
// MP4 + H.264 backend
// ---------------------------------------------------------------------------

struct Mp4H264Source {
    demuxer: tpt_kinetix_demux::Mp4Demuxer,
    decoder: tpt_kinetix_h264::H264Decoder,
    info: VideoInfo,
    video_index: usize,
    buffered: VecDeque<RgbaFrame>,
    next_index: u64,
    exhausted: bool,
    seek_target: Option<f64>,
}

impl Mp4H264Source {
    fn open(path: &Path) -> Result<Self, AssetError> {
        let data = std::fs::read(path)?;
        let demuxer = tpt_kinetix_demux::Mp4Demuxer::new(data)
            .map_err(|e| AssetError::codec(format!("mp4 demux failed: {e}")))?;

        let video_index = demuxer
            .tracks()
            .iter()
            .position(|t| t.media_type == tpt_kinetix_core::codec::MediaType::Video)
            .ok_or_else(|| {
                AssetError::validation(format!("{} has no video track", path.display()))
            })?;
        let track = &demuxer.tracks()[video_index];
        if track.codec != Some(tpt_kinetix_core::codec::CodecId::H264) {
            return Err(AssetError::validation(format!(
                "{} uses {:?}; only H.264 tracks are supported",
                path.display(),
                track.codec
            )));
        }

        let duration_secs = if track.timescale > 0 {
            track.duration as f64 / f64::from(track.timescale)
        } else {
            0.0
        };
        let frame_count = track.sample_count() as u32;
        let frame_rate = if duration_secs > 0.0 {
            f64::from(frame_count) / duration_secs
        } else {
            30.0
        };

        let info = VideoInfo {
            width: track.width,
            height: track.height,
            frame_rate,
            codec: "h264".to_string(),
            pixel_format: "yuv420p".to_string(),
            bit_rate: None,
            frame_count,
            duration_secs,
        };

        Ok(Self {
            demuxer,
            decoder: tpt_kinetix_h264::H264Decoder::new(),
            info,
            video_index,
            buffered: VecDeque::new(),
            next_index: 0,
            exhausted: false,
            seek_target: None,
        })
    }

    /// Pulls packets until at least one frame is buffered (or the stream
    /// ends), converting AVCC samples to Annex-B for the decoder and
    /// YUV420p planes to RGBA.
    fn fill(&mut self) -> Result<(), AssetError> {
        while self.buffered.is_empty() && !self.exhausted {
            let packet = match self.demuxer.read_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    self.exhausted = true;
                    return Ok(());
                }
                Err(e) => return Err(error_map::codec(e)),
            };
            if packet.stream_index != self.video_index as u32 {
                continue;
            }
            let annexb = avcc_to_annexb(&packet.data)?;
            let decoded = tpt_kinetix_core::Packet {
                pts: packet.pts,
                dts: packet.dts,
                data: annexb,
                stream_index: packet.stream_index,
                is_key_frame: packet.is_key_frame,
            };
            let frame = self.decoder.decode(&decoded).map_err(error_map::codec)?;
            let Some(frame) = frame else { continue };
            if frame.pixel_format != tpt_kinetix_core::PixelFormat::Yuv420p {
                return Err(AssetError::codec(format!(
                    "unexpected pixel format {:?}",
                    frame.pixel_format
                )));
            }
            let index = self.next_index;
            self.next_index += 1;
            let time_secs = if !frame.pts.is_none() && frame.pts.as_secs_f64().is_finite() {
                frame.pts.as_secs_f64()
            } else {
                index as f64 / self.info.frame_rate.max(1e-9)
            };
            self.buffered.push_back(RgbaFrame {
                index,
                time_secs,
                width: frame.width,
                height: frame.height,
                data: yuv420p_to_rgba(&frame.data, frame.width, frame.height)?,
            });
        }
        Ok(())
    }
}

impl VideoSource for Mp4H264Source {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn seek_to(&mut self, time_secs: f64) {
        let clamped = time_secs.max(0.0);
        let _ = self.demuxer.seek((clamped * 1000.0) as i64);
        // The decoder keeps reference state; a seek must start from a clean
        // slate. Keyframe access units re-initialize it in-band.
        self.decoder = tpt_kinetix_h264::H264Decoder::new();
        self.buffered.clear();
        self.next_index = (clamped * self.info.frame_rate).round() as u64;
        self.exhausted = false;
        self.seek_target = Some(clamped);
    }

    fn next_frame(&mut self) -> Result<Option<RgbaFrame>, AssetError> {
        loop {
            self.fill()?;
            match self.buffered.pop_front() {
                Some(frame) => {
                    if let Some(target) = self.seek_target {
                        let half_frame = 0.5 / self.info.frame_rate.max(1e-9);
                        if frame.time_secs + half_frame < target {
                            continue; // still before the seek target: drop
                        }
                        self.seek_target = None;
                    }
                    return Ok(Some(frame));
                }
                None => {
                    if self.exhausted {
                        self.seek_target = None;
                        return Ok(None);
                    }
                }
            }
        }
    }
}

/// Converts one AVCC sample (4-byte big-endian length-prefixed NAL units)
/// into an Annex-B access unit (00 00 00 01 start codes).
fn avcc_to_annexb(data: &[u8]) -> Result<Vec<u8>, AssetError> {
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().expect("sized")) as usize;
        i += 4;
        if i + len > data.len() {
            return Err(AssetError::codec("truncated AVCC sample"));
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[i..i + len]);
        i += len;
    }
    if out.is_empty() {
        return Err(AssetError::codec("empty AVCC sample"));
    }
    Ok(out)
}

/// BT.601 limited-range YUV420p (contiguous Y, Cb, Cr planes) → RGBA8.
fn yuv420p_to_rgba(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, AssetError> {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (y_len, c_len) = (w * h, cw * ch);
    if data.len() < y_len + 2 * c_len {
        return Err(AssetError::codec(format!(
            "YUV420p frame too small: {} bytes for {width}x{height}",
            data.len()
        )));
    }
    let (y_plane, rest) = data.split_at(y_len);
    let (cb_plane, cr_plane) = rest.split_at(c_len);

    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = i32::from(y_plane[row * w + col]) - 16;
            let d = i32::from(cb_plane[(row / 2) * cw + (col / 2)]) - 128;
            let e = i32::from(cr_plane[(row / 2) * cw + (col / 2)]) - 128;

            let r = (298 * y + 409 * e + 128) >> 8;
            let g = (298 * y - 100 * d - 208 * e + 128) >> 8;
            let b = (298 * y + 516 * d + 128) >> 8;

            let offset = (row * w + col) * 4;
            rgba[offset] = r.clamp(0, 255) as u8;
            rgba[offset + 1] = g.clamp(0, 255) as u8;
            rgba[offset + 2] = b.clamp(0, 255) as u8;
            rgba[offset + 3] = 255;
        }
    }
    Ok(rgba)
}

// ---------------------------------------------------------------------------
// TPT proxy stream backend
// ---------------------------------------------------------------------------

struct ProxyStreamSource {
    reader: std::fs::File,
    offsets: Vec<(u64, u64)>, // (payload offset, payload length)
    decoder: tpt_kinetix_lossless::LosslessDecoder,
    sequence: tpt_kinetix_lossless::headers::SequenceHeader,
    info: VideoInfo,
    header: container::Header,
    next_index: u64,
    seek_target: Option<f64>,
}

impl ProxyStreamSource {
    fn open(path: &Path) -> Result<Self, AssetError> {
        let mut file = std::fs::File::open(path)?;
        let header = container::Header::read(&mut file)
            .map_err(|e| AssetError::codec(format!("bad proxy stream {}: {e}", path.display())))?;
        let file_len = file.metadata()?.len();
        if file_len < container::HEADER_LEN {
            return Err(AssetError::codec(format!(
                "proxy stream {} is shorter than its header",
                path.display()
            )));
        }
        // Sequential scan builds the frame offset table. The declared
        // frame_count and per-frame lengths are attacker-controlled; the
        // shared scan bounds its allocations by the real file size and
        // degrades to the valid prefix on truncation, which in turn bounds
        // every per-frame allocation in `decode_at`.
        let offsets = container::scan_frame_table(&mut file, file_len, header.frame_count);

        let sequence = header.sequence();
        let info = VideoInfo {
            width: header.width,
            height: header.height,
            frame_rate: header.fps,
            codec: "kinetix-lossless".to_string(),
            pixel_format: "rgba8".to_string(),
            bit_rate: None,
            frame_count: offsets.len() as u32,
            duration_secs: offsets.len() as f64 / header.fps.max(1e-9),
        };

        Ok(Self {
            reader: file,
            offsets,
            decoder: tpt_kinetix_lossless::LosslessDecoder::new(),
            sequence,
            info,
            header,
            next_index: 0,
            seek_target: None,
        })
    }

    fn decode_at(&mut self, index: u64) -> Result<Option<RgbaFrame>, AssetError> {
        let Some(&(offset, len)) = self.offsets.get(index as usize) else {
            return Ok(None);
        };
        self.reader.seek(SeekFrom::Start(offset))?;
        let mut payload = vec![0u8; len as usize];
        self.reader.read_exact(&mut payload)?;

        let data = container::decode_payload(
            &mut self.decoder,
            &self.sequence,
            &payload,
            self.header.width,
            self.header.height,
        )?;

        Ok(Some(RgbaFrame {
            index,
            time_secs: index as f64 / self.header.fps.max(1e-9),
            width: self.header.width,
            height: self.header.height,
            data,
        }))
    }
}

impl VideoSource for ProxyStreamSource {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn seek_to(&mut self, time_secs: f64) {
        let clamped = time_secs.max(0.0);
        self.next_index = (clamped * self.header.fps).round() as u64;
        self.seek_target = Some(clamped);
    }

    fn next_frame(&mut self) -> Result<Option<RgbaFrame>, AssetError> {
        loop {
            match self.decode_at(self.next_index)? {
                Some(frame) => {
                    self.next_index += 1;
                    if let Some(target) = self.seek_target {
                        let half_frame = 0.5 / self.header.fps.max(1e-9);
                        if frame.time_secs + half_frame < target {
                            continue;
                        }
                        self.seek_target = None;
                    }
                    return Ok(Some(frame));
                }
                None => {
                    self.seek_target = None;
                    return Ok(None);
                }
            }
        }
    }
}
