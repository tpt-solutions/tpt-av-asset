# tpt-av-asset-pipeline

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-pipeline.svg)](https://crates.io/crates/tpt-av-asset-pipeline)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-pipeline)](https://docs.rs/tpt-av-asset-pipeline)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Prioritized, cancellable, resumable background asset processing for the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset).

`ProcessingPipeline` runs jobs on a worker thread pool:

- **`job::Job`** — the unit of work; `execute` polls the progress
  reporter's cancellation token between work units.
- **`queue::JobQueue`** — Critical-first, FIFO-within-priority dispatch.
- **`scheduler::Scheduler`** — inter-job dependencies; jobs whose
  dependencies fail are cancelled, never run.
- **`progress::ProgressTracker`** — per-job state/fraction snapshots with
  blocking waits.
- **`jobs`** — the concrete jobs (waveform, thumbnails, video/audio
  proxies), plus crash-recovery rebuilding from `AssetDb` records.
- **`importer::AssetImporter`** — probe → index → schedule → return
  immediately.

Job records persist into `tpt-av-asset-db`'s `jobs` table on every state
transition, so `ProcessingPipeline::recover_interrupted` can re-enqueue
unfinished work after a crash; the concrete generators resume from
whatever the caches already hold.

## Example

```rust,no_run
use tpt_av_asset_cache::CacheStorage;
use tpt_av_asset_db::AssetDb;
use tpt_av_asset_pipeline::{AssetImporter, ProcessingPipeline};

let pipeline = ProcessingPipeline::new(4)?;
let db = AssetDb::open("assets.redb".as_ref())?;
let storage = CacheStorage::new("cache");
let importer = AssetImporter::new(pipeline, db, storage);

let asset_id = importer.import("clip.mp4".as_ref())?;
# Ok::<(), tpt_av_asset_utils::AssetError>(())
```

## Part of the TPT AV asset workspace

This crate is one of several that make up
[`tpt-av-asset`](https://github.com/tpt-solutions/tpt-av-asset), a
background media asset management engine (proxy generation, waveform
caching, thumbnail indexing, filesystem monitoring). See the
[workspace README](https://github.com/tpt-solutions/tpt-av-asset#readme)
for how the crates fit together.

## License

Dual-licensed under either of

- [MIT license](https://github.com/tpt-solutions/tpt-av-asset/blob/master/LICENSE-MIT)
- [Apache License, Version 2.0](https://github.com/tpt-solutions/tpt-av-asset/blob/master/LICENSE-APACHE)

at your option.
