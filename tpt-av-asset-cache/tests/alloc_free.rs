//! Proves the real-time safety guarantee of the waveform read path with an
//! allocation-counting global allocator: `read_chunk` and `read_range_into`
//! must perform **zero heap allocations**.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use tpt_av_asset_cache::{CacheStorage, WaveformCache, WaveformChunk, WaveformGenerator};
use tpt_av_asset_utils::{AssetId, ProgressReporter};

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-cache-alloc-{name}-{}-{}",
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
fn read_chunk_and_read_range_into_allocate_nothing() {
    let dir = temp_dir("rt");
    let storage = CacheStorage::new(dir.join("cache"));
    let asset = AssetId::from_parts(1, 2, 3);

    // Build a cache from a synthetic WAV (allocation is fine here).
    let wav = dir.join("tone.wav");
    tpt_cadence::write_test_wav(&wav, 1.0, 8_000, 1).unwrap();
    let mut cache = WaveformCache::open(asset, &storage).unwrap();
    let generator = WaveformGenerator::new(512, 8_000);
    generator
        .generate(&wav, &mut cache, &ProgressReporter::new())
        .unwrap();
    assert!(cache.chunk_count() > 0);

    // Dedicated reader like the audio thread would hold.
    let reader = cache.reader().unwrap();

    // Warm-up: first touches may lazily initialize platform state (allowed).
    reader.read_chunk(0).unwrap().unwrap();
    let mut scratch = [WaveformChunk {
        index: 0,
        min: 0.0,
        max: 0.0,
        rms: 0.0,
    }; 8];
    reader.read_range_into(0, &mut scratch).unwrap();

    // Measure: zero allocations from here on.
    ALLOC_COUNT.store(0, Ordering::SeqCst);

    for i in 0..reader.chunk_count() {
        let chunk = reader.read_chunk(i).unwrap().expect("chunk must exist");
        assert!(chunk.min <= chunk.max);
        assert!(chunk.rms >= 0.0);
    }
    let read = reader.read_range_into(0, &mut scratch).unwrap();
    assert_eq!(read, scratch.len().min(reader.chunk_count() as usize));

    let allocations = ALLOC_COUNT.load(Ordering::SeqCst);
    assert_eq!(
        allocations, 0,
        "real-time safe read path performed {allocations} heap allocations"
    );

    // The path through WaveformCache::read_chunk must be equally clean.
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    cache.read_chunk(0).unwrap().unwrap();
    assert_eq!(ALLOC_COUNT.load(Ordering::SeqCst), 0);

    let _ = std::fs::remove_dir_all(&dir);
}
