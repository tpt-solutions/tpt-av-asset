//! On-disk cache storage layout.
//!
//! ```text
//! {root}/
//! ├── waveforms/{asset_hash}.peaks
//! ├── thumbnails/{asset_hash}/NNNNNN.jpg
//! └── proxies/{asset_hash}_proxy.mp4 | .flac
//! ```
//!
//! `asset_hash` mixes path hash, mtime, and size (see [`AssetId::hash`]), so
//! a modified source file addresses a fresh namespace automatically — stale
//! files are simply never referenced again and can be pruned.

use std::path::{Path, PathBuf};

use tpt_av_asset_utils::AssetId;

/// Root of the on-disk cache tree.
#[derive(Debug, Clone)]
pub struct CacheStorage {
    root: PathBuf,
}

impl CacheStorage {
    /// Creates a storage handle rooted at `root` (not created on disk until
    /// something is written).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The cache root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Creates the `waveforms`, `thumbnails`, and `proxies` directories.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`](tpt_av_asset_utils::AssetError::Io) if the directories cannot be created.
    pub fn ensure_layout(&self) -> Result<(), tpt_av_asset_utils::AssetError> {
        for dir in [self.waveform_dir(), self.thumbnail_root(), self.proxy_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// The 16-hex-digit cache namespace name for an asset.
    pub fn asset_hash(asset_id: AssetId) -> String {
        format!("{:016x}", asset_id.hash())
    }

    fn waveform_dir(&self) -> PathBuf {
        self.root.join("waveforms")
    }

    fn thumbnail_root(&self) -> PathBuf {
        self.root.join("thumbnails")
    }

    fn proxy_dir(&self) -> PathBuf {
        self.root.join("proxies")
    }

    /// Path of the waveform peak file for an asset:
    /// `waveforms/{asset_hash}.peaks`.
    pub fn waveform_path(&self, asset_id: AssetId) -> PathBuf {
        self.waveform_dir()
            .join(format!("{}.peaks", Self::asset_hash(asset_id)))
    }

    /// Directory holding an asset's thumbnails:
    /// `thumbnails/{asset_hash}/`.
    pub fn thumbnail_dir(&self, asset_id: AssetId) -> PathBuf {
        self.thumbnail_root().join(Self::asset_hash(asset_id))
    }

    /// Path of a proxy output: `proxies/{asset_hash}_proxy.{mp4|flac}`.
    pub fn proxy_path(&self, asset_id: AssetId, extension: &str) -> PathBuf {
        self.proxy_dir()
            .join(format!("{}_proxy.{extension}", Self::asset_hash(asset_id)))
    }

    /// Deletes every cache file for an asset (waveform file, thumbnail
    /// directory, proxy files) whether or not they are registered in the
    /// database. Returns how many paths were removed.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`](tpt_av_asset_utils::AssetError::Io) if removal fails.
    pub fn remove_asset_files(
        &self,
        asset_id: AssetId,
    ) -> Result<usize, tpt_av_asset_utils::AssetError> {
        let mut removed = 0;
        let waveform = self.waveform_path(asset_id);
        if waveform.exists() {
            std::fs::remove_file(&waveform)?;
            removed += 1;
        }
        let thumbs = self.thumbnail_dir(asset_id);
        if thumbs.is_dir() {
            std::fs::remove_dir_all(&thumbs)?;
            removed += 1;
        }
        for proxy in [
            self.proxy_path(asset_id, "mp4"),
            self.proxy_path(asset_id, "flac"),
        ] {
            if proxy.exists() {
                std::fs::remove_file(&proxy)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths_embed_asset_hash() {
        let storage = CacheStorage::new("/tmp/cache");
        let a = AssetId::from_parts(1, 2, 3);
        let hash = CacheStorage::asset_hash(a);

        assert_eq!(
            storage.waveform_path(a),
            PathBuf::from(format!("/tmp/cache/waveforms/{hash}.peaks"))
        );
        assert_eq!(
            storage.thumbnail_dir(a),
            PathBuf::from(format!("/tmp/cache/thumbnails/{hash}"))
        );
        assert_eq!(
            storage.proxy_path(a, "mp4"),
            PathBuf::from(format!("/tmp/cache/proxies/{hash}_proxy.mp4"))
        );
    }

    #[test]
    fn different_asset_versions_hash_differently() {
        let old = AssetId::from_parts(1, 100, 10);
        let new = AssetId::from_parts(1, 200, 10); // same file, later mtime
        assert_ne!(CacheStorage::asset_hash(old), CacheStorage::asset_hash(new));
    }
}
