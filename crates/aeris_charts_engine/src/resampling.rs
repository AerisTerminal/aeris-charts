//! Engine-owned OHLCV resampling.
//!
//! The host supplies UTC session/period boundaries. The engine deliberately has no calendar or
//! timezone fallback: a row outside those boundaries is omitted, making policy explicit and
//! deterministic across browser and native executors.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{ChartEngine, SeriesId, SeriesKind};

pub const MAX_RESAMPLE_BOUNDARIES: usize = 20_000;
pub const MAX_RESAMPLED_SERIES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResampleBoundary {
    /// Inclusive UTC timestamp in seconds.
    pub start_time: i64,
    /// Exclusive UTC timestamp in seconds.
    pub end_time: i64,
    /// Opaque host identity shared by periods belonging to the same session.
    pub session_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResampleOptions {
    /// Width of one derived bar. Buckets restart at each supplied boundary.
    pub interval_seconds: u32,
    pub boundaries: Vec<ResampleBoundary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResampledBar {
    pub timestamp: i64,
    pub session_id: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub source_rows: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResampleError {
    UnknownSeries(SeriesId),
    UnsupportedSource(SeriesId),
    UnsupportedTarget(SeriesId),
    InvalidInterval,
    InvalidBoundaries,
    TooManyBoundaries,
    TooManySeries,
    DependencyCycle,
}

impl std::fmt::Display for ResampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSeries(id) => write!(f, "unknown series {id}"),
            Self::UnsupportedSource(id) => write!(f, "series {id} is not an OHLC source"),
            Self::UnsupportedTarget(id) => write!(f, "series {id} is not a compatible target"),
            Self::InvalidInterval => write!(f, "resample interval must be greater than zero"),
            Self::InvalidBoundaries => write!(
                f,
                "resample boundaries must be ordered, disjoint, and non-empty"
            ),
            Self::TooManyBoundaries => write!(f, "resample boundary limit exceeded"),
            Self::TooManySeries => write!(f, "resampled-series limit exceeded"),
            Self::DependencyCycle => {
                write!(f, "resampling dependencies may not be chained or cyclic")
            }
        }
    }
}

impl std::error::Error for ResampleError {}

#[derive(Clone, Debug)]
pub(crate) struct ResampleBinding {
    pub(crate) source: SeriesId,
    pub(crate) volume_source: Option<SeriesId>,
    pub(crate) target: SeriesId,
    pub(crate) volume_target: Option<SeriesId>,
    pub(crate) options: ResampleOptions,
    pub(crate) bars: Vec<ResampledBar>,
}

impl ChartEngine {
    pub fn configure_resampled_series(
        &mut self,
        source: SeriesId,
        volume_source: Option<SeriesId>,
        target: SeriesId,
        volume_target: Option<SeriesId>,
        options: ResampleOptions,
    ) -> Result<(), ResampleError> {
        validate_options(&options)?;
        if self.resampled_series.len() >= MAX_RESAMPLED_SERIES
            && !self.resampled_series.contains_key(&target)
        {
            return Err(ResampleError::TooManySeries);
        }
        let kind = |id| self.series_entry(id).map(|entry| entry.kind);
        if !matches!(
            kind(source),
            Some(SeriesKind::Candlestick | SeriesKind::Bar)
        ) {
            return Err(if kind(source).is_none() {
                ResampleError::UnknownSeries(source)
            } else {
                ResampleError::UnsupportedSource(source)
            });
        }
        if !matches!(
            kind(target),
            Some(SeriesKind::Candlestick | SeriesKind::Bar)
        ) {
            return Err(if kind(target).is_none() {
                ResampleError::UnknownSeries(target)
            } else {
                ResampleError::UnsupportedTarget(target)
            });
        }
        for id in [volume_source, volume_target].into_iter().flatten() {
            if !matches!(kind(id), Some(SeriesKind::Histogram)) {
                return Err(if kind(id).is_none() {
                    ResampleError::UnknownSeries(id)
                } else {
                    ResampleError::UnsupportedTarget(id)
                });
            }
        }
        let owned = self
            .resampled_series
            .values()
            .flat_map(|binding| [Some(binding.target), binding.volume_target])
            .flatten()
            .collect::<HashSet<_>>();
        if source == target
            || volume_source == Some(target)
            || volume_source.is_some_and(|id| volume_target == Some(id))
            || owned.contains(&source)
            || volume_source.is_some_and(|id| owned.contains(&id))
            || self.resampled_series.values().any(|binding| {
                binding.target == target && binding.source != source
                    || binding.volume_target == Some(target)
                    || volume_target
                        .is_some_and(|id| binding.target == id || binding.volume_target == Some(id))
            })
        {
            return Err(ResampleError::DependencyCycle);
        }
        self.resampled_series.insert(
            target,
            ResampleBinding {
                source,
                volume_source,
                target,
                volume_target,
                options,
                bars: Vec::new(),
            },
        );
        self.refresh_resampled_target(target)
    }

    pub fn resampled_bars(&self, target: SeriesId) -> Option<&[ResampledBar]> {
        Some(&self.resampled_series.get(&target)?.bars)
    }

    pub(crate) fn refresh_resampled_dependents(&mut self, source: SeriesId) {
        let targets = self
            .resampled_series
            .values()
            .filter(|binding| binding.source == source || binding.volume_source == Some(source))
            .map(|binding| binding.target)
            .collect::<Vec<_>>();
        for target in targets {
            let _ = self.refresh_resampled_target(target);
        }
    }

    fn refresh_resampled_target(&mut self, target: SeriesId) -> Result<(), ResampleError> {
        let (source, volume_source, volume_target, options) = {
            let binding = self
                .resampled_series
                .get(&target)
                .ok_or(ResampleError::UnknownSeries(target))?;
            (
                binding.source,
                binding.volume_source,
                binding.volume_target,
                binding.options.clone(),
            )
        };
        let (times, columns) = self
            .data
            .series_data(source)
            .ok_or(ResampleError::UnknownSeries(source))?;
        let volume = volume_source.and_then(|id| self.data.series_data(id));
        let bars = resample_rows(times, columns, volume, &options);
        let out_times = bars.iter().map(|bar| bar.timestamp).collect::<Vec<_>>();
        let open = bars.iter().map(|bar| bar.open).collect::<Vec<_>>();
        let high = bars.iter().map(|bar| bar.high).collect::<Vec<_>>();
        let low = bars.iter().map(|bar| bar.low).collect::<Vec<_>>();
        let close = bars.iter().map(|bar| bar.close).collect::<Vec<_>>();
        self.install_series_columns(target, out_times.clone(), open, high, low, close);
        self.enforce_series_cap(target);
        self.recompute_indicators_for(target);
        if let Some(volume_target) = volume_target {
            let values = bars.iter().map(|bar| bar.volume).collect::<Vec<_>>();
            self.install_series_columns(
                volume_target,
                out_times,
                values.clone(),
                values.clone(),
                values.clone(),
                values,
            );
            self.enforce_series_cap(volume_target);
            self.recompute_indicators_for(volume_target);
        }
        self.resampled_series
            .get_mut(&target)
            .expect("binding exists")
            .bars = bars;
        self.sync_time_points();
        self.invalidate_frame_scene();
        Ok(())
    }

    pub(crate) fn resampling_capacity_bytes(&self) -> usize {
        self.resampled_series
            .values()
            .map(|binding| {
                binding.options.boundaries.capacity() * std::mem::size_of::<ResampleBoundary>()
                    + binding.bars.capacity() * std::mem::size_of::<ResampledBar>()
            })
            .sum()
    }
}

fn validate_options(options: &ResampleOptions) -> Result<(), ResampleError> {
    if options.interval_seconds == 0 {
        return Err(ResampleError::InvalidInterval);
    }
    if options.boundaries.len() > MAX_RESAMPLE_BOUNDARIES {
        return Err(ResampleError::TooManyBoundaries);
    }
    if options.boundaries.is_empty()
        || options
            .boundaries
            .iter()
            .any(|boundary| boundary.start_time >= boundary.end_time)
        || options
            .boundaries
            .windows(2)
            .any(|pair| pair[0].end_time > pair[1].start_time)
    {
        return Err(ResampleError::InvalidBoundaries);
    }
    Ok(())
}

fn resample_rows(
    times: &[i64],
    columns: [&[f64]; 4],
    volume: Option<(&[i64], [&[f64]; 4])>,
    options: &ResampleOptions,
) -> Vec<ResampledBar> {
    let mut output = Vec::new();
    let mut row = 0usize;
    let interval = i64::from(options.interval_seconds);
    for boundary in &options.boundaries {
        row = row.max(times.partition_point(|&time| time < boundary.start_time));
        while row < times.len() && times[row] < boundary.end_time {
            let bucket =
                (i128::from(times[row]) - i128::from(boundary.start_time)) / i128::from(interval);
            // The bucket start lies between boundary.start_time and the current row, both i64.
            let timestamp =
                (i128::from(boundary.start_time) + bucket * i128::from(interval)) as i64;
            let end = boundary.end_time.min(timestamp.saturating_add(interval));
            let first = row;
            while row < times.len() && times[row] < end {
                row += 1;
            }
            let volume_sum = volume.map_or(0.0, |(volume_times, volume_columns)| {
                let start = volume_times.partition_point(|&time| time < timestamp);
                let finish = volume_times.partition_point(|&time| time < end);
                volume_columns[3][start..finish].iter().copied().sum()
            });
            output.push(ResampledBar {
                timestamp,
                session_id: boundary.session_id,
                open: columns[0][first],
                high: columns[1][first..row]
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max),
                low: columns[2][first..row]
                    .iter()
                    .copied()
                    .fold(f64::INFINITY, f64::min),
                close: columns[3][row - 1],
                volume: volume_sum,
                source_rows: u32::try_from(row - first).unwrap_or(u32::MAX),
            });
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SeriesKind;

    #[test]
    fn derived_volume_cannot_overwrite_its_source() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let source = chart.add_series(SeriesKind::Candlestick);
        let volume_source = chart.add_series(SeriesKind::Histogram);
        let target = chart.add_series(SeriesKind::Candlestick);
        chart
            .set_series_data(source, &[10.0], &[1.0], &[2.0], &[0.5], &[1.5])
            .unwrap();
        chart
            .set_series_data(volume_source, &[10.0], &[7.0], &[7.0], &[7.0], &[7.0])
            .unwrap();
        let result = chart.configure_resampled_series(
            source,
            Some(volume_source),
            target,
            Some(volume_source),
            ResampleOptions {
                interval_seconds: 60,
                boundaries: vec![ResampleBoundary {
                    start_time: 0,
                    end_time: 60,
                    session_id: 1,
                }],
            },
        );
        assert_eq!(result, Err(ResampleError::DependencyCycle));
        assert!(chart.resampled_bars(target).is_none());
        assert_eq!(chart.data.series_data(volume_source).unwrap().1[3], &[7.0]);
    }

    #[test]
    fn source_correction_rebuilds_derived_bars_and_study() {
        let build = |last_close: f64| {
            let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
            let source = chart.add_series(SeriesKind::Candlestick);
            let target = chart.add_series(SeriesKind::Candlestick);
            chart
                .configure_resampled_series(
                    source,
                    None,
                    target,
                    None,
                    ResampleOptions {
                        interval_seconds: 120,
                        boundaries: vec![ResampleBoundary {
                            start_time: 0,
                            end_time: 360,
                            session_id: 8,
                        }],
                    },
                )
                .unwrap();
            let study = chart.add_rsi(target, 2).unwrap();
            chart
                .set_series_data(
                    source,
                    &[0.0, 60.0, 120.0, 180.0, 240.0, 300.0],
                    &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                    &[
                        2.0,
                        3.0,
                        4.0,
                        5.0,
                        6.0,
                        if last_close > 7.0 { 8.0 } else { 7.0 },
                    ],
                    &[0.5, 1.5, 2.5, 3.5, 4.5, 5.5],
                    &[1.5, 2.5, 3.5, 4.5, 5.5, last_close],
                )
                .unwrap();
            (chart, source, target, study)
        };
        let (mut corrected, source, target, study) = build(6.5);
        assert!(corrected.update_series_bar(source, 300.0, [6.0, 8.0, 5.5, 7.5]));
        let (reference, _, reference_target, reference_study) = build(7.5);
        assert_eq!(corrected.resampled_bars(target).unwrap()[2].close, 7.5);
        assert_eq!(
            corrected.resampled_bars(target).unwrap(),
            reference.resampled_bars(reference_target).unwrap()
        );
        assert_eq!(
            corrected.data.series_data(study),
            reference.data.series_data(reference_study)
        );
    }

    #[test]
    fn boundaries_restart_buckets_and_exclude_out_of_session_rows() {
        let times = vec![0, 60, 120, 1_000, 1_060, 2_000];
        let columns = [
            vec![10.0, 11.0, 12.0, 20.0, 21.0, 99.0],
            vec![11.0, 13.0, 14.0, 21.0, 23.0, 100.0],
            vec![9.0, 10.0, 11.0, 19.0, 20.0, 98.0],
            vec![10.5, 12.0, 13.0, 20.5, 22.0, 99.5],
        ];
        let volume_times = times.clone();
        let volume = [
            vec![1.0; 6],
            vec![1.0; 6],
            vec![1.0; 6],
            vec![2.0, 3.0, 5.0, 7.0, 11.0, 13.0],
        ];
        let bars = resample_rows(
            &times,
            columns.each_ref().map(Vec::as_slice),
            Some((&volume_times, volume.each_ref().map(Vec::as_slice))),
            &ResampleOptions {
                interval_seconds: 120,
                boundaries: vec![
                    ResampleBoundary {
                        start_time: 0,
                        end_time: 180,
                        session_id: 7,
                    },
                    ResampleBoundary {
                        start_time: 1_000,
                        end_time: 1_120,
                        session_id: 8,
                    },
                ],
            },
        );
        assert_eq!(bars.len(), 3);
        assert_eq!(
            (
                bars[0].open,
                bars[0].high,
                bars[0].low,
                bars[0].close,
                bars[0].volume
            ),
            (10.0, 13.0, 9.0, 12.0, 5.0)
        );
        assert_eq!(
            (bars[2].timestamp, bars[2].session_id, bars[2].volume),
            (1_000, 8, 18.0)
        );
    }

    #[test]
    fn volume_uses_full_bucket_instead_of_last_price_timestamp() {
        let times = [0, 60];
        let prices = [1.0, 2.0];
        let volume_times = [0, 30, 90, 120];
        let volume = [2.0, 3.0, 5.0, 11.0];
        let bars = resample_rows(
            &times,
            [&prices; 4],
            Some((&volume_times, [&volume; 4])),
            &ResampleOptions {
                interval_seconds: 120,
                boundaries: vec![ResampleBoundary {
                    start_time: 0,
                    end_time: 120,
                    session_id: 1,
                }],
            },
        );
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].volume, 10.0);
    }
}
