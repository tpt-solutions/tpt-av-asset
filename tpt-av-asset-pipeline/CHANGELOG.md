# Changelog

All notable changes to `tpt-av-asset-pipeline` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `ProcessingPipeline`: prioritized, multi-threaded, cancellable job execution.
- `JobQueue`: Critical-first, FIFO-within-priority dispatch.
- `Scheduler`: inter-job dependencies; dependents of a failed job are cancelled, not run.
- `ProgressTracker`: per-job state/fraction snapshots with blocking waits.
- Concrete jobs: `WaveformJob`, `ThumbnailJob`, `VideoProxyJob`, `AudioProxyJob`.
- Crash recovery: `ProcessingPipeline::recover_interrupted` re-enqueues unfinished work from `AssetDb`'s `jobs` table.
- `AssetImporter`: probe → index → schedule, returning immediately.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
