//! Proxy generation engine for the TPT AV asset stack.
//!
//! [`ProxyGenerator`] renders lightweight proxies of heavy media: video is
//! decoded through the kinetix stack, aspect-fit into the target
//! resolution, and re-encoded with `tpt-kinetix-lossless` into the TPT
//! proxy stream (`.tkvp`); audio is decoded through `tpt-cadence` and
//! re-written as PCM WAV (`.wav`) until the cadence `Encoder` trait ships
//! FLAC. Profiles ([`ProxyProfile`]) provide the presets `proxy_1080p_low`,
//! `proxy_720p_medium`, and `audio_proxy_flac`.
//!
//! Long runs report progress through
//! [`ProgressReporter`](tpt_av_asset_utils::ProgressReporter) and abort
//! with
//! [`AssetError::Cancelled`](tpt_av_asset_utils::AssetError::Cancelled)
//! when cancelled; partial output files are always removed, so an output
//! path only ever contains a complete proxy.

pub mod audio_proxy;
pub mod encoder;
pub mod generator;
pub mod profile;
pub mod video_proxy;
pub mod wav_writer;

pub use encoder::VideoEncoderSink;
pub use generator::ProxyGenerator;
pub use profile::ProxyProfile;
pub use wav_writer::{WavSpec, WavWriter};
