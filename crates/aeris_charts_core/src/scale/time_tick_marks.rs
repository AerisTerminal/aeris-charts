//! Time tick-mark weights and mark selection.
//! Ports of `src/model/horz-scale-behavior-time/time-scale-point-weight-generator.ts`
//! and `src/model/tick-marks.ts`.
//!
//! Weights are assigned per point by comparing consecutive UTC timestamps: the largest
//! calendar/time boundary crossed between neighbors determines the weight. Mark selection
//! keeps higher weights first and inserts lower-weight marks only where they fit.

use std::collections::{BTreeMap, BTreeSet};

use crate::TimePointIndex;
use crate::time_zone::ChartTimeZone;

/// Exact values from the reference's `TickMarkWeight` (`horz-scale-behavior-time/types.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum TickMarkWeight {
    LessThanSecond = 0,
    Second = 10,
    Minute1 = 20,
    Minute5 = 21,
    Minute30 = 22,
    Hour1 = 30,
    Hour3 = 31,
    Hour6 = 32,
    Hour12 = 33,
    Day = 50,
    Month = 60,
    Year = 70,
}

/// (year, month 1-12, day 1-31) from days since the Unix epoch.
/// Howard Hinnant's `civil_from_days` — exact for the proleptic Gregorian calendar,
/// matching JS `Date` UTC accessors.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Days since the Unix epoch for a proleptic-Gregorian civil date. Returns `None` for an invalid
/// month/day or arithmetic overflow. This is the inverse of [`civil_from_timestamp`] at UTC
/// midnight and keeps calendar-aligned temporal axes independent of platform date libraries.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let leap = year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month {
        return None;
    }

    let adjusted_year = year.checked_sub(i64::from(month <= 2))?;
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

/// (year, month 1-12, day 1-31) of a UTC timestamp in seconds.
pub fn civil_from_timestamp(ts: i64) -> (i64, u32, u32) {
    civil_from_days(ts.div_euclid(86_400))
}

/// Intraday boundary divisors in seconds, smallest to largest (reference iterates largest first).
const INTRADAY_DIVISORS: [(i64, TickMarkWeight); 8] = [
    (1, TickMarkWeight::Second),
    (60, TickMarkWeight::Minute1),
    (300, TickMarkWeight::Minute5),
    (1800, TickMarkWeight::Minute30),
    (3600, TickMarkWeight::Hour1),
    (10_800, TickMarkWeight::Hour3),
    (21_600, TickMarkWeight::Hour6),
    (43_200, TickMarkWeight::Hour12),
];

/// Port of `weightByTime`: weight of `current` given the previous point's timestamp.
pub fn weight_by_time(current_ts: i64, prev_ts: i64) -> TickMarkWeight {
    weight_by_time_in_time_zone(current_ts, prev_ts, ChartTimeZone::default())
}

/// Tick weight using the selected chart display time zone. Canonical inputs stay in UTC; only
/// the civil/calendar boundary classification is localized.
pub fn weight_by_time_in_time_zone(
    current_ts: i64,
    prev_ts: i64,
    time_zone: ChartTimeZone,
) -> TickMarkWeight {
    let (cy, cm, cd, py, pm, pd) = match (
        time_zone.local_parts(current_ts),
        time_zone.local_parts(prev_ts),
    ) {
        (Some(current), Some(previous)) => (
            i64::from(current.year),
            current.month,
            current.day,
            i64::from(previous.year),
            previous.month,
            previous.day,
        ),
        _ => {
            let (cy, cm, cd) = civil_from_timestamp(current_ts);
            let (py, pm, pd) = civil_from_timestamp(prev_ts);
            (cy, cm, cd, py, pm, pd)
        }
    };

    if cy != py {
        return TickMarkWeight::Year;
    } else if cm != pm {
        return TickMarkWeight::Month;
    } else if cd != pd {
        return TickMarkWeight::Day;
    }

    let current_local = time_zone.local_epoch_seconds(current_ts);
    let previous_local = time_zone.local_epoch_seconds(prev_ts);
    for &(divisor, weight) in INTRADAY_DIVISORS.iter().rev() {
        if previous_local.div_euclid(divisor) != current_local.div_euclid(divisor) {
            return weight;
        }
    }

    TickMarkWeight::LessThanSecond
}

/// Port of `fillWeightsForPoints`. `times` are UTC timestamps in seconds; writes
/// `weights[start_index..]`. The first point's weight is guessed by extrapolating the
/// average time diff backwards.
pub fn fill_weights_for_points(times: &[i64], weights: &mut [u8], start_index: usize) {
    fill_weights_for_points_in_time_zone(times, weights, start_index, ChartTimeZone::default());
}

pub fn fill_weights_for_points_in_time_zone(
    times: &[i64],
    weights: &mut [u8],
    start_index: usize,
    time_zone: ChartTimeZone,
) {
    debug_assert_eq!(times.len(), weights.len());
    if times.is_empty() {
        return;
    }

    let mut prev_time: Option<i64> = if start_index == 0 {
        None
    } else {
        Some(times[start_index - 1])
    };
    for index in start_index..times.len() {
        let current = times[index];
        if let Some(prev) = prev_time {
            weights[index] = weight_by_time_in_time_zone(current, prev, time_zone) as u8;
        }
        prev_time = Some(current);
    }

    if start_index == 0 && times.len() > 1 {
        weights[0] =
            inferred_first_weight(times[0], *times.last().unwrap(), times.len(), time_zone);
    }
}

/// The accumulated adjacent gaps telescope to `last - first`; no interior timestamps are
/// needed to revise the first mark when the cadence changes. One point retains weight zero.
pub fn inferred_first_weight(first: i64, last: i64, len: usize, time_zone: ChartTimeZone) -> u8 {
    if len <= 1 {
        return 0;
    }
    let average_time_diff = (((last - first) as f64) / (len as f64 - 1.0)).ceil() as i64;
    weight_by_time_in_time_zone(first, first - average_time_diff, time_zone) as u8
}

/// A selectable tick mark: time-point index + weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickMark {
    pub index: TimePointIndex,
    pub weight: u8,
}

/// Port of the `TickMarks` container: marks grouped by weight, selection by available space.
#[derive(Default)]
pub struct TimeTickMarks {
    marks_by_weight: BTreeMap<u8, Vec<TimePointIndex>>,
    // Separate from potentially million-entry buckets: changing the inferred first weight
    // never removes/shifts an index in a bucket.
    first_mark: Option<TickMark>,
    cache: Option<(i64, Vec<TickMark>)>,
}

impl TimeTickMarks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Estimated live vector payload retained by tick selection. Map/node allocator overhead is
    /// intentionally excluded; benchmark reports label this as structure payload, not heap truth.
    pub fn payload_bytes(&self) -> usize {
        let buckets = self
            .marks_by_weight
            .values()
            .map(|indices| indices.len() * std::mem::size_of::<TimePointIndex>())
            .sum::<usize>();
        let cache = self.cache.as_ref().map_or(0, |(_, marks)| {
            marks.len() * std::mem::size_of::<TickMark>()
        });
        buckets
            + cache
            + self
                .first_mark
                .map_or(0, |_| std::mem::size_of::<TickMark>())
    }

    pub fn capacity_bytes(&self) -> usize {
        let buckets = self
            .marks_by_weight
            .values()
            .map(|indices| indices.capacity() * std::mem::size_of::<TimePointIndex>())
            .sum::<usize>();
        let cache = self.cache.as_ref().map_or(0, |(_, marks)| {
            marks.capacity() * std::mem::size_of::<TickMark>()
        });
        buckets
            + cache
            + self
                .first_mark
                .map_or(0, |_| std::mem::size_of::<TickMark>())
    }

    /// Full rebuild from per-point weights (incremental `firstChangedPointIndex` variant
    /// comes with the data layer).
    pub fn set_weights(&mut self, weights: &[u8]) {
        self.set_weights_from(0, weights);
    }

    /// Full rebuild from per-point weights whose first logical index may be negative.
    pub fn set_weights_from(&mut self, start_index: TimePointIndex, weights: &[u8]) {
        self.marks_by_weight.clear();
        self.first_mark = None;
        self.cache = None;
        for (index, &weight) in weights.iter().enumerate() {
            let Ok(index) = TimePointIndex::try_from(index) else {
                break;
            };
            let Some(index) = start_index.checked_add(index) else {
                break;
            };
            if self.first_mark.is_none() {
                self.first_mark = Some(TickMark { index, weight });
            } else {
                self.marks_by_weight.entry(weight).or_default().push(index);
            }
        }
    }

    /// Revise the first mark without touching any weight bucket.
    pub fn set_first_weight(&mut self, weight: u8) {
        if let Some(first) = self.first_mark.as_mut()
            && first.weight != weight
        {
            first.weight = weight;
            self.cache = None;
        }
    }

    /// Append weights for newly-added points without rebuilding prior weight buckets.
    pub fn append_weights(&mut self, start_index: usize, weights: &[u8]) {
        self.cache = None;
        for (offset, &weight) in weights.iter().enumerate().skip(start_index) {
            self.push_weight(offset as TimePointIndex, weight);
        }
    }

    /// Streaming hot path: append a single point's weight without any O(n) scratch vector.
    /// Matches `fill_weights_for_points` semantics for one appended point with a known
    /// predecessor.
    pub fn push_weight(&mut self, index: TimePointIndex, weight: u8) {
        self.cache = None;
        if self.first_mark.is_none() {
            self.first_mark = Some(TickMark { index, weight });
        } else {
            self.marks_by_weight.entry(weight).or_default().push(index);
        }
    }

    /// Port of `TickMarks.build`: `max_width` is the max label width in px
    /// (`(font_size + 4) * 5 / 8 * max_label_chars`), `spacing` the current bar spacing.
    pub fn build(&mut self, spacing: f64, max_width: f64) -> &[TickMark] {
        let max_indexes_per_mark = (max_width / spacing).ceil() as i64;
        if self
            .cache
            .as_ref()
            .is_none_or(|(cached, _)| *cached != max_indexes_per_mark)
        {
            let marks = self.build_impl(max_indexes_per_mark);
            self.cache = Some((max_indexes_per_mark, marks));
        }
        match self.cache.as_ref() {
            Some((_, marks)) => marks,
            None => &[],
        }
    }

    fn build_impl(&self, max_indexes_per_mark: i64) -> Vec<TickMark> {
        let mut marks: Vec<TickMark> = Vec::new();

        // There are at most the fixed calendar weight classes here. Include the standalone
        // first mark in its normal weight pass so spacing and priority remain unchanged.
        let weights = self
            .marks_by_weight
            .keys()
            .copied()
            .chain(self.first_mark.map(|mark| mark.weight))
            .collect::<BTreeSet<_>>();
        for weight in weights.into_iter().rev() {
            let current_weight_marks = self.marks_by_weight.get(&weight);
            // built marks so far become prev_marks; marks restarts
            let prev_marks = marks;
            marks = Vec::with_capacity(
                prev_marks.len()
                    + current_weight_marks.map_or(0, Vec::len)
                    + usize::from(self.first_mark.is_some_and(|mark| mark.weight == weight)),
            );

            let mut prev_marks_pointer = 0usize;
            let mut right_index = i64::MAX;
            let mut left_index = i64::MIN;

            let first_index = self
                .first_mark
                .filter(|mark| mark.weight == weight)
                .map(|mark| mark.index);
            for current_index in first_index
                .into_iter()
                .chain(current_weight_marks.into_iter().flatten().copied())
            {
                // move all prev marks strictly left of current into the result
                while prev_marks_pointer < prev_marks.len() {
                    let last_mark = prev_marks[prev_marks_pointer];
                    if last_mark.index < current_index {
                        prev_marks_pointer += 1;
                        marks.push(last_mark);
                        left_index = last_mark.index;
                        right_index = i64::MAX;
                    } else {
                        right_index = last_mark.index;
                        break;
                    }
                }

                // saturating: sentinels are i64::MAX/MIN (reference uses ±Infinity)
                if right_index.saturating_sub(current_index) >= max_indexes_per_mark
                    && current_index.saturating_sub(left_index) >= max_indexes_per_mark
                {
                    marks.push(TickMark {
                        index: current_index,
                        weight,
                    });
                    left_index = current_index;
                }
            }

            // append the unused prev marks
            for &m in &prev_marks[prev_marks_pointer..] {
                marks.push(m);
            }
        }

        marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_conversion() {
        // 2018-06-25T04:00:00Z (the reference's doc example timestamp)
        assert_eq!(civil_from_timestamp(1_529_899_200), (2018, 6, 25));
        // epoch
        assert_eq!(civil_from_timestamp(0), (1970, 1, 1));
        // leap day
        assert_eq!(civil_from_timestamp(1_582_934_400), (2020, 2, 29));
        // pre-epoch
        assert_eq!(civil_from_timestamp(-86_400), (1969, 12, 31));
        for &(year, month, day) in &[
            (1970, 1, 1),
            (1969, 12, 31),
            (2020, 2, 29),
            (-400, 3, 1),
            (285_000, 12, 31),
        ] {
            let days = days_from_civil(year, month, day).unwrap();
            assert_eq!(civil_from_timestamp(days * 86_400), (year, month, day));
        }
        assert_eq!(days_from_civil(2021, 2, 29), None);
    }

    #[test]
    fn selected_time_zone_controls_calendar_tick_boundaries() {
        let new_york = ChartTimeZone::parse("America/New_York").unwrap();
        // 2026-01-02 04:59 -> 05:00 UTC is 2026-01-01 23:59 -> 2026-01-02 00:00 in New York.
        assert_eq!(
            weight_by_time_in_time_zone(1_767_330_000, 1_767_329_940, new_york),
            TickMarkWeight::Day
        );
        assert_eq!(
            weight_by_time(1_767_330_000, 1_767_329_940),
            TickMarkWeight::Hour1
        );
    }

    #[test]
    fn weights_by_boundary() {
        let day = 86_400;
        // year boundary: 2019-12-31 -> 2020-01-01
        let y2020 = 1_577_836_800; // 2020-01-01T00:00:00Z
        assert_eq!(weight_by_time(y2020, y2020 - day), TickMarkWeight::Year);
        // month boundary: 2020-01-31 -> 2020-02-01
        let feb1 = 1_580_515_200;
        assert_eq!(weight_by_time(feb1, feb1 - day), TickMarkWeight::Month);
        // plain day boundary
        let jan15 = 1_579_046_400; // 2020-01-15
        assert_eq!(weight_by_time(jan15, jan15 - day), TickMarkWeight::Day);
        // intraday: crossing 12h boundary
        assert_eq!(
            weight_by_time(jan15 + 43_200, jan15 + 43_100),
            TickMarkWeight::Hour12
        );
        // crossing 1h but not 3h
        assert_eq!(
            weight_by_time(jan15 + 3600, jan15 + 3599),
            TickMarkWeight::Hour1
        );
        // crossing 1min but not 5min
        assert_eq!(
            weight_by_time(jan15 + 60, jan15 + 59),
            TickMarkWeight::Minute1
        );
        // same second
        assert_eq!(weight_by_time(jan15, jan15), TickMarkWeight::LessThanSecond);
    }

    #[test]
    fn fill_weights_guesses_first_point() {
        // hourly bars starting mid-day
        let times: Vec<i64> = (0..48).map(|i| 1_579_046_400 + i * 3600).collect();
        let mut weights = vec![0u8; times.len()];
        fill_weights_for_points(&times, &mut weights, 0);

        // first point: avg diff 3600 back -> crosses an hour boundary at minimum
        assert!(weights[0] >= TickMarkWeight::Hour1 as u8);
        // index 24 is the next midnight -> Day weight
        assert_eq!(weights[24], TickMarkWeight::Day as u8);
        // other intraday points are hour-weighted
        assert_eq!(weights[1], TickMarkWeight::Hour1 as u8);
        assert_eq!(weights[12], TickMarkWeight::Hour12 as u8);
    }

    #[test]
    fn first_weight_uses_only_endpoints_count_and_time_zone() {
        let irregular = [86_280, 86_340, 86_400, 86_460, 172_800];
        let mut weights = [0; 5];
        fill_weights_for_points(&irregular, &mut weights, 0);
        assert_eq!(weights, [32, 20, 50, 20, 50]);
        assert_eq!(
            weights[0],
            inferred_first_weight(
                irregular[0],
                irregular[4],
                irregular.len(),
                ChartTimeZone::default()
            )
        );
        assert_eq!(
            inferred_first_weight(86_280, 86_280, 1, ChartTimeZone::default()),
            0
        );
        let zone = ChartTimeZone::parse("America/New_York").unwrap();
        fill_weights_for_points_in_time_zone(&irregular, &mut weights, 0, zone);
        assert_eq!(
            weights[0],
            inferred_first_weight(irregular[0], irregular[4], irregular.len(), zone)
        );
    }

    #[test]
    fn reclassifying_first_mark_does_not_move_large_bucket() {
        let mut marks = TimeTickMarks::new();
        marks.set_weights(&vec![20; 100_000]);
        let bucket = marks.marks_by_weight.get(&20).unwrap();
        let original_ptr = bucket.as_ptr();
        let original_capacity = bucket.capacity();
        marks.build(100.0, 0.0);
        marks.set_first_weight(32);
        let bucket = marks.marks_by_weight.get(&20).unwrap();
        assert_eq!(bucket.as_ptr(), original_ptr);
        assert_eq!(bucket.capacity(), original_capacity);
        assert_eq!(bucket.len(), 99_999);
        let selected = marks.build(100.0, 0.0);
        assert_eq!(
            selected.first(),
            Some(&TickMark {
                index: 0,
                weight: 32
            })
        );
        assert_eq!(selected.len(), 100_000);
        marks.push_weight(100_000, 50);
        assert_eq!(
            marks.build(100.0, 0.0).last(),
            Some(&TickMark {
                index: 100_000,
                weight: 50
            })
        );
    }

    #[test]
    fn build_keeps_high_weights_and_spacing() {
        // 100 daily points; every 10th is Month weight, rest Day
        let mut weights = vec![TickMarkWeight::Day as u8; 100];
        for i in (0..100).step_by(10) {
            weights[i] = TickMarkWeight::Month as u8;
        }
        let mut tm = TimeTickMarks::new();
        tm.set_weights(&weights);

        // plenty of space: max_indexes_per_mark = ceil(80/40) = 2 -> months + days that fit
        let marks = tm.build(40.0, 80.0).to_vec();
        assert!(!marks.is_empty());
        // all month marks must be present
        let month_count = marks
            .iter()
            .filter(|m| m.weight == TickMarkWeight::Month as u8)
            .count();
        assert_eq!(month_count, 10);
        // result sorted by index
        assert!(marks.windows(2).all(|w| w[0].index < w[1].index));
        // no two marks closer than max_indexes_per_mark... except between two high-weight marks
        // (higher weights always win); day marks must respect spacing vs neighbors
        for w in marks.windows(2) {
            if w[0].weight != w[1].weight {
                assert!((w[1].index - w[0].index) >= 2, "{:?}", w);
            }
        }

        // tight space: only high-weight marks survive
        let tight = tm.build(4.0, 80.0).to_vec(); // max_indexes_per_mark = 20
        assert!(
            tight
                .iter()
                .all(|m| m.weight == TickMarkWeight::Month as u8)
        );
        // and they respect the 20-index spacing (every other month mark dropped)
        assert!(tight.windows(2).all(|w| w[1].index - w[0].index >= 20));
    }

    #[test]
    fn build_cache_invalidates_on_spacing_change() {
        let weights = vec![TickMarkWeight::Day as u8; 50];
        let mut tm = TimeTickMarks::new();
        tm.set_weights(&weights);
        let wide = tm.build(80.0, 80.0).len(); // 1 index per mark -> all fit
        let narrow = tm.build(2.0, 80.0).len(); // 40 indexes per mark -> few fit
        assert!(wide > narrow);
    }

    #[test]
    fn append_weights_keeps_existing_marks_and_adds_new_point() {
        let mut marks = TimeTickMarks::new();
        marks.set_weights(&[50]);
        marks.append_weights(1, &[50, 50]);
        let built = marks.build(10.0, 10.0);
        assert!(built.iter().any(|mark| mark.index == 0));
        assert!(built.iter().any(|mark| mark.index == 1));
    }

    #[test]
    fn full_rebuild_preserves_negative_projected_indices() {
        let mut marks = TimeTickMarks::new();
        marks.set_weights_from(-2, &[70, 60, 50, 40]);
        let built = marks.build(100.0, 1.0);
        assert!(built.iter().any(|mark| mark.index == -2));
        assert!(built.iter().any(|mark| mark.index == -1));
        assert!(built.iter().any(|mark| mark.index == 0));
        assert!(built.iter().any(|mark| mark.index == 1));
    }
}
