//! Cache invalidation logic, tied into `tpt-av-asset-db`.
//!
//! [`invalidate_asset`] removes both the database rows recording cache
//! entries and the underlying files. It is the single entry point used by
//! the file watcher (source changed/moved/deleted) and by applications that
//! want to force regeneration.

use tpt_av_asset_db::{AssetDb, CacheType};
use tpt_av_asset_utils::{AssetError, AssetId};

use crate::storage::CacheStorage;

/// Invalidates every cache entry for `asset_id`: deletes the on-disk
/// artifacts (waveform file, thumbnail directory, proxies) and clears the
/// database rows. Returns the number of database rows removed.
///
/// Unregistered artifacts at the standard storage locations are removed as
/// well, so the call is idempotent and self-healing.
///
/// # Errors
/// Returns [`AssetError`] on db or filesystem failure.
pub fn invalidate_asset(
    db: &AssetDb,
    storage: &CacheStorage,
    asset_id: AssetId,
) -> Result<usize, AssetError> {
    for (cache_type, path) in db.list_cache_entries(asset_id)? {
        let result = match cache_type {
            CacheType::VideoThumbnails => {
                if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    Ok(())
                }
            }
            _ => {
                if path.exists() {
                    std::fs::remove_file(&path)
                } else {
                    Ok(())
                }
            }
        };
        if let Err(e) = result {
            log::warn!("invalidation: could not remove {}: {e}", path.display());
        }
    }

    // Defensive sweep of standard locations (covers unregistered artifacts).
    storage.remove_asset_files(asset_id)?;

    let removed = db.invalidate_caches(asset_id)?;
    log::debug!(
        "invalidation: cleared {removed} cache entries for asset {:016x}",
        asset_id.hash()
    );
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-av-asset-cache-inval-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn invalidate_removes_files_and_rows() {
        let dir = temp_dir("files");
        let db = AssetDb::open(&dir.join("db.redb")).unwrap();
        let storage = CacheStorage::new(dir.join("cache"));
        storage.ensure_layout().unwrap();

        let id = AssetId::from_parts(11, 22, 33);
        let peaks = storage.waveform_path(id);
        std::fs::write(&peaks, b"fake").unwrap();
        let thumbs = storage.thumbnail_dir(id);
        std::fs::create_dir_all(&thumbs).unwrap();
        std::fs::write(thumbs.join("000000.jpg"), b"fake").unwrap();

        db.record_cache_entry(id, CacheType::WaveformPeaks, &peaks).unwrap();
        db.record_cache_entry(id, CacheType::VideoThumbnails, &thumbs).unwrap();
        assert_eq!(db.list_cache_entries(id).unwrap().len(), 2);

        let removed = invalidate_asset(&db, &storage, id).unwrap();
        assert_eq!(removed, 2);
        assert!(!peaks.exists());
        assert!(!thumbs.exists());
        assert!(db.list_cache_entries(id).unwrap().is_empty());

        // Idempotent.
        assert_eq!(invalidate_asset(&db, &storage, id).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
