//! Browse video thumbnails from the cache.
//!
//! ```text
//! cargo run -p tpt-av-asset-examples --bin thumbnail_browser [-- path/to/video.tkv]
//! ```
//!
//! Thumbnails are generated into the cache if missing (one JPEG per
//! interval), then listed with their times and encoded sizes.

use std::time::Duration;

use tpt_av_asset_cache::{CacheStorage, ThumbnailCache, ThumbnailGenerator};
use tpt_av_asset_utils::{AssetError, AssetId, ProgressReporter};

const INTERVAL: f64 = 0.5;
const RESOLUTION: (u32, u32) = (320, 180);

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), AssetError> {
    let home = tpt_av_asset_examples::engine_home();
    let storage = CacheStorage::new(home.join("cache"));

    let video = match std::env::args().nth(1) {
        Some(path) => tpt_av_asset_examples::ensure_demo_media(std::path::Path::new(&path))?,
        None => tpt_av_asset_examples::ensure_demo_media(
            &std::path::PathBuf::from("demo").join("demo_clip.tkvp"),
        )?,
    };
    let asset = AssetId::from_path(&video)?;

    // Generate into the cache when missing.
    if ThumbnailCache::open(asset, &storage)
        .map(|c| c.thumbnail_count())
        .unwrap_or(0)
        == 0
    {
        println!("generating thumbnails (this runs once per file version)…");
        let mut cache = ThumbnailCache::create(asset, &storage, INTERVAL, RESOLUTION)?;
        let generator = ThumbnailGenerator::new(INTERVAL, RESOLUTION);
        let reporter = ProgressReporter::with_callback(|event| {
            tpt_av_asset_examples::print_progress(event.fraction, "thumbnails");
        });
        generator.generate(&video, &mut cache, &reporter)?;
        std::thread::sleep(Duration::from_millis(0));
    }

    let cache = ThumbnailCache::open(asset, &storage)?;
    println!(
        "\nthumbnail cache for {} — interval {:.2}s, {} thumbnails, {}×{}",
        video.display(),
        cache.interval_secs(),
        cache.thumbnail_count(),
        cache.resolution().0,
        cache.resolution().1
    );
    println!("  {:>6}  {:>10}  {:>9}  file", "slot", "time", "size");

    let dir = storage.thumbnail_dir(asset);
    let mut total_bytes = 0u64;
    for (slot, time) in cache.times().into_iter().enumerate() {
        let file = dir.join(format!("{slot:06}.jpg"));
        let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        total_bytes += size;
        println!(
            "  {:>6}  {:>9.3}s  {:>7} B  {}",
            slot,
            time,
            size,
            file.display()
        );
    }
    println!("  total: {total_bytes} B");

    // Exact and nearest lookups, as a scrubbing UI would use them.
    let middle = cache
        .times()
        .len()
        .checked_sub(1)
        .and_then(|last| cache.times().get(last / 2).copied())
        .unwrap_or(0.0);
    if let Some(thumb) = cache.read_thumbnail(middle)? {
        println!(
            "\nexact read at {middle:.2}s → {}×{} RGBA ({} bytes decoded)",
            thumb.width,
            thumb.height,
            thumb.data.len()
        );
    }
    if let Some(nearest) = cache.read_nearest(middle + 0.13)? {
        println!(
            "nearest to {:.2}s → slot at {:.2}s (read_nearest)",
            middle + 0.13,
            nearest.time_secs
        );
    }
    Ok(())
}
