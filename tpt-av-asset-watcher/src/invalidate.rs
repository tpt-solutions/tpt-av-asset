//! Automatic cache invalidation: watcher events → `AssetDb::invalidate_caches`
//! + on-disk cache removal.
//!
//! When a watched file is modified, moved, or deleted, any asset registered
//! at that path has stale caches (its [`AssetId`] embeds mtime and size).
//! [`CacheInvalidator::handle_event`] finds the asset via the database path
//! index and drops its caches, so the next import regenerates everything.

use tpt_av_asset_cache::{invalidate_asset, CacheStorage};
use tpt_av_asset_db::AssetDb;
use tpt_av_asset_utils::AssetError;

use crate::event::{FileEvent, FileEventType};

/// Bridges watcher events into cache invalidation.
#[derive(Debug, Clone)]
pub struct CacheInvalidator {
    db: AssetDb,
    storage: CacheStorage,
}

impl CacheInvalidator {
    /// Creates an invalidator bound to a database and cache root.
    pub fn new(db: AssetDb, storage: CacheStorage) -> Self {
        Self { db, storage }
    }

    /// The database this invalidator works against.
    pub fn db(&self) -> &AssetDb {
        &self.db
    }

    /// The cache root this invalidator prunes.
    pub fn storage(&self) -> &CacheStorage {
        &self.storage
    }

    /// Handles one file event: for `Modified`/`Created`/`Deleted`/`Renamed`,
    /// invalidates every cache belonging to the asset registered at the
    /// event's path (the old path, for renames). Unknown paths are ignored
    /// so unrelated files cost nothing. Returns whether an asset was found
    /// and invalidated.
    ///
    /// # Errors
    /// Returns [`AssetError`] on database or filesystem failure.
    pub fn handle_event(&self, event: &FileEvent) -> Result<bool, AssetError> {
        let path = match &event.event_type {
            FileEventType::Renamed { old_path } => old_path.clone(),
            _ => event.path.clone(),
        };
        self.invalidate_at(&path)
    }

    /// Handles a batch of events, returning how many assets were
    /// invalidated.
    ///
    /// # Errors
    /// Returns [`AssetError`] on database or filesystem failure.
    pub fn handle_all(&self, events: &[FileEvent]) -> Result<usize, AssetError> {
        let mut invalidated = 0;
        for event in events {
            if self.handle_event(event)? {
                invalidated += 1;
            }
        }
        Ok(invalidated)
    }

    /// Invalidates caches for every asset registered at `path` — including
    /// stale historical versions whose rows still linger. Returns whether
    /// anything was invalidated.
    fn invalidate_at(&self, path: &std::path::Path) -> Result<bool, AssetError> {
        let assets = self.db.get_assets_by_path(path)?;
        for info in &assets {
            invalidate_asset(&self.db, &self.storage, info.id)?;
            log::debug!(
                "invalidation: dropped caches for asset {:016x} (path {})",
                info.id.hash(),
                path.display()
            );
        }
        Ok(!assets.is_empty())
    }
}
