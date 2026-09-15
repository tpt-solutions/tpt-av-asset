//! macOS watcher backend.
//!
//! Thin wrapper over the `notify` crate, whose macOS implementation is
//! backed by FSEvents. Events are mapped into the crate-neutral
//! [`FileEvent`] shape, including true `Renamed` events where FSEvents
//! reports both sides.

use std::path::Path;
use std::sync::mpsc::Sender;

use tpt_av_asset_utils::AssetError;

use super::WatcherBackend;
use crate::event::FileEvent;

/// macOS (FSEvents) backend.
pub struct FseventsBackend {
    watcher: Option<notify::RecommendedWatcher>,
    sender: Sender<FileEvent>,
}

impl FseventsBackend {
    /// Creates the backend; the underlying FSEvents watcher starts on the
    /// first [`WatcherBackend::watch`] call.
    pub fn new(sender: Sender<FileEvent>) -> Self {
        Self {
            watcher: None,
            sender,
        }
    }
}

impl WatcherBackend for FseventsBackend {
    fn watch(&mut self, directory: &Path) -> Result<(), AssetError> {
        if self.watcher.is_none() {
            self.watcher = Some(super::spawn_notify_watcher(self.sender.clone())?);
        }
        self.watcher
            .as_mut()
            .expect("watcher just initialized")
            .watch(directory, notify::RecursiveMode::Recursive)
            .map_err(|e| {
                AssetError::validation(format!("cannot watch {}: {e}", directory.display()))
            })
    }

    fn unwatch(&mut self, directory: &Path) -> Result<(), AssetError> {
        if let Some(watcher) = self.watcher.as_mut() {
            watcher.unwatch(directory).map_err(|e| {
                AssetError::validation(format!("cannot unwatch {}: {e}", directory.display()))
            })?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "fsevents"
    }
}
