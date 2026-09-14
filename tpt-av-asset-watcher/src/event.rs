//! File event types.

use std::path::PathBuf;
use std::time::SystemTime;

/// File event types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEventType {
    /// File was created.
    Created,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File was renamed/moved; carries the previous path.
    Renamed {
        /// Path the file was renamed/moved from.
        old_path: PathBuf,
    },
}

impl FileEventType {
    /// True for events that mean "the file at `path` changed identity or
    /// vanished" — the triggers for cache invalidation.
    pub fn is_invalidation_trigger(&self) -> bool {
        !matches!(self, FileEventType::Created)
    }
}

/// A file event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    /// Event type.
    pub event_type: FileEventType,
    /// File path (the new path for renames).
    pub path: PathBuf,
    /// Timestamp of the underlying filesystem event.
    pub timestamp: SystemTime,
}

impl FileEvent {
    /// Builds an event stamped with the current time.
    pub fn now(event_type: FileEventType, path: impl Into<PathBuf>) -> Self {
        Self {
            event_type,
            path: path.into(),
            timestamp: SystemTime::now(),
        }
    }
}
