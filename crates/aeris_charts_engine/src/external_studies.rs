//! Engine-owned projection of host-computed scalar studies.
//!
//! A host owns provider subscriptions and computes values. The chart engine owns validation,
//! generation fencing, series/pane lifecycle, presentation, and group operations.

use crate::indicators::{indicator_output_style, output_style_is_valid};
use crate::{
    ChartEngine, IndicatorChromeOptions, IndicatorOutputStyle, MarketTone, PriceFormatKind,
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

/// Legend title of the engine-owned options-volume study.
pub const OPTIONS_VOLUME_TITLE: &str = "Options Volume";
const OPTIONS_VOLUME_OUTPUTS: [(&str, MarketTone); 2] =
    [("Calls", MarketTone::Up), ("Puts", MarketTone::Down)];
const OPTIONS_VOLUME_PRECISION: u32 = 1;

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
    legend_label: Option<String>,
    input_requirements: ExternalStudyInputRequirements,
    /// The output's own visibility from its style, kept apart from the whole-study toggle so
    /// showing the study again never brings back a line the host hid. The series paints only
    /// when both are set.
    output_visible: bool,
    study_visible: bool,
    momentum_histogram: bool,
    options_volume: bool,
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
        if self.is_options_volume_study(study_id) {
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
        for (output_index, ((title, _), values)) in OPTIONS_VOLUME_OUTPUTS
            .iter()
            .zip([&calls, &puts])
            .enumerate()
        {
            if let Err(error) = self.install_output(
                study_id,
                output_index,
                options_volume_descriptor(title),
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
        for (&(candidate, _), state) in &mut self.external_study_outputs {
            if candidate == study_id {
                changed |= state.study_visible != visible;
                state.study_visible = visible;
                painted.push((state.series_id, visible && state.output_visible));
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
            .map(|(&(_, output_index), state)| (output_index, state.series_id))
            .collect::<Vec<_>>();
        for (output_index, series_id) in outputs {
            if let Some(series) = self.series_entry_mut(series_id) {
                series.line_color = series
                    .market_tone
                    .is_none()
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

fn options_volume_descriptor(title: &str) -> ExternalStudyOutputDescriptor<'_> {
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
        assert_eq!(row.title, OPTIONS_VOLUME_TITLE);
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
}
