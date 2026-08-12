//! Bar-index ranges. Port of `src/model/range-impl.ts` and the logical range concept from
//! `src/model/time-data.ts`.

use crate::TimePointIndex;

/// Inclusive integer range of time-point indices ("strict range" in the reference charting library).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrictRange {
    left: TimePointIndex,
    right: TimePointIndex,
}

impl StrictRange {
    /// Panics if `left > right` (mirrors the reference's assertion).
    pub fn new(left: TimePointIndex, right: TimePointIndex) -> Self {
        assert!(left <= right, "right should be >= left");
        Self { left, right }
    }

    pub fn left(&self) -> TimePointIndex {
        self.left
    }

    pub fn right(&self) -> TimePointIndex {
        self.right
    }

    pub fn count(&self) -> i64 {
        self.right - self.left + 1
    }

    pub fn contains(&self, index: TimePointIndex) -> bool {
        self.left <= index && index <= self.right
    }
}

/// Float logical range: bar-centric coordinates where integer values are bar centers.
/// `from`/`to` are the left/right borders of the visible window in bar units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRange {
    left: f64,
    right: f64,
}

impl LogicalRange {
    pub fn new(left: f64, right: f64) -> Self {
        assert!(left <= right, "right should be >= left");
        Self { left, right }
    }

    pub fn left(&self) -> f64 {
        self.left
    }

    pub fn right(&self) -> f64 {
        self.right
    }

    /// Strict (integer) range covered by this logical range, rounded outward.
    /// Port of `TimeScaleVisibleRange.strictRange()`:
    /// `floor(left)` .. `ceil(right)`.
    pub fn to_strict(&self) -> StrictRange {
        StrictRange::new(self.left.floor() as i64, self.right.ceil() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_range_basics() {
        let r = StrictRange::new(-2, 5);
        assert_eq!(r.count(), 8);
        assert!(r.contains(-2));
        assert!(r.contains(5));
        assert!(!r.contains(6));
    }

    #[test]
    fn logical_to_strict_rounds_outward() {
        let r = LogicalRange::new(1.2, 7.8);
        let s = r.to_strict();
        assert_eq!(s.left(), 1);
        assert_eq!(s.right(), 8);
    }
}
