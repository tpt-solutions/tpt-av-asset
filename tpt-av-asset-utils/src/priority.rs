//! Job priority levels.

use std::fmt;

/// Job priority levels, ordered so that [`Priority::Critical`] is the
/// highest (`Critical > High > Normal > Low`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Deferred processing (pre-caching, background tidy-up).
    Low,
    /// Regular background processing.
    Normal,
    /// User-initiated work that should jump the queue.
    High,
    /// UI-blocking work that must run first.
    Critical,
}

impl Priority {
    /// Highest priority value.
    pub const MAX: Priority = Priority::Critical;
    /// Lowest priority value.
    pub const MIN: Priority = Priority::Low;

    /// Stable wire tag (used by the database encoding).
    pub fn as_u8(self) -> u8 {
        match self {
            Priority::Low => 0,
            Priority::Normal => 1,
            Priority::High => 2,
            Priority::Critical => 3,
        }
    }

    /// Inverse of [`Priority::as_u8`]; returns `None` for unknown tags.
    pub fn from_u8(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Priority::Low),
            1 => Some(Priority::Normal),
            2 => Some(Priority::High),
            3 => Some(Priority::Critical),
            _ => None,
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Priority::Low => "Low",
            Priority::Normal => "Normal",
            Priority::High => "High",
            Priority::Critical => "Critical",
        };
        f.write_str(name)
    }
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_sorts_highest() {
        let mut priorities = vec![Priority::Normal, Priority::Critical, Priority::Low, Priority::High];
        priorities.sort();
        assert_eq!(
            priorities,
            vec![Priority::Low, Priority::Normal, Priority::High, Priority::Critical]
        );
        assert!(Priority::Critical > Priority::High);
        assert_eq!(Priority::default(), Priority::Normal);
    }

    #[test]
    fn tags_roundtrip() {
        for tag in 0..4u8 {
            let p = Priority::from_u8(tag).unwrap();
            assert_eq!(p.as_u8(), tag);
        }
        assert!(Priority::from_u8(4).is_none());
    }

    #[test]
    fn display_names() {
        assert_eq!(Priority::Critical.to_string(), "Critical");
        assert_eq!(Priority::Low.to_string(), "Low");
    }
}
