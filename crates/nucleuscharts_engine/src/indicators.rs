//! Engine-owned indicator producers.
//!
//! Indicators are bound to a source series and recomputed on source updates; their outputs are
//! ordinary engine series (`nucleuscharts_indicators` holds the pure math). Extracted from `lib.rs`.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum IndicatorKind {
    Sma {
        period: usize,
    },
    Ema {
        period: usize,
    },
    EmaRibbon {
        periods: [usize; nucleuscharts_indicators::MAX_OUTPUTS],
    },
    Bollinger {
        period: usize,
        deviation: f64,
    },
    Rsi {
        period: usize,
    },
    Macd {
        fast: usize,
        slow: usize,
        signal: usize,
    },
    Stochastic {
        k_period: usize,
        d_period: usize,
    },
    Atr {
        period: usize,
    },
    Vwap,
    Wma {
        period: usize,
    },
}

/// One live indicator producer's typed, runtime-independent definition.
///
/// Bindings are enumerated in creation order, which is also dependency order: an output must
/// exist before it can become a later binding's source. Hosts can therefore recreate bindings in
/// this order while remapping each old output identity to the newly returned output identity.
#[derive(Clone, Debug, PartialEq)]
pub struct IndicatorBindingInfo {
    /// Stable chart-local binding identity, equal to the first output identity.
    pub binding_id: SeriesId,
    pub kind: IndicatorKind,
    pub source: SeriesId,
    /// Parallel volume column source for VWAP; `None` means unit weights.
    pub volume_source: Option<SeriesId>,
    /// Output identities in the indicator's documented order.
    pub outputs: Vec<SeriesId>,
}

#[derive(Clone, Debug)]
pub(crate) struct IndicatorBinding {
    pub(crate) source: SeriesId,
    pub(crate) kind: IndicatorKind,
    pub(crate) outputs: Vec<SeriesId>,
    /// Parallel volume column source (VWAP); `None` = unit weights.
    pub(crate) volume_source: Option<SeriesId>,
    runtime: nucleuscharts_indicators::IncrementalState,
    source_generation: u64,
    volume_generation: Option<u64>,
}

#[derive(Clone, Copy)]
pub(crate) struct IndicatorChange {
    pub(crate) from: usize,
    pub(crate) previous_generation: u64,
    pub(crate) full_replace: bool,
}

/// Stretch factor of the pane a separate-pane indicator creates for itself (TradingView
/// oscillators stack as a shorter strip under the price pane).
pub(crate) const OSCILLATOR_PANE_STRETCH: f64 = 0.3;

pub const EMA_RIBBON_DEFAULT_PERIODS: [usize; nucleuscharts_indicators::MAX_OUTPUTS] =
    [5, 10, 20, 50, 200];
pub const EMA_RIBBON_DEFAULT_COLORS: [&str; nucleuscharts_indicators::MAX_OUTPUTS] =
    ["#335cff", "#FF9800", "#7d52f4", "#fb4ba3", "#fb3748"];

/// TradingView oscillator band-line color (RSI 30/70, Stochastic 20/80).
const BAND_LEVEL_COLOR: Color = Color::rgb(0x78, 0x7B, 0x86);

/// MACD histogram four-state palette: strong when moving away from zero, weak when falling
/// back toward it (TradingView-style). Packed `0xRRGGBBAA`.
const MACD_UP: u32 = rgb_u32(nucleuscharts_core::style::MARKET_UP_RGB, 0xff);
const MACD_UP_WEAK: u32 = rgb_u32(
    nucleuscharts_core::style::MARKET_UP_RGB,
    nucleuscharts_core::style::MARKET_VOLUME_ALPHA,
);
const MACD_DOWN: u32 = rgb_u32(nucleuscharts_core::style::MARKET_DOWN_RGB, 0xff);
const MACD_DOWN_WEAK: u32 = rgb_u32(
    nucleuscharts_core::style::MARKET_DOWN_RGB,
    nucleuscharts_core::style::MARKET_VOLUME_ALPHA,
);

const fn rgb_u32(rgb: (u8, u8, u8), alpha: u8) -> u32 {
    (rgb.0 as u32) << 24 | (rgb.1 as u32) << 16 | (rgb.2 as u32) << 8 | alpha as u32
}

/// An indicator output series' lineage: which binding it belongs to (kind + params), the
/// source series it derives from, and which output slot it is (Bollinger: 0 = upper,
/// 1 = middle, 2 = lower; SMA/EMA: always 0). Platforms read this to render their own
/// indicator chrome (legend chips, counts, settings) without the engine owning any UI.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct IndicatorInfo {
    /// Stable identity shared by every output of this binding. The first output's opaque series
    /// identity is safe because output identities are monotonic and the binding owns all outputs.
    pub binding_id: SeriesId,
    pub kind: &'static str,
    pub parameters: IndicatorParameters,
    pub period: usize,
    pub deviation: Option<f64>,
    pub source: SeriesId,
    pub volume_source: Option<SeriesId>,
    pub output_name: &'static str,
    pub output_index: usize,
    pub output_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct IndicatorParameters {
    pub period: Option<usize>,
    pub periods: Option<[usize; nucleuscharts_indicators::MAX_OUTPUTS]>,
    pub deviation: Option<f64>,
    pub fast: Option<usize>,
    pub slow: Option<usize>,
    pub signal: Option<usize>,
    pub k_period: Option<usize>,
    pub d_period: Option<usize>,
}

impl ChartEngine {
    pub(crate) fn indicator_memory_usage(&self) -> (usize, usize) {
        self.indicators.iter().fold((0, 0), |usage, binding| {
            (
                usage.0 + binding.runtime.runtime_bytes(),
                usage.1 + binding.runtime.transfer_capacity_bytes(),
            )
        })
    }

    pub fn last_indicator_work_rows(&self) -> usize {
        self.indicators
            .iter()
            .map(|binding| binding.runtime.last_work_rows())
            .sum()
    }

    /// Return one typed definition for each live indicator binding in creation/dependency order.
    pub fn indicator_bindings(&self) -> Vec<IndicatorBindingInfo> {
        self.indicators
            .iter()
            .map(|binding| IndicatorBindingInfo {
                binding_id: binding.outputs[0],
                kind: binding.kind.clone(),
                source: binding.source,
                volume_source: binding.volume_source,
                outputs: binding.outputs.clone(),
            })
            .collect()
    }

    /// The binding an output series belongs to, or `None` when `id` is not an indicator output
    /// (a plain series, an unknown/removed id, or a source series itself).
    pub fn indicator_info(&self, id: SeriesId) -> Option<IndicatorInfo> {
        self.indicators.iter().find_map(|binding| {
            binding
                .outputs
                .iter()
                .position(|&output| output == id)
                .map(|output_index| {
                    let (kind, period, deviation, parameters) = match binding.kind {
                        IndicatorKind::Sma { period } => (
                            "sma",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Ema { period } => (
                            "ema",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::EmaRibbon { periods } => (
                            "ema_ribbon",
                            periods[output_index],
                            None,
                            IndicatorParameters {
                                periods: Some(periods),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Bollinger { period, deviation } => (
                            "bollinger",
                            period,
                            Some(deviation),
                            IndicatorParameters {
                                period: Some(period),
                                deviation: Some(deviation),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Rsi { period } => (
                            "rsi",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        // MACD/Stochastic pack their second period into `deviation`.
                        IndicatorKind::Macd { fast, slow, signal } => (
                            "macd",
                            slow,
                            Some(signal as f64),
                            IndicatorParameters {
                                fast: Some(fast),
                                slow: Some(slow),
                                signal: Some(signal),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Stochastic { k_period, d_period } => (
                            "stochastic",
                            k_period,
                            Some(d_period as f64),
                            IndicatorParameters {
                                k_period: Some(k_period),
                                d_period: Some(d_period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Atr { period } => (
                            "atr",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Vwap => ("vwap", 0, None, IndicatorParameters::default()),
                        IndicatorKind::Wma { period } => (
                            "wma",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                    };
                    IndicatorInfo {
                        binding_id: binding.outputs[0],
                        kind,
                        parameters,
                        period,
                        deviation,
                        source: binding.source,
                        volume_source: binding.volume_source,
                        output_name: indicator_output_name(&binding.kind, output_index),
                        output_index,
                        output_count: binding.outputs.len(),
                    }
                })
        })
    }

    /// Add a Rust-native simple moving-average producer. The returned line series is owned by the
    /// engine and is recomputed whenever its source series changes.
    pub fn add_sma(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Sma { period }, None)
            .into_iter()
            .next()
    }

    /// Add a Rust-native exponential moving-average producer.
    pub fn add_ema(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Ema { period }, None)
            .into_iter()
            .next()
    }

    /// Add five exponential moving averages as one binding in fastest-to-slowest output order.
    pub fn add_ema_ribbon(
        &mut self,
        source: SeriesId,
        periods: [usize; nucleuscharts_indicators::MAX_OUTPUTS],
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::EmaRibbon { periods }, None)
    }

    /// Atomically update all periods of an EMA ribbon while retaining its output identities and
    /// presentation options. `id` may identify any output in the ribbon.
    pub fn set_ema_ribbon_periods(
        &mut self,
        id: SeriesId,
        periods: [usize; nucleuscharts_indicators::MAX_OUTPUTS],
    ) -> bool {
        if periods.contains(&0) {
            return false;
        }
        let Some(index) = self.indicators.iter().position(|binding| {
            matches!(binding.kind, IndicatorKind::EmaRibbon { .. }) && binding.outputs.contains(&id)
        }) else {
            return false;
        };
        let IndicatorKind::EmaRibbon { periods: previous } = self.indicators[index].kind else {
            unreachable!("binding kind checked above")
        };
        if periods == previous {
            return true;
        }

        let outputs = self.indicators[index].outputs.clone();
        for (output_index, &output) in outputs.iter().enumerate() {
            let previous_title = format!("EMA {}", previous[output_index]);
            if let Some(series) = self.series.iter_mut().find(|series| series.id == output) {
                if series.title == previous_title {
                    series.title = format!("EMA {}", periods[output_index]);
                }
            }
        }
        let kind = IndicatorKind::EmaRibbon { periods };
        self.indicators[index].kind = kind.clone();
        self.indicators[index].runtime = incremental_state(&kind);
        let changes = self.rebuild_indicator(index, 0, true);
        self.indicator_changes.clear();
        self.indicator_changes.extend(changes.into_iter().flatten());
        self.propagate_indicator_changes();
        self.sync_time_points();
        true
    }

    /// Add upper, middle, and lower Bollinger-band line series in that order.
    pub fn add_bollinger(
        &mut self,
        source: SeriesId,
        period: usize,
        deviation: f64,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Bollinger { period, deviation }, None)
    }

    /// Add a Wilder RSI line in its own oscillator pane (with dotted 30/70 band lines).
    pub fn add_rsi(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Rsi { period }, None)
            .into_iter()
            .next()
    }

    /// Add MACD line, signal line, and histogram series in that order, in their own
    /// oscillator pane. The histogram is a Histogram-kind series whose per-bar color follows
    /// the four TradingView states (strong/weak × above/below zero).
    pub fn add_macd(
        &mut self,
        source: SeriesId,
        fast: usize,
        slow: usize,
        signal: usize,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Macd { fast, slow, signal }, None)
    }

    /// Add Stochastic %K and %D lines in that order, in their own oscillator pane (with
    /// dotted 20/80 band lines).
    pub fn add_stochastic(
        &mut self,
        source: SeriesId,
        k_period: usize,
        d_period: usize,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::Stochastic { k_period, d_period },
            None,
        )
    }

    /// Add a Wilder ATR line in its own oscillator pane.
    pub fn add_atr(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Atr { period }, None)
            .into_iter()
            .next()
    }

    /// Add a session-anchored (UTC-day reset) VWAP line on the source's pane.
    /// `volume_source` supplies the per-bar volume column (its close slot); `None` = unit
    /// weights.
    pub fn add_vwap(
        &mut self,
        source: SeriesId,
        volume_source: Option<SeriesId>,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Vwap, volume_source)
            .into_iter()
            .next()
    }

    /// Add a weighted moving-average line (linear weights, recent heaviest) on the source's pane.
    pub fn add_wma(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Wma { period }, None)
            .into_iter()
            .next()
    }

    /// Add an indicator from its typed definition, applying the same output, pane, and chrome
    /// defaults as the specialized convenience methods. Invalid definitions return no outputs and
    /// leave the chart unchanged.
    pub fn add_indicator_kind(
        &mut self,
        source: SeriesId,
        kind: IndicatorKind,
        volume_source: Option<SeriesId>,
    ) -> Vec<SeriesId> {
        let ids = self.add_indicator(source, kind.clone(), volume_source);
        match kind {
            IndicatorKind::Rsi { .. } => {
                if let Some(&id) = ids.first() {
                    self.place_outputs_in_oscillator_pane(&ids);
                    self.add_band_levels(id, &[30.0, 70.0]);
                }
            }
            IndicatorKind::Macd { .. } => {
                if let Some(&histogram) = ids.get(2) {
                    self.convert_series_kind(histogram, SeriesKind::Histogram);
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::Stochastic { .. } => {
                if let Some(&k) = ids.first() {
                    self.place_outputs_in_oscillator_pane(&ids);
                    self.add_band_levels(k, &[20.0, 80.0]);
                }
            }
            IndicatorKind::Atr { .. } => {
                if !ids.is_empty() {
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::Sma { .. }
            | IndicatorKind::Ema { .. }
            | IndicatorKind::EmaRibbon { .. }
            | IndicatorKind::Bollinger { .. }
            | IndicatorKind::Vwap
            | IndicatorKind::Wma { .. } => {}
        }
        ids
    }

    /// Move output series into a fresh oscillator pane below everything (TradingView
    /// separate-pane default, reduced stretch).
    fn place_outputs_in_oscillator_pane(&mut self, ids: &[SeriesId]) {
        let Some(pane) = self.add_pane(false) else {
            return;
        };
        if let Some(p) = self.panes.get_mut(pane) {
            p.stretch_factor = OSCILLATOR_PANE_STRETCH;
        }
        for &id in ids {
            self.set_series_pane(id, pane, OSCILLATOR_PANE_STRETCH);
        }
    }

    /// Dotted muted band lines (RSI 30/70, Stochastic 20/80) without axis labels — default
    /// oscillator chrome; platforms restyle or replace via their own primitives.
    fn add_band_levels(&mut self, id: SeriesId, levels: &[f64]) {
        for &price in levels {
            let line_id =
                self.create_price_line(id, price, BAND_LEVEL_COLOR, 1, LineStyle::Dotted, "");
            if let Some(s) = self.series.iter_mut().find(|s| s.id == id) {
                if let Some(line) = s.price_lines.iter_mut().find(|l| l.id == line_id) {
                    line.axis_label_visible = false;
                }
            }
        }
    }

    /// The bollinger band-fill companion for an output series: when `id` is a bollinger UPPER
    /// (output slot 0), the LOWER series (slot 2) the fill closes toward, else `None`. The
    /// frame builder paints the fill between them under the band strokes (TradingView's
    /// background fill).
    pub(crate) fn bollinger_fill_companion(&self, id: SeriesId) -> Option<SeriesId> {
        self.indicators.iter().find_map(|binding| {
            if matches!(binding.kind, IndicatorKind::Bollinger { .. })
                && binding.outputs.first() == Some(&id)
            {
                binding.outputs.get(2).copied()
            } else {
                None
            }
        })
    }

    /// The oscillator channel band `(lower, upper)` in price units when `id` is the primary
    /// output (slot 0) of an RSI (30/70) or Stochastic (20/80) binding — the frame builder
    /// paints a translucent band between them across the pane.
    pub(crate) fn oscillator_channel(&self, id: SeriesId) -> Option<(f64, f64)> {
        self.indicators.iter().find_map(|binding| {
            if binding.outputs.first() != Some(&id) {
                return None;
            }
            match binding.kind {
                IndicatorKind::Rsi { .. } => Some((30.0, 70.0)),
                IndicatorKind::Stochastic { .. } => Some((20.0, 80.0)),
                _ => None,
            }
        })
    }

    /// Drop every indicator binding that reads from or writes to `id`, returning the output series
    /// ids those bindings owned so the caller can tombstone them alongside `id`. Used by
    /// `remove_series`: removing a source drops its derived indicators; removing an indicator's own
    /// output series drops the whole binding (and its sibling outputs).
    pub(crate) fn drop_indicators_touching(&mut self, id: SeriesId) -> Vec<SeriesId> {
        let mut dropped_outputs = Vec::new();
        self.indicators.retain(|binding| {
            let touches_removed = binding.source == id
                || binding.volume_source == Some(id)
                || binding.outputs.contains(&id)
                || dropped_outputs.contains(&binding.source)
                || binding
                    .volume_source
                    .is_some_and(|source| dropped_outputs.contains(&source));
            if touches_removed {
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
        volume_source: Option<SeriesId>,
    ) -> Vec<SeriesId> {
        if self.series_entry(source).is_none()
            || match &kind {
                IndicatorKind::Vwap => {
                    volume_source.is_some_and(|id| self.series_entry(id).is_none())
                }
                _ => volume_source.is_some(),
            }
            || match &kind {
                IndicatorKind::Sma { period }
                | IndicatorKind::Ema { period }
                | IndicatorKind::Bollinger { period, .. }
                | IndicatorKind::Rsi { period }
                | IndicatorKind::Atr { period }
                | IndicatorKind::Wma { period } => *period == 0,
                IndicatorKind::EmaRibbon { periods } => periods.contains(&0),
                IndicatorKind::Macd { fast, slow, signal } => {
                    *fast == 0 || *slow == 0 || *signal == 0
                }
                IndicatorKind::Stochastic { k_period, d_period } => {
                    *k_period == 0 || *d_period == 0
                }
                IndicatorKind::Vwap => false,
            }
        {
            return Vec::new();
        }
        let runtime = incremental_state(&kind);
        let output_count = runtime.output_count();
        let source_price_format = self.series_entry(source).map(|series| {
            (
                series.price_format.kind,
                series.price_format.precision,
                series.price_format.min_move,
            )
        });
        let ids = (0..output_count)
            .map(|_| self.add_series(SeriesKind::Line))
            .collect::<Vec<_>>();
        // Indicator chrome defaults: no candle-close countdown (theirs is a line value, not a
        // bar close), the auto-generated name chip shows (platforms override the name through
        // the series `title` option — custom-script indicators will set their own), and the
        // line draws at 2px — every default is overridable through the ordinary series options.
        for (output_index, &id) in ids.iter().enumerate() {
            if let Some(s) = self.series.iter_mut().find(|s| s.id == id) {
                s.countdown_visible = false;
                s.title_visible = true;
                s.title = indicator_output_title(&kind, output_index);
                s.line_width = Some(2.0);
                if let Some(color) = indicator_output_color(&kind, output_index) {
                    s.line_color = Some(color.to_string());
                }
                if let Some((kind, precision, min_move)) = source_price_format {
                    s.price_format.kind = kind;
                    s.price_format.precision = precision;
                    s.price_format.min_move = min_move;
                }
            }
        }
        self.indicators.push(IndicatorBinding {
            source,
            runtime,
            kind,
            outputs: ids.clone(),
            volume_source,
            source_generation: 0,
            volume_generation: None,
        });
        self.rebuild_indicator(self.indicators.len() - 1, 0, true);
        ids
    }

    pub(crate) fn recompute_indicators_for(&mut self, dependency: SeriesId) {
        self.indicator_changes.clear();
        self.indicator_changes.push((
            dependency,
            IndicatorChange {
                from: 0,
                previous_generation: 0,
                full_replace: true,
            },
        ));
        self.propagate_indicator_changes();
        self.sync_time_points();
    }

    pub(crate) fn update_indicators_after_change(
        &mut self,
        dependency: SeriesId,
        change: IndicatorChange,
    ) {
        self.indicator_changes.clear();
        self.indicator_changes.push((dependency, change));
        self.propagate_indicator_changes();
        self.sync_time_points();
    }

    fn propagate_indicator_changes(&mut self) {
        // Bindings are topological by construction: an indicator output must exist before it can
        // be selected as a later indicator's source. One forward pass therefore updates direct
        // dependencies and every downstream chain without repeatedly scanning the whole graph.
        for index in 0..self.indicators.len() {
            let update = {
                let binding = &self.indicators[index];
                self.indicator_changes
                    .iter()
                    .filter_map(|&(dependency, change)| {
                        let tracked = if binding.source == dependency {
                            binding.source_generation
                        } else if binding.volume_source == Some(dependency) {
                            binding.volume_generation.unwrap_or(0)
                        } else {
                            return None;
                        };
                        let stale = tracked != change.previous_generation;
                        Some((
                            if stale { 0 } else { change.from },
                            change.full_replace || stale,
                        ))
                    })
                    .reduce(|left, right| (left.0.min(right.0), left.1 || right.1))
            };
            if let Some((from, full_replace)) = update {
                let changes = self.rebuild_indicator(index, from, full_replace);
                self.indicator_changes.extend(changes.into_iter().flatten());
            }
        }
    }

    fn rebuild_indicator(
        &mut self,
        index: usize,
        from: usize,
        full_replace: bool,
    ) -> [Option<(SeriesId, IndicatorChange)>; nucleuscharts_indicators::MAX_OUTPUTS] {
        let mut changes = [None; nucleuscharts_indicators::MAX_OUTPUTS];
        let outputs: [Option<SeriesId>; nucleuscharts_indicators::MAX_OUTPUTS] =
            std::array::from_fn(|slot| self.indicators[index].outputs.get(slot).copied());
        for &output in outputs.iter().flatten() {
            self.invalidate_frame_series(output);
        }

        let source = self.indicators[index].source;
        let volume_source = self.indicators[index].volume_source;
        {
            let Some((times, values)) = self.data.series_data(source) else {
                return changes;
            };
            let volume = volume_source
                .and_then(|id| self.data.series_data(id))
                .map_or(&[][..], |(_, values)| values[3]);
            let binding = &mut self.indicators[index];
            binding.runtime.rebuild_from(
                nucleuscharts_indicators::IndicatorInput {
                    times,
                    high: values[1],
                    low: values[2],
                    close: values[3],
                    volume,
                },
                if full_replace { 0 } else { from },
            );
        }
        self.indicators[index].source_generation = self.data.series_generation(source).unwrap_or(0);
        self.indicators[index].volume_generation =
            volume_source.and_then(|id| self.data.series_generation(id));

        let mut full_histogram_colors = None;
        for (output_index, output) in outputs.iter().flatten().copied().enumerate() {
            let previous_generation = self.data.series_generation(output).unwrap_or(0);
            let source_from = self.indicators[index].runtime.output_from(output_index);

            let output_from = if full_replace {
                let values = self.indicators[index].runtime.take_output(output_index);
                if output_index == 2
                    && matches!(self.indicators[index].kind, IndicatorKind::Macd { .. })
                {
                    full_histogram_colors = Some(macd_histogram_colors(&values));
                }
                self.data
                    .set_single_data_aligned(output, source, source_from, values);
                0
            } else {
                let values = self.indicators[index].runtime.output(output_index);
                self.data
                    .update_single_aligned(output, source, source_from, values)
                    .expect("indicator output remains aligned to its source")
            };
            if self.data.series_generation(output).unwrap_or(0) != previous_generation {
                changes[output_index] = Some((
                    output,
                    IndicatorChange {
                        from: output_from,
                        previous_generation,
                        full_replace,
                    },
                ));
            }
        }

        if let IndicatorKind::Macd { slow, signal, .. } = self.indicators[index].kind {
            let histogram_id = outputs[2].unwrap();
            if let Some(colors) = full_histogram_colors {
                self.data
                    .set_point_colors(histogram_id, [Some(colors), None, None]);
            } else {
                let histogram = self.indicators[index].runtime.output(2);
                let first_histogram = slow.saturating_add(signal).saturating_sub(2);
                let source_from = self.indicators[index].runtime.output_from(2);
                let output_start = source_from.saturating_sub(first_histogram);
                let mut previous = output_start.checked_sub(1).and_then(|row| {
                    self.data
                        .series_data(histogram_id)
                        .and_then(|(_, values)| values[3].get(row).copied())
                });
                for (offset, &value) in histogram.iter().enumerate() {
                    let color = macd_histogram_color(value, previous);
                    self.data.set_point_color(
                        histogram_id,
                        PointColorChannel::Body,
                        output_start + offset,
                        color,
                    );
                    previous = Some(value);
                }
            }
        }
        self.indicators[index].runtime.release_transfer_capacity();
        changes
    }
}

fn macd_histogram_colors(values: &[f64]) -> Vec<u32> {
    let mut colors = Vec::with_capacity(values.len());
    let mut previous = None;
    for &value in values {
        colors.push(macd_histogram_color(value, previous));
        previous = Some(value);
    }
    colors
}

fn incremental_state(kind: &IndicatorKind) -> nucleuscharts_indicators::IncrementalState {
    match *kind {
        IndicatorKind::Sma { period } => nucleuscharts_indicators::IncrementalState::sma(period),
        IndicatorKind::Ema { period } => nucleuscharts_indicators::IncrementalState::ema(period),
        IndicatorKind::EmaRibbon { periods } => {
            nucleuscharts_indicators::IncrementalState::ema_ribbon(periods)
        }
        IndicatorKind::Bollinger { period, deviation } => {
            nucleuscharts_indicators::IncrementalState::bollinger(period, deviation)
        }
        IndicatorKind::Rsi { period } => nucleuscharts_indicators::IncrementalState::rsi(period),
        IndicatorKind::Macd { fast, slow, signal } => {
            nucleuscharts_indicators::IncrementalState::macd(fast, slow, signal)
        }
        IndicatorKind::Stochastic { k_period, d_period } => {
            nucleuscharts_indicators::IncrementalState::stochastic(k_period, d_period)
        }
        IndicatorKind::Atr { period } => nucleuscharts_indicators::IncrementalState::atr(period),
        IndicatorKind::Vwap => nucleuscharts_indicators::IncrementalState::vwap(),
        IndicatorKind::Wma { period } => nucleuscharts_indicators::IncrementalState::wma(period),
    }
}

fn macd_histogram_color(value: f64, previous: Option<f64>) -> u32 {
    let rising = previous.is_none_or(|previous| value >= previous);
    if value >= 0.0 {
        if rising {
            MACD_UP
        } else {
            MACD_UP_WEAK
        }
    } else if rising {
        MACD_DOWN_WEAK
    } else {
        MACD_DOWN
    }
}

/// The auto-generated indicator name behind the (hidden-by-default) name chip — what
/// TradingView shows in its indicator legend ("SMA 20", "MACD 12 26 9"). Platforms can read it
/// via the series options or override it with their own `title`.
fn indicator_title(kind: &IndicatorKind) -> String {
    let params = |d: f64| {
        if d.fract() == 0.0 {
            format!("{}", d as i64)
        } else {
            format!("{d}")
        }
    };
    match kind {
        IndicatorKind::Sma { period } => format!("SMA {period}"),
        IndicatorKind::Ema { period } => format!("EMA {period}"),
        IndicatorKind::EmaRibbon { periods } => format!(
            "EMA Ribbon {} {} {} {} {}",
            periods[0], periods[1], periods[2], periods[3], periods[4]
        ),
        IndicatorKind::Bollinger { period, deviation } => {
            format!("Bollinger {period} {}", params(*deviation))
        }
        IndicatorKind::Rsi { period } => format!("RSI {period}"),
        IndicatorKind::Macd { fast, slow, signal } => format!("MACD {fast} {slow} {signal}"),
        IndicatorKind::Stochastic { k_period, d_period } => {
            format!("Stochastic {k_period} {d_period}")
        }
        IndicatorKind::Atr { period } => format!("ATR {period}"),
        IndicatorKind::Vwap => "VWAP".to_string(),
        IndicatorKind::Wma { period } => format!("WMA {period}"),
    }
}

fn indicator_output_title(kind: &IndicatorKind, output_index: usize) -> String {
    match kind {
        IndicatorKind::EmaRibbon { periods } => format!("EMA {}", periods[output_index]),
        _ => indicator_title(kind),
    }
}

fn indicator_output_color(kind: &IndicatorKind, output_index: usize) -> Option<&'static str> {
    matches!(kind, IndicatorKind::EmaRibbon { .. }).then(|| EMA_RIBBON_DEFAULT_COLORS[output_index])
}

fn indicator_output_name(kind: &IndicatorKind, output_index: usize) -> &'static str {
    match kind {
        IndicatorKind::Sma { .. } => "SMA",
        IndicatorKind::Ema { .. } => "EMA",
        IndicatorKind::EmaRibbon { .. } => {
            ["EMA 1", "EMA 2", "EMA 3", "EMA 4", "EMA 5"][output_index]
        }
        IndicatorKind::Bollinger { .. } => ["Upper", "Basis", "Lower"][output_index],
        IndicatorKind::Rsi { .. } => "RSI",
        IndicatorKind::Macd { .. } => ["MACD", "Signal", "Histogram"][output_index],
        IndicatorKind::Stochastic { .. } => ["%K", "%D"][output_index],
        IndicatorKind::Atr { .. } => "ATR",
        IndicatorKind::Vwap => "VWAP",
        IndicatorKind::Wma { .. } => "WMA",
    }
}
