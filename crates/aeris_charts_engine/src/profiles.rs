//! Boundary-driven volume, delta, TPO, and anchored-VWAP analytics.
//!
//! These snapshots are executor-neutral. Tape requests read the canonical classified trade stream;
//! candle requests are explicitly marked as approximations. Calendar/session policy is never
//! inferred: every periodic query receives host-supplied UTC boundaries.

use std::collections::BTreeMap;

use aeris_charts_indicators::volume_profile::{volume_profile, ProfileBar};
use serde::{Deserialize, Serialize};

use crate::{AggressorSide, ChartEngine, DrawingId, DrawingKind, ResampleBoundary, SeriesId};

pub const MAX_PROFILE_PERIODS: usize = 5_000;
pub const MAX_PROFILE_ROWS: usize = 2_048;
pub const MAX_PROFILE_DEVELOPING_POINTS: usize = 100_000;
pub const MAX_TPO_PERIODS: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProfileSource {
    Tape {
        stream_id: u64,
    },
    Candles {
        price_series: SeriesId,
        volume_series: SeriesId,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRequest {
    pub source: ProfileSource,
    pub start_timestamp_micros: i64,
    pub end_timestamp_micros: i64,
    pub tick_size: f64,
    pub row_count: usize,
    pub value_area_percent: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRowSnapshot {
    pub low: f64,
    pub high: f64,
    pub bid_volume: f64,
    pub ask_volume: f64,
    pub unknown_volume: f64,
    pub total_volume: f64,
    pub delta: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevelopingValueArea {
    pub timestamp_micros: i64,
    pub poc: f64,
    pub value_area_low: f64,
    pub value_area_high: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSnapshot {
    pub start_timestamp_micros: i64,
    pub end_timestamp_micros: i64,
    pub session_id: u64,
    pub rows: Vec<ProfileRowSnapshot>,
    pub total_volume: f64,
    pub poc: Option<f64>,
    pub value_area_low: Option<f64>,
    pub value_area_high: Option<f64>,
    pub developing: Vec<DevelopingValueArea>,
    pub candle_approximation: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NakedProfileLevel {
    pub session_id: u64,
    pub price: f64,
    pub kind: NakedProfileLevelKind,
    pub start_timestamp_micros: i64,
    pub touched_timestamp_micros: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NakedProfileLevelKind {
    Poc,
    ValueAreaLow,
    ValueAreaHigh,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TpoRequest {
    pub price_series: SeriesId,
    pub boundaries: Vec<ResampleBoundary>,
    pub period_seconds: u32,
    pub tick_size: f64,
    pub value_area_percent: f64,
    pub initial_balance_periods: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TpoRowSnapshot {
    pub price: f64,
    /// Zero-based period indices. Hosts map these to letters or block glyphs.
    pub periods: Vec<u16>,
    pub single_print: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TpoSnapshot {
    pub session_id: u64,
    pub start_timestamp_micros: i64,
    pub end_timestamp_micros: i64,
    pub rows: Vec<TpoRowSnapshot>,
    pub poc: Option<f64>,
    pub value_area_low: Option<f64>,
    pub value_area_high: Option<f64>,
    pub initial_balance_low: Option<f64>,
    pub initial_balance_high: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchoredVwapPoint {
    pub timestamp_micros: i64,
    pub vwap: f64,
    pub upper_band: f64,
    pub lower_band: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDrawingOptions {
    pub source: ProfileSource,
    pub tick_size: f64,
    pub row_count: usize,
    pub value_area_percent: f64,
    pub band_multiplier: f64,
    pub width_percent: f64,
}

impl Default for ProfileDrawingOptions {
    fn default() -> Self {
        Self {
            source: ProfileSource::Candles {
                price_series: 0,
                volume_series: 0,
            },
            tick_size: 0.01,
            row_count: 48,
            value_area_percent: 70.0,
            band_multiplier: 1.0,
            width_percent: 30.0,
        }
    }
}

impl ProfileDrawingOptions {
    pub(crate) fn valid(&self) -> bool {
        valid_tick(self.tick_size)
            && (1..=MAX_PROFILE_ROWS).contains(&self.row_count)
            && (0.0 < self.value_area_percent && self.value_area_percent <= 100.0)
            && self.band_multiplier.is_finite()
            && self.band_multiplier >= 0.0
            && self.width_percent.is_finite()
            && (0.0 < self.width_percent && self.width_percent <= 100.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "data")]
pub enum ProfileDrawingSnapshot {
    Volume(ProfileSnapshot),
    Vwap(Vec<AnchoredVwapPoint>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProfileError {
    InvalidRequest,
    UnknownSource,
    LimitExceeded,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "invalid profile request",
            Self::UnknownSource => "profile source is unavailable",
            Self::LimitExceeded => "profile result limit exceeded",
        })
    }
}
impl std::error::Error for ProfileError {}

impl ChartEngine {
    pub fn configure_profile_drawing(
        &mut self,
        drawing_id: DrawingId,
        options: ProfileDrawingOptions,
    ) -> Result<(), ProfileError> {
        let drawing = self
            .drawing(drawing_id)
            .ok_or(ProfileError::UnknownSource)?;
        if !matches!(
            drawing.kind,
            DrawingKind::FixedRangeVolumeProfile
                | DrawingKind::AnchoredVolumeProfile
                | DrawingKind::AnchoredVwap
        ) || !options.valid()
        {
            return Err(ProfileError::InvalidRequest);
        }
        self.validate_profile_source(options.source)?;
        self.set_drawing_profile_options(drawing_id, options)
            .then_some(())
            .ok_or(ProfileError::UnknownSource)
    }

    pub fn profile_drawing_options(&self, drawing_id: DrawingId) -> Option<&ProfileDrawingOptions> {
        self.drawing(drawing_id)?.profile.as_ref()
    }

    pub fn profile_drawing_snapshot(
        &self,
        drawing_id: DrawingId,
    ) -> Result<ProfileDrawingSnapshot, ProfileError> {
        let drawing = self
            .drawing(drawing_id)
            .ok_or(ProfileError::UnknownSource)?;
        let options = drawing
            .profile
            .as_ref()
            .ok_or(ProfileError::UnknownSource)?;
        let first = drawing
            .points
            .first()
            .ok_or(ProfileError::InvalidRequest)?
            .logical;
        let last = match drawing.kind {
            DrawingKind::FixedRangeVolumeProfile => {
                drawing
                    .points
                    .get(1)
                    .ok_or(ProfileError::InvalidRequest)?
                    .logical
            }
            DrawingKind::AnchoredVolumeProfile | DrawingKind::AnchoredVwap => {
                self.data.merged_times().len().saturating_sub(1) as f64
            }
            _ => return Err(ProfileError::InvalidRequest),
        };
        let start = self
            .logical_timestamp_micros(first.min(last))
            .ok_or(ProfileError::InvalidRequest)?;
        let end = self
            .logical_timestamp_micros(first.max(last))
            .ok_or(ProfileError::InvalidRequest)?
            .saturating_add(1);
        if drawing.kind == DrawingKind::AnchoredVwap {
            return self
                .anchored_vwap(options.source, start, end, options.band_multiplier)
                .map(ProfileDrawingSnapshot::Vwap);
        }
        self.volume_profile_snapshot(&ProfileRequest {
            source: options.source,
            start_timestamp_micros: start,
            end_timestamp_micros: end,
            tick_size: options.tick_size,
            row_count: options.row_count,
            value_area_percent: options.value_area_percent,
        })
        .map(ProfileDrawingSnapshot::Volume)
    }

    fn logical_timestamp_micros(&self, logical: f64) -> Option<i64> {
        if !logical.is_finite() || logical < 0.0 {
            return None;
        }
        let index = logical.round() as usize;
        if let Some(points) = self.sequence_points() {
            return points.get(index).map(|point| point.open_timestamp_micros);
        }
        self.data
            .merged_times()
            .get(index)
            .map(|time| time.saturating_mul(1_000_000))
    }

    fn validate_profile_source(&self, source: ProfileSource) -> Result<(), ProfileError> {
        match source {
            ProfileSource::Tape { stream_id } => self
                .trade_stream(stream_id)
                .map(|_| ())
                .ok_or(ProfileError::UnknownSource),
            ProfileSource::Candles {
                price_series,
                volume_series,
            } => {
                if self.data.series_data(price_series).is_some()
                    && self.data.series_data(volume_series).is_some()
                {
                    Ok(())
                } else {
                    Err(ProfileError::UnknownSource)
                }
            }
        }
    }

    pub fn volume_profile_snapshot(
        &self,
        request: &ProfileRequest,
    ) -> Result<ProfileSnapshot, ProfileError> {
        validate_profile_request(request)?;
        match request.source {
            ProfileSource::Tape { stream_id } => self.tape_profile(stream_id, request),
            ProfileSource::Candles {
                price_series,
                volume_series,
            } => self.candle_profile(price_series, volume_series, request),
        }
    }

    pub fn periodic_volume_profiles(
        &self,
        source: ProfileSource,
        boundaries: &[ResampleBoundary],
        tick_size: f64,
        row_count: usize,
        value_area_percent: f64,
    ) -> Result<Vec<ProfileSnapshot>, ProfileError> {
        if boundaries.is_empty()
            || boundaries.len() > MAX_PROFILE_PERIODS
            || boundaries.iter().any(|b| b.start_time >= b.end_time)
            || boundaries
                .windows(2)
                .any(|pair| pair[0].end_time > pair[1].start_time)
        {
            return Err(ProfileError::InvalidRequest);
        }
        boundaries
            .iter()
            .map(|boundary| {
                let mut profile = self.volume_profile_snapshot(&ProfileRequest {
                    source,
                    start_timestamp_micros: boundary.start_time.saturating_mul(1_000_000),
                    end_timestamp_micros: boundary.end_time.saturating_mul(1_000_000),
                    tick_size,
                    row_count,
                    value_area_percent,
                })?;
                profile.session_id = boundary.session_id;
                Ok(profile)
            })
            .collect()
    }

    pub fn naked_profile_levels(
        &self,
        source: ProfileSource,
        profiles: &[ProfileSnapshot],
    ) -> Result<Vec<NakedProfileLevel>, ProfileError> {
        let mut levels = Vec::with_capacity(profiles.len().saturating_mul(3));
        for profile in profiles {
            for (kind, price) in [
                (NakedProfileLevelKind::Poc, profile.poc),
                (NakedProfileLevelKind::ValueAreaLow, profile.value_area_low),
                (
                    NakedProfileLevelKind::ValueAreaHigh,
                    profile.value_area_high,
                ),
            ] {
                if let Some(price) = price {
                    levels.push(NakedProfileLevel {
                        session_id: profile.session_id,
                        price,
                        kind,
                        start_timestamp_micros: profile.end_timestamp_micros,
                        touched_timestamp_micros: self.first_touch(
                            source,
                            profile.end_timestamp_micros,
                            price,
                        )?,
                    });
                }
            }
        }
        Ok(levels)
    }

    pub fn tpo_profiles(&self, request: &TpoRequest) -> Result<Vec<TpoSnapshot>, ProfileError> {
        if request.boundaries.is_empty()
            || request.boundaries.len() > MAX_PROFILE_PERIODS
            || request.period_seconds == 0
            || !valid_tick(request.tick_size)
            || !(0.0 < request.value_area_percent && request.value_area_percent <= 100.0)
            || request.initial_balance_periods == 0
        {
            return Err(ProfileError::InvalidRequest);
        }
        let (times, values) = self
            .data
            .series_data(request.price_series)
            .ok_or(ProfileError::UnknownSource)?;
        let period_micros = i64::from(request.period_seconds).saturating_mul(1_000_000);
        let mut output = Vec::with_capacity(request.boundaries.len());
        for boundary in &request.boundaries {
            let start = boundary.start_time.saturating_mul(1_000_000);
            let end = boundary.end_time.saturating_mul(1_000_000);
            let first = times.partition_point(|&time| time.saturating_mul(1_000_000) < start);
            let finish = times.partition_point(|&time| time.saturating_mul(1_000_000) < end);
            let mut rows: BTreeMap<i64, Vec<u16>> = BTreeMap::new();
            let mut ib_low = f64::INFINITY;
            let mut ib_high = f64::NEG_INFINITY;
            for row in first..finish {
                let time = times[row].saturating_mul(1_000_000);
                let period = ((time - start) / period_micros).clamp(0, i64::from(u16::MAX)) as u16;
                if usize::from(period) >= MAX_TPO_PERIODS {
                    return Err(ProfileError::LimitExceeded);
                }
                if period < request.initial_balance_periods {
                    ib_low = ib_low.min(values[2][row]);
                    ib_high = ib_high.max(values[1][row]);
                }
                let low = price_level(values[2][row], request.tick_size);
                let high = price_level(values[1][row], request.tick_size);
                if high.saturating_sub(low) as usize > MAX_PROFILE_ROWS {
                    return Err(ProfileError::LimitExceeded);
                }
                for level in low..=high {
                    let periods = rows.entry(level).or_default();
                    if periods.last() != Some(&period) {
                        periods.push(period);
                    }
                }
            }
            if rows.len() > MAX_PROFILE_ROWS {
                return Err(ProfileError::LimitExceeded);
            }
            let counts = rows
                .iter()
                .map(|(&level, periods)| (level, periods.len() as f64))
                .collect::<Vec<_>>();
            let (poc, val, vah) =
                value_area_levels(&counts, request.value_area_percent, request.tick_size);
            output.push(TpoSnapshot {
                session_id: boundary.session_id,
                start_timestamp_micros: start,
                end_timestamp_micros: end,
                rows: rows
                    .into_iter()
                    .map(|(level, periods)| TpoRowSnapshot {
                        price: level as f64 * request.tick_size,
                        single_print: periods.len() == 1,
                        periods,
                    })
                    .collect(),
                poc,
                value_area_low: val,
                value_area_high: vah,
                initial_balance_low: ib_low.is_finite().then_some(ib_low),
                initial_balance_high: ib_high.is_finite().then_some(ib_high),
            });
        }
        Ok(output)
    }

    pub fn anchored_vwap(
        &self,
        source: ProfileSource,
        start_timestamp_micros: i64,
        end_timestamp_micros: i64,
        band_multiplier: f64,
    ) -> Result<Vec<AnchoredVwapPoint>, ProfileError> {
        if start_timestamp_micros >= end_timestamp_micros
            || !band_multiplier.is_finite()
            || band_multiplier < 0.0
        {
            return Err(ProfileError::InvalidRequest);
        }
        let mut weighted = 0.0;
        let mut weight = 0.0;
        let mut weighted_square = 0.0;
        let mut output = Vec::new();
        let mut push = |timestamp_micros: i64, price: f64, volume: f64| {
            if price.is_finite() && volume.is_finite() && volume > 0.0 {
                weighted += price * volume;
                weighted_square += price * price * volume;
                weight += volume;
                let vwap = weighted / weight;
                let deviation = (weighted_square / weight - vwap * vwap).max(0.0).sqrt();
                output.push(AnchoredVwapPoint {
                    timestamp_micros,
                    vwap,
                    upper_band: vwap + deviation * band_multiplier,
                    lower_band: vwap - deviation * band_multiplier,
                });
            }
        };
        match source {
            ProfileSource::Tape { stream_id } => {
                for (trade, _) in self
                    .trade_stream(stream_id)
                    .ok_or(ProfileError::UnknownSource)?
                    .classified_trades()
                    .filter(|(trade, _)| {
                        trade.timestamp_micros >= start_timestamp_micros
                            && trade.timestamp_micros < end_timestamp_micros
                    })
                {
                    push(trade.timestamp_micros, trade.price, trade.volume);
                }
            }
            ProfileSource::Candles {
                price_series,
                volume_series,
            } => {
                let (times, prices) = self
                    .data
                    .series_data(price_series)
                    .ok_or(ProfileError::UnknownSource)?;
                let (volume_times, volumes) = self
                    .data
                    .series_data(volume_series)
                    .ok_or(ProfileError::UnknownSource)?;
                for row in times.partition_point(|&time| {
                    time.saturating_mul(1_000_000) < start_timestamp_micros
                })
                    ..times.partition_point(|&time| {
                        time.saturating_mul(1_000_000) < end_timestamp_micros
                    })
                {
                    if let Ok(volume_row) = volume_times.binary_search(&times[row]) {
                        push(
                            times[row].saturating_mul(1_000_000),
                            (prices[1][row] + prices[2][row] + prices[3][row]) / 3.0,
                            volumes[3][volume_row],
                        );
                    }
                }
            }
        }
        Ok(output)
    }

    fn tape_profile(
        &self,
        stream_id: u64,
        request: &ProfileRequest,
    ) -> Result<ProfileSnapshot, ProfileError> {
        let stream = self
            .trade_stream(stream_id)
            .ok_or(ProfileError::UnknownSource)?;
        let mut rows: BTreeMap<i64, ProfileRowSnapshot> = BTreeMap::new();
        let mut developing = Vec::new();
        for (trade, side) in stream.classified_trades().filter(|(trade, _)| {
            trade.timestamp_micros >= request.start_timestamp_micros
                && trade.timestamp_micros < request.end_timestamp_micros
        }) {
            let level = price_level(trade.price, request.tick_size);
            let row = rows.entry(level).or_insert_with(|| ProfileRowSnapshot {
                low: level as f64 * request.tick_size,
                high: (level + 1) as f64 * request.tick_size,
                ..Default::default()
            });
            match side {
                AggressorSide::Buy => row.ask_volume += trade.volume,
                AggressorSide::Sell => row.bid_volume += trade.volume,
                AggressorSide::Unknown => row.unknown_volume += trade.volume,
            }
            row.total_volume += trade.volume;
            row.delta = row.ask_volume - row.bid_volume;
            if rows.len() > MAX_PROFILE_ROWS {
                return Err(ProfileError::LimitExceeded);
            }
            if developing.len() < MAX_PROFILE_DEVELOPING_POINTS {
                let counts = rows
                    .iter()
                    .map(|(&level, row)| (level, row.total_volume))
                    .collect::<Vec<_>>();
                let (poc, val, vah) =
                    value_area_levels(&counts, request.value_area_percent, request.tick_size);
                if let (Some(poc), Some(value_area_low), Some(value_area_high)) = (poc, val, vah) {
                    developing.push(DevelopingValueArea {
                        timestamp_micros: trade.timestamp_micros,
                        poc,
                        value_area_low,
                        value_area_high,
                    });
                }
            }
        }
        let counts = rows
            .iter()
            .map(|(&level, row)| (level, row.total_volume))
            .collect::<Vec<_>>();
        let (poc, value_area_low, value_area_high) =
            value_area_levels(&counts, request.value_area_percent, request.tick_size);
        Ok(ProfileSnapshot {
            start_timestamp_micros: request.start_timestamp_micros,
            end_timestamp_micros: request.end_timestamp_micros,
            rows: rows.into_values().collect(),
            total_volume: counts.iter().map(|(_, volume)| volume).sum(),
            poc,
            value_area_low,
            value_area_high,
            developing,
            candle_approximation: false,
            ..Default::default()
        })
    }

    fn candle_profile(
        &self,
        price_series: SeriesId,
        volume_series: SeriesId,
        request: &ProfileRequest,
    ) -> Result<ProfileSnapshot, ProfileError> {
        let (times, prices) = self
            .data
            .series_data(price_series)
            .ok_or(ProfileError::UnknownSource)?;
        let (volume_times, volumes) = self
            .data
            .series_data(volume_series)
            .ok_or(ProfileError::UnknownSource)?;
        let first = times.partition_point(|&time| {
            time.saturating_mul(1_000_000) < request.start_timestamp_micros
        });
        let finish = times
            .partition_point(|&time| time.saturating_mul(1_000_000) < request.end_timestamp_micros);
        let bars = (first..finish)
            .filter_map(|row| {
                volume_times
                    .binary_search(&times[row])
                    .ok()
                    .map(|volume_row| ProfileBar {
                        open: prices[0][row],
                        high: prices[1][row],
                        low: prices[2][row],
                        close: prices[3][row],
                        volume: volumes[3][volume_row],
                    })
            })
            .collect::<Vec<_>>();
        let profile = volume_profile(
            bars.iter().copied(),
            request.row_count.min(MAX_PROFILE_ROWS),
            request.value_area_percent,
            request.tick_size,
        )
        .map_err(|_| ProfileError::InvalidRequest)?;
        let rows = profile
            .rows
            .iter()
            .map(|row| ProfileRowSnapshot {
                low: row.low,
                high: row.high,
                bid_volume: row.down_volume,
                ask_volume: row.up_volume,
                unknown_volume: 0.0,
                total_volume: row.volume,
                delta: row.up_volume - row.down_volume,
            })
            .collect::<Vec<_>>();
        let center = |index: Option<usize>| {
            index
                .and_then(|index| rows.get(index))
                .map(|row| (row.low + row.high) * 0.5)
        };
        let poc = center(profile.poc_index);
        let value_area_low = center(profile.value_area_low_index);
        let value_area_high = center(profile.value_area_high_index);
        Ok(ProfileSnapshot {
            start_timestamp_micros: request.start_timestamp_micros,
            end_timestamp_micros: request.end_timestamp_micros,
            rows,
            total_volume: profile.total_volume,
            poc,
            value_area_low,
            value_area_high,
            candle_approximation: true,
            ..Default::default()
        })
    }

    fn first_touch(
        &self,
        source: ProfileSource,
        after: i64,
        price: f64,
    ) -> Result<Option<i64>, ProfileError> {
        match source {
            ProfileSource::Tape { stream_id } => Ok(self
                .trade_stream(stream_id)
                .ok_or(ProfileError::UnknownSource)?
                .classified_trades()
                .find(|(trade, _)| {
                    trade.timestamp_micros >= after && (trade.price - price).abs() <= f64::EPSILON
                })
                .map(|(trade, _)| trade.timestamp_micros)),
            ProfileSource::Candles { price_series, .. } => {
                let (times, values) = self
                    .data
                    .series_data(price_series)
                    .ok_or(ProfileError::UnknownSource)?;
                Ok(times
                    .iter()
                    .enumerate()
                    .find(|(row, time)| {
                        time.saturating_mul(1_000_000) >= after
                            && values[2][*row] <= price
                            && values[1][*row] >= price
                    })
                    .map(|(_, time)| time.saturating_mul(1_000_000)))
            }
        }
    }
}

fn validate_profile_request(request: &ProfileRequest) -> Result<(), ProfileError> {
    if request.start_timestamp_micros >= request.end_timestamp_micros
        || !valid_tick(request.tick_size)
        || !(1..=MAX_PROFILE_ROWS).contains(&request.row_count)
        || !request.value_area_percent.is_finite()
        || !(0.0 < request.value_area_percent && request.value_area_percent <= 100.0)
    {
        Err(ProfileError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn valid_tick(tick: f64) -> bool {
    tick.is_finite() && tick > 0.0
}
fn price_level(price: f64, tick: f64) -> i64 {
    (price / tick).round() as i64
}

fn value_area_levels(
    rows: &[(i64, f64)],
    percent: f64,
    tick: f64,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    if rows.is_empty() {
        return (None, None, None);
    }
    let mut poc = 0;
    for index in 1..rows.len() {
        if rows[index].1 > rows[poc].1 {
            poc = index;
        }
    }
    let target = rows.iter().map(|row| row.1).sum::<f64>() * percent / 100.0;
    let (mut low, mut high, mut included) = (poc, poc, rows[poc].1);
    while included < target && (low > 0 || high + 1 < rows.len()) {
        if high + 1 == rows.len() || (low > 0 && rows[low - 1].1 >= rows[high + 1].1) {
            low -= 1;
            included += rows[low].1;
        } else {
            high += 1;
            included += rows[high].1;
        }
    }
    (
        Some(rows[poc].0 as f64 * tick),
        Some(rows[low].0 as f64 * tick),
        Some(rows[high].0 as f64 * tick),
    )
}
