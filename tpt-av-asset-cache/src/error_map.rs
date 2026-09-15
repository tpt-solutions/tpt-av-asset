//! Error-conversion helpers shared by the cadence/kinetix glue layers.

use tpt_av_asset_utils::AssetError;

/// Maps any foreign error into [`AssetError::Codec`].
pub(crate) fn codec(e: impl std::fmt::Display) -> AssetError {
    AssetError::codec(e.to_string())
}
