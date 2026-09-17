# Changelog

All notable changes to `tpt-av-asset-test-media` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-16

Initial release.

### Added

- `write_test_wav`: synthetic PCM WAV fixture with a varying-envelope sine wave.
- `write_proxy_video` / `gradient_painter`: TPT proxy-stream (`.tkvp`) video fixtures.
- `ffmpeg_available`, `generate_h264_testsrc_annexb`, `mux_annexb_to_mp4`, `generate_h264_testsrc_mp4`: real H.264-in-MP4 fixtures, gated on `ffmpeg` being present.

[Unreleased]: https://github.com/tpt-solutions/tpt-av-asset/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-av-asset/releases/tag/v0.1.0
