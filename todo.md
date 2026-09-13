# tpt-av-asset — Project Todo

Source of truth for API shapes, architecture, and rationale: [`spec.txt`](spec.txt).

**Deviations from `spec.txt` tracked here:**
- **License:** dual-licensed **MIT OR Apache-2.0** (not MIT-only as spec.txt states). Author/owner: TPT Solutions.
- **External deps:** `tpt-kinetix` (video) and `tpt-cadence` (audio) don't exist yet. Early phases build against local stub/trait abstractions; real git dependencies get swapped in once those repos are ready (see Phase 7).

---

### Phase 0 — Repository & Infrastructure Setup

- [ ] `git init` in the project root
- [ ] Add `.gitignore` (`/target`, `Cargo.lock` policy for workspace, editor files, `~/.tpt-av-asset/` local cache dir if ever created in-repo for testing)
- [ ] Add `LICENSE-MIT` (standard MIT text, TPT Solutions copyright)
- [ ] Add `LICENSE-APACHE` (Apache License 2.0 text)
- [ ] Write `README.md` (project overview, dual-license badges, ecosystem links)
- [ ] Write `DESIGN.md` (adapt from `spec.txt`, updated for dual licensing)
- [ ] Write `CONTRIBUTING.md` noting contributions are dual-licensed MIT OR Apache-2.0 and must not introduce GPL/LGPL/AGPL/MPL deps
- [ ] Create workspace `Cargo.toml` with `license = "MIT OR Apache-2.0"` and the 6 workspace members
- [ ] Create `deny.toml` (license allow/deny lists per spec section 7)
- [ ] Scaffold all 6 member crate directories with placeholder `Cargo.toml` + empty `src/lib.rs`:
  - [ ] `tpt-av-asset-utils`
  - [ ] `tpt-av-asset-db`
  - [ ] `tpt-av-asset-cache`
  - [ ] `tpt-av-asset-proxy`
  - [ ] `tpt-av-asset-watcher`
  - [ ] `tpt-av-asset-pipeline`
- [ ] Create `examples/` directory (empty placeholders for the 4 example binaries)
- [ ] Define local stub crates/traits standing in for `tpt-kinetix` (video decode trait: open file, iterate frames, get `VideoInfo`) and `tpt-cadence` (audio decode trait: open file, iterate PCM samples, get `AudioInfo`) so downstream code compiles/tests without the real git dependencies
- [ ] Create GitHub repo `github.com/tpt-solutions/tpt-av-asset`
- [ ] Push initial scaffold, set default branch, add repo description/topics
- [ ] Add GitHub Actions CI workflow: `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo deny check`
- [ ] Enable branch protection on default branch (require CI passing before merge)

---

### Phase 1 — Foundation & Database

**`tpt-av-asset-utils`**
- [ ] `error.rs` — `AssetError` enum covering I/O, db, codec, and validation failures
- [ ] `asset_id.rs` — `AssetId` struct + `AssetId::from_path()` (content-addressed via path hash + mtime + size)
- [ ] `media_info.rs` — `MediaInfo`, `MediaType`, `VideoInfo`, `AudioInfo`
- [ ] `time_range.rs` — time range type used across cache/proxy APIs
- [ ] `priority.rs` — `Priority` enum (Critical/High/Normal/Low)
- [ ] `progress.rs` — `ProgressReporter` / progress reporting types
- [ ] Unit tests for `AssetId` stability/invalidation behavior (same file → same id; modified file → new id)

**`tpt-av-asset-db`**
- [ ] Add `redb` dependency, `schema.rs` defining table definitions
- [ ] `asset_table.rs` — asset metadata storage
- [ ] `cache_table.rs` — cache entry tracking (`CacheType` enum: WaveformPeaks/VideoThumbnails/VideoProxy/AudioProxy)
- [ ] `job_table.rs` — job queue persistence
- [ ] `transaction.rs` — atomic transaction helpers
- [ ] `query.rs` — query API
- [ ] Implement `AssetDb`: `open`, `upsert_asset`, `get_asset`, `list_assets`, `remove_asset`, `record_cache_entry`, `has_cache_entry`, `invalidate_caches`
- [ ] Unit tests: `tests/` — insert/get/list/remove asset; cache entry record/check/invalidate; concurrent access
- [ ] Verify `cargo deny check` passes (pure MIT/Apache-2.0 dependency tree)

---

### Phase 2 — Waveform Caching

- [ ] `storage.rs` — on-disk `CacheStorage` (file layout: `cache/waveforms/{asset_id_hash}.peaks`)
- [ ] `waveform.rs` — `WaveformCache`, `WaveformChunk` (min/max/RMS), `open`/`write_chunk`/`read_chunk`/`read_range`/`chunk_count`
- [ ] `reader.rs` — real-time safe read path (allocation-free, lock-free); document and test this guarantee explicitly
- [ ] `invalidation.rs` — cache invalidation logic tied into `tpt-av-asset-db`
- [ ] `WaveformGenerator::new` + `generate()` implemented against the `tpt-cadence` stub trait (reads PCM, computes min/max/RMS per chunk, writes to cache, reports progress)
- [ ] Benchmark/test proving `read_chunk`/`read_range` make no heap allocations (e.g. via a allocation-counting test harness)
- [ ] Integration tests: generate + read back waveform peaks for a synthetic/test WAV file

---

### Phase 3 — Thumbnail Caching

- [ ] `thumbnail.rs` — `ThumbnailCache`, `Thumbnail`, `open`/`write_thumbnail`/`read_thumbnail`/`read_nearest`/`thumbnail_count`
- [ ] Cache storage layout: `cache/thumbnails/{asset_id_hash}/NNNNNN.jpg`
- [ ] Add `image` crate dependency for resizing/compression
- [ ] `ThumbnailGenerator::new` + `generate()` implemented against the `tpt-kinetix` stub trait (decode frames at interval, resize, write to cache, report progress)
- [ ] Integration tests: generate + read back thumbnails for a synthetic/test video source (via stub)

---

### Phase 4 — Proxy Generation

- [ ] `profile.rs` — `ProxyProfile` + presets: `proxy_1080p_low()`, `proxy_720p_medium()`, `audio_proxy_flac()`
- [ ] `encoder.rs` — lightweight encoder wrapper (wraps `tpt-kinetix` stub for now)
- [ ] `video_proxy.rs` — video proxy generation (decode → resize → re-encode → write)
- [ ] `audio_proxy.rs` — audio proxy generation (decode PCM → re-encode → write)
- [ ] `generator.rs` — high-level `ProxyGenerator` (`generate_video_proxy`, `generate_audio_proxy`)
- [ ] Progress reporting wired through `ProgressReporter`
- [ ] Cancellation support (generator checks a cancellation token/flag during long loops)
- [ ] Integration tests: generate a proxy from a synthetic source, verify output profile matches target resolution/bitrate

---

### Phase 5 — Filesystem Watching

- [ ] Add `notify` crate dependency
- [ ] `event.rs` — `FileEvent`, `FileEventType` (Created/Modified/Deleted/Renamed)
- [ ] `debounce.rs` — event debouncing logic
- [ ] `watcher.rs` — `MediaWatcher` (`new`, `watch`, `unwatch`, `recv`, `try_recv`) wrapping backend
- [ ] `backend/` platform implementations:
  - [ ] `read_dir.rs` (Windows fallback) — build and test first since dev is on Windows
  - [ ] `inotify.rs` (Linux)
  - [ ] `fsevents.rs` (macOS)
- [ ] Wire watcher events into `tpt-av-asset-db::invalidate_caches` for automatic cache invalidation on file change/move/delete
- [ ] Integration tests on Windows (local); add CI matrix (ubuntu-latest, macos-latest, windows-latest) to test all three backends in GitHub Actions

---

### Phase 6 — Background Pipeline

- [ ] `job.rs` — `Job` trait (`id`, `priority`, `execute`, `asset_id`), `JobId` type
- [ ] `queue.rs` — `JobQueue` (prioritized by `Priority`)
- [ ] `worker.rs` — `WorkerPool` (thread pool executing jobs)
- [ ] `scheduler.rs` — scheduling logic (priorities, dependencies between jobs)
- [ ] `progress.rs` — `ProgressTracker` / `JobProgress`
- [ ] `pipeline.rs` — `ProcessingPipeline` (`new`, `submit`, `cancel`, `get_progress`, `wait_for`, `start`, `stop`)
- [ ] Job cancellation tests (mid-execution cancel leaves no corrupt state)
- [ ] Job resumption tests (partial progress persisted, resumable after simulated crash/restart)
- [ ] `AssetImporter` high-level API (`new`, `import()`): compute asset id → extract media info → insert into db → schedule waveform/thumbnail/proxy jobs → return asset id immediately
- [ ] End-to-end test: import a test media file through `AssetImporter` and confirm all cache types get populated via the pipeline

---

### Phase 7 — Examples, Docs & Release

- [ ] `examples/import_media.rs` — import a media file and generate all caches
- [ ] `examples/waveform_viewer.rs` — display waveform peaks from cache
- [ ] `examples/thumbnail_browser.rs` — browse video thumbnails
- [ ] `examples/proxy_generator.rs` — generate proxies for a folder of videos
- [ ] Finalize `README.md` / `DESIGN.md` with accurate dual-license badges and crate table
- [ ] Add rustdoc comments across public APIs; verify `cargo doc` builds cleanly
- [ ] Swap stub `tpt-kinetix`/`tpt-cadence` traits for the real git dependencies once those repos are ready; re-run full test/integration suite against real decoders
- [ ] Cross-platform CI green on Linux/macOS/Windows
- [ ] Tag `v0.1.0`
- [ ] Dry-run `cargo publish --dry-run` for each of the 6 crates in dependency order
- [ ] Publish crates to crates.io in dependency order (utils → db → cache → proxy → watcher → pipeline)
