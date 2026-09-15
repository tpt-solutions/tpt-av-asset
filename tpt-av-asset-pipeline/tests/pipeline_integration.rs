//! Pipeline integration tests: priority dispatch, cancellation,
//! crash/resume, dependencies, and the end-to-end `AssetImporter` flow.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tpt_av_asset_cache::CacheStorage;
use tpt_av_asset_db::{AssetDb, CacheType, JobState};
use tpt_av_asset_pipeline::{AssetImporter, Job, JobId, ProcessingPipeline, ProgressTracker};
use tpt_av_asset_utils::{AssetError, AssetId, Priority, ProgressReporter};
use tpt_kinetix::{gradient_painter, write_test_video};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-pipeline-it-{name}-{}-{}",
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

/// A test job that loops in small slices so it can be cancelled mid-run.
struct SlowJob {
    id: JobId,
    priority: Priority,
    slices: usize,
    done: Arc<AtomicUsize>,
    order: Arc<Mutex<Vec<u64>>>,
}

impl Job for SlowJob {
    fn id(&self) -> JobId {
        self.id
    }
    fn priority(&self) -> Priority {
        self.priority
    }
    fn asset_id(&self) -> AssetId {
        AssetId::from_parts(1, 1, 1)
    }
    fn kind(&self) -> &'static str {
        "slow"
    }
    fn payload(&self) -> String {
        String::new()
    }
    fn execute(&mut self, progress: &ProgressReporter) -> Result<(), AssetError> {
        for i in 0..self.slices {
            progress.check_cancelled()?;
            progress.report_ratio(i as f64, self.slices as f64);
            std::thread::sleep(Duration::from_millis(20));
            self.done.fetch_add(1, Ordering::SeqCst);
        }
        self.order.lock().unwrap().push(self.id.0);
        Ok(())
    }
}

fn slow(id: u64, priority: Priority, order: &Arc<Mutex<Vec<u64>>>) -> Box<SlowJob> {
    Box::new(SlowJob {
        id: JobId(id),
        priority,
        slices: 8,
        done: Arc::new(AtomicUsize::new(0)),
        order: Arc::clone(order),
    })
}

#[test]
fn critical_priority_dispatches_first() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut pipeline = ProcessingPipeline::new(1).unwrap();
    let low = pipeline.submit(slow(1, Priority::Low, &order)).unwrap();
    let critical = pipeline
        .submit(slow(2, Priority::Critical, &order))
        .unwrap();
    let normal = pipeline.submit(slow(3, Priority::Normal, &order)).unwrap();
    pipeline.start().unwrap();

    assert!(
        pipeline.wait_for_all(Duration::from_secs(15)),
        "jobs must finish"
    );
    pipeline.stop().unwrap();

    let order = order.lock().unwrap().clone();
    assert_eq!(order, vec![2, 3, 1], "critical → normal → low");
    assert_eq!(
        pipeline.get_progress(critical).unwrap().unwrap().state,
        JobState::Completed
    );
    assert_eq!(
        pipeline.get_progress(low).unwrap().unwrap().state,
        JobState::Completed
    );
    assert_eq!(
        pipeline.get_progress(normal).unwrap().unwrap().state,
        JobState::Completed
    );
}

#[test]
fn cancel_mid_execution_leaves_consistent_state() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut pipeline = ProcessingPipeline::new(1).unwrap();

    let first = pipeline.submit(slow(10, Priority::High, &order)).unwrap();
    let victim = pipeline.submit(slow(11, Priority::Normal, &order)).unwrap();
    pipeline.start().unwrap();

    // Wait until the victim starts running, then cancel it.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let state = pipeline.get_progress(victim).unwrap().unwrap().state;
        if state == JobState::Running {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "victim never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    pipeline.cancel(victim).unwrap();

    let final_state = pipeline
        .wait_for_timeout(victim, Duration::from_secs(5))
        .unwrap()
        .state;
    assert_eq!(final_state, JobState::Cancelled);

    // The first job still completes; pipeline remains healthy.
    pipeline.wait_for(first).unwrap();
    assert_eq!(
        pipeline.get_progress(first).unwrap().unwrap().state,
        JobState::Completed
    );

    // Cancelling an unknown id errors; cancelling a terminal job is a no-op.
    assert!(matches!(
        pipeline.cancel(JobId(9999)),
        Err(AssetError::JobNotFound(9999))
    ));
    pipeline.cancel(victim).unwrap();

    pipeline.stop().unwrap();
}

#[test]
fn failed_dependency_cancels_dependent() {
    use std::sync::atomic::AtomicBool;

    struct FailingJob {
        id: JobId,
    }
    impl Job for FailingJob {
        fn id(&self) -> JobId {
            self.id
        }
        fn priority(&self) -> Priority {
            Priority::High
        }
        fn asset_id(&self) -> AssetId {
            AssetId::from_parts(2, 2, 2)
        }
        fn kind(&self) -> &'static str {
            "failing"
        }
        fn payload(&self) -> String {
            String::new()
        }
        fn execute(&mut self, _p: &ProgressReporter) -> Result<(), AssetError> {
            Err(AssetError::codec("boom"))
        }
    }

    struct MarkJob {
        id: JobId,
        ran: Arc<AtomicBool>,
    }
    impl Job for MarkJob {
        fn id(&self) -> JobId {
            self.id
        }
        fn priority(&self) -> Priority {
            Priority::Normal
        }
        fn asset_id(&self) -> AssetId {
            AssetId::from_parts(3, 3, 3)
        }
        fn kind(&self) -> &'static str {
            "mark"
        }
        fn payload(&self) -> String {
            String::new()
        }
        fn execute(&mut self, _p: &ProgressReporter) -> Result<(), AssetError> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    let ran = Arc::new(AtomicBool::new(false));
    let mut pipeline = ProcessingPipeline::new(1).unwrap();
    pipeline.start().unwrap();

    let failing = pipeline
        .submit(Box::new(FailingJob {
            id: pipeline.next_job_id(),
        }))
        .unwrap();
    let dependent = pipeline
        .submit_with_deps(
            Box::new(MarkJob {
                id: pipeline.next_job_id(),
                ran: Arc::clone(&ran),
            }),
            vec![failing],
        )
        .unwrap();

    pipeline.wait_for(failing).unwrap();
    pipeline.wait_for(dependent).unwrap();

    assert_eq!(
        pipeline.get_progress(failing).unwrap().unwrap().state,
        JobState::Failed
    );
    assert_eq!(
        pipeline.get_progress(dependent).unwrap().unwrap().state,
        JobState::Cancelled,
        "dependent of a failed job must never run"
    );
    assert!(!ran.load(Ordering::SeqCst));
    pipeline.stop().unwrap();
}

#[test]
fn progress_tracker_reports_fractions() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut pipeline = ProcessingPipeline::new(1).unwrap();
    pipeline.start().unwrap();

    let job = pipeline.submit(slow(20, Priority::Normal, &order)).unwrap();
    // Observe some intermediate progress.
    let mut saw_progress = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(snapshot) = pipeline.wait_for_timeout(job, Duration::from_millis(5)) {
            if snapshot.state == JobState::Completed {
                break;
            }
            if snapshot.fraction > 0.0 && snapshot.fraction < 1.0 {
                saw_progress = true;
            }
        }
    }
    pipeline.wait_for(job).unwrap();
    let done = pipeline.get_progress(job).unwrap().unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert!((done.fraction - 1.0).abs() < f64::EPSILON);
    assert!(saw_progress, "intermediate fractions must be observable");

    pipeline.stop().unwrap();
}

#[test]
fn waveform_job_resume_across_pipeline_crash() {
    let dir = temp_dir("resume");
    let db = AssetDb::open(&dir.join("db.redb")).unwrap();
    let storage = CacheStorage::new(dir.join("cache"));

    let wav = dir.join("song.wav");
    tpt_cadence::write_test_wav(&wav, 4.0, 8_000, 1).unwrap();
    let asset = AssetId::from_path(&wav).unwrap();
    let mut info =
        tpt_av_asset_utils::MediaInfo::new(asset, &wav, tpt_av_asset_utils::MediaType::Audio);
    info.duration_secs = Some(4.0);
    info.audio = Some(tpt_av_asset_utils::AudioInfo {
        sample_rate: 8_000,
        channels: 1,
        bit_depth: 16,
        codec: "pcm_s16le".into(),
        bit_rate: None,
        duration_secs: 4.0,
    });
    db.upsert_asset(&info).unwrap();

    // Deterministic partial progress: run the generator directly and cancel
    // it at 10% — exactly what an interrupted/crashed generation leaves
    // behind on disk.
    let total_chunks = 4usize * 8_000;
    let total_chunks = total_chunks.div_ceil(1024); // 31.25 → 32 chunks (last partial)
    let mut cache = tpt_av_asset_cache::WaveformCache::open(asset, &storage).unwrap();
    let reporter = ProgressReporter::with_self_callback(|token| {
        move |e| {
            if e.fraction >= 0.1 {
                token.cancel();
            }
        }
    });
    let result = tpt_av_asset_cache::WaveformGenerator::new(1024, 8_000)
        .generate(&wav, &mut cache, &reporter);
    assert!(matches!(result, Err(AssetError::Cancelled)));
    let partial = cache.chunk_count();
    assert!(partial > 0 && partial < total_chunks);
    drop(cache);

    // A pipeline schedules the waveform job and "crashes" before starting:
    // the record is persisted as Pending.
    let first = ProcessingPipeline::with_db(1, db.clone()).unwrap();
    first
        .submit(Box::new(tpt_av_asset_pipeline::WaveformJob::new(
            first.next_job_id(),
            asset,
            wav.clone(),
            1024,
            8_000,
            storage.clone(),
            db.clone(),
        )))
        .unwrap();
    // No start(): the process "dies" with the job queued.
    assert_eq!(db.jobs_in_state(JobState::Pending).unwrap().len(), 1);
    drop(first);

    // Restart: recover, re-enqueue, and finish.
    let mut second = ProcessingPipeline::with_db(1, db.clone()).unwrap();
    second.start().unwrap();
    let recovered = second.recover_interrupted(&db, &storage).unwrap();
    assert_eq!(recovered, 1, "the pending waveform job must be re-enqueued");
    assert!(
        second.wait_for_all(Duration::from_secs(30)),
        "recovered job must finish"
    );
    second.stop().unwrap();

    let cache = tpt_av_asset_cache::WaveformCache::open(asset, &storage).unwrap();
    assert_eq!(
        cache.chunk_count(),
        total_chunks,
        "resumed generation completes"
    );
    assert!(
        cache.chunk_count() >= partial,
        "chunk count must never regress"
    );
    assert!(db.has_cache_entry(asset, CacheType::WaveformPeaks).unwrap());
    let record = &db.all_jobs().unwrap()[0];
    assert_eq!(record.state, JobState::Completed);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn importer_end_to_end_populates_all_caches() {
    let dir = temp_dir("e2e");

    let db = AssetDb::open(&dir.join("db.redb")).unwrap();
    let storage = CacheStorage::new(dir.join("cache"));
    let wav = dir.join("song.wav");
    tpt_cadence::write_test_wav(&wav, 1.5, 8_000, 1).unwrap();

    let mut pipeline = ProcessingPipeline::with_db(2, db.clone()).unwrap();
    pipeline.start().unwrap();
    let importer = AssetImporter::new(pipeline.clone(), db.clone(), storage.clone())
        .with_thumbnails(0.5, (64, 36))
        .with_chunk_size(512);
    let asset = importer.import(&wav).unwrap();

    assert!(
        pipeline.wait_for_all(Duration::from_secs(30)),
        "import jobs must finish"
    );

    assert!(db.get_asset(asset).unwrap().is_some());
    assert!(db.has_cache_entry(asset, CacheType::WaveformPeaks).unwrap());
    assert!(db.has_cache_entry(asset, CacheType::AudioProxy).unwrap());
    assert!(storage.waveform_path(asset).exists());
    assert!(storage.proxy_path(asset, "flac").exists());

    // --- Video asset ---
    let video = dir.join("clip.tkv");
    write_test_video(&video, 192, 108, 10.0, 1.0, gradient_painter).unwrap();
    let video_asset = importer.import(&video).unwrap();
    assert!(pipeline.wait_for_all(Duration::from_secs(30)));

    assert!(db
        .has_cache_entry(video_asset, CacheType::VideoThumbnails)
        .unwrap());
    assert!(db
        .has_cache_entry(video_asset, CacheType::VideoProxy)
        .unwrap());
    assert!(storage.thumbnail_dir(video_asset).exists());
    assert!(storage.proxy_path(video_asset, "mp4").exists());

    // import() returns immediately with an indexed asset:
    assert_eq!(db.list_assets().unwrap().len(), 2);
    pipeline.stop().unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unsupported_format_is_rejected_on_import() {
    let dir = temp_dir("unsupported");
    let db = AssetDb::open(&dir.join("db.redb")).unwrap();
    let storage = CacheStorage::new(dir.join("cache"));
    let mystery = dir.join("mystery.bin");
    std::fs::write(&mystery, b"not media").unwrap();

    let pipeline = ProcessingPipeline::new(1).unwrap();
    let importer = AssetImporter::new(pipeline, db, storage);
    let err = importer.import(&mystery).unwrap_err();
    assert!(matches!(err, AssetError::UnsupportedFormat(_)));

    // Images import (no background jobs) without decoders.
    let picture = dir.join("photo.png");
    std::fs::write(&picture, b"\x89PNG\r\n\x1a\nstub").unwrap();
    let asset = importer.import(&picture).unwrap();
    assert_eq!(
        importer.db().get_asset(asset).unwrap().unwrap().media_type,
        tpt_av_asset_utils::MediaType::Image
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tracker_isolation_between_pipelines() {
    let tracker = ProgressTracker::new();
    let id = JobId(1);
    tracker.register(id);
    assert_eq!(tracker.snapshots().len(), 1);
    assert_eq!(tracker.snapshot(JobId(2)), None);
}
