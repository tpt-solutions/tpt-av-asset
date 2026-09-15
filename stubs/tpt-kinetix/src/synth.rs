//! Synthetic test-video generation helpers.

use std::path::Path;

use crate::{Frame, Result, TkvEncoder, VideoEncoder};

/// Writes a synthetic TKV video. Each frame is painted by `painter(frame_index,
/// width, height, rgba_out)`, so tests can produce content that changes over
/// time (use [`gradient_painter`] for a moving gradient).
///
/// # Errors
/// Returns [`Error`](crate::Error) if the file cannot be created or written.
pub fn write_test_video<P>(
    path: P,
    width: u32,
    height: u32,
    frame_rate: f64,
    duration_secs: f64,
    mut painter: impl FnMut(u32, u32, u32, &mut [u8]),
) -> Result<()>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let mut enc: Box<dyn VideoEncoder> =
        Box::new(TkvEncoder::new(path, width, height, frame_rate)?);
    let frame_count = (duration_secs * frame_rate).round().max(0.0) as u32;
    let mut data = vec![0u8; frame_bytes(width, height)];
    for index in 0..frame_count {
        painter(index, width, height, &mut data);
        enc.write_frame(&Frame {
            index,
            time_secs: f64::from(index) / frame_rate,
            width,
            height,
            data: data.clone(),
        })?;
    }
    enc.finish()?;
    Ok(())
}

fn frame_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

/// A painter that draws a red/green gradient with a moving white vertical
/// band, so consecutive frames differ and thumbnails are distinguishable.
pub fn gradient_painter(index: u32, width: u32, height: u32, data: &mut [u8]) {
    let band_x = (index as usize * 3) % width as usize;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = (y * width as usize + x) * 4;
            data[offset] = (x * 255 / width.max(1) as usize) as u8;
            data[offset + 1] = (y * 255 / height.max(1) as usize) as u8;
            data[offset + 2] = 32;
            data[offset + 3] = 255;
            if x == band_x || x == band_x + 1 {
                data[offset] = 255;
                data[offset + 1] = 255;
                data[offset + 2] = 255;
            }
        }
    }
}
