# tpt-av-asset-cache

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-cache.svg)](https://crates.io/crates/tpt-av-asset-cache)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-cache)](https://docs.rs/tpt-av-asset-cache)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Waveform and thumbnail caching for the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset).

Two on-disk caches built on `CacheStorage`, addressed by a hash of the
asset's path, modification time, and size — so a modified source file gets
a fresh cache namespace automatically:

```text
{root}/
├── waveforms/{asset_hash}.peaks
├── thumbnails/{asset_hash}/NNNNNN.jpg
└── proxies/{asset_hash}_proxy.tkvp | .wav
```

- **`WaveformCache`** — fixed-size min/max/RMS peak chunks (`.peaks` files).
  Reading is **real-time safe**: `WaveformReader::read_chunk` and
  `read_range_into` perform positioned reads into caller-provided buffers
  and return `Copy` values — allocation-free and lock-free, safe to call
  from an audio or render thread. An allocation-counting test in the crate's
  test suite guards this guarantee.
- **`ThumbnailCache`** — one JPEG per interval, for fast timeline scrubbing.

`WaveformGenerator` and `ThumbnailGenerator` populate the caches by decoding
through the `tpt-cadence` / `tpt-kinetix` decoder traits, with progress
reporting, cooperative cancellation, and resumable partial progress.
`invalidate_asset` ties cache invalidation into `tpt-av-asset-db`.

## Example

```rust,no_run
use std::path::Path;
use tpt_av_asset_cache::WaveformReader;

let reader = WaveformReader::open(Path::new("clip.peaks"))?;
if let Some(chunk) = reader.read_chunk(0)? {
    println!("{chunk:?}");
}
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
