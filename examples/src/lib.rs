//! Shared helpers for the tpt-av-asset demo binaries.

use std::path::{Path, PathBuf};

use tpt_av_asset_cache::CacheStorage;
use tpt_av_asset_db::AssetDb;
use tpt_av_asset_utils::AssetError;

/// Default engine home: `./.tpt-av-asset` (gitignored), overridable with
/// `TPT_AV_ASSET_HOME`. Matches the on-disk layout from the design doc:
/// `db/assets.redb` + `cache/{waveforms,thumbnails,proxies}`.
pub fn engine_home() -> PathBuf {
    std::env::var_os("TPT_AV_ASSET_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".tpt-av-asset"))
}

/// Opens (creating if needed) the demo engine's database and cache storage.
///
/// # Errors
/// Returns [`AssetError`] if the storage cannot be created.
pub fn open_engine(home: &Path) -> Result<(AssetDb, CacheStorage), AssetError> {
    let db = AssetDb::open(&home.join("db").join("assets.redb"))?;
    let storage = CacheStorage::new(home.join("cache"));
    storage.ensure_layout()?;
    Ok((db, storage))
}

/// Returns `path` if it exists; otherwise synthesizes a small demo file at
/// that path (a WAV for audio extensions, a TKV clip for video ones) so the
/// demos are self-contained.
///
/// # Errors
/// Returns [`AssetError`] when the file can neither be found nor created.
pub fn ensure_demo_media(path: &Path) -> Result<PathBuf, AssetError> {
    if path.exists() {
        return Ok(path.to_path_buf());
    }
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav")
        .to_ascii_lowercase();

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    match extension.as_str() {
        "tkv" => {
            tpt_kinetix::write_test_video(path, 320, 180, 24.0, 2.0, tpt_kinetix::gradient_painter)
                .map_err(|e| AssetError::codec(e.to_string()))?;
            println!("synthesized demo video: {}", path.display());
        }
        _ => {
            tpt_cadence::write_test_wav(path, 5.0, 44_100, 1)
                .map_err(|e| AssetError::codec(e.to_string()))?;
            println!("synthesized demo audio: {}", path.display());
        }
    }
    Ok(path.to_path_buf())
}

/// Progress printer for long-running jobs.
pub fn print_progress(fraction: f64, label: &str) {
    let filled = (fraction * 30.0).round() as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(30 - filled);
    print!("\r  [{bar}] {:5.1}% {label}   ", fraction * 100.0);
    if fraction >= 1.0 {
        println!();
    }
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
}
