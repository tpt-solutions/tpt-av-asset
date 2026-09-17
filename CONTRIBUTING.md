# Contributing to tpt-av-asset

`tpt-av-asset` — the media asset management layer of the TPT AV Stack — is not
currently accepting pull requests. Please don't open one; it will be closed
unreviewed.

## Fuzzing

The parsing surfaces that ingest untrusted media have `cargo-fuzz` targets
under `fuzz/`:

| Target | Surface |
| :--- | :--- |
| `proxy_stream_container` | `.tkvp` header, frame-table scan, lossless frame decode |
| `db_row_codec` | `tpt-av-asset-db` row decoders, `AssetId` keys |

Fuzzing needs nightly Rust and `cargo-fuzz`:

```sh
cargo install cargo-fuzz --locked
cargo fuzz run proxy_stream_container -- -max_total_time=60
cargo fuzz run db_row_codec -- -max_total_time=60
```

CI runs a short (non-blocking) smoke fuzz of both targets on Linux for every
push. If you change a parser, run the relevant target locally for a few
minutes before opening the PR; crashes go to `fuzz/artifacts/` and a minimal
repro can be replayed with `cargo fuzz run <target> <artifact-file>`. Any
panic, hang, or oversized allocation on hostile input is a bug — see
[SECURITY.md](SECURITY.md) for the hardening rules new parsers must follow.

## Security

See [SECURITY.md](SECURITY.md) for the supported versions, what is in scope,
and how to report vulnerabilities (email, not GitHub issues).

## Reporting issues

Bug reports and feature requests are welcome. Open a GitHub issue with:

- A minimal reproduction (a synthetic WAV or proxy-stream video produced by
  the `tpt-av-asset-test-media` crate is ideal).
- The output of `cargo version`.
