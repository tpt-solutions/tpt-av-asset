# tpt-av-asset — Project Todo

Source of truth for API shapes, architecture, and rationale: [`spec.txt`](spec.txt).

**Status (2026-09-16):** Phases 0–7 implemented locally. 84 tests green;
`cargo fmt` / `clippy -D warnings` / `cargo doc -D warnings` / `cargo deny
check` all clean; all four examples run end-to-end. Remaining items are the
GitHub-side tasks (no `gh` credentials on this machine) and the steps that
are blocked on the real `tpt-kinetix` / `tpt-cadence` repositories.

**Deviations from `spec.txt` tracked here:**
- **License:** dual-licensed **MIT OR Apache-2.0** (not MIT-only as spec.txt states). Author/owner: TPT Solutions.
- **External deps:** `tpt-kinetix` (video) and `tpt-cadence` (audio) don't exist yet. Early phases build against local stub/trait abstractions under [`stubs/`](stubs) (workspace members, `publish = false`); real git dependencies get swapped in once those repos are ready (see Phase 7). The stubs provide a real PCM WAV decoder/encoder and a synthetic `TKV1` RGBA video container, so the whole engine is testable today.
- **`examples/` is a seventh workspace member** (`tpt-av-asset-examples`, `publish = false`) so the four demo binaries build with plain `cargo`; the six core crates remain the publishable set.
- **`Priority` variants are declared `Low → Critical`** so the derived `Ord` ranks Critical highest (spec.txt declared Critical first, which would invert `Ord`).
- **Audio proxy encoding:** the stand-in `tpt-cadence` writes 16-bit PCM WAV bytes into the `.flac`-named proxy output; real FLAC arrives with the real crate.
- **`deny.toml`:** with cargo-deny 0.20 the license allow-list *is* the policy (explicit `deny`/`copyleft` license keys were removed upstream); `Unicode-3.0`/`Unicode-DFS-2016` were added for `unicode-ident`'s data files.

---

### Phase 0 — Repository & Infrastructure Setup

- [x] `git init` in the project root
- [x] Add `.gitignore` (`/target`, `Cargo.lock` policy for workspace, editor files, `~/.tpt-av-asset/` local cache dir if ever created in-repo for testing)
- [x] Add `LICENSE-MIT` (standard MIT text, TPT Solutions copyright)
- [x] Add `LICENSE-APACHE` (Apache License 2.0 text)
- [x] Write `README.md` (project overview, dual-license badges, ecosystem links)
- [x] Write `DESIGN.md` (adapt from `spec.txt`, updated for dual licensing)
- [x] Write `CONTRIBUTING.md` noting contributions are dual-licensed MIT OR Apache-2.0 and must not introduce GPL/LGPL/AGPL/MPL deps
- [x] Create workspace `Cargo.toml` with `license = "MIT OR Apache-2.0"` and the 6 workspace members
- [x] Create `deny.toml` (license allow-list per spec section 7)
- [x] Scaffold all 6 member crate directories with placeholder `Cargo.toml` + empty `src/lib.rs`:
  - [x] `tpt-av-asset-utils`
  - [x] `tpt-av-asset-db`
  - [x] `tpt-av-asset-cache`
  - [x] `tpt-av-asset-proxy`
  - [x] `tpt-av-asset-watcher`
  - [x] `tpt-av-asset-pipeline`
- [x] Create `examples/` directory (placeholders for the 4 example binaries; later a workspace member with real demos)
- [x] Define local stub crates/traits standing in for `tpt-kinetix` (video decode trait: open file, iterate frames, get `VideoInfo`) and `tpt-cadence` (audio decode trait: open file, iterate PCM samples, get `AudioInfo`) so downstream code compiles/tests without the real git dependencies
- [ ] Create GitHub repo `github.com/tpt-solutions/tpt-av-asset` *(blocked: no `gh` CLI/credentials on this machine)*
- [ ] Push initial scaffold, set default branch, add repo description/topics *(blocked: same)*
- [x] Add GitHub Actions CI workflow: `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo deny check` (+ `cargo doc -D warnings`)
- [ ] Enable branch protection on default branch (require CI passing before merge) *(needs repo admin on GitHub)*

---

### Phase 1 — Foundation & Database

**`tpt-av-asset-utils`**
- [x] `error.rs` — `AssetError` enum covering I/O, db, codec, and validation failures
- [x] `asset_id.rs` — `AssetId` struct + `AssetId::from_path()` (content-addressed via path hash + mtime + size)
- [x] `media_info.rs` — `MediaInfo`, `MediaType`, `VideoInfo`, `AudioInfo`
- [x] `time_range.rs` — time range type used across cache/proxy APIs
- [x] `priority.rs` — `Priority` enum (Critical/High/Normal/Low)
- [x] `progress.rs` — `ProgressReporter` / progress reporting types (+ cancellation token)
- [x] Unit tests for `AssetId` stability/invalidation behavior (same file → same id; modified file → new id)

**`tpt-av-asset-db`**
- [x] Add `redb` dependency, `schema.rs` defining table definitions
- [x] `asset_table.rs` — asset metadata storage
- [x] `cache_table.rs` — cache entry tracking (`CacheType` enum: WaveformPeaks/VideoThumbnails/VideoProxy/AudioProxy)
- [x] `job_table.rs` — job queue persistence
- [x] `transaction.rs` — atomic transaction helpers
- [x] `query.rs` — query API
- [x] Implement `AssetDb`: `open`, `upsert_asset`, `get_asset`, `list_assets`, `remove_asset`, `record_cache_entry`, `has_cache_entry`, `invalidate_caches`
- [x] Unit tests: `tests/` — insert/get/list/remove asset; cache entry record/check/invalidate; concurrent access
- [x] Verify `cargo deny check` passes (pure MIT/Apache-2.0 dependency tree)

---

### Phase 2 — Waveform Caching

- [x] `storage.rs` — on-disk `CacheStorage` (file layout: `cache/waveforms/{asset_id_hash}.peaks`)
- [x] `waveform.rs` — `WaveformCache`, `WaveformChunk` (min/max/RMS), `open`/`write_chunk`/`read_chunk`/`read_range`/`chunk_count`
- [x] `reader.rs` — real-time safe read path (allocation-free, lock-free positioned reads); documented and test-verified
- [x] `invalidation.rs` — cache invalidation logic tied into `tpt-av-asset-db`
- [x] `WaveformGenerator::new` + `generate()` implemented against the `tpt-cadence` stub trait (reads PCM, computes min/max/RMS per chunk, writes to cache, reports progress)
- [x] Benchmark/test proving `read_chunk`/`read_range_into` make no heap allocations (global-allocator counting harness in `tests/alloc_free.rs`; `read_range` is the allocating convenience variant)
- [x] Integration tests: generate + read back waveform peaks for a synthetic/test WAV file

---

### Phase 3 — Thumbnail Caching

- [x] `thumbnail.rs` — `ThumbnailCache`, `Thumbnail`, `open`/`write_thumbnail`/`read_thumbnail`/`read_nearest`/`thumbnail_count`
- [x] Cache storage layout: `cache/thumbnails/{asset_id_hash}/NNNNNN.jpg` (+ `meta` sidecar)
- [x] Add `image` crate dependency for resizing/compression
- [x] `ThumbnailGenerator::new` + `generate()` implemented against the `tpt-kinetix` stub trait (decode frames at interval, resize, write to cache, report progress)
- [x] Integration tests: generate + read back thumbnails for a synthetic/test video source (via stub)

---

### Phase 4 — Proxy Generation

- [x] `profile.rs` — `ProxyProfile` + presets: `proxy_1080p_low()`, `proxy_720p_medium()`, `audio_proxy_flac()`
- [x] `encoder.rs` — lightweight encoder wrapper (wraps `tpt-kinetix` stub for now)
- [x] `video_proxy.rs` — video proxy generation (decode → resize → re-encode → write)
- [x] `audio_proxy.rs` — audio proxy generation (decode PCM → re-encode → write)
- [x] `generator.rs` — high-level `ProxyGenerator` (`generate_video_proxy`, `generate_audio_proxy`)
- [x] Progress reporting wired through `ProgressReporter`
- [x] Cancellation support (generator checks a cancellation token/flag during long loops; partial outputs are deleted)
- [x] Integration tests: generate a proxy from a synthetic source, verify output profile matches target resolution/bitrate

---

### Phase 5 — Filesystem Watching

- [x] Add `notify` crate dependency
- [x] `event.rs` — `FileEvent`, `FileEventType` (Created/Modified/Deleted/Renamed)
- [x] `debounce.rs` — event debouncing logic
- [x] `watcher.rs` — `MediaWatcher` (`new`, `watch`, `unwatch`, `recv`, `try_recv`) wrapping backend
- [x] `backend/` platform implementations:
  - [x] `read_dir.rs` (Windows fallback) — built and tested first since dev is on Windows
  - [x] `inotify.rs` (Linux) — wraps the `notify` crate (inotify under the hood)
  - [x] `fsevents.rs` (macOS) — wraps the `notify` crate (FSEvents under the hood)
- [x] Wire watcher events into `tpt-av-asset-db::invalidate_caches` for automatic cache invalidation on file change/move/delete (`CacheInvalidator`)
- [x] Integration tests on Windows (local); CI matrix (ubuntu-latest, macos-latest, windows-latest) covers all three backends in GitHub Actions *(matrix config committed; runs once the repo is pushed)*

---

### Phase 6 — Background Pipeline

- [x] `job.rs` — `Job` trait (`id`, `priority`, `execute`, `asset_id`), `JobId` type
- [x] `queue.rs` — `JobQueue` (prioritized by `Priority`)
- [x] `worker.rs` — `WorkerPool` (thread pool executing jobs; panic-isolated)
- [x] `scheduler.rs` — scheduling logic (priorities, dependencies between jobs)
- [x] `progress.rs` — `ProgressTracker` / `JobProgress`
- [x] `pipeline.rs` — `ProcessingPipeline` (`new`, `submit`, `cancel`, `get_progress`, `wait_for`, `start`, `stop`)
- [x] Job cancellation tests (mid-execution cancel leaves no corrupt state)
- [x] Job resumption tests (partial progress persisted, resumable after simulated crash/restart via `recover_interrupted`)
- [x] `AssetImporter` high-level API (`new`, `import()`): compute asset id → extract media info → insert into db → schedule waveform/thumbnail/proxy jobs → return asset id immediately
- [x] End-to-end test: import a test media file through `AssetImporter` and confirm all cache types get populated via the pipeline

---

### Phase 7 — Examples, Docs & Release

- [x] `examples/import_media.rs` — import a media file and generate all caches
- [x] `examples/waveform_viewer.rs` — display waveform peaks from cache
- [x] `examples/thumbnail_browser.rs` — browse video thumbnails
- [x] `examples/proxy_generator.rs` — generate proxies for a folder of videos
- [x] Finalize `README.md` / `DESIGN.md` with accurate dual-license badges and crate table
- [x] Add rustdoc comments across public APIs; verify `cargo doc` builds cleanly (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`)
- [ ] Swap stub `tpt-kinetix`/`tpt-cadence` traits for the real git dependencies once those repos are ready; re-run full test/integration suite against real decoders *(blocked: repos don't exist yet)*
- [ ] Cross-platform CI green on Linux/macOS/Windows *(workflow committed; needs the GitHub remote)*
- [x] Tag `v0.1.0` *(tagged locally on the release commit; push with the repo)*
- [x] Dry-run packaging for each of the 6 crates in dependency order (`cargo package`: utils verified; db+ resolve against the registry only after the preceding crate is actually published)
- [ ] Publish crates to crates.io in dependency order (utils → db → cache → proxy → watcher → pipeline) *(blocked: needs crates.io token, published `tpt-kinetix`/`tpt-cadence`, and the preceding crates in the index)*
