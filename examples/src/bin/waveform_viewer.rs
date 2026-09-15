//! Display waveform peaks from the cache as ASCII bars.
//!
//! ```text
//! cargo run -p tpt-av-asset-examples --bin waveform_viewer [-- path/to/audio.wav]
//! ```
//!
//! Peaks are generated into the cache if missing, then rendered through the
//! real-time-safe reader (`WaveformReader::read_chunk` — allocation-free,
//! lock-free).

use tpt_av_asset_cache::{CacheStorage, WaveformCache, WaveformGenerator};
use tpt_av_asset_utils::{AssetError, AssetId, ProgressReporter};

const CHUNK_SIZE: u32 = 2048;
const BARS: usize = 60;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), AssetError> {
    let home = tpt_av_asset_examples::engine_home();
    let storage = CacheStorage::new(home.join("cache"));

    let audio = match std::env::args().nth(1) {
        Some(path) => tpt_av_asset_examples::ensure_demo_media(std::path::Path::new(&path))?,
        None => tpt_av_asset_examples::ensure_demo_media(
            &std::path::PathBuf::from("demo").join("demo_song.wav"),
        )?,
    };
    let asset = AssetId::from_path(&audio)?;

    // Generate into the cache if it doesn't cover this asset yet.
    let mut cache = WaveformCache::open(asset, &storage)?;
    if cache.chunk_count() == 0 {
        println!("generating peaks (this runs once per file version)…");
        let generator = WaveformGenerator::new(CHUNK_SIZE, 44_100);
        let reporter = ProgressReporter::with_callback(|event| {
            tpt_av_asset_examples::print_progress(event.fraction, "peaks");
        });
        generator.generate(&audio, &mut cache, &reporter)?;
    }

    // The audio/render thread would hold one of these:
    let reader = cache.reader()?;

    println!(
        "\nwaveform for {} — {} chunks × {} frames @ {} Hz\n",
        audio.display(),
        reader.chunk_count(),
        reader.chunk_size(),
        reader.sample_rate()
    );

    let total = reader.chunk_count();
    let step = (total.div_ceil(BARS as u64).max(1)) as usize;
    let mut index: u64 = 0;
    while index < total {
        let chunk = reader.read_chunk(index)?.expect("index < chunk_count");
        let half = 12usize;
        let up = (chunk.max * half as f32).round() as usize;
        let down = (chunk.min.abs() * half as f32).round() as usize;
        let mut line = String::with_capacity(2 * half + 8);
        line.push_str(&" ".repeat(half.saturating_sub(up)));
        line.push_str(&"█".repeat(up.min(half)));
        line.push('|');
        line.push_str(&"█".repeat(down.min(half)));
        line.push_str(&" ".repeat(half.saturating_sub(down)));
        println!("  {:>4} {:<28} rms {:5.3}", index, line, chunk.rms);
        index += step as u64;
    }

    println!(
        "\n(reads above go through WaveformReader::read_chunk: no heap allocations, no locks)"
    );
    Ok(())
}
