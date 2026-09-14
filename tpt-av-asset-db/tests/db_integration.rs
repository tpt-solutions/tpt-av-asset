//! Integration tests for `AssetDb`: asset CRUD, cache entries, job records,
//! persistence across reopen, and concurrent access.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tpt_av_asset_db::{AssetDb, CacheType, JobRecord, JobState};
use tpt_av_asset_utils::{AssetId, MediaInfo, MediaType, Priority};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-db-it-{name}-{}-{}",
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

fn sample_asset(id: AssetId, name: &str, media_type: MediaType) -> MediaInfo {
    MediaInfo::new(id, Path::new("/media").join(name).as_path(), media_type)
}

#[test]
fn asset_insert_get_list_remove() {
    let dir = temp_dir("crud");
    let db = AssetDb::open(&dir.join("assets.redb")).unwrap();

    let a = AssetId::from_parts(1, 100, 10);
    let b = AssetId::from_parts(2, 200, 20);

    let mut info_a = sample_asset(a, "a.wav", MediaType::Audio);
    info_a.duration_secs = Some(12.5);
    let info_b = sample_asset(b, "b.mov", MediaType::Video);

    assert!(db.get_asset(a).unwrap().is_none());
    db.upsert_asset(&info_a).unwrap();
    db.upsert_asset(&info_b).unwrap();

    let got = db.get_asset(a).unwrap().unwrap();
    assert_eq!(got, info_a, "roundtrip must preserve all fields");

    let all = db.list_assets().unwrap();
    assert_eq!(all.len(), 2);

    // Upsert replaces.
    info_a.duration_secs = Some(13.0);
    db.upsert_asset(&info_a).unwrap();
    assert_eq!(db.get_asset(a).unwrap().unwrap().duration_secs, Some(13.0));
    assert_eq!(db.list_assets().unwrap().len(), 2);

    db.remove_asset(a).unwrap();
    assert!(db.get_asset(a).unwrap().is_none());
    // Removing again is a no-op.
    db.remove_asset(a).unwrap();
    assert_eq!(db.list_assets().unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asset_lookup_by_path() {
    let dir = temp_dir("by-path");
    let media_dir = dir.join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let file = media_dir.join("song.wav");
    std::fs::write(&file, b"data").unwrap();

    let db = AssetDb::open(&dir.join("db.redb")).unwrap();
    let id = AssetId::from_path(&file).unwrap();
    db.upsert_asset(&MediaInfo::new(id, &file, MediaType::Audio)).unwrap();

    // Same file via a relative-ish path variant.
    let found = db.get_asset_by_path(&media_dir.join("song.wav")).unwrap();
    assert!(found.is_some(), "canonical path comparison must find it");
    assert_eq!(found.unwrap().id, id);

    assert!(db.get_asset_by_path(&media_dir.join("other.wav")).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_entry_record_check_list_invalidate() {
    let dir = temp_dir("cache-entries");
    let db = AssetDb::open(&dir.join("db.redb")).unwrap();

    let a = AssetId::from_parts(7, 70, 700);
    let other = AssetId::from_parts(8, 80, 800);

    assert!(!db.has_cache_entry(a, CacheType::WaveformPeaks).unwrap());

    db.record_cache_entry(a, CacheType::WaveformPeaks, Path::new("/cache/a.peaks"))
        .unwrap();
    db.record_cache_entry(a, CacheType::VideoThumbnails, Path::new("/cache/a-thumbs"))
        .unwrap();
    db.record_cache_entry(other, CacheType::WaveformPeaks, Path::new("/cache/b.peaks"))
        .unwrap();

    assert!(db.has_cache_entry(a, CacheType::WaveformPeaks).unwrap());
    assert_eq!(
        db.get_cache_entry(a, CacheType::WaveformPeaks).unwrap(),
        Some(PathBuf::from("/cache/a.peaks"))
    );

    let entries = db.list_cache_entries(a).unwrap();
    assert_eq!(entries.len(), 2);

    let removed = db.invalidate_caches(a).unwrap();
    assert_eq!(removed, 2);
    assert!(!db.has_cache_entry(a, CacheType::WaveformPeaks).unwrap());
    assert!(db.list_cache_entries(a).unwrap().is_empty());
    // The other asset's entry must survive.
    assert!(db.has_cache_entry(other, CacheType::WaveformPeaks).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn job_records_persist() {
    let dir = temp_dir("jobs");
    let db = AssetDb::open(&dir.join("db.redb")).unwrap();

    let record = JobRecord {
        job_id: 1,
        asset_id: AssetId::from_parts(5, 50, 500),
        priority: Priority::High,
        state: JobState::Pending,
        kind: "waveform".into(),
        progress: 0.0,
        resume_hint: 0,
        payload: "/media/a.wav".into(),
        created_ms: 1_000,
        updated_ms: 1_000,
        error: None,
    };
    db.upsert_job(&record).unwrap();

    let mut running = record.clone();
    running.state = JobState::Running;
    running.progress = 0.4;
    running.resume_hint = 9;
    db.upsert_job(&running).unwrap();

    let got = db.get_job(1).unwrap().unwrap();
    assert_eq!(got.state, JobState::Running);
    assert!((got.progress - 0.4).abs() < f64::EPSILON);
    assert_eq!(got.resume_hint, 9);

    assert!(db.jobs_in_state(JobState::Pending).unwrap().is_empty());
    assert_eq!(db.jobs_in_state(JobState::Running).unwrap().len(), 1);

    db.delete_job(1).unwrap();
    assert!(db.get_job(1).unwrap().is_none());
    assert!(!db.delete_job(1).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn data_survives_reopen() {
    let dir = temp_dir("reopen");
    let db_path = dir.join("db.redb");
    let a = AssetId::from_parts(3, 30, 300);

    {
        let db = AssetDb::open(&db_path).unwrap();
        db.upsert_asset(&sample_asset(a, "c.mp3", MediaType::Audio)).unwrap();
        db.record_cache_entry(a, CacheType::AudioProxy, Path::new("/cache/c.flac"))
            .unwrap();
    }
    {
        let db = AssetDb::open(&db_path).unwrap();
        assert!(db.get_asset(a).unwrap().is_some());
        assert!(db.has_cache_entry(a, CacheType::AudioProxy).unwrap());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_access() {
    let dir = temp_dir("concurrent");
    let db = Arc::new(AssetDb::open(&dir.join("db.redb")).unwrap());

    let handles: Vec<_> = (0..8u64)
        .map(|worker| {
            let db = Arc::clone(&db);
            std::thread::spawn(move || {
                for i in 0..10u64 {
                    let id = AssetId::from_parts(worker * 100 + i, worker, i);
                    let info = sample_asset(id, "f.wav", MediaType::Audio);
                    db.upsert_asset(&info).unwrap();
                    assert!(db.get_asset(id).unwrap().is_some());
                    db.record_cache_entry(id, CacheType::WaveformPeaks, Path::new("/cache/x"))
                        .unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(db.list_assets().unwrap().len(), 80);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupted_row_is_reported_not_panicked() {
    // Feeding garbage to the decoder must yield a clean error.
    assert!(tpt_av_asset_db::decode_media_info(&[0u8; 3]).is_err());
}
