//! Price-by-volume distribution from OHLCV bars. Volume is distributed uniformly over
//! each bar's low/high interval; this is an OHLCV estimate, not tick-level order flow.

pub const MAX_VOLUME_PROFILE_ROWS: usize = 512;

#[derive(Clone, Copy, Debug)]
pub struct ProfileBar {
    pub low: f64,
    pub high: f64,
    pub volume: f64,
}

impl ProfileBar {
    fn valid(self) -> bool {
        self.low.is_finite()
            && self.high.is_finite()
            && self.high >= self.low
            && self.volume.is_finite()
            && self.volume > 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileRow {
    pub low: f64,
    pub high: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeProfile {
    pub rows: Vec<ProfileRow>,
    pub total_volume: f64,
    pub bar_count: usize,
    pub poc_index: Option<usize>,
    pub value_area_low_index: Option<usize>,
    pub value_area_high_index: Option<usize>,
}

/// Two passes over the input and O(rows) storage/work beyond those passes. Full-bin
/// contributions use a difference array, avoiding a bars × rows inner loop.
/// POC ties choose the lowest price; value-area expansion chooses the larger adjacent
/// row, choosing the lower row on ties, until it reaches the requested volume fraction.
pub fn volume_profile(
    bars: impl Iterator<Item = ProfileBar> + Clone,
    row_count: usize,
    value_area_percent: f64,
    minimum_span: f64,
) -> Result<VolumeProfile, &'static str> {
    if !(1..=MAX_VOLUME_PROFILE_ROWS).contains(&row_count)
        || !value_area_percent.is_finite()
        || !(0.0..=100.0).contains(&value_area_percent)
        || value_area_percent == 0.0
        || !minimum_span.is_finite()
        || minimum_span <= 0.0
    {
        return Err("invalid volume-profile parameters");
    }
    let mut result = VolumeProfile::default();
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for bar in bars.clone().filter(|bar| bar.valid()) {
        low = low.min(bar.low);
        high = high.max(bar.high);
        result.total_volume += bar.volume;
        result.bar_count += 1;
    }
    if result.bar_count == 0 {
        return Ok(result);
    }
    if !result.total_volume.is_finite() {
        return Err("volume-profile volume overflow");
    }
    if low == high {
        low -= minimum_span * 0.5;
        high += minimum_span * 0.5;
    }
    let step = (high - low) / row_count as f64;
    if !step.is_finite() || step <= 0.0 || low + step == low {
        return Err("volume-profile price range cannot be represented");
    }
    let mut volumes = vec![0.0; row_count];
    let mut differences = vec![0.0; row_count + 1];
    let bin = |price: f64| (((price - low) / step).floor() as usize).min(row_count - 1);
    for bar in bars.filter(|bar| bar.valid()) {
        let first = bin(bar.low);
        let last = bin(bar.high);
        if first == last || bar.low == bar.high {
            volumes[first] += bar.volume;
            continue;
        }
        let span = bar.high - bar.low;
        // Divide the price overlap before multiplying by volume to avoid density overflow.
        volumes[first] += bar.volume * ((low + (first + 1) as f64 * step - bar.low) / span);
        volumes[last] += bar.volume * ((bar.high - (low + last as f64 * step)) / span);
        if last > first + 1 {
            let full = bar.volume * (step / span);
            differences[first + 1] += full;
            differences[last] -= full;
        }
    }
    let mut running = 0.0;
    for (index, volume) in volumes.iter_mut().enumerate() {
        running += differences[index];
        *volume = (*volume + running).max(0.0);
        if !volume.is_finite() {
            return Err("volume-profile bin overflow");
        }
        result.rows.push(ProfileRow {
            low: low + index as f64 * step,
            high: if index + 1 == row_count {
                high
            } else {
                low + (index + 1) as f64 * step
            },
            volume: *volume,
        });
    }
    let mut poc = 0;
    for index in 1..row_count {
        if volumes[index] > volumes[poc] {
            poc = index;
        }
    }
    let (mut lower, mut upper) = (poc, poc);
    let mut included = volumes[poc];
    let target = result.total_volume * (value_area_percent / 100.0);
    while included < target && (lower > 0 || upper + 1 < row_count) {
        if upper + 1 == row_count || (lower > 0 && volumes[lower - 1] >= volumes[upper + 1]) {
            lower -= 1;
            included += volumes[lower];
        } else {
            upper += 1;
            included += volumes[upper];
        }
    }
    result.poc_index = Some(poc);
    result.value_area_low_index = Some(lower);
    result.value_area_high_index = Some(upper);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_overlap_conserves_volume_and_value_area_is_contiguous() {
        let bars = [
            ProfileBar {
                low: 0.0,
                high: 4.0,
                volume: 40.0,
            },
            ProfileBar {
                low: 1.0,
                high: 2.0,
                volume: 30.0,
            },
        ];
        let profile = volume_profile(bars.into_iter(), 4, 70.0, 0.01).unwrap();
        assert_eq!(
            profile
                .rows
                .iter()
                .map(|row| row.volume)
                .collect::<Vec<_>>(),
            [10.0, 40.0, 10.0, 10.0]
        );
        assert_eq!(profile.total_volume, 70.0);
        assert_eq!(profile.poc_index, Some(1));
        assert_eq!(
            (profile.value_area_low_index, profile.value_area_high_index),
            (Some(0), Some(1))
        );
    }

    #[test]
    fn flat_missing_and_invalid_data_never_create_fictitious_volume() {
        let bars = [
            ProfileBar {
                low: 2.0,
                high: 2.0,
                volume: 7.0,
            },
            ProfileBar {
                low: 0.0,
                high: 3.0,
                volume: -1.0,
            },
            ProfileBar {
                low: f64::NAN,
                high: 4.0,
                volume: 5.0,
            },
        ];
        let profile = volume_profile(bars.into_iter(), 8, 100.0, 0.01).unwrap();
        assert_eq!(profile.bar_count, 1);
        assert_eq!(profile.rows.iter().map(|row| row.volume).sum::<f64>(), 7.0);
        assert!(volume_profile(std::iter::empty(), 512, 70.0, 0.01)
            .unwrap()
            .rows
            .is_empty());
        assert!(volume_profile(std::iter::empty(), 513, 70.0, 0.01).is_err());
        assert!(volume_profile(bars.into_iter(), 4, f64::NAN, 0.01).is_err());
    }

    #[test]
    fn overflow_is_reported_and_ties_are_deterministic() {
        let bars = [ProfileBar {
            low: 0.0,
            high: 4.0,
            volume: 40.0,
        }];
        assert_eq!(
            volume_profile(bars.into_iter(), 4, 70.0, 0.01)
                .unwrap()
                .poc_index,
            Some(0)
        );
        let huge = [ProfileBar {
            low: 0.0,
            high: 1.0,
            volume: f64::MAX,
        }; 2];
        assert!(volume_profile(huge.into_iter(), 4, 70.0, 0.01).is_err());
    }
}
