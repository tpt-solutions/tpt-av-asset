//! Cross-platform filesystem monitoring for media directories.
//!
//! [`MediaWatcher`] delivers debounced [`FileEvent`]s (`Created`,
//! `Modified`, `Deleted`, `Renamed`) for everything under the watched
//! directories, regardless of platform backend:
//!
//! - Linux: inotify via the `notify` crate
//! - macOS: FSEvents via the `notify` crate
//! - Windows (default): periodic directory diff (`backend::read_dir`)
//!
//! Events pass through a per-path quiet-window debouncer, so an edit storm
//! surfaces as a single `Modified`.
//!
//! [`CacheInvalidator`] bridges these events into
//! `tpt-av-asset-db::invalidate_caches` plus on-disk cache removal, making
//! cache invalidation automatic when a source file changes, moves, or is
//! deleted.

pub mod backend;
pub mod debounce;
pub mod event;
pub mod invalidate;
pub mod watcher;

pub use backend::{read_dir::PollingBackend, WatcherBackend};
pub use debounce::EventDebouncer;
pub use event::{FileEvent, FileEventType};
pub use invalidate::CacheInvalidator;
pub use watcher::MediaWatcher;
