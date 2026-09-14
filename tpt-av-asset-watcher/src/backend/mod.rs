//! Platform watcher backends.
//!
//! | Module | Platform | Mechanism |
//! | :--- | :--- | :--- |
//! | [`read_dir`] | Windows (default), any platform | Periodic directory diff (polling). |
//! | [`inotify`]  | Linux | `notify` crate (inotify under the hood). |
//! | [`fsevents`] | macOS | `notify` crate (FSEvents under the hood). |

pub mod read_dir;

#[cfg(target_os = "linux")]
pub mod inotify;
#[cfg(target_os = "macos")]
pub mod fsevents;

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

use tpt_av_asset_utils::AssetError;

use crate::event::FileEvent;

/// A platform watcher implementation. Backends receive raw filesystem
/// events through the channel they were constructed with; `MediaWatcher`
/// debounces and surfaces them.
pub trait WatcherBackend: Send {
    /// Adds a directory (recursively) to the watch set.
    ///
    /// # Errors
    /// Returns [`AssetError`] if the directory cannot be watched.
    fn watch(&mut self, directory: &Path) -> Result<(), AssetError>;

    /// Removes a directory from the watch set. Unwatching an unknown
    /// directory is a no-op.
    ///
    /// # Errors
    /// Returns [`AssetError`] if the backend fails to detach.
    fn unwatch(&mut self, directory: &Path) -> Result<(), AssetError>;

    /// Human-readable backend name (logs/diagnostics).
    fn name(&self) -> &'static str;
}

/// Picks the platform default: inotify on Linux, FSEvents on macOS,
/// the polling directory-diff backend everywhere else (Windows).
pub fn default_backend(
    sender: Sender<FileEvent>,
    poll_interval: Duration,
) -> Box<dyn WatcherBackend> {
    #[cfg(target_os = "linux")]
    {
        Box::new(inotify::InotifyBackend::new(sender))
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(fsevents::FseventsBackend::new(sender))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Box::new(read_dir::PollingBackend::new(sender, poll_interval))
    }
}

/// Maps a `notify` event into raw [`FileEvent`]s (shared by the inotify and
/// FSEvents wrappers).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn map_notify_event(
    event: notify::Event,
    sender: &Sender<FileEvent>,
) {
    use notify::event::{ModifyKind, RenameMode};

    let timestamp = std::time::SystemTime::now();
    let events: Vec<FileEvent> = match &event.kind {
        notify::EventKind::Create(_) => event
            .paths
            .first()
            .map(|p| vec![FileEvent { event_type: crate::event::FileEventType::Created, path: p.clone(), timestamp }])
            .unwrap_or_default(),
        notify::EventKind::Remove(_) => event
            .paths
            .first()
            .map(|p| vec![FileEvent { event_type: crate::event::FileEventType::Deleted, path: p.clone(), timestamp }])
            .unwrap_or_default(),
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            if event.paths.len() >= 2 {
                vec![FileEvent {
                    event_type: crate::event::FileEventType::Renamed { old_path: event.paths[0].clone() },
                    path: event.paths[1].clone(),
                    timestamp,
                }]
            } else {
                Vec::new()
            }
        }
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::From)) => event
            .paths
            .first()
            .map(|p| vec![FileEvent { event_type: crate::event::FileEventType::Deleted, path: p.clone(), timestamp }])
            .unwrap_or_default(),
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::To)) => event
            .paths
            .first()
            .map(|p| vec![FileEvent { event_type: crate::event::FileEventType::Created, path: p.clone(), timestamp }])
            .unwrap_or_default(),
        notify::EventKind::Modify(_) => event
            .paths
            .first()
            .map(|p| vec![FileEvent { event_type: crate::event::FileEventType::Modified, path: p.clone(), timestamp }])
            .unwrap_or_default(),
        // Access events and anything else are irrelevant to caching.
        _ => Vec::new(),
    };
    for file_event in events {
        let _ = sender.send(file_event);
    }
}

/// Shared construction of the `notify` recommended watcher for the current
/// platform.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn spawn_notify_watcher(
    sender: Sender<FileEvent>,
) -> Result<notify::RecommendedWatcher, AssetError> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())
        .map_err(|e| AssetError::validation(format!("failed to start file watcher: {e}")))?;
    std::thread::Builder::new()
        .name("tpt-av-asset-notify".into())
        .spawn(move || {
            for res in rx {
                match res {
                    Ok(event) => map_notify_event(event, &sender),
                    Err(e) => log::warn!("file watcher error: {e}"),
                }
            }
        })
        .map_err(|e| AssetError::validation(format!("failed to spawn watcher thread: {e}")))?;
    Ok(watcher)
}
