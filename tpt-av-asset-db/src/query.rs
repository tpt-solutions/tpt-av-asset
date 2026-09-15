//! Read-side query API: listing assets, path lookups, and bulk reads that
//! span more than one table.

use std::path::Path;

use redb::ReadableTable;
use tpt_av_asset_utils::{AssetError, AssetId, MediaInfo};

use crate::asset_table::decode_media_info;
use crate::transaction;
use crate::AssetDb;

impl AssetDb {
    /// Lists all assets in the database.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn list_assets(&self) -> Result<Vec<MediaInfo>, AssetError> {
        let txn = self.db.begin_read().map_err(transaction::db_err)?;
        let table = txn
            .open_table(crate::schema::ASSETS)
            .map_err(transaction::db_err)?;
        let mut assets = Vec::new();
        for row in table.iter().map_err(transaction::db_err)? {
            let (_, value) = row.map_err(transaction::db_err)?;
            assets.push(decode_media_info(value.value())?);
        }
        Ok(assets)
    }

    /// Finds an asset by file path. Matching compares canonical paths, so
    /// relative/absolute variants of the same file are equivalent.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn get_asset_by_path(&self, path: &Path) -> Result<Option<MediaInfo>, AssetError> {
        Ok(self.get_assets_by_path(path)?.into_iter().next())
    }

    /// Finds every asset registered at a file path — historical versions
    /// included, since a modified file produces a new [`AssetId`] while the
    /// old row (and its caches) may still linger. Cache invalidation uses
    /// this to sweep stale versions too.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn get_assets_by_path(&self, path: &Path) -> Result<Vec<MediaInfo>, AssetError> {
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        Ok(self
            .list_assets()?
            .into_iter()
            .filter(|info| {
                let stored =
                    std::fs::canonicalize(&info.path).unwrap_or_else(|_| info.path.clone());
                stored == target
            })
            .collect())
    }
}
