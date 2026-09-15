//! Integration tests: generate proxies from synthetic sources and verify
//! the outputs match the target profile.

use std::path::{Path, PathBuf};

use tpt_av_asset_proxy::{ProxyGenerator, ProxyProfile};
use tpt_av_asset_test_media::{gradient_painter, write_proxy_video, write_test_wav};
use tpt_av_asset_utils::{AssetError, ProgressReporter};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-proxy-it-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn video_proxy_matches_target_resolution_and_length() {
    let dir = temp_dir("video");
    let source = dir.join("source.tkvp");
    // 1920x1080 at 10 fps, 1 s → 10 frames. (Proxies never upscale, so the
    // source must be larger than the target to exercise downscaling.)
    write_proxy_video(&source, 1920, 1080, 10.0, 10, gradient_painter).unwrap();

    let generator = ProxyGenerator::new(ProxyProfile::proxy_720p_medium());
    let output = dir.join("source_proxy.tkvp");
    generator
        .generate_video_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    assert!(output.exists());
    let mut proxy = tpt_av_asset_cache::video::open_video(&output).unwrap();
    let info = proxy.info().clone();
    // 16:9 downscaled into 1280x720.
    assert_eq!((info.width, info.height), (1280, 720));
    assert_eq!(info.codec, "kinetix-lossless");
    assert_eq!(
        info.frame_count, 10,
        "no frame_rate override keeps all frames"
    );
    assert!((info.duration_secs - 1.0).abs() < 1e-9);

    // Content survives the round trip (non-uniform pixels).
    let frame = proxy.next_frame().unwrap().unwrap();
    let first = *frame.data.first().unwrap();
    assert!(
        frame.data.iter().any(|p| *p != first),
        "proxy must not be a flat image"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn video_proxy_is_lossless_for_matching_dimensions() {
    let dir = temp_dir("lossless");
    let source = dir.join("source.tkvp");
    // Target == source resolution: the re-encode must be bit-exact per
    // pixel (tpt-kinetix-lossless is reversible).
    write_proxy_video(&source, 64, 48, 8.0, 4, gradient_painter).unwrap();

    let generator = ProxyGenerator::new(ProxyProfile::proxy_720p_medium());
    let output = dir.join("exact.tkvp");
    generator
        .generate_video_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    let mut original = tpt_av_asset_cache::video::open_video(&source).unwrap();
    let mut proxy = tpt_av_asset_cache::video::open_video(&output).unwrap();
    for _ in 0..4 {
        let a = original.next_frame().unwrap().unwrap();
        let b = proxy.next_frame().unwrap().unwrap();
        assert_eq!((a.width, a.height), (b.width, b.height));
        assert_eq!(
            a.data, b.data,
            "same-resolution lossless re-encode is bit-exact"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn frame_rate_override_decimates() {
    let dir = temp_dir("fps");
    let source = dir.join("source.tkvp");
    write_proxy_video(&source, 64, 48, 20.0, 20, gradient_painter).unwrap();

    let mut profile = ProxyProfile::proxy_720p_medium();
    profile.frame_rate = Some(10.0); // half the frames
    let generator = ProxyGenerator::new(profile);
    let output = dir.join("half.tkvp");
    generator
        .generate_video_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    let info = tpt_av_asset_cache::video::open_video(&output)
        .unwrap()
        .info()
        .clone();
    assert_eq!(info.frame_count, 10, "20 fps source decimated to 10 fps");
    assert!((info.frame_rate - 10.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_proxy_roundtrips_pcm() {
    let dir = temp_dir("audio");
    let source = dir.join("tone.wav");
    write_test_wav(&source, 1.0, 16_000, 2).unwrap();

    let generator = ProxyGenerator::new(ProxyProfile::audio_proxy_flac());
    let output = dir.join("tone_proxy.wav");
    generator
        .generate_audio_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    // Audio proxies are 16-bit PCM WAV until the cadence encoder lands.
    let mut decoder = tpt_av_asset_cache::audio::open_audio(&output).unwrap();
    let info = decoder.info().clone();
    assert_eq!(info.sample_rate, 16_000);
    assert_eq!(info.channels, 2);
    assert_eq!(info.codec, "wav");
    assert!((info.duration_secs.unwrap_or(0.0) - 1.0).abs() < 1e-6);

    let mut buf = vec![0f32; 4_096];
    let mut total_samples = 0usize;
    loop {
        let frames = decoder.decode(&mut buf).unwrap();
        if frames == 0 {
            break;
        }
        total_samples += frames;
    }
    assert_eq!(
        total_samples as u64,
        info.total_frames.unwrap_or(0),
        "proxy must carry every source frame"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancellation_removes_partial_output() {
    let dir = temp_dir("cancel");
    let source = dir.join("source.tkvp");
    write_proxy_video(&source, 64, 48, 10.0, 10, gradient_painter).unwrap();
    let output = dir.join("partial.tkvp");

    // Pre-cancelled token: nothing may survive at the output path.
    let progress = ProgressReporter::new();
    progress.cancel();
    let generator = ProxyGenerator::new(ProxyProfile::proxy_1080p_low());
    let err = generator.generate_video_proxy(&source, &output, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    assert!(!output.exists(), "partial proxy output must be removed");

    // Audio side: same guarantee.
    let wav = dir.join("tone.wav");
    write_test_wav(&wav, 1.0, 8_000, 1).unwrap();
    let out = dir.join("tone.wav");
    let err = generator.generate_audio_proxy(&wav, &out, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    assert!(!out.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_profile_is_rejected_upfront() {
    let dir = temp_dir("badprofile");
    let source = dir.join("source.tkvp");
    write_proxy_video(&source, 64, 48, 10.0, 5, gradient_painter).unwrap();

    let mut profile = ProxyProfile::proxy_720p_medium();
    profile.video_bit_rate = 0;
    let generator = ProxyGenerator::new(profile);
    let output = dir.join("nope.tkvp");
    let err = generator.generate_video_proxy(&source, &output, &ProgressReporter::new());
    assert!(matches!(err, Err(AssetError::Validation(_))));
    assert!(!Path::new(&output).exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn proxy_stream_seeking_returns_correct_frames() {
    let dir = temp_dir("seek");
    let source = dir.join("source.tkvp");
    write_proxy_video(&source, 32, 24, 10.0, 10, gradient_painter).unwrap();

    let mut source_video = tpt_av_asset_cache::video::open_video(&source).unwrap();
    source_video.seek_to(0.55);
    let frame = source_video.next_frame().unwrap().unwrap();
    assert!(
        (frame.time_secs - 0.6).abs() < 1e-9,
        "seek to 0.55s lands on the 0.6s frame, got {}",
        frame.time_secs
    );

    source_video.seek_to(5.0);
    assert!(
        source_video.next_frame().unwrap().is_none(),
        "seeking past the end yields no frames"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
