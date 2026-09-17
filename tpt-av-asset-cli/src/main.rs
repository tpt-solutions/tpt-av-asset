//! `tpt-av-asset` — command-line interface to the TPT AV asset engine.
//!
//! Media files are imported into an engine home (`~/.tpt-av-asset` by
//! default, override with `TPT_AV_ASSET_HOME`): a `redb` database under
//! `db/` plus the on-disk caches under `cache/` (waveform peaks, video
//! thumbnails, proxies). Heavy work runs on the background pipeline; the
//! CLI waits and renders progress.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use tpt_av_asset_cache::{
    CacheStorage, ThumbnailCache, ThumbnailGenerator, WaveformCache, WaveformGenerator,
};
use tpt_av_asset_db::AssetDb;
use tpt_av_asset_pipeline::{AssetImporter, ProcessingPipeline};
use tpt_av_asset_proxy::{ProxyGenerator, ProxyProfile};
use tpt_av_asset_utils::{AssetError, AssetId, ProgressReporter};
use tpt_av_asset_watcher::{CacheInvalidator, FileEvent, MediaWatcher};

#[derive(Parser)]
#[command(
    name = "tpt-av-asset",
    version,
    about = "Media asset management engine: waveforms, thumbnails, proxies, watching",
    after_help = "Engine home defaults to ~/.tpt-av-asset (override: TPT_AV_ASSET_HOME)."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Import a media file and generate all caches (waveform peaks,
    /// thumbnails, proxies) through the background pipeline.
    Import {
        /// Media file to import.
        path: PathBuf,
        /// Background worker threads.
        #[arg(long, default_value_t = 2)]
        workers: usize,
    },
    /// Render (or read) the waveform peak cache for an audio file as ASCII.
    Waveform {
        /// Audio file (WAV/FLAC).
        path: PathBuf,
        /// Frames per peak chunk.
        #[arg(long, default_value_t = 2_048)]
        chunk_size: u32,
    },
    /// Generate (or read) the thumbnail cache for a video file and list it.
    Thumbnail {
        /// Video file (MP4/H.264 or `.tkvp`).
        path: PathBuf,
        /// Seconds between thumbnails.
        #[arg(long, default_value_t = 0.5)]
        interval: f64,
        /// Thumbnail resolution.
        #[arg(long, default_value_t = 320)]
        width: u32,
        #[arg(long, default_value_t = 180)]
        height: u32,
    },
    /// Generate proxies for a media file or every media file in a folder.
    Proxy {
        /// Media file or folder (`.wav`/`.tkvp`/`.mp4` files).
        path: PathBuf,
        /// Proxy profile preset.
        #[arg(long, value_enum, default_value_t = ProfilePreset::Medium)]
        profile: ProfilePreset,
        /// Output directory (default: `<folder>/proxies`).
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Watch a folder and drop stale caches on modify/move/delete.
    Watch {
        /// Folder to watch (recursively).
        path: PathBuf,
        /// Per-path event debounce window in milliseconds.
        #[arg(long, default_value_t = 250)]
        debounce_ms: u64,
    },
}

#[derive(ValueEnum, Clone, Copy)]
enum ProfilePreset {
    /// 1920x1080, low bitrate.
    Low,
    /// 1280x720, medium bitrate.
    Medium,
    /// Audio-only (currently written as PCM WAV).
    Flac,
}

impl ProfilePreset {
    fn profile(self) -> ProxyProfile {
        match self {
            ProfilePreset::Low => ProxyProfile::proxy_1080p_low(),
            ProfilePreset::Medium => ProxyProfile::proxy_720p_medium(),
            ProfilePreset::Flac => ProxyProfile::audio_proxy_flac(),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Engine home: `~/.tpt-av-asset` (spec layout), overridable with
/// `TPT_AV_ASSET_HOME`.
fn engine_home() -> PathBuf {
    std::env::var_os("TPT_AV_ASSET_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".tpt-av-asset"))
                .unwrap_or_else(|| PathBuf::from(".tpt-av-asset"))
        })
}

fn open_engine() -> Result<(AssetDb, CacheStorage), AssetError> {
    let home = engine_home();
    let db = AssetDb::open(&home.join("db").join("assets.redb"))?;
    let storage = CacheStorage::new(home.join("cache"));
    storage.ensure_layout()?;
    Ok((db, storage))
}

fn run(cli: Cli) -> Result<(), AssetError> {
    match cli.command {
        Commands::Import { path, workers } => cmd_import(&path, workers),
        Commands::Waveform { path, chunk_size } => cmd_waveform(&path, chunk_size),
        Commands::Thumbnail {
            path,
            interval,
            width,
            height,
        } => cmd_thumbnail(&path, interval, (width, height)),
        Commands::Proxy {
            path,
            profile,
            output,
        } => cmd_proxy(&path, profile.profile(), output.as_deref()),
        Commands::Watch { path, debounce_ms } => cmd_watch(&path, debounce_ms),
    }
}

fn cmd_import(path: &Path, workers: usize) -> Result<(), AssetError> {
    let (db, storage) = open_engine()?;
    let mut pipeline = ProcessingPipeline::with_db(workers.max(1), db.clone())?;
    pipeline.start()?;
    let importer = AssetImporter::new(pipeline.clone(), db.clone(), storage.clone());

    let started = std::time::Instant::now();
    let (asset, job_ids) = importer.import_with_jobs(path)?;
    println!(
        "imported {} → asset {:016x}, {} background job(s)",
        path.display(),
        asset.hash(),
        job_ids.len()
    );

    let deadline = Duration::from_secs(600);
    loop {
        let terminal = job_ids
            .iter()
            .filter(|job| {
                pipeline
                    .wait_for_timeout(**job, Duration::from_millis(50))
                    .map(|snapshot| snapshot.is_terminal())
                    .unwrap_or(true)
            })
            .count();
        if terminal == job_ids.len() {
            break;
        }
        if started.elapsed() > deadline {
            pipeline.stop()?;
            return Err(AssetError::validation("jobs did not finish in time"));
        }
    }
    pipeline.stop()?;

    println!("done in {:.1?}:", started.elapsed());
    for (cache_type, cache_path) in db.list_cache_entries(asset)? {
        println!("  {cache_type}: {}", cache_path.display());
    }
    Ok(())
}

fn cmd_waveform(path: &Path, chunk_size: u32) -> Result<(), AssetError> {
    let (_, storage) = open_engine()?;
    let asset = AssetId::from_path(path)?;

    let mut cache = WaveformCache::open(asset, &storage)?;
    if cache.chunk_count() == 0 {
        println!("generating peaks for {}…", path.display());
        let generator = WaveformGenerator::new(chunk_size, 44_100);
        let reporter = ProgressReporter::with_callback(|event| print_progress(event.fraction));
        generator.generate(path, &mut cache, &reporter)?;
    }

    let reader = cache.reader()?;
    println!(
        "\nwaveform: {} chunks × {} frames @ {} Hz",
        reader.chunk_count(),
        reader.chunk_size(),
        reader.sample_rate()
    );
    let total = reader.chunk_count();
    let step = total.div_ceil(60).max(1);
    let half = 12usize;
    let mut index = 0u64;
    while index < total {
        let chunk = reader.read_chunk(index)?.expect("index < chunk_count");
        let up = (chunk.max * half as f32).round() as usize;
        let down = (chunk.min.abs() * half as f32).round() as usize;
        let bar = format!(
            "{}|{}",
            "█".repeat(up.min(half)),
            "█".repeat(down.min(half))
        );
        println!("  {:>5} {:<26} rms {:5.3}", index, bar, chunk.rms);
        index += step;
    }
    Ok(())
}

fn cmd_thumbnail(path: &Path, interval: f64, resolution: (u32, u32)) -> Result<(), AssetError> {
    let (_, storage) = open_engine()?;
    let asset = AssetId::from_path(path)?;

    if ThumbnailCache::open(asset, &storage)
        .map(|c| c.thumbnail_count())
        .unwrap_or(0)
        == 0
    {
        println!("generating thumbnails for {}…", path.display());
        let mut cache = ThumbnailCache::create(asset, &storage, interval, resolution)?;
        let reporter = ProgressReporter::with_callback(|event| print_progress(event.fraction));
        ThumbnailGenerator::new(interval, resolution).generate(path, &mut cache, &reporter)?;
    }

    let cache = ThumbnailCache::open(asset, &storage)?;
    println!(
        "thumbnail cache: {} thumbnails, interval {:.2}s, {}×{}",
        cache.thumbnail_count(),
        cache.interval_secs(),
        cache.resolution().0,
        cache.resolution().1
    );
    for (slot, time) in cache.times().into_iter().enumerate() {
        println!("  {:>5}  {:>8.3}s", slot, time);
    }
    Ok(())
}

fn cmd_proxy(path: &Path, profile: ProxyProfile, output: Option<&Path>) -> Result<(), AssetError> {
    profile.validate()?;
    let profile_name = profile.name.clone();
    let generator = ProxyGenerator::new(profile);

    let sources = if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        let e = e.to_ascii_lowercase();
                        e == "wav" || e == "flac" || e == "tkvp" || e == "mp4"
                    })
            })
            .collect();
        files.sort();
        files
    } else {
        vec![path.to_path_buf()]
    };
    if sources.is_empty() {
        println!("no media files under {}", path.display());
        return Ok(());
    }

    let out_dir = output.map(Path::to_path_buf).unwrap_or_else(|| {
        if path.is_dir() {
            path.join("proxies")
        } else {
            path.parent()
                .map(|p| p.join("proxies"))
                .unwrap_or_else(|| PathBuf::from("proxies"))
        }
    });
    std::fs::create_dir_all(&out_dir)?;

    println!(
        "generating {} proxy/proxies ({})",
        sources.len(),
        profile_name
    );
    let mut failed = 0usize;
    for source in &sources {
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "media".into());
        let is_audio = source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                let e = e.to_ascii_lowercase();
                e == "wav" || e == "flac"
            });
        // Audio sources always take the audio route (`.wav` today); video
        // sources take the profile's video extension (`.tkvp`).
        let extension = if is_audio { "wav" } else { "tkvp" };
        let target = out_dir.join(format!("{stem}_proxy.{extension}"));
        let reporter = ProgressReporter::with_callback(|event| print_progress(event.fraction));
        let result = if is_audio {
            generator.generate_audio_proxy(source, &target, &reporter)
        } else {
            generator.generate_video_proxy(source, &target, &reporter)
        };
        match result {
            Ok(()) => println!(
                "  {} → {} ({} B)",
                source.display(),
                target.display(),
                std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
            ),
            Err(e) => {
                failed += 1;
                println!("  {} FAILED: {e}", source.display());
            }
        }
    }
    if failed > 0 {
        return Err(AssetError::validation(format!("{failed} render(s) failed")));
    }
    Ok(())
}

fn cmd_watch(path: &Path, debounce_ms: u64) -> Result<(), AssetError> {
    let (db, storage) = open_engine()?;
    let invalidator = Arc::new(CacheInvalidator::new(db, storage));

    let mut watcher = MediaWatcher::with_debounce(Duration::from_millis(debounce_ms.max(25)))?;
    watcher.watch(path)?;
    println!(
        "watching {} ({} backend, {} ms debounce) — Ctrl-C to stop",
        path.display(),
        watcher.backend_name(),
        debounce_ms
    );

    let sink: Arc<Mutex<Vec<FileEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let pending = Arc::clone(&sink);
    let invalidator_for_print = Arc::clone(&invalidator);
    loop {
        // Drain whatever arrived, then invalidate + report.
        let mut dropped = 0usize;
        for event in pending.lock().expect("sink poisoned").drain(..) {
            match invalidator_for_print.handle_event(&event) {
                Ok(true) => dropped += 1,
                Ok(false) => {}
                Err(e) => eprintln!("  invalidation failed: {e}"),
            }
            println!("  {:?} {}", event.event_type, event.path.display());
        }
        if dropped > 0 {
            println!("  → invalidated caches for {dropped} asset(s)");
        }

        match watcher.try_recv() {
            Ok(Some(event)) => {
                pending.lock().expect("sink poisoned").push(event);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e),
        }
    }
}

fn print_progress(fraction: f64) {
    let filled = (fraction * 30.0).round() as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(30 - filled);
    print!("\r  [{bar}] {:5.1}%   ", fraction * 100.0);
    if fraction >= 1.0 {
        println!();
    }
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
}
