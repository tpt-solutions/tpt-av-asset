//! Windows watcher backend (and portable fallback): a periodic directory
//! diff.
//!
//! Every `interval`, the watched trees are re-scanned and compared with the
//! previous snapshot. Differences become `Created` / `Modified` / `Deleted`
//! events. Renames surface as a `Deleted` + `Created` pair (the platform
//! gives polling no rename atomically); Linux/macOS backends deliver true
//! `Renamed` events.
//!
//! Scans are recursive with a depth cap of 16 to protect against pathological
//! trees. This backend is the default on Windows because it needs nothing
//! beyond `std`, and it builds and tests first since development happens on
//! Windows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tpt_av_asset_utils::AssetError;

use super::WatcherBackend;
use crate::event::{FileEvent, FileEventType};

const MAX_SCAN_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileState {
    size: u64,
    modified_ms: u64,
}

#[derive(Debug, Default)]
struct PollState {
    directories: Vec<PathBuf>,
    files: HashMap<PathBuf, FileState>,
}

/// Polling directory-diff backend.
pub struct PollingBackend {
    interval: Duration,
    state: Arc<Mutex<PollState>>,
    shutdown: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PollingBackend {
    /// Creates the backend and starts its scanner thread.
    pub fn new(sender: Sender<FileEvent>, interval: Duration) -> Self {
        let state = Arc::new(Mutex::new(PollState::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let handle = start_scanner(
            Arc::clone(&state),
            Arc::clone(&shutdown),
            sender.clone(),
            interval,
        );
        Self {
            interval,
            state,
            shutdown,
            handle: Some(handle),
        }
    }

    /// The scan interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

impl Drop for PollingBackend {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl WatcherBackend for PollingBackend {
    fn watch(&mut self, directory: &Path) -> Result<(), AssetError> {
        let dir = std::fs::canonicalize(directory)?;
        let mut state = self.state.lock().expect("poll state poisoned");
        if state.directories.contains(&dir) {
            return Ok(());
        }
        let mut snapshot = HashMap::new();
        scan_dir(&dir, &mut snapshot, 0);
        state.files.extend(snapshot);
        state.directories.push(dir);
        Ok(())
    }

    fn unwatch(&mut self, directory: &Path) -> Result<(), AssetError> {
        let dir = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
        let mut state = self.state.lock().expect("poll state poisoned");
        state.directories.retain(|d| *d != dir);
        state.files.retain(|path, _| !path.starts_with(&dir));
        Ok(())
    }

    fn name(&self) -> &'static str {
        "read_dir-polling"
    }
}

fn start_scanner(
    state: Arc<Mutex<PollState>>,
    shutdown: Arc<AtomicBool>,
    sender: Sender<FileEvent>,
    interval: Duration,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("tpt-av-asset-poll".into())
        .spawn(move || loop {
            if shutdown.load(Ordering::Acquire) {
                break;
            }
            // Sleep for the interval in small shutdown-checkable slices.
            let started = std::time::Instant::now();
            while started.elapsed() < interval {
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25).min(interval));
            }

            let mut state = match state.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            let mut fresh = HashMap::new();
            for dir in state.directories.clone() {
                scan_dir(&dir, &mut fresh, 0);
            }

            // Diff old → new.
            let mut events = Vec::new();
            for (path, old) in &state.files {
                match fresh.get(path) {
                    None => events.push(FileEvent::now(FileEventType::Deleted, path)),
                    Some(new) if new != old => {
                        events.push(FileEvent::now(FileEventType::Modified, path))
                    }
                    Some(_) => {}
                }
            }
            for (path, new) in &fresh {
                if !state.files.contains_key(path) {
                    events.push(FileEvent::now(FileEventType::Created, path));
                }
                let _ = new;
            }

            state.files = fresh;
            drop(state);
            for event in events {
                let _ = sender.send(event);
            }
        })
        .expect("failed to spawn polling thread")
}

fn scan_dir(dir: &Path, files: &mut HashMap<PathBuf, FileState>, depth: usize) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            scan_dir(&path, files, depth + 1);
        } else if meta.is_file() {
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            files.insert(
                path,
                FileState {
                    size: meta.len(),
                    modified_ms,
                },
            );
        }
    }
}
