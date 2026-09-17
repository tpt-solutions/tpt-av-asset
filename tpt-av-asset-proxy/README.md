# tpt-av-asset-proxy

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-proxy.svg)](https://crates.io/crates/tpt-av-asset-proxy)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-proxy)](https://docs.rs/tpt-av-asset-proxy)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Background proxy generation for video and audio assets, part of the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset).

`ProxyGenerator` renders lightweight proxies of heavy media:

- **Video** is decoded through the `tpt-kinetix` stack, aspect-fit into the
  target resolution, and re-encoded with `tpt-kinetix-lossless` into the TPT
  proxy stream (`.tkvp`).
- **Audio** is decoded through `tpt-cadence` and re-written as PCM WAV
  (`.wav`), until the `tpt-cadence` `Encoder` trait ships FLAC.

`ProxyProfile` provides three presets: `proxy_1080p_low`,
`proxy_720p_medium`, and `audio_proxy_flac`.

Long runs report progress through
[`ProgressReporter`](https://docs.rs/tpt-av-asset-utils/latest/tpt_av_asset_utils/struct.ProgressReporter.html)
and abort with `AssetError::Cancelled` when cancelled; a cancelled or failed
run always removes its partial output file, so `output_path` only ever
holds a complete proxy.

> **Early-stage note:** both ecosystem dependencies are decode-only today —
> see the [workspace README](https://github.com/tpt-solutions/tpt-av-asset#readme)
> for the interim proxy/audio strategy this drives.

## Example

```rust,no_run
use std::path::Path;
use tpt_av_asset_proxy::{ProxyGenerator, ProxyProfile};
use tpt_av_asset_utils::ProgressReporter;

let generator = ProxyGenerator::new(ProxyProfile::proxy_1080p_low());
generator.generate_video_proxy(
    Path::new("source.mp4"),
    Path::new("source_proxy.tkvp"),
    &ProgressReporter::new(),
)?;
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
