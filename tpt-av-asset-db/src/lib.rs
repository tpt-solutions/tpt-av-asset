//! Embedded media asset database for the TPT AV stack.
//!
//! [`AssetDb`] wraps a [`redb::Database`] with three tables:
//!
//! | Table | Key | Value |
//! | :--- | :--- | :--- |
//! | `assets` | 24-byte [`AssetId`] triple | encoded [`MediaInfo`] |
//! | `cache_entries` | asset key + [`CacheType`] tag | cache file path |
//! | `jobs` | `u64` job id | encoded [`JobRecord`] |
//!
//! Rows use a compact little-endian binary encoding (see `schema.rs`); no
//! serde or bincode. Reads take short read transactions, writes take short
//! write transactions, so the database is safe to share across threads.
//!
//! The `jobs` table exists so the background pipeline can persist job state
//! and resume unfinished work after a crash (see `tpt-av-asset-pipeline`).

mod asset_table;
mod cache_table;
mod job_table;
mod query;
mod schema;
mod transaction;

use std::path::Path;
use std::sync::Arc;

pub use asset_table::{decode_media_info, encode_media_info};
pub use cache_table::CacheType;
pub use job_table::{JobRecord, JobState};
pub use schema::{asset_key, cache_key};

use tpt_av_asset_utils::AssetError;

/// The embedded media database.
///
/// Cheap to clone (`Arc`-backed); clones share the same underlying file.
#[derive(Clone)]
pub struct AssetDb {
    db: Arc<redb::Database>,
}

impl AssetDb {
    /// Opens or creates a database at the specified path, creating parent
    /// directories and initializing the schema.
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] for filesystem failures and
    /// [`AssetError::Db`] for storage failures.
    pub fn open(path: &Path) -> Result<Self, AssetError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let db = redb::Database::create(path).map_err(transaction::db_err)?;
        {
            let txn = db.begin_write().map_err(transaction::db_err)?;
            txn.open_table(schema::ASSETS).map_err(transaction::db_err)?;
            txn.open_table(schema::CACHE_ENTRIES)
                .map_err(transaction::db_err)?;
            txn.open_table(schema::JOBS).map_err(transaction::db_err)?;
            txn.commit().map_err(transaction::db_err)?;
        }
        Ok(Self {
            db: Arc::new(db),
        })
    }
}
