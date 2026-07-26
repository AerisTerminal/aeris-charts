//! Engine-owned indicator producers (SMA/EMA/Bollinger).
//!
//! Indicators are bound to a source series and recomputed on source updates; their outputs are
//! ordinary engine series (`aion_indicators` holds the pure math). Extracted from `lib.rs`.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum IndicatorKind {
    Sma { period: usize },
    Ema { period: usize },
    Bollinger { period: usize, deviation: f64 },
}

#[derive(Clone, Debug)]
pub(crate) struct IndicatorBinding {
    source: SeriesId,
    kind: IndicatorKind,
    outputs: Vec<SeriesId>,
    last_source_len: usize,
    last_source_time: Option<i64>,
}

/// An indicator output series' lineage: which binding it belongs to (kind + params), the
/// source series it derives from, and which output slot it is (Bollinger: 0 = upper,
/// 1 = middle, 2 = lower; SMA/EMA: always 0). Platforms read this to render their own
/// indicator chrome (legend chips, counts, settings) without the engine owning any UI.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct IndicatorInfo {
    pub kind: &'static str,
    pub period: usize,
    pub deviation: Option<f64>,
    pub source: SeriesId,
    pub output_index: usize,
}

impl ChartEngine {
    /// The binding an output series belongs to, or `None` when `id` is not an indicator output
    /// (a plain series, an unknown/removed id, or a source series itself).
    pub fn indicator_info(&self, id: SeriesId) -> Option<IndicatorInfo> {
        self.indicators.iter().find_map(|binding| {
            binding
                .outputs
                .iter()
                .position(|&output| output == id)
                .map(|output_index| {
                    let (kind, period, deviation) = match binding.kind {
                        IndicatorKind::Sma { period } => ("sma", period, None),
                        IndicatorKind::Ema { period } => ("ema", period, None),
                        IndicatorKind::Bollinger { period, deviation } => {
                            ("bollinger", period, Some(deviation))
                        }
                    };
                    IndicatorInfo {
                        kind,
                        period,
                        deviation,
                        source: binding.source,
                        output_index,
                    }
                })
        })
    }

    /// Add a Rust-native simple moving-average producer. The returned line series is owned by the
    /// engine and is recomputed whenever its source series changes.
    pub fn add_sma(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator(source, IndicatorKind::Sma { period }, 1)
            .into_iter()
            .next()
    }

    /// Add a Rust-native exponential moving-average producer.
    pub fn add_ema(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator(source, IndicatorKind::Ema { period }, 1)
            .into_iter()
            .next()
    }

    /// Add upper, middle, and lower Bollinger-band line series in that order.
    pub fn add_bollinger(
        &mut self,
        source: SeriesId,
        period: usize,
        deviation: f64,
    ) -> Vec<SeriesId> {
        self.add_indicator(source, IndicatorKind::Bollinger { period, deviation }, 3)
    }

    /// Drop every indicator binding that reads from or writes to `id`, returning the output series
    /// ids those bindings owned so the caller can tombstone them alongside `id`. Used by
    /// `remove_series`: removing a source drops its derived indicators; removing an indicator's own
    /// output series drops the whole binding (and its sibling outputs).
    pub(crate) fn drop_indicators_touching(&mut self, id: SeriesId) -> Vec<SeriesId> {
        let mut dropped_outputs = Vec::new();
        self.indicators.retain(|binding| {
            if binding.source == id || binding.outputs.contains(&id) {
                dropped_outputs.extend(binding.outputs.iter().copied());
                false
            } else {
                true
            }
        });
        dropped_outputs
    }

    fn add_indicator(
        &mut self,
        source: SeriesId,
        kind: IndicatorKind,
        outputs: usize,
    ) -> Vec<SeriesId> {
        if source >= self.series.len()
            || outputs == 0
            || matches!(
                &kind,
                IndicatorKind::Sma { period: 0 }
                    | IndicatorKind::Ema { period: 0 }
                    | IndicatorKind::Bollinger { period: 0, .. }
            )
        {
            return Vec::new();
        }
        let ids = (0..outputs)
            .map(|_| self.add_series(SeriesKind::Line))
            .collect::<Vec<_>>();
        self.indicators.push(IndicatorBinding {
            source,
            kind,
            outputs: ids.clone(),
            last_source_len: 0,
            last_source_time: None,
        });
        self.recompute_indicators();
        ids
    }

    pub(crate) fn recompute_indicators(&mut self) {
        for index in 0..self.indicators.len() {
            let binding = self.indicators[index].clone();
            let Some((times, values)) = self.data.series_data(binding.source) else {
                continue;
            };
            let times = times.to_vec();
            let close = values[3].to_vec();
            match binding.kind {
                IndicatorKind::Sma { period } => {
                    let values = aion_indicators::sma(&close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Ema { period } => {
                    let values = aion_indicators::ema(&close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Bollinger { period, deviation } => {
                    let values = aion_indicators::bollinger(&close, period, deviation);
                    let mut upper = Vec::with_capacity(values.len());
                    let mut middle = Vec::with_capacity(values.len());
                    let mut lower = Vec::with_capacity(values.len());
                    for point in values {
                        upper.push(point.upper);
                        middle.push(point.middle);
                        lower.push(point.lower);
                    }
                    self.install_indicator_output(binding.outputs[0], &times, &upper);
                    self.install_indicator_output(binding.outputs[1], &times, &middle);
                    self.install_indicator_output(binding.outputs[2], &times, &lower);
                }
            }
            self.indicators[index].last_source_len = times.len();
            self.indicators[index].last_source_time = times.last().copied();
        }
        self.sync_time_points();
    }

    pub(crate) fn update_indicators_after_source_update(&mut self, source: SeriesId, time: i64) {
        for index in 0..self.indicators.len() {
            if self.indicators[index].source != source {
                continue;
            }
            let binding = self.indicators[index].clone();
            let Some((times, values)) = self.data.series_data(source) else {
                continue;
            };
            let source_len = times.len();
            let source_last_time = times.last().copied();
            let close = values[3];
            let tail_update = binding.last_source_len > 0
                && binding
                    .last_source_time
                    .map(|last| time >= last)
                    .unwrap_or(false)
                && (source_len == binding.last_source_len
                    || source_len == binding.last_source_len + 1);
            if !tail_update {
                self.recompute_indicators();
                return;
            }
            let appended = source_len == binding.last_source_len + 1;
            match binding.kind {
                IndicatorKind::Sma { period } => {
                    if let Some(value) = rolling_mean(close, period) {
                        self.data.update(binding.outputs[0], time, [value; 4]);
                    }
                }
                IndicatorKind::Ema { period } => {
                    if let Some(value) =
                        rolling_ema_tail(close, period, &self.data, binding.outputs[0], appended)
                    {
                        self.data.update(binding.outputs[0], time, [value; 4]);
                    }
                }
                IndicatorKind::Bollinger { period, deviation } => {
                    if let Some((upper, middle, lower)) =
                        rolling_bollinger(close, period, deviation)
                    {
                        self.data.update(binding.outputs[0], time, [upper; 4]);
                        self.data.update(binding.outputs[1], time, [middle; 4]);
                        self.data.update(binding.outputs[2], time, [lower; 4]);
                    }
                }
            }
            self.indicators[index].last_source_len = source_len;
            self.indicators[index].last_source_time = source_last_time.or(Some(time));
        }
        self.sync_time_points();
    }

    fn install_indicator_output(&mut self, id: SeriesId, times: &[i64], values: &[Option<f64>]) {
        let mut out_times = Vec::new();
        let mut out_values = Vec::new();
        for (&time, value) in times.iter().zip(values) {
            if let Some(value) = value {
                out_times.push(time);
                out_values.push(*value);
            }
        }
        self.data.set_data(
            id,
            out_times,
            out_values.clone(),
            out_values.clone(),
            out_values.clone(),
            out_values,
        );
    }
}

fn rolling_mean(values: &[f64], period: usize) -> Option<f64> {
    (period > 0 && values.len() >= period)
        .then(|| values[values.len() - period..].iter().sum::<f64>() / period as f64)
}

fn rolling_bollinger(values: &[f64], period: usize, deviation: f64) -> Option<(f64, f64, f64)> {
    let window =
        (period > 0 && values.len() >= period).then(|| &values[values.len() - period..])?;
    let middle = window.iter().sum::<f64>() / period as f64;
    let spread = (window.iter().map(|v| (v - middle).powi(2)).sum::<f64>() / period as f64).sqrt()
        * deviation.max(0.0);
    Some((middle + spread, middle, middle - spread))
}

fn rolling_ema_tail(
    values: &[f64],
    period: usize,
    data: &DataLayer,
    output: SeriesId,
    appended: bool,
) -> Option<f64> {
    if period == 0 || values.len() < period {
        return None;
    }
    if values.len() == period {
        return rolling_mean(values, period);
    }
    let previous = data.series_data(output)?;
    let output_values = previous.1[3];
    let previous_ema = if appended {
        output_values.last().copied()?
    } else if output_values.len() >= 2 {
        output_values[output_values.len() - 2]
    } else {
        return rolling_mean(&values[..values.len() - 1], period);
    };
    let alpha = 2.0 / (period as f64 + 1.0);
    Some(alpha * values[values.len() - 1] + (1.0 - alpha) * previous_ema)
}
