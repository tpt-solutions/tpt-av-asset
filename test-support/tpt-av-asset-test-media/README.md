# tpt-av-asset-test-media

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-test-media.svg)](https://crates.io/crates/tpt-av-asset-test-media)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-test-media)](https://docs.rs/tpt-av-asset-test-media)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Synthetic test-media helpers for the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset).

The real `tpt-cadence` / `tpt-kinetix` crates ship decoders (kinetix also
has a lossless encoder), but there is no way to *author* source media with
them yet — so tests and demos need helpers that write real files those
decoders accept:

- **`write_test_wav`** — a PCM WAV the cadence WAV decoder reads (a 440 Hz
  sine with a slow amplitude envelope, so waveform chunks have meaningful,
  varying peaks).
- **`write_proxy_video`** — a TPT proxy-stream video (RGBA frames encoded
  with `tpt-kinetix-lossless`) that `tpt-av-asset-cache::video` opens.
- **`mux_annexb_to_mp4`** / **`generate_h264_testsrc_mp4`** — real
  H.264-in-MP4 fixtures for the kinetix demux+decode path. These require
  `ffmpeg` on `PATH` (checked with `ffmpeg_available`); every consumer must
  handle `None` and skip gracefully when it's unavailable, the same
  convention the kinetix and cadence test suites follow.

This crate is a workspace dev-dependency, not something applications embed
— it exists so `tpt-av-asset`'s own test suite and the `examples/` demos
have deterministic, dependency-free source media to work with.

## Example

```rust,no_run
use std::path::Path;
use tpt_av_asset_test_media::{gradient_painter, write_proxy_video, write_test_wav};

write_test_wav(Path::new("tone.wav"), 2.0, 48_000, 2)?;
write_proxy_video(Path::new("clip.tkvp"), 320, 180, 30.0, 60, gradient_painter)?;
# Ok::<(), String>(())
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
