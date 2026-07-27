//! Multi-series data layer. Port of the merged-time-point concept from `src/model/data-layer.ts`,
//! restructured for our SoA storage.
//!
//! The horizontal axis is indexed by position in the **union** of every series' timestamps
//! (the "merged time points"). Each series maps its own data onto those indices; a series with
//! no point at a given index is simply absent there (whitespace). This is what lets a price
//! series and a volume series — or a candlestick and a moving-average overlay — share one time
//! scale even when their sample sets differ.

use crate::helpers::algorithms::lower_bound;
use crate::model::data_validation::is_whitespace_values;
use crate::model::plot_list::PlotList;
use crate::TimePointIndex;

pub type SeriesId = usize;

/// Per-data-item color channels (reference data-item colors, model/series-bar-colorer.ts). `Body` is
/// the candle/bar body color, the line/area stroke (reference `lineColor` on area items) and point
/// marker, and the histogram column color; `Wick`/`Border` are the candlestick parts
/// (`wickColor`/`borderColor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointColorChannel {
    Body = 0,
    Wick = 1,
    Border = 2,
}

/// Number of [`PointColorChannel`] slots carried per row.
pub const POINT_COLOR_CHANNELS: usize = 3;

/// A `0` entry in a present channel means "no override at this row" (a fully transparent color
/// is not renderable, so 0 is reserved as the absent marker).
pub const POINT_COLOR_ABSENT: u32 = 0;

struct RawSeries {
    times: Vec<i64>,
    values: [Vec<f64>; 4],
    /// Per-row color overrides indexed by `PointColorChannel`. Each channel is either empty
    /// (absent for the whole series) or aligned 1:1 with `times`; kept in lockstep with the
    /// value columns across set_data/update so plot rows (which mirror raw rows) stay aligned.
    point_colors: [Vec<u32>; POINT_COLOR_CHANNELS],
    /// Custom series (plugin platform Phase C-c): their values live host-side, so the rows
    /// here carry times only (whitespace-style) — yet they still mark real bars for the
    /// time-scale base index (the reference's custom plot rows carry values, so they count in
    /// `_getBaseIndex`).
    rows_count_as_data: bool,
    /// Rebuilt against merged indices; keys are positions in `merged_times`.
    plot: PlotList,
}

impl RawSeries {
    fn empty() -> Self {
        Self {
            times: Vec::new(),
            values: [vec![], vec![], vec![], vec![]],
            point_colors: [vec![], vec![], vec![]],
            rows_count_as_data: false,
            plot: PlotList::new(),
        }
    }
}

#[derive(Default)]
pub struct DataLayer {
    series: Vec<RawSeries>,
    merged_times: Vec<i64>,
}

impl DataLayer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_series(&mut self) -> SeriesId {
        self.series.push(RawSeries::empty());
        self.series.len() - 1
    }

    pub fn series_count(&self) -> usize {
        self.series.len()
    }

    /// Union of all series' timestamps, sorted ascending (the time-scale points).
    pub fn merged_times(&self) -> &[i64] {
        &self.merged_times
    }

    pub fn plot(&self, id: SeriesId) -> &PlotList {
        &self.series[id].plot
    }

    pub fn plot_mut(&mut self, id: SeriesId) -> &mut PlotList {
        &mut self.series[id].plot
    }

    /// Mark a series whose time-only rows still count as data rows for [`base_index`] (custom
    /// series, Phase C-c). Set at series creation / kind conversion by the engine, which owns
    /// the kind knowledge; an unknown id is ignored.
    pub fn set_rows_count_as_data(&mut self, id: SeriesId, flag: bool) {
        if let Some(s) = self.series.get_mut(id) {
            s.rows_count_as_data = flag;
        }
    }

    /// Raw series columns for platform-independent derived-data producers.
    pub fn series_data(&self, id: SeriesId) -> Option<(&[i64], [&[f64]; 4])> {
        let s = self.series.get(id)?;
        Some((
            &s.times,
            [&s.values[0], &s.values[1], &s.values[2], &s.values[3]],
        ))
    }

    /// Merged index of the last point that has data (the time-scale base index), or None.
    /// Whitespace rows (reference `{time}`-only items) occupy time points but carry no data, so —
    /// like the reference's `_getBaseIndex` (data-layer.ts:495-510), which reads the whitespace-filtered
    /// series rows — the base index is the last point holding a real bar in any series. When
    /// every series' rows are whitespace the index is 0 (the reference's initialized `baseIndex`).
    pub fn base_index(&self) -> Option<TimePointIndex> {
        if self.merged_times.is_empty() {
            return None;
        }
        let mut last_data_time: Option<i64> = None;
        for s in &self.series {
            let row = s.times.len();
            for row in (0..row).rev() {
                let values = [
                    s.values[0][row],
                    s.values[1][row],
                    s.values[2][row],
                    s.values[3][row],
                ];
                if s.rows_count_as_data || !is_whitespace_values(values) {
                    last_data_time = Some(match last_data_time {
                        Some(t) => t.max(s.times[row]),
                        None => s.times[row],
                    });
                    break;
                }
            }
        }
        match last_data_time {
            // The time came from a series, so it is in the merged union by construction; fall
            // back to the insertion point instead of panicking if that invariant ever breaks.
            Some(t) => Some(
                self.merged_times
                    .binary_search(&t)
                    .unwrap_or_else(|pos| pos.min(self.merged_times.len() - 1))
                    as TimePointIndex,
            ),
            None => Some(0),
        }
    }

    /// Full (re)assignment of a series' data. `times` must be ascending. Rebuilds the merged
    /// time points and re-maps every series onto the new index space. Resets the series'
    /// per-point colors (the host re-installs them against the new rows afterwards).
    pub fn set_data(
        &mut self,
        id: SeriesId,
        times: Vec<i64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
    ) {
        debug_assert!(
            times.windows(2).all(|w| w[0] < w[1]),
            "series times must be ascending unique"
        );
        let s = &mut self.series[id];
        s.times = times;
        s.values = [open, high, low, close];
        s.point_colors = [vec![], vec![], vec![]];
        self.rebuild_merged();
        self.reindex_all();
    }

    /// Install the series' per-row color channels (reference data-item colors). Each channel is
    /// `None`/empty for absent, or must match the series' row count exactly; a length mismatch
    /// rejects the whole call (false, no partial state). Within a channel, a `0` entry means
    /// "no override at this row" ([`POINT_COLOR_ABSENT`]).
    pub fn set_point_colors(
        &mut self,
        id: SeriesId,
        channels: [Option<Vec<u32>>; POINT_COLOR_CHANNELS],
    ) -> bool {
        let rows = self.series[id].times.len();
        if channels
            .iter()
            .flatten()
            .any(|channel| !channel.is_empty() && channel.len() != rows)
        {
            return false;
        }
        let s = &mut self.series[id];
        for (slot, channel) in s.point_colors.iter_mut().zip(channels) {
            *slot = channel.unwrap_or_default();
        }
        true
    }

    /// The per-point color override at `row` for `channel`, or `None` when the channel is
    /// absent or the row carries [`POINT_COLOR_ABSENT`]. Plot rows mirror raw rows, so a plot
    /// row offset indexes here directly.
    pub fn point_color(&self, id: SeriesId, channel: PointColorChannel, row: usize) -> Option<u32> {
        let s = self.series.get(id)?;
        let channel = &s.point_colors[channel as usize];
        if channel.is_empty() {
            return None;
        }
        channel
            .get(row)
            .copied()
            .filter(|&c| c != POINT_COLOR_ABSENT)
    }

    /// Whether the series has any per-point color channel installed.
    pub fn has_point_colors(&self, id: SeriesId) -> bool {
        self.series
            .get(id)
            .is_some_and(|s| s.point_colors.iter().any(|c| !c.is_empty()))
    }

    /// Streaming update of a series' last point: replaces the last bar or appends a new one.
    /// The fast path (append at a new global max time, or replace an existing point) avoids a
    /// full rebuild.
    pub fn update(&mut self, id: SeriesId, time: i64, values: [f64; 4]) {
        self.update_styled(id, time, values, [None; POINT_COLOR_CHANNELS]);
    }

    /// [`update`] plus the target bar's per-point colors (reference `series.update` with data-item
    /// colors; `None` = no custom color for that channel). The color channels stay aligned
    /// with the rows in every path: appended rows push, a replaced last bar takes the new
    /// channels (a plain `update` clears that bar's overrides, matching the reference's whole-bar
    /// replacement), and a mid-history insert splices.
    pub fn update_styled(
        &mut self,
        id: SeriesId,
        time: i64,
        values: [f64; 4],
        colors: [Option<u32>; POINT_COLOR_CHANNELS],
    ) {
        let last_merged = self.merged_times.last().copied();

        // Case 1: brand-new global max time — appended at the end, no indices shift.
        if last_merged.is_none_or(|last| time > last) {
            let new_index = self.merged_times.len() as TimePointIndex;
            self.merged_times.push(time);
            let s = &mut self.series[id];
            push_raw(s, time, values, colors);
            s.plot.upsert_last(new_index, values);
            return;
        }

        // Case 2: an existing merged time, at or after this series' own last point — a
        // replace-last or append-at-series-end that maps to a non-decreasing plot index.
        let series_last = self.series[id].times.last().copied();
        let existing = self.merged_times.binary_search(&time).ok();
        if let (Some(pos), true) = (existing, series_last.is_none_or(|lt| time >= lt)) {
            let s = &mut self.series[id];
            if series_last == Some(time) {
                let row = s.times.len() - 1;
                for (col, v) in s.values.iter_mut().zip(values) {
                    col[row] = v;
                }
                for (channel, color) in s.point_colors.iter_mut().zip(colors) {
                    if !channel.is_empty() {
                        channel[row] = color.unwrap_or(POINT_COLOR_ABSENT);
                    }
                }
            } else {
                push_raw(s, time, values, colors);
            }
            s.plot.upsert_last(pos as TimePointIndex, values);
            return;
        }

        // Case 3: insert into the middle of this series (and possibly the merged set) — rebuild.
        let s = &mut self.series[id];
        let insert = lower_bound(&s.times, |&t| t < time);
        if s.times.get(insert) == Some(&time) {
            for (col, v) in s.values.iter_mut().zip(values) {
                col[insert] = v;
            }
            for (channel, color) in s.point_colors.iter_mut().zip(colors) {
                if !channel.is_empty() {
                    channel[insert] = color.unwrap_or(POINT_COLOR_ABSENT);
                }
            }
        } else {
            s.times.insert(insert, time);
            for (col, v) in s.values.iter_mut().zip(values) {
                col.insert(insert, v);
            }
            for (channel, color) in s.point_colors.iter_mut().zip(colors) {
                if !channel.is_empty() {
                    channel.insert(insert, color.unwrap_or(POINT_COLOR_ABSENT));
                }
            }
        }
        self.rebuild_merged();
        self.reindex_all();
    }

    /// Remove the last `count` rows of a series (reference `popSeriesData`, data-layer.ts:338-383):
    /// `count` 0 is a no-op, larger counts clamp to the row count. Per-point color channels
    /// truncate in lockstep with their rows, and the merged time points are rebuilt so times
    /// no series occupies anymore leave the shared axis. Returns the new row count.
    pub fn pop(&mut self, id: SeriesId, count: usize) -> usize {
        let keep = self.series[id].times.len().saturating_sub(count);
        let s = &mut self.series[id];
        if keep == s.times.len() {
            return keep;
        }
        s.times.truncate(keep);
        for col in &mut s.values {
            col.truncate(keep);
        }
        for channel in &mut s.point_colors {
            if !channel.is_empty() {
                channel.truncate(keep);
            }
        }
        self.rebuild_merged();
        self.reindex_all();
        keep
    }

    fn rebuild_merged(&mut self) {
        let total: usize = self.series.iter().map(|s| s.times.len()).sum();
        let mut all = Vec::with_capacity(total);
        for s in &self.series {
            all.extend_from_slice(&s.times);
        }
        all.sort_unstable();
        all.dedup();
        self.merged_times = all;
    }

    fn reindex_all(&mut self) {
        // borrow merged_times immutably while mutating each series' plot
        let merged = &self.merged_times;
        for s in &mut self.series {
            if s.times.is_empty() {
                s.plot.set_data(vec![], vec![], vec![], vec![], vec![]);
                continue;
            }
            let indices: Vec<TimePointIndex> = s
                .times
                .iter()
                .map(|t| {
                    // `merged_times` is the union of all series' times, so every series time is
                    // found by construction. Fall back to the insertion point (nearest index)
                    // instead of panicking so an invariant break degrades instead of aborting;
                    // the insertion point also keeps `indices` aligned with the value columns.
                    let index = merged.binary_search(t).unwrap_or_else(|pos| {
                        debug_assert!(false, "series time {t} missing from merged time points");
                        pos.min(merged.len().saturating_sub(1))
                    });
                    index as TimePointIndex
                })
                .collect();
            s.plot.set_data(
                indices,
                s.values[0].clone(),
                s.values[1].clone(),
                s.values[2].clone(),
                s.values[3].clone(),
            );
        }
    }
}

fn push_raw(
    s: &mut RawSeries,
    time: i64,
    values: [f64; 4],
    colors: [Option<u32>; POINT_COLOR_CHANNELS],
) {
    s.times.push(time);
    for (col, v) in s.values.iter_mut().zip(values) {
        col.push(v);
    }
    for (channel, color) in s.point_colors.iter_mut().zip(colors) {
        if !channel.is_empty() {
            channel.push(color.unwrap_or(POINT_COLOR_ABSENT));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::plot_list::{MismatchDirection, PlotValueIndex};

    /// Value of a series at a merged *index* (maps index -> sparse row).
    fn value_at_index(
        dl: &DataLayer,
        id: SeriesId,
        index: TimePointIndex,
        plot: PlotValueIndex,
    ) -> f64 {
        let row = dl
            .plot(id)
            .search(index, MismatchDirection::None)
            .expect("index present");
        dl.plot(id).value_at(row, plot)
    }

    /// Sets a single-value series (all OHLC = the value) at the given times.
    fn set(dl: &mut DataLayer, id: SeriesId, times: &[i64], vals: &[f64]) {
        let col = |f: fn(f64) -> f64| vals.iter().map(|&v| f(v)).collect::<Vec<f64>>();
        dl.set_data(
            id,
            times.to_vec(),
            col(|v| v),
            col(|v| v),
            col(|v| v),
            col(|v| v),
        );
    }

    #[test]
    fn merged_union_and_per_series_indices() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        let b = dl.add_series();
        // A at times 1,2,3 ; B at 2,4 -> merged 1,2,3,4
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        set(&mut dl, b, &[2, 4], &[5.0, 7.0]);

        assert_eq!(dl.merged_times(), &[1, 2, 3, 4]);
        assert_eq!(dl.base_index(), Some(3));

        // A occupies merged indices 0,1,2 ; B occupies 1,3 (whitespace at 0,2)
        assert_eq!(dl.plot(a).indices(), &[0, 1, 2]);
        assert_eq!(dl.plot(b).indices(), &[1, 3]);
        assert!(dl.plot(b).contains(1));
        assert!(!dl.plot(b).contains(0));
        assert!(!dl.plot(b).contains(2));
    }

    #[test]
    fn adding_a_series_reindexes_existing() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[10, 20, 30], &[1.0, 2.0, 3.0]);
        assert_eq!(dl.plot(a).indices(), &[0, 1, 2]);

        // new series introduces earlier + interleaved times -> A's indices shift
        let b = dl.add_series();
        set(&mut dl, b, &[5, 15, 25], &[9.0, 9.0, 9.0]);
        assert_eq!(dl.merged_times(), &[5, 10, 15, 20, 25, 30]);
        assert_eq!(dl.plot(a).indices(), &[1, 3, 5]);
        assert_eq!(dl.plot(b).indices(), &[0, 2, 4]);
    }

    #[test]
    fn update_appends_new_max_without_rebuild() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        dl.update(a, 4, [40.0, 41.0, 39.0, 40.0]);
        assert_eq!(dl.merged_times(), &[1, 2, 3, 4]);
        assert_eq!(dl.plot(a).indices(), &[0, 1, 2, 3]);
        assert_eq!(value_at_index(&dl, a, 3, PlotValueIndex::Close), 40.0);
    }

    #[test]
    fn update_replaces_last_point() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        dl.update(a, 3, [33.0, 35.0, 31.0, 34.0]);
        assert_eq!(dl.merged_times(), &[1, 2, 3]); // no new point
        assert_eq!(value_at_index(&dl, a, 2, PlotValueIndex::High), 35.0);
    }

    #[test]
    fn update_into_other_series_gap_uses_existing_index() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        let b = dl.add_series();
        set(&mut dl, a, &[1, 2, 3, 4], &[1.0, 2.0, 3.0, 4.0]);
        set(&mut dl, b, &[1, 4], &[9.0, 9.0]); // whitespace at 2,3
                                               // B gets a point at time 3 (an existing merged time, index 2)
        dl.update(b, 3, [7.0, 7.0, 7.0, 7.0]);
        assert_eq!(dl.merged_times(), &[1, 2, 3, 4]);
        assert!(dl.plot(b).contains(2)); // time 3 -> merged index 2
        assert_eq!(value_at_index(&dl, b, 2, PlotValueIndex::Close), 7.0);
    }

    #[test]
    fn empty_layer() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        assert_eq!(dl.merged_times(), &[] as &[i64]);
        assert_eq!(dl.base_index(), None);
        assert!(dl.plot(a).is_empty());
    }

    #[test]
    fn point_colors_validate_lengths_and_clear() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        assert!(!dl.has_point_colors(a));

        // A channel longer than the row count rejects the whole call (no partial state).
        assert!(!dl.set_point_colors(a, [Some(vec![7, 8]), None, None]));
        assert!(!dl.has_point_colors(a));

        assert!(dl.set_point_colors(a, [Some(vec![11, 0, 33]), Some(vec![1, 2, 3]), None]));
        assert!(dl.has_point_colors(a));
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 0), Some(11));
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 1), None); // 0 = absent
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 2), Some(33));
        assert_eq!(dl.point_color(a, PointColorChannel::Wick, 1), Some(2));
        assert_eq!(dl.point_color(a, PointColorChannel::Border, 1), None);

        // None/empty channels clear.
        assert!(dl.set_point_colors(a, [None, Some(vec![]), None]));
        assert!(!dl.has_point_colors(a));
    }

    #[test]
    fn set_data_resets_point_colors() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[1, 2], &[10.0, 20.0]);
        assert!(dl.set_point_colors(a, [Some(vec![5, 6]), None, None]));
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        assert!(!dl.has_point_colors(a));
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 0), None);
    }

    #[test]
    fn update_keeps_point_colors_aligned() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        assert!(dl.set_point_colors(a, [Some(vec![11, 22, 33]), None, None]));

        // Append with a styled update: the new row carries its override.
        dl.update_styled(a, 4, [40.0, 41.0, 39.0, 40.0], [Some(44), None, None]);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 3), Some(44));

        // Append with a plain update: the new row has no override, the channel stays aligned.
        dl.update(a, 5, [50.0, 51.0, 49.0, 50.0]);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 4), None);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 3), Some(44));

        // Replace-last with a plain update clears that bar's override (reference whole-bar
        // replacement); a styled replace sets it.
        dl.update(a, 5, [55.0, 56.0, 54.0, 55.0]);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 4), None);
        dl.update_styled(a, 5, [55.0, 56.0, 54.0, 55.0], [Some(55), None, None]);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 4), Some(55));
    }

    #[test]
    fn pop_truncates_rows_colors_and_merged_times() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        let b = dl.add_series();
        set(&mut dl, a, &[1, 2, 3, 4], &[10.0, 20.0, 30.0, 40.0]);
        set(&mut dl, b, &[3, 4], &[90.0, 95.0]);
        assert!(dl.set_point_colors(a, [Some(vec![11, 22, 33, 44]), None, None]));

        // Clamp to the row count; colors shift along with their rows (reference popSeriesData).
        assert_eq!(dl.pop(a, 10), 0);
        assert_eq!(dl.plot(a).indices(), &[] as &[i64]);
        // A's times left the merged axis; B's remain.
        assert_eq!(dl.merged_times(), &[3, 4]);
        assert!(!dl.has_point_colors(a));

        set(&mut dl, a, &[1, 2, 3, 4], &[10.0, 20.0, 30.0, 40.0]);
        assert!(dl.set_point_colors(a, [Some(vec![11, 22, 33, 44]), None, None]));
        assert_eq!(dl.pop(a, 0), 4); // count 0 is a no-op
        assert_eq!(dl.pop(a, 2), 2);
        assert_eq!(dl.plot(a).indices(), &[0, 1]);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 0), Some(11));
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 1), Some(22));
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 2), None);
        // shared times 3,4 survive through B
        assert_eq!(dl.merged_times(), &[1, 2, 3, 4]);
        assert_eq!(dl.pop(a, 5), 0);
        assert_eq!(dl.merged_times(), &[3, 4]);
    }

    #[test]
    fn rows_count_as_data_series_anchor_the_base_index() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        let nan = f64::NAN;
        // A custom series (Phase C-c): time-only (whitespace-style) rows whose values live
        // host-side. Like the reference's custom plot rows (which carry values), they count as data for
        // the base index once flagged.
        dl.set_data(
            a,
            vec![1, 2, 3],
            vec![nan, nan, nan],
            vec![nan, nan, nan],
            vec![nan, nan, nan],
            vec![nan, nan, nan],
        );
        // Unflagged, an all-whitespace series anchors on 0 (the reference's initialized baseIndex).
        assert_eq!(dl.base_index(), Some(0));
        dl.set_rows_count_as_data(a, true);
        assert_eq!(dl.base_index(), Some(2));
        // A second ordinary series' real bars still win by time, and clearing the flag
        // restores whitespace semantics.
        let b = dl.add_series();
        set(&mut dl, b, &[1], &[7.0]);
        assert_eq!(dl.base_index(), Some(2));
        dl.set_rows_count_as_data(a, false);
        assert_eq!(dl.base_index(), Some(0));
    }

    #[test]
    fn whitespace_rows_stay_in_place_and_off_the_base_index() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        let nan = f64::NAN;
        // bars at 1,2,4 and explicit whitespace rows at 3,5 (reference `{time}`-only items)
        dl.set_data(
            a,
            vec![1, 2, 3, 4, 5],
            vec![10.0, 20.0, nan, 40.0, nan],
            vec![10.0, 20.0, nan, 40.0, nan],
            vec![10.0, 20.0, nan, 40.0, nan],
            vec![10.0, 20.0, nan, 40.0, nan],
        );
        // the whitespace times occupy merged slots (reference keeps the time-scale points)
        assert_eq!(dl.merged_times(), &[1, 2, 3, 4, 5]);
        assert_eq!(dl.plot(a).indices(), &[0, 1, 2, 3, 4]);
        assert!(dl.plot(a).is_whitespace_row(2));
        assert!(dl.plot(a).is_whitespace_row(4));
        // base index = the last point with real data (reference _getBaseIndex), not the trailing ws
        assert_eq!(dl.base_index(), Some(3));
    }

    #[test]
    fn whitespace_update_replaces_and_appends() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        let nan = f64::NAN;
        set(&mut dl, a, &[1, 2, 3], &[10.0, 20.0, 30.0]);
        // reference `series.update` with a `{time}`-only item replaces the last bar with whitespace.
        dl.update(a, 3, [nan; 4]);
        assert!(dl.plot(a).is_whitespace_row(2));
        assert_eq!(dl.base_index(), Some(1));
        // a whitespace append creates a time point but does not move the base index
        dl.update(a, 4, [nan; 4]);
        assert_eq!(dl.merged_times(), &[1, 2, 3, 4]);
        assert_eq!(dl.base_index(), Some(1));
        // ...and a real bar at that whitespace time moves it again (the gated reference shift case)
        dl.update(a, 4, [40.0, 41.0, 39.0, 40.0]);
        assert!(!dl.plot(a).is_whitespace_row(3));
        assert_eq!(dl.base_index(), Some(3));
    }

    #[test]
    fn mid_history_insert_splices_point_colors() {
        let mut dl = DataLayer::new();
        let a = dl.add_series();
        set(&mut dl, a, &[1, 2, 4], &[10.0, 20.0, 40.0]);
        assert!(dl.set_point_colors(a, [Some(vec![11, 22, 44]), None, None]));

        // Insert a new bar at time 3 (mid-history rebuild): colors shift with the rows.
        dl.update_styled(a, 3, [30.0, 31.0, 29.0, 30.0], [Some(33), None, None]);
        assert_eq!(
            (0..4)
                .map(|row| dl.point_color(a, PointColorChannel::Body, row))
                .collect::<Vec<_>>(),
            vec![Some(11), Some(22), Some(33), Some(44)]
        );

        // Overwrite a mid-history bar with a plain update: only its override clears.
        dl.update(a, 2, [20.0, 21.0, 19.0, 20.0]);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 1), None);
        assert_eq!(dl.point_color(a, PointColorChannel::Body, 2), Some(33));
    }
}
