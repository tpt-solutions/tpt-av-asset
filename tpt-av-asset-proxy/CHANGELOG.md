# Changelog

All notable changes to `tpt-av-asset-proxy` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `ProxyGenerator`: video and audio proxy generation with progress reporting and cancellation.
- `ProxyProfile` presets: `proxy_1080p_low`, `proxy_720p_medium`, `audio_proxy_flac`.
- Video proxies rendered via `tpt-kinetix-lossless` into the TPT proxy stream (`.tkvp`).
- Audio proxies written as PCM WAV via the crate's own `WavWriter`, pending a `tpt-cadence` FLAC encoder.
- Partial-output cleanup on cancellation or failure.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
