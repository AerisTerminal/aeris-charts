//! Engine-owned indicator producers (SMA/EMA/Bollinger).
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

#[derive(Clone, Debug)]
pub(crate) struct IndicatorBinding {
    source: SeriesId,
    kind: IndicatorKind,
    outputs: Vec<SeriesId>,
    /// Parallel volume column source (VWAP); `None` = unit weights.
    volume_source: Option<SeriesId>,
    last_source_len: usize,
    last_source_time: Option<i64>,
}

/// Stretch factor of the pane a separate-pane indicator creates for itself (TradingView
/// oscillators stack as a shorter strip under the price pane).
pub(crate) const OSCILLATOR_PANE_STRETCH: f64 = 0.3;

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
                        IndicatorKind::Rsi { period } => ("rsi", period, None),
                        // MACD/Stochastic pack their second period into `deviation`.
                        IndicatorKind::Macd { slow, signal, .. } => {
                            ("macd", slow, Some(signal as f64))
                        }
                        IndicatorKind::Stochastic { k_period, d_period } => {
                            ("stochastic", k_period, Some(d_period as f64))
                        }
                        IndicatorKind::Atr { period } => ("atr", period, None),
                        IndicatorKind::Vwap => ("vwap", 0, None),
                        IndicatorKind::Wma { period } => ("wma", period, None),
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
        self.add_indicator(source, IndicatorKind::Sma { period }, 1, None)
            .into_iter()
            .next()
    }

    /// Add a Rust-native exponential moving-average producer.
    pub fn add_ema(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator(source, IndicatorKind::Ema { period }, 1, None)
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
        self.add_indicator(
            source,
            IndicatorKind::Bollinger { period, deviation },
            3,
            None,
        )
    }

    /// Add a Wilder RSI line in its own oscillator pane (with dotted 30/70 band lines).
    pub fn add_rsi(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        let ids = self.add_indicator(source, IndicatorKind::Rsi { period }, 1, None);
        let &id = ids.first()?;
        self.place_outputs_in_oscillator_pane(&[id]);
        self.add_band_levels(id, &[30.0, 70.0]);
        Some(id)
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
        let ids = self.add_indicator(source, IndicatorKind::Macd { fast, slow, signal }, 3, None);
        if let Some(&histogram) = ids.get(2) {
            self.convert_series_kind(histogram, SeriesKind::Histogram);
            self.place_outputs_in_oscillator_pane(&ids);
        }
        ids
    }

    /// Add Stochastic %K and %D lines in that order, in their own oscillator pane (with
    /// dotted 20/80 band lines).
    pub fn add_stochastic(
        &mut self,
        source: SeriesId,
        k_period: usize,
        d_period: usize,
    ) -> Vec<SeriesId> {
        let ids = self.add_indicator(
            source,
            IndicatorKind::Stochastic { k_period, d_period },
            2,
            None,
        );
        if let Some(&k) = ids.first() {
            self.place_outputs_in_oscillator_pane(&ids);
            self.add_band_levels(k, &[20.0, 80.0]);
        }
        ids
    }

    /// Add a Wilder ATR line in its own oscillator pane.
    pub fn add_atr(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        let ids = self.add_indicator(source, IndicatorKind::Atr { period }, 1, None);
        let &id = ids.first()?;
        self.place_outputs_in_oscillator_pane(&[id]);
        Some(id)
    }

    /// Add a session-anchored (UTC-day reset) VWAP line on the source's pane.
    /// `volume_source` supplies the per-bar volume column (its close slot); `None` = unit
    /// weights.
    pub fn add_vwap(
        &mut self,
        source: SeriesId,
        volume_source: Option<SeriesId>,
    ) -> Option<SeriesId> {
        self.add_indicator(source, IndicatorKind::Vwap, 1, volume_source)
            .into_iter()
            .next()
    }

    /// Add a weighted moving-average line (linear weights, recent heaviest) on the source's pane.
    pub fn add_wma(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator(source, IndicatorKind::Wma { period }, 1, None)
            .into_iter()
            .next()
    }

    /// Move output series into a fresh oscillator pane below everything (TradingView
    /// separate-pane default, reduced stretch).
    fn place_outputs_in_oscillator_pane(&mut self, ids: &[SeriesId]) {
        let pane = self.add_pane(false);
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
        volume_source: Option<SeriesId>,
    ) -> Vec<SeriesId> {
        if source >= self.series.len()
            || outputs == 0
            || matches!(
                &kind,
                IndicatorKind::Sma { period: 0 }
                    | IndicatorKind::Ema { period: 0 }
                    | IndicatorKind::Bollinger { period: 0, .. }
                    | IndicatorKind::Rsi { period: 0 }
                    | IndicatorKind::Macd { fast: 0, .. }
                    | IndicatorKind::Macd { slow: 0, .. }
                    | IndicatorKind::Macd { signal: 0, .. }
                    | IndicatorKind::Stochastic { k_period: 0, .. }
                    | IndicatorKind::Stochastic { d_period: 0, .. }
                    | IndicatorKind::Atr { period: 0 }
                    | IndicatorKind::Wma { period: 0 }
            )
        {
            return Vec::new();
        }
        let source_price_format = self.series.get(source).map(|series| {
            (
                series.price_format.kind,
                series.price_format.precision,
                series.price_format.min_move,
            )
        });
        let ids = (0..outputs)
            .map(|_| self.add_series(SeriesKind::Line))
            .collect::<Vec<_>>();
        // Indicator chrome defaults: no candle-close countdown (theirs is a line value, not a
        // bar close), the auto-generated name chip shows (platforms override the name through
        // the series `title` option — custom-script indicators will set their own), and the
        // line draws at 1px — every default is overridable through the ordinary series options.
        let title = indicator_title(&kind);
        for &id in &ids {
            if let Some(s) = self.series.iter_mut().find(|s| s.id == id) {
                s.countdown_visible = false;
                s.title_visible = true;
                s.title = title.clone();
                s.line_width = Some(1.0);
                if let Some((kind, precision, min_move)) = source_price_format {
                    s.price_format.kind = kind;
                    s.price_format.precision = precision;
                    s.price_format.min_move = min_move;
                }
            }
        }
        self.indicators.push(IndicatorBinding {
            source,
            kind,
            outputs: ids.clone(),
            volume_source,
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
            // H/L-consuming kinds clone those columns too (kept empty otherwise).
            let (high, low) = if matches!(
                binding.kind,
                IndicatorKind::Stochastic { .. } | IndicatorKind::Atr { .. } | IndicatorKind::Vwap
            ) {
                (values[1].to_vec(), values[2].to_vec())
            } else {
                (Vec::new(), Vec::new())
            };
            match binding.kind {
                IndicatorKind::Sma { period } => {
                    let values = nucleuscharts_indicators::sma(&close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Ema { period } => {
                    let values = nucleuscharts_indicators::ema(&close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Bollinger { period, deviation } => {
                    let values = nucleuscharts_indicators::bollinger(&close, period, deviation);
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
                IndicatorKind::Rsi { period } => {
                    let values = nucleuscharts_indicators::rsi(&close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Macd { fast, slow, signal } => {
                    let points = nucleuscharts_indicators::macd(&close, fast, slow, signal);
                    let mut macd = Vec::with_capacity(points.len());
                    let mut signal_line = Vec::with_capacity(points.len());
                    let mut histogram = Vec::with_capacity(points.len());
                    for point in points {
                        macd.push(point.macd);
                        signal_line.push(point.signal);
                        histogram.push(point.histogram);
                    }
                    self.install_indicator_output(binding.outputs[0], &times, &macd);
                    self.install_indicator_output(binding.outputs[1], &times, &signal_line);
                    self.install_indicator_output(binding.outputs[2], &times, &histogram);
                    self.install_macd_histogram_colors(binding.outputs[2], &histogram);
                }
                IndicatorKind::Stochastic { k_period, d_period } => {
                    let points = nucleuscharts_indicators::stochastic(
                        &high, &low, &close, k_period, d_period,
                    );
                    let mut k = Vec::with_capacity(points.len());
                    let mut d = Vec::with_capacity(points.len());
                    for point in points {
                        k.push(point.k);
                        d.push(point.d);
                    }
                    self.install_indicator_output(binding.outputs[0], &times, &k);
                    self.install_indicator_output(binding.outputs[1], &times, &d);
                }
                IndicatorKind::Atr { period } => {
                    let values = nucleuscharts_indicators::atr(&high, &low, &close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Vwap => {
                    let volumes = binding
                        .volume_source
                        .and_then(|id| self.data.series_data(id))
                        .map(|(_, v)| v[3].to_vec())
                        .unwrap_or_default();
                    let values =
                        nucleuscharts_indicators::vwap(&times, &high, &low, &close, &volumes);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
                IndicatorKind::Wma { period } => {
                    let values = nucleuscharts_indicators::wma(&close, period);
                    self.install_indicator_output(binding.outputs[0], &times, &values);
                }
            }
            self.indicators[index].last_source_len = times.len();
            self.indicators[index].last_source_time = times.last().copied();
        }
        self.sync_time_points();
    }

    /// Per-bar MACD histogram colors (four TradingView states): strong when the bar moves away
    /// from zero, weak when it falls back toward it. Installed AFTER the data (a data install
    /// resets point colors), one entry per installed (non-warm-up) row.
    fn install_macd_histogram_colors(&mut self, id: SeriesId, values: &[Option<f64>]) {
        let mut colors = Vec::new();
        let mut previous: Option<f64> = None;
        for &value in values.iter().flatten() {
            let rising = previous.is_none_or(|p| value >= p);
            colors.push(if value >= 0.0 {
                if rising {
                    MACD_UP
                } else {
                    MACD_UP_WEAK
                }
            } else if rising {
                MACD_DOWN_WEAK
            } else {
                MACD_DOWN
            });
            previous = Some(value);
        }
        self.data.set_point_colors(id, [Some(colors), None, None]);
    }

    pub(crate) fn update_indicators_after_source_update(&mut self, source: SeriesId, time: i64) {
        for index in 0..self.indicators.len() {
            if self.indicators[index].source != source {
                continue;
            }
            let binding = self.indicators[index].clone();
            // Only SMA/EMA/Bollinger have an incremental tail path; every other kind recomputes
            // in full on each source update (correct first, cheap at column scale).
            if !matches!(
                binding.kind,
                IndicatorKind::Sma { .. }
                    | IndicatorKind::Ema { .. }
                    | IndicatorKind::Bollinger { .. }
            ) {
                self.recompute_indicators();
                return;
            }
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
                // Non-incremental kinds exited via the full-recompute guard above.
                _ => unreachable!("guarded to SMA/EMA/Bollinger above"),
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
