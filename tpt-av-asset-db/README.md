# tpt-av-asset-db

[![Crates.io](https://img.shields.io/crates/v/tpt-av-asset-db.svg)](https://crates.io/crates/tpt-av-asset-db)
[![docs.rs](https://img.shields.io/docsrs/tpt-av-asset-db)](https://docs.rs/tpt-av-asset-db)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Embedded media asset database for the
[TPT AV asset engine](https://github.com/tpt-solutions/tpt-av-asset), backed
by [`redb`](https://crates.io/crates/redb) — pure Rust, no SQLite, no C
bindings.

`AssetDb` wraps a single `redb::Database` file with three tables:

| Table | Key | Value |
| :--- | :--- | :--- |
| `assets` | 24-byte `AssetId` triple | encoded `MediaInfo` |
| `cache_entries` | asset key + `CacheType` tag | cache file path |
| `jobs` | `u64` job id | encoded `JobRecord` |

Rows use a compact little-endian binary encoding (no serde, no bincode).
Reads take short read transactions and writes take short write
transactions, so an `AssetDb` (cheap to clone — it's `Arc`-backed) is safe
to share across threads.

The `jobs` table exists so [`tpt-av-asset-pipeline`](https://crates.io/crates/tpt-av-asset-pipeline)
can persist job state and re-enqueue unfinished work after a crash.

## Example

```rust,no_run
use std::path::Path;
use tpt_av_asset_db::AssetDb;

let db = AssetDb::open(Path::new("~/.tpt-av-asset/db/assets.redb"))?;
let assets = db.list_assets()?;
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
