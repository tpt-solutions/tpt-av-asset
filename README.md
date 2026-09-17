# tpt-av-asset

**A pure-Rust, background media asset management engine. Proxy generation,
waveform caching, thumbnail indexing, and filesystem monitoring. The
performance backbone of the TPT AV Stack.**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-pipeline.svg)](https://crates.io/crates/tpt-av-asset-pipeline)
[![CI](https://github.com/tpt-solutions/tpt-av-asset/actions/workflows/ci.yml/badge.svg)](https://github.com/tpt-solutions/tpt-av-asset/actions)
[![Status](https://img.shields.io/badge/status-early--stage%20%2F%20pre--1.0-orange)](#status)

`tpt-av-asset` is the **media asset management layer** of the TPT AV Stack.
It provides the background infrastructure that makes professional media
applications feel instantaneous — even when working with massive 4K/8K video
files or multi-hour podcast recordings:

- **Background proxy generation** — lightweight versions of heavy media files
  for smooth playback.
- **Waveform peak caching** — pre-computed min/max/RMS peaks for instant
  waveform rendering.
- **Video thumbnail caching** — pre-computed thumbnails for timeline
  scrubbing.
- **Media database** — fast, concurrent indexing of all imported assets
  (`redb`, pure Rust, no C bindings).
- **Filesystem monitoring** — automatic detection of file changes, moves, and
  deletions with automatic cache invalidation.
- **Background processing pipelines** — multi-threaded, prioritized,
  cancellable and resumable job queues.

## Design highlights

1. **Background-first** — all heavy processing happens on background worker
   threads; the UI thread never blocks.
2. **Incremental & resumable** — jobs persist partial progress; a cancelled
   or crashed job resumes where it left off.
3. **Cache-invalidation aware** — when a source file changes, its caches are
   automatically invalidated and regenerated.
4. **Pure Rust** — `redb`, `notify`, and `image`; no SQLite, no ffmpeg, no
   C/C++ bindings.
5. **Real-time safe reads** — reading waveform peaks is allocation-free and
   lock-free, safe for the audio/render thread (guarded by an
   allocation-counting test).
6. **Permissive licensing only** — MIT OR Apache-2.0, enforced via
   `cargo-deny`.

## Workspace crates

| Crate | Role |
| :--- | :--- |
| [`tpt-av-asset-utils`](tpt-av-asset-utils) | Shared types: `AssetId`, `MediaInfo`, `Priority`, progress, errors. |
| [`tpt-av-asset-db`](tpt-av-asset-db) | Embedded media database (assets, cache entries, jobs). |
| [`tpt-av-asset-cache`](tpt-av-asset-cache) | Waveform peak + thumbnail caches, on-disk storage, RT-safe reader. |
| [`tpt-av-asset-proxy`](tpt-av-asset-proxy) | Proxy generation engine (video downscale/re-encode, audio convert). |
| [`tpt-av-asset-watcher`](tpt-av-asset-watcher) | Cross-platform filesystem monitoring with debouncing. |
| [`tpt-av-asset-pipeline`](tpt-av-asset-pipeline) | Prioritized background job pipeline and high-level `AssetImporter`. |

Each sub-crate is independently useful: use just the cache, just the proxy
generator, or just the database.

## Ecosystem

| Crate | Role |
| :--- | :--- |
| [`tpt-kinetix`](https://github.com/tpt-solutions/tpt-kinetix) | Media containers, video codecs (source of video frames). |
| [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence) | Audio codecs (source of PCM data). |
| `tpt-av-asset` | **Asset management (this repo).** |

> **Early-stage note:** `tpt-av-asset` builds against the real
> [`tpt-kinetix`](https://github.com/tpt-solutions/tpt-kinetix) and
> [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence) git
> dependencies. Both ecosystems are decode-only today, so video proxies are
> rendered with `tpt-kinetix-lossless` into the TPT proxy stream (`.tkvp`)
> and audio proxies are stored as PCM WAV (`.wav`) until the kinetix H.264
> encoder and cadence FLAC encoder land — see `todo.md` "Deviations".

## Quick start

See [examples/](examples) for runnable demos:

```sh
cargo run -p tpt-av-asset-examples --bin import_media      # import + generate all caches
cargo run -p tpt-av-asset-examples --bin waveform_viewer   # ASCII-render cached peaks
cargo run -p tpt-av-asset-examples --bin thumbnail_browser # list cached thumbnails
cargo run -p tpt-av-asset-examples --bin proxy_generator   # proxies for a folder
```

## On-disk layout

```text
~/.tpt-av-asset/
├── db/assets.redb                  # embedded database
└── cache/
    ├── waveforms/{asset_hash}.peaks
    ├── thumbnails/{asset_hash}/NNNNNN.jpg
    └── proxies/{asset_hash}_proxy.tkvp | .wav
```

## Contributing

Not currently accepting pull requests — see [CONTRIBUTING.md](CONTRIBUTING.md).
Bug reports and feature requests are welcome as GitHub issues.

## License

Dual-licensed under either of:

- [MIT license](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

Copyright © 2026 TPT Solutions.
