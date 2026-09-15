//! Integration tests: generate + read back waveform peaks for a synthetic
//! WAV, including cancellation and crash/resume behavior.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tpt_av_asset_cache::{CacheStorage, WaveformCache, WaveformGenerator};
use tpt_av_asset_utils::{AssetError, AssetId, ProgressReporter};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-cache-wav-{name}-{}-{}",
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

fn synthetic_wav(
    dir: &std::path::Path,
    name: &str,
    duration: f64,
    rate: u32,
    channels: u16,
) -> PathBuf {
    let path = dir.join(name);
    tpt_av_asset_test_media::write_test_wav(&path, duration, rate, channels).unwrap();
    path
}

#[test]
fn generate_and_read_back_stereo() {
    let dir = temp_dir("roundtrip");
    let storage = CacheStorage::new(dir.join("cache"));
    storage.ensure_layout().unwrap();
    let wav = synthetic_wav(&dir, "tone.wav", 2.0, 16_000, 2);
    let asset = AssetId::from_path(&wav).unwrap();

    let mut cache = WaveformCache::open(asset, &storage).unwrap();
    let generator = WaveformGenerator::new(1024, 16_000);

    let max_fraction = Arc::new(Mutex::new(0.0f64));
    let tracker = Arc::clone(&max_fraction);
    let progress = ProgressReporter::with_callback(move |e| {
        let mut guard = tracker.lock().unwrap();
        *guard = guard.max(e.fraction);
    });
    generator.generate(&wav, &mut cache, &progress).unwrap();
    assert!(
        *max_fraction.lock().unwrap() >= 0.99,
        "progress must reach the end"
    );

    // 2 s at 16 kHz with 1024-frame chunks → ceil(32000/1024) = 32 chunks.
    assert_eq!(cache.chunk_count(), 32);
    assert_eq!(cache.chunk_size(), 1024);

    for i in 0..cache.chunk_count() {
        let chunk = cache.read_chunk(i).unwrap().unwrap();
        assert_eq!(chunk.index, i);
        assert!(
            chunk.min <= chunk.max,
            "chunk {i} min={} max={}",
            chunk.min,
            chunk.max
        );
        assert!(chunk.min >= -1.0 && chunk.max <= 1.0);
        assert!(chunk.rms > 0.0 && chunk.rms <= 1.0);
    }

    // Range read spanning the end clamps.
    let tail = cache.read_range(30, 40).unwrap();
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[1].index, 31);

    // Out-of-range reads are None, not errors.
    assert!(cache.read_chunk(32).unwrap().is_none());
    assert!(cache.read_chunk(999).unwrap().is_none());

    // On-disk location follows the storage layout.
    assert!(cache.path().exists());
    assert_eq!(cache.path().extension().unwrap(), "peaks");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancellation_aborts_with_error_and_partial_cache() {
    let dir = temp_dir("cancel");
    let storage = CacheStorage::new(dir.join("cache"));
    let wav = synthetic_wav(&dir, "tone.wav", 4.0, 8_000, 1);
    let asset = AssetId::from_path(&wav).unwrap();
    let mut cache = WaveformCache::open(asset, &storage).unwrap();

    // Pre-cancelled token: the generator must abort before writing anything.
    let progress = ProgressReporter::new();
    progress.cancel();
    let err = WaveformGenerator::new(256, 8_000).generate(&wav, &mut cache, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    assert_eq!(
        cache.chunk_count(),
        0,
        "cancel before first chunk must not write"
    );

    // Cancel mid-generation once observed progress crosses 5%.
    let mut cache = WaveformCache::open(asset, &storage).unwrap();
    let progress = ProgressReporter::with_self_callback(|token| {
        move |e| {
            if e.fraction >= 0.05 {
                token.cancel();
            }
        }
    });
    let err = WaveformGenerator::new(256, 8_000).generate(&wav, &mut cache, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    let partial = cache.chunk_count();
    let total = 4 * 8_000 / 256;
    assert!(
        partial > 0 && partial < total,
        "partial progress: {partial}/{total}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn resume_completes_without_regressing() {
    let dir = temp_dir("resume");
    let storage = CacheStorage::new(dir.join("cache"));
    let wav = synthetic_wav(&dir, "tone.wav", 3.0, 8_000, 1);
    let asset = AssetId::from_path(&wav).unwrap();
    let generator = WaveformGenerator::new(256, 8_000);
    // 24000 frames / 256 per chunk = 93.75 → 94 chunks (last one partial).
    let total = 3usize * 8_000;
    let total = total.div_ceil(256);

    // First pass: cancel early, leaving partial progress on disk.
    let mut cache = WaveformCache::open(asset, &storage).unwrap();
    let progress = ProgressReporter::with_self_callback(|token| {
        move |e| {
            if e.fraction >= 0.02 {
                token.cancel();
            }
        }
    });
    assert!(matches!(
        generator.generate(&wav, &mut cache, &progress),
        Err(AssetError::Cancelled)
    ));
    let partial = cache.chunk_count();
    assert!(partial > 0, "some chunks must be persisted before cancel");
    drop(cache);

    // Second pass: a fresh process would re-open the same file; chunk_count
    // must never regress and the run must complete.
    let mut cache = WaveformCache::open(asset, &storage).unwrap();
    assert_eq!(
        cache.chunk_count(),
        partial,
        "reopen must see persisted progress"
    );

    let min_seen = Arc::new(Mutex::new(f64::MAX));
    let tracker = Arc::clone(&min_seen);
    let progress = ProgressReporter::with_callback(move |e| {
        let mut guard = tracker.lock().unwrap();
        *guard = guard.min(e.fraction);
    });
    generator.generate(&wav, &mut cache, &progress).unwrap();
    assert_eq!(cache.chunk_count(), total);
    // First fraction reported must be at least partial/total (resume, not restart).
    assert!(
        *min_seen.lock().unwrap() >= partial as f64 / total as f64,
        "generator must resume from persisted progress"
    );

    // The final chunk carries real data.
    let last = cache.read_chunk(cache.chunk_count() - 1).unwrap().unwrap();
    assert!(last.min <= last.max);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn params_must_match_existing_cache() {
    let dir = temp_dir("params");
    let storage = CacheStorage::new(dir.join("cache"));
    let wav = synthetic_wav(&dir, "tone.wav", 0.5, 8_000, 1);
    let asset = AssetId::from_path(&wav).unwrap();

    let mut cache = WaveformCache::open(asset, &storage).unwrap();
    WaveformGenerator::new(512, 8_000)
        .generate(&wav, &mut cache, &ProgressReporter::new())
        .unwrap();

    // Different chunk size on the same cache file is rejected.
    let err =
        WaveformGenerator::new(1024, 8_000).generate(&wav, &mut cache, &ProgressReporter::new());
    assert!(matches!(err, Err(AssetError::Validation(_))));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn writes_counter_grows_monotonically() {
    // Sanity guard used by the resume test's assumptions: progress callbacks
    // fire monotonically during a clean run.
    let dir = temp_dir("monotonic");
    let storage = CacheStorage::new(dir.join("cache"));
    let wav = synthetic_wav(&dir, "tone.wav", 1.0, 8_000, 1);
    let asset = AssetId::from_path(&wav).unwrap();
    let mut cache = WaveformCache::open(asset, &storage).unwrap();

    let last = Arc::new(Mutex::new(0.0f64));
    let tracker = Arc::clone(&last);
    let monotonic = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&monotonic);
    let progress = ProgressReporter::with_callback(move |e| {
        let mut guard = tracker.lock().unwrap();
        if e.fraction < *guard {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        *guard = guard.max(e.fraction);
    });
    WaveformGenerator::new(512, 8_000)
        .generate(&wav, &mut cache, &progress)
        .unwrap();
    assert_eq!(
        monotonic.load(Ordering::SeqCst),
        0,
        "progress must be monotonic"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
