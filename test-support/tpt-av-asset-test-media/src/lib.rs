//! Synthetic test-media helpers for the TPT AV asset stack.
//!
//! The real `tpt-cadence` / `tpt-kinetix` crates ship decoders (kinetix also
//! has a lossless encoder), but there is no way to *author* source media with
//! them yet — so tests and demos need helpers that write real files the
//! decoders accept:
//!
//! - [`write_test_wav`] — a PCM WAV the cadence WAV decoder reads.
//! - [`write_proxy_video`] — a TPT proxy-stream video (RGBA frames encoded
//!   with `tpt-kinetix-lossless`) that `tpt-av-asset-cache::video` opens.
//! - [`mux_annexb_to_mp4`] / [`generate_h264_testsrc_mp4`] — real H.264-in-MP4
//!   fixtures for the kinetix demux+decode path. Requires `ffmpeg`; every
//!   consumer must handle `None`.
//!
//! The `ffmpeg`-gated helpers follow the same convention as the kinetix and
//! cadence test suites: skip gracefully when the tool is unavailable.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use tpt_av_asset_cache::container::Writer as ProxyWriter;

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

/// Writes a synthetic PCM WAV file: a 440 Hz sine with a slow amplitude
/// envelope, so waveform chunks have meaningful, varying peaks.
///
/// # Errors
/// Returns [`std::io::Error`] if the file cannot be created or written.
pub fn write_test_wav(
    path: &Path,
    duration_secs: f64,
    sample_rate: u32,
    channels: u16,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut file = std::fs::File::create(path)?;
    let data_len =
        (duration_secs * f64::from(sample_rate)).round() as u32 * u32::from(channels) * 2;
    let block_align = channels * 2;
    let byte_rate = sample_rate * u32::from(block_align);

    // 44-byte canonical PCM header, sizes patched up front (we know the count).
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?; // PCM
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;

    const BLOCK: usize = 4_096;
    let mut block: Vec<u8> = Vec::with_capacity(BLOCK * 2);
    let total = u64::from(data_len / 2);
    let mut i = 0u64;
    while i < total {
        block.clear();
        let end = (i + BLOCK as u64).min(total);
        for n in i..end {
            let sample_index = n / u64::from(channels);
            let t = sample_index as f64 / f64::from(sample_rate);
            // Sine with a slow tremolo so peak values vary between chunks.
            let envelope = 0.4 + 0.2 * (2.0 * std::f64::consts::PI * 0.7 * t).sin();
            let sample = (2.0 * std::f64::consts::PI * 440.0 * t).sin() * envelope;
            let value = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
            block.extend_from_slice(&value.to_le_bytes());
        }
        i = end;
        file.write_all(&block)?;
    }
    file.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Video: TPT proxy stream
// ---------------------------------------------------------------------------

/// A tiny painter used by demos/tests: a red/green gradient with a moving
/// white vertical band, so consecutive frames differ.
pub fn gradient_painter(index: u32, width: u32, height: u32, rgba: &mut [u8]) {
    let band_x = (index as usize * 3) % width as usize;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = (y * width as usize + x) * 4;
            rgba[offset] = (x * 255 / width.max(1) as usize) as u8;
            rgba[offset + 1] = (y * 255 / height.max(1) as usize) as u8;
            rgba[offset + 2] = 32;
            rgba[offset + 3] = 255;
            if x == band_x || x == band_x + 1 {
                rgba[offset] = 255;
                rgba[offset + 1] = 255;
                rgba[offset + 2] = 255;
            }
        }
    }
}

/// Writes a TPT proxy-stream video: `frame_count` RGBA frames painted by
/// `painter`, losslessly encoded with `tpt-kinetix-lossless` via the cache
/// crate's container writer. The result opens with
/// `tpt_av_asset_cache::video::open_video`.
///
/// # Errors
/// Returns a descriptive error string if the file cannot be created,
/// written, or a frame fails to encode.
pub fn write_proxy_video(
    path: &Path,
    width: u32,
    height: u32,
    fps: f64,
    frame_count: u32,
    mut painter: impl FnMut(u32, u32, u32, &mut [u8]),
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut writer = ProxyWriter::create(path, width, height, fps).map_err(|e| e.to_string())?;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for index in 0..frame_count {
        painter(index, width, height, &mut rgba);
        writer.write_frame(&rgba).map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// H.264 fixtures (ffmpeg-gated)
// ---------------------------------------------------------------------------

/// True when `ffmpeg` is on `PATH` (probed once per process).
pub fn ffmpeg_available() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHE: AtomicU8 = AtomicU8::new(0);
    match CACHE.load(Ordering::Relaxed) {
        0 => {
            let ok = Command::new("ffmpeg")
                .arg("-version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            CACHE.store(if ok { 2 } else { 1 }, Ordering::Relaxed);
            ok
        }
        2 => true,
        _ => false,
    }
}

/// Generates a short synthetic clip (`testsrc`) with ffmpeg as H.264
/// Annex-B, all-intra, CAVLC, yuv420p — the profile the kinetix H.264
/// decoder handles. Returns `None` when ffmpeg is unavailable (callers
/// should skip the test, exactly like the kinetix conformance suites).
#[must_use]
pub fn generate_h264_testsrc_annexb(
    dir: &Path,
    width: u32,
    height: u32,
    frames: u32,
    fps: u32,
) -> Option<Vec<u8>> {
    if !ffmpeg_available() {
        return None;
    }
    let out = dir.join(format!("testsrc_{width}x{height}_{frames}f.h264"));
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size={width}x{height}:rate={fps}"),
            "-frames:v",
            &frames.to_string(),
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-g",
            "1",
            "-bf",
            "0",
            "-pix_fmt",
            "yuv420p",
            "-x264-params",
            "cabac=0:8x8dct=0:ref=1:bframes=0:weightp=0:aud=0",
            &out.to_string_lossy(),
        ])
        .output();
    match status {
        Ok(o) if o.status.success() => std::fs::read(&out).ok(),
        _ => None,
    }
}

/// Muxes an H.264 Annex-B elementary stream into a minimal MP4 (single
/// `avc1` video track) using `tpt-kinetix-mux`.
///
/// SPS/PPS are written into **every keyframe access unit** (in-band) because
/// the kinetix MP4 demuxer does not yet surface the `avcC` extradata; the
/// decoder needs them in-band to initialize.
///
/// # Errors
/// Returns a descriptive error string for malformed streams.
pub fn mux_annexb_to_mp4(
    annexb: &[u8],
    width: u32,
    height: u32,
    fps: u32,
) -> Result<Vec<u8>, String> {
    let nals = split_annexb(annexb);

    // Group NALs into access units: an AU ends at a slice NAL (1/5).
    let mut units: Vec<(Vec<u8>, bool)> = Vec::new(); // (avcc payload, is_key)
    let mut pending_sps_pps: Vec<u8> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let mut saw_sps = false;
    let mut saw_pps = false;
    let mut sps: Vec<u8> = Vec::new();
    let mut pps: Vec<u8> = Vec::new();

    for nal in nals {
        match nal[0] & 0x1F {
            7 => {
                sps = nal.clone();
                saw_sps = true;
                pending_sppfully(&mut pending_sps_pps, &nal);
            }
            8 => {
                pps = nal.clone();
                saw_pps = true;
                pending_sppfully(&mut pending_sps_pps, &nal);
            }
            5 | 1 => {
                if !saw_sps || !saw_pps {
                    return Err("stream contains a slice before SPS/PPS".into());
                }
                current.extend_from_slice(&pending_sps_pps);
                pending_sps_pps.clear();
                current.extend_from_slice(&avcc_len(&nal));
                let is_key = nal[0] & 0x1F == 5;
                units.push((std::mem::take(&mut current), is_key));
                // Re-emit SPS/PPS with every access unit so seeks re-init.
                pending_sppfully(&mut pending_sps_pps, &sps);
                pending_sppfully(&mut pending_sps_pps, &pps);
                saw_sps = false;
                saw_pps = false;
            }
            _ => {}
        }
    }
    if units.is_empty() {
        return Err("no access units found in stream".into());
    }

    let mut muxer = tpt_kinetix_mux::Mp4Muxer::new(tpt_kinetix_mux::Mp4MuxerConfig {
        width: width as u16,
        height: height as u16,
        timescale: fps,
        sps,
        pps,
    });
    let duration_per_frame = fps;
    for (payload, is_key) in &units {
        muxer.write_sample(payload, duration_per_frame, *is_key);
    }
    Ok(muxer.finish())
}

fn pending_sppfully(pending: &mut Vec<u8>, nal: &[u8]) {
    pending.extend_from_slice(&avcc_len(nal));
}

fn avcc_len(nal: &[u8]) -> [u8; 4] {
    let len = nal.len() as u32;
    len.to_be_bytes()
}

/// Splits an Annex-B stream into NAL units (start codes stripped).
fn split_annexb(data: &[u8]) -> Vec<Vec<u8>> {
    let mut nals = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    let mut i = 0;
    while i < data.len() {
        if i + 3 <= data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            if let Some(nal) = current.take() {
                if !nal.is_empty() {
                    nals.push(nal);
                }
            }
            current = Some(Vec::new());
            i += 3;
        } else if i + 4 <= data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1
        {
            if let Some(nal) = current.take() {
                if !nal.is_empty() {
                    nals.push(nal);
                }
            }
            current = Some(Vec::new());
            i += 4;
        } else {
            if let Some(nal) = current.as_mut() {
                nal.push(data[i]);
            }
            i += 1;
        }
    }
    if let Some(nal) = current {
        if !nal.is_empty() {
            nals.push(nal);
        }
    }
    nals
}

/// Convenience: generates a real H.264-in-MP4 test clip (skips with `None`
/// when ffmpeg is unavailable) and writes it to `path`.
///
/// # Errors
/// Returns an error string on muxing failures.
pub fn generate_h264_testsrc_mp4(
    dir: &Path,
    path: &Path,
    width: u32,
    height: u32,
    frames: u32,
    fps: u32,
) -> Result<Option<PathBuf>, String> {
    let Some(annexb) = generate_h264_testsrc_annexb(dir, width, height, frames, fps) else {
        return Ok(None);
    };
    let mp4 = mux_annexb_to_mp4(&annexb, width, height, fps)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, mp4).map_err(|e| e.to_string())?;
    Ok(Some(path.to_path_buf()))
}
