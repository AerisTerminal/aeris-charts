//! Time tick-mark weights and mark selection.
//! Ports of `src/model/horz-scale-behavior-time/time-scale-point-weight-generator.ts`
//! and `src/model/tick-marks.ts` (RENDERING_SPEC.md §11).
//!
//! Weights are assigned per point by comparing consecutive UTC timestamps: the largest
//! calendar/time boundary crossed between neighbors determines the weight. Mark selection
//! keeps higher weights first and inserts lower-weight marks only where they fit.

use std::collections::BTreeMap;

use crate::TimePointIndex;

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
    let (cy, cm, cd) = civil_from_timestamp(current_ts);
    let (py, pm, pd) = civil_from_timestamp(prev_ts);

    if cy != py {
        return TickMarkWeight::Year;
    } else if cm != pm {
        return TickMarkWeight::Month;
    } else if cd != pd {
        return TickMarkWeight::Day;
    }

    for &(divisor, weight) in INTRADAY_DIVISORS.iter().rev() {
        if prev_ts.div_euclid(divisor) != current_ts.div_euclid(divisor) {
            return weight;
        }
    }

    TickMarkWeight::LessThanSecond
}

/// Port of `fillWeightsForPoints`. `times` are UTC timestamps in seconds; writes
/// `weights[start_index..]`. The first point's weight is guessed by extrapolating the
/// average time diff backwards.
pub fn fill_weights_for_points(times: &[i64], weights: &mut [u8], start_index: usize) {
    debug_assert_eq!(times.len(), weights.len());
    if times.is_empty() {
        return;
    }

    let mut prev_time: Option<i64> = if start_index == 0 {
        None
    } else {
        Some(times[start_index - 1])
    };
    let mut total_time_diff: i64 = 0;

    for index in start_index..times.len() {
        let current = times[index];
        if let Some(prev) = prev_time {
            weights[index] = weight_by_time(current, prev) as u8;
        }
        total_time_diff += current - prev_time.unwrap_or(current);
        prev_time = Some(current);
    }

    if start_index == 0 && times.len() > 1 {
        // guess a weight for the first point: pretend the previous point was the average
        // time diff back in history
        let average_time_diff =
            ((total_time_diff as f64) / (times.len() as f64 - 1.0)).ceil() as i64;
        let approx_prev = times[0] - average_time_diff;
        weights[0] = weight_by_time(times[0], approx_prev) as u8;
    }
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
    cache: Option<(i64, Vec<TickMark>)>,
}

impl TimeTickMarks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Full rebuild from per-point weights (incremental `firstChangedPointIndex` variant
    /// comes with the data layer).
    pub fn set_weights(&mut self, weights: &[u8]) {
        self.marks_by_weight.clear();
        self.cache = None;
        for (index, &weight) in weights.iter().enumerate() {
            self.marks_by_weight
                .entry(weight)
                .or_default()
                .push(index as TimePointIndex);
        }
    }

    /// Append weights for newly-added points without rebuilding prior weight buckets.
    pub fn append_weights(&mut self, start_index: usize, weights: &[u8]) {
        self.cache = None;
        for (offset, &weight) in weights.iter().enumerate().skip(start_index) {
            self.marks_by_weight
                .entry(weight)
                .or_default()
                .push(offset as TimePointIndex);
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

        for (&weight, current_weight_marks) in self.marks_by_weight.iter().rev() {
            // built marks so far become prev_marks; marks restarts
            let prev_marks = marks;
            marks = Vec::with_capacity(prev_marks.len() + current_weight_marks.len());

            let mut prev_marks_pointer = 0usize;
            let mut right_index = i64::MAX;
            let mut left_index = i64::MIN;

            for &current_index in current_weight_marks {
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
        assert!(tight
            .iter()
            .all(|m| m.weight == TickMarkWeight::Month as u8));
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
}
