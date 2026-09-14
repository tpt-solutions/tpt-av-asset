//! Event debouncing: coalesces event storms per path.
//!
//! Editors and converters frequently touch a file many times in quick
//! succession. The debouncer keeps, per path, only the most recent event and
//! only surfaces it once the path has been quiet for the configured window
//! — so downstream cache invalidation runs once, not once per keystroke.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::event::FileEvent;

/// Per-path event coalescer.
#[derive(Debug)]
pub struct EventDebouncer {
    window: Duration,
    pending: HashMap<PathBuf, (FileEvent, Instant)>,
}

impl EventDebouncer {
    /// Creates a debouncer with the given quiet window.
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
        }
    }

    /// The configured quiet window.
    pub fn window(&self) -> Duration {
        self.window
    }

    /// Feeds a raw event in. A newer event for the same path replaces the
    /// pending one and restarts its quiet timer.
    pub fn push(&mut self, event: FileEvent, now: Instant) {
        self.pending
            .insert(event.path.clone(), (event, now + self.window));
    }

    /// Pops the oldest event whose quiet window has elapsed, if any.
    pub fn pop_ready(&mut self, now: Instant) -> Option<FileEvent> {
        let oldest = self
            .pending
            .iter()
            .filter(|(_, (_, ready_at))| *ready_at <= now)
            .min_by_key(|(_, (_, ready_at))| *ready_at)
            .map(|(path, _)| path.clone());
        oldest.map(|path| self.pending.remove(&path).expect("key just yielded").0)
    }

    /// Forces out every pending event (used on shutdown and in tests).
    pub fn flush(&mut self) -> Vec<FileEvent> {
        let mut events: Vec<(Instant, FileEvent)> = self
            .pending
            .drain()
            .map(|(_, (event, ready_at))| (ready_at, event))
            .collect();
        events.sort_by_key(|(ready_at, _)| *ready_at);
        events.into_iter().map(|(_, event)| event).collect()
    }

    /// Number of paths with pending (not yet surfaced) events.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True if nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{FileEventType, FileEvent};
    use std::time::SystemTime;

    fn event(path: &str, kind: FileEventType) -> FileEvent {
        FileEvent {
            event_type: kind,
            path: PathBuf::from(path),
            timestamp: SystemTime::now(),
        }
    }

    #[test]
    fn events_coalesce_per_path_and_surface_after_window() {
        let mut d = EventDebouncer::new(Duration::from_millis(100));
        let t0 = Instant::now();

        d.push(event("/m/a.wav", FileEventType::Created), t0);
        d.push(event("/m/a.wav", FileEventType::Modified), t0 + Duration::from_millis(10));
        d.push(event("/m/b.wav", FileEventType::Created), t0);

        // Window not elapsed: nothing ready.
        assert!(d.pop_ready(t0 + Duration::from_millis(50)).is_none());
        assert_eq!(d.len(), 2, "two paths pending");

        // After both windows: b.wav's quiet time (t0+100ms) is older than
        // a.wav's restarted one (t0+110ms), so it surfaces first.
        let b = d.pop_ready(t0 + Duration::from_millis(150)).unwrap();
        assert_eq!(b.path, PathBuf::from("/m/b.wav"));
        assert_eq!(d.len(), 1);

        let a = d.pop_ready(t0 + Duration::from_millis(150)).unwrap();
        assert_eq!(a.path, PathBuf::from("/m/a.wav"));
        assert_eq!(a.event_type, FileEventType::Modified, "coalesced to the last event");
        assert!(d.is_empty());
        assert!(d.pop_ready(t0 + Duration::from_secs(1)).is_none());
    }

    #[test]
    fn newest_event_wins_and_timer_restarts() {
        let mut d = EventDebouncer::new(Duration::from_millis(100));
        let t0 = Instant::now();
        d.push(event("/m/a.wav", FileEventType::Created), t0);
        d.push(
            event("/m/a.wav", FileEventType::Modified),
            t0 + Duration::from_millis(90),
        );
        // 100ms after t0 the first window would have elapsed, but the second
        // push restarted the timer.
        assert!(d.pop_ready(t0 + Duration::from_millis(120)).is_none());
        let e = d.pop_ready(t0 + Duration::from_millis(200)).unwrap();
        assert_eq!(e.event_type, FileEventType::Modified);
    }

    #[test]
    fn flush_returns_everything_deterministically() {
        let mut d = EventDebouncer::new(Duration::from_secs(10));
        let t0 = Instant::now();
        d.push(event("/m/a.wav", FileEventType::Created), t0);
        d.push(event("/m/c.wav", FileEventType::Created), t0 + Duration::from_millis(5));
        d.push(event("/m/b.wav", FileEventType::Created), t0 + Duration::from_millis(1));
        let events = d.flush();
        assert_eq!(
            events.iter().map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(),
            vec!["a.wav", "b.wav", "c.wav"],
            "flush order follows quiet-time order"
        );
        assert!(d.is_empty());
    }
}
