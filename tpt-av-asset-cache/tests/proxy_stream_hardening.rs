//! Regression tests: hostile `.tkvp` inputs must fail fast (or degrade to
//! the valid prefix) instead of attempting huge allocations.

use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tpt_av_asset_cache::{container, open_video};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-av-asset-cache-dos-{name}-{}-{}",
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

/// Builds a raw container file from a header plus arbitrary body bytes.
fn write_raw(path: &std::path::Path, header: &container::Header, body: &[u8]) {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&header.bytes()).unwrap();
    file.write_all(body).unwrap();
}

fn tiny_header() -> container::Header {
    container::Header {
        width: 64,
        height: 48,
        fps: 30.0,
        frame_count: 0,
    }
}

#[test]
fn absurd_declared_frame_count_does_not_allocate() {
    let dir = temp_dir("frame-count");
    let path = dir.join("hostile.tkvp");
    // Header claims u32::MAX frames; the file has 12 bytes after the header.
    let mut header = tiny_header();
    header.frame_count = u32::MAX;
    write_raw(&path, &header, &[0xAB; 12]);

    let started = Instant::now();
    let mut source = open_video(&path).expect("valid header must open");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "open must not attempt a huge allocation"
    );
    assert_eq!(source.info().frame_count, 0, "no real frames exist");
    assert!(source.next_frame().unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn absurd_frame_length_does_not_allocate() {
    let dir = temp_dir("frame-len");
    let path = dir.join("hostile.tkvp");
    // One frame whose length prefix claims 4 GiB; the file ends right there.
    let header = tiny_header();
    let mut body = Vec::new();
    body.extend_from_slice(&u32::MAX.to_le_bytes());
    body.extend_from_slice(&[0x55; 16]);
    write_raw(&path, &header, &body);

    let started = Instant::now();
    let mut source = open_video(&path).expect("valid header must open");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "open must not attempt a huge allocation"
    );
    assert_eq!(source.info().frame_count, 0);
    assert!(source.next_frame().unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn valid_prefix_survives_a_truncated_tail() {
    let dir = temp_dir("prefix");
    let path = dir.join("partial.tkvp");
    // First frame is small and real (garbage payload is fine — the offset
    // table only needs the declared length to fit); the second frame's
    // length prefix overruns the file.
    let header = tiny_header();
    let mut body = Vec::new();
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&[0x11; 16]);
    body.extend_from_slice(&u32::MAX.to_le_bytes()); // hostile tail
    write_raw(&path, &header, &body);

    let source = open_video(&path).expect("valid header must open");
    assert_eq!(
        source.info().frame_count,
        1,
        "only the in-bounds frame counts"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tiny_and_garbage_files_error_cleanly() {
    let dir = temp_dir("tiny");
    // Shorter than the sniff window: must error, not panic.
    let tiny = dir.join("tiny.bin");
    std::fs::write(&tiny, [0u8; 3]).unwrap();
    assert!(open_video(&tiny).is_err());

    // Shorter than the 32-byte container header.
    let short = dir.join("short.tkvp");
    let mut header = tiny_header();
    header.frame_count = 1;
    let mut raw = header.bytes().to_vec();
    raw.truncate(20);
    std::fs::write(&short, raw).unwrap();
    assert!(open_video(&short).is_err());

    // Random garbage: unsupported format.
    let garbage = dir.join("garbage.bin");
    std::fs::write(&garbage, [7u8; 64]).unwrap();
    assert!(open_video(&garbage).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}
