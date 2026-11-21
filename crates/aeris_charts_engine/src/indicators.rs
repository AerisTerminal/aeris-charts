//! Engine-owned indicator producers.
//!
//! Indicators are bound to a source series and recomputed on source updates; their outputs are
//! ordinary engine series (`aeris_charts_indicators` holds the pure math). Extracted from `lib.rs`.

use super::*;
use crate::custom_studies::{CustomBinding, CustomBindingState};
use aeris_charts_indicators::structure_studies::{StructureStudy, StructureStudyKind};
use aeris_charts_indicators::study_annotations::StudyAnnotations;
use aeris_charts_indicators::{SessionSource, SessionStudy, SessionStudyState};
use std::borrow::Cow;

/// Scalar source selected by a study.  The aggregate sources are calculated from the source
/// bar's OHLC columns without changing the canonical source series or duplicating its storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorInputSource {
    Open,
    High,
    Low,
    #[default]
    Close,
    Hl2,
    Hlc3,
    Ohlc4,
    Hlcc4,
}

impl IndicatorInputSource {
    pub const ALL: [Self; 8] = [
        Self::Open,
        Self::High,
        Self::Low,
        Self::Close,
        Self::Hl2,
        Self::Hlc3,
        Self::Ohlc4,
        Self::Hlcc4,
    ];
}

/// Typed parameter kinds exposed to hosts when building study editors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorParameterType {
    Integer,
    Number,
    Boolean,
    Source,
    Series,
    Choice,
}

/// Session policy selected by calendar-aware studies. No timezone is inferred from the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyCalendarPolicy {
    Utc,
    Host,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousPeriod {
    Day,
    Week,
    Month,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndicatorParameterDescriptor {
    pub name: String,
    pub parameter_type: IndicatorParameterType,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

impl IndicatorParameterDescriptor {
    /// A choice has an ordered, nonempty option list and a default from that list.
    pub fn choice(name: &str, default: &str, options: &[&str]) -> Option<Self> {
        if options.is_empty() || !options.contains(&default) {
            return None;
        }
        Some(Self {
            name: name.into(),
            parameter_type: IndicatorParameterType::Choice,
            default: serde_json::json!(default),
            min: None,
            max: None,
            options: Some(options.iter().map(|option| (*option).into()).collect()),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndicatorOutputDescriptor {
    pub name: String,
    pub index: usize,
    pub supports_style: bool,
}

/// Persistable presentation state for one output of an indicator binding.
///
/// The series store remains the owner of the live style. This compact snapshot is exposed with
/// the binding contract so a host can persist and restore a study without reconstructing styles
/// from indicator kind heuristics.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndicatorOutputStyle {
    pub visible: bool,
    pub line_color: Option<String>,
    pub line_width: Option<f64>,
    pub line_style: u8,
    pub point_markers: bool,
    pub up_color: Option<String>,
    pub down_color: Option<String>,
    pub area_top_color: Option<String>,
    pub area_bottom_color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndicatorSchema {
    pub revision: u32,
    pub kind: String,
    pub parameters: Vec<IndicatorParameterDescriptor>,
    pub outputs: Vec<IndicatorOutputDescriptor>,
}

pub const INDICATOR_SCHEMA_REVISION: u32 = 2;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndicatorKind {
    Sma {
        period: usize,
    },
    Ema {
        period: usize,
    },
    Vwma {
        period: usize,
    },
    Cci {
        period: usize,
    },
    StochasticRsi {
        rsi_period: usize,
        stochastic_period: usize,
    },
    Donchian {
        period: usize,
    },
    PivotPoints {
        variant: aeris_charts_indicators::PivotKind,
    },
    Keltner {
        period: usize,
        multiplier: f64,
    },
    AdxDmi {
        period: usize,
    },
    ParabolicSar,
    SuperTrend {
        period: usize,
        multiplier: f64,
    },
    Ichimoku,
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
    Obv,
    RelativeVolume {
        period: usize,
    },
    Cmf {
        period: usize,
    },
    Mfi {
        period: usize,
    },
    Volume {
        period: usize,
        /// Smooths the volume average exponentially instead of over a simple window. Earlier
        /// saved definitions carry no flag and keep the simple average.
        #[serde(default)]
        exponential: bool,
    },
    VwapBands {
        reset: aeris_charts_indicators::VwapReset,
        standard_deviation: f64,
        percent: f64,
    },
    SwingPoints {
        left: usize,
        right: usize,
    },
    SessionLevels {
        calendar: StudyCalendarPolicy,
    },
    PreviousPeriodLevels {
        period: PreviousPeriod,
        calendar: StudyCalendarPolicy,
    },
    OpeningRange {
        duration_seconds: u32,
        calendar: StudyCalendarPolicy,
    },
    Custom {
        type_id: String,
        version: u32,
        parameters: BTreeMap<String, serde_json::Value>,
        output_count: usize,
    },
}

impl IndicatorKind {
    /// Resolve the browser's legacy name/period/deviation schema query to an engine definition.
    /// The implicit (14, 2) query uses each study's canonical defaults. Legacy
    /// period/deviation overrides apply only to kinds whose mapping uses those arguments;
    /// multi-period kinds with fixed canonical parameters ignore the period argument.
    pub fn schema_definition(kind: &str, period: usize, deviation: f64) -> Option<Self> {
        use aeris_charts_indicators::{PivotKind, VwapReset};
        Some(match kind {
            "swing_points" => Self::SwingPoints { left: 5, right: 5 },
            "session_levels" => Self::SessionLevels {
                calendar: StudyCalendarPolicy::Utc,
            },
            "previous_period_levels" => Self::PreviousPeriodLevels {
                period: PreviousPeriod::Day,
                calendar: StudyCalendarPolicy::Utc,
            },
            "opening_range" => Self::OpeningRange {
                duration_seconds: 1800,
                calendar: StudyCalendarPolicy::Utc,
            },
            "sma" => Self::Sma { period },
            "ema" => Self::Ema { period },
            "vwma" => Self::Vwma { period },
            "cci" => Self::Cci { period },
            "stochastic_rsi" => Self::StochasticRsi {
                rsi_period: period,
                stochastic_period: period,
            },
            "donchian" => Self::Donchian { period },
            "pivot_points" => Self::PivotPoints {
                variant: match period {
                    1 => PivotKind::Standard,
                    2 => PivotKind::Fibonacci,
                    3 => PivotKind::Camarilla,
                    4 => PivotKind::Woodie,
                    5 => PivotKind::DeMark,
                    _ => return None,
                },
            },
            "keltner" => Self::Keltner {
                period,
                multiplier: deviation,
            },
            "adx_dmi" => Self::AdxDmi { period },
            "parabolic_sar" => Self::ParabolicSar,
            "supertrend" => Self::SuperTrend {
                period,
                // The legacy query's implicit 2.0 is not SuperTrend's own 3.0 default.
                multiplier: if deviation == 2.0 { 3.0 } else { deviation },
            },
            "ichimoku" => Self::Ichimoku,
            "bollinger" => Self::Bollinger { period, deviation },
            "rsi" => Self::Rsi { period },
            "macd" => Self::Macd {
                fast: if period == 14 { 12 } else { period },
                slow: if period == 14 {
                    26
                } else {
                    period.saturating_mul(2)
                },
                signal: if period == 14 { 9 } else { period },
            },
            "stochastic" => Self::Stochastic {
                k_period: period,
                d_period: if period == 14 { 3 } else { period },
            },
            "atr" => Self::Atr { period },
            "vwap" => Self::Vwap,
            "obv" => Self::Obv,
            "relative_volume" => Self::RelativeVolume { period },
            "cmf" => Self::Cmf { period },
            "mfi" => Self::Mfi { period },
            "volume" => Self::Volume {
                period,
                exponential: false,
            },
            "vwap_bands" => Self::VwapBands {
                reset: VwapReset::Session,
                // The legacy TS query supplies 2.0 even with no arguments, whereas the
                // study's own default is 1.0. Other values remain explicit overrides.
                standard_deviation: if deviation == 2.0 { 1.0 } else { deviation },
                percent: 10.0,
            },
            _ => return None,
        })
    }
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
    pub source_input: IndicatorInputSource,
    /// Parallel volume column source for VWAP; `None` means unit weights.
    pub volume_source: Option<SeriesId>,
    /// Output identities in the indicator's documented order.
    pub outputs: Vec<SeriesId>,
    /// Per-output presentation snapshots in the same order as `outputs`.
    pub styles: Vec<IndicatorOutputStyle>,
}

pub(crate) struct IndicatorBinding {
    pub(crate) source: SeriesId,
    pub(crate) source_input: IndicatorInputSource,
    pub(crate) kind: IndicatorKind,
    pub(crate) outputs: Vec<SeriesId>,
    /// Parallel volume column source (VWAP); `None` = unit weights.
    pub(crate) volume_source: Option<SeriesId>,
    /// Structural-study geometry is binding-owned, never a synthetic output series.
    pub(crate) annotations: Option<StudyAnnotations>,
    /// Incremental OHLC scanner for structure studies; its history is not persisted.
    pub(crate) structure: Option<StructureStudy>,
    pub(crate) session: Option<SessionStudyState>,
    pub(crate) calendar: Option<StudyCalendarPolicy>,
    pub(crate) runtime: BindingRuntime,
    pub(crate) source_generation: u64,
    pub(crate) volume_generation: Option<u64>,
    pub(crate) inputs: BindingInputs,
    /// Sorted UTC timestamps where a new period/session begins for segmented studies
    /// (Pivot Points, Session Levels, Previous Period Levels, Opening Range, VWAP Bands).
    pub(crate) segment_break_times: Vec<i64>,
}

/// Fill policy between an indicator output and its companion lower/opposite band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BandFillMode {
    /// Uniform translucent fill using the upper output's resolved color at `alpha`.
    Uniform { alpha: f64 },
    /// Two-tone cloud fill (e.g. Ichimoku Kumo), splitting cleanly at exact crossover intersections.
    Directional { bullish: Color, bearish: Color },
}

/// Shared engine descriptor for a band or cloud fill emitted underneath the strokes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BandFillSpec {
    pub(crate) companion: SeriesId,
    pub(crate) mode: BandFillMode,
}

/// A binding's derived price input (HL2 and the other averages) and its volume aligned to the
/// source timestamps. Both persist between updates and are rewritten from the first changed source
/// row, so a live tick never rebuilds a full-length column.
#[derive(Default)]
pub(crate) struct BindingInputs {
    derived: Vec<f64>,
    volume: Vec<f64>,
    /// First source row with a finite selected input, when one exists.
    leading: Option<usize>,
    last_rows: usize,
}

impl BindingInputs {
    /// Rewrite rows from source row `from` on. Source and volume rows before `from` must be
    /// unchanged since the previous refresh. Source rows without a volume sample at their
    /// timestamp take `missing_volume`.
    pub(crate) fn refresh(
        &mut self,
        from: usize,
        source_input: IndicatorInputSource,
        times: &[i64],
        values: [&[f64]; 4],
        volume: Option<(&[i64], &[f64])>,
        missing_volume: f64,
    ) {
        let n = times.len();
        let from = from.min(n);
        self.last_rows = 0;
        if let Some(price) = derived_price(source_input) {
            self.derived.truncate(from);
            let start = self.derived.len();
            self.last_rows += n - start;
            self.derived.extend((start..n).map(|row| {
                price([
                    values[0][row],
                    values[1][row],
                    values[2][row],
                    values[3][row],
                ])
            }));
        } else if self.derived.capacity() > 0 {
            self.derived = Vec::new();
        }
        if let Some((volume_times, volume_values)) = volume {
            self.volume.truncate(from);
            let start = self.volume.len();
            self.last_rows += n - start;
            let mut volume_row = times.get(start).map_or(volume_times.len(), |&time| {
                volume_times.partition_point(|&volume_time| volume_time < time)
            });
            for &time in &times[start..] {
                while volume_times.get(volume_row).is_some_and(|&t| t < time) {
                    volume_row += 1;
                }
                let value = if volume_times.get(volume_row) == Some(&time) {
                    volume_values.get(volume_row).copied()
                } else {
                    None
                };
                self.volume.push(value.unwrap_or(missing_volume));
            }
        } else if self.volume.capacity() > 0 {
            self.volume = Vec::new();
        }
        if !self.leading.is_some_and(|leading| leading < from) {
            let close = self.close(source_input, values);
            self.leading = close.iter().position(|value| value.is_finite());
        }
    }

    pub(crate) fn close<'a>(
        &'a self,
        source_input: IndicatorInputSource,
        values: [&'a [f64]; 4],
    ) -> &'a [f64] {
        match source_input {
            IndicatorInputSource::Open => values[0],
            IndicatorInputSource::High => values[1],
            IndicatorInputSource::Low => values[2],
            IndicatorInputSource::Close => values[3],
            IndicatorInputSource::Hl2
            | IndicatorInputSource::Hlc3
            | IndicatorInputSource::Ohlc4
            | IndicatorInputSource::Hlcc4 => &self.derived,
        }
    }

    /// Volume per source row, or empty when the binding has no volume source.
    pub(crate) fn volume(&self) -> &[f64] {
        &self.volume
    }

    /// First source row with a finite selected input, or `None` when every row is a gap.
    pub(crate) fn leading(&self) -> Option<usize> {
        self.leading
    }

    /// Rows rewritten by the last refresh.
    pub(crate) fn last_rows(&self) -> usize {
        self.last_rows
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        (self.derived.capacity() + self.volume.capacity()) * std::mem::size_of::<f64>()
    }
}

fn derived_price(source: IndicatorInputSource) -> Option<fn([f64; 4]) -> f64> {
    match source {
        IndicatorInputSource::Open
        | IndicatorInputSource::High
        | IndicatorInputSource::Low
        | IndicatorInputSource::Close => None,
        IndicatorInputSource::Hl2 => Some(|[_, high, low, _]| (high + low) * 0.5),
        IndicatorInputSource::Hlc3 => Some(|[_, high, low, close]| (high + low + close) / 3.0),
        IndicatorInputSource::Ohlc4 => {
            Some(|[open, high, low, close]| (open + high + low + close) * 0.25)
        }
        IndicatorInputSource::Hlcc4 => {
            Some(|[_, high, low, close]| (high + low + 2.0 * close) * 0.25)
        }
    }
}

pub(crate) enum BindingRuntime {
    BuiltIn(Box<aeris_charts_indicators::IncrementalState>),
    Custom(CustomBinding),
}

impl BindingRuntime {
    pub(crate) fn custom(&self) -> Option<&CustomBinding> {
        match self {
            Self::Custom(custom) => Some(custom),
            Self::BuiltIn(_) => None,
        }
    }

    pub(crate) fn custom_mut(&mut self) -> Option<&mut CustomBinding> {
        match self {
            Self::Custom(custom) => Some(custom),
            Self::BuiltIn(_) => None,
        }
    }

    fn built_in(&mut self) -> &mut aeris_charts_indicators::IncrementalState {
        match self {
            Self::BuiltIn(runtime) => runtime.as_mut(),
            Self::Custom(_) => unreachable!("custom bindings are dispatched before built-in work"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct IndicatorChange {
    pub(crate) from: usize,
    pub(crate) previous_generation: u64,
    pub(crate) full_replace: bool,
}

/// Chart-wide chrome policy for engine-owned indicator bindings.
///
/// The engine retains this policy so newly-created and restored bindings cannot silently diverge
/// from existing outputs. Hosts choose the preference; they do not walk output series to enforce
/// it themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndicatorChromeOptions {
    pub name_labels_visible: bool,
    pub value_labels_visible: bool,
    pub price_lines_visible: bool,
}

/// Where an indicator binding draws relative to the series it is computed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorPane {
    /// Over the source's pane on its overlay scale, without a price axis of its own. Volume
    /// keeps to the bottom fifth of the pane so it never covers the bars it was traded in.
    Source,
    /// In a pane of its own below the chart.
    Separate,
}

impl Default for IndicatorChromeOptions {
    fn default() -> Self {
        Self {
            name_labels_visible: true,
            value_labels_visible: true,
            // A last-value line per indicator plot crowds the price pane; hosts opt in.
            price_lines_visible: false,
        }
    }
}

/// Share of the source pane kept clear above volume drawn over it.
const VOLUME_OVERLAY_TOP_MARGIN: f64 = 0.8;

/// An indicator output series' lineage: which binding it belongs to (kind + params), the
/// source series it derives from, and which output slot it is (Bollinger: 0 = upper,
/// 1 = middle, 2 = lower; SMA/EMA: always 0). Platforms read this to render their own
/// indicator chrome (legend chips, counts, settings) without the engine owning any UI.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct IndicatorInfo {
    /// Stable identity shared by every output of this binding. The first output's opaque series
    /// identity is safe because output identities are monotonic and the binding owns all outputs.
    pub binding_id: SeriesId,
    pub kind: Cow<'static, str>,
    pub parameters: IndicatorParameters,
    pub period: usize,
    pub deviation: Option<f64>,
    pub source: SeriesId,
    pub source_input: IndicatorInputSource,
    pub volume_source: Option<SeriesId>,
    /// Current engine-owned presentation state for this output.
    pub style: IndicatorOutputStyle,
    pub output_name: Cow<'static, str>,
    /// The output's display label in the chart's [`ChartLanguage`] (see
    /// [`indicator_output_label`]), recomputed on every query.
    pub output_label: Cow<'static, str>,
    pub output_index: usize,
    pub output_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct IndicatorParameters {
    pub calendar: Option<StudyCalendarPolicy>,
    pub previous_period: Option<PreviousPeriod>,
    pub duration_seconds: Option<u32>,
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub period: Option<usize>,
    pub pivot_kind: Option<aeris_charts_indicators::PivotKind>,
    pub deviation: Option<f64>,
    pub fast: Option<usize>,
    pub slow: Option<usize>,
    pub signal: Option<usize>,
    pub k_period: Option<usize>,
    pub d_period: Option<usize>,
    pub reset: Option<aeris_charts_indicators::VwapReset>,
    pub standard_deviation: Option<f64>,
    pub percent: Option<f64>,
    pub exponential: Option<bool>,
    pub custom: Option<BTreeMap<String, serde_json::Value>>,
}

fn indicator_default_line_width(kind: &IndicatorKind, output_index: usize) -> f64 {
    // The volume average and cloud/band guide edges are thin guides, like EMA.
    if matches!(kind, IndicatorKind::Volume { .. }) && output_index == 1 {
        return 1.0;
    }
    if matches!(kind, IndicatorKind::Ichimoku) && matches!(output_index, 2 | 3) {
        return 1.0;
    }
    if matches!(kind, IndicatorKind::VwapBands { .. }) && output_index > 0 {
        return 1.0;
    }
    if matches!(kind, IndicatorKind::Ema { .. }) {
        1.0
    } else {
        2.0
    }
}

/// Deterministic MA palette cycle (slot 0 keeps canonical `DEFAULT_LINE_COLOR` so single-MA
/// charts stay consistent; subsequent MAs step through distinct high-contrast hues).
const MA_PALETTE: [SeriesDefaultColor; 6] = [
    SeriesDefaultColor::Fixed(crate::DEFAULT_LINE_COLOR),
    SeriesDefaultColor::Themed {
        dark: Color::rgb(0xff, 0x98, 0x00),
        light: Color::rgb(0xe6, 0x51, 0x00),
    },
    SeriesDefaultColor::Themed {
        dark: Color::rgb(0xab, 0x47, 0xbc),
        light: Color::rgb(0x8e, 0x24, 0xaa),
    },
    SeriesDefaultColor::Themed {
        dark: Color::rgb(0x00, 0xbc, 0xd4),
        light: Color::rgb(0x00, 0x83, 0x8f),
    },
    SeriesDefaultColor::Themed {
        dark: Color::rgb(0xec, 0x40, 0x7a),
        light: Color::rgb(0xc2, 0x18, 0x5b),
    },
    SeriesDefaultColor::Themed {
        dark: Color::rgb(0x66, 0xbb, 0x6a),
        light: Color::rgb(0x2e, 0x7d, 0x32),
    },
];

fn is_moving_average_kind(kind: &IndicatorKind) -> bool {
    matches!(
        kind,
        IndicatorKind::Sma { .. } | IndicatorKind::Ema { .. } | IndicatorKind::Vwma { .. }
    )
}

fn indicator_default_color(
    kind: &IndicatorKind,
    output_index: usize,
    ma_cycle_index: usize,
) -> (Option<MarketTone>, Option<SeriesDefaultColor>) {
    const AMBER: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0xff, 0x98, 0x00),
        light: Color::rgb(0xe6, 0x51, 0x00),
    };
    const GOLD: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0xff, 0xca, 0x28),
        light: Color::rgb(0xd9, 0x77, 0x06),
    };
    const PURPLE: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0xab, 0x47, 0xbc),
        light: Color::rgb(0x8e, 0x24, 0xaa),
    };
    const CYAN: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0x26, 0xc6, 0xda),
        light: Color::rgb(0x00, 0x83, 0x8f),
    };
    const SKY: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0x29, 0xb6, 0xf6),
        light: Color::rgb(0x02, 0x77, 0xbd),
    };
    const TEAL: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0x26, 0xa6, 0x9a),
        light: Color::rgb(0x00, 0x79, 0x6b),
    };
    const CORAL: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0xff, 0x70, 0x43),
        light: Color::rgb(0xd8, 0x43, 0x15),
    };
    const SLATE: SeriesDefaultColor = SeriesDefaultColor::Themed {
        dark: Color::rgb(0x90, 0xa4, 0xae),
        light: Color::rgb(0x54, 0x6e, 0x7a),
    };
    const ROYAL_BLUE: SeriesDefaultColor = SeriesDefaultColor::Fixed(Color::rgb(0x29, 0x62, 0xff));
    const CRIMSON: SeriesDefaultColor = SeriesDefaultColor::Fixed(Color::rgb(0xef, 0x53, 0x50));
    match kind {
        IndicatorKind::Sma { .. } | IndicatorKind::Ema { .. } | IndicatorKind::Vwma { .. } => {
            (None, Some(MA_PALETTE[ma_cycle_index % MA_PALETTE.len()]))
        }
        IndicatorKind::Bollinger { .. } => match output_index {
            0 | 2 => (
                None,
                Some(SeriesDefaultColor::Fixed(crate::DEFAULT_LINE_COLOR)),
            ),
            1 => (None, Some(AMBER)),
            _ => (None, None),
        },
        IndicatorKind::Keltner { .. } => match output_index {
            0 | 2 => (None, Some(TEAL)),
            1 => (None, Some(AMBER)),
            _ => (None, None),
        },
        IndicatorKind::Donchian { .. } => match output_index {
            0 | 2 => (None, Some(SKY)),
            1 => (None, Some(SLATE)),
            _ => (None, None),
        },
        IndicatorKind::Macd { .. } => match output_index {
            0 => (None, Some(ROYAL_BLUE)),
            1 => (None, Some(AMBER)),
            _ => (None, None),
        },
        IndicatorKind::Stochastic { .. } => match output_index {
            0 => (None, Some(ROYAL_BLUE)),
            1 => (None, Some(AMBER)),
            _ => (None, None),
        },
        IndicatorKind::StochasticRsi { .. } => (None, Some(ROYAL_BLUE)),
        IndicatorKind::Rsi { .. } => (None, Some(PURPLE)),
        IndicatorKind::Cci { .. } => (None, Some(CYAN)),
        IndicatorKind::Atr { .. } => (None, Some(CORAL)),
        IndicatorKind::AdxDmi { .. } => match output_index {
            0 => (Some(MarketTone::Up), None),
            1 => (Some(MarketTone::Down), None),
            2 => (None, Some(GOLD)),
            _ => (None, None),
        },
        IndicatorKind::Ichimoku => match output_index {
            0 => (None, Some(ROYAL_BLUE)),
            1 => (None, Some(CRIMSON)),
            2 => (Some(MarketTone::Up), None),
            3 => (Some(MarketTone::Down), None),
            4 => (None, Some(PURPLE)),
            _ => (None, None),
        },
        IndicatorKind::PivotPoints { .. } => match output_index {
            0 => (None, Some(GOLD)),
            1 | 3 => (Some(MarketTone::Down), None),
            2 | 4 => (Some(MarketTone::Up), None),
            _ => (None, None),
        },
        IndicatorKind::SessionLevels { .. } | IndicatorKind::SwingPoints { .. } => {
            match output_index {
                0 => (Some(MarketTone::Up), None),
                1 => (Some(MarketTone::Down), None),
                _ => (None, None),
            }
        }
        IndicatorKind::PreviousPeriodLevels { .. } => match output_index {
            0 => (Some(MarketTone::Up), None),
            1 => (Some(MarketTone::Down), None),
            2 => (None, Some(GOLD)),
            _ => (None, None),
        },
        IndicatorKind::OpeningRange { .. } => match output_index {
            0 | 1 => (None, Some(SKY)),
            2 => (None, Some(SLATE)),
            _ => (None, None),
        },
        IndicatorKind::Vwap => (None, Some(GOLD)),
        IndicatorKind::VwapBands { .. } => match output_index {
            0 => (None, Some(GOLD)),
            1 | 2 => (None, Some(SKY)),
            3 | 4 => (None, Some(TEAL)),
            _ => (None, None),
        },
        IndicatorKind::Volume { .. } => match output_index {
            1 => (None, Some(AMBER)),
            _ => (None, None),
        },
        IndicatorKind::Obv => (None, Some(CYAN)),
        IndicatorKind::RelativeVolume { .. } => (None, Some(SKY)),
        IndicatorKind::Cmf { .. } => (Some(MarketTone::Up), None),
        IndicatorKind::Mfi { .. } => (None, Some(PURPLE)),
        IndicatorKind::SuperTrend { .. } => (Some(MarketTone::Up), None),
        IndicatorKind::ParabolicSar => (None, Some(GOLD)),
        IndicatorKind::Custom { .. } => (None, None),
    }
}

fn indicator_default_line_style(kind: &IndicatorKind, output_index: usize) -> u8 {
    match kind {
        IndicatorKind::Donchian { .. } if output_index == 1 => 2, // Dashed midline
        IndicatorKind::OpeningRange { .. } if output_index == 2 => 2, // Dashed OR mid
        IndicatorKind::PreviousPeriodLevels { .. } if output_index == 2 => 2, // Dashed prior close
        IndicatorKind::VwapBands { .. } if output_index >= 3 => 2, // Dashed % bands
        _ => 0,
    }
}

fn indicator_default_line_break_mode(kind: &IndicatorKind) -> LineBreakMode {
    match kind {
        IndicatorKind::SuperTrend { .. } => LineBreakMode::OnColorChange,
        IndicatorKind::PivotPoints { .. }
        | IndicatorKind::SessionLevels { .. }
        | IndicatorKind::PreviousPeriodLevels { .. }
        | IndicatorKind::OpeningRange { .. }
        | IndicatorKind::Vwap
        | IndicatorKind::VwapBands { .. } => LineBreakMode::OnSegmentBreak,
        _ => LineBreakMode::Continuous,
    }
}

fn indicator_default_bar_offset(kind: &IndicatorKind, output_index: usize) -> i32 {
    match kind {
        IndicatorKind::Ichimoku => match output_index {
            2 | 3 => 26,
            4 => -26,
            _ => 0,
        },
        _ => 0,
    }
}

fn indicator_default_level_tag(kind: &IndicatorKind, output_index: usize) -> Option<String> {
    let tag: &str = match kind {
        IndicatorKind::PivotPoints { .. } => ["P", "R1", "S1", "R2", "S2"].get(output_index)?,
        IndicatorKind::SessionLevels { .. } => ["SH", "SL"].get(output_index)?,
        IndicatorKind::PreviousPeriodLevels { period, .. } => match period {
            PreviousPeriod::Day => ["PDH", "PDL", "PDC"].get(output_index)?,
            PreviousPeriod::Week => ["PWH", "PWL", "PWC"].get(output_index)?,
            PreviousPeriod::Month => ["PMH", "PML", "PMC"].get(output_index)?,
        },
        IndicatorKind::OpeningRange { .. } => ["ORH", "ORL", "ORM"].get(output_index)?,
        _ => return None,
    };
    Some(tag.to_string())
}

fn indicator_default_reference_lines(
    kind: &IndicatorKind,
    output_index: usize,
) -> [Option<SeriesReferenceLine>; MAX_SERIES_REFERENCE_LINES] {
    let mut lines = [None; MAX_SERIES_REFERENCE_LINES];
    if output_index != 0 {
        return lines;
    }
    let dashed = |value: f64| {
        Some(SeriesReferenceLine {
            value,
            style: LineStyle::Dashed,
            color: None,
        })
    };
    match kind {
        IndicatorKind::Rsi { .. } => {
            lines[0] = dashed(50.0);
        }
        IndicatorKind::Macd { .. } | IndicatorKind::Cci { .. } | IndicatorKind::Cmf { .. } => {
            lines[0] = dashed(0.0);
        }
        IndicatorKind::AdxDmi { .. } => {
            lines[0] = dashed(25.0);
        }
        IndicatorKind::RelativeVolume { .. } => {
            lines[0] = dashed(1.0);
        }
        _ => {}
    }
    lines
}

fn utc_month_key(time_seconds: i64) -> i64 {
    let z = time_seconds.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(m <= 2);
    year * 12 + (m - 1)
}

fn host_session_key(spans: &[aeris_charts_indicators::SessionSpan], time: i64) -> Option<i64> {
    if spans.is_empty() {
        return None;
    }
    let idx = spans.partition_point(|span| span.start <= time);
    if idx == 0 {
        return None;
    }
    let mut group_idx = idx - 1;
    let span = spans[group_idx];
    if time < span.start || time >= span.end || span.start >= span.end {
        return None;
    }
    // Walk back across adjacent spans sharing the same session_id so merged pieces keep one key.
    while group_idx > 0 {
        let prev = spans[group_idx - 1];
        if prev.end == spans[group_idx].start
            && prev.session_id == span.session_id
            && prev.start < prev.end
        {
            group_idx -= 1;
        } else {
            break;
        }
    }
    Some(group_idx as i64)
}

fn segment_period_key(
    kind: &IndicatorKind,
    calendar: Option<StudyCalendarPolicy>,
    host_spans: &[aeris_charts_indicators::SessionSpan],
    time: i64,
) -> Option<i64> {
    let utc_day = |t: i64| t.div_euclid(86_400);
    let utc_week = |t: i64| (utc_day(t) + 3).div_euclid(7);
    match kind {
        IndicatorKind::PivotPoints { .. } => Some(utc_day(time)),
        IndicatorKind::SessionLevels { .. } | IndicatorKind::OpeningRange { .. } => {
            if calendar == Some(StudyCalendarPolicy::Host) {
                host_session_key(host_spans, time)
            } else {
                Some(utc_day(time))
            }
        }
        IndicatorKind::PreviousPeriodLevels { period, .. } => match period {
            PreviousPeriod::Day => {
                if calendar == Some(StudyCalendarPolicy::Host) {
                    host_session_key(host_spans, time)
                } else {
                    Some(utc_day(time))
                }
            }
            PreviousPeriod::Week => Some(utc_week(time)),
            PreviousPeriod::Month => Some(utc_month_key(time)),
        },
        IndicatorKind::Vwap => Some(utc_day(time)),
        IndicatorKind::VwapBands { reset, .. } => match reset {
            VwapReset::Session => Some(utc_day(time)),
            VwapReset::Weekly => Some(utc_week(time)),
            VwapReset::Monthly => Some(utc_month_key(time)),
        },
        _ => None,
    }
}

/// Incrementally refresh sorted UTC break timestamps from source row `from` onward.
/// O(1) on ordinary tip appends and replacements.
fn update_segment_break_times(
    breaks: &mut Vec<i64>,
    kind: &IndicatorKind,
    calendar: Option<StudyCalendarPolicy>,
    host_spans: &[aeris_charts_indicators::SessionSpan],
    times: &[i64],
    from: usize,
    full_replace: bool,
) {
    if indicator_default_line_break_mode(kind) != LineBreakMode::OnSegmentBreak {
        breaks.clear();
        return;
    }
    if times.is_empty() {
        breaks.clear();
        return;
    }
    let start = if full_replace {
        0
    } else {
        from.min(times.len())
    };
    if start == 0 {
        breaks.clear();
    } else if let Some(&cutoff) = times.get(start) {
        let keep = breaks.partition_point(|&t| t < cutoff);
        breaks.truncate(keep);
    }
    let mut prev_key = if start > 0 {
        segment_period_key(kind, calendar, host_spans, times[start - 1])
    } else {
        None
    };
    let scan_start = start.max(1);
    if start == 0 {
        prev_key = segment_period_key(kind, calendar, host_spans, times[0]);
    }
    for &time in &times[scan_start..] {
        let key = segment_period_key(kind, calendar, host_spans, time);
        if prev_key.is_some() && key != prev_key {
            breaks.push(time);
        }
        prev_key = key;
    }
}

impl ChartEngine {
    pub fn add_session_levels(
        &mut self,
        source: SeriesId,
        calendar: StudyCalendarPolicy,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::SessionLevels { calendar }, None)
    }

    pub fn add_previous_period_levels(
        &mut self,
        source: SeriesId,
        period: PreviousPeriod,
        calendar: StudyCalendarPolicy,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::PreviousPeriodLevels { period, calendar },
            None,
        )
    }

    pub fn add_opening_range(
        &mut self,
        source: SeriesId,
        duration_seconds: u32,
        calendar: StudyCalendarPolicy,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::OpeningRange {
                duration_seconds,
                calendar,
            },
            None,
        )
    }

    /// Add confirmed swing levels on the source price pane.
    pub fn add_swing_points(
        &mut self,
        source: SeriesId,
        left: usize,
        right: usize,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::SwingPoints { left, right }, None)
    }

    pub(crate) fn reset_indicator_output_styles_to_defaults(&mut self) {
        let outputs = self
            .indicators
            .iter()
            .map(|binding| (binding.kind.clone(), binding.outputs.clone()))
            .collect::<Vec<_>>();
        for (kind, ids) in outputs {
            for (output_index, id) in ids.into_iter().enumerate() {
                let custom_style = match &kind {
                    IndicatorKind::Custom {
                        type_id, version, ..
                    } => self
                        .custom_studies
                        .get(type_id)
                        .filter(|registration| registration.definition.version == *version)
                        .and_then(|registration| registration.definition.outputs.get(output_index))
                        .map(|output| output.default_style.clone()),
                    _ => None,
                };
                let Some(series) = self.series_entry_mut(id) else {
                    continue;
                };
                series.countdown_visible = false;
                series.line_width = Some(indicator_default_line_width(&kind, output_index));
                series.line_style = indicator_default_line_style(&kind, output_index);
                series.line_color = None;
                if matches!(kind, IndicatorKind::ParabolicSar) {
                    series.line_visible = false;
                    series.point_markers = true;
                    series.point_marker_shape = PointMarkerShape::Circle;
                    series.point_markers_radius = Some(2.5);
                }
                if matches!(kind, IndicatorKind::Cmf { .. }) {
                    series.baseline = Some(0.0);
                }
                if let Some(style) = custom_style {
                    series.visible = style.visible;
                    series.line_width = style.line_width;
                    series.line_color = style.line_color;
                    series.line_style = style.line_style;
                    series.point_markers = style.point_markers;
                    series.up_color = style.up_color;
                    series.down_color = style.down_color;
                    series.area_top_color = style.area_top_color;
                    series.area_bottom_color = style.area_bottom_color;
                }
            }
        }
    }

    /// Applies Aeris's canonical four-state momentum palette to an existing histogram series.
    ///
    /// Colors are derived from each value's sign and whether it moved toward or away from zero.
    /// The caller owns only the semantic request; Aeris retains palette and row-style ownership.
    pub fn apply_momentum_histogram_colors(&mut self, id: SeriesId) -> bool {
        if self
            .series_entry(id)
            .is_none_or(|series| series.kind != SeriesKind::Histogram)
        {
            return false;
        }
        let market = self.market_colors();
        let Some(colors) = self
            .data
            .series_data(id)
            .map(|(_, values)| momentum_histogram_colors(values[3], market))
        else {
            return false;
        };
        self.set_series_point_colors(id, Some(colors), None, None)
    }

    /// Per-point palettes are stored on the data layer, so a market-color change recolors every
    /// built-in momentum histogram, directional trend/SAR overlay, RVOL histogram, and external
    /// momentum histogram from its values.
    pub(crate) fn refresh_momentum_histogram_colors(&mut self) {
        let mut macd_ids = Vec::new();
        let mut rvol_ids = Vec::new();
        let mut directional_pairs = Vec::new();
        for binding in &self.indicators {
            match binding.kind {
                IndicatorKind::Macd { .. } => {
                    if let Some(&id) = binding.outputs.get(2) {
                        macd_ids.push(id);
                    }
                }
                IndicatorKind::RelativeVolume { .. } => {
                    if let Some(&id) = binding.outputs.first() {
                        rvol_ids.push(id);
                    }
                }
                IndicatorKind::SuperTrend { .. } | IndicatorKind::ParabolicSar => {
                    if let Some(&id) = binding.outputs.first() {
                        directional_pairs.push((id, binding.source));
                    }
                }
                _ => {}
            }
        }
        macd_ids.extend(self.external_momentum_histogram_series());
        for id in macd_ids {
            self.apply_momentum_histogram_colors(id);
        }
        let market = self.market_colors();
        for id in rvol_ids {
            if let Some((_, values)) = self.data.series_data(id) {
                let colors = rvol_histogram_colors(values[3], market);
                self.set_series_point_colors(id, Some(colors), None, None);
            }
        }
        for (output_id, source_id) in directional_pairs {
            if let (Some((out_times, out_vals)), Some((src_times, src_vals))) = (
                self.data.series_data(output_id),
                self.data.series_data(source_id),
            ) {
                let src_start = out_times
                    .first()
                    .and_then(|first| src_times.binary_search(first).ok())
                    .unwrap_or(0);
                let colors = directional_vs_close_colors(
                    out_vals[3],
                    src_vals[3].get(src_start..).unwrap_or(&[]),
                    market,
                );
                self.set_series_point_colors(output_id, Some(colors), None, None);
            }
        }
    }

    pub(crate) fn indicator_memory_usage(&self) -> (usize, usize) {
        self.indicators.iter().fold((0, 0), |usage, binding| {
            (
                usage.0
                    + match &binding.runtime {
                        BindingRuntime::BuiltIn(runtime) => runtime.runtime_bytes(),
                        BindingRuntime::Custom(_) => 0,
                    }
                    + binding
                        .structure
                        .as_ref()
                        .map_or(0, StructureStudy::capacity_bytes)
                    + binding
                        .session
                        .as_ref()
                        .map_or(0, SessionStudyState::capacity_bytes)
                    + binding
                        .annotations
                        .as_ref()
                        .map_or(0, StudyAnnotations::capacity_bytes)
                    + binding.inputs.capacity_bytes()
                    + binding.segment_break_times.capacity() * std::mem::size_of::<i64>(),
                usage.1
                    + match &binding.runtime {
                        BindingRuntime::BuiltIn(runtime) => runtime.transfer_capacity_bytes(),
                        BindingRuntime::Custom(_) => 0,
                    },
            )
        })
    }

    /// Source rows read by each binding's last update, including derived-input and aligned-volume
    /// rows it rewrote, summed over bindings.
    pub fn last_indicator_work_rows(&self) -> usize {
        self.indicators
            .iter()
            .map(|binding| {
                binding.inputs.last_rows()
                    + match &binding.runtime {
                        BindingRuntime::BuiltIn(runtime) => runtime.last_work_rows(),
                        BindingRuntime::Custom(_) => 0,
                    }
            })
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
                source_input: binding.source_input,
                volume_source: binding.volume_source,
                outputs: binding.outputs.clone(),
                styles: binding
                    .outputs
                    .iter()
                    .filter_map(|&id| self.series_entry(id).map(indicator_output_style))
                    .collect(),
            })
            .collect()
    }

    /// Snapshot the bounded annotation history of a structural study by its binding identity.
    ///
    /// An ordinary scalar binding has no annotation output; passing an output other than the
    /// binding's first output is not a binding identity.
    pub fn study_annotations(&self, binding: SeriesId) -> Result<StudyAnnotations, ChartError> {
        let producer = self
            .indicators
            .iter()
            .find(|producer| producer.outputs.first() == Some(&binding))
            .ok_or_else(|| ChartError::new(ErrorCode::InvalidHandle, "unknown study binding"))?;
        producer
            .annotations
            .as_ref()
            .or_else(|| producer.structure.as_ref().map(StructureStudy::annotations))
            .cloned()
            .ok_or_else(|| {
                ChartError::new(
                    ErrorCode::UnsupportedOperation,
                    "study binding has no structural annotations",
                )
            })
    }

    /// Install bounded test geometry without exposing an incomplete structural-study kind.
    #[cfg(test)]
    pub(crate) fn inject_study_annotations_for_test(
        &mut self,
        binding: SeriesId,
        annotations: StudyAnnotations,
    ) -> bool {
        let Some(producer) = self
            .indicators
            .iter_mut()
            .find(|producer| producer.outputs.first() == Some(&binding))
        else {
            return false;
        };
        producer.annotations = Some(annotations);
        let source = producer.source;
        let outputs = producer.outputs.clone();
        self.invalidate_frame_series(source);
        for output in outputs {
            self.invalidate_frame_series(output);
        }
        true
    }

    /// Whether the chart currently owns at least one live native indicator binding.
    #[must_use]
    pub fn has_indicator_bindings(&self) -> bool {
        !self.indicators.is_empty()
    }

    /// Remove every native indicator binding as one engine-owned operation.
    pub fn clear_indicator_bindings(&mut self) -> bool {
        let binding_ids = self
            .indicators
            .iter()
            .filter_map(|binding| binding.outputs.first().copied())
            .collect::<Vec<_>>();
        if binding_ids.is_empty() {
            return false;
        }
        // One undo brings every binding back.
        self.begin_history_group();
        for binding_id in binding_ids {
            let _ = self.remove_indicator_binding(binding_id);
        }
        self.end_history_group();
        true
    }

    /// Current chart-wide chrome policy inherited by every engine-owned indicator output.
    #[must_use]
    pub const fn indicator_chrome_options(&self) -> IndicatorChromeOptions {
        self.indicator_chrome
    }

    /// Apply one chart-wide indicator chrome policy to current and future bindings.
    pub fn set_indicator_chrome_options(&mut self, options: IndicatorChromeOptions) -> bool {
        let mut changed = self.indicator_chrome != options;
        self.indicator_chrome = options;
        let outputs = self
            .indicators
            .iter()
            .flat_map(|binding| binding.outputs.iter().copied())
            .collect::<Vec<_>>();
        for output in outputs {
            if let Some(series) = self.series_entry_mut(output) {
                changed |= series.title_visible != options.name_labels_visible
                    || series.last_value_visible != options.value_labels_visible
                    || series.price_line_visible != options.price_lines_visible;
                series.title_visible = options.name_labels_visible;
                series.last_value_visible = options.value_labels_visible;
                series.price_line_visible = options.price_lines_visible;
            }
        }
        changed |= self.apply_indicator_chrome_to_external_studies(options);
        changed |= self.apply_indicator_chrome_to_trade_studies(options);
        if changed {
            self.invalidate_frame_layout_and_axis();
        }
        changed
    }

    /// Set every output in one binding visible or hidden as one engine-owned operation.
    pub fn set_indicator_binding_visible(&mut self, binding_id: SeriesId, visible: bool) -> bool {
        let Some(outputs) = self
            .indicators
            .iter()
            .find(|binding| binding.outputs.first() == Some(&binding_id))
            .map(|binding| binding.outputs.clone())
        else {
            return false;
        };
        let changed = outputs.iter().any(|&output| {
            self.series_entry(output)
                .is_some_and(|series| series.visible != visible)
        });
        for output in outputs {
            self.set_series_visible(output, visible);
        }
        changed
    }

    /// Where one binding currently draws: over its source's pane or in a pane of its own.
    #[must_use]
    pub fn indicator_binding_pane(&self, binding_id: SeriesId) -> Option<IndicatorPane> {
        let binding = self
            .indicators
            .iter()
            .find(|binding| binding.outputs.first() == Some(&binding_id))?;
        let source_pane = self.series_entry(binding.source)?.pane_index;
        let output_pane = self.series_entry(binding_id)?.pane_index;
        Some(if output_pane == source_pane {
            IndicatorPane::Source
        } else {
            IndicatorPane::Separate
        })
    }

    /// Move every output of one binding over its source's pane or into a fresh pane of its own,
    /// as one engine-owned operation. A pane the outputs leave empty collapses. Returns whether
    /// the placement changed.
    pub fn set_indicator_binding_pane(
        &mut self,
        binding_id: SeriesId,
        pane: IndicatorPane,
    ) -> bool {
        let Some((source, outputs, volume)) = self
            .indicators
            .iter()
            .find(|binding| binding.outputs.first() == Some(&binding_id))
            .map(|binding| {
                (
                    binding.source,
                    binding.outputs.clone(),
                    matches!(binding.kind, IndicatorKind::Volume { .. }),
                )
            })
        else {
            return false;
        };
        if self.indicator_binding_pane(binding_id) == Some(pane) {
            return false;
        }
        match pane {
            IndicatorPane::Source => {
                for &output in &outputs {
                    // Collapsing the pane an output leaves renumbers the panes below it.
                    let Some(source_pane) = self.series_entry(source).map(|s| s.pane_index) else {
                        return false;
                    };
                    // The source pane exists, so no pane is created and no stretch applies.
                    if !self.try_set_series_pane(output, source_pane, None) {
                        return false;
                    }
                    self.set_series_price_scale(output, PriceScaleTarget::Overlay);
                }
                if volume && let Some(source_pane) = self.series_entry(source).map(|s| s.pane_index)
                {
                    self.set_price_scale_margins_for(
                        source_pane,
                        PriceScaleTarget::Overlay,
                        VOLUME_OVERLAY_TOP_MARGIN,
                        0.0,
                    );
                }
            }
            IndicatorPane::Separate => {
                self.place_outputs_in_oscillator_pane(&outputs);
                for &output in &outputs {
                    self.set_series_price_scale(output, PriceScaleTarget::Right);
                }
            }
        }
        self.invalidate_frame_layout_and_axis();
        self.indicator_binding_pane(binding_id) == Some(pane)
    }

    /// Remove one complete indicator binding by its stable binding identity.
    pub fn remove_indicator_binding(&mut self, binding_id: SeriesId) -> bool {
        if !self
            .indicators
            .iter()
            .any(|binding| binding.outputs.first() == Some(&binding_id))
        {
            return false;
        }
        self.remove_series(binding_id)
    }

    /// Remove the complete native indicator binding that owns one output series.
    pub fn remove_indicator_for_series(&mut self, series_id: SeriesId) -> bool {
        let Some(binding_id) = self.indicator_binding_id(series_id) else {
            return false;
        };
        self.remove_indicator_binding(binding_id)
    }

    /// Replace one output's presentation atomically while retaining the binding and output id.
    /// Invalid widths are rejected before any series state is changed.
    pub fn set_indicator_output_style(
        &mut self,
        output: SeriesId,
        style: IndicatorOutputStyle,
    ) -> bool {
        if !output_style_is_valid(&style)
            || !self
                .indicators
                .iter()
                .any(|binding| binding.outputs.contains(&output))
        {
            return false;
        }
        let visible = style.visible;
        self.write_output_style(output, style, visible)
    }

    /// Write a validated output style. `painted_visible` is the series' resulting visibility; it
    /// goes through `set_series_visible` so autoscale and axes stop following a hidden line.
    pub(crate) fn write_output_style(
        &mut self,
        output: SeriesId,
        style: IndicatorOutputStyle,
        painted_visible: bool,
    ) -> bool {
        let Some(series) = self.series_entry_mut(output) else {
            return false;
        };
        series.line_color = style.line_color;
        series.line_width = style.line_width;
        series.line_style = style.line_style;
        series.point_markers = style.point_markers;
        series.up_color = style.up_color;
        series.down_color = style.down_color;
        series.area_top_color = style.area_top_color;
        series.area_bottom_color = style.area_bottom_color;
        self.invalidate_frame_series(output);
        self.set_series_visible(output, painted_visible);
        true
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
                        IndicatorKind::Custom { ref parameters, .. } => (
                            "custom",
                            0,
                            None,
                            IndicatorParameters {
                                custom: Some(parameters.clone()),
                                ..Default::default()
                            },
                        ),
                        IndicatorKind::SwingPoints { left, right } => (
                            "swing_points",
                            0,
                            None,
                            IndicatorParameters {
                                left: Some(left),
                                right: Some(right),
                                ..Default::default()
                            },
                        ),
                        IndicatorKind::SessionLevels { calendar } => (
                            "session_levels",
                            0,
                            None,
                            IndicatorParameters {
                                calendar: Some(calendar),
                                ..Default::default()
                            },
                        ),
                        IndicatorKind::PreviousPeriodLevels { period, calendar } => (
                            "previous_period_levels",
                            0,
                            None,
                            IndicatorParameters {
                                calendar: Some(calendar),
                                previous_period: Some(period),
                                ..Default::default()
                            },
                        ),
                        IndicatorKind::OpeningRange {
                            duration_seconds,
                            calendar,
                        } => (
                            "opening_range",
                            0,
                            None,
                            IndicatorParameters {
                                calendar: Some(calendar),
                                duration_seconds: Some(duration_seconds),
                                ..Default::default()
                            },
                        ),
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
                        IndicatorKind::Vwma { period } => (
                            "vwma",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Cci { period } => (
                            "cci",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::StochasticRsi {
                            rsi_period,
                            stochastic_period,
                        } => (
                            "stochastic_rsi",
                            rsi_period,
                            Some(stochastic_period as f64),
                            IndicatorParameters {
                                period: Some(rsi_period),
                                k_period: Some(stochastic_period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Donchian { period } => (
                            "donchian",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::PivotPoints { variant } => (
                            "pivot_points",
                            0,
                            None,
                            IndicatorParameters {
                                pivot_kind: Some(variant),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Keltner { period, multiplier } => (
                            "keltner",
                            period,
                            Some(multiplier),
                            IndicatorParameters {
                                period: Some(period),
                                deviation: Some(multiplier),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::AdxDmi { period } => (
                            "adx_dmi",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::ParabolicSar => {
                            ("parabolic_sar", 0, None, IndicatorParameters::default())
                        }
                        IndicatorKind::SuperTrend { period, multiplier } => (
                            "supertrend",
                            period,
                            Some(multiplier),
                            IndicatorParameters {
                                period: Some(period),
                                deviation: Some(multiplier),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Ichimoku => {
                            ("ichimoku", 0, None, IndicatorParameters::default())
                        }
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
                        IndicatorKind::Obv => ("obv", 0, None, IndicatorParameters::default()),
                        IndicatorKind::RelativeVolume { period } => (
                            "relative_volume",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Cmf { period } => (
                            "cmf",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Mfi { period } => (
                            "mfi",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::Volume {
                            period,
                            exponential,
                        } => (
                            "volume",
                            period,
                            None,
                            IndicatorParameters {
                                period: Some(period),
                                exponential: Some(exponential),
                                ..IndicatorParameters::default()
                            },
                        ),
                        IndicatorKind::VwapBands {
                            reset,
                            standard_deviation,
                            percent,
                        } => (
                            "vwap_bands",
                            0,
                            None,
                            IndicatorParameters {
                                reset: Some(reset),
                                standard_deviation: Some(standard_deviation),
                                percent: Some(percent),
                                ..IndicatorParameters::default()
                            },
                        ),
                    };
                    let output_name = if let IndicatorKind::Custom {
                        ref type_id,
                        ref version,
                        ..
                    } = binding.kind
                    {
                        Cow::Owned(
                            self.custom_studies
                                .get(type_id)
                                .filter(|d| d.definition.version == *version)
                                .and_then(|d| d.definition.outputs.get(output_index))
                                .map_or_else(
                                    || format!("Output {}", output_index + 1),
                                    |o| o.name.clone(),
                                ),
                        )
                    } else {
                        Cow::Borrowed(indicator_output_name(&binding.kind, output_index))
                    };
                    // A custom study's output names are the host's own words in every language.
                    let output_label = if matches!(binding.kind, IndicatorKind::Custom { .. }) {
                        output_name.clone()
                    } else {
                        Cow::Owned(indicator_output_label(
                            &binding.kind,
                            output_index,
                            self.language,
                        ))
                    };
                    IndicatorInfo {
                        binding_id: binding.outputs[0],
                        kind: if let IndicatorKind::Custom { ref type_id, .. } = binding.kind {
                            Cow::Owned(type_id.clone())
                        } else {
                            Cow::Borrowed(kind)
                        },
                        parameters,
                        period,
                        deviation,
                        source: binding.source,
                        source_input: binding.source_input,
                        volume_source: binding.volume_source,
                        style: self
                            .series_entry(binding.outputs[output_index])
                            .map(indicator_output_style)
                            .unwrap_or_default(),
                        output_name,
                        output_label,
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

    pub fn add_vwma(
        &mut self,
        source: SeriesId,
        volume_source: Option<SeriesId>,
        period: usize,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Vwma { period }, volume_source)
            .into_iter()
            .next()
    }

    pub fn add_cci(&mut self, source: SeriesId, period: usize) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Cci { period }, None)
            .into_iter()
            .next()
    }

    pub fn add_stochastic_rsi(
        &mut self,
        source: SeriesId,
        rsi_period: usize,
        stochastic_period: usize,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::StochasticRsi {
                rsi_period,
                stochastic_period,
            },
            None,
        )
        .into_iter()
        .next()
    }

    pub fn add_donchian(&mut self, source: SeriesId, period: usize) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Donchian { period }, None)
    }

    pub fn add_pivot_points(
        &mut self,
        source: SeriesId,
        kind: aeris_charts_indicators::PivotKind,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::PivotPoints { variant: kind }, None)
    }

    pub fn add_keltner(
        &mut self,
        source: SeriesId,
        period: usize,
        multiplier: f64,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Keltner { period, multiplier }, None)
    }

    pub fn add_adx_dmi(&mut self, source: SeriesId, period: usize) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::AdxDmi { period }, None)
    }

    pub fn add_parabolic_sar(&mut self, source: SeriesId) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::ParabolicSar, None)
            .into_iter()
            .next()
    }

    pub fn add_supertrend(
        &mut self,
        source: SeriesId,
        period: usize,
        multiplier: f64,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::SuperTrend { period, multiplier },
            None,
        )
        .into_iter()
        .next()
    }

    pub fn add_ichimoku(&mut self, source: SeriesId) -> Vec<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Ichimoku, None)
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
    /// four conventional states (strong/weak × above/below zero).
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

    /// Add on-balance volume in its own oscillator pane. `volume_source` is required and supplies
    /// the per-bar volume column used by the cumulative direction signal.
    pub fn add_obv(&mut self, source: SeriesId, volume_source: SeriesId) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Obv, Some(volume_source))
            .into_iter()
            .next()
    }

    pub fn add_relative_volume(
        &mut self,
        source: SeriesId,
        volume_source: SeriesId,
        period: usize,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::RelativeVolume { period },
            Some(volume_source),
        )
        .into_iter()
        .next()
    }

    /// Add Chaikin money flow in its own oscillator pane.
    pub fn add_cmf(
        &mut self,
        source: SeriesId,
        volume_source: SeriesId,
        period: usize,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Cmf { period }, Some(volume_source))
            .into_iter()
            .next()
    }

    /// Add money flow index in its own oscillator pane.
    pub fn add_mfi(
        &mut self,
        source: SeriesId,
        volume_source: SeriesId,
        period: usize,
    ) -> Option<SeriesId> {
        self.add_indicator_kind(source, IndicatorKind::Mfi { period }, Some(volume_source))
            .into_iter()
            .next()
    }

    /// Add an engine-owned volume histogram and simple moving-average line in its own pane.
    pub fn add_volume(
        &mut self,
        source: SeriesId,
        volume_source: SeriesId,
        period: usize,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::Volume {
                period,
                exponential: false,
            },
            Some(volume_source),
        )
    }

    /// Add session/weekly/monthly VWAP basis, standard-deviation and percentage bands.
    pub fn add_vwap_bands(
        &mut self,
        source: SeriesId,
        volume_source: Option<SeriesId>,
        reset: aeris_charts_indicators::VwapReset,
        standard_deviation: f64,
        percent: f64,
    ) -> Vec<SeriesId> {
        self.add_indicator_kind(
            source,
            IndicatorKind::VwapBands {
                reset,
                standard_deviation,
                percent,
            },
            volume_source,
        )
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
        self.add_indicator_kind_with_input(source, IndicatorInputSource::Close, kind, volume_source)
    }

    /// Add an indicator with an explicit scalar source. Existing convenience methods use close;
    /// this typed path is used for hlc3/hl2 and other study-input selections.
    pub fn add_indicator_kind_with_input(
        &mut self,
        source: SeriesId,
        source_input: IndicatorInputSource,
        kind: IndicatorKind,
        volume_source: Option<SeriesId>,
    ) -> Vec<SeriesId> {
        // OHLC structural rules consume the complete candle. A scalar source override
        // must not be accepted and then silently ignored by their scanner.
        if structure_output_count(&kind).is_some() && source_input != IndicatorInputSource::Close {
            return Vec::new();
        }
        let ids = self.add_indicator(source, source_input, kind.clone(), volume_source);
        match kind {
            IndicatorKind::Macd { .. } => {
                if let Some(&histogram) = ids.get(2) {
                    self.convert_series_kind(histogram, SeriesKind::Histogram);
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::RelativeVolume { .. } => {
                if let Some(&histogram) = ids.first() {
                    self.convert_series_kind(histogram, SeriesKind::Histogram);
                    if let Some(series) = self.series_entry_mut(histogram) {
                        series.base = 0.0;
                    }
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::Cmf { .. } => {
                if let Some(&baseline_series) = ids.first() {
                    self.convert_series_kind(baseline_series, SeriesKind::Baseline);
                    if let Some(series) = self.series_entry_mut(baseline_series) {
                        series.baseline = Some(0.0);
                    }
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::Rsi { .. }
            | IndicatorKind::Stochastic { .. }
            | IndicatorKind::Atr { .. }
            | IndicatorKind::AdxDmi { .. }
            | IndicatorKind::Cci { .. }
            | IndicatorKind::StochasticRsi { .. }
            | IndicatorKind::Obv
            | IndicatorKind::Mfi { .. } => {
                if !ids.is_empty() {
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::Volume { .. } => {
                if let Some(&histogram) = ids.first() {
                    self.convert_series_kind(histogram, SeriesKind::Histogram);
                    // Volume columns read in the direction of the bar that traded them.
                    if let Some(series) = self.series_entry_mut(histogram) {
                        series.histogram_updown = true;
                    }
                    self.place_outputs_in_oscillator_pane(&ids);
                }
            }
            IndicatorKind::Sma { .. }
            | IndicatorKind::Ema { .. }
            | IndicatorKind::Vwma { .. }
            | IndicatorKind::Donchian { .. }
            | IndicatorKind::PivotPoints { .. }
            | IndicatorKind::Keltner { .. }
            | IndicatorKind::Bollinger { .. }
            | IndicatorKind::ParabolicSar
            | IndicatorKind::SuperTrend { .. }
            | IndicatorKind::Ichimoku
            | IndicatorKind::Vwap
            | IndicatorKind::VwapBands { .. }
            | IndicatorKind::SwingPoints { .. }
            | IndicatorKind::SessionLevels { .. }
            | IndicatorKind::PreviousPeriodLevels { .. }
            | IndicatorKind::OpeningRange { .. } => {
                if let Some((pane, target)) = self.series_price_scale(source) {
                    for &id in &ids {
                        if let Some((output_pane, output_target)) = self.series_price_scale(id) {
                            if output_pane != pane {
                                self.set_series_pane(id, pane, None);
                            }
                            if output_target != target {
                                self.set_series_price_scale(id, target);
                            }
                        }
                    }
                }
            }
            IndicatorKind::Custom { .. } => {}
        }
        if let Some(&binding) = ids.first() {
            self.record_indicator_added(binding);
        }
        ids
    }

    /// Change the scalar source of an existing binding while retaining all output identities.
    /// The owner rebuilds from the source and propagates the revision to chained studies.
    pub fn set_indicator_input_source(
        &mut self,
        output: SeriesId,
        source_input: IndicatorInputSource,
    ) -> bool {
        let Some(index) = self
            .indicators
            .iter()
            .position(|binding| binding.outputs.contains(&output))
        else {
            return false;
        };
        if self.indicators[index].source_input == source_input {
            return true;
        }
        if self.indicators[index].structure.is_some() {
            return false;
        }
        let kind = self.indicators[index].kind.clone();
        self.indicators[index].source_input = source_input;
        match &mut self.indicators[index].runtime {
            BindingRuntime::Custom(custom) => custom.state = CustomBindingState::Pending,
            runtime @ BindingRuntime::BuiltIn(_) => {
                *runtime = BindingRuntime::BuiltIn(Box::new(incremental_state(&kind)));
            }
        }
        self.indicator_changes.clear();
        let changes = self.rebuild_indicator(index, 0, true);
        self.indicator_changes.extend(changes.into_iter().flatten());
        self.propagate_indicator_changes();
        self.sync_time_points();
        true
    }

    /// Return the bounded typed editor schema for an indicator definition.
    pub fn indicator_schema(kind: &IndicatorKind) -> IndicatorSchema {
        if let IndicatorKind::Custom {
            type_id,
            parameters,
            output_count,
            ..
        } = kind
        {
            return IndicatorSchema {
                revision: INDICATOR_SCHEMA_REVISION,
                kind: type_id.clone(),
                parameters: parameters
                    .iter()
                    .map(|(name, default)| IndicatorParameterDescriptor {
                        name: name.clone(),
                        parameter_type: IndicatorParameterType::Number,
                        default: default.clone(),
                        min: None,
                        max: None,
                        options: None,
                    })
                    .collect(),
                outputs: (0..*output_count)
                    .map(|index| IndicatorOutputDescriptor {
                        name: format!("Output {}", index + 1),
                        index,
                        supports_style: true,
                    })
                    .collect(),
            };
        }
        let mut parameters = vec![IndicatorParameterDescriptor {
            name: "source".into(),
            parameter_type: IndicatorParameterType::Source,
            default: serde_json::json!(IndicatorInputSource::Close),
            min: None,
            max: None,
            options: None,
        }];
        if structure_output_count(kind).is_some() {
            parameters.clear();
        }
        if matches!(
            kind,
            IndicatorKind::SessionLevels { .. }
                | IndicatorKind::PreviousPeriodLevels { .. }
                | IndicatorKind::OpeningRange { .. }
        ) {
            parameters.clear();
        }
        let integer = |name: &str, default: usize| IndicatorParameterDescriptor {
            name: name.into(),
            parameter_type: IndicatorParameterType::Integer,
            default: serde_json::json!(default),
            min: Some(1.0),
            max: Some(1_000_000.0),
            options: None,
        };
        let number = |name: &str, default: f64| IndicatorParameterDescriptor {
            name: name.into(),
            parameter_type: IndicatorParameterType::Number,
            default: serde_json::json!(default),
            min: Some(0.0),
            max: Some(1_000_000.0),
            options: None,
        };
        let swing = |parameters: &mut Vec<IndicatorParameterDescriptor>, left, right| {
            for (name, value) in [("left", left), ("right", right)] {
                let mut descriptor = integer(name, value);
                descriptor.max = Some(50.0);
                parameters.push(descriptor);
            }
        };
        match *kind {
            IndicatorKind::SwingPoints { left, right } => swing(&mut parameters, left, right),
            IndicatorKind::SessionLevels { calendar }
            | IndicatorKind::PreviousPeriodLevels { calendar, .. }
            | IndicatorKind::OpeningRange { calendar, .. } => {
                if let IndicatorKind::PreviousPeriodLevels { period, .. } = kind {
                    parameters.push(
                        IndicatorParameterDescriptor::choice(
                            "period",
                            match period {
                                PreviousPeriod::Day => "day",
                                PreviousPeriod::Week => "week",
                                PreviousPeriod::Month => "month",
                            },
                            &["day", "week", "month"],
                        )
                        .unwrap(),
                    );
                }
                if let IndicatorKind::OpeningRange {
                    duration_seconds, ..
                } = kind
                {
                    parameters.push(integer("duration_seconds", *duration_seconds as usize));
                }
                parameters.push(
                    IndicatorParameterDescriptor::choice(
                        "calendar",
                        if calendar == StudyCalendarPolicy::Utc {
                            "utc"
                        } else {
                            "host"
                        },
                        &["utc", "host"],
                    )
                    .unwrap(),
                );
            }
            IndicatorKind::Sma { period }
            | IndicatorKind::Ema { period }
            | IndicatorKind::Cci { period }
            | IndicatorKind::Donchian { period }
            | IndicatorKind::Rsi { period }
            | IndicatorKind::Atr { period }
            | IndicatorKind::AdxDmi { period } => parameters.push(integer("period", period)),
            IndicatorKind::PivotPoints { variant } => {
                parameters.push(integer("kind", pivot_kind_index(variant)))
            }
            IndicatorKind::Bollinger { period, deviation } => {
                parameters.push(integer("period", period));
                parameters.push(number("deviation", deviation));
            }
            IndicatorKind::RelativeVolume { period } => {
                parameters.push(integer("period", period));
                parameters.push(IndicatorParameterDescriptor {
                    name: "volume_source".into(),
                    parameter_type: IndicatorParameterType::Series,
                    default: serde_json::Value::Null,
                    min: None,
                    max: None,
                    options: None,
                });
            }
            IndicatorKind::Keltner { period, multiplier }
            | IndicatorKind::SuperTrend { period, multiplier } => {
                parameters.push(integer("period", period));
                parameters.push(number("multiplier", multiplier));
            }
            IndicatorKind::StochasticRsi {
                rsi_period,
                stochastic_period,
            } => {
                parameters.push(integer("rsi_period", rsi_period));
                parameters.push(integer("stochastic_period", stochastic_period));
            }
            IndicatorKind::ParabolicSar | IndicatorKind::Ichimoku => {}
            IndicatorKind::Macd { fast, slow, signal } => {
                parameters.push(integer("fast", fast));
                parameters.push(integer("slow", slow));
                parameters.push(integer("signal", signal));
            }
            IndicatorKind::Stochastic { k_period, d_period } => {
                parameters.push(integer("k_period", k_period));
                parameters.push(integer("d_period", d_period));
            }
            IndicatorKind::Vwap | IndicatorKind::Obv => {
                parameters.push(IndicatorParameterDescriptor {
                    name: "volume_source".into(),
                    parameter_type: IndicatorParameterType::Series,
                    default: serde_json::Value::Null,
                    min: None,
                    max: None,
                    options: None,
                });
            }
            IndicatorKind::Cmf { period }
            | IndicatorKind::Mfi { period }
            | IndicatorKind::Vwma { period } => {
                parameters.push(integer("period", period));
                parameters.push(IndicatorParameterDescriptor {
                    name: "volume_source".into(),
                    parameter_type: IndicatorParameterType::Series,
                    default: serde_json::Value::Null,
                    min: None,
                    max: None,
                    options: None,
                });
            }
            IndicatorKind::Volume {
                period,
                exponential,
            } => {
                parameters.push(integer("period", period));
                parameters.push(IndicatorParameterDescriptor {
                    name: "exponential".into(),
                    parameter_type: IndicatorParameterType::Boolean,
                    default: serde_json::json!(exponential),
                    min: None,
                    max: None,
                    options: None,
                });
                parameters.push(IndicatorParameterDescriptor {
                    name: "volume_source".into(),
                    parameter_type: IndicatorParameterType::Series,
                    default: serde_json::Value::Null,
                    min: None,
                    max: None,
                    options: None,
                });
            }
            IndicatorKind::VwapBands {
                reset,
                standard_deviation,
                percent,
            } => {
                parameters.push(IndicatorParameterDescriptor {
                    name: "reset".into(),
                    parameter_type: IndicatorParameterType::Source,
                    default: serde_json::json!(reset),
                    min: None,
                    max: None,
                    options: None,
                });
                parameters.push(number("standard_deviation", standard_deviation));
                parameters.push(number("percent", percent));
                parameters.push(IndicatorParameterDescriptor {
                    name: "volume_source".into(),
                    parameter_type: IndicatorParameterType::Series,
                    default: serde_json::Value::Null,
                    min: None,
                    max: None,
                    options: None,
                });
            }
            IndicatorKind::Custom { .. } => unreachable!("custom schema handled above"),
        }
        let output_count = session_output_count(kind)
            .or_else(|| structure_output_count(kind))
            .unwrap_or_else(|| incremental_state(kind).output_count());
        IndicatorSchema {
            revision: INDICATOR_SCHEMA_REVISION,
            kind: indicator_kind_name(kind).into(),
            parameters,
            outputs: (0..output_count)
                .map(|index| IndicatorOutputDescriptor {
                    name: indicator_output_name(kind, index).into(),
                    index,
                    supports_style: true,
                })
                .collect(),
        }
    }

    /// Move output series into a fresh oscillator pane below everything (the public reference
    /// separate-pane default, reduced stretch).
    pub(crate) fn place_outputs_in_oscillator_pane(&mut self, ids: &[SeriesId]) {
        let restored_pane = self.study_restore_pane_cursor.take().and_then(|index| {
            if index > 0 && index < self.panes.len() {
                self.study_restore_pane_cursor = Some(index + 1);
                Some(index)
            } else {
                None
            }
        });
        let Some(pane) = restored_pane.or_else(|| self.add_pane(false)) else {
            return;
        };
        let stretch = self.separate_pane_stretch();
        if let Some(p) = self.panes.get_mut(pane) {
            p.stretch_factor = stretch;
        }
        for &id in ids {
            self.set_series_pane(id, pane, Some(stretch));
        }
    }

    /// The source series and annotations of the study whose first output is `anchor`. Frame
    /// construction and hit testing share this lookup.
    pub(crate) fn study_annotations_for_anchor(
        &self,
        anchor: SeriesId,
    ) -> Option<(SeriesId, &StudyAnnotations)> {
        let binding = self.indicators.iter().find(|binding| {
            binding.outputs.first() == Some(&anchor)
                && (binding.annotations.is_some() || binding.structure.is_some())
        })?;
        let annotations = binding.annotations.as_ref().or_else(|| {
            binding
                .structure
                .as_ref()
                .map(|structure| structure.annotations())
        })?;
        Some((binding.source, annotations))
    }

    /// Generalized band/cloud fill descriptor for an output series: returns the companion output
    /// and fill policy (uniform translucent fill or directional crossover-split Kumo cloud), or
    /// `None` when `id` does not anchor a band fill.
    pub(crate) fn band_fill_spec(&self, id: SeriesId) -> Option<BandFillSpec> {
        let market = self.market_colors();
        self.indicators
            .iter()
            .find_map(|binding| match binding.kind {
                IndicatorKind::Bollinger { .. } if binding.outputs.first() == Some(&id) => binding
                    .outputs
                    .get(2)
                    .copied()
                    .map(|companion| BandFillSpec {
                        companion,
                        mode: BandFillMode::Uniform { alpha: 0.2 },
                    }),
                IndicatorKind::Keltner { .. } | IndicatorKind::Donchian { .. }
                    if binding.outputs.first() == Some(&id) =>
                {
                    binding
                        .outputs
                        .get(2)
                        .copied()
                        .map(|companion| BandFillSpec {
                            companion,
                            mode: BandFillMode::Uniform { alpha: 0.14 },
                        })
                }
                IndicatorKind::OpeningRange { .. } if binding.outputs.first() == Some(&id) => {
                    binding
                        .outputs
                        .get(1)
                        .copied()
                        .map(|companion| BandFillSpec {
                            companion,
                            mode: BandFillMode::Uniform { alpha: 0.10 },
                        })
                }
                IndicatorKind::VwapBands { .. } if binding.outputs.get(1) == Some(&id) => binding
                    .outputs
                    .get(2)
                    .copied()
                    .map(|companion| BandFillSpec {
                        companion,
                        mode: BandFillMode::Uniform { alpha: 0.12 },
                    }),
                IndicatorKind::VwapBands { .. } if binding.outputs.get(3) == Some(&id) => binding
                    .outputs
                    .get(4)
                    .copied()
                    .map(|companion| BandFillSpec {
                        companion,
                        mode: BandFillMode::Uniform { alpha: 0.07 },
                    }),
                IndicatorKind::Ichimoku if binding.outputs.get(2) == Some(&id) => binding
                    .outputs
                    .get(3)
                    .copied()
                    .map(|companion| BandFillSpec {
                        companion,
                        mode: BandFillMode::Directional {
                            bullish: Color::rgba(market.up.r(), market.up.g(), market.up.b(), 46),
                            bearish: Color::rgba(
                                market.down.r(),
                                market.down.g(),
                                market.down.b(),
                                46,
                            ),
                        },
                    }),
                _ => None,
            })
    }

    /// Sorted UTC timestamps where segmented period/session studies break their stroke & fills.
    pub(crate) fn series_segment_break_times(&self, id: SeriesId) -> &[i64] {
        self.indicators
            .iter()
            .find(|binding| binding.outputs.contains(&id))
            .map_or(&[], |binding| binding.segment_break_times.as_slice())
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
        source_input: IndicatorInputSource,
        kind: IndicatorKind,
        volume_source: Option<SeriesId>,
    ) -> Vec<SeriesId> {
        if !structure_kind_is_valid(&kind)
            || self.series_entry(source).is_none()
            || match &kind {
                IndicatorKind::Obv
                | IndicatorKind::RelativeVolume { .. }
                | IndicatorKind::Cmf { .. }
                | IndicatorKind::Mfi { .. } => volume_source.is_none_or(|id| {
                    id == source
                        || self
                            .series_entry(id)
                            .is_none_or(|series| !series.kind.stores_scalar_values())
                }),
                // The volume series may also be the source: its columns then follow traded
                // volume even where a presentation leaves the price rows as whitespace.
                IndicatorKind::Volume { .. } => volume_source.is_none_or(|id| {
                    self.series_entry(id)
                        .is_none_or(|series| !series.kind.stores_scalar_values())
                }),
                IndicatorKind::Vwap
                | IndicatorKind::VwapBands { .. }
                | IndicatorKind::Vwma { .. } => volume_source.is_some_and(|id| {
                    id == source
                        || self
                            .series_entry(id)
                            .is_none_or(|series| !series.kind.stores_scalar_values())
                }),
                _ => volume_source.is_some(),
            }
            || match &kind {
                IndicatorKind::Sma { period }
                | IndicatorKind::Ema { period }
                | IndicatorKind::Vwma { period }
                | IndicatorKind::Donchian { period }
                | IndicatorKind::Rsi { period }
                | IndicatorKind::Atr { period }
                | IndicatorKind::AdxDmi { period }
                | IndicatorKind::Cci { period }
                | IndicatorKind::RelativeVolume { period }
                | IndicatorKind::Cmf { period }
                | IndicatorKind::Mfi { period }
                | IndicatorKind::Volume { period, .. } => *period == 0,
                IndicatorKind::Bollinger { period, deviation } => {
                    *period == 0 || !deviation.is_finite() || *deviation < 0.0
                }
                IndicatorKind::Keltner { period, multiplier }
                | IndicatorKind::SuperTrend { period, multiplier } => {
                    *period == 0 || !multiplier.is_finite() || *multiplier < 0.0
                }
                IndicatorKind::StochasticRsi {
                    rsi_period,
                    stochastic_period,
                } => *rsi_period == 0 || *stochastic_period == 0,
                IndicatorKind::ParabolicSar
                | IndicatorKind::PivotPoints { .. }
                | IndicatorKind::Ichimoku
                | IndicatorKind::Vwap
                | IndicatorKind::Obv
                | IndicatorKind::SessionLevels { .. }
                | IndicatorKind::PreviousPeriodLevels { .. } => false,
                IndicatorKind::Macd { fast, slow, signal } => {
                    *fast == 0 || *slow == 0 || *signal == 0
                }
                IndicatorKind::Stochastic { k_period, d_period } => {
                    *k_period == 0 || *d_period == 0
                }
                IndicatorKind::VwapBands {
                    standard_deviation,
                    percent,
                    ..
                } => {
                    !standard_deviation.is_finite()
                        || *standard_deviation < 0.0
                        || !percent.is_finite()
                        || *percent < 0.0
                }
                IndicatorKind::SwingPoints { .. } => !structure_kind_is_valid(&kind),
                IndicatorKind::OpeningRange {
                    duration_seconds, ..
                } => *duration_seconds == 0,
                IndicatorKind::Custom { .. } => true,
            }
        {
            return Vec::new();
        }
        let runtime = incremental_state(&kind);
        let output_count = session_output_count(&kind)
            .or_else(|| structure_output_count(&kind))
            .unwrap_or_else(|| runtime.output_count());
        let structure = structure_study_kind(&kind).map(StructureStudy::new);
        // Volume columns and their average are quantities, formatted like the volume they read.
        let format_source = match kind {
            IndicatorKind::Volume { .. } => volume_source.unwrap_or(source),
            _ => source,
        };
        let source_price_format = self.series_entry(format_source).map(|series| {
            (
                series.price_format.kind,
                series.price_format.precision,
                series.price_format.min_move,
            )
        });
        let source_placement = self
            .series_entry(source)
            .map(|series| (series.pane_index, series.price_scale_target));
        let ma_cycle_index = if is_moving_average_kind(&kind) {
            self.indicators
                .iter()
                .filter(|binding| binding.source == source && is_moving_average_kind(&binding.kind))
                .count()
        } else {
            0
        };
        let ids = (0..output_count)
            .map(|_| self.add_series(SeriesKind::Line))
            .collect::<Vec<_>>();
        // Indicator chrome defaults: no candle-close countdown (theirs is a line value, not a
        // bar close), the auto-generated name chip shows (platforms override the name through
        // the series `title` option — custom-script indicators will set their own), and each
        // output gets its semantic default palette, stroke style, break policy, reference lines,
        // and level tag — every visual default remains overridable through ordinary series options.
        for (output_index, &id) in ids.iter().enumerate() {
            if let Some(s) = self.series.iter_mut().find(|s| s.id == id) {
                s.countdown_visible = false;
                s.title_visible = self.indicator_chrome.name_labels_visible;
                s.last_value_visible = self.indicator_chrome.value_labels_visible;
                s.price_line_visible = self.indicator_chrome.price_lines_visible;
                s.title = indicator_output_title(&kind, output_index, self.language);
                s.line_width = Some(indicator_default_line_width(&kind, output_index));
                s.line_style = indicator_default_line_style(&kind, output_index);
                let (tone, default_color) =
                    indicator_default_color(&kind, output_index, ma_cycle_index);
                s.market_tone = tone;
                s.default_line_color = default_color;
                s.line_break_mode = indicator_default_line_break_mode(&kind);
                s.bar_offset = indicator_default_bar_offset(&kind, output_index);
                s.level_tag = indicator_default_level_tag(&kind, output_index);
                s.reference_lines = indicator_default_reference_lines(&kind, output_index);
                if matches!(kind, IndicatorKind::ParabolicSar) {
                    s.line_visible = false;
                    s.point_markers = true;
                    s.point_marker_shape = PointMarkerShape::Circle;
                    s.point_markers_radius = Some(2.5);
                }
                if matches!(
                    kind,
                    IndicatorKind::SwingPoints { .. }
                        | IndicatorKind::SessionLevels { .. }
                        | IndicatorKind::PreviousPeriodLevels { .. }
                        | IndicatorKind::OpeningRange { .. }
                ) && let Some((pane_index, price_scale_target)) = source_placement
                {
                    s.pane_index = pane_index;
                    s.price_scale_target = price_scale_target;
                }
                if matches!(kind, IndicatorKind::SwingPoints { .. }) {
                    s.line_type = LineType::WithSteps;
                }
                // The last-price pulse marks the traded series, never a derived study line.
                s.last_price_animation = false;
                if output_index == 0 {
                    s.threshold_region = match kind {
                        IndicatorKind::Rsi { .. } => Some(SeriesThresholdRegion {
                            lower: 30.0,
                            upper: 70.0,
                        }),
                        IndicatorKind::Stochastic { .. }
                        | IndicatorKind::StochasticRsi { .. }
                        | IndicatorKind::Mfi { .. } => Some(SeriesThresholdRegion {
                            lower: 20.0,
                            upper: 80.0,
                        }),
                        IndicatorKind::Cci { .. } => Some(SeriesThresholdRegion {
                            lower: -100.0,
                            upper: 100.0,
                        }),
                        _ => None,
                    };
                }
                if let Some((kind, precision, min_move)) = source_price_format {
                    s.price_format.kind = kind;
                    s.price_format.precision = precision;
                    s.price_format.min_move = min_move;
                }
            }
            self.adopt_scale_price_format(id);
        }
        let calendar = match &kind {
            IndicatorKind::SessionLevels { calendar }
            | IndicatorKind::PreviousPeriodLevels { calendar, .. }
            | IndicatorKind::OpeningRange { calendar, .. } => Some(*calendar),
            _ => None,
        };
        let session = session_study_kind(&kind).map(SessionStudyState::new);
        self.indicators.push(IndicatorBinding {
            source,
            source_input,
            runtime: BindingRuntime::BuiltIn(Box::new(runtime)),
            kind,
            outputs: ids.clone(),
            volume_source,
            annotations: None,
            structure,
            session,
            calendar,
            source_generation: 0,
            volume_generation: None,
            inputs: BindingInputs::default(),
            segment_break_times: Vec::new(),
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
        self.refresh_resampled_dependents(dependency);
    }

    pub(crate) fn recompute_all_indicators(&mut self) {
        self.indicator_changes.clear();
        for binding in &self.indicators {
            for dependency in [Some(binding.source), binding.volume_source]
                .into_iter()
                .flatten()
            {
                if !self
                    .indicator_changes
                    .iter()
                    .any(|(existing, _)| *existing == dependency)
                {
                    self.indicator_changes.push((
                        dependency,
                        IndicatorChange {
                            from: 0,
                            previous_generation: 0,
                            full_replace: true,
                        },
                    ));
                }
            }
        }
        self.propagate_indicator_changes();
        self.sync_time_points();
    }

    pub(crate) fn update_indicators_after_change(
        &mut self,
        dependency: SeriesId,
        change: IndicatorChange,
    ) {
        if let (Some(series), Some(generation)) = (
            self.series_entry(dependency),
            self.data.series_generation(dependency),
        ) {
            let from = if change.full_replace { 0 } else { change.from };
            series.heikin_ashi_cache.borrow_mut().note_change(
                change.previous_generation,
                generation,
                from,
            );
            if let Some(tpo) = series.tpo.as_ref() {
                tpo.cache
                    .borrow_mut()
                    .note_change(change.previous_generation, generation, from);
            }
        }
        self.indicator_changes.clear();
        self.indicator_changes.push((dependency, change));
        self.propagate_indicator_changes();
        self.sync_time_points();
        self.refresh_resampled_dependents(dependency);
    }

    pub(crate) fn propagate_indicator_changes(&mut self) {
        // Bindings are topological by construction: an indicator output must exist before it can
        // be selected as a later indicator's source. One forward pass therefore updates direct
        // dependencies and every downstream chain without repeatedly scanning the whole graph.
        for index in 0..self.indicators.len() {
            let update = {
                let binding = &self.indicators[index];
                self.indicator_changes
                    .iter()
                    .filter_map(|&(dependency, change)| {
                        let (tracked, from) = if binding.source == dependency {
                            (binding.source_generation, change.from)
                        } else if binding.volume_source == Some(dependency) {
                            (
                                binding.volume_generation.unwrap_or(0),
                                self.volume_change_source_row(
                                    binding.source,
                                    dependency,
                                    change.from,
                                ),
                            )
                        } else {
                            return None;
                        };
                        let stale = tracked != change.previous_generation;
                        Some((if stale { 0 } else { from }, change.full_replace || stale))
                    })
                    .reduce(|left, right| (left.0.min(right.0), left.1 || right.1))
            };
            if let Some((from, full_replace)) = update {
                let changes = self.rebuild_indicator(index, from, full_replace);
                self.indicator_changes.extend(changes.into_iter().flatten());
            }
        }
    }

    /// First source row whose aligned volume may differ after the volume series changed from its
    /// own row `volume_from`. Volume rows keep their own timeline, so the row numbers differ when
    /// the two series do not share every timestamp.
    fn volume_change_source_row(
        &self,
        source: SeriesId,
        volume: SeriesId,
        volume_from: usize,
    ) -> usize {
        let (Some((source_times, _)), Some((volume_times, _))) =
            (self.data.series_data(source), self.data.series_data(volume))
        else {
            return 0;
        };
        volume_times[..volume_from.min(volume_times.len())]
            .last()
            .map_or(0, |&unchanged| {
                source_times.partition_point(|&time| time <= unchanged)
            })
    }

    fn rebuild_structure_indicator(
        &mut self,
        index: usize,
        from: usize,
        full_replace: bool,
        outputs: [Option<SeriesId>; aeris_charts_indicators::MAX_OUTPUTS],
    ) -> [Option<(SeriesId, IndicatorChange)>; aeris_charts_indicators::MAX_OUTPUTS] {
        let mut changes = [None; aeris_charts_indicators::MAX_OUTPUTS];
        let source = self.indicators[index].source;
        let Some((times, values)) = self.data.series_data(source) else {
            return changes;
        };
        let len = times.len();
        let existing_starts: [usize; aeris_charts_indicators::MAX_OUTPUTS] =
            std::array::from_fn(|slot| {
                outputs[slot]
                    .and_then(|output| self.data.series_data(output))
                    .and_then(|(output_times, _)| output_times.first().copied())
                    .and_then(|first| times.binary_search(&first).ok())
                    .unwrap_or(len)
            });
        let source_generation = self.data.series_generation(source).unwrap_or(0);
        let start = if full_replace { 0 } else { from.min(len) };
        let input = aeris_charts_indicators::IndicatorInput {
            times,
            open: values[0],
            high: values[1],
            low: values[2],
            close: values[3],
            volume: &[],
        };
        let binding = &mut self.indicators[index];
        let structure = binding.structure.as_mut().expect("structure runtime");
        // Only a full replacement or a discontinuous source requires a fresh runtime.
        // Appends and tip replacements preserve its bounded checkpoint state.
        let replace = full_replace || start > structure.len();
        let output_start = if replace { 0 } else { start };
        if replace {
            *structure = StructureStudy::new(structure_study_kind(&binding.kind).unwrap());
        }
        structure.update(input, output_start);
        binding.source_generation = source_generation;
        for (output_index, output) in outputs.iter().flatten().copied().enumerate() {
            let previous_generation = self.data.series_generation(output).unwrap_or(0);
            let output_replace = replace || start <= existing_starts[output_index];
            let mut values = structure.outputs()[output_index][output_start..len]
                .iter()
                .map(|value| value.unwrap_or(f64::NAN))
                .collect::<Vec<_>>();
            let output_from = if output_replace {
                let first = values
                    .iter()
                    .position(|value| !value.is_nan())
                    .unwrap_or(values.len());
                values.drain(..first);
                self.data
                    .set_single_data_aligned(output, source, output_start + first, values);
                0
            } else {
                self.data
                    .update_single_aligned(output, source, output_start, &values)
                    .expect("structure output remains aligned to source")
            };

            if self.data.series_generation(output).unwrap_or(0) != previous_generation {
                changes[output_index] = Some((
                    output,
                    IndicatorChange {
                        from: output_from,
                        previous_generation,
                        full_replace: output_replace,
                    },
                ));
            }
        }
        changes
    }

    fn rebuild_session_indicator(
        &mut self,
        index: usize,
        from: usize,
        full_replace: bool,
        outputs: [Option<SeriesId>; aeris_charts_indicators::MAX_OUTPUTS],
    ) -> [Option<(SeriesId, IndicatorChange)>; aeris_charts_indicators::MAX_OUTPUTS] {
        let mut changes = [None; aeris_charts_indicators::MAX_OUTPUTS];
        let source = self.indicators[index].source;
        let Some((times, values)) = self.data.series_data(source) else {
            return changes;
        };
        let existing_starts: [usize; aeris_charts_indicators::MAX_OUTPUTS] =
            std::array::from_fn(|slot| {
                outputs[slot]
                    .and_then(|output| self.data.series_data(output))
                    .and_then(|(output_times, _)| output_times.first().copied())
                    .and_then(|first| times.binary_search(&first).ok())
                    .unwrap_or(times.len())
            });
        let session_source = if self.indicators[index].calendar == Some(StudyCalendarPolicy::Host) {
            SessionSource::Host(&self.study_calendar_spans)
        } else {
            SessionSource::Utc
        };
        let kind = session_study_kind(&self.indicators[index].kind).expect("session kind");
        {
            let binding = &mut self.indicators[index];
            update_segment_break_times(
                &mut binding.segment_break_times,
                &binding.kind,
                binding.calendar,
                &self.study_calendar_spans,
                times,
                from,
                full_replace,
            );
        }
        let state = self.indicators[index]
            .session
            .as_mut()
            .expect("session runtime");
        if full_replace {
            *state = SessionStudyState::new(kind);
        }
        state.update(
            aeris_charts_indicators::IndicatorInput {
                times,
                open: values[0],
                high: values[1],
                low: values[2],
                close: values[3],
                volume: &[],
            },
            session_source,
            if full_replace { 0 } else { from },
        );
        let points = state.outputs();
        let source_generation = self.data.series_generation(source).unwrap_or(0);
        let start = if full_replace {
            0
        } else {
            from.min(points.len())
        };
        for (output_index, output) in outputs.iter().flatten().copied().enumerate() {
            let previous_generation = self.data.series_generation(output).unwrap_or(0);
            let output_replace = full_replace || start <= existing_starts[output_index];
            let mut values = points[start..]
                .iter()
                .map(|p| {
                    match output_index {
                        0 => p.high,
                        1 => p.low,
                        2 if matches!(kind, SessionStudy::OpeningRange { .. }) => {
                            p.high.zip(p.low).map(|(high, low)| (high + low) * 0.5)
                        }
                        2 => p.close,
                        _ => None,
                    }
                    .unwrap_or(f64::NAN)
                })
                .collect::<Vec<_>>();
            let output_from = if output_replace {
                let first = values
                    .iter()
                    .position(|value| !value.is_nan())
                    .unwrap_or(values.len());
                values.drain(..first);
                self.data
                    .set_single_data_aligned(output, source, start + first, values);
                0
            } else {
                self.data
                    .update_single_aligned(output, source, start, &values)
                    .expect("session output remains aligned to source")
            };
            if self.data.series_generation(output).unwrap_or(0) != previous_generation {
                changes[output_index] = Some((
                    output,
                    IndicatorChange {
                        from: output_from,
                        previous_generation,
                        full_replace: output_replace,
                    },
                ));
            }
        }
        self.indicators[index].source_generation = source_generation;
        changes
    }

    pub(crate) fn rebuild_indicator(
        &mut self,
        index: usize,
        from: usize,
        full_replace: bool,
    ) -> [Option<(SeriesId, IndicatorChange)>; aeris_charts_indicators::MAX_OUTPUTS] {
        let mut changes = [None; aeris_charts_indicators::MAX_OUTPUTS];
        let outputs: [Option<SeriesId>; aeris_charts_indicators::MAX_OUTPUTS] =
            std::array::from_fn(|slot| self.indicators[index].outputs.get(slot).copied());
        for &output in outputs.iter().flatten() {
            self.invalidate_frame_series(output);
        }

        let source = self.indicators[index].source;
        let source_input = self.indicators[index].source_input;
        let volume_source = self.indicators[index].volume_source;
        let source_generation = self.data.series_generation(source).unwrap_or(0);
        if self.indicators[index].structure.is_some() {
            return self.rebuild_structure_indicator(index, from, full_replace, outputs);
        }
        if session_output_count(&self.indicators[index].kind).is_some() {
            return self.rebuild_session_indicator(index, from, full_replace, outputs);
        }
        if matches!(self.indicators[index].runtime, BindingRuntime::Custom(_)) {
            return self.rebuild_custom_indicator(index, from, full_replace, outputs);
        }
        if (full_replace || self.indicators[index].source_generation != source_generation)
            && let Some(annotations) = self.indicators[index].annotations.as_mut()
        {
            annotations.rebuild_from(if full_replace { 0 } else { from });
        }
        let leading_source = {
            let Some((times, values)) = self.data.series_data(source) else {
                return changes;
            };
            let binding = &mut self.indicators[index];
            // Missing volume counts as no volume for the volume studies and as unit weight for
            // the volume-weighted averages.
            let missing_volume = if matches!(
                &binding.kind,
                IndicatorKind::Obv
                    | IndicatorKind::RelativeVolume { .. }
                    | IndicatorKind::Cmf { .. }
                    | IndicatorKind::Mfi { .. }
                    | IndicatorKind::Volume { .. }
            ) {
                0.0
            } else {
                1.0
            };
            binding.inputs.refresh(
                if full_replace { 0 } else { from },
                source_input,
                times,
                values,
                volume_source
                    .and_then(|id| self.data.series_data(id))
                    .map(|(volume_times, volume)| (volume_times, volume[3])),
                missing_volume,
            );
            let selected_close = binding.inputs.close(source_input, values);
            let volume = binding.inputs.volume();
            // A leading source gap is not a sample, nor may it contribute a phantom
            // previous bar to formulas with a prior-close reference.
            let first_source = binding.inputs.leading().unwrap_or(times.len());
            let reset = full_replace || from <= first_source;
            if reset {
                binding.runtime =
                    BindingRuntime::BuiltIn(Box::new(incremental_state(&binding.kind)));
            }
            binding.runtime.built_in().rebuild_from(
                aeris_charts_indicators::IndicatorInput {
                    times: &times[first_source..],
                    open: &values[0][first_source..],
                    high: &values[1][first_source..],
                    low: &values[2][first_source..],
                    close: &selected_close[first_source..],
                    volume: volume.get(first_source..).unwrap_or(&[]),
                },
                if reset { 0 } else { from - first_source },
            );
            update_segment_break_times(
                &mut binding.segment_break_times,
                &binding.kind,
                binding.calendar,
                &self.study_calendar_spans,
                times,
                from,
                full_replace,
            );
            first_source
        };
        self.indicators[index].source_generation = source_generation;
        self.indicators[index].volume_generation =
            volume_source.and_then(|id| self.data.series_generation(id));

        let mut full_histogram_colors = None;
        for (output_index, output) in outputs.iter().flatten().copied().enumerate() {
            let previous_generation = self.data.series_generation(output).unwrap_or(0);
            let source_from = leading_source
                + self.indicators[index]
                    .runtime
                    .built_in()
                    .output_from(output_index);
            let existing_start = self
                .data
                .series_data(output)
                .and_then(|(output_times, _)| output_times.first().copied())
                .and_then(|first| {
                    self.data
                        .series_data(source)
                        .and_then(|(times, _)| times.binary_search(&first).ok())
                })
                .unwrap_or_else(|| {
                    self.data
                        .series_data(source)
                        .map_or(0, |(times, _)| times.len())
                });
            let output_replace =
                full_replace || from <= existing_start || source_from <= existing_start;
            let output_from = if output_replace {
                let mut values = self.indicators[index]
                    .runtime
                    .built_in()
                    .take_output(output_index);
                let first = values
                    .iter()
                    .position(|value| !value.is_nan())
                    .unwrap_or(values.len());
                values.drain(..first);
                if output_index == 2
                    && matches!(self.indicators[index].kind, IndicatorKind::Macd { .. })
                {
                    full_histogram_colors =
                        Some(momentum_histogram_colors(&values, self.market_colors()));
                } else if output_index == 0
                    && matches!(
                        self.indicators[index].kind,
                        IndicatorKind::RelativeVolume { .. }
                    )
                {
                    full_histogram_colors =
                        Some(rvol_histogram_colors(&values, self.market_colors()));
                } else if output_index == 0
                    && matches!(
                        self.indicators[index].kind,
                        IndicatorKind::SuperTrend { .. } | IndicatorKind::ParabolicSar
                    )
                {
                    let aligned_start = source_from + first;
                    let src_close = self
                        .data
                        .series_data(source)
                        .and_then(|(_, vals)| vals[3].get(aligned_start..))
                        .unwrap_or(&[]);
                    full_histogram_colors = Some(directional_vs_close_colors(
                        &values,
                        src_close,
                        self.market_colors(),
                    ));
                }
                self.data
                    .set_single_data_aligned(output, source, source_from + first, values);
                0
            } else {
                let values = self.indicators[index]
                    .runtime
                    .built_in()
                    .output(output_index);
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
                        full_replace: output_replace,
                    },
                ));
            }
        }

        let point_colored = match self.indicators[index].kind {
            IndicatorKind::Macd { .. } => Some((outputs[2].unwrap(), 2, 0u8)),
            IndicatorKind::RelativeVolume { .. } => Some((outputs[0].unwrap(), 0, 1u8)),
            IndicatorKind::SuperTrend { .. } | IndicatorKind::ParabolicSar => {
                Some((outputs[0].unwrap(), 0, 2u8))
            }
            _ => None,
        };
        if let Some((colored_id, output_index, mode)) = point_colored {
            if let Some(colors) = full_histogram_colors {
                self.data
                    .set_point_colors(colored_id, [Some(colors), None, None]);
            } else {
                let market = self.market_colors();
                let runtime = self.indicators[index].runtime.built_in();
                let slice = runtime.output(output_index);
                let source_from = leading_source + runtime.output_from(output_index);
                let output_start = source_from.saturating_sub(
                    self.data
                        .series_data(colored_id)
                        .and_then(|(output_times, _)| output_times.first().copied())
                        .and_then(|first| {
                            self.data
                                .series_data(source)
                                .and_then(|(times, _)| times.binary_search(&first).ok())
                        })
                        .unwrap_or(source_from),
                );
                match mode {
                    0 => {
                        let mut previous = output_start.checked_sub(1).and_then(|row| {
                            self.data
                                .series_data(colored_id)
                                .and_then(|(_, values)| values[3].get(row).copied())
                        });
                        for (offset, &value) in slice.iter().enumerate() {
                            let color = momentum_histogram_color(value, previous, market);
                            self.data.set_point_color(
                                colored_id,
                                PointColorChannel::Body,
                                output_start + offset,
                                color,
                            );
                            previous = Some(value);
                        }
                    }
                    1 => {
                        for (offset, &value) in slice.iter().enumerate() {
                            let color = rvol_histogram_color(value, market);
                            self.data.set_point_color(
                                colored_id,
                                PointColorChannel::Body,
                                output_start + offset,
                                color,
                            );
                        }
                    }
                    _ => {
                        let closes: Vec<f64> = self
                            .data
                            .series_data(source)
                            .and_then(|(_, values)| {
                                values[3].get(source_from..source_from + slice.len())
                            })
                            .map_or_else(Vec::new, |s| s.to_vec());
                        for (offset, &value) in slice.iter().enumerate() {
                            let close = closes.get(offset).copied().unwrap_or(f64::NAN);
                            let color = directional_vs_close_color(value, close, market);
                            self.data.set_point_color(
                                colored_id,
                                PointColorChannel::Body,
                                output_start + offset,
                                color,
                            );
                        }
                    }
                }
            }
        }
        self.indicators[index]
            .runtime
            .built_in()
            .release_transfer_capacity();
        changes
    }
}

fn rvol_histogram_color(value: f64, market: crate::frame::MarketColors) -> u32 {
    if !value.is_finite() || value < 1.0 {
        Color::rgba(0x78, 0x7b, 0x86, 0x8c).0
    } else if value >= 1.5 {
        Color::rgb(0xff, 0x98, 0x00).0
    } else {
        market.up.0
    }
}

fn rvol_histogram_colors(values: &[f64], market: crate::frame::MarketColors) -> Vec<u32> {
    values
        .iter()
        .map(|&value| rvol_histogram_color(value, market))
        .collect()
}

fn directional_vs_close_color(value: f64, close: f64, market: crate::frame::MarketColors) -> u32 {
    if value.is_finite() && close.is_finite() && value <= close {
        market.up.0
    } else {
        market.down.0
    }
}

fn directional_vs_close_colors(
    values: &[f64],
    closes: &[f64],
    market: crate::frame::MarketColors,
) -> Vec<u32> {
    values
        .iter()
        .enumerate()
        .map(|(i, &value)| {
            let close = closes.get(i).copied().unwrap_or(f64::NAN);
            directional_vs_close_color(value, close, market)
        })
        .collect()
}

fn momentum_histogram_colors(values: &[f64], market: crate::frame::MarketColors) -> Vec<u32> {
    let mut colors = Vec::with_capacity(values.len());
    let mut previous = None;
    for &value in values {
        if value.is_finite() {
            colors.push(momentum_histogram_color(value, previous, market));
            previous = Some(value);
        } else {
            colors.push(aeris_charts_core::model::data_layer::POINT_COLOR_ABSENT);
            previous = None;
        }
    }
    colors
}

fn indicator_kind_name(kind: &IndicatorKind) -> &'static str {
    match kind {
        IndicatorKind::SwingPoints { .. } => "swing_points",
        IndicatorKind::SessionLevels { .. } => "session_levels",
        IndicatorKind::PreviousPeriodLevels { .. } => "previous_period_levels",
        IndicatorKind::OpeningRange { .. } => "opening_range",
        IndicatorKind::Sma { .. } => "sma",
        IndicatorKind::Ema { .. } => "ema",
        IndicatorKind::Vwma { .. } => "vwma",
        IndicatorKind::Cci { .. } => "cci",
        IndicatorKind::StochasticRsi { .. } => "stochastic_rsi",
        IndicatorKind::Donchian { .. } => "donchian",
        IndicatorKind::PivotPoints { .. } => "pivot_points",
        IndicatorKind::Keltner { .. } => "keltner",
        IndicatorKind::AdxDmi { .. } => "adx_dmi",
        IndicatorKind::ParabolicSar => "parabolic_sar",
        IndicatorKind::SuperTrend { .. } => "supertrend",
        IndicatorKind::Ichimoku => "ichimoku",
        IndicatorKind::Bollinger { .. } => "bollinger",
        IndicatorKind::Rsi { .. } => "rsi",
        IndicatorKind::Macd { .. } => "macd",
        IndicatorKind::Stochastic { .. } => "stochastic",
        IndicatorKind::Atr { .. } => "atr",
        IndicatorKind::Vwap => "vwap",
        IndicatorKind::Obv => "obv",
        IndicatorKind::RelativeVolume { .. } => "relative_volume",
        IndicatorKind::Cmf { .. } => "cmf",
        IndicatorKind::Mfi { .. } => "mfi",
        IndicatorKind::Volume { .. } => "volume",
        IndicatorKind::VwapBands { .. } => "vwap_bands",
        IndicatorKind::Custom { .. } => "custom",
    }
}

fn structure_output_count(kind: &IndicatorKind) -> Option<usize> {
    match kind {
        IndicatorKind::SwingPoints { .. } => Some(2),
        _ => None,
    }
}

fn session_output_count(kind: &IndicatorKind) -> Option<usize> {
    match kind {
        IndicatorKind::SessionLevels { .. } => Some(2),
        IndicatorKind::PreviousPeriodLevels { .. } | IndicatorKind::OpeningRange { .. } => Some(3),
        _ => None,
    }
}

fn session_study_kind(kind: &IndicatorKind) -> Option<SessionStudy> {
    match kind {
        IndicatorKind::SessionLevels { .. } => Some(SessionStudy::SessionLevels),
        IndicatorKind::PreviousPeriodLevels { period, .. } => {
            Some(SessionStudy::PreviousPeriodLevels(match period {
                PreviousPeriod::Day => aeris_charts_indicators::PreviousPeriod::Day,
                PreviousPeriod::Week => aeris_charts_indicators::PreviousPeriod::Week,
                PreviousPeriod::Month => aeris_charts_indicators::PreviousPeriod::Month,
            }))
        }
        IndicatorKind::OpeningRange {
            duration_seconds, ..
        } => Some(SessionStudy::OpeningRange {
            duration_seconds: i64::from(*duration_seconds),
        }),
        _ => None,
    }
}

pub(crate) fn structure_kind_is_valid(kind: &IndicatorKind) -> bool {
    structure_study_kind(kind).is_none_or(StructureStudyKind::is_valid)
}

fn structure_study_kind(kind: &IndicatorKind) -> Option<StructureStudyKind> {
    match kind {
        IndicatorKind::SwingPoints { left, right } => {
            Some(StructureStudyKind::swing_points(*left, *right))
        }
        _ => None,
    }
}

fn pivot_kind_index(kind: aeris_charts_indicators::PivotKind) -> usize {
    match kind {
        aeris_charts_indicators::PivotKind::Standard => 1,
        aeris_charts_indicators::PivotKind::Fibonacci => 2,
        aeris_charts_indicators::PivotKind::Camarilla => 3,
        aeris_charts_indicators::PivotKind::Woodie => 4,
        aeris_charts_indicators::PivotKind::DeMark => 5,
    }
}

pub(crate) fn incremental_state(kind: &IndicatorKind) -> aeris_charts_indicators::IncrementalState {
    match *kind {
        // Structural and session bindings execute their own scanners, not this scalar placeholder.
        IndicatorKind::SwingPoints { .. }
        | IndicatorKind::SessionLevels { .. }
        | IndicatorKind::PreviousPeriodLevels { .. }
        | IndicatorKind::OpeningRange { .. } => aeris_charts_indicators::IncrementalState::sma(1),
        IndicatorKind::Sma { period } => aeris_charts_indicators::IncrementalState::sma(period),
        IndicatorKind::Ema { period } => aeris_charts_indicators::IncrementalState::ema(period),
        IndicatorKind::Vwma { period } => aeris_charts_indicators::IncrementalState::vwma(period),
        IndicatorKind::Donchian { period } => {
            aeris_charts_indicators::IncrementalState::donchian(period)
        }
        IndicatorKind::PivotPoints { variant } => {
            aeris_charts_indicators::IncrementalState::pivot_points(variant)
        }
        IndicatorKind::Keltner { period, multiplier } => {
            aeris_charts_indicators::IncrementalState::keltner(period, multiplier)
        }
        IndicatorKind::AdxDmi { period } => {
            aeris_charts_indicators::IncrementalState::adx_dmi(period)
        }
        IndicatorKind::Cci { period } => aeris_charts_indicators::IncrementalState::cci(period),
        IndicatorKind::StochasticRsi {
            rsi_period,
            stochastic_period,
        } => {
            aeris_charts_indicators::IncrementalState::stochastic_rsi(rsi_period, stochastic_period)
        }
        IndicatorKind::ParabolicSar => aeris_charts_indicators::IncrementalState::parabolic_sar(),
        IndicatorKind::SuperTrend { period, multiplier } => {
            aeris_charts_indicators::IncrementalState::supertrend(period, multiplier)
        }
        IndicatorKind::Ichimoku => aeris_charts_indicators::IncrementalState::ichimoku(),
        IndicatorKind::Bollinger { period, deviation } => {
            aeris_charts_indicators::IncrementalState::bollinger(period, deviation)
        }
        IndicatorKind::Rsi { period } => aeris_charts_indicators::IncrementalState::rsi(period),
        IndicatorKind::Macd { fast, slow, signal } => {
            aeris_charts_indicators::IncrementalState::macd(fast, slow, signal)
        }
        IndicatorKind::Stochastic { k_period, d_period } => {
            aeris_charts_indicators::IncrementalState::stochastic(k_period, d_period)
        }
        IndicatorKind::Atr { period } => aeris_charts_indicators::IncrementalState::atr(period),
        IndicatorKind::Vwap => aeris_charts_indicators::IncrementalState::vwap(),
        IndicatorKind::Obv => aeris_charts_indicators::IncrementalState::obv(),
        IndicatorKind::RelativeVolume { period } => {
            aeris_charts_indicators::IncrementalState::relative_volume(period)
        }
        IndicatorKind::Cmf { period } => aeris_charts_indicators::IncrementalState::cmf(period),
        IndicatorKind::Mfi { period } => aeris_charts_indicators::IncrementalState::mfi(period),
        IndicatorKind::Volume {
            period,
            exponential,
        } => aeris_charts_indicators::IncrementalState::volume(period, exponential),
        IndicatorKind::VwapBands {
            reset,
            standard_deviation,
            percent,
        } => aeris_charts_indicators::IncrementalState::vwap_bands(
            reset,
            standard_deviation,
            percent,
        ),
        IndicatorKind::Custom { .. } => unreachable!("custom runtime is dispatched separately"),
    }
}

/// Four-state momentum palette: strong when moving away from zero, weak when falling back toward
/// it (industry-standard). Packed `0xRRGGBBAA`.
fn momentum_histogram_color(
    value: f64,
    previous: Option<f64>,
    market: crate::frame::MarketColors,
) -> u32 {
    let rising = previous.is_none_or(|previous| value >= previous);
    let color = if value >= 0.0 {
        if rising {
            market.up.solid()
        } else {
            market.volume_up()
        }
    } else if rising {
        market.volume_down()
    } else {
        market.down.solid()
    };
    color.0
}

/// An indicator parameter in its compact title form (`2`, `1.5`).
fn indicator_title_param(d: f64) -> String {
    if d.fract() == 0.0 {
        format!("{}", d as i64)
    } else {
        format!("{d}")
    }
}

/// The auto-generated indicator name behind the (hidden-by-default) name chip — what
/// the public reference shows in its indicator legend ("SMA 20", "MACD 12 26 9") — in the
/// chart's display language. Platforms can read it via the series options or override it with
/// their own `title`; a custom study keeps its host type id.
pub(crate) fn indicator_title(kind: &IndicatorKind, language: ChartLanguage) -> String {
    use crate::text::indicators as t;
    use aeris_charts_indicators::PivotKind;
    let params = indicator_title_param;
    let fixed = |phrase: crate::text::Phrase| phrase.text(language).to_string();
    let period_only = |template: crate::text::Template, period: &usize| {
        template.format(language, &[("period", period)])
    };
    match kind {
        IndicatorKind::SwingPoints { left, right } => {
            t::SWING_POINTS.format(language, &[("left", left), ("right", right)])
        }
        IndicatorKind::SessionLevels { .. } => fixed(t::SESSION_LEVELS),
        IndicatorKind::PreviousPeriodLevels { period, .. } => fixed(match period {
            PreviousPeriod::Day => t::PREVIOUS_DAY_LEVELS,
            PreviousPeriod::Week => t::PREVIOUS_WEEK_LEVELS,
            PreviousPeriod::Month => t::PREVIOUS_MONTH_LEVELS,
        }),
        IndicatorKind::OpeningRange {
            duration_seconds, ..
        } => t::OPENING_RANGE.format(language, &[("seconds", duration_seconds)]),
        IndicatorKind::Sma { period } => period_only(t::SMA, period),
        IndicatorKind::Ema { period } => period_only(t::EMA, period),
        IndicatorKind::Vwma { period } => period_only(t::VWMA, period),
        IndicatorKind::Cci { period } => period_only(t::CCI, period),
        IndicatorKind::StochasticRsi {
            rsi_period,
            stochastic_period,
        } => t::STOCHASTIC_RSI.format(
            language,
            &[
                ("rsi_period", rsi_period),
                ("stochastic_period", stochastic_period),
            ],
        ),
        IndicatorKind::Donchian { period } => period_only(t::DONCHIAN, period),
        IndicatorKind::PivotPoints { variant } => fixed(match variant {
            PivotKind::Standard => t::PIVOT_POINTS_STANDARD,
            PivotKind::Fibonacci => t::PIVOT_POINTS_FIBONACCI,
            PivotKind::Camarilla => t::PIVOT_POINTS_CAMARILLA,
            PivotKind::Woodie => t::PIVOT_POINTS_WOODIE,
            PivotKind::DeMark => t::PIVOT_POINTS_DEMARK,
        }),
        IndicatorKind::Keltner { period, multiplier } => t::KELTNER.format(
            language,
            &[("period", period), ("multiplier", &params(*multiplier))],
        ),
        IndicatorKind::AdxDmi { period } => period_only(t::ADX_DMI, period),
        IndicatorKind::ParabolicSar => fixed(t::PARABOLIC_SAR),
        IndicatorKind::SuperTrend { period, multiplier } => t::SUPERTREND.format(
            language,
            &[("period", period), ("multiplier", &params(*multiplier))],
        ),
        IndicatorKind::Ichimoku => fixed(t::ICHIMOKU),
        IndicatorKind::Bollinger { period, deviation } => t::BOLLINGER.format(
            language,
            &[("period", period), ("deviation", &params(*deviation))],
        ),
        IndicatorKind::Rsi { period } => period_only(t::RSI, period),
        IndicatorKind::Macd { fast, slow, signal } => t::MACD.format(
            language,
            &[("fast", fast), ("slow", slow), ("signal", signal)],
        ),
        IndicatorKind::Stochastic { k_period, d_period } => {
            t::STOCHASTIC.format(language, &[("k_period", k_period), ("d_period", d_period)])
        }
        IndicatorKind::Atr { period } => period_only(t::ATR, period),
        IndicatorKind::Vwap => fixed(t::VWAP),
        IndicatorKind::Obv => fixed(t::OBV),
        IndicatorKind::RelativeVolume { period } => period_only(t::RELATIVE_VOLUME, period),
        IndicatorKind::Cmf { period } => period_only(t::CMF, period),
        IndicatorKind::Mfi { period } => period_only(t::MFI, period),
        IndicatorKind::Volume { .. } => fixed(t::VOLUME),
        IndicatorKind::VwapBands { .. } => fixed(t::VWAP_BANDS),
        IndicatorKind::Custom { type_id, .. } => type_id.clone(),
    }
}

/// The title an indicator output series receives at bind time, in the chart's display language.
pub(crate) fn indicator_output_title(
    kind: &IndicatorKind,
    output_index: usize,
    language: ChartLanguage,
) -> String {
    use crate::text::indicators as t;
    match kind {
        IndicatorKind::Volume {
            period,
            exponential,
        } if output_index == 1 => t::VOLUME_AVERAGE.format(
            language,
            &[
                ("average", &if *exponential { "EMA" } else { "SMA" }),
                ("period", period),
            ],
        ),
        _ => indicator_title(kind, language),
    }
}

/// The localized display label of one output (`"Upper"`/`"Верхняя"`, `"Signal"`/`"Сигнал"`),
/// for legends and host chrome in the chart's display language. This is a presentation string:
/// the stable schema identifier stays [`IndicatorOutputDescriptor::name`], and English shows
/// that identifier itself. Acronym labels (`RSI`, `%K`, `R1`) are language-neutral and pass
/// through.
pub fn indicator_output_label(
    kind: &IndicatorKind,
    output_index: usize,
    language: ChartLanguage,
) -> String {
    use crate::text::indicators as t;
    let phrase = match (kind, output_index) {
        (IndicatorKind::SwingPoints { .. }, 0) => t::OUTPUT_SWING_HIGH,
        (IndicatorKind::SwingPoints { .. }, 1) => t::OUTPUT_SWING_LOW,
        (
            IndicatorKind::SessionLevels { .. }
            | IndicatorKind::PreviousPeriodLevels { .. }
            | IndicatorKind::OpeningRange { .. },
            0,
        ) => t::OUTPUT_HIGH,
        (
            IndicatorKind::SessionLevels { .. }
            | IndicatorKind::PreviousPeriodLevels { .. }
            | IndicatorKind::OpeningRange { .. },
            1,
        ) => t::OUTPUT_LOW,
        (IndicatorKind::PreviousPeriodLevels { .. }, 2) => t::OUTPUT_CLOSE,
        (IndicatorKind::OpeningRange { .. }, 2) => t::OUTPUT_MID,
        (
            IndicatorKind::Donchian { .. }
            | IndicatorKind::Keltner { .. }
            | IndicatorKind::Bollinger { .. },
            index,
        ) if index < 3 => [t::OUTPUT_UPPER, t::OUTPUT_BASIS, t::OUTPUT_LOWER][index],
        (IndicatorKind::PivotPoints { .. }, 0) => t::OUTPUT_PIVOT,
        (IndicatorKind::Ichimoku, index) if index < 5 => [
            t::OUTPUT_CONVERSION,
            t::OUTPUT_BASE,
            t::OUTPUT_LEADING_A,
            t::OUTPUT_LEADING_B,
            t::OUTPUT_LAGGING,
        ][index],
        (IndicatorKind::Macd { .. }, 1) => t::OUTPUT_SIGNAL,
        (IndicatorKind::Macd { .. }, 2) => t::OUTPUT_HISTOGRAM,
        (IndicatorKind::Volume { .. }, 0) => t::OUTPUT_VOLUME,
        (IndicatorKind::VwapBands { .. }, index) if index < 5 => [
            t::OUTPUT_BASIS,
            t::OUTPUT_STD_UPPER,
            t::OUTPUT_STD_LOWER,
            t::OUTPUT_PERCENT_UPPER,
            t::OUTPUT_PERCENT_LOWER,
        ][index],
        (IndicatorKind::StochasticRsi { .. }, 0) => t::OUTPUT_STOCH_RSI,
        (IndicatorKind::SuperTrend { .. }, 0) => t::OUTPUT_SUPERTREND,
        (IndicatorKind::RelativeVolume { .. }, 0) => t::OUTPUT_RELATIVE_VOLUME,
        _ => return indicator_output_name(kind, output_index).to_string(),
    };
    phrase.text(language).to_string()
}

pub(crate) fn indicator_output_style(series: &SeriesEntry) -> IndicatorOutputStyle {
    IndicatorOutputStyle {
        visible: series.visible,
        line_color: series.line_color.clone(),
        line_width: series.line_width,
        line_style: series.line_style,
        point_markers: series.point_markers,
        up_color: series.up_color.clone(),
        down_color: series.down_color.clone(),
        area_top_color: series.area_top_color.clone(),
        area_bottom_color: series.area_bottom_color.clone(),
    }
}

/// Whether every width, line style, and color in a host style is acceptable, checked before any
/// series state changes.
pub(crate) fn output_style_is_valid(style: &IndicatorOutputStyle) -> bool {
    style
        .line_width
        .is_none_or(|width| width.is_finite() && width > 0.0)
        && style.line_style <= 4
        && style_color_is_valid(style.line_color.as_deref())
        && style_color_is_valid(style.up_color.as_deref())
        && style_color_is_valid(style.down_color.as_deref())
        && style_color_is_valid(style.area_top_color.as_deref())
        && style_color_is_valid(style.area_bottom_color.as_deref())
}

fn style_color_is_valid(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        value.len() <= 256 && aeris_charts_render::color::Color::parse_css(value).is_some()
    })
}

fn indicator_output_name(kind: &IndicatorKind, output_index: usize) -> &'static str {
    match kind {
        IndicatorKind::SwingPoints { .. } => ["swing_high", "swing_low"][output_index],
        IndicatorKind::SessionLevels { .. } => ["high", "low"][output_index],
        IndicatorKind::PreviousPeriodLevels { .. } => ["high", "low", "close"][output_index],
        IndicatorKind::OpeningRange { .. } => ["high", "low", "mid"][output_index],
        IndicatorKind::Sma { .. } => "SMA",
        IndicatorKind::Ema { .. } => "EMA",
        IndicatorKind::Vwma { .. } => "VWMA",
        IndicatorKind::Cci { .. } => "CCI",
        IndicatorKind::StochasticRsi { .. } => "Stoch RSI",
        IndicatorKind::Donchian { .. } => ["Upper", "Basis", "Lower"][output_index],
        IndicatorKind::PivotPoints { .. } => ["Pivot", "R1", "S1", "R2", "S2"][output_index],
        IndicatorKind::Keltner { .. } => ["Upper", "Basis", "Lower"][output_index],
        IndicatorKind::AdxDmi { .. } => ["+DI", "-DI", "ADX"][output_index],
        IndicatorKind::ParabolicSar => "SAR",
        IndicatorKind::SuperTrend { .. } => "SuperTrend",
        IndicatorKind::Ichimoku => {
            ["Conversion", "Base", "Leading A", "Leading B", "Lagging"][output_index]
        }
        IndicatorKind::Bollinger { .. } => ["Upper", "Basis", "Lower"][output_index],
        IndicatorKind::Rsi { .. } => "RSI",
        IndicatorKind::Macd { .. } => ["MACD", "Signal", "Histogram"][output_index],
        IndicatorKind::Stochastic { .. } => ["%K", "%D"][output_index],
        IndicatorKind::Atr { .. } => "ATR",
        IndicatorKind::Vwap => "VWAP",
        IndicatorKind::Obv => "OBV",
        IndicatorKind::RelativeVolume { .. } => "Relative Volume",
        IndicatorKind::Cmf { .. } => "CMF",
        IndicatorKind::Mfi { .. } => "MFI",
        IndicatorKind::Volume { .. } => ["Volume", "MA"][output_index],
        IndicatorKind::VwapBands { .. } => {
            ["Basis", "Std Upper", "Std Lower", "% Upper", "% Lower"][output_index]
        }
        IndicatorKind::Custom { .. } => "Custom",
    }
}

#[cfg(test)]
mod annotation_binding_tests {
    use super::*;
    use aeris_charts_indicators::study_annotations::{StudyMarker, StudyMarkerKind};

    #[test]
    fn schema_revision_two_exposes_only_choice_options() {
        assert_eq!(INDICATOR_SCHEMA_REVISION, 2);
        let choice =
            IndicatorParameterDescriptor::choice("calendar", "host", &["utc", "host"]).unwrap();
        assert_eq!(
            choice.options.as_deref(),
            Some(&["utc".into(), "host".into()][..])
        );
        assert_eq!(choice.default, serde_json::json!("host"));
        assert!(
            IndicatorParameterDescriptor::choice("calendar", "local", &["utc", "host"]).is_none()
        );
        let schema = ChartEngine::indicator_schema(&IndicatorKind::Sma { period: 14 });
        assert_eq!(schema.revision, 2);
        assert!(
            schema
                .parameters
                .iter()
                .all(|parameter| parameter.options.is_none())
        );
    }

    #[test]
    fn binding_snapshot_repairs_on_source_changes_and_drops_on_removal() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let values = [10.0, 11.0, 12.0, 13.0];
        chart
            .set_series_data(0, &[1.0, 2.0, 3.0, 4.0], &values, &values, &values, &values)
            .unwrap();
        let binding = chart.add_sma(0, 2).unwrap();
        assert_eq!(
            chart.study_annotations(binding).unwrap_err().code(),
            ErrorCode::UnsupportedOperation
        );
        assert_eq!(
            chart.study_annotations(0).unwrap_err().code(),
            ErrorCode::InvalidHandle
        );
        let mut annotations = StudyAnnotations::default();
        for confirm_row in [1, 3] {
            annotations.push_marker(StudyMarker {
                row: confirm_row,
                confirm_row,
                price: values[confirm_row],
                kind: StudyMarkerKind::SwingHigh,
            });
        }
        assert!(chart.inject_study_annotations_for_test(binding, annotations.clone()));
        let snapshot = chart.study_annotations(binding).unwrap();
        assert_eq!(snapshot, annotations);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["markers"].as_array().unwrap().len(), 2);
        assert!(json.get("zones").is_none());
        assert!(chart.update_series_bar(0, 4.0, [14.0; 4]));
        let repaired = chart.study_annotations(binding).unwrap();
        assert_eq!(repaired.markers().len(), 1);
        assert_eq!(repaired.markers()[0].confirm_row, 1);
        assert_eq!(snapshot.markers().len(), 2); // The returned snapshot does not alias the binding.
        assert!(chart.remove_indicator_binding(binding));
        assert_eq!(
            chart.study_annotations(binding).unwrap_err().code(),
            ErrorCode::InvalidHandle
        );
    }

    #[test]
    fn full_source_replacement_clears_test_annotations() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let values = [10.0, 11.0, 12.0];
        chart
            .set_series_data(0, &[1.0, 2.0, 3.0], &values, &values, &values, &values)
            .unwrap();
        let binding = chart.add_sma(0, 2).unwrap();
        let mut annotations = StudyAnnotations::default();
        annotations.push_marker(StudyMarker {
            row: 1,
            confirm_row: 2,
            price: 11.0,
            kind: StudyMarkerKind::SwingLow,
        });
        assert!(chart.inject_study_annotations_for_test(binding, annotations));
        chart
            .set_series_data(
                0,
                &[2.0, 3.0],
                &values[..2],
                &values[..2],
                &values[..2],
                &values[..2],
            )
            .unwrap();
        assert!(
            chart
                .study_annotations(binding)
                .unwrap()
                .markers()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod schema_mapping_tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn named_schema_uses_canonical_multi_parameter_defaults() {
        // These are the public TS query's implicit (period=14, deviation=2) arguments.
        // Every independent parameter must match the Rust study definition, not a
        // repetition or arithmetic derivation of the query's one period argument.
        for (name, expected) in [
            (
                "stochastic_rsi",
                json!({"rsi_period":14,"stochastic_period":14}),
            ),
            ("keltner", json!({"period":14,"multiplier":2.0})),
            ("supertrend", json!({"period":14,"multiplier":3.0})),
            ("bollinger", json!({"period":14,"deviation":2.0})),
            ("macd", json!({"fast":12,"slow":26,"signal":9})),
            ("stochastic", json!({"k_period":14,"d_period":3})),
            (
                "vwap_bands",
                json!({"standard_deviation":1.0,"percent":10.0,"volume_source":null}),
            ),
        ] {
            let definition = IndicatorKind::schema_definition(name, 14, 2.0).unwrap();
            let schema = ChartEngine::indicator_schema(&definition);
            assert_eq!(schema.kind, name);
            let actual: serde_json::Map<String, Value> = schema
                .parameters
                .into_iter()
                .filter(|parameter| parameter.name != "source")
                .map(|parameter| (parameter.name, parameter.default))
                .collect();
            let expected = expected.as_object().unwrap();
            for (parameter, default) in expected {
                assert_eq!(actual.get(parameter), Some(default), "{name}.{parameter}");
            }
            // No unexpected parameters other than the reset enum descriptor, if present.
            assert_eq!(
                actual.len(),
                expected.len() + usize::from(name == "vwap_bands"),
                "{name}"
            );
        }
    }

    #[test]
    fn named_schema_retains_legacy_overrides_and_unknowns() {
        fn schema(name: &str, period: usize, deviation: f64) -> IndicatorSchema {
            ChartEngine::indicator_schema(
                &IndicatorKind::schema_definition(name, period, deviation).unwrap(),
            )
        }
        fn default(name: &str, period: usize, deviation: f64, parameter: &str) -> Value {
            schema(name, period, deviation)
                .parameters
                .into_iter()
                .find(|item| item.name == parameter)
                .unwrap()
                .default
        }
        assert_eq!(default("stochastic", 21, 2.0, "k_period"), json!(21));
        assert_eq!(default("stochastic", 21, 2.0, "d_period"), json!(21));
        assert_eq!(default("macd", 21, 2.0, "fast"), json!(21));
        assert_eq!(default("macd", 21, 2.0, "slow"), json!(42));
        assert_eq!(default("macd", 21, 2.0, "signal"), json!(21));
        assert_eq!(default("bollinger", 21, 2.5, "deviation"), json!(2.5));
        assert_eq!(
            default("vwap_bands", 14, 2.5, "standard_deviation"),
            json!(2.5)
        );
        assert!(IndicatorKind::schema_definition("unknown", 14, 2.0).is_none());
        assert!(IndicatorKind::schema_definition("pivot_points", 14, 2.0).is_none());
        assert!(IndicatorKind::schema_definition("aroon", 14, 2.0).is_none());
        assert!(IndicatorKind::schema_definition("wma", 14, 2.0).is_none());
    }

    #[test]
    fn volume_indicator_defaults_to_separate_pane() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let volume = chart.add_series(SeriesKind::Histogram);
        let times = [1.0, 2.0, 3.0, 4.0];
        let prices = [100.0, 102.0, 101.0, 103.0];
        let vols = [10.0, 20.0, 15.0, 25.0];
        chart
            .set_series_data(0, &times, &prices, &prices, &prices, &prices)
            .unwrap();
        chart
            .set_series_data(volume, &times, &vols, &vols, &vols, &vols)
            .unwrap();
        let outputs = chart.add_volume(0, volume, 2);
        assert_eq!(outputs.len(), 2);
        assert_eq!(
            chart.indicator_binding_pane(outputs[0]),
            Some(IndicatorPane::Separate)
        );
        assert_ne!(
            chart.series_entry(outputs[0]).unwrap().pane_index,
            chart.series_entry(0).unwrap().pane_index
        );
        assert_eq!(
            chart.series_entry(outputs[0]).unwrap().pane_index,
            chart.series_entry(outputs[1]).unwrap().pane_index
        );
    }
}

#[cfg(test)]
mod structure_engine_tests {
    use super::*;
    use aeris_charts_indicators::structure_studies::StructureStudy;

    #[test]
    fn retained_source_bounds_annotations_and_attributes_their_bytes() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let n = 5_000;
        let times: Vec<_> = (0..n).map(|i| i as f64 + 1.).collect();
        let close = vec![100.0; n];
        let high: Vec<_> = (0..n)
            .map(|i| if i % 2 == 0 { 103.0 } else { 101.0 })
            .collect();
        let low: Vec<_> = (0..n)
            .map(|i| if i % 2 == 0 { 99.0 } else { 97.0 })
            .collect();
        chart
            .set_series_data(0, &times, &close, &high, &low, &close)
            .unwrap();
        let anchor = chart.add_swing_points(0, 1, 1)[0];
        let before = chart.study_annotations(anchor).unwrap();
        assert!(before.markers().len() > 4_096);
        let large = chart.memory_usage().indicator_runtime_bytes;
        assert!(
            large
                >= before.markers().len()
                    * std::mem::size_of::<aeris_charts_indicators::StudyMarker>()
        );
        assert!(chart.set_series_max_points(0, Some(128)));
        let after = chart.study_annotations(anchor).unwrap();
        assert!(after.markers().len() < 128);
        assert!(after.markers().iter().all(|marker| marker.row < 128));
        assert!(chart.memory_usage().indicator_runtime_bytes < large);
    }

    #[test]
    fn structure_bindings_align_confirmed_swing_levels() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let high = [12.0, 14.0, 13.0, 11.0, 13.0];
        let low = [9.0, 10.0, 9.0, 8.0, 10.0];
        let open = [10.0, 11.0, 10.0, 9.0, 11.0];
        let close = [11.0, 12.0, 10.0, 10.0, 12.0];
        chart
            .set_series_data(0, &[1.0, 2.0, 3.0, 4.0, 5.0], &open, &high, &low, &close)
            .unwrap();
        let swings = chart.add_swing_points(0, 1, 1);
        assert_eq!(swings.len(), 2);
        assert!(
            swings
                .iter()
                .all(|id| chart.series_entry(*id).unwrap().line_type == LineType::WithSteps)
        );
        let (high_times, high_values) = chart.data.series_data(swings[0]).unwrap();
        assert_eq!(high_times[0], 3);
        assert_eq!(high_values[3][0], 14.0);
        assert_eq!(
            chart.study_annotations(swings[0]).unwrap().markers()[0].row,
            1
        );
    }

    #[test]
    fn invalid_structure_parameters_do_not_create_outputs() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        for (left, right) in [(0, 5), (5, 0), (51, 5), (5, 51)] {
            assert!(chart.add_swing_points(0, left, right).is_empty());
        }
        assert!(
            chart
                .add_indicator_kind_with_input(
                    0,
                    IndicatorInputSource::Hlc3,
                    IndicatorKind::SwingPoints { left: 2, right: 2 },
                    None,
                )
                .is_empty()
        );
        assert!(chart.indicator_bindings().is_empty());
    }

    #[test]
    fn structure_schema_reports_exact_bounds() {
        let kind = IndicatorKind::SwingPoints { left: 3, right: 7 };
        let schema = ChartEngine::indicator_schema(&kind);
        assert_eq!(schema.revision, 2);
        assert_eq!(
            schema
                .outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            ["swing_high", "swing_low"]
        );
        assert_eq!(schema.parameters.len(), 2);
        for (name, value) in [("left", 3), ("right", 7)] {
            let descriptor = schema.parameters.iter().find(|p| p.name == name).unwrap();
            assert_eq!(descriptor.default, serde_json::json!(value));
            assert_eq!(descriptor.min, Some(1.0));
            assert_eq!(descriptor.max, Some(50.0));
        }
    }

    #[test]
    fn structure_append_tip_and_history_repair_match_pure_study() {
        let kinds = [
            IndicatorKind::SwingPoints { left: 2, right: 1 },
            IndicatorKind::SwingPoints { left: 1, right: 3 },
        ];
        let times = (1..=24).map(f64::from).collect::<Vec<_>>();
        let mut close = (0..24)
            .map(|row| 100.0 + ((row * 7) % 13) as f64)
            .collect::<Vec<_>>();
        let open = close.iter().map(|c| c - 0.5).collect::<Vec<_>>();
        let high = close.iter().map(|c| c + 2.0).collect::<Vec<_>>();
        let mut low = open.iter().map(|o| o - 2.0).collect::<Vec<_>>();
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &times[..8],
                &open[..8],
                &high[..8],
                &low[..8],
                &close[..8],
            )
            .unwrap();
        let bindings = kinds
            .iter()
            .map(|kind| chart.add_indicator_kind(0, kind.clone(), None))
            .collect::<Vec<_>>();
        let assert_equal = |chart: &ChartEngine, len: usize, close: &[f64], low: &[f64]| {
            let integer_times = (1..=len as i64).collect::<Vec<_>>();
            for (kind, ids) in kinds.iter().zip(&bindings) {
                let mut expected = StructureStudy::new(structure_study_kind(kind).unwrap());
                expected.update(
                    aeris_charts_indicators::IndicatorInput {
                        times: &integer_times,
                        open: &open[..len],
                        high: &high[..len],
                        low: &low[..len],
                        close: &close[..len],
                        volume: &[],
                    },
                    0,
                );
                assert_eq!(
                    chart.study_annotations(ids[0]).unwrap(),
                    *expected.annotations(),
                    "{kind:?}"
                );
                for (index, &id) in ids.iter().enumerate() {
                    let (actual_times, actual) = chart.data.series_data(id).unwrap();
                    let from = expected.outputs()[index]
                        .iter()
                        .position(Option::is_some)
                        .unwrap_or(len);
                    assert_eq!(
                        actual_times,
                        &integer_times[from..],
                        "{kind:?} output {index}"
                    );
                    for (row, (actual, expected)) in actual[3]
                        .iter()
                        .zip(&expected.outputs()[index][from..])
                        .enumerate()
                    {
                        assert!(
                            expected.is_some_and(|value| *actual == value)
                                || expected.is_none() && actual.is_nan(),
                            "{kind:?} output {index}, row {row}"
                        );
                    }
                }
            }
        };
        assert_equal(&chart, 8, &close, &low);
        for len in 9..=24 {
            assert!(chart.update_series_bar(
                0,
                times[len - 1],
                [open[len - 1], high[len - 1], low[len - 1], close[len - 1]]
            ));
            assert_equal(&chart, len, &close, &low);
        }
        close[23] -= 1.0;
        assert!(chart.update_series_bar(0, times[23], [open[23], high[23], low[23], close[23]]));
        assert_equal(&chart, 24, &close, &low);
        low[10] -= 1.0;
        assert!(chart.update_series_bar(0, times[10], [open[10], high[10], low[10], close[10]]));
        assert_equal(&chart, 24, &close, &low);
    }
}
