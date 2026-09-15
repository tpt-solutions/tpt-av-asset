//! Integration tests: generate proxies from synthetic sources and verify
//! the outputs match the target profile.

use std::path::{Path, PathBuf};

use tpt_av_asset_proxy::{ProxyGenerator, ProxyProfile};
use tpt_av_asset_utils::{AssetError, ProgressReporter};
use tpt_kinetix::{gradient_painter, write_test_video};

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
    let source = dir.join("source.tkv");
    // 1920x1080 at 10 fps, 1 s → 10 frames. (Proxies never upscale, so the
    // source must be larger than the target to exercise downscaling.)
    write_test_video(&source, 1920, 1080, 10.0, 1.0, gradient_painter).unwrap();

    let generator = ProxyGenerator::new(ProxyProfile::proxy_720p_medium());
    let output = dir.join("source_proxy.mp4");
    generator
        .generate_video_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    assert!(output.exists());
    let mut proxy = tpt_kinetix::open(&output).unwrap();
    let info = proxy.info().clone();
    // 16:9 downscaled into 1280x720.
    assert_eq!((info.width, info.height), (1280, 720));
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
fn frame_rate_override_decimates() {
    let dir = temp_dir("fps");
    let source = dir.join("source.tkv");
    write_test_video(&source, 64, 48, 20.0, 1.0, gradient_painter).unwrap();

    let mut profile = ProxyProfile::proxy_720p_medium();
    profile.frame_rate = Some(10.0); // half the frames
    let generator = ProxyGenerator::new(profile);
    let output = dir.join("half.tkv");
    generator
        .generate_video_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    let info = tpt_kinetix::open(&output).unwrap().info().clone();
    assert_eq!(info.frame_count, 10, "20 fps source decimated to 10 fps");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_proxy_roundtrips_pcm() {
    let dir = temp_dir("audio");
    let source = dir.join("tone.wav");
    tpt_cadence::write_test_wav(&source, 1.0, 16_000, 2).unwrap();

    let generator = ProxyGenerator::new(ProxyProfile::audio_proxy_flac());
    let output = dir.join("tone_proxy.flac");
    generator
        .generate_audio_proxy(&source, &output, &ProgressReporter::new())
        .unwrap();

    // The stand-in codec writes PCM WAV; duration/rate/channels must match.
    let mut decoder = tpt_cadence::open(&output).unwrap();
    let info = decoder.info().clone();
    assert_eq!(info.sample_rate, 16_000);
    assert_eq!(info.channels, 2);
    assert!((info.duration_secs - 1.0).abs() < 1e-6);

    let mut buf = vec![0f32; 4096];
    let mut total = 0usize;
    loop {
        let n = decoder.read_samples(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        total += n;
    }
    assert_eq!(total as u64, info.total_samples());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancellation_removes_partial_output() {
    let dir = temp_dir("cancel");
    let source = dir.join("source.tkv");
    write_test_video(&source, 64, 48, 10.0, 1.0, gradient_painter).unwrap();
    let output = dir.join("partial.mp4");

    // Pre-cancelled token: nothing may survive at the output path.
    let progress = ProgressReporter::new();
    progress.cancel();
    let generator = ProxyGenerator::new(ProxyProfile::proxy_1080p_low());
    let err = generator.generate_video_proxy(&source, &output, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    assert!(!output.exists(), "partial proxy output must be removed");

    // Audio side: same guarantee.
    let wav = dir.join("tone.wav");
    tpt_cadence::write_test_wav(&wav, 1.0, 8_000, 1).unwrap();
    let out = dir.join("tone.flac");
    let err = generator.generate_audio_proxy(&wav, &out, &progress);
    assert!(matches!(err, Err(AssetError::Cancelled)));
    assert!(!out.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_profile_is_rejected_upfront() {
    let dir = temp_dir("badprofile");
    let source = dir.join("source.tkv");
    write_test_video(&source, 64, 48, 10.0, 0.5, gradient_painter).unwrap();

    let mut profile = ProxyProfile::proxy_720p_medium();
    profile.video_bit_rate = 0;
    let generator = ProxyGenerator::new(profile);
    let output = dir.join("nope.mp4");
    let err = generator.generate_video_proxy(&source, &output, &ProgressReporter::new());
    assert!(matches!(err, Err(AssetError::Validation(_))));
    assert!(!Path::new(&output).exists());

    let _ = std::fs::remove_dir_all(&dir);
}
