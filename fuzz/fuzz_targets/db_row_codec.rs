//! Fuzz target: the hand-rolled binary row codec in `tpt-av-asset-db` and
//! the `AssetId` key encoding in `tpt-av-asset-utils`.
//!
//! The decoders must return clean errors for any input — truncated rows,
//! unknown tags, trailing bytes — never panic and never loop.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_av_asset_db::{decode_job_record, decode_media_info};
use tpt_av_asset_utils::AssetId;

fuzz_target!(|data: &[u8]| {
    // MediaInfo rows.
    let _ = decode_media_info(data);

    // Job records (the job id comes from the table key in production).
    let _ = decode_job_record(1, data);

    // AssetId keys want exactly 24 bytes; try the slice and its prefixes.
    let _ = AssetId::from_key_bytes(data);
    if data.len() >= 24 {
        let _ = AssetId::from_key_bytes(&data[..24]);
    }
});
