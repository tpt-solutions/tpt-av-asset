//! Integration tests: end-to-end watcher behavior (creation, modification,
//! deletion, rename via the polling backend) and the CacheInvalidator
//! bridging into `tpt-av-asset-db`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tpt_av_asset_db::{AssetDb, CacheType};
use tpt_av_asset_utils::{AssetId, MediaInfo, MediaType};
use tpt_av_asset_watcher::{
    CacheInvalidator, FileEvent, FileEventType, MediaWatcher, PollingBackend,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-watcher-it-{name}-{}-{}",
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

/// Collects events for up to `deadline` until `pred` is satisfied.
fn wait_for(
    watcher: &mut MediaWatcher,
    deadline: Duration,
    mut pred: impl FnMut(&FileEvent) -> bool,
) -> Option<FileEvent> {
    let start = Instant::now();
    while start.elapsed() < deadline {
        match watcher.try_recv() {
            Ok(Some(event)) => {
                if pred(&event) {
                    return Some(event);
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    }
    None
}

#[test]
fn polling_backend_detects_lifecycle_events() {
    let dir = temp_dir("polling");
    let media = dir.join("media");
    std::fs::create_dir_all(&media).unwrap();

    let window = Duration::from_millis(150);
    let mut watcher = MediaWatcher::with_backend(
        move |tx| Box::new(PollingBackend::new(tx, Duration::from_millis(100))),
        window,
    );
    watcher.watch(&media).unwrap();
    assert_eq!(watcher.watched_directories().len(), 1);

    // Created.
    let file = media.join("song.wav");
    std::fs::write(&file, b"v1").unwrap();
    let created = wait_for(&mut watcher, Duration::from_secs(5), |e| {
        e.event_type == FileEventType::Created && e.path.ends_with("song.wav")
    });
    assert!(created.is_some(), "creation must surface as Created");

    // Modified (size changes make polling deterministic).
    std::thread::sleep(window * 2);
    std::fs::write(&file, b"v1-with-more-data").unwrap();
    let modified = wait_for(&mut watcher, Duration::from_secs(5), |e| {
        e.event_type == FileEventType::Modified && e.path.ends_with("song.wav")
    });
    assert!(modified.is_some(), "rewrite must surface as Modified");

    // Deleted.
    std::thread::sleep(window * 2);
    std::fs::remove_file(&file).unwrap();
    let deleted = wait_for(&mut watcher, Duration::from_secs(5), |e| {
        e.event_type == FileEventType::Deleted && e.path.ends_with("song.wav")
    });
    assert!(deleted.is_some(), "deletion must surface as Deleted");

    // Unwatch stops events.
    watcher.unwatch(&media).unwrap();
    std::fs::write(media.join("later.wav"), b"x").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let later = wait_for(&mut watcher, Duration::from_millis(300), |e| {
        e.path.ends_with("later.wav")
    });
    assert!(later.is_none(), "unwatched directories must not emit events");

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
#[test]
fn polling_backend_rename_surfaces_as_delete_plus_create() {
    let dir = temp_dir("rename");
    let media = dir.join("media");
    std::fs::create_dir_all(&media).unwrap();

    let mut watcher = MediaWatcher::with_backend(
        move |tx| Box::new(PollingBackend::new(tx, Duration::from_millis(80))),
        Duration::from_millis(120),
    );
    watcher.watch(&media).unwrap();

    let old = media.join("before.wav");
    std::fs::write(&old, b"data").unwrap();
    assert!(
        wait_for(&mut watcher, Duration::from_secs(5), |e| e.path.ends_with("before.wav")).is_some()
    );

    std::thread::sleep(Duration::from_millis(250));
    std::fs::rename(&old, media.join("after.wav")).unwrap();

    let saw_delete = wait_for(&mut watcher, Duration::from_secs(5), |e| {
        e.event_type == FileEventType::Deleted && e.path.ends_with("before.wav")
    })
    .is_some();
    let saw_create = wait_for(&mut watcher, Duration::from_secs(5), |e| {
        e.event_type == FileEventType::Created && e.path.ends_with("after.wav")
    })
    .is_some();
    assert!(saw_delete && saw_create, "polling reports renames as delete+create");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_invalidator_reacts_to_modification() {
    let dir = temp_dir("invalidator");
    let db = AssetDb::open(&dir.join("db.redb")).unwrap();
    let storage = tpt_av_asset_cache::CacheStorage::new(dir.join("cache"));
    storage.ensure_layout().unwrap();

    // Register an asset with a waveform cache entry + a real file on disk.
    let media = dir.join("media");
    std::fs::create_dir_all(&media).unwrap();
    let file = media.join("episode.wav");
    std::fs::write(&file, b"original-bytes").unwrap();
    let id = AssetId::from_path(&file).unwrap();
    let mut info = MediaInfo::new(id, &file, MediaType::Audio);
    info.duration_secs = Some(12.0);
    db.upsert_asset(&info).unwrap();
    let peaks = storage.waveform_path(id);
    std::fs::write(&peaks, b"peaks-bytes").unwrap();
    db.record_cache_entry(id, CacheType::WaveformPeaks, &peaks).unwrap();
    assert!(db.has_cache_entry(id, CacheType::WaveformPeaks).unwrap());

    let invalidator = CacheInvalidator::new(db.clone(), storage.clone());

    // Direct dispatch: Modified → invalidate.
    let event = FileEvent::now(FileEventType::Modified, &file);
    assert!(invalidator.handle_event(&event).unwrap());
    assert!(!db.has_cache_entry(id, CacheType::WaveformPeaks).unwrap());
    assert!(!peaks.exists(), "stale peaks file must be deleted");

    // Unknown paths are ignored.
    let unknown = FileEvent::now(FileEventType::Modified, media.join("unknown.wav"));
    assert!(!invalidator.handle_event(&unknown).unwrap());

    // Batch dispatch with a fresh asset + a Deleted event.
    std::fs::write(&file, b"second-episode").unwrap();
    let id2 = AssetId::from_path(&file).unwrap();
    let info2 = MediaInfo::new(id2, &file, MediaType::Audio);
    db.upsert_asset(&info2).unwrap();
    let peaks2 = storage.waveform_path(id2);
    std::fs::write(&peaks2, b"peaks2").unwrap();
    db.record_cache_entry(id2, CacheType::WaveformPeaks, &peaks2).unwrap();

    let events = vec![FileEvent::now(FileEventType::Deleted, &file)];
    assert_eq!(invalidator.handle_all(&events).unwrap(), 1);
    assert!(!db.has_cache_entry(id2, CacheType::WaveformPeaks).unwrap());
    assert!(!peaks2.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn debounced_events_coalesce_through_watcher() {
    // Push several raw events into the debouncer through the watcher by
    // hammering a file inside one polling window: the watcher must surface
    // at most one Modified per quiet period (not one per write).
    let dir = temp_dir("coalesce");
    let media = dir.join("media");
    std::fs::create_dir_all(&media).unwrap();

    let window = Duration::from_millis(300);
    let mut watcher = MediaWatcher::with_backend(
        move |tx| Box::new(PollingBackend::new(tx, Duration::from_millis(60))),
        window,
    );
    watcher.watch(&media).unwrap();

    let file = media.join("storm.wav");
    std::fs::write(&file, b"0").unwrap();
    // Wait for the initial Created to fully land, then storm writes within
    // one debounce window.
    assert!(wait_for(&mut watcher, Duration::from_secs(5), |e| {
        e.event_type == FileEventType::Created
    })
    .is_some());

    std::thread::sleep(window * 2);
    for i in 1..=5u32 {
        std::fs::write(&file, format!("payload-{i}")).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }

    // Count Modified events over two windows: the poller may see 1–3 raw
    // Modifieds, but the debouncer must collapse them to at most 2.
    let deadline = Instant::now() + window * 3;
    let mut modified = 0;
    while Instant::now() < deadline {
        match watcher.try_recv() {
            Ok(Some(e)) => {
                if e.event_type == FileEventType::Modified && e.path.ends_with("storm.wav") {
                    modified += 1;
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    assert!(modified <= 2, "debouncer must collapse the storm (saw {modified})");
    assert!(modified >= 1, "at least one Modified must surface");

    let _ = std::fs::remove_dir_all(&dir);
}
