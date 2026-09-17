# tpt-av-asset-utils

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-utils.svg)](https://crates.io/crates/tpt-av-asset-utils)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-utils)](https://docs.rs/tpt-av-asset-utils)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Shared types, identifiers, progress reporting, and error handling for the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset).

This crate has no dependencies beyond `std`, so every other crate in the
`tpt-av-asset` workspace — and any downstream application — can build on it
without pulling in `redb`, `image`, or the codec stacks.

## What's in it

- **[`AssetId`]** — a content-addressed identifier computed from a file's
  canonicalized path, modification time, and size. Editing a file yields a
  new ID, so caches keyed on the old one are naturally treated as stale.
- **[`MediaInfo`], [`MediaType`], [`VideoInfo`], [`AudioInfo`]** — plain
  metadata describing a probed media file (dimensions, frame rate, sample
  rate, channels, duration).
- **[`TimeRange`]** — a half-open `[start, end)` time range used across the
  cache and proxy APIs.
- **[`Priority`]** — job priority levels (`Low < Normal < High < Critical`)
  used by the background pipeline's scheduler.
- **[`ProgressReporter`] / [`ProgressEvent`]** — a progress callback plus a
  cooperative cancellation token, shared by every long-running generator
  (waveform, thumbnail, proxy) so callers get uniform progress/cancel
  semantics regardless of which subsystem is running.
- **[`AssetError`]** — the single error type used across the whole engine,
  covering I/O, database, decode, encode, and cancellation failures.

[`AssetId`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/struct.AssetId.html
[`MediaInfo`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/enum.MediaInfo.html
[`MediaType`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/enum.MediaType.html
[`VideoInfo`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/struct.VideoInfo.html
[`AudioInfo`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/struct.AudioInfo.html
[`TimeRange`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/struct.TimeRange.html
[`Priority`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/enum.Priority.html
[`ProgressReporter`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/struct.ProgressReporter.html
[`ProgressEvent`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/enum.ProgressEvent.html
[`AssetError`]: https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/enum.AssetError.html

## Example

```rust
use std::path::Path;
use tpt_av_asset_utils::AssetId;

let id = AssetId::from_path(Path::new("clip.mp4"))?;
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
