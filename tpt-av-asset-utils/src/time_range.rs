//! Time range type shared by cache, proxy, and pipeline APIs.

use crate::error::AssetError;

/// A half-open time range `[start, end)` in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeRange {
    /// Range start in seconds (inclusive).
    pub start_secs: f64,
    /// Range end in seconds (exclusive).
    pub end_secs: f64,
}

impl TimeRange {
    /// Creates a validated range.
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] if either bound is negative/NaN or
    /// `start >= end`.
    pub fn new(start_secs: f64, end_secs: f64) -> Result<Self, AssetError> {
        if !(start_secs.is_finite() && end_secs.is_finite())
            || start_secs < 0.0
            || start_secs >= end_secs
        {
            return Err(AssetError::validation(format!(
                "invalid time range [{start_secs}, {end_secs})"
            )));
        }
        Ok(Self {
            start_secs,
            end_secs,
        })
    }

    /// Range length in seconds.
    pub fn duration(&self) -> f64 {
        self.end_secs - self.start_secs
    }

    /// True if `t` lies within the range.
    pub fn contains(&self, t: f64) -> bool {
        t >= self.start_secs && t < self.end_secs
    }

    /// True if the ranges share at least one instant.
    pub fn intersects(&self, other: &TimeRange) -> bool {
        self.start_secs < other.end_secs && other.start_secs < self.end_secs
    }

    /// The overlapping portion of the two ranges, if any.
    pub fn overlap(&self, other: &TimeRange) -> Option<TimeRange> {
        if !self.intersects(other) {
            return None;
        }
        TimeRange::new(
            self.start_secs.max(other.start_secs),
            self.end_secs.min(other.end_secs),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_rules() {
        assert!(TimeRange::new(1.0, 2.0).is_ok());
        assert!(TimeRange::new(-1.0, 2.0).is_err());
        assert!(TimeRange::new(2.0, 2.0).is_err());
        assert!(TimeRange::new(3.0, 2.0).is_err());
        assert!(TimeRange::new(f64::NAN, 2.0).is_err());
    }

    #[test]
    fn containment_and_intersection() {
        let a = TimeRange::new(0.0, 10.0).unwrap();
        assert!(a.contains(0.0) && !a.contains(10.0) && a.contains(9.999));

        let b = TimeRange::new(5.0, 15.0).unwrap();
        let c = TimeRange::new(10.0, 12.0).unwrap();
        let d = TimeRange::new(20.0, 30.0).unwrap();
        assert!(a.intersects(&b));
        assert!(
            !a.intersects(&c),
            "half-open: [0,10) and [10,12) do not touch"
        );
        assert!(!a.intersects(&d));

        let ov = a.overlap(&b).unwrap();
        assert_eq!((ov.start_secs, ov.end_secs), (5.0, 10.0));
        assert!(a.overlap(&d).is_none());
        assert!((a.duration() - 10.0).abs() < f64::EPSILON);
    }
}
