//! The TPT proxy stream container (`.tkvp`): RGBA8 frames losslessly
//! encoded with `tpt-kinetix-lossless`.
//!
//! ```text
//! offset 0   magic "TPTVPR1"                    (8 bytes)
//! offset 8   width   u32 LE
//! offset 12  height  u32 LE
//! offset 16  fps     f64 LE
//! offset 24  frame_count u32 LE                 (0 until `Writer::finish`)
//! offset 28  version u8 = 1
//! offset 29  transform_id u8 = 0
//! offset 30  plane_count u8 = 3                  (R, G, B)
//! offset 31  bit_depth  u8 = 16
//! offset 32  frames: [u32 LE payload_len][payload]…
//! ```
//!
//! Each payload is a self-contained `tpt-kinetix-lossless` frame (header +
//! plane residuals) with a per-plane checksum, so any frame is decodable
//! independently — random access is a single positioned read.

use std::io::{Seek as _, Write as _};
use std::path::{Path, PathBuf};

use tpt_av_asset_utils::AssetError;
use tpt_kinetix_lossless::headers::{PlaneSpec, SequenceHeader};
use tpt_kinetix_lossless::{LosslessDecoder, LosslessEncoder, Plane};

use crate::error_map;

/// Container magic bytes.
pub const MAGIC: &[u8; 7] = b"TPTVPR1";

/// Fixed header length in bytes.
pub const HEADER_LEN: u64 = 32;

/// Fixed v1 sequence parameters.
pub const VERSION: u8 = 1;
pub const TRANSFORM_ID: u8 = 0;
pub const PLANE_COUNT: usize = 3;
pub const BIT_DEPTH: u8 = 16;

/// 8-bit → 16-bit sample scaling (`v * 257` fills both bytes exactly).
const UPSCALE: u16 = 257;

/// Container header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Header {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Declared frame count (may be 0 in a file still being written).
    pub frame_count: u32,
}

impl Header {
    /// Reads and validates the fixed 32-byte header.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for a bad magic or parameters.
    pub fn read(reader: &mut impl std::io::Read) -> Result<Self, AssetError> {
        let mut raw = [0u8; HEADER_LEN as usize];
        reader.read_exact(&mut raw)?;
        Self::parse(&raw)
    }

    /// Parses the fixed header from its 32-byte representation.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for a bad magic or parameters.
    pub fn parse(raw: &[u8; 32]) -> Result<Self, AssetError> {
        if &raw[0..7] != MAGIC {
            return Err(AssetError::validation("not a TPT proxy stream"));
        }
        let header = Self {
            width: u32::from_le_bytes(raw[8..12].try_into().expect("sized")),
            height: u32::from_le_bytes(raw[12..16].try_into().expect("sized")),
            fps: f64::from_le_bytes(raw[16..24].try_into().expect("sized")),
            frame_count: u32::from_le_bytes(raw[24..28].try_into().expect("sized")),
        };
        if header.width == 0 || header.height == 0 {
            return Err(AssetError::validation("zero-sized proxy stream frame"));
        }
        if !header.fps.is_finite() || header.fps <= 0.0 {
            return Err(AssetError::validation(format!(
                "invalid proxy stream fps {}",
                header.fps
            )));
        }
        if raw[28] != VERSION
            || raw[29] != TRANSFORM_ID
            || raw[30] as usize != PLANE_COUNT
            || raw[31] != BIT_DEPTH
        {
            return Err(AssetError::validation("unsupported proxy stream variant"));
        }
        Ok(header)
    }

    /// The lossless sequence header matching this container.
    #[must_use]
    pub fn sequence(&self) -> SequenceHeader {
        SequenceHeader {
            version: VERSION,
            max_width: self.width as u16,
            max_height: self.height as u16,
            transform_id: TRANSFORM_ID,
            planes: vec![
                PlaneSpec {
                    bit_depth: BIT_DEPTH
                };
                PLANE_COUNT
            ],
        }
    }

    /// Serializes the header to its 32-byte on-disk representation.
    #[must_use]
    pub fn bytes(&self) -> [u8; 32] {
        let mut raw = [0u8; 32];
        raw[0..7].copy_from_slice(MAGIC);
        raw[8..12].copy_from_slice(&self.width.to_le_bytes());
        raw[12..16].copy_from_slice(&self.height.to_le_bytes());
        raw[16..24].copy_from_slice(&self.fps.to_le_bytes());
        raw[24..28].copy_from_slice(&self.frame_count.to_le_bytes());
        raw[28] = VERSION;
        raw[29] = TRANSFORM_ID;
        raw[30] = PLANE_COUNT as u8;
        raw[31] = BIT_DEPTH;
        raw
    }
}

/// Sequential container writer: encodes RGBA frames losslessly and appends
/// them; `frame_count` in the header is patched by [`Writer::finish`].
pub struct Writer {
    file: std::io::BufWriter<std::fs::File>,
    header: Header,
    path: PathBuf,
    frame_count: u32,
    sequence: SequenceHeader,
    encoder: LosslessEncoder,
}

impl Writer {
    /// Creates the file and writes the placeholder header.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] if the file cannot be created.
    pub fn create(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self, AssetError> {
        if width > u16::MAX as u32 || height > u16::MAX as u32 {
            return Err(AssetError::validation(
                "proxy stream dimensions must fit in u16",
            ));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let header = Header {
            width,
            height,
            fps,
            frame_count: 0,
        };
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        file.write_all(&header.bytes())?;
        Ok(Self {
            file,
            header,
            path: path.to_path_buf(),
            frame_count: 0,
            sequence: header.sequence(),
            encoder: LosslessEncoder::new(),
        })
    }

    /// The file being written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Frames written so far.
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Encodes one RGBA8 frame and appends it.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] for a wrong-length buffer and
    /// [`AssetError::Codec`]/[`AssetError::Io`] on encode/write failure.
    pub fn write_frame(&mut self, rgba: &[u8]) -> Result<(), AssetError> {
        let expected = self.header.width as usize * self.header.height as usize * 4;
        if rgba.len() != expected {
            return Err(AssetError::validation(format!(
                "frame data length {} does not match {}x{} RGBA",
                rgba.len(),
                self.header.width,
                self.header.height
            )));
        }
        let payload = self
            .encoder
            .encode_frame(
                &self.sequence,
                &rgba_to_planes(rgba, self.header.width, self.header.height),
            )
            .map_err(error_map::codec)?;
        self.file.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.file.write_all(&payload)?;
        self.frame_count += 1;
        Ok(())
    }

    /// Flushes and patches the frame count in the header.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] on failure.
    pub fn finish(mut self) -> Result<(), AssetError> {
        self.file.flush()?;
        let mut file = self.file.into_inner().map_err(|e| e.into_error())?;
        let mut raw = self.header.bytes();
        raw[24..28].copy_from_slice(&self.frame_count.to_le_bytes());
        file.seek(std::io::SeekFrom::Start(0))?;
        file.write_all(&raw)?;
        Ok(())
    }
}

/// Splits an RGBA8 buffer into three 16-bit planes (R, G, B).
#[must_use]
pub fn rgba_to_planes(rgba: &[u8], width: u32, height: u32) -> Vec<Plane> {
    let pixels = width as usize * height as usize;
    let mut r = vec![0u16; pixels];
    let mut g = vec![0u16; pixels];
    let mut b = vec![0u16; pixels];
    for (i, pixel) in rgba.chunks_exact(4).enumerate() {
        r[i] = u16::from(pixel[0]) * UPSCALE;
        g[i] = u16::from(pixel[1]) * UPSCALE;
        b[i] = u16::from(pixel[2]) * UPSCALE;
    }
    vec![
        Plane {
            width,
            height,
            bit_depth: BIT_DEPTH,
            data: r,
        },
        Plane {
            width,
            height,
            bit_depth: BIT_DEPTH,
            data: g,
        },
        Plane {
            width,
            height,
            bit_depth: BIT_DEPTH,
            data: b,
        },
    ]
}

/// Rebuilds an RGBA8 buffer from three decoded planes.
///
/// # Errors
/// Returns [`AssetError::Codec`] when the plane set is malformed.
pub fn planes_to_rgba(planes: &[Plane], width: u32, height: u32) -> Result<Vec<u8>, AssetError> {
    if planes.len() != PLANE_COUNT {
        return Err(AssetError::codec(format!(
            "expected {PLANE_COUNT} planes, got {}",
            planes.len()
        )));
    }
    let pixels = width as usize * height as usize;
    let mut rgba = vec![0u8; pixels * 4];
    for (channel, plane) in planes.iter().enumerate() {
        if plane.data.len() != pixels {
            return Err(AssetError::codec("decoded plane size mismatch"));
        }
        for (i, &sample) in plane.data.iter().enumerate() {
            rgba[i * 4 + channel] = (sample >> 8) as u8;
        }
    }
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    Ok(rgba)
}

/// Decodes a single frame payload (used by [`crate::video::ProxyStreamSource`]).
pub(crate) fn decode_payload(
    decoder: &mut LosslessDecoder,
    sequence: &SequenceHeader,
    payload: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, AssetError> {
    let planes = decoder
        .decode_frame(sequence, payload)
        .map_err(error_map::codec)?;
    planes_to_rgba(&planes, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tpt-av-asset-cache-container-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn container_roundtrip_preserves_pixels() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("clip.tkvp");
        let (w, h) = (8u32, 6u32);
        let frames: Vec<Vec<u8>> = (0..3u32)
            .map(|i| {
                let mut rgba = vec![0u8; (w * h * 4) as usize];
                for (offset, pixel) in rgba.chunks_exact_mut(4).enumerate() {
                    pixel[0] = (i as u8).wrapping_add(offset as u8);
                    pixel[1] = offset as u8;
                    pixel[2] = 32;
                    pixel[3] = 255;
                }
                rgba
            })
            .collect();

        let mut writer = Writer::create(&path, w, h, 12.0).unwrap();
        for frame in &frames {
            writer.write_frame(frame).unwrap();
        }
        assert_eq!(writer.frame_count(), 3);
        writer.finish().unwrap();

        // Header validation.
        let mut file = std::fs::File::open(&path).unwrap();
        let header = Header::read(&mut file).unwrap();
        assert_eq!(header.width, w);
        assert_eq!(header.height, h);
        assert!((header.fps - 12.0).abs() < f64::EPSILON);
        assert_eq!(header.frame_count, 3);

        // Decode every frame back and compare pixels exactly (lossless).
        let mut decoder = LosslessDecoder::new();
        let sequence = header.sequence();
        file.seek(std::io::SeekFrom::Start(HEADER_LEN)).unwrap();
        for frame in &frames {
            let mut len_bytes = [0u8; 4];
            file.read_exact(&mut len_bytes).unwrap();
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut payload = vec![0u8; len];
            file.read_exact(&mut payload).unwrap();
            let restored = decode_payload(&mut decoder, &sequence, &payload, w, h).unwrap();
            assert_eq!(restored, *frame, "lossless roundtrip must be bit-exact");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_rejects_bad_magic_and_params() {
        let mut raw = [0u8; 32];
        assert!(Header::parse(&raw).is_err());
        raw[0..7].copy_from_slice(MAGIC);
        assert!(Header::parse(&raw).is_err(), "zero dimensions");
        raw[8..12].copy_from_slice(&64u32.to_le_bytes());
        raw[12..16].copy_from_slice(&48u32.to_le_bytes());
        raw[16..24].copy_from_slice(&30.0f64.to_le_bytes());
        raw[28] = VERSION;
        raw[29] = TRANSFORM_ID;
        raw[30] = PLANE_COUNT as u8;
        raw[31] = BIT_DEPTH;
        assert!(Header::parse(&raw).is_ok());
        raw[31] = 12; // unsupported bit depth
        assert!(Header::parse(&raw).is_err());
    }
}
