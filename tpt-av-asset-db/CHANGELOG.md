# Changelog

All notable changes to `tpt-av-asset-db` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `AssetDb`: embedded `redb`-backed database with `assets`, `cache_entries`, and `jobs` tables.
- Compact binary row encoding (no serde/bincode) for asset metadata, cache paths, and job records.
- `list_assets`, `get_asset_by_path`, `get_assets_by_path` query helpers.
- `CacheType` cache-entry tagging and `JobRecord` / `JobState` job persistence for crash recovery.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
