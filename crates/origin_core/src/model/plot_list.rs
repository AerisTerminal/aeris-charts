//! Series row storage with chunked min/max cache. Port of `src/model/plot-list.ts`,
//! restructured as SoA (parallel column vectors) for cache-friendly scans.
//!
//! Rows are keyed by *time-point index* (position in the merged time scale), which may be
//! sparse when a series has whitespace. All searches are binary over the sorted index column.

use std::collections::HashMap;

use crate::helpers::algorithms::{lower_bound, upper_bound};
use crate::TimePointIndex;

/// `CHUNK_SIZE` in reference.
const CHUNK_SIZE: i64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotValueIndex {
    Open = 0,
    High = 1,
    Low = 2,
    Close = 3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinMax {
    pub min: f64,
    pub max: f64,
}

fn merge_min_max(first: Option<MinMax>, second: Option<MinMax>) -> Option<MinMax> {
    match (first, second) {
        (None, s) => s,
        (f, None) => f,
        (Some(f), Some(s)) => Some(MinMax {
            min: f.min.min(s.min),
            max: f.max.max(s.max),
        }),
    }
}

/// Search direction when no row exists at the exact index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MismatchDirection {
    NearestLeft,
    None,
    NearestRight,
}

#[derive(Default)]
pub struct PlotList {
    indices: Vec<TimePointIndex>,
    /// open/high/low/close columns; single-value series alias the same value into all four,
    /// matching the reference's plot row layout.
    values: [Vec<f64>; 4],
    /// (plot, chunk_index) -> chunk min/max. Cleared on set_data.
    min_max_cache: HashMap<(usize, i64), Option<MinMax>>,
}

impl PlotList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_data(
        &mut self,
        indices: Vec<TimePointIndex>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
    ) {
        debug_assert!(
            indices.windows(2).all(|w| w[0] < w[1]),
            "indices must be sorted unique"
        );
        debug_assert!(
            indices.len() == open.len()
                && open.len() == high.len()
                && high.len() == low.len()
                && low.len() == close.len()
        );
        self.indices = indices;
        self.values = [open, high, low, close];
        self.min_max_cache.clear();
    }

    /// Streaming append/replace of the last row (the `update()` hot path).
    pub fn upsert_last(&mut self, index: TimePointIndex, values: [f64; 4]) {
        match self.indices.last() {
            Some(&last) if index == last => {
                let row = self.indices.len() - 1;
                for (col, v) in self.values.iter_mut().zip(values) {
                    col[row] = v;
                }
                // invalidate the chunk containing this row
                let chunk = index.div_euclid(CHUNK_SIZE);
                for plot in 0..4 {
                    self.min_max_cache.remove(&(plot, chunk));
                }
            }
            Some(&last) if index > last => {
                self.indices.push(index);
                for (col, v) in self.values.iter_mut().zip(values) {
                    col.push(v);
                }
            }
            None => {
                self.indices.push(index);
                for (col, v) in self.values.iter_mut().zip(values) {
                    col.push(v);
                }
            }
            _ => {
                // Out-of-order updates are routed to the rebuild path by `DataLayer::update`;
                // reaching this arm is an internal ordering bug. Ignore rather than poison the
                // whole chart (a panic aborts the wasm instance).
                debug_assert!(false, "cannot update older data: index {index} < last");
            }
        }
    }

    pub fn size(&self) -> usize {
        self.indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn first_index(&self) -> Option<TimePointIndex> {
        self.indices.first().copied()
    }

    pub fn last_index(&self) -> Option<TimePointIndex> {
        self.indices.last().copied()
    }

    pub fn indices(&self) -> &[TimePointIndex] {
        &self.indices
    }

    pub fn column(&self, plot: PlotValueIndex) -> &[f64] {
        &self.values[plot as usize]
    }

    pub fn contains(&self, index: TimePointIndex) -> bool {
        self.bsearch(index).is_some()
    }

    /// Row offset of `index`, honoring the mismatch direction. Port of `_search`.
    pub fn search(&self, index: TimePointIndex, direction: MismatchDirection) -> Option<usize> {
        let exact = self.bsearch(index);
        if exact.is_none() && direction != MismatchDirection::None {
            return match direction {
                MismatchDirection::NearestLeft => self.search_nearest_left(index),
                MismatchDirection::NearestRight => self.search_nearest_right(index),
                MismatchDirection::None => unreachable!(),
            };
        }
        exact
    }

    pub fn value_at(&self, row: usize, plot: PlotValueIndex) -> f64 {
        self.values[plot as usize][row]
    }

    /// Whether `row` is a whitespace row (an reference `{time}`-only item, data-consumer.ts
    /// `isWhitespaceData`): the boundary convention is that all four values are NaN — a real
    /// bar never has all four NaN, and single-value series alias the one value into every
    /// column, so a NaN value is whitespace there too. Whitespace rows occupy their time
    /// point (they keep the axis slot and stay aligned with the per-point color channels)
    /// but draw nothing and are skipped by autoscale and last-value tracking.
    pub fn is_whitespace_row(&self, row: usize) -> bool {
        self.values.iter().all(|col| col[row].is_nan())
    }

    /// Row of the last non-whitespace bar at or left of `index`, or `None` when every bar
    /// at or left of it is whitespace (or there is no bar at all). the reference's whitespace-filtered
    /// plot list makes `search(NearestLeft)` land on the last real bar directly
    /// (series.ts `lastValueData`); here the whitespace rows are in the list, so scan past
    /// them to the same effect.
    pub fn last_non_whitespace_row(&self, index: TimePointIndex) -> Option<usize> {
        let mut row = self.search(index, MismatchDirection::NearestLeft)?;
        loop {
            if !self.is_whitespace_row(row) {
                return Some(row);
            }
            if row == 0 {
                return None;
            }
            row -= 1;
        }
    }

    /// Row of the first non-whitespace bar at or right of `index` (the mirror of
    /// [`last_non_whitespace_row`], for first-value/base-value selection).
    pub fn first_non_whitespace_row(&self, index: TimePointIndex) -> Option<usize> {
        let mut row = self.search(index, MismatchDirection::NearestRight)?;
        loop {
            if !self.is_whitespace_row(row) {
                return Some(row);
            }
            row += 1;
            if row >= self.indices.len() {
                return None;
            }
        }
    }

    /// Row offsets `[start, end)` whose merged index lies in the inclusive range `[from, to]`.
    /// Used to slice a (possibly sparse) series to the visible window for rendering.
    pub fn visible_rows(&self, from: TimePointIndex, to: TimePointIndex) -> std::ops::Range<usize> {
        self.lowerbound(from)..self.upperbound(to)
    }

    /// Min/max over the time-point index range `[start, end]` (inclusive, like the strict
    /// visible range), merged over `plots`. Port of `minMaxOnRangeCached`.
    pub fn min_max_on_range_cached(
        &mut self,
        start: TimePointIndex,
        end: TimePointIndex,
        plots: &[PlotValueIndex],
    ) -> Option<MinMax> {
        if self.is_empty() {
            return None;
        }

        let mut result: Option<MinMax> = None;
        for &plot in plots {
            let plot_min_max = self.min_max_on_range_cached_impl(start, end, plot);
            result = merge_min_max(result, plot_min_max);
        }
        result
    }

    fn bsearch(&self, index: TimePointIndex) -> Option<usize> {
        let start = self.lowerbound(index);
        if start != self.indices.len() && index >= self.indices[start] {
            return Some(start);
        }
        None
    }

    fn search_nearest_left(&self, index: TimePointIndex) -> Option<usize> {
        let pos = self.lowerbound(index).saturating_sub(1);
        (pos != self.indices.len() && self.indices[pos] < index).then_some(pos)
    }

    fn search_nearest_right(&self, index: TimePointIndex) -> Option<usize> {
        let pos = self.upperbound(index);
        (pos != self.indices.len() && index < self.indices[pos]).then_some(pos)
    }

    fn lowerbound(&self, index: TimePointIndex) -> usize {
        lower_bound(&self.indices, |&i| i < index)
    }

    fn upperbound(&self, index: TimePointIndex) -> usize {
        upper_bound(&self.indices, |&i| i > index)
    }

    /// Brute min/max over row offsets `[start_row, end_row)`, skipping NaN.
    /// (the reference's for-loop is a no-op when start >= end; Rust slicing would panic, so guard.)
    fn plot_min_max(&self, start_row: usize, end_row: usize, plot: usize) -> Option<MinMax> {
        if start_row >= end_row {
            return None;
        }
        let mut result: Option<MinMax> = None;
        let col = &self.values[plot];
        for &v in &col[start_row..end_row] {
            if v.is_nan() {
                continue;
            }
            result = Some(match result {
                None => MinMax { min: v, max: v },
                Some(mm) => MinMax {
                    min: mm.min.min(v),
                    max: mm.max.max(v),
                },
            });
        }
        result
    }

    fn min_max_on_range_cached_impl(
        &mut self,
        start: TimePointIndex,
        end: TimePointIndex,
        plot: PlotValueIndex,
    ) -> Option<MinMax> {
        if self.is_empty() {
            return None;
        }
        let plot = plot as usize;

        let (Some(first_index), Some(last_index)) = (self.first_index(), self.last_index()) else {
            return None;
        };

        let s = start.max(first_index);
        let e = end.min(last_index);

        // chunk boundaries in time-point index space
        let cached_low = ((s as f64) / CHUNK_SIZE as f64).ceil() as i64 * CHUNK_SIZE;
        let cached_high =
            cached_low.max(((e as f64) / CHUNK_SIZE as f64).floor() as i64 * CHUNK_SIZE);

        let mut result: Option<MinMax> = None;

        // head: [s, min(e, cached_low, end)) — non-inclusive end via upperbound
        {
            let start_row = self.lowerbound(s);
            let end_row = self.upperbound(e.min(cached_low).min(end));
            result = merge_min_max(result, self.plot_min_max(start_row, end_row, plot));
        }

        // cached chunks
        let mut c = (cached_low + 1).max(s);
        while c < cached_high {
            let chunk_index = c.div_euclid(CHUNK_SIZE);

            let chunk_min_max = match self.min_max_cache.get(&(plot, chunk_index)) {
                Some(&mm) => mm,
                None => {
                    let chunk_start = self.lowerbound(chunk_index * CHUNK_SIZE);
                    let chunk_end = self.upperbound((chunk_index + 1) * CHUNK_SIZE - 1);
                    let mm = self.plot_min_max(chunk_start, chunk_end, plot);
                    self.min_max_cache.insert((plot, chunk_index), mm);
                    mm
                }
            };

            result = merge_min_max(result, chunk_min_max);
            c += CHUNK_SIZE;
        }

        // tail: [cached_high, e]
        {
            let start_row = self.lowerbound(cached_high);
            let end_row = self.upperbound(e);
            result = merge_min_max(result, self.plot_min_max(start_row, end_row, plot));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_list(n: i64) -> PlotList {
        // index i: low = i, high = i + 10
        let indices: Vec<i64> = (0..n).collect();
        let open: Vec<f64> = (0..n).map(|i| i as f64 + 5.0).collect();
        let high: Vec<f64> = (0..n).map(|i| i as f64 + 10.0).collect();
        let low: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let close: Vec<f64> = (0..n).map(|i| i as f64 + 7.0).collect();
        let mut pl = PlotList::new();
        pl.set_data(indices, open, high, low, close);
        pl
    }

    #[test]
    fn search_modes() {
        let mut pl = PlotList::new();
        pl.set_data(
            vec![2, 5, 9],
            vec![1.0, 2.0, 3.0],
            vec![1.0, 2.0, 3.0],
            vec![1.0, 2.0, 3.0],
            vec![1.0, 2.0, 3.0],
        );
        assert_eq!(pl.search(5, MismatchDirection::None), Some(1));
        assert_eq!(pl.search(4, MismatchDirection::None), None);
        assert_eq!(pl.search(4, MismatchDirection::NearestLeft), Some(0));
        assert_eq!(pl.search(4, MismatchDirection::NearestRight), Some(1));
        assert_eq!(pl.search(1, MismatchDirection::NearestLeft), None);
        assert_eq!(pl.search(10, MismatchDirection::NearestRight), None);
    }

    #[test]
    fn min_max_matches_brute_force_across_chunks() {
        let mut pl = make_list(200);
        for (start, end) in [
            (0i64, 199i64),
            (5, 25),
            (29, 31),
            (30, 89),
            (61, 150),
            (100, 100),
        ] {
            let cached = pl
                .min_max_on_range_cached(start, end, &[PlotValueIndex::Low, PlotValueIndex::High])
                .unwrap();
            // brute force: low = i, high = i + 10
            assert_eq!(cached.min, start as f64, "range {start}..{end}");
            assert_eq!(cached.max, end as f64 + 10.0, "range {start}..{end}");
        }
    }

    #[test]
    fn min_max_cache_is_consistent_on_repeat() {
        let mut pl = make_list(500);
        let a = pl
            .min_max_on_range_cached(50, 450, &[PlotValueIndex::Low])
            .unwrap();
        let b = pl
            .min_max_on_range_cached(50, 450, &[PlotValueIndex::Low])
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.min, 50.0);
        assert_eq!(a.max, 450.0);
    }

    #[test]
    fn min_max_clamps_to_data_bounds() {
        let mut pl = make_list(10);
        let mm = pl
            .min_max_on_range_cached(-100, 100, &[PlotValueIndex::Close])
            .unwrap();
        assert_eq!(mm.min, 7.0);
        assert_eq!(mm.max, 16.0);
    }

    #[test]
    fn nan_values_are_skipped() {
        let mut pl = PlotList::new();
        pl.set_data(
            vec![0, 1, 2],
            vec![1.0, f64::NAN, 3.0],
            vec![1.0, f64::NAN, 3.0],
            vec![1.0, f64::NAN, 3.0],
            vec![1.0, f64::NAN, 3.0],
        );
        let mm = pl
            .min_max_on_range_cached(0, 2, &[PlotValueIndex::Close])
            .unwrap();
        assert_eq!(mm.min, 1.0);
        assert_eq!(mm.max, 3.0);
    }

    #[test]
    fn upsert_last_invalidates_chunk_cache() {
        let mut pl = make_list(100);
        // warm the cache
        let before = pl
            .min_max_on_range_cached(0, 99, &[PlotValueIndex::High])
            .unwrap();
        assert_eq!(before.max, 109.0);
        // replace last bar with a spike
        pl.upsert_last(99, [50.0, 999.0, 40.0, 60.0]);
        let after = pl
            .min_max_on_range_cached(0, 99, &[PlotValueIndex::High])
            .unwrap();
        assert_eq!(after.max, 999.0);
        // append a new bar
        pl.upsert_last(100, [1.0, 2000.0, 0.5, 1.5]);
        let appended = pl
            .min_max_on_range_cached(0, 100, &[PlotValueIndex::High])
            .unwrap();
        assert_eq!(appended.max, 2000.0);
    }

    #[test]
    fn sparse_indices_whitespace() {
        let mut pl = PlotList::new();
        // data at indices 0, 10, 20 (whitespace between)
        pl.set_data(
            vec![0, 10, 20],
            vec![1.0, 5.0, 3.0],
            vec![2.0, 6.0, 4.0],
            vec![0.5, 4.0, 2.0],
            vec![1.5, 5.5, 3.5],
        );
        let mm = pl
            .min_max_on_range_cached(5, 15, &[PlotValueIndex::High])
            .unwrap();
        assert_eq!(mm.max, 6.0); // only index 10 in range
        assert_eq!(pl.search(15, MismatchDirection::NearestLeft), Some(1));
    }

    #[test]
    fn whitespace_rows_are_skipped_by_non_whitespace_searches() {
        let mut pl = PlotList::new();
        // bars at 0, 3; explicit whitespace rows at 1, 2 (all-NaN, reference `{time}`-only items)
        let nan = f64::NAN;
        pl.set_data(
            vec![0, 1, 2, 3],
            vec![1.0, nan, nan, 4.0],
            vec![1.0, nan, nan, 4.0],
            vec![1.0, nan, nan, 4.0],
            vec![1.0, nan, nan, 4.0],
        );
        assert!(pl.is_whitespace_row(1));
        assert!(pl.is_whitespace_row(2));
        assert!(!pl.is_whitespace_row(0));
        assert!(!pl.is_whitespace_row(3));

        // last-value tracking scans left past whitespace (series.ts lastValueData)
        assert_eq!(pl.last_non_whitespace_row(3), Some(3));
        assert_eq!(pl.last_non_whitespace_row(2), Some(0));
        assert_eq!(pl.last_non_whitespace_row(10), Some(3));
        // first-value selection scans right past whitespace
        assert_eq!(pl.first_non_whitespace_row(0), Some(0));
        assert_eq!(pl.first_non_whitespace_row(1), Some(3));
        assert_eq!(pl.first_non_whitespace_row(-5), Some(0));
        // min/max already ignores the NaN rows
        let mm = pl
            .min_max_on_range_cached(0, 3, &[PlotValueIndex::Close])
            .unwrap();
        assert_eq!(mm.min, 1.0);
        assert_eq!(mm.max, 4.0);
    }

    #[test]
    fn all_whitespace_list_has_no_non_whitespace_rows() {
        let mut pl = PlotList::new();
        let nan = f64::NAN;
        pl.set_data(
            vec![0, 1],
            vec![nan, nan],
            vec![nan, nan],
            vec![nan, nan],
            vec![nan, nan],
        );
        assert_eq!(pl.last_non_whitespace_row(1), None);
        assert_eq!(pl.first_non_whitespace_row(0), None);
        assert_eq!(
            pl.min_max_on_range_cached(0, 1, &[PlotValueIndex::Close]),
            None
        );
    }
}
