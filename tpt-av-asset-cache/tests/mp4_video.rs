//! MP4/H.264 integration through the real kinetix demux+decode stack.
//!
//! There is no H.264 encoder in the ecosystem yet, so the fixture is
//! generated with `ffmpeg` (same convention as the kinetix conformance
//! suites): when ffmpeg is unavailable the tests print a notice and skip.

use std::path::PathBuf;

use tpt_av_asset_cache::CacheStorage;
use tpt_av_asset_cache::{ThumbnailCache, ThumbnailGenerator};
use tpt_av_asset_test_media::{ffmpeg_available, generate_h264_testsrc_mp4};
use tpt_av_asset_utils::{AssetId, ProgressReporter};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-cache-mp4-{name}-{}-{}",
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
fn mp4_h264_probe_thumbnails_and_seeking() {
    let dir = temp_dir("mp4");
    let video = match generate_h264_testsrc_mp4(&dir, &dir.join("clip.mp4"), 64, 48, 8, 4) {
        Ok(Some(path)) => path,
        Ok(None) => {
            eprintln!("skipping: ffmpeg not available (real H.264 fixture needs an encoder)");
            return;
        }
        Err(e) => {
            if !ffmpeg_available() {
                eprintln!("skipping: ffmpeg not available");
                return;
            }
            panic!("fixture generation failed with ffmpeg present: {e}");
        }
    };

    // Probe through the demuxer.
    let info = tpt_av_asset_cache::video::probe_video(&video).unwrap();
    assert_eq!((info.width, info.height), (64, 48));
    assert_eq!(info.codec, "h264");
    assert_eq!(info.frame_count, 8);
    assert!(
        (info.duration_secs - 2.0).abs() < 0.3,
        "got {}",
        info.duration_secs
    );

    // Seek lands on (or after) the requested time and decodes real content.
    let mut source = tpt_av_asset_cache::video::open_video(&video).unwrap();
    source.seek_to(0.5);
    let frame = source.next_frame().unwrap().expect("frames after seek");
    assert!(frame.time_secs >= 0.5 - 1e-6, "got {}", frame.time_secs);
    assert!(
        frame.data.chunks_exact(4).any(|p| p[0] != p[1]),
        "real testsrc frames are colorful"
    );

    // Thumbnail generation drives the same source through the generator.
    let asset = AssetId::from_path(&video).unwrap();
    let storage = CacheStorage::new(dir.join("cache"));
    let mut cache = ThumbnailCache::create(asset, &storage, 0.5, (32, 24)).unwrap();
    ThumbnailGenerator::new(0.5, (32, 24))
        .generate(&video, &mut cache, &ProgressReporter::new())
        .unwrap();
    assert_eq!(cache.thumbnail_count(), 4);
    let thumb = cache.read_thumbnail(1.0).unwrap().unwrap();
    assert_eq!((thumb.width, thumb.height), (32, 24));

    let _ = std::fs::remove_dir_all(&dir);
}
