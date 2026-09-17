//! Fuzz target: the `.tkvp` proxy-stream container surface.
//!
//! Exercises `container::Header::parse`, the bounded `scan_frame_table`,
//! and lossless frame decoding (`container::decode_payload`) over arbitrary
//! bytes. Any panic, hang, or unbounded allocation is a bug — the
//! production parser must fail cleanly on hostile input (see
//! `tpt-av-asset-cache/tests/proxy_stream_hardening.rs` for the contract).

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use tpt_av_asset_cache::container::{decode_payload, scan_frame_table, Header};
use tpt_kinetix_lossless::LosslessDecoder;

/// Upper bound on pixels per frame the target will decode. The goal is to
/// explore parser paths, not to hand the OOM killer a blank check; the
/// lossless decoder itself rejects size-mismatched payloads before any
/// large allocation in production.
const MAX_PIXELS: u64 = 1 << 20;

fuzz_target!(|data: &[u8]| {
    if data.len() < 32 {
        return;
    }
    let (header_bytes, _body) = data.split_at(32);
    let Ok(header_bytes) = <[u8; 32]>::try_from(header_bytes) else {
        return;
    };
    let Ok(header) = Header::parse(&header_bytes) else {
        return;
    };

    // Feed the scan the full file length like `ProxyStreamSource::open` does;
    // it stops at the first table entry that would overrun the file.
    let file_len = data.len() as u64;
    let mut cursor = Cursor::new(data);
    let table = scan_frame_table(&mut cursor, file_len, header.frame_count);

    if u64::from(header.width) * u64::from(header.height) > MAX_PIXELS {
        return;
    }

    let sequence = header.sequence();
    let mut decoder = LosslessDecoder::new();
    for (offset, len) in table {
        let end = offset.saturating_add(len);
        if end > file_len {
            continue;
        }
        let payload = &data[offset as usize..end as usize];
        // Errors are fine; panics/hangs are not.
        let _ = decode_payload(&mut decoder, &sequence, payload, header.width, header.height);
    }
});
