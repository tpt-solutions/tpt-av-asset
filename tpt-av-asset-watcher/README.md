# tpt-av-asset-watcher

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-watcher.svg)](https://crates.io/crates/tpt-av-asset-watcher)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-watcher)](https://docs.rs/tpt-av-asset-watcher)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Cross-platform filesystem monitoring for media directories, part of the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset).

`MediaWatcher` delivers debounced `FileEvent`s (`Created`, `Modified`,
`Deleted`, `Renamed`) for everything under the watched directories,
regardless of platform backend:

- **Linux** — inotify, via the [`notify`](https://crates.io/crates/notify) crate
- **macOS** — FSEvents, via the [`notify`](https://crates.io/crates/notify) crate
- **Windows** (default) — periodic directory diff (`backend::read_dir`)

Events pass through a per-path quiet-window debouncer, so an edit storm
surfaces as a single `Modified`.

`CacheInvalidator` bridges these events into `tpt-av-asset-db` plus on-disk
cache removal, making cache invalidation automatic when a source file
changes, moves, or is deleted.

## Example

```rust,no_run
use tpt_av_asset_watcher::MediaWatcher;

let mut watcher = MediaWatcher::new()?;
watcher.watch("/media/library".as_ref())?;
loop {
    let event = watcher.recv()?;
    println!("{event:?}");
}
# #[allow(unreachable_code)]
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
