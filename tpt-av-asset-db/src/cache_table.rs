//! Cache entry tracking: the `cache_entries` table records which on-disk
//! cache artifacts exist for each asset, so invalidation and status queries
//! don't need to scan the cache directory.

use std::path::{Path, PathBuf};

use redb::ReadableTable;
use tpt_av_asset_utils::{AssetError, AssetId};

use crate::schema::{cache_key, Dec, Enc};
use crate::transaction;
use crate::AssetDb;

/// Types of cache entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheType {
    /// Waveform peaks (audio).
    WaveformPeaks,
    /// Video thumbnails.
    VideoThumbnails,
    /// Video proxy file.
    VideoProxy,
    /// Audio proxy file.
    AudioProxy,
}

impl CacheType {
    /// Stable wire tag (used in keys and rows).
    pub fn as_u8(self) -> u8 {
        match self {
            CacheType::WaveformPeaks => 0,
            CacheType::VideoThumbnails => 1,
            CacheType::VideoProxy => 2,
            CacheType::AudioProxy => 3,
        }
    }

    /// Inverse of [`CacheType::as_u8`]; returns `None` for unknown tags.
    pub fn from_u8(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(CacheType::WaveformPeaks),
            1 => Some(CacheType::VideoThumbnails),
            2 => Some(CacheType::VideoProxy),
            3 => Some(CacheType::AudioProxy),
            _ => None,
        }
    }
}

impl std::fmt::Display for CacheType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            CacheType::WaveformPeaks => "waveform_peaks",
            CacheType::VideoThumbnails => "video_thumbnails",
            CacheType::VideoProxy => "video_proxy",
            CacheType::AudioProxy => "audio_proxy",
        };
        f.write_str(name)
    }
}

impl AssetDb {
    /// Records that a cache entry exists for an asset.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn record_cache_entry(
        &self,
        asset_id: AssetId,
        cache_type: CacheType,
        cache_path: &Path,
    ) -> Result<(), AssetError> {
        let key = cache_key(&asset_id, cache_type.as_u8());
        let mut row = Enc::new();
        row.str(&cache_path.to_string_lossy());
        let row = row.finish();

        transaction::with_write_txn(&self.db, |txn| {
            let mut table = txn
                .open_table(crate::schema::CACHE_ENTRIES)
                .map_err(transaction::db_err)?;
            table
                .insert(key.as_slice(), row.as_slice())
                .map_err(transaction::db_err)?;
            Ok(())
        })
    }

    /// Checks if a cache entry exists for an asset.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn has_cache_entry(
        &self,
        asset_id: AssetId,
        cache_type: CacheType,
    ) -> Result<bool, AssetError> {
        let key = cache_key(&asset_id, cache_type.as_u8());
        let txn = self.db.begin_read().map_err(transaction::db_err)?;
        let table = txn
            .open_table(crate::schema::CACHE_ENTRIES)
            .map_err(transaction::db_err)?;
        Ok(table
            .get(key.as_slice())
            .map_err(transaction::db_err)?
            .is_some())
    }

    /// Returns the recorded cache path for an entry, if present.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn get_cache_entry(
        &self,
        asset_id: AssetId,
        cache_type: CacheType,
    ) -> Result<Option<PathBuf>, AssetError> {
        let key = cache_key(&asset_id, cache_type.as_u8());
        let txn = self.db.begin_read().map_err(transaction::db_err)?;
        let table = txn
            .open_table(crate::schema::CACHE_ENTRIES)
            .map_err(transaction::db_err)?;
        match table.get(key.as_slice()).map_err(transaction::db_err)? {
            Some(row) => {
                let mut d = Dec::new(row.value());
                let path = d.str()?;
                Ok(Some(PathBuf::from(path)))
            }
            None => Ok(None),
        }
    }

    /// Lists all cache entries recorded for an asset, as
    /// `(cache type, path)` pairs.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn list_cache_entries(
        &self,
        asset_id: AssetId,
    ) -> Result<Vec<(CacheType, PathBuf)>, AssetError> {
        let prefix = asset_id.key_bytes();
        let txn = self.db.begin_read().map_err(transaction::db_err)?;
        let table = txn
            .open_table(crate::schema::CACHE_ENTRIES)
            .map_err(transaction::db_err)?;
        let mut entries = Vec::new();
        for row in table
            .range(prefix.as_slice()..)
            .map_err(transaction::db_err)?
        {
            let (key, value) = row.map_err(transaction::db_err)?;
            let key_bytes: &[u8] = key.value();
            if key_bytes.len() != 25 || key_bytes[..24] != prefix[..] {
                break; // left this asset's key range
            }
            let cache_type = CacheType::from_u8(key_bytes[24])
                .ok_or_else(|| AssetError::db("corrupt row: bad cache type tag"))?;
            let mut d = Dec::new(value.value());
            entries.push((cache_type, PathBuf::from(d.str()?)));
        }
        Ok(entries)
    }

    /// Invalidates (deletes) all cache entries for an asset. Returns the
    /// number of entries removed; the caller is responsible for deleting the
    /// underlying files (see `tpt-av-asset-cache::invalidation`).
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn invalidate_caches(&self, asset_id: AssetId) -> Result<usize, AssetError> {
        let prefix = asset_id.key_bytes();
        let txn = self.db.begin_write().map_err(transaction::db_err)?;
        let removed = {
            let mut table = txn
                .open_table(crate::schema::CACHE_ENTRIES)
                .map_err(transaction::db_err)?;
            let keys: Vec<[u8; 25]> = table
                .range(prefix.as_slice()..)
                .map_err(transaction::db_err)?
                .filter_map(|row| {
                    let (key, _) = row.ok()?;
                    let bytes: &[u8] = key.value();
                    (bytes.len() == 25 && bytes[..24] == prefix[..])
                        .then(|| bytes.try_into().expect("25 bytes"))
                })
                .collect();
            let mut removed = 0usize;
            for key in keys {
                if table
                    .remove(key.as_slice())
                    .map_err(transaction::db_err)?
                    .is_some()
                {
                    removed += 1;
                }
            }
            removed
        };
        txn.commit().map_err(transaction::db_err)?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_type_tags_roundtrip() {
        for tag in 0..4u8 {
            assert_eq!(CacheType::from_u8(tag).unwrap().as_u8(), tag);
        }
        assert!(CacheType::from_u8(4).is_none());
        assert_eq!(CacheType::VideoProxy.to_string(), "video_proxy");
    }

    #[test]
    fn cache_type_ordering_is_declaration_order() {
        let types = [
            CacheType::AudioProxy,
            CacheType::WaveformPeaks,
            CacheType::VideoProxy,
        ];
        assert_eq!(
            types.iter().map(|t| t.as_u8()).collect::<Vec<_>>(),
            vec![3, 0, 2]
        );
    }
}
