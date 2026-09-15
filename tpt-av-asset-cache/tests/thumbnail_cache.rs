//! Integration tests: generate + read back thumbnails for a synthetic video
//! source (via the tpt-kinetix stand-in).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tpt_av_asset_cache::{CacheStorage, ThumbnailCache, ThumbnailGenerator};
use tpt_av_asset_utils::{AssetError, AssetId, ProgressReporter};
use tpt_kinetix::{gradient_painter, write_test_video};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-cache-thumb-{name}-{}-{}",
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
fn generate_and_read_back_thumbnails() {
    let dir = temp_dir("roundtrip");
    let storage = CacheStorage::new(dir.join("cache"));
    storage.ensure_layout().unwrap();

    // 1.0 s @ 10 fps = 10 frames, 64x48.
    let video = dir.join("clip.tkv");
    write_test_video(&video, 64, 48, 10.0, 1.0, gradient_painter).unwrap();
    let asset = AssetId::from_path(&video).unwrap();

    // Thumbnails every 0.25 s → 4 slots.
    let mut cache = ThumbnailCache::create(asset, &storage, 0.25, (32, 24)).unwrap();
    let generator = ThumbnailGenerator::new(0.25, (32, 24));

    let max_fraction = Arc::new(Mutex::new(0.0f64));
    let tracker = Arc::clone(&max_fraction);
    let progress = ProgressReporter::with_callback(move |e| {
        let mut guard = tracker.lock().unwrap();
        *guard = guard.max(e.fraction);
    });
    generator.generate(&video, &mut cache, &progress).unwrap();
    assert!(*max_fraction.lock().unwrap() >= 0.99);

    assert_eq!(cache.thumbnail_count(), 4);
    assert_eq!(cache.resolution(), (32, 24));

    let times = cache.times();
    assert_eq!(times, vec![0.0, 0.25, 0.5, 0.75]);

    // Exact read: decoded RGBA at the configured resolution.
    let thumb = cache.read_thumbnail(0.5).unwrap().unwrap();
    assert!((thumb.time_secs - 0.5).abs() < f64::EPSILON);
    assert_eq!((thumb.width, thumb.height), (32, 24));
    assert_eq!(thumb.data.len(), 32 * 24 * 4);

    // JPEG files live at the documented layout.
    let thumbs_dir = storage.thumbnail_dir(asset);
    assert!(thumbs_dir.join("000002.jpg").exists());
    assert!(thumbs_dir.join("meta").exists());

    // Nearest read.
    let near = cache.read_nearest(0.44).unwrap().unwrap();
    assert!(
        (near.time_secs - 0.5).abs() < 1e-9,
        "0.44 rounds to the 0.5 slot"
    );
    let near_start = cache.read_nearest(0.1).unwrap().unwrap();
    assert!((near_start.time_secs).abs() < 1e-9, "0.1 is nearest to t=0");

    // Beyond the cached range still returns the closest slot.
    let beyond = cache.read_thumbnail(10.0);
    assert!(beyond.is_ok());

    // Distinct frames produce distinct thumbnails (moving gradient).
    let t0 = cache.read_thumbnail(0.0).unwrap().unwrap();
    let t1 = cache.read_thumbnail(0.75).unwrap().unwrap();
    assert_ne!(t0.data, t1.data);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_requires_meta_and_create_is_idempotent_safe() {
    let dir = temp_dir("meta");
    let storage = CacheStorage::new(dir.join("cache"));
    let asset = AssetId::from_parts(9, 9, 9);

    // open() without create() is a clean validation error, not a panic.
    let err = ThumbnailCache::open(asset, &storage).unwrap_err();
    assert!(matches!(err, AssetError::Validation(_)));

    let mut cache = ThumbnailCache::create(asset, &storage, 1.0, (16, 16)).unwrap();
    cache
        .write_thumbnail(&tpt_av_asset_cache::Thumbnail {
            time_secs: 0.0,
            data: vec![128; 16 * 16 * 4],
            width: 16,
            height: 16,
        })
        .unwrap();
    assert_eq!(cache.thumbnail_count(), 1);

    // Reopen recovers interval/resolution from meta.
    let reopened = ThumbnailCache::open(asset, &storage).unwrap();
    assert_eq!(reopened.interval_secs(), 1.0);
    assert_eq!(reopened.resolution(), (16, 16));
    assert_eq!(reopened.thumbnail_count(), 1);
    assert!(reopened.read_nearest(0.9).unwrap().is_some());

    // Malformed pixel data is a validation error.
    let err = cache.write_thumbnail(&tpt_av_asset_cache::Thumbnail {
        time_secs: 1.0,
        data: vec![0; 10],
        width: 16,
        height: 16,
    });
    assert!(matches!(err, Err(AssetError::Validation(_))));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn thumbnail_cancellation_leaves_partial_cache() {
    let dir = temp_dir("cancel");
    let storage = CacheStorage::new(dir.join("cache"));

    let video = dir.join("clip.tkv");
    write_test_video(&video, 32, 24, 10.0, 1.0, gradient_painter).unwrap();
    let asset = AssetId::from_path(&video).unwrap();

    let mut cache = ThumbnailCache::create(asset, &storage, 0.1, (16, 12)).unwrap();
    let progress = ProgressReporter::with_self_callback(|token| {
        move |e| {
            if e.fraction >= 0.5 {
                token.cancel();
            }
        }
    });
    let err = ThumbnailGenerator::new(0.1, (16, 12)).generate(&video, &mut cache, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    assert!(
        cache.thumbnail_count() < 10,
        "some thumbnails must be cached"
    );
    assert!(cache.thumbnail_count() > 0, "but not zero");

    let _ = std::fs::remove_dir_all(&dir);
}
