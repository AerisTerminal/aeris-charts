//! Engine-owned projection of host-computed scalar studies.
//!
//! A host owns provider subscriptions and computes values. The chart engine owns validation,
//! generation fencing, series/pane lifecycle, presentation, and group operations.

use crate::indicators::{indicator_output_style, output_style_is_valid};
use crate::{
    ChartEngine, Color, IndicatorChromeOptions, IndicatorOutputStyle, MarketTone, PriceFormatKind,
    SeriesId, SeriesKind, SeriesThresholdRegion,
};
use aeris_charts_core::model::data_validation::{MAX_SAFE_VALUE, validate_timestamp};
use std::fmt;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const DEFAULT_EXTERNAL_STUDY_LINE_WIDTH: f64 = 2.0;
const MAX_EXTERNAL_STUDY_TITLE_BYTES: usize = 256;
const MAX_EXTERNAL_STUDY_LEGEND_LABEL_BYTES: usize = 128;
const DEFAULT_EXTERNAL_STUDY_COLORS: [&str; 5] =
    ["#335cff", "#FF9800", "#7d52f4", "#fb4ba3", "#fb3748"];

/// Hard chart-local bound for host-computed scalar outputs.
pub const MAX_EXTERNAL_STUDY_OUTPUTS: usize = 256;

const OPTIONS_VOLUME_OUTPUTS: [(crate::text::Phrase, MarketTone); 2] = [
    (crate::text::legend::CALLS, MarketTone::Up),
    (crate::text::legend::PUTS, MarketTone::Down),
];
const OPTIONS_VOLUME_PRECISION: u32 = 1;
const OPEN_INTEREST_PRECISION: u32 = 1;
const LIQUIDATION_PRECISION: u32 = 1;
const LIQUIDATION_OUTPUTS: [(crate::text::Phrase, MarketTone); 2] = [
    (crate::text::legend::SHORTS, MarketTone::Up),
    (crate::text::legend::LONGS, MarketTone::Down),
];

/// One bar-aligned liquidation volume sample supplied to [`ChartEngine::set_liquidations`] or
/// [`ChartEngine::update_liquidations`].
///
/// Both `long_volume` (liquidated long positions) and `short_volume` (liquidated short positions)
/// must be finite and non-negative. Zero leaves that side empty on the bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidationBar {
    pub timestamp_unix_nanos: i64,
    pub long_volume: f64,
    pub short_volume: f64,
}

/// How an engine-owned Liquidations study renders its sub-pane histogram series.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LiquidationStyle {
    /// Short liquidations rise above zero (forced buys, bullish hue) and long liquidations fall
    /// below zero (forced sells, bearish hue).
    #[default]
    Split,
    /// Long and short liquidations stack upward from zero so total bar height equals combined
    /// liquidations while keeping both sides visually distinct.
    Stacked,
    /// Single net liquidation delta column (`short - long`) colored by sign around zero.
    Net,
}

/// Presentation options of an engine-owned Liquidations study. Unset colors follow the active
/// chart theme: bullish/up for short liquidations (forced buys) and bearish/down for long
/// liquidations (forced sells).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiquidationOptions {
    pub style: LiquidationStyle,
    pub long_color: Option<Color>,
    pub short_color: Option<Color>,
}

/// One bar-aligned Open Interest sample supplied to [`ChartEngine::set_open_interest`] or
/// [`ChartEngine::update_open_interest`].
///
/// When `open` is `None`, the engine derives continuous candlestick opens from the prior bar's
/// close (falling back to `close` on the first bar). Optional `high` and `low` supply intra-bar
/// sub-period extremes; the engine always enforces `low <= open.min(close)` and
/// `high >= open.max(close)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenInterestBar {
    pub timestamp_unix_nanos: i64,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: f64,
}

/// How an engine-owned Open Interest study renders its sub-pane series.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OpenInterestStyle {
    Candles,
    Bars,
    #[default]
    Line,
    Area,
}

impl OpenInterestStyle {
    pub(crate) const fn series_kind(self) -> SeriesKind {
        match self {
            Self::Candles => SeriesKind::Candlestick,
            Self::Bars => SeriesKind::Bar,
            Self::Line => SeriesKind::Line,
            Self::Area => SeriesKind::Area,
        }
    }
}

/// Presentation options of an engine-owned Open Interest study. Unset colors follow the active
/// chart theme: bullish/bearish for candles and bars, and the chart's default line color for
/// line and area.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpenInterestOptions {
    pub style: OpenInterestStyle,
    pub line_color: Option<Color>,
    pub up_color: Option<Color>,
    pub down_color: Option<Color>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExternalStudyInputRequirements(u8);

impl ExternalStudyInputRequirements {
    const BARS_BIT: u8 = 1 << 0;
    const TRADES_BIT: u8 = 1 << 1;
    const QUOTES_BIT: u8 = 1 << 2;
    const DEPTH_BIT: u8 = 1 << 3;

    pub const NONE: Self = Self(0);
    pub const BARS: Self = Self(Self::BARS_BIT);

    #[must_use]
    pub const fn with(self, stream: ExternalStudyInputStream) -> Self {
        let bit = match stream {
            ExternalStudyInputStream::Bars => Self::BARS_BIT,
            ExternalStudyInputStream::Trades => Self::TRADES_BIT,
            ExternalStudyInputStream::Quotes => Self::QUOTES_BIT,
            ExternalStudyInputStream::Depth => Self::DEPTH_BIT,
        };
        Self(self.0 | bit)
    }

    #[must_use]
    pub const fn contains(self, stream: ExternalStudyInputStream) -> bool {
        let bit = match stream {
            ExternalStudyInputStream::Bars => Self::BARS_BIT,
            ExternalStudyInputStream::Trades => Self::TRADES_BIT,
            ExternalStudyInputStream::Quotes => Self::QUOTES_BIT,
            ExternalStudyInputStream::Depth => Self::DEPTH_BIT,
        };
        self.0 & bit != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStudyInputStream {
    Bars,
    Trades,
    Quotes,
    Depth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStudyPlotKind {
    Line,
    Histogram,
    Area,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStudyPaneTarget {
    Price,
    Dedicated { group: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStudyScaleTarget {
    Primary,
    Left,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExternalStudyThresholdRegion {
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExternalStudyPointStyle {
    #[default]
    Uniform,
    MomentumHistogram,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExternalStudyOutputDescriptor<'a> {
    pub title: &'a str,
    pub legend_label: Option<&'a str>,
    pub plot: ExternalStudyPlotKind,
    pub pane: ExternalStudyPaneTarget,
    pub scale: ExternalStudyScaleTarget,
    pub settings_available: bool,
    pub threshold_region: Option<ExternalStudyThresholdRegion>,
    pub point_style: ExternalStudyPointStyle,
    pub input_requirements: ExternalStudyInputRequirements,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalStudyOutputInfo {
    pub study_id: u64,
    pub output_index: usize,
    pub series_id: SeriesId,
    pub generation: u64,
    pub settings_available: bool,
    pub legend_label: Option<String>,
    pub input_requirements: ExternalStudyInputRequirements,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExternalStudyOutputState {
    pub(crate) series_id: SeriesId,
    generation: u64,
    settings_available: bool,
    pub(crate) legend_label: Option<String>,
    input_requirements: ExternalStudyInputRequirements,
    /// The output's own visibility from its style, kept apart from the whole-study toggle so
    /// showing the study again never brings back a line the host hid. The series paints only
    /// when both are set.
    output_visible: bool,
    study_visible: bool,
    momentum_histogram: bool,
    pub(crate) options_volume: bool,
    pub(crate) open_interest: bool,
    open_interest_options: OpenInterestOptions,
    pub(crate) liquidations: bool,
    liquidation_options: LiquidationOptions,
    /// `(bar_second, high_bits, low_bits)` of the newest forming open-interest bar so repeated
    /// snapshots and live updates preserve intra-bar extremes.
    forming_wick: Option<(i64, u64, u64)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStudyError {
    LengthMismatch,
    UnsupportedTimestampPrecision,
    NonIncreasingTimestamp,
    InvalidValue,
    InvalidPresentation,
    MetadataTooLong,
    CapacityExceeded,
    InstallationRejected,
}

impl fmt::Display for ExternalStudyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LengthMismatch => "study timestamps and values must have the same length",
            Self::UnsupportedTimestampPrecision => {
                "study timestamps must be whole supported UTC seconds"
            }
            Self::NonIncreasingTimestamp => "study timestamps must be strictly increasing",
            Self::InvalidValue => {
                "study values must be finite and within the supported scalar range"
            }
            Self::InvalidPresentation => "study presentation is invalid for its plot kind",
            Self::MetadataTooLong => {
                "study title or legend label exceeds the bounded metadata limit"
            }
            Self::CapacityExceeded => "chart external-study output capacity is exhausted",
            Self::InstallationRejected => "chart rejected the study output",
        })
    }
}

impl std::error::Error for ExternalStudyError {}

struct PreparedColumns {
    times: Vec<f64>,
    scalar: Vec<f64>,
}

struct PreparedOhlcColumns {
    seconds: Vec<i64>,
    times: Vec<f64>,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    forming_wick: Option<(i64, u64, u64)>,
}

impl ChartEngine {
    pub fn install_external_study_output(
        &mut self,
        study_id: u64,
        output_index: usize,
        descriptor: ExternalStudyOutputDescriptor<'_>,
        generation: u64,
        timestamps_unix_nanos: &[i64],
        values: &[Option<f64>],
    ) -> Result<bool, ExternalStudyError> {
        if self.is_options_volume_study(study_id)
            || self.is_open_interest_study(study_id)
            || self.is_liquidation_study(study_id)
        {
            return Err(ExternalStudyError::InstallationRejected);
        }
        self.install_output(
            study_id,
            output_index,
            descriptor,
            generation,
            timestamps_unix_nanos,
            values,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)] // the public transaction plus its study-kind flag
    fn install_output(
        &mut self,
        study_id: u64,
        output_index: usize,
        descriptor: ExternalStudyOutputDescriptor<'_>,
        generation: u64,
        timestamps_unix_nanos: &[i64],
        values: &[Option<f64>],
        options_volume: bool,
    ) -> Result<bool, ExternalStudyError> {
        let key = (study_id, output_index);
        if self
            .external_study_outputs
            .get(&key)
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        validate_descriptor(descriptor)?;
        let columns = prepare_columns(timestamps_unix_nanos, values)?;
        let existing = self
            .external_study_outputs
            .get(&key)
            .map(|state| (state.series_id, state.output_visible, state.study_visible));
        let (series_id, output_visible, study_visible) = if let Some(existing) = existing {
            set_series_data(self, existing.0, &columns)?;
            existing
        } else {
            if self.external_study_outputs.len() >= MAX_EXTERNAL_STUDY_OUTPUTS {
                return Err(ExternalStudyError::CapacityExceeded);
            }
            // An output that first arrives while its study is hidden joins it hidden.
            let study_visible = self.external_study_visible(study_id).unwrap_or(true);
            let series_id = self.install_new_external_study_series(
                study_id,
                output_index,
                descriptor,
                &columns,
            )?;
            self.set_series_visible(series_id, study_visible);
            (series_id, true, study_visible)
        };
        apply_presentation(self, series_id, descriptor);
        self.external_study_outputs.insert(
            key,
            ExternalStudyOutputState {
                series_id,
                generation,
                settings_available: descriptor.settings_available,
                legend_label: descriptor.legend_label.map(str::to_owned),
                input_requirements: descriptor.input_requirements,
                output_visible,
                study_visible,
                momentum_histogram: descriptor.point_style
                    == ExternalStudyPointStyle::MomentumHistogram,
                options_volume,
                open_interest: false,
                open_interest_options: OpenInterestOptions::default(),
                liquidations: false,
                liquidation_options: LiquidationOptions::default(),
                forming_wick: None,
            },
        );
        Ok(true)
    }

    /// Install or replace the options-volume study `study_id` from host-supplied per-bar call and
    /// put volume. The engine draws it as one compact separate pane: call columns rise above zero
    /// in the painted bullish color and put columns fall below it in the bearish color, both
    /// following theme switches until a host style pins a color. Volumes must be non-negative;
    /// `None` leaves that bar empty. The study then behaves as any external study for
    /// visibility, output styles, selection, legend, and removal (`remove_external_studies`).
    /// Returns `Ok(false)` for a stale `generation`; a rejected push changes nothing.
    pub fn set_options_volume(
        &mut self,
        study_id: u64,
        generation: u64,
        timestamps_unix_nanos: &[i64],
        calls: &[Option<f64>],
        puts: &[Option<f64>],
    ) -> Result<bool, ExternalStudyError> {
        let existing = self.options_volume_series(study_id);
        if existing.is_none()
            && self.external_study_visible(study_id).is_some()
            && !self.is_options_volume_study(study_id)
        {
            return Err(ExternalStudyError::InstallationRejected);
        }
        if calls.len() != timestamps_unix_nanos.len() || puts.len() != timestamps_unix_nanos.len() {
            return Err(ExternalStudyError::LengthMismatch);
        }
        let calls = calls
            .iter()
            .map(|&value| options_volume_value(value))
            .collect::<Result<Vec<_>, _>>()?;
        let puts = puts
            .iter()
            .map(|&value| options_volume_value(value).map(|value| value.map(|value| -value)))
            .collect::<Result<Vec<_>, _>>()?;
        // Timestamps are checked once up front so the second output cannot fail after the first
        // one changed.
        prepare_columns(timestamps_unix_nanos, &calls)?;
        if self
            .external_study_outputs
            .get(&(study_id, 0))
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        if existing.is_none()
            && self.external_study_outputs.len() + OPTIONS_VOLUME_OUTPUTS.len()
                > MAX_EXTERNAL_STUDY_OUTPUTS
        {
            return Err(ExternalStudyError::CapacityExceeded);
        }
        let language = self.language;
        for (output_index, ((title, _), values)) in OPTIONS_VOLUME_OUTPUTS
            .iter()
            .zip([&calls, &puts])
            .enumerate()
        {
            if let Err(error) = self.install_output(
                study_id,
                output_index,
                options_volume_descriptor(language, *title),
                generation,
                timestamps_unix_nanos,
                values,
                true,
            ) {
                if existing.is_none() {
                    self.remove_external_studies(&[study_id]);
                }
                return Err(error);
            }
        }
        if existing.is_none()
            && let Some(series) = self.options_volume_series(study_id)
        {
            for (id, (_, tone)) in series.into_iter().zip(OPTIONS_VOLUME_OUTPUTS) {
                if let Some(entry) = self.series_entry_mut(id) {
                    entry.market_tone = Some(tone);
                    entry.line_color = None;
                    entry.price_format.kind = PriceFormatKind::Volume;
                    entry.price_format.precision = OPTIONS_VOLUME_PRECISION;
                    entry.price_format.formatter = None;
                }
                self.invalidate_frame_series(id);
            }
        }
        Ok(true)
    }

    /// Append or replace the newest bar of an options-volume study, the streaming counterpart of
    /// [`Self::set_options_volume`]. The bar must not be older than the study's newest bar.
    pub fn update_options_volume(
        &mut self,
        study_id: u64,
        generation: u64,
        timestamp_unix_nanos: i64,
        calls: f64,
        puts: f64,
    ) -> Result<bool, ExternalStudyError> {
        let series = self
            .options_volume_series(study_id)
            .ok_or(ExternalStudyError::InstallationRejected)?;
        let (Some(calls), Some(puts)) = (
            options_volume_value(Some(calls))?,
            options_volume_value(Some(puts))?,
        ) else {
            return Err(ExternalStudyError::InvalidValue);
        };
        let columns = prepare_columns(&[timestamp_unix_nanos], &[Some(calls)])?;
        let time = columns.times[0];
        if self
            .external_study_outputs
            .get(&(study_id, 0))
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        let newest = self
            .data
            .series_data(series[0])
            .and_then(|(times, _)| times.last().copied());
        if newest.is_some_and(|newest| newest as f64 > time) {
            return Err(ExternalStudyError::NonIncreasingTimestamp);
        }
        for (id, value) in series.into_iter().zip([calls, -puts]) {
            if !self.update_series_bar(id, time, [value; 4]) {
                return Err(ExternalStudyError::InstallationRejected);
            }
        }
        for output_index in 0..OPTIONS_VOLUME_OUTPUTS.len() {
            if let Some(state) = self
                .external_study_outputs
                .get_mut(&(study_id, output_index))
            {
                state.generation = generation;
            }
        }
        Ok(true)
    }

    /// The call and put series of a live options-volume study.
    #[must_use]
    pub fn options_volume_series(&self, study_id: u64) -> Option<[SeriesId; 2]> {
        let series = |output_index| {
            self.external_study_outputs
                .get(&(study_id, output_index))
                .filter(|state| state.options_volume)
                .map(|state| state.series_id)
                .filter(|&id| self.series_entry(id).is_some())
        };
        Some([series(0)?, series(1)?])
    }

    pub(crate) fn is_options_volume_study(&self, study_id: u64) -> bool {
        self.external_study_outputs
            .iter()
            .any(|(&(candidate, _), state)| candidate == study_id && state.options_volume)
    }

    /// Install or replace the Open Interest study `study_id` from host-supplied bar-aligned
    /// samples, keeping its current [`OpenInterestOptions`] (or defaulting to
    /// [`OpenInterestStyle::Candles`] when newly created).
    pub fn set_open_interest(
        &mut self,
        study_id: u64,
        generation: u64,
        bars: &[OpenInterestBar],
    ) -> Result<bool, ExternalStudyError> {
        let options = self.open_interest_options(study_id).unwrap_or_default();
        self.set_open_interest_with_options(study_id, generation, bars, options)
    }

    /// Install or replace the Open Interest study `study_id` with explicit presentation `options`
    /// (candles, OHLC bars, line, or filled area plus optional custom colors). The engine derives
    /// continuous OHLC candles (inheriting each bar's open from the previous bar's close when
    /// `open` is `None`), preserves intra-bar high/low wicks on the newest forming bar across
    /// refreshes, trims leading bars older than the primary price series without losing the first
    /// visible bar's open, and formats the right axis as volume in one dedicated pane.
    ///
    /// The study behaves as any external study for visibility, indicator chrome, selection,
    /// localized legend, language updates, and removal (`remove_external_studies`).
    /// Returns `Ok(false)` for a stale `generation`; a rejected push changes nothing.
    pub fn set_open_interest_with_options(
        &mut self,
        study_id: u64,
        generation: u64,
        bars: &[OpenInterestBar],
        options: OpenInterestOptions,
    ) -> Result<bool, ExternalStudyError> {
        let existing = self.open_interest_series(study_id);
        if existing.is_none()
            && self.external_study_visible(study_id).is_some()
            && !self.is_open_interest_study(study_id)
        {
            return Err(ExternalStudyError::InstallationRejected);
        }
        let key = (study_id, 0);
        if self
            .external_study_outputs
            .get(&key)
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        let existing_forming_wick = self
            .external_study_outputs
            .get(&key)
            .and_then(|state| state.forming_wick);
        let mut prepared = prepare_open_interest_columns(bars, existing_forming_wick)?;
        let next_forming_wick = prepared.forming_wick;
        // Trim leading/trailing bars outside the loaded primary price series so Open Interest
        // never prepends empty price-whitespace columns to the left of the price chart, while
        // keeping the continuous open computed from the preceding OI bar.
        if let Some((primary_times, _)) = self.data.series_data(0)
            && let (Some(&first_primary), Some(&last_primary)) =
                (primary_times.first(), primary_times.last())
        {
            let mut start = 0usize;
            let mut end = prepared.seconds.len();
            let candidate_start = prepared.seconds.partition_point(|&sec| sec < first_primary);
            if candidate_start < end {
                start = candidate_start;
            }
            let candidate_end = prepared.seconds.partition_point(|&sec| sec <= last_primary);
            if candidate_end > start {
                end = candidate_end;
            }
            if start > 0 || end < prepared.seconds.len() {
                prepared.times = prepared.times[start..end].to_vec();
                prepared.open = prepared.open[start..end].to_vec();
                prepared.high = prepared.high[start..end].to_vec();
                prepared.low = prepared.low[start..end].to_vec();
                prepared.close = prepared.close[start..end].to_vec();
            }
        }
        let (series_id, output_visible, study_visible) = if let Some(series_id) = existing {
            let previous_style = self
                .external_study_outputs
                .get(&key)
                .map(|state| state.open_interest_options.style);
            if previous_style != Some(options.style) {
                self.convert_series_kind(series_id, options.style.series_kind());
            }
            self.set_series_data(
                series_id,
                &prepared.times,
                &prepared.open,
                &prepared.high,
                &prepared.low,
                &prepared.close,
            )
            .map_err(|_| ExternalStudyError::InstallationRejected)?;
            let state = self
                .external_study_outputs
                .get(&key)
                .map(|state| (state.output_visible, state.study_visible))
                .unwrap_or((true, true));
            (series_id, state.0, state.1)
        } else {
            if self.external_study_outputs.len() >= MAX_EXTERNAL_STUDY_OUTPUTS {
                return Err(ExternalStudyError::CapacityExceeded);
            }
            let study_visible = self.external_study_visible(study_id).unwrap_or(true);
            let series_id = self.install_new_open_interest_series(study_id, &prepared, options)?;
            self.set_series_visible(series_id, study_visible);
            (series_id, true, study_visible)
        };
        let title = crate::text::legend::OPEN_INTEREST
            .text(self.language)
            .to_owned();
        if let Some(series) = self.series_entry_mut(series_id) {
            series.title = title;
        }
        self.apply_open_interest_colors(series_id, options);
        self.external_study_outputs.insert(
            key,
            ExternalStudyOutputState {
                series_id,
                generation,
                settings_available: true,
                legend_label: None,
                input_requirements: ExternalStudyInputRequirements::NONE,
                output_visible,
                study_visible,
                momentum_histogram: false,
                options_volume: false,
                open_interest: true,
                open_interest_options: options,
                liquidations: false,
                liquidation_options: LiquidationOptions::default(),
                forming_wick: next_forming_wick,
            },
        );
        Ok(true)
    }

    /// Reconfigure an existing Open Interest study's style (candles, bars, line, area) and colors
    /// in place, preserving its pane and bar history.
    pub fn set_open_interest_options(
        &mut self,
        study_id: u64,
        options: OpenInterestOptions,
    ) -> bool {
        let Some(series_id) = self.open_interest_series(study_id) else {
            return false;
        };
        let key = (study_id, 0);
        let Some(previous) = self
            .external_study_outputs
            .get(&key)
            .map(|state| state.open_interest_options)
        else {
            return false;
        };
        if previous == options {
            return false;
        }
        if let Some(state) = self.external_study_outputs.get_mut(&key) {
            state.open_interest_options = options;
        }
        if previous.style != options.style {
            self.convert_series_kind(series_id, options.style.series_kind());
        }
        self.apply_open_interest_colors(series_id, options);
        true
    }

    /// Current presentation options of a live Open Interest study.
    #[must_use]
    pub fn open_interest_options(&self, study_id: u64) -> Option<OpenInterestOptions> {
        self.external_study_outputs
            .get(&(study_id, 0))
            .filter(|state| state.open_interest && self.series_entry(state.series_id).is_some())
            .map(|state| state.open_interest_options)
    }

    fn apply_open_interest_colors(&mut self, series_id: SeriesId, options: OpenInterestOptions) {
        if let Some(entry) = self.series_entry_mut(series_id) {
            entry.line_color = options.line_color.map(|color| color.to_css());
            entry.up_color = options.up_color.map(|color| color.to_css());
            entry.down_color = options.down_color.map(|color| color.to_css());
        }
        self.invalidate_frame_series(series_id);
    }

    /// Append or replace the newest bar of an Open Interest candlestick study, the streaming
    /// counterpart of [`Self::set_open_interest`]. The bar must not be older than the study's
    /// newest bar; updates to the forming bar accumulate its intra-bar high/low wicks.
    pub fn update_open_interest(
        &mut self,
        study_id: u64,
        generation: u64,
        bar: OpenInterestBar,
    ) -> Result<bool, ExternalStudyError> {
        let series_id = self
            .open_interest_series(study_id)
            .ok_or(ExternalStudyError::InstallationRejected)?;
        let key = (study_id, 0);
        if self
            .external_study_outputs
            .get(&key)
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        let (second, time) = validate_open_interest_timestamp(bar.timestamp_unix_nanos)?;
        let close = open_interest_value(bar.close)?;
        let explicit_open = bar.open.map(open_interest_value).transpose()?;
        let explicit_high = bar.high.map(open_interest_value).transpose()?;
        let explicit_low = bar.low.map(open_interest_value).transpose()?;
        let last = self.series_data(series_id).pop();
        if let Some(ref previous) = last
            && previous.time > second
        {
            return Err(ExternalStudyError::NonIncreasingTimestamp);
        }
        let (open, high, low) = match last {
            Some(previous) if previous.time == second => {
                let open = explicit_open.unwrap_or(previous.open);
                let high = open
                    .max(close)
                    .max(explicit_high.unwrap_or(close))
                    .max(previous.high);
                let low = open
                    .min(close)
                    .min(explicit_low.unwrap_or(close))
                    .min(previous.low);
                (open, high, low)
            }
            Some(previous) => {
                let open = explicit_open.unwrap_or(previous.close);
                let high = open.max(close).max(explicit_high.unwrap_or(close));
                let low = open.min(close).min(explicit_low.unwrap_or(close));
                (open, high, low)
            }
            None => {
                let open = explicit_open.unwrap_or(close);
                let high = open.max(close).max(explicit_high.unwrap_or(close));
                let low = open.min(close).min(explicit_low.unwrap_or(close));
                (open, high, low)
            }
        };
        if !self.update_series_bar(series_id, time, [open, high, low, close]) {
            return Err(ExternalStudyError::InstallationRejected);
        }
        if let Some(state) = self.external_study_outputs.get_mut(&key) {
            state.generation = generation;
            state.forming_wick = Some((second, high.to_bits(), low.to_bits()));
        }
        Ok(true)
    }

    /// The candlestick series id of a live Open Interest study.
    #[must_use]
    pub fn open_interest_series(&self, study_id: u64) -> Option<SeriesId> {
        self.external_study_outputs
            .get(&(study_id, 0))
            .filter(|state| state.open_interest)
            .map(|state| state.series_id)
            .filter(|&id| self.series_entry(id).is_some())
    }

    pub(crate) fn is_open_interest_study(&self, study_id: u64) -> bool {
        self.external_study_outputs
            .iter()
            .any(|(&(candidate, _), state)| candidate == study_id && state.open_interest)
    }

    /// Install or replace the Liquidations study `study_id` from host-supplied bar-aligned
    /// long and short liquidation volumes, keeping its current [`LiquidationOptions`] (or
    /// defaulting to [`LiquidationStyle::Split`] when newly created).
    pub fn set_liquidations(
        &mut self,
        study_id: u64,
        generation: u64,
        bars: &[LiquidationBar],
    ) -> Result<bool, ExternalStudyError> {
        let options = self.liquidation_options(study_id).unwrap_or_default();
        self.set_liquidations_with_options(study_id, generation, bars, options)
    }

    /// Install or replace the Liquidations study `study_id` with explicit presentation `options`
    /// (`Split`, `Stacked`, or `Net` plus optional long/short colors). Both `long_volume` and
    /// `short_volume` must be finite and non-negative. The engine trims bars outside the loaded
    /// primary price series, formats the right axis as volume in one dedicated pane, and exposes
    /// localized legend values with settings enabled.
    ///
    /// Returns `Ok(false)` for a stale `generation`; a rejected push changes nothing.
    pub fn set_liquidations_with_options(
        &mut self,
        study_id: u64,
        generation: u64,
        bars: &[LiquidationBar],
        options: LiquidationOptions,
    ) -> Result<bool, ExternalStudyError> {
        let existing = self.liquidation_series(study_id);
        if existing.is_none()
            && self.external_study_visible(study_id).is_some()
            && !self.is_liquidation_study(study_id)
        {
            return Err(ExternalStudyError::InstallationRejected);
        }
        if self
            .external_study_outputs
            .get(&(study_id, 0))
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        let mut prepared = prepare_liquidation_columns(bars, options.style)?;
        if let Some((primary_times, _)) = self.data.series_data(0)
            && let (Some(&first_primary), Some(&last_primary)) =
                (primary_times.first(), primary_times.last())
        {
            let mut start = 0usize;
            let mut end = prepared.seconds.len();
            let candidate_start = prepared.seconds.partition_point(|&sec| sec < first_primary);
            if candidate_start < end {
                start = candidate_start;
            }
            let candidate_end = prepared.seconds.partition_point(|&sec| sec <= last_primary);
            if candidate_end > start {
                end = candidate_end;
            }
            if start > 0 || end < prepared.seconds.len() {
                prepared.times = prepared.times[start..end].to_vec();
                prepared.primary = prepared.primary[start..end].to_vec();
                prepared.secondary = prepared.secondary[start..end].to_vec();
            }
        }
        let primary_cols = PreparedColumns {
            times: prepared.times.clone(),
            scalar: prepared.primary,
        };
        let secondary_cols = PreparedColumns {
            times: prepared.times,
            scalar: prepared.secondary,
        };
        let language = self.language;
        let (series_ids, output_visibles, study_visible) = if let Some(series_ids) = existing {
            set_series_data(self, series_ids[0], &primary_cols)?;
            set_series_data(self, series_ids[1], &secondary_cols)?;
            let out0 = self
                .external_study_outputs
                .get(&(study_id, 0))
                .map(|state| (state.output_visible, state.study_visible))
                .unwrap_or((true, true));
            let out1 = self
                .external_study_outputs
                .get(&(study_id, 1))
                .map(|state| state.output_visible)
                .unwrap_or(true);
            (series_ids, [out0.0, out1], out0.1)
        } else {
            if self.external_study_outputs.len() + LIQUIDATION_OUTPUTS.len()
                > MAX_EXTERNAL_STUDY_OUTPUTS
            {
                return Err(ExternalStudyError::CapacityExceeded);
            }
            let study_visible = self.external_study_visible(study_id).unwrap_or(true);
            let s0 = self.install_new_liquidation_series(
                study_id,
                LIQUIDATION_OUTPUTS[0].0.text(language),
                &primary_cols,
            )?;
            let s1 = match self.install_new_liquidation_series(
                study_id,
                LIQUIDATION_OUTPUTS[1].0.text(language),
                &secondary_cols,
            ) {
                Ok(id) => id,
                Err(error) => {
                    let _ = self.remove_series(s0);
                    self.external_study_panes.remove(&(study_id, 0));
                    return Err(error);
                }
            };
            ([s0, s1], [true, true], study_visible)
        };
        for (index, (id, (phrase, _))) in
            series_ids.into_iter().zip(LIQUIDATION_OUTPUTS).enumerate()
        {
            let label = phrase.text(language).to_owned();
            if let Some(series) = self.series_entry_mut(id) {
                series.title = label.clone();
            }
            self.external_study_outputs.insert(
                (study_id, index),
                ExternalStudyOutputState {
                    series_id: id,
                    generation,
                    settings_available: true,
                    legend_label: Some(label),
                    input_requirements: ExternalStudyInputRequirements::NONE,
                    output_visible: output_visibles[index],
                    study_visible,
                    momentum_histogram: false,
                    options_volume: false,
                    open_interest: false,
                    open_interest_options: OpenInterestOptions::default(),
                    liquidations: true,
                    liquidation_options: options,
                    forming_wick: None,
                },
            );
        }
        self.apply_liquidation_presentation(series_ids, options, study_visible, output_visibles);
        Ok(true)
    }

    /// Reconfigure an existing Liquidations study's style (`Split`, `Stacked`, `Net`) and colors
    /// in place, preserving its pane and bar history.
    pub fn set_liquidation_options(&mut self, study_id: u64, options: LiquidationOptions) -> bool {
        let Some(series_ids) = self.liquidation_series(study_id) else {
            return false;
        };
        let Some(previous) = self.liquidation_options(study_id) else {
            return false;
        };
        if previous == options {
            return false;
        }
        if previous.style != options.style {
            let merged = merge_liquidation_series(
                &self.series_data(series_ids[0]),
                &self.series_data(series_ids[1]),
                previous.style,
                options.style,
            );
            let primary_cols = PreparedColumns {
                times: merged.times.clone(),
                scalar: merged.primary,
            };
            let secondary_cols = PreparedColumns {
                times: merged.times,
                scalar: merged.secondary,
            };
            if set_series_data(self, series_ids[0], &primary_cols).is_err()
                || set_series_data(self, series_ids[1], &secondary_cols).is_err()
            {
                return false;
            }
        }
        let mut study_visible = true;
        let mut output_visibles = [true, true];
        for (index, slot) in output_visibles.iter_mut().enumerate() {
            if let Some(state) = self.external_study_outputs.get_mut(&(study_id, index)) {
                state.liquidation_options = options;
                if index == 0 {
                    study_visible = state.study_visible;
                }
                *slot = state.output_visible;
            }
        }
        self.apply_liquidation_presentation(series_ids, options, study_visible, output_visibles);
        true
    }

    /// Current presentation options of a live Liquidations study.
    #[must_use]
    pub fn liquidation_options(&self, study_id: u64) -> Option<LiquidationOptions> {
        self.external_study_outputs
            .get(&(study_id, 0))
            .filter(|state| state.liquidations && self.series_entry(state.series_id).is_some())
            .map(|state| state.liquidation_options)
    }

    /// Append or replace the newest bar of a Liquidations study, the streaming counterpart of
    /// [`Self::set_liquidations`]. The bar must not be older than the study's newest bar.
    pub fn update_liquidations(
        &mut self,
        study_id: u64,
        generation: u64,
        bar: LiquidationBar,
    ) -> Result<bool, ExternalStudyError> {
        let series_ids = self
            .liquidation_series(study_id)
            .ok_or(ExternalStudyError::InstallationRejected)?;
        if self
            .external_study_outputs
            .get(&(study_id, 0))
            .is_some_and(|state| state.generation >= generation)
        {
            return Ok(false);
        }
        let (second, time) = validate_open_interest_timestamp(bar.timestamp_unix_nanos)?;
        let long_volume = open_interest_value(bar.long_volume)?;
        let short_volume = open_interest_value(bar.short_volume)?;
        let newest = series_ids
            .into_iter()
            .filter_map(|id| {
                self.data
                    .series_data(id)
                    .and_then(|(times, _)| times.last().copied())
            })
            .max();
        if newest.is_some_and(|newest| newest > second) {
            return Err(ExternalStudyError::NonIncreasingTimestamp);
        }
        let options = self.liquidation_options(study_id).unwrap_or_default();
        let (primary, secondary) =
            encode_liquidation_pair(options.style, short_volume, long_volume);
        for (id, value) in series_ids.into_iter().zip([primary, secondary]) {
            if !self.update_series_bar(id, time, [value; 4]) {
                return Err(ExternalStudyError::InstallationRejected);
            }
        }
        for output_index in 0..LIQUIDATION_OUTPUTS.len() {
            if let Some(state) = self
                .external_study_outputs
                .get_mut(&(study_id, output_index))
            {
                state.generation = generation;
            }
        }
        Ok(true)
    }

    /// The primary and secondary histogram series ids of a live Liquidations study.
    #[must_use]
    pub fn liquidation_series(&self, study_id: u64) -> Option<[SeriesId; 2]> {
        let series = |output_index| {
            self.external_study_outputs
                .get(&(study_id, output_index))
                .filter(|state| state.liquidations)
                .map(|state| state.series_id)
                .filter(|&id| self.series_entry(id).is_some())
        };
        Some([series(0)?, series(1)?])
    }

    pub(crate) fn is_liquidation_study(&self, study_id: u64) -> bool {
        self.external_study_outputs
            .iter()
            .any(|(&(candidate, _), state)| candidate == study_id && state.liquidations)
    }

    /// Decode `(short_volume, long_volume)` for a live Liquidations study at `logical_index`
    /// (or at its newest non-whitespace bar when `logical_index` is `None`).
    pub(crate) fn liquidation_bar_values_at(
        &self,
        study_id: u64,
        logical_index: Option<i64>,
    ) -> Option<(f64, f64)> {
        let [s0, s1] = self.liquidation_series(study_id)?;
        let options = self.liquidation_options(study_id)?;
        let p0 = self.data.plot(s0);
        let p1 = self.data.plot(s1);
        let target_index = match logical_index {
            Some(index) => index,
            None => {
                let i0 = p0
                    .last_non_whitespace_row(aeris_charts_core::TimePointIndex::MAX)
                    .and_then(|row| p0.index_at(row));
                let i1 = p1
                    .last_non_whitespace_row(aeris_charts_core::TimePointIndex::MAX)
                    .and_then(|row| p1.index_at(row));
                match (i0, i1) {
                    (Some(a), Some(b)) => a.max(b),
                    (Some(a), None) => a,
                    (None, Some(b)) => b,
                    (None, None) => return None,
                }
            }
        };
        let r0 = p0
            .search(
                target_index,
                aeris_charts_core::model::plot_list::MismatchDirection::None,
            )
            .filter(|&row| !p0.is_whitespace_row(row));
        let r1 = p1
            .search(
                target_index,
                aeris_charts_core::model::plot_list::MismatchDirection::None,
            )
            .filter(|&row| !p1.is_whitespace_row(row));
        if r0.is_none() && r1.is_none() {
            return None;
        }
        let v0 = r0.map_or(f64::NAN, |row| {
            p0.value_at(
                row,
                aeris_charts_core::model::plot_list::PlotValueIndex::Close,
            )
        });
        let v1 = r1.map_or(f64::NAN, |row| {
            p1.value_at(
                row,
                aeris_charts_core::model::plot_list::PlotValueIndex::Close,
            )
        });
        Some(decode_liquidation_pair(options.style, v0, v1))
    }

    fn apply_liquidation_presentation(
        &mut self,
        series_ids: [SeriesId; 2],
        options: LiquidationOptions,
        study_visible: bool,
        output_visibles: [bool; 2],
    ) {
        if let Some(entry) = self.series_entry_mut(series_ids[0]) {
            match options.style {
                LiquidationStyle::Split | LiquidationStyle::Stacked => {
                    entry.delta_histogram = false;
                    entry.market_tone = Some(MarketTone::Up);
                    entry.line_color = options.short_color.map(|color| color.to_css());
                    entry.up_color = None;
                    entry.down_color = None;
                }
                LiquidationStyle::Net => {
                    entry.delta_histogram = true;
                    entry.market_tone = None;
                    entry.line_color = None;
                    entry.up_color = options.short_color.map(|color| color.to_css());
                    entry.down_color = options.long_color.map(|color| color.to_css());
                }
            }
        }
        if let Some(entry) = self.series_entry_mut(series_ids[1]) {
            entry.delta_histogram = false;
            entry.market_tone = Some(MarketTone::Down);
            entry.line_color = options.long_color.map(|color| color.to_css());
            entry.up_color = None;
            entry.down_color = None;
        }
        self.set_series_visible(series_ids[0], study_visible && output_visibles[0]);
        self.set_series_visible(
            series_ids[1],
            options.style != LiquidationStyle::Net && study_visible && output_visibles[1],
        );
        self.invalidate_frame_series(series_ids[0]);
        self.invalidate_frame_series(series_ids[1]);
    }

    fn install_new_liquidation_series(
        &mut self,
        study_id: u64,
        title: &str,
        columns: &PreparedColumns,
    ) -> Result<SeriesId, ExternalStudyError> {
        let chrome = self.indicator_chrome;
        let series_id = self.add_series(SeriesKind::Histogram);
        if let Err(error) = set_series_data(self, series_id, columns) {
            let _ = self.remove_series(series_id);
            return Err(error);
        }
        if let Some(series) = self.series_entry_mut(series_id) {
            series.title = title.to_owned();
            series.title_visible = chrome.name_labels_visible;
            series.last_value_visible = chrome.value_labels_visible;
            series.price_line_visible = chrome.price_lines_visible;
            series.price_format.kind = PriceFormatKind::Volume;
            series.price_format.precision = LIQUIDATION_PRECISION;
            series.price_format.formatter = None;
        }
        let pane_key = (study_id, 0);
        let mut created_pane = None;
        let pane_index = if let Some(index) = self
            .external_study_panes
            .get(&pane_key)
            .and_then(|pane_id| self.pane_index_for_id(*pane_id))
        {
            index
        } else {
            self.external_study_panes.remove(&pane_key);
            let Some(index) = self.add_pane(false) else {
                let _ = self.remove_series(series_id);
                return Err(ExternalStudyError::InstallationRejected);
            };
            self.panes[index].stretch_factor = self.separate_pane_stretch();
            let Some(pane_id) = self.pane_stable_id(index) else {
                let _ = self.remove_series(series_id);
                let _ = self.remove_pane(index);
                return Err(ExternalStudyError::InstallationRejected);
            };
            created_pane = Some((pane_key, pane_id, index));
            index
        };
        if !self.try_set_series_pane_and_scale(series_id, pane_index, None, "right") {
            let _ = self.remove_series(series_id);
            if let Some((_, _, index)) = created_pane {
                let _ = self.remove_pane(index);
            }
            return Err(ExternalStudyError::InstallationRejected);
        }
        if let Some((key, pane_id, _)) = created_pane {
            self.external_study_panes.insert(key, pane_id);
        }
        Ok(series_id)
    }

    fn install_new_open_interest_series(
        &mut self,
        study_id: u64,
        columns: &PreparedOhlcColumns,
        options: OpenInterestOptions,
    ) -> Result<SeriesId, ExternalStudyError> {
        let chrome = self.indicator_chrome;
        let series_id = self.add_series(options.style.series_kind());
        if self
            .set_series_data(
                series_id,
                &columns.times,
                &columns.open,
                &columns.high,
                &columns.low,
                &columns.close,
            )
            .is_err()
        {
            let _ = self.remove_series(series_id);
            return Err(ExternalStudyError::InstallationRejected);
        }
        let title = crate::text::legend::OPEN_INTEREST
            .text(self.language)
            .to_owned();
        if let Some(series) = self.series_entry_mut(series_id) {
            series.title = title;
            series.title_visible = chrome.name_labels_visible;
            series.last_value_visible = chrome.value_labels_visible;
            series.price_line_visible = chrome.price_lines_visible;
            series.line_color = options.line_color.map(|color| color.to_css());
            series.up_color = options.up_color.map(|color| color.to_css());
            series.down_color = options.down_color.map(|color| color.to_css());
            series.price_format.kind = PriceFormatKind::Volume;
            series.price_format.precision = OPEN_INTEREST_PRECISION;
            series.price_format.formatter = None;
        }
        let pane_key = (study_id, 0);
        let mut created_pane = None;
        let pane_index = if let Some(index) = self
            .external_study_panes
            .get(&pane_key)
            .and_then(|pane_id| self.pane_index_for_id(*pane_id))
        {
            index
        } else {
            self.external_study_panes.remove(&pane_key);
            let Some(index) = self.add_pane(false) else {
                let _ = self.remove_series(series_id);
                return Err(ExternalStudyError::InstallationRejected);
            };
            self.panes[index].stretch_factor = self.separate_pane_stretch();
            let Some(pane_id) = self.pane_stable_id(index) else {
                let _ = self.remove_series(series_id);
                let _ = self.remove_pane(index);
                return Err(ExternalStudyError::InstallationRejected);
            };
            created_pane = Some((pane_key, pane_id, index));
            index
        };
        if !self.try_set_series_pane_and_scale(series_id, pane_index, None, "right") {
            let _ = self.remove_series(series_id);
            if let Some((_, _, index)) = created_pane {
                let _ = self.remove_pane(index);
            }
            return Err(ExternalStudyError::InstallationRejected);
        }
        if let Some((key, pane_id, _)) = created_pane {
            self.external_study_panes.insert(key, pane_id);
        }
        Ok(series_id)
    }

    fn install_new_external_study_series(
        &mut self,
        study_id: u64,
        output_index: usize,
        descriptor: ExternalStudyOutputDescriptor<'_>,
        columns: &PreparedColumns,
    ) -> Result<SeriesId, ExternalStudyError> {
        let source_price_format = self.series_entry(0).map(|series| {
            (
                series.price_format.kind,
                series.price_format.precision,
                series.price_format.min_move,
            )
        });
        let chrome = self.indicator_chrome;
        let series_id = self.add_series(series_kind(descriptor.plot));
        if let Err(error) = set_series_data(self, series_id, columns) {
            let _ = self.remove_series(series_id);
            return Err(error);
        }
        if let Some(series) = self.series_entry_mut(series_id) {
            series.title = descriptor.title.to_owned();
            series.title_visible = chrome.name_labels_visible;
            series.last_value_visible = chrome.value_labels_visible;
            series.price_line_visible = chrome.price_lines_visible;
            series.line_color = Some(default_output_color(output_index));
            series.line_width = Some(DEFAULT_EXTERNAL_STUDY_LINE_WIDTH);
            if let Some((kind, precision, min_move)) = source_price_format {
                series.price_format.kind = kind;
                series.price_format.precision = precision;
                series.price_format.min_move = min_move;
            }
        }
        self.adopt_scale_price_format(series_id);
        let mut created_pane = None;
        let pane_index = match descriptor.pane {
            ExternalStudyPaneTarget::Price => 0,
            ExternalStudyPaneTarget::Dedicated { group } => {
                let pane_key = (study_id, group);
                if let Some(index) = self
                    .external_study_panes
                    .get(&pane_key)
                    .and_then(|pane_id| self.pane_index_for_id(*pane_id))
                {
                    index
                } else {
                    self.external_study_panes.remove(&pane_key);
                    let Some(index) = self.add_pane(false) else {
                        let _ = self.remove_series(series_id);
                        return Err(ExternalStudyError::InstallationRejected);
                    };
                    self.panes[index].stretch_factor = self.separate_pane_stretch();
                    let Some(pane_id) = self.pane_stable_id(index) else {
                        let _ = self.remove_series(series_id);
                        let _ = self.remove_pane(index);
                        return Err(ExternalStudyError::InstallationRejected);
                    };
                    created_pane = Some((pane_key, pane_id, index));
                    index
                }
            }
        };
        if !self.try_set_series_pane_and_scale(
            series_id,
            pane_index,
            None,
            scale_id(descriptor.scale),
        ) {
            let _ = self.remove_series(series_id);
            if let Some((_, _, index)) = created_pane {
                let _ = self.remove_pane(index);
            }
            return Err(ExternalStudyError::InstallationRejected);
        }
        if let Some((key, pane_id, _)) = created_pane {
            self.external_study_panes.insert(key, pane_id);
        }
        Ok(series_id)
    }

    #[must_use]
    pub fn external_study_output_info(
        &self,
        study_id: u64,
        output_index: usize,
    ) -> Option<ExternalStudyOutputInfo> {
        let state = self.external_study_outputs.get(&(study_id, output_index))?;
        self.series_entry(state.series_id)?;
        Some(output_info(study_id, output_index, state))
    }

    #[must_use]
    pub fn external_study_outputs(&self) -> Vec<ExternalStudyOutputInfo> {
        self.external_study_outputs
            .iter()
            .filter_map(|(&(study_id, output_index), state)| {
                self.series_entry(state.series_id)
                    .map(|_| output_info(study_id, output_index, state))
            })
            .collect()
    }

    /// Whether the chart currently owns at least one live external study output.
    #[must_use]
    pub fn has_external_studies(&self) -> bool {
        self.external_study_outputs
            .values()
            .any(|state| self.series_entry(state.series_id).is_some())
    }

    /// Return the runtime study that owns a live output series.
    #[must_use]
    pub fn external_study_for_series(&self, series_id: SeriesId) -> Option<u64> {
        self.external_study_outputs
            .iter()
            .find_map(|(&(study_id, _), state)| {
                (state.series_id == series_id && self.series_entry(series_id).is_some())
                    .then_some(study_id)
            })
    }

    pub(crate) fn external_study_series(&self, study_id: u64) -> Vec<SeriesId> {
        self.external_study_outputs
            .iter()
            .filter_map(|(&(candidate, _), state)| {
                (candidate == study_id && self.series_entry(state.series_id).is_some())
                    .then_some(state.series_id)
            })
            .collect()
    }

    /// The whole-study visibility toggle. Outputs hidden through their own style stay hidden
    /// while the study is shown.
    #[must_use]
    pub(crate) fn external_momentum_histogram_series(&self) -> Vec<SeriesId> {
        self.external_study_outputs
            .values()
            .filter(|state| state.momentum_histogram)
            .map(|state| state.series_id)
            .collect()
    }

    pub fn external_study_visible(&self, study_id: u64) -> Option<bool> {
        self.external_study_outputs
            .iter()
            .find_map(|(&(candidate, _), state)| {
                (candidate == study_id && self.series_entry(state.series_id).is_some())
                    .then_some(state.study_visible)
            })
    }

    pub fn set_external_study_visible(&mut self, study_id: u64, visible: bool) -> bool {
        let mut changed = false;
        let mut painted = Vec::new();
        for (&(candidate, output_index), state) in &mut self.external_study_outputs {
            if candidate == study_id {
                changed |= state.study_visible != visible;
                state.study_visible = visible;
                let net_hidden_secondary = state.liquidations
                    && output_index == 1
                    && state.liquidation_options.style == LiquidationStyle::Net;
                painted.push((
                    state.series_id,
                    !net_hidden_secondary && visible && state.output_visible,
                ));
            }
        }
        for (id, visible) in painted {
            self.set_series_visible(id, visible);
        }
        changed
    }

    /// Current presentation of one output. `visible` is the output's own visibility,
    /// independent of the whole-study toggle, so a host can persist and restore it as is.
    #[must_use]
    pub fn external_study_output_style(
        &self,
        study_id: u64,
        output_index: usize,
    ) -> Option<IndicatorOutputStyle> {
        let state = self.external_study_outputs.get(&(study_id, output_index))?;
        let series = self.series_entry(state.series_id)?;
        Some(IndicatorOutputStyle {
            visible: state.output_visible,
            ..indicator_output_style(series)
        })
    }

    /// Replace one output's color, width, line style, and visibility. The style survives later
    /// data pushes and whole-study visibility toggles. An invalid width, line style, or color is
    /// rejected before anything changes.
    pub fn set_external_study_output_style(
        &mut self,
        study_id: u64,
        output_index: usize,
        style: IndicatorOutputStyle,
    ) -> bool {
        let key = (study_id, output_index);
        let Some(state) = self.external_study_outputs.get(&key) else {
            return false;
        };
        let (series_id, study_visible) = (state.series_id, state.study_visible);
        if !output_style_is_valid(&style) || self.series_entry(series_id).is_none() {
            return false;
        }
        let output_visible = style.visible;
        if let Some(state) = self.external_study_outputs.get_mut(&key) {
            state.output_visible = output_visible;
        }
        self.write_output_style(series_id, style, output_visible && study_visible)
    }

    /// Drop the bookkeeping of outputs whose series were removed directly, so the slot no longer
    /// counts toward capacity and a later push for that output installs it again instead of
    /// failing against a dead series.
    pub(crate) fn forget_removed_external_study_outputs(&mut self, removed: &[SeriesId]) {
        let before = self.external_study_outputs.len();
        self.external_study_outputs
            .retain(|_, state| !removed.contains(&state.series_id));
        if self.external_study_outputs.len() != before {
            let outputs = &self.external_study_outputs;
            self.external_study_panes.retain(|&(study_id, _), _| {
                outputs.keys().any(|&(candidate, _)| candidate == study_id)
            });
        }
    }

    /// Restore the chart-owned default color and width of every output. Visibility survives,
    /// matching the reset contract of every other series.
    pub(crate) fn reset_external_study_styles_to_defaults(&mut self) {
        let outputs = self
            .external_study_outputs
            .iter()
            .map(|(&(_, output_index), state)| {
                (
                    output_index,
                    state.series_id,
                    state.open_interest || state.liquidations,
                )
            })
            .collect::<Vec<_>>();
        for (output_index, series_id, is_specialized) in outputs {
            if let Some(series) = self.series_entry_mut(series_id) {
                series.line_color = (!is_specialized && series.market_tone.is_none())
                    .then(|| default_output_color(output_index));
                series.line_width = Some(DEFAULT_EXTERNAL_STUDY_LINE_WIDTH);
            }
        }
    }

    pub fn remove_external_studies(&mut self, study_ids: &[u64]) -> bool {
        if study_ids.is_empty() {
            return false;
        }
        let keys = self
            .external_study_outputs
            .keys()
            .copied()
            .filter(|(study_id, _)| study_ids.contains(study_id))
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return false;
        }
        for key in keys {
            if let Some(state) = self.external_study_outputs.remove(&key) {
                if self.selected_series() == Some(state.series_id) {
                    self.set_selected_series(None);
                }
                let _ = self.remove_series(state.series_id);
            }
        }
        self.external_study_panes
            .retain(|(study_id, _), _| !study_ids.contains(study_id));
        true
    }

    pub(crate) fn apply_indicator_chrome_to_external_studies(
        &mut self,
        options: IndicatorChromeOptions,
    ) -> bool {
        let ids = self
            .external_study_outputs
            .values()
            .map(|state| state.series_id)
            .collect::<Vec<_>>();
        let mut changed = false;
        for id in ids {
            if let Some(series) = self.series_entry_mut(id) {
                changed |= series.title_visible != options.name_labels_visible
                    || series.last_value_visible != options.value_labels_visible
                    || series.price_line_visible != options.price_lines_visible;
                series.title_visible = options.name_labels_visible;
                series.last_value_visible = options.value_labels_visible;
                series.price_line_visible = options.price_lines_visible;
            }
        }
        changed
    }
}

fn output_info(
    study_id: u64,
    output_index: usize,
    state: &ExternalStudyOutputState,
) -> ExternalStudyOutputInfo {
    ExternalStudyOutputInfo {
        study_id,
        output_index,
        series_id: state.series_id,
        generation: state.generation,
        settings_available: state.settings_available,
        legend_label: state.legend_label.clone(),
        input_requirements: state.input_requirements,
    }
}

fn validate_descriptor(
    descriptor: ExternalStudyOutputDescriptor<'_>,
) -> Result<(), ExternalStudyError> {
    if descriptor.title.len() > MAX_EXTERNAL_STUDY_TITLE_BYTES
        || descriptor
            .legend_label
            .is_some_and(|label| label.len() > MAX_EXTERNAL_STUDY_LEGEND_LABEL_BYTES)
    {
        return Err(ExternalStudyError::MetadataTooLong);
    }
    if descriptor.threshold_region.is_some_and(|region| {
        !region.lower.is_finite()
            || !region.upper.is_finite()
            || region.lower >= region.upper
            || !matches!(
                descriptor.plot,
                ExternalStudyPlotKind::Line | ExternalStudyPlotKind::Area
            )
    }) || (descriptor.point_style == ExternalStudyPointStyle::MomentumHistogram
        && descriptor.plot != ExternalStudyPlotKind::Histogram)
    {
        return Err(ExternalStudyError::InvalidPresentation);
    }
    Ok(())
}

fn prepare_columns(
    timestamps_unix_nanos: &[i64],
    values: &[Option<f64>],
) -> Result<PreparedColumns, ExternalStudyError> {
    if timestamps_unix_nanos.len() != values.len() {
        return Err(ExternalStudyError::LengthMismatch);
    }
    let mut times = Vec::with_capacity(timestamps_unix_nanos.len());
    let mut scalar = Vec::with_capacity(values.len());
    let mut previous_second = None;
    for (&timestamp, value) in timestamps_unix_nanos.iter().zip(values) {
        if timestamp.rem_euclid(NANOS_PER_SECOND) != 0 {
            return Err(ExternalStudyError::UnsupportedTimestampPrecision);
        }
        let second = timestamp.div_euclid(NANOS_PER_SECOND);
        let time = second as f64;
        if validate_timestamp(time).is_err() || time as i64 != second {
            return Err(ExternalStudyError::UnsupportedTimestampPrecision);
        }
        if previous_second.is_some_and(|previous| previous >= second) {
            return Err(ExternalStudyError::NonIncreasingTimestamp);
        }
        previous_second = Some(second);
        times.push(time);
        scalar.push(match value {
            None => f64::NAN,
            Some(value) if value.is_finite() && value.abs() <= MAX_SAFE_VALUE => *value,
            Some(_) => return Err(ExternalStudyError::InvalidValue),
        });
    }
    Ok(PreparedColumns { times, scalar })
}

fn set_series_data(
    engine: &mut ChartEngine,
    series_id: SeriesId,
    columns: &PreparedColumns,
) -> Result<(), ExternalStudyError> {
    engine
        .set_series_data(
            series_id,
            &columns.times,
            &columns.scalar,
            &columns.scalar,
            &columns.scalar,
            &columns.scalar,
        )
        .map(|_| ())
        .map_err(|_| ExternalStudyError::InstallationRejected)
}

fn options_volume_descriptor(
    language: crate::ChartLanguage,
    title: crate::text::Phrase,
) -> ExternalStudyOutputDescriptor<'static> {
    let title = title.text(language);
    ExternalStudyOutputDescriptor {
        title,
        legend_label: Some(title),
        plot: ExternalStudyPlotKind::Histogram,
        pane: ExternalStudyPaneTarget::Dedicated { group: 0 },
        scale: ExternalStudyScaleTarget::Primary,
        settings_available: false,
        threshold_region: None,
        point_style: ExternalStudyPointStyle::Uniform,
        input_requirements: ExternalStudyInputRequirements::NONE,
    }
}

/// A host options volume: `None` stays an empty bar; a value must be a finite, non-negative
/// count within the scalar range.
fn options_volume_value(value: Option<f64>) -> Result<Option<f64>, ExternalStudyError> {
    match value {
        Some(value) if !(value.is_finite() && (0.0..=MAX_SAFE_VALUE).contains(&value)) => {
            Err(ExternalStudyError::InvalidValue)
        }
        value => Ok(value),
    }
}

fn open_interest_value(value: f64) -> Result<f64, ExternalStudyError> {
    if value.is_finite() && (0.0..=MAX_SAFE_VALUE).contains(&value) {
        Ok(value)
    } else {
        Err(ExternalStudyError::InvalidValue)
    }
}

fn validate_open_interest_timestamp(
    timestamp_unix_nanos: i64,
) -> Result<(i64, f64), ExternalStudyError> {
    if timestamp_unix_nanos.rem_euclid(NANOS_PER_SECOND) != 0 {
        return Err(ExternalStudyError::UnsupportedTimestampPrecision);
    }
    let second = timestamp_unix_nanos.div_euclid(NANOS_PER_SECOND);
    let time = second as f64;
    if validate_timestamp(time).is_err() || time as i64 != second {
        return Err(ExternalStudyError::UnsupportedTimestampPrecision);
    }
    Ok((second, time))
}

struct PreparedLiquidationColumns {
    seconds: Vec<i64>,
    times: Vec<f64>,
    primary: Vec<f64>,
    secondary: Vec<f64>,
}

fn encode_liquidation_pair(style: LiquidationStyle, short: f64, long: f64) -> (f64, f64) {
    match style {
        LiquidationStyle::Split => (
            if short > 0.0 { short } else { f64::NAN },
            if long > 0.0 { -long } else { f64::NAN },
        ),
        LiquidationStyle::Stacked => (
            if short > 0.0 { short + long } else { f64::NAN },
            if long > 0.0 { long } else { f64::NAN },
        ),
        LiquidationStyle::Net => (
            if short > 0.0 || long > 0.0 {
                short - long
            } else {
                f64::NAN
            },
            if long > 0.0 { long } else { f64::NAN },
        ),
    }
}

fn decode_liquidation_pair(style: LiquidationStyle, s0: f64, s1: f64) -> (f64, f64) {
    match style {
        LiquidationStyle::Split => (
            if s0.is_finite() { s0.max(0.0) } else { 0.0 },
            if s1.is_finite() { (-s1).max(0.0) } else { 0.0 },
        ),
        LiquidationStyle::Stacked => {
            let long = if s1.is_finite() { s1.max(0.0) } else { 0.0 };
            let short = if s0.is_finite() {
                (s0 - long).max(0.0)
            } else {
                0.0
            };
            (short, long)
        }
        LiquidationStyle::Net => {
            let long = if s1.is_finite() { s1.max(0.0) } else { 0.0 };
            let short = if s0.is_finite() {
                (s0 + long).max(0.0)
            } else {
                0.0
            };
            (short, long)
        }
    }
}

fn prepare_liquidation_columns(
    bars: &[LiquidationBar],
    style: LiquidationStyle,
) -> Result<PreparedLiquidationColumns, ExternalStudyError> {
    let mut seconds = Vec::with_capacity(bars.len());
    let mut times = Vec::with_capacity(bars.len());
    let mut primary = Vec::with_capacity(bars.len());
    let mut secondary = Vec::with_capacity(bars.len());
    let mut previous_second: Option<i64> = None;

    for bar in bars {
        let (second, time) = validate_open_interest_timestamp(bar.timestamp_unix_nanos)?;
        if previous_second.is_some_and(|prev| prev >= second) {
            return Err(ExternalStudyError::NonIncreasingTimestamp);
        }
        previous_second = Some(second);
        let long = open_interest_value(bar.long_volume)?;
        let short = open_interest_value(bar.short_volume)?;
        let (p, s) = encode_liquidation_pair(style, short, long);
        seconds.push(second);
        times.push(time);
        primary.push(p);
        secondary.push(s);
    }

    Ok(PreparedLiquidationColumns {
        seconds,
        times,
        primary,
        secondary,
    })
}

fn merge_liquidation_series(
    primary_points: &[crate::SeriesDataPoint],
    secondary_points: &[crate::SeriesDataPoint],
    from_style: LiquidationStyle,
    to_style: LiquidationStyle,
) -> PreparedLiquidationColumns {
    let mut seconds = Vec::with_capacity(primary_points.len().max(secondary_points.len()));
    let mut times = Vec::with_capacity(seconds.capacity());
    let mut primary = Vec::with_capacity(seconds.capacity());
    let mut secondary = Vec::with_capacity(seconds.capacity());
    let (mut i, mut j) = (0usize, 0usize);
    while i < primary_points.len() || j < secondary_points.len() {
        let (time, v0, v1) = match (primary_points.get(i), secondary_points.get(j)) {
            (Some(a), Some(b)) if a.time == b.time => {
                i += 1;
                j += 1;
                (a.time, a.close, b.close)
            }
            (Some(a), Some(b)) if a.time < b.time => {
                i += 1;
                (a.time, a.close, f64::NAN)
            }
            (Some(a), None) => {
                i += 1;
                (a.time, a.close, f64::NAN)
            }
            (_, Some(b)) => {
                j += 1;
                (b.time, f64::NAN, b.close)
            }
            (None, None) => break,
        };
        let (short, long) = decode_liquidation_pair(from_style, v0, v1);
        let (p, s) = encode_liquidation_pair(to_style, short, long);
        seconds.push(time);
        times.push(time as f64);
        primary.push(p);
        secondary.push(s);
    }
    PreparedLiquidationColumns {
        seconds,
        times,
        primary,
        secondary,
    }
}

fn prepare_open_interest_columns(
    bars: &[OpenInterestBar],
    previous_forming_wick: Option<(i64, u64, u64)>,
) -> Result<PreparedOhlcColumns, ExternalStudyError> {
    let mut seconds = Vec::with_capacity(bars.len());
    let mut times = Vec::with_capacity(bars.len());
    let mut open = Vec::with_capacity(bars.len());
    let mut high = Vec::with_capacity(bars.len());
    let mut low = Vec::with_capacity(bars.len());
    let mut close = Vec::with_capacity(bars.len());
    let mut previous_second: Option<i64> = None;
    let mut previous_close: Option<f64> = None;

    for bar in bars {
        let (second, time) = validate_open_interest_timestamp(bar.timestamp_unix_nanos)?;
        if previous_second.is_some_and(|prev| prev >= second) {
            return Err(ExternalStudyError::NonIncreasingTimestamp);
        }
        previous_second = Some(second);
        let c = open_interest_value(bar.close)?;
        let o = match bar.open {
            Some(value) => open_interest_value(value)?,
            None => previous_close.unwrap_or(c),
        };
        let raw_h = match bar.high {
            Some(value) => open_interest_value(value)?,
            None => c,
        };
        let raw_l = match bar.low {
            Some(value) => open_interest_value(value)?,
            None => c,
        };
        let h = o.max(c).max(raw_h);
        let l = o.min(c).min(raw_l);
        seconds.push(second);
        times.push(time);
        open.push(o);
        high.push(h);
        low.push(l);
        close.push(c);
        previous_close = Some(c);
    }

    let forming_wick = if let Some(&last_sec) = seconds.last()
        && let (Some(last_high), Some(last_low)) = (high.last_mut(), low.last_mut())
    {
        if let Some((prev_sec, prev_high_bits, prev_low_bits)) = previous_forming_wick
            && prev_sec == last_sec
        {
            *last_high = last_high.max(f64::from_bits(prev_high_bits));
            *last_low = last_low.min(f64::from_bits(prev_low_bits));
        }
        Some((last_sec, last_high.to_bits(), last_low.to_bits()))
    } else {
        None
    };

    Ok(PreparedOhlcColumns {
        seconds,
        times,
        open,
        high,
        low,
        close,
        forming_wick,
    })
}

fn default_output_color(output_index: usize) -> String {
    DEFAULT_EXTERNAL_STUDY_COLORS[output_index % DEFAULT_EXTERNAL_STUDY_COLORS.len()].to_owned()
}

/// Descriptor-driven presentation refreshed on every push. Color and width are set once at
/// creation and then belong to the host's style, so they are deliberately not touched here.
fn apply_presentation(
    engine: &mut ChartEngine,
    series_id: SeriesId,
    descriptor: ExternalStudyOutputDescriptor<'_>,
) {
    if let Some(series) = engine.series_entry_mut(series_id) {
        series.title = descriptor.title.to_owned();
    }
    let threshold = descriptor
        .threshold_region
        .map(|region| SeriesThresholdRegion {
            lower: region.lower,
            upper: region.upper,
        });
    let _ = engine.set_series_threshold_region(series_id, threshold);
    if descriptor.point_style == ExternalStudyPointStyle::MomentumHistogram {
        let _ = engine.apply_momentum_histogram_colors(series_id);
    }
}

const fn series_kind(plot: ExternalStudyPlotKind) -> SeriesKind {
    match plot {
        ExternalStudyPlotKind::Line => SeriesKind::Line,
        ExternalStudyPlotKind::Histogram => SeriesKind::Histogram,
        ExternalStudyPlotKind::Area => SeriesKind::Area,
    }
}

const fn scale_id(scale: ExternalStudyScaleTarget) -> &'static str {
    match scale {
        ExternalStudyScaleTarget::Primary => "right",
        ExternalStudyScaleTarget::Left => "left",
        ExternalStudyScaleTarget::Overlay => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor<'a>(
        title: &'a str,
        pane: ExternalStudyPaneTarget,
    ) -> ExternalStudyOutputDescriptor<'a> {
        ExternalStudyOutputDescriptor {
            title,
            legend_label: None,
            plot: ExternalStudyPlotKind::Line,
            pane,
            scale: ExternalStudyScaleTarget::Primary,
            settings_available: true,
            threshold_region: None,
            point_style: ExternalStudyPointStyle::Uniform,
            input_requirements: ExternalStudyInputRequirements::BARS
                .with(ExternalStudyInputStream::Trades),
        }
    }

    #[test]
    fn options_volume_is_one_compact_pane_of_theme_signed_volume_columns() {
        use crate::{ChartTheme, FinancialLegendRequest};
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let bars = [1.0, 2.0, 3.0];
        chart
            .set_series_data(0, &bars, &bars, &bars, &bars, &bars)
            .unwrap();
        let times = [1_000_000_000, 2_000_000_000, 3_000_000_000];
        assert_eq!(
            chart.set_options_volume(
                9,
                1,
                &times,
                &[Some(1500.0), Some(0.0), Some(30.0)],
                &[Some(1100.0), Some(20.0), Some(5.0)],
            ),
            Ok(true)
        );
        let [calls, puts] = chart.options_volume_series(9).unwrap();
        let closes = |chart: &ChartEngine, id| chart.data.series_data(id).unwrap().1[3].to_vec();
        assert_eq!(closes(&chart, calls), [1500.0, 0.0, 30.0]);
        assert_eq!(closes(&chart, puts), [-1100.0, -20.0, -5.0]);
        assert_eq!(chart.panes.len(), 2, "both outputs share one separate pane");
        assert_eq!(chart.panes[1].stretch_factor, chart.separate_pane_stretch());
        for (id, title) in [(calls, "Calls"), (puts, "Puts")] {
            let series = chart.series_entry(id).unwrap();
            assert_eq!(
                (series.kind, series.pane_index, series.title.as_str()),
                (SeriesKind::Histogram, 1, title)
            );
            assert_eq!(series.price_format.kind, PriceFormatKind::Volume);
        }

        let stroke = |chart: &ChartEngine, id| {
            crate::frame::series_stroke_color(
                chart.series_entry(id).unwrap(),
                chart.market_colors(),
                chart.surface_theme(),
            )
        };
        for theme in [ChartTheme::Dark, ChartTheme::Light] {
            chart.set_theme(theme);
            let market = chart.market_colors();
            assert_eq!(
                (stroke(&chart, calls), stroke(&chart, puts)),
                (market.up, market.down)
            );
        }
        let legend = chart.financial_legend(FinancialLegendRequest {
            logical_index: None,
            primary_title: "BTC",
            show_primary_ohlc: false,
            primary_values_series: None,
            leading_series: &[],
            trailing_series: &[],
        });
        let row = legend
            .iter()
            .find(|row| row.identity == crate::FinancialLegendIdentity::ExternalStudy(9))
            .unwrap();
        assert_eq!(
            row.title,
            crate::text::legend::OPTIONS_VOLUME.text(crate::ChartLanguage::English)
        );
        let market = chart.market_colors();
        assert_eq!(
            row.values
                .iter()
                .map(|value| value.color.clone())
                .collect::<Vec<_>>(),
            [Some(market.up.to_css()), Some(market.down.to_css())]
        );

        // A host style pins a color; the style reset hands the side's color back.
        let pinned = IndicatorOutputStyle {
            line_color: Some("#123456".into()),
            ..chart.external_study_output_style(9, 0).unwrap()
        };
        assert!(chart.set_external_study_output_style(9, 0, pinned));
        assert_eq!(stroke(&chart, calls), crate::Color::rgb(0x12, 0x34, 0x56));
        chart.reset_external_study_styles_to_defaults();
        assert_eq!(stroke(&chart, calls), chart.market_colors().up);

        assert_eq!(
            chart.update_options_volume(9, 2, 3_000_000_000, 40.0, 60.0),
            Ok(true)
        );
        assert_eq!(
            chart.update_options_volume(9, 3, 4_000_000_000, 7.0, 8.0),
            Ok(true)
        );
        assert_eq!(closes(&chart, calls), [1500.0, 0.0, 40.0, 7.0]);
        assert_eq!(closes(&chart, puts), [-1100.0, -20.0, -60.0, -8.0]);
        assert_eq!(
            chart.update_options_volume(9, 3, 5_000_000_000, 1.0, 1.0),
            Ok(false)
        );
        assert_eq!(
            chart.update_options_volume(9, 4, 2_000_000_000, 1.0, 1.0),
            Err(ExternalStudyError::NonIncreasingTimestamp)
        );
        assert_eq!(
            chart.update_options_volume(9, 4, 5_000_000_000, -1.0, 1.0),
            Err(ExternalStudyError::InvalidValue)
        );
        assert_eq!(
            chart.set_options_volume(9, 4, &times, &[Some(1.0); 3], &[Some(-1.0); 3]),
            Err(ExternalStudyError::InvalidValue)
        );
        assert_eq!(
            closes(&chart, calls).len(),
            4,
            "a rejected push changes nothing"
        );
        assert_eq!(
            chart.install_external_study_output(
                9,
                2,
                descriptor("Other", ExternalStudyPaneTarget::Price),
                9,
                &times,
                &[Some(1.0); 3]
            ),
            Err(ExternalStudyError::InstallationRejected)
        );
        chart
            .install_external_study_output(
                7,
                0,
                descriptor("Signal", ExternalStudyPaneTarget::Price),
                1,
                &times,
                &[Some(1.0); 3],
            )
            .unwrap();
        assert_eq!(
            chart.set_options_volume(7, 2, &times, &[Some(1.0); 3], &[Some(1.0); 3]),
            Err(ExternalStudyError::InstallationRejected),
            "another study's id is never taken over"
        );

        assert!(chart.remove_external_studies(&[9]));
        assert_eq!(chart.options_volume_series(9), None);
        assert_eq!(chart.panes.len(), 1, "removal releases the pane");
    }

    #[test]
    fn external_study_transaction_owns_validation_generation_and_groups() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times = [1_000_000_000, 2_000_000_000];
        let values = [Some(1.0), Some(2.0)];
        let pane = ExternalStudyPaneTarget::Dedicated { group: 3 };
        assert_eq!(
            chart.install_external_study_output(
                7,
                0,
                descriptor("Signal", pane),
                1,
                &times,
                &values
            ),
            Ok(true)
        );
        assert_eq!(
            chart.install_external_study_output(
                7,
                1,
                descriptor("Average", pane),
                1,
                &times,
                &values
            ),
            Ok(true)
        );
        let outputs = chart.external_study_outputs();
        assert_eq!(outputs.len(), 2);
        assert!(chart.has_external_studies());
        assert_eq!(
            outputs[0].input_requirements,
            descriptor("", pane).input_requirements
        );
        let first = chart.series_entry(outputs[0].series_id).unwrap();
        let second = chart.series_entry(outputs[1].series_id).unwrap();
        assert_ne!(first.pane_index, 0);
        assert_eq!(first.pane_index, second.pane_index);
        assert_eq!(
            chart.external_study_for_series(outputs[1].series_id),
            Some(7)
        );
        chart.set_selected_series(Some(outputs[1].series_id));
        assert_eq!(chart.selected_series(), Some(outputs[1].series_id));
        assert_eq!(
            chart.selected_series_members().collect::<Vec<_>>(),
            outputs
                .iter()
                .map(|output| output.series_id)
                .collect::<Vec<_>>()
        );

        assert_eq!(
            chart.install_external_study_output(
                7,
                0,
                descriptor("Signal", pane),
                1,
                &times,
                &[Some(9.0), Some(9.0)]
            ),
            Ok(false),
            "a duplicate generation must not replace current data"
        );
        assert!(chart.set_external_study_visible(7, false));
        assert_eq!(chart.external_study_visible(7), Some(false));
        assert!(chart.remove_external_studies(&[7]));
        assert!(chart.external_study_outputs().is_empty());
        assert!(!chart.has_external_studies());
        assert_eq!(chart.external_study_for_series(outputs[0].series_id), None);
    }

    #[test]
    fn dedicated_external_study_pane_matches_engine_study_pane_height() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times = [1_000_000_000, 2_000_000_000];
        let values = [Some(1.0), Some(2.0)];
        chart
            .set_series_data(
                0,
                &[1.0, 2.0],
                &[1.0, 2.0],
                &[1.0, 2.0],
                &[1.0, 2.0],
                &[1.0, 2.0],
            )
            .unwrap();
        chart.panes[0].stretch_factor = 2.0;
        let rsi = chart.add_rsi(0, 2).unwrap();
        let rsi_pane = chart.series_entry(rsi).unwrap().pane_index;
        assert_eq!(
            chart.install_external_study_output(
                5,
                0,
                descriptor("Signal", ExternalStudyPaneTarget::Dedicated { group: 0 }),
                1,
                &times,
                &values,
            ),
            Ok(true)
        );
        let external = chart.external_study_outputs()[0].series_id;
        let external_pane = chart.series_entry(external).unwrap().pane_index;
        assert_ne!(external_pane, rsi_pane);
        assert_eq!(
            chart.panes[rsi_pane].stretch_factor,
            2.0 * crate::SEPARATE_INDICATOR_PANE_STRETCH
        );
        assert_eq!(
            chart.panes[external_pane].stretch_factor,
            chart.panes[rsi_pane].stretch_factor
        );
    }

    fn install_five_outputs(chart: &mut ChartEngine, study_id: u64, generation: u64) {
        for output_index in 0..5 {
            assert_eq!(
                chart.install_external_study_output(
                    study_id,
                    output_index,
                    descriptor("Five outputs", ExternalStudyPaneTarget::Price),
                    generation,
                    &[1_000_000_000, 2_000_000_000],
                    &[Some(1.0), Some(2.0)],
                ),
                Ok(true)
            );
        }
    }

    fn painted(chart: &ChartEngine, study_id: u64, output_index: usize) -> bool {
        let info = chart
            .external_study_output_info(study_id, output_index)
            .unwrap();
        chart.series_entry(info.series_id).unwrap().visible
    }

    #[test]
    fn external_study_output_style_survives_pushes_and_study_toggles() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        install_five_outputs(&mut chart, 9, 1);
        let defaults = chart.external_study_output_style(9, 2).unwrap();
        assert_eq!(
            defaults.line_color.as_deref(),
            Some(DEFAULT_EXTERNAL_STUDY_COLORS[2])
        );
        assert_eq!(defaults.line_width, Some(DEFAULT_EXTERNAL_STUDY_LINE_WIDTH));
        assert!(defaults.visible);

        let styled = IndicatorOutputStyle {
            line_color: Some("#ff00ff".to_owned()),
            line_width: Some(3.0),
            line_style: 2,
            ..defaults.clone()
        };
        assert!(chart.set_external_study_output_style(9, 2, styled.clone()));
        let hidden = IndicatorOutputStyle {
            visible: false,
            ..chart.external_study_output_style(9, 4).unwrap()
        };
        assert!(chart.set_external_study_output_style(9, 4, hidden));
        assert!(
            !chart.set_external_study_output_style(
                9,
                1,
                IndicatorOutputStyle {
                    line_width: Some(0.0),
                    ..defaults.clone()
                }
            ),
            "an invalid width is rejected"
        );
        assert!(!chart.set_external_study_output_style(9, 5, defaults.clone()));

        install_five_outputs(&mut chart, 9, 2);
        assert_eq!(
            chart.external_study_output_style(9, 2),
            Some(styled),
            "a data push keeps the host style"
        );
        assert!(!painted(&chart, 9, 4), "a hidden ribbon line stays hidden");
        assert!(painted(&chart, 9, 3));

        assert!(chart.set_external_study_visible(9, false));
        assert_eq!(chart.external_study_visible(9), Some(false));
        assert!((0..5).all(|index| !painted(&chart, 9, index)));
        assert!(chart.set_external_study_visible(9, true));
        assert!(
            !painted(&chart, 9, 4),
            "showing the study does not bring back a hidden line"
        );
        assert!((0..4).all(|index| painted(&chart, 9, index)));
        assert!(!chart.external_study_output_style(9, 4).unwrap().visible);
    }

    #[test]
    fn external_study_outputs_join_hidden_studies_and_reset_restores_defaults() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times = [1_000_000_000];
        let values = [Some(1.0)];
        let line = descriptor("Study", ExternalStudyPaneTarget::Price);
        assert_eq!(
            chart.install_external_study_output(4, 0, line, 1, &times, &values),
            Ok(true)
        );
        assert!(chart.set_external_study_visible(4, false));
        assert_eq!(
            chart.install_external_study_output(4, 1, line, 1, &times, &values),
            Ok(true)
        );
        assert!(
            !painted(&chart, 4, 1),
            "a late output joins its study hidden"
        );

        let style = IndicatorOutputStyle {
            line_color: Some("#123456".to_owned()),
            line_width: Some(1.0),
            visible: false,
            ..chart.external_study_output_style(4, 0).unwrap()
        };
        assert!(chart.set_external_study_output_style(4, 0, style));
        chart.reset_style_to_defaults();
        let reset = chart.external_study_output_style(4, 0).unwrap();
        assert_eq!(
            reset.line_color.as_deref(),
            Some(DEFAULT_EXTERNAL_STUDY_COLORS[0])
        );
        assert_eq!(reset.line_width, Some(DEFAULT_EXTERNAL_STUDY_LINE_WIDTH));
        assert!(!reset.visible, "a style reset keeps visibility");
    }

    #[test]
    fn removing_one_external_output_series_releases_its_slot() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        install_five_outputs(&mut chart, 11, 1);
        let removed = chart.external_study_output_info(11, 3).unwrap().series_id;
        assert!(chart.remove_series(removed));
        assert_eq!(chart.external_study_output_info(11, 3), None);
        assert_eq!(chart.external_study_outputs().len(), 4);
        assert_eq!(
            chart.install_external_study_output(
                11,
                2,
                descriptor("Five outputs", ExternalStudyPaneTarget::Price),
                2,
                &[1_000_000_000],
                &[Some(5.0)],
            ),
            Ok(true),
            "the remaining outputs keep updating"
        );
        assert_eq!(
            chart.install_external_study_output(
                11,
                3,
                descriptor("Five outputs", ExternalStudyPaneTarget::Price),
                2,
                &[1_000_000_000],
                &[Some(5.0)],
            ),
            Ok(true),
            "a later push installs the removed output again instead of failing"
        );
        assert_ne!(
            chart.external_study_output_info(11, 3).unwrap().series_id,
            removed
        );
    }

    #[test]
    fn invalid_external_study_input_is_atomic_and_chrome_is_retained() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let chrome = IndicatorChromeOptions {
            name_labels_visible: false,
            value_labels_visible: false,
            price_lines_visible: false,
        };
        assert!(chart.set_indicator_chrome_options(chrome));
        assert_eq!(
            chart.install_external_study_output(
                8,
                0,
                descriptor("Bad", ExternalStudyPaneTarget::Price),
                1,
                &[1_000_000_001],
                &[Some(1.0)],
            ),
            Err(ExternalStudyError::UnsupportedTimestampPrecision)
        );
        assert!(chart.external_study_outputs().is_empty());

        assert_eq!(
            chart.install_external_study_output(
                8,
                0,
                descriptor("Good", ExternalStudyPaneTarget::Price),
                2,
                &[1_000_000_000],
                &[Some(1.0)],
            ),
            Ok(true)
        );
        let output = chart.external_study_outputs().pop().unwrap();
        let series = chart.series_entry(output.series_id).unwrap();
        assert!(!series.title_visible);
        assert!(!series.last_value_visible);
        assert!(!series.price_line_visible);
    }

    #[test]
    fn open_interest_is_one_candlestick_pane_with_continuous_ohlc_wicks_and_legend() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        // Primary series covers t = 2..=4; OI includes t = 1 so open[2] inherits close[1] while
        // leading t = 1 is trimmed to avoid prepending empty price whitespace.
        chart
            .set_series_data(
                0,
                &[2.0, 3.0, 4.0],
                &[100.0, 101.0, 102.0],
                &[103.0, 104.0, 105.0],
                &[99.0, 100.0, 101.0],
                &[101.0, 102.0, 103.0],
            )
            .unwrap();

        let bars = [
            OpenInterestBar {
                timestamp_unix_nanos: 1_000_000_000,
                open: None,
                high: Some(10_200.0),
                low: Some(9_800.0),
                close: 10_000.0,
            },
            OpenInterestBar {
                timestamp_unix_nanos: 2_000_000_000,
                open: None,
                high: Some(10_600.0),
                low: Some(9_950.0),
                close: 10_500.0,
            },
            OpenInterestBar {
                timestamp_unix_nanos: 3_000_000_000,
                open: None,
                high: Some(10_550.0),
                low: Some(10_100.0),
                close: 10_200.0,
            },
            OpenInterestBar {
                timestamp_unix_nanos: 4_000_000_000,
                open: None,
                high: Some(10_700.0),
                low: Some(10_150.0),
                close: 10_400.0,
            },
        ];

        assert_eq!(chart.set_open_interest(11, 1, &bars), Ok(true));
        let oi_id = chart
            .open_interest_series(11)
            .expect("open interest series");
        let entry = chart.series_entry(oi_id).expect("series entry");
        assert_eq!(entry.kind, SeriesKind::Line);
        assert_eq!(entry.price_format.kind, PriceFormatKind::Volume);
        assert_eq!(entry.pane_index, 1);
        assert_eq!(chart.panes.len(), 2);

        // Switch to Candles to verify OHLC derivation and candlestick legend readout.
        assert!(chart.set_open_interest_options(
            11,
            OpenInterestOptions {
                style: OpenInterestStyle::Candles,
                ..OpenInterestOptions::default()
            },
        ));
        assert_eq!(
            chart.series_entry(oi_id).unwrap().kind,
            SeriesKind::Candlestick
        );

        // Leading t=1 was trimmed, yet t=2 inherited open=10_000.0 from t=1's close.
        let points = chart.series_data(oi_id);
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].time, 2);
        assert_eq!(points[0].open, 10_000.0);
        assert_eq!(points[0].high, 10_600.0);
        assert_eq!(points[0].low, 9_950.0);
        assert_eq!(points[0].close, 10_500.0);

        // Refreshing the forming bar (t=4) preserves earlier intra-bar high/low wicks.
        let mut refreshed = bars;
        refreshed[3].high = Some(10_450.0);
        refreshed[3].low = Some(10_350.0);
        refreshed[3].close = 10_420.0;
        assert_eq!(chart.set_open_interest(11, 2, &refreshed), Ok(true));
        let last = chart.series_data(oi_id).pop().unwrap();
        assert_eq!(last.open, 10_200.0);
        assert_eq!(last.high, 10_700.0);
        assert_eq!(last.low, 10_150.0);
        assert_eq!(last.close, 10_420.0);

        // Streaming update_open_interest expands the forming bar wick and opens new bars from prior close.
        assert_eq!(
            chart.update_open_interest(
                11,
                3,
                OpenInterestBar {
                    timestamp_unix_nanos: 4_000_000_000,
                    open: None,
                    high: Some(10_850.0),
                    low: None,
                    close: 10_600.0,
                }
            ),
            Ok(true)
        );
        let last = chart.series_data(oi_id).pop().unwrap();
        assert_eq!(last.high, 10_850.0);
        assert_eq!(last.low, 10_150.0);
        assert_eq!(last.close, 10_600.0);

        assert_eq!(
            chart.update_open_interest(
                11,
                4,
                OpenInterestBar {
                    timestamp_unix_nanos: 5_000_000_000,
                    open: None,
                    high: None,
                    low: None,
                    close: 10_300.0,
                }
            ),
            Ok(true)
        );
        let newest = chart.series_data(oi_id).pop().unwrap();
        assert_eq!(newest.open, 10_600.0);
        assert_eq!(newest.close, 10_300.0);

        // Financial legend exposes localized title + OHLC values tinted by candle direction.
        let legend = chart.financial_legend(crate::FinancialLegendRequest {
            logical_index: None,
            primary_title: "BTC",
            show_primary_ohlc: true,
            primary_values_series: None,
            leading_series: &[],
            trailing_series: &[],
        });
        let row = legend
            .iter()
            .find(|row| row.identity == crate::FinancialLegendIdentity::ExternalStudy(11))
            .expect("open interest legend row");
        assert_eq!(
            row.title,
            crate::text::legend::OPEN_INTEREST.text(crate::ChartLanguage::English)
        );
        assert_eq!(row.tone, crate::FinancialLegendTone::Bearish);
        assert_eq!(row.values.len(), 4);
        assert!(row.values[0].text.starts_with("O "));
        assert!(row.values[1].text.starts_with("H "));
        assert!(row.values[2].text.starts_with("L "));
        assert!(row.values[3].text.starts_with("C "));
        let expected_down = chart.market_colors().down.to_css();
        assert!(
            row.values
                .iter()
                .all(|v| v.color.as_deref() == Some(expected_down.as_str()))
        );

        // Language switch re-titles the engine-owned Open Interest series automatically.
        chart.set_language(crate::ChartLanguage::Japanese);
        assert_eq!(
            chart.series_entry(oi_id).unwrap().title,
            crate::text::legend::OPEN_INTEREST.text(crate::ChartLanguage::Japanese)
        );
        chart.set_language(crate::ChartLanguage::English);

        // Switching styles (Bars, Line, Area, Candles) and custom colors reconfigures in place.
        assert!(row.settings_available);
        for (style, expected_kind) in [
            (OpenInterestStyle::Bars, SeriesKind::Bar),
            (OpenInterestStyle::Line, SeriesKind::Line),
            (OpenInterestStyle::Area, SeriesKind::Area),
            (OpenInterestStyle::Candles, SeriesKind::Candlestick),
        ] {
            let options = OpenInterestOptions {
                style,
                line_color: Some(Color::rgb(0x29, 0x62, 0xFF)),
                up_color: Some(Color::rgb(0x00, 0xC8, 0x53)),
                down_color: Some(Color::rgb(0xFF, 0x17, 0x44)),
            };
            assert!(chart.set_open_interest_options(11, options));
            assert_eq!(chart.open_interest_series(11), Some(oi_id));
            assert_eq!(chart.series_entry(oi_id).unwrap().kind, expected_kind);
            assert_eq!(chart.open_interest_options(11), Some(options));
            let mode_legend = chart.financial_legend(crate::FinancialLegendRequest {
                logical_index: None,
                primary_title: "BTC",
                show_primary_ohlc: true,
                primary_values_series: None,
                leading_series: &[],
                trailing_series: &[],
            });
            let mode_row = mode_legend
                .iter()
                .find(|r| r.identity == crate::FinancialLegendIdentity::ExternalStudy(11))
                .expect("oi legend row");
            if matches!(style, OpenInterestStyle::Candles | OpenInterestStyle::Bars) {
                assert_eq!(mode_row.values.len(), 4);
            } else {
                assert_eq!(mode_row.values.len(), 1);
            }
        }

        // Stale generation, invalid values, and study-id hijacking are rejected cleanly.
        assert_eq!(chart.set_open_interest(11, 4, &bars), Ok(false));
        assert_eq!(
            chart.update_open_interest(
                11,
                5,
                OpenInterestBar {
                    timestamp_unix_nanos: 3_000_000_000,
                    open: None,
                    high: None,
                    low: None,
                    close: 10_000.0,
                }
            ),
            Err(ExternalStudyError::NonIncreasingTimestamp)
        );
        assert_eq!(
            chart.update_open_interest(
                11,
                5,
                OpenInterestBar {
                    timestamp_unix_nanos: 5_000_000_000,
                    open: None,
                    high: None,
                    low: None,
                    close: -5.0,
                }
            ),
            Err(ExternalStudyError::InvalidValue)
        );
        assert_eq!(
            chart.install_external_study_output(
                11,
                0,
                descriptor("Hijack", ExternalStudyPaneTarget::Price),
                9,
                &[1_000_000_000],
                &[Some(1.0)],
            ),
            Err(ExternalStudyError::InstallationRejected)
        );

        // Visibility and removal follow the external-study lifecycle and release the pane.
        assert!(chart.set_external_study_visible(11, false));
        assert_eq!(chart.series_visible(oi_id), Some(false));
        assert!(chart.remove_external_studies(&[11]));
        assert_eq!(chart.open_interest_series(11), None);
        assert_eq!(chart.panes.len(), 1);
    }

    #[test]
    fn liquidations_study_supports_styles_colors_updates_legend_and_lifecycle() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .set_series_data(
                0,
                &[2.0, 3.0, 4.0],
                &[100.0, 101.0, 102.0],
                &[102.0, 103.0, 104.0],
                &[99.0, 100.0, 101.0],
                &[101.0, 102.0, 103.0],
            )
            .unwrap();

        let bars = [
            // Older than the primary series: trimmed automatically.
            LiquidationBar {
                timestamp_unix_nanos: 1_000_000_000,
                long_volume: 50_000.0,
                short_volume: 10_000.0,
            },
            LiquidationBar {
                timestamp_unix_nanos: 2_000_000_000,
                long_volume: 250_000.0,
                short_volume: 0.0,
            },
            LiquidationBar {
                timestamp_unix_nanos: 3_000_000_000,
                long_volume: 100_000.0,
                short_volume: 400_000.0,
            },
            LiquidationBar {
                timestamp_unix_nanos: 4_000_000_000,
                long_volume: 600_000.0,
                short_volume: 150_000.0,
            },
        ];

        assert_eq!(chart.set_liquidations(12, 1, &bars), Ok(true));
        let [s0, s1] = chart.liquidation_series(12).expect("liquidation series");
        assert_eq!(chart.panes.len(), 2);
        assert_eq!(chart.series_entry(s0).unwrap().kind, SeriesKind::Histogram);
        assert_eq!(chart.series_entry(s1).unwrap().kind, SeriesKind::Histogram);
        assert_eq!(
            chart.series_entry(s0).unwrap().pane_index,
            chart.series_entry(s1).unwrap().pane_index
        );
        assert_eq!(
            chart.series_entry(s0).unwrap().price_format.kind,
            PriceFormatKind::Volume
        );
        // Leading bar older than primary series was trimmed.
        assert_eq!(chart.series_data(s1).first().unwrap().time, 2);
        assert_eq!(chart.series_data(s1).first().unwrap().close, -250_000.0);

        // Live streaming bar update on newest timestamp.
        assert_eq!(
            chart.update_liquidations(
                12,
                2,
                LiquidationBar {
                    timestamp_unix_nanos: 4_000_000_000,
                    long_volume: 750_000.0,
                    short_volume: 200_000.0,
                }
            ),
            Ok(true)
        );
        assert_eq!(
            chart.liquidation_bar_values_at(12, None),
            Some((200_000.0, 750_000.0))
        );

        // Legend reports localized positive Short & Long values with settings enabled.
        let legend = chart.financial_legend(crate::FinancialLegendRequest {
            logical_index: None,
            primary_title: "BTC",
            show_primary_ohlc: true,
            primary_values_series: None,
            leading_series: &[],
            trailing_series: &[],
        });
        let row = legend
            .iter()
            .find(|r| r.identity == crate::FinancialLegendIdentity::ExternalStudy(12))
            .expect("liquidations legend row");
        assert_eq!(
            row.title,
            crate::text::legend::LIQUIDATIONS.text(crate::ChartLanguage::English)
        );
        assert!(row.settings_available);
        assert_eq!(row.values.len(), 2);
        assert!(row.values[0].text.starts_with("Short "));
        assert!(row.values[1].text.starts_with("Long "));
        assert!(!row.values[1].text.contains('-'));

        // Language switch re-titles the study outputs.
        chart.set_language(crate::ChartLanguage::German);
        assert_eq!(
            chart.series_entry(s0).unwrap().title,
            crate::text::legend::SHORTS.text(crate::ChartLanguage::German)
        );
        chart.set_language(crate::ChartLanguage::English);

        // Switching across Split, Stacked, and Net preserves exact decoded volumes in place.
        let custom_long = Color::rgb(0xFF, 0x33, 0x44);
        let custom_short = Color::rgb(0x00, 0xCC, 0x66);
        for style in [
            LiquidationStyle::Stacked,
            LiquidationStyle::Net,
            LiquidationStyle::Split,
        ] {
            let options = LiquidationOptions {
                style,
                long_color: Some(custom_long),
                short_color: Some(custom_short),
            };
            assert!(chart.set_liquidation_options(12, options));
            assert_eq!(chart.liquidation_options(12), Some(options));
            assert_eq!(
                chart.liquidation_bar_values_at(12, None),
                Some((200_000.0, 750_000.0))
            );
            assert_eq!(
                chart.series_visible(s1),
                Some(style != LiquidationStyle::Net)
            );
            let mode_legend = chart.financial_legend(crate::FinancialLegendRequest {
                logical_index: None,
                primary_title: "BTC",
                show_primary_ohlc: true,
                primary_values_series: None,
                leading_series: &[],
                trailing_series: &[],
            });
            let mode_row = mode_legend
                .iter()
                .find(|r| r.identity == crate::FinancialLegendIdentity::ExternalStudy(12))
                .expect("liquidations legend row");
            if style == LiquidationStyle::Net {
                assert_eq!(mode_row.values.len(), 1);
                assert!(mode_row.values[0].text.starts_with("Net "));
                assert_eq!(mode_row.values[0].color, Some(custom_long.to_css()));
            } else {
                assert_eq!(mode_row.values.len(), 2);
                assert_eq!(mode_row.values[0].color, Some(custom_short.to_css()));
                assert_eq!(mode_row.values[1].color, Some(custom_long.to_css()));
            }
        }

        // Visibility toggle in Net mode keeps the hidden helper series hidden.
        assert!(chart.set_liquidation_options(
            12,
            LiquidationOptions {
                style: LiquidationStyle::Net,
                long_color: None,
                short_color: None,
            }
        ));
        assert!(chart.set_external_study_visible(12, false));
        assert_eq!(chart.series_visible(s0), Some(false));
        assert_eq!(chart.series_visible(s1), Some(false));
        assert!(chart.set_external_study_visible(12, true));
        assert_eq!(chart.series_visible(s0), Some(true));
        assert_eq!(chart.series_visible(s1), Some(false));

        // Stale generation, negative volumes, and hijacking are rejected.
        assert_eq!(chart.set_liquidations(12, 2, &bars), Ok(false));
        assert_eq!(
            chart.update_liquidations(
                12,
                3,
                LiquidationBar {
                    timestamp_unix_nanos: 4_000_000_000,
                    long_volume: -1.0,
                    short_volume: 0.0,
                }
            ),
            Err(ExternalStudyError::InvalidValue)
        );
        assert_eq!(
            chart.install_external_study_output(
                12,
                0,
                descriptor("Hijack", ExternalStudyPaneTarget::Price),
                9,
                &[2_000_000_000],
                &[Some(1.0)],
            ),
            Err(ExternalStudyError::InstallationRejected)
        );

        assert!(chart.remove_external_studies(&[12]));
        assert_eq!(chart.liquidation_series(12), None);
        assert_eq!(chart.panes.len(), 1);
    }
}
