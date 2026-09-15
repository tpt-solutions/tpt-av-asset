//! The public [`MediaWatcher`] API.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use tpt_av_asset_utils::AssetError;

use crate::backend::{default_backend, WatcherBackend};
use crate::debounce::EventDebouncer;
use crate::event::FileEvent;

/// Default per-path quiet window.
pub const DEFAULT_DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);

/// Filesystem watcher for media directories.
///
/// Events from the platform backend are coalesced per path by an
/// [`EventDebouncer`]: `recv` blocks until a path has been quiet for the
/// window, `try_recv` returns `None` while events are still inside their
/// window.
pub struct MediaWatcher {
    directories: Vec<PathBuf>,
    receiver: Receiver<FileEvent>,
    debouncer: EventDebouncer,
    backend: Box<dyn WatcherBackend>,
}

impl MediaWatcher {
    /// Creates a new media watcher with the platform default backend and
    /// [`DEFAULT_DEBOUNCE_WINDOW`].
    ///
    /// # Errors
    /// Returns [`AssetError`] if the backend cannot be created.
    pub fn new() -> Result<Self, AssetError> {
        Self::with_debounce(DEFAULT_DEBOUNCE_WINDOW)
    }

    /// Creates a watcher with a custom debounce window (the polling backend
    /// scans at this interval too, so smaller windows react faster at a
    /// higher CPU cost).
    ///
    /// # Errors
    /// Returns [`AssetError`] if the backend cannot be created.
    pub fn with_debounce(window: Duration) -> Result<Self, AssetError> {
        let (sender, receiver) = channel();
        let backend = default_backend(sender, window);
        Ok(Self {
            directories: Vec::new(),
            receiver,
            debouncer: EventDebouncer::new(window),
            backend,
        })
    }

    /// Creates a watcher around an explicit backend. `make_backend`
    /// receives the event sender the watcher reads from — backends must
    /// emit their raw events into that channel (tests and backends with
    /// special configuration).
    pub fn with_backend(
        make_backend: impl FnOnce(Sender<FileEvent>) -> Box<dyn WatcherBackend>,
        window: Duration,
    ) -> Self {
        let (sender, receiver) = channel();
        let backend = make_backend(sender);
        Self {
            directories: Vec::new(),
            receiver,
            debouncer: EventDebouncer::new(window),
            backend,
        }
    }

    /// Adds a directory (recursively) to the watch set.
    ///
    /// # Errors
    /// Returns [`AssetError`] if the directory does not exist or the
    /// backend refuses.
    pub fn watch(&mut self, directory: &Path) -> Result<(), AssetError> {
        self.backend.watch(directory)?;
        let canonical =
            std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        if !self.directories.contains(&canonical) {
            self.directories.push(canonical);
        }
        Ok(())
    }

    /// Stops watching a directory. Unwatching an unknown directory is a
    /// no-op.
    ///
    /// # Errors
    /// Returns [`AssetError`] if the backend fails to detach.
    pub fn unwatch(&mut self, directory: &Path) -> Result<(), AssetError> {
        self.backend.unwatch(directory)?;
        let canonical =
            std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        self.directories.retain(|d| *d != canonical);
        Ok(())
    }

    /// The watched directories (canonicalized).
    pub fn watched_directories(&self) -> &[PathBuf] {
        &self.directories
    }

    /// Receives the next debounced file event (blocking until one is
    /// ready).
    ///
    /// # Errors
    /// Returns [`AssetError::ChannelClosed`] when the watcher backend has
    /// been dropped without producing further events.
    pub fn recv(&mut self) -> Result<FileEvent, AssetError> {
        loop {
            if let Some(event) = self.debouncer.pop_ready(Instant::now()) {
                return Ok(event);
            }
            match self.receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(raw) => self.debouncer.push(raw, Instant::now()),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    // Surface anything still inside its window, then report closure.
                    if let Some(event) = self.debouncer.pop_ready(Instant::now()) {
                        return Ok(event);
                    }
                    let _ = self.debouncer.flush();
                    return Err(AssetError::ChannelClosed);
                }
            }
        }
    }

    /// Non-blocking receive: drains everything currently available from the
    /// backend and returns one event if any path's window has elapsed.
    ///
    /// # Errors
    /// Returns [`AssetError::ChannelClosed`] when the backend is gone.
    pub fn try_recv(&mut self) -> Result<Option<FileEvent>, AssetError> {
        loop {
            match self.receiver.try_recv() {
                Ok(raw) => self.debouncer.push(raw, Instant::now()),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err(AssetError::ChannelClosed),
            }
        }
        Ok(self.debouncer.pop_ready(Instant::now()))
    }

    /// Forces out all pending (still-debouncing) events, in quiet-time
    /// order. Useful at shutdown.
    pub fn drain_pending(&mut self) -> Vec<FileEvent> {
        self.debouncer.flush()
    }

    /// Backend name (diagnostics).
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }
}
