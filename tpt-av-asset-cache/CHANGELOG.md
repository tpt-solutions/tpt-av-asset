# Changelog

All notable changes to `tpt-av-asset-cache` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `WaveformCache` / `WaveformGenerator`: min/max/RMS peak caching (`.peaks` files).
- `WaveformReader`: real-time-safe, allocation-free, lock-free positioned reads, guarded by an allocation-counting test.
- `ThumbnailCache` / `ThumbnailGenerator`: per-interval JPEG thumbnail caching for timeline scrubbing.
- `CacheStorage`: on-disk cache layout addressed by asset content hash.
- `invalidate_asset`: cache invalidation wired into `tpt-av-asset-db`.
- `video` / `audio` modules: decode-side integration with the `tpt-kinetix` and `tpt-cadence` codec stacks.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
