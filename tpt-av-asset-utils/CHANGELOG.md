# Changelog

All notable changes to `tpt-av-asset-utils` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `AssetId`: content-addressed identifier from canonicalized path, mtime, and size.
- `MediaInfo`, `MediaType`, `VideoInfo`, `AudioInfo`: probed media metadata.
- `TimeRange`: half-open time range used by cache and proxy APIs.
- `Priority`: job priority levels (`Low`, `Normal`, `High`, `Critical`).
- `ProgressReporter` / `ProgressEvent`: progress callback with cooperative cancellation.
- `AssetError`: unified error type for the `tpt-av-asset` workspace.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
