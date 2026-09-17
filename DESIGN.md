# tpt-av-asset — Design

**A pure-Rust, background media asset management engine. Proxy generation,
waveform caching, thumbnail indexing, and filesystem monitoring. The
performance backbone of the TPT AV Stack.**

License: **MIT OR Apache-2.0** · Status: early-stage / pre-1.0 ·
Author/owner: TPT Solutions.

> This document is the design-of-record. The original specification lives in
> [`spec.txt`](spec.txt); where the two differ, this file and
> [`todo.md`](todo.md) (see "Deviations") are authoritative — notably the
> dual MIT/Apache-2.0 licensing and the interim decode-only integration
> strategy for the real `tpt-kinetix`/`tpt-cadence` crates.

## 1. Vision & Philosophy

`tpt-av-asset` is the **media asset management layer** of the TPT AV Stack.
Most open-source media tools either block the UI while generating waveforms
or proxies, rely on proprietary asset systems, or have no asset management at
all. `tpt-av-asset` provides the background infrastructure that makes
professional media applications feel instantaneous.

### Core tenets

1. **Background-first architecture** — all heavy processing (proxies,
   waveforms, thumbnails) happens on background thread pools. The UI thread
   never blocks.
2. **Incremental & resumable** — jobs persist partial progress and can be
   paused, cancelled, and resumed after crashes. Partial progress is never
   lost.
3. **Cache-invalidation aware** — when source files change, caches are
   automatically invalidated and regenerated.
4. **Pure Rust, no C/C++ bindings** — `redb` for the embedded database; no
   SQLite, no ffmpeg.
5. **Real-time safe reads** — cache reads (waveform peaks) are
   allocation-free and lock-free, safe for the audio/render thread.
6. **Permissive licensing only** — dual-licensed MIT OR Apache-2.0; no
   GPL/LGPL/AGPL/MPL dependencies anywhere in the tree (enforced by
   `cargo-deny`).
7. **Composable architecture** — each sub-crate is independently useful.

## 2. Ecosystem integration

`tpt-av-asset` sits in the **performance layer** of the TPT AV Stack.

| Crate | Role | Relationship |
| :--- | :--- | :--- |
| `tpt-kinetix` | Media containers, video codecs | Source of video frames for thumbnails/proxies. |
| `tpt-cadence` | Audio codecs | Source of PCM data for waveforms/audio proxies. |
| `tpt-audio` | Audio processing | Reads waveform peaks for UI rendering. |
| `tpt-visual` | Video processing | Reads thumbnails for timeline scrubbing. |
| **`tpt-av-asset`** | **Asset management (this repo)** | Background proxy generation, caching, indexing, watching. |

### Data flow

```text
User imports "4K_interview.mp4" (2 hours, 50GB)
→ tpt-av-asset-watcher detects new file
→ tpt-av-asset-db indexes file metadata (duration, resolution, codecs)
→ tpt-av-asset-pipeline schedules background jobs:
    Job 1: Generate proxy (1080p, low bitrate)
    Job 2: Extract video thumbnails (every N seconds)
    Job 3: Generate waveform peaks for the audio track
→ Background worker pool processes jobs (prioritized, resumable)
→ tpt-av-asset-cache stores results on disk
→ tpt-audio / tpt-visual read from cache (real-time safe, instant)
```

## 3. Repository architecture

Cargo workspace; all sub-crates share the `tpt-av-asset-` prefix.

```text
tpt-av-asset/
├── Cargo.toml                       # workspace manifest
├── deny.toml                        # cargo-deny license audit
├── LICENSE-MIT / LICENSE-APACHE
├── README.md / DESIGN.md / CONTRIBUTING.md
├── tpt-av-asset-utils/              # shared types, error handling
├── tpt-av-asset-db/                 # embedded media database (redb)
├── tpt-av-asset-cache/              # waveform + thumbnail caches
├── tpt-av-asset-proxy/              # proxy generation engine
├── tpt-av-asset-watcher/            # filesystem monitoring
├── tpt-av-asset-pipeline/           # background processing orchestration
├── stubs/                           # stand-ins for tpt-kinetix / tpt-cadence
└── examples/                        # demo binaries
```

Dependency direction (no cycles):

```text
utils ← db ← cache ← proxy ← watcher ← pipeline
```

## 4. Core API design

### 4.1 Asset identification (`tpt-av-asset-utils`)

`AssetId` is content-addressed: `(hash(path), mtime_ms, size)`. If a file is
modified it gets a new ID, and old caches become invalid automatically (the
cache filename embeds the full ID hash, so stale entries simply stop being
referenced and can be pruned).

`MediaInfo` carries file metadata plus optional `VideoInfo` (width, height,
frame rate, codec, pixel format, bit rate) and `AudioInfo` (sample rate,
channels, bit depth, codec, bit rate). Supporting types: `TimeRange`,
`Priority` (Low < Normal < High < Critical), `ProgressReporter` (progress
callback + cancellation flag, `Send + Sync`, cheap to clone).

### 4.2 Media database (`tpt-av-asset-db`)

`AssetDb` wraps a `redb::Database` with three tables:

| Table | Key | Value |
| :--- | :--- | :--- |
| `assets` | 24-byte `AssetId` triple | encoded `MediaInfo` |
| `cache_entries` | asset key + `CacheType` | cache file path |
| `jobs` | `u64` job id | encoded `JobRecord` |

Values are encoded with a compact hand-rolled little-endian binary format
(no serde/bincode dependency). All mutations go through short write
transactions; reads use read transactions, so concurrent access is safe.
API: `open`, `upsert_asset`, `get_asset`, `get_asset_by_path`,
`list_assets`, `remove_asset`, `record_cache_entry`, `has_cache_entry`,
`list_cache_entries`, `invalidate_caches`, plus job-table accessors used by
the pipeline for crash recovery.

### 4.3 Waveform cache (`tpt-av-asset-cache`)

Fixed-size chunks of audio are summarized as `(min, max, rms)` peaks. The
on-disk format (`.peaks`) is a small header (magic, chunk size, sample rate,
chunk count) followed by fixed 12-byte records, so any chunk is reachable
with a single positioned read.

**Real-time safety:** `WaveformReader::read_chunk` performs exactly one
`seek + read` into a stack buffer and returns a `Copy` value — no heap
allocation, no userspace lock. This is guarded by a global-allocator
allocation-counting test. `read_range` is a convenience API and is *not*
real-time safe; `read_range_into` is the allocation-free variant.

`WaveformGenerator` opens the source with the `tpt-cadence` decode trait,
buckets PCM samples per chunk, computes min/max/RMS, appends to the cache
(resuming from `chunk_count()` on re-run), reports progress, and aborts
cleanly on cancellation.

### 4.4 Thumbnail cache (`tpt-av-asset-cache`)

One JPEG per interval in `thumbnails/{asset_hash}/NNNNNN.jpg`, plus a `meta`
sidecar (interval, resolution, source duration). `read_nearest` picks the
closest cached thumbnail to a requested time. `ThumbnailGenerator` seeks the
`tpt-kinetix` decoder at each interval boundary, resizes via the `image`
crate, and encodes JPEG.

### 4.5 Proxy generation (`tpt-av-asset-proxy`)

`ProxyProfile` presets: `proxy_1080p_low` (1920×1080),
`proxy_720p_medium` (1280×720), `audio_proxy_flac`. Video proxies decode
through the kinetix stack (MP4/H.264 or the TPT proxy stream), aspect-fit
into the target resolution, and re-encode with `tpt-kinetix-lossless` —
the only encoder in the kinetix stack today — into `.tkvp` files. Audio
proxies decode through the cadence readers and are stored as 16-bit PCM
WAV until the cadence `Encoder` trait ships FLAC. Progress reporting and
cooperative cancellation are wired through `ProgressReporter`; a cancelled
run deletes its partial output file.

### 4.6 Filesystem watcher (`tpt-av-asset-watcher`)

`MediaWatcher` exposes `watch` / `unwatch` / `recv` / `try_recv` over
platform backends behind a `WatcherBackend` trait:

| Backend | Platform | Mechanism |
| :--- | :--- | :--- |
| `backend/read_dir.rs` | Windows (default), any | Periodic directory diff (polling). |
| `backend/inotify.rs` | Linux | `notify` crate (inotify under the hood). |
| `backend/fsevents.rs` | macOS | `notify` crate (FSEvents under the hood). |

Events are `Created` / `Modified` / `Deleted` / `Renamed { old_path }`, and
pass through a per-path time-window debouncer (coalescing edit storms).
`CacheInvalidator` bridges watcher events to `AssetDb::invalidate_caches` +
on-disk cache removal, using the path index to find the affected asset.

### 4.7 Background pipeline (`tpt-av-asset-pipeline`)

`Job` trait: `id`, `priority`, `asset_id`, `execute(&ProgressReporter)`.
`JobQueue` is a priority queue (Critical first, FIFO within priority);
`Scheduler` adds inter-job dependencies; `WorkerPool` runs N worker threads;
`ProgressTracker` tracks per-job `JobProgress` (state machine: Pending →
Running → Completed / Failed / Cancelled) and supports blocking `wait_for`.

`ProcessingPipeline` persists job records to the db's `jobs` table on every
state transition, enabling crash recovery: after restart, `recover_jobs`
re-enqueues unfinished jobs and the concrete generators (waveform, thumbs)
naturally skip already-completed chunks/thumbnails.

`AssetImporter::import(path)` computes the `AssetId`, probes `MediaInfo`
through the decoder traits, upserts the db row, schedules
waveform/thumbnail/proxy jobs, and returns immediately.

## 5. Performance architecture

### Multi-level caching

```text
Level 1: in-process reader state (open fd, header)   — fast, no allocation
Level 2: on-disk cache (.peaks / .jpg / proxies)     — persistent, restart-safe
Level 3: source file (decode on demand)              — only on cache miss
```

### Thread architecture

| Thread | Constraints |
| :--- | :--- |
| Main/UI | May allocate and block; queries db; reads caches RT-safely. |
| Background workers | May allocate and block; generate caches/proxies. |
| File watcher | Monitors the filesystem; triggers invalidation. |
| Audio/render | **Zero allocation, no locks**; only `read_chunk`. |

### Storage layout

```text
~/.tpt-av-asset/
├── db/assets.redb
└── cache/
    ├── waveforms/{asset_id_hash}.peaks
    ├── thumbnails/{asset_id_hash}/NNNNNN.jpg
    └── proxies/{asset_id_hash}_proxy.tkvp | .wav
└── logs/pipeline.log
```

`{asset_id_hash}` mixes path hash, mtime, and size, so a modified source
file addresses a fresh cache namespace automatically.

## 6. Dependency & licensing rules

Allowed: workspace crates, the sibling ecosystem git dependencies
(`tpt-kinetix`, `tpt-cadence`, both MIT/Apache-2.0), `redb`
(MIT/Apache-2.0), `notify` (MIT/Apache-2.0), `image` (MIT/Apache-2.0),
`log` (MIT/Apache-2.0). Git sources are allow-listed in `deny.toml`.

Banned: `sqlite3-sys` and other C-based stores, `ffmpeg-sys`/`ffmpeg-next`,
and any crate pulling GPL/LGPL/AGPL/MPL into the tree. Enforced by
`deny.toml` + CI.

## 7. Testing strategy

- Unit tests per module (`AssetId` stability/invalidation, priority
  ordering, WAV round-trips, db CRUD + concurrency, debouncer coalescing).
- Allocation-counting global allocator proves the RT-safe read path.
- Integration tests generate synthetic media with `tpt-av-asset-test-media`
  (WAV, proxy-stream video, and ffmpeg-gated H.264-in-MP4 clips) and exercise
  generate → cache → read-back end to end.
- Pipeline tests cover mid-execution cancellation (no corrupt state) and
  crash/resume (partial progress persisted, completion without redoing).
- CI matrix: ubuntu-latest (inotify backend), macos-latest (FSEvents
  backend), windows-latest (read_dir backend).
