# Changelog

All notable changes to `tpt-av-asset-watcher` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `MediaWatcher`: debounced `Created` / `Modified` / `Deleted` / `Renamed` file events.
- Platform backends: inotify and FSEvents via `notify`, plus a polling `read_dir` backend for Windows.
- `EventDebouncer`: per-path quiet-window coalescing.
- `CacheInvalidator`: bridges filesystem events into `tpt-av-asset-db` and on-disk cache removal.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
