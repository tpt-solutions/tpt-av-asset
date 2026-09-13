# Contributing to tpt-av-asset

Thank you for your interest in improving `tpt-av-asset` — the media asset
management layer of the TPT AV Stack.

## Licensing

**All contributions are dual-licensed under [MIT](LICENSE-MIT) OR
[Apache-2.0](LICENSE-APACHE).** By opening a pull request you agree that your
code may be distributed under either license, at the option of the recipient.

## Dependency rules (hard requirements)

To keep the project free, unencumbered, and pure Rust:

1. **No copyleft dependencies.** You must not introduce any dependency whose
   license is GPL, LGPL, AGPL, or MPL — in *any* crate of the workspace, and
   in *any* transitive dependency. This is enforced automatically by
   `cargo deny check` (see `deny.toml`) in CI.
2. **No C/C++ bindings.** No `-sys` crates wrapping native media libraries
   (no ffmpeg, no sqlite3, no gstreamer). Pure Rust only.
3. New dependencies need a short justification in the PR description.

## Code requirements

- **Real-time safe cache reads.** Methods documented as real-time safe
  (e.g. `WaveformCache::read_chunk`) must stay allocation-free and free of
  userspace locks. The allocation-counting test harness guards this — keep it
  passing.
- **Background jobs support cancellation and progress.** Any long-running
  generator must check `ProgressReporter::is_cancelled()` inside its loops
  and report progress as it goes.
- **No blocking on the hot path.** Heavy work belongs in the background
  pipeline, never in cache read paths.

## Development workflow

```sh
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo deny check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
```

CI runs all of the above on Linux, macOS, and Windows. A PR is mergeable only
when CI is green.

## Style

- Run `cargo fmt` before committing.
- Public API items need rustdoc comments, including `# Errors` sections where
  applicable.
- Keep sub-crates independently useful; avoid cross-crate circular needs.
  The dependency direction is:
  `utils ← db ← cache ← proxy ← watcher ← pipeline`.

## Reporting issues

Open a GitHub issue with a minimal reproduction (a synthetic WAV or TKV file
produced by the stub crates is ideal) and the output of `cargo version`.
