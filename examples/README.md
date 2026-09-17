# tpt-av-asset-examples

Demo binaries for the [`tpt-av-asset`](https://github.com/tpt-solutions/tpt-av-asset)
engine. Not published (`publish = false`) — this crate exists to exercise
the workspace end-to-end, not to be depended on.

All four demos share an engine home directory (`./.tpt-av-asset`, override
with `TPT_AV_ASSET_HOME`) holding the database and on-disk caches:

```sh
cargo run -p tpt-av-asset-examples --bin import_media      # import + generate all caches
cargo run -p tpt-av-asset-examples --bin waveform_viewer   # ASCII-render cached peaks
cargo run -p tpt-av-asset-examples --bin thumbnail_browser # list cached thumbnails
cargo run -p tpt-av-asset-examples --bin proxy_generator   # proxies for a folder
```

Run any binary without arguments to have it operate on a synthetic demo
asset generated under `demo/` (via [`tpt-av-asset-test-media`](../test-support/tpt-av-asset-test-media)).

See the [workspace README](https://github.com/tpt-solutions/tpt-av-asset#readme)
for how the underlying crates fit together.
