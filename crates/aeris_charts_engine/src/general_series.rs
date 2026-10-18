use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::Arc;

use aeris_charts_core::scale::general_scale::{
    AngularCategoryScale, BandScale, LinearScale, PointScale, PolarPlot, PolarPolygon, PolarSector,
};
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType};
use aeris_charts_render::line::{LinePoint, expand_band_into, expand_line_into};

use crate::general_axes::NumericAxisScale;
use crate::general_histogram::{
    GeneralHistogramBins, compute_histogram_bins, validate_histogram_options,
    validate_histogram_samples,
};
use crate::{
    AxisDimension, ChartEngine, ChartError, ErrorCode, GeneralAxisDomain, GeneralDatasetId,
    GeneralRowIdentity, GeneralScaleType, GeneralXKind, GeneralXyInput, HorizontalDomain, PaneId,
};

pub const MAX_GENERAL_SERIES: usize = 1_024;
pub const MAX_GENERAL_PIE_SLICES: usize = 4_096;
pub const MAX_GENERAL_RADAR_VALUES: usize = 4_096;
pub const MAX_GENERAL_REFERENCES: usize = 1_024;
pub const MAX_GENERAL_SERIES_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_SERIES_COLOR_BYTES: usize = 256;
pub const MAX_GENERAL_SERIES_GROUP_ID_BYTES: usize = 128;
pub const MAX_GENERAL_SERIES_STACK_ID_BYTES: usize = 128;
pub const MAX_GENERAL_ACCESSIBILITY_ITEMS: usize = 512;
pub const MAX_GENERAL_BRUSH_ITEMS: usize = 4_096;
pub const MAX_GENERAL_SHARED_TOOLTIP_ITEMS: usize = 512;
pub const MAX_GENERAL_LEGEND_SLICES_PER_SERIES: usize = 32;
pub const MAX_GENERAL_LEGEND_SLICES_PER_SNAPSHOT: usize = 512;
pub const MIN_GENERAL_POINT_RADIUS: f64 = 1.0;
pub const MAX_GENERAL_POINT_RADIUS: f64 = 64.0;
pub const DEFAULT_GENERAL_FILL_OPACITY: f64 = 72.0 / 255.0;
pub const MAX_GENERAL_HISTOGRAM_BINS: usize = 1_000;
const SCATTER_GRID_BASE_CELL_CSS: f64 = 32.0;
const MAX_SCATTER_GRID_CELLS: usize = 65_536;
const MAX_POLAR_CONTEXT_CACHES: usize = 64;
const PIE_COLORS: [Color; 8] = [
    Color::rgb(77, 141, 210),
    Color::rgb(72, 174, 145),
    Color::rgb(227, 161, 75),
    Color::rgb(206, 101, 114),
    Color::rgb(143, 117, 191),
    Color::rgb(83, 168, 189),
    Color::rgb(146, 170, 87),
    Color::rgb(181, 138, 97),
];

pub(crate) fn general_polar_fill_color(
    series: &GeneralSeries,
    dataset: &crate::GeneralDataset,
    row: usize,
) -> Color {
    let base = dataset
        .row_color(row)
        .or_else(|| series.color().and_then(Color::parse_css))
        .unwrap_or(PIE_COLORS[row % PIE_COLORS.len()]);
    Color::rgba(
        base.r(),
        base.g(),
        base.b(),
        ((f64::from(base.a()) / 255.0 * series.fill_opacity()).clamp(0.0, 1.0) * 255.0).round()
            as u8,
    )
}

/// One proposed series and its packed columns for a non-mutating pane validation.
#[derive(Clone, Debug)]
pub struct GeneralSeriesProposal {
    pub kind: GeneralSeriesKind,
    pub x_axis_id: String,
    pub y_axis_id: String,
    pub data: GeneralXyInput,
}

/// Complete proposed general-pane graph. Validation never installs any of these objects.
#[derive(Clone, Debug)]
pub struct GeneralPaneProposal {
    pub pane: usize,
    pub axes: Vec<crate::GeneralAxisOptions>,
    pub series: Vec<GeneralSeriesProposal>,
    pub references: Vec<GeneralReferenceOptions>,
}

/// One keyed series in a complete pane composition.
#[derive(Clone, Debug)]
pub struct GeneralPaneSeriesComposition {
    pub key: String,
    pub current: Option<GeneralSeriesId>,
    pub options: GeneralSeriesOptions,
    pub data: GeneralXyInput,
    /// Optional per-row label, CSS color, and point-symbol columns, one entry per data row.
    pub labels: Option<Vec<Option<String>>>,
    pub colors: Option<Vec<Option<String>>>,
    pub symbols: Option<Vec<Option<String>>>,
}

/// One keyed reference in a complete pane composition.
#[derive(Clone, Debug)]
pub struct GeneralPaneReferenceComposition {
    pub key: String,
    pub current: Option<GeneralReferenceId>,
    pub options: GeneralReferenceOptions,
}

/// Complete desired general graph for one pane.
#[derive(Clone, Debug)]
pub struct GeneralPaneComposition {
    pub pane: usize,
    pub axes: Vec<crate::GeneralAxisOptions>,
    pub series: Vec<GeneralPaneSeriesComposition>,
    pub references: Vec<GeneralPaneReferenceComposition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneralPaneComposedSeries {
    pub key: String,
    pub id: GeneralSeriesId,
    pub dataset: GeneralDatasetId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneralPaneComposedReference {
    pub key: String,
    pub id: GeneralReferenceId,
}

/// Stable handles resulting from a successful pane composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneralPaneCompositionResult {
    pub series: Vec<GeneralPaneComposedSeries>,
    pub references: Vec<GeneralPaneComposedReference>,
    /// The brush was cleared because its axis was removed or structurally replaced.
    pub brush_cleared: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralSeriesKind {
    XyLine,
    XyArea,
    RangeArea,
    RangeBar,
    ErrorBar,
    Column,
    Histogram,
    HorizontalBar,
    BoxPlot,
    HeatmapGrid,
    Scatter,
    Bubble,
    Pie,
    Radar,
    RadialBar,
    PolarArea,
}

/// Shared polar sector sweep and radial bounds. A positive inner radius makes a pie a donut.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GeneralPolarOptions {
    pub start_angle: f64,
    pub end_angle: f64,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub pad_angle: f64,
}

impl Default for GeneralPolarOptions {
    fn default() -> Self {
        Self {
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
            inner_radius: 0.0,
            outer_radius: 1.0,
            pad_angle: 0.0,
        }
    }
}

/// Height measure of each histogram bin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralHistogramNormalization {
    /// Number of samples in the bin.
    #[default]
    Count,
    /// Share of all valid samples, in percent.
    Percent,
    /// Share of all valid samples divided by the bin width, so the bar areas sum to one.
    Density,
}

/// Engine-side binning policy of a histogram series.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GeneralHistogramOptions {
    /// Exact number of equal-width bins spanning the sample range. `None` uses Sturges' rule
    /// with a nice 1/2/5 bin width aligned to multiples of that width.
    pub bin_count: Option<usize>,
    /// Bin width aligned to multiples of itself. Overrides `bin_count` when both are set.
    pub bin_width: Option<f64>,
    pub normalization: GeneralHistogramNormalization,
    /// Accumulate bins from the left. Count and percent heights become running totals, and
    /// density becomes the empirical cumulative distribution ending at one.
    pub cumulative: bool,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum GeneralStackMode {
    #[default]
    Normal,
    /// Add every value to the preceding boundary, including negative values.
    Cumulative,
    /// Center each signed stack's final total around zero.
    Silhouette,
    /// Shift a cumulative stack baseline to reduce weighted vertical movement.
    Wiggle,
    Percent,
    /// Stack positive values from zero and suppress negative marks.
    Positive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralAreaBaseline {
    #[default]
    Zero,
    DomainMin,
    DomainMax,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralLineStyle {
    #[default]
    Solid,
    Dotted,
    Dashed,
}

impl GeneralLineStyle {
    pub(crate) fn render_style(self) -> LineStyle {
        match self {
            Self::Solid => LineStyle::Solid,
            Self::Dotted => LineStyle::Dotted,
            Self::Dashed => LineStyle::Dashed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralInterpolation {
    #[default]
    Linear,
    /// Legacy horizontal-then-vertical step, equivalent to `StepAfter`.
    Step,
    StepBefore,
    StepMiddle,
    StepAfter,
    Curved,
    Monotone,
    Natural,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralPointSymbol {
    #[default]
    Circle,
    Square,
    Diamond,
    Triangle,
}

impl GeneralInterpolation {
    pub(crate) fn render_type(self) -> LineType {
        match self {
            Self::Linear => LineType::Simple,
            Self::Step | Self::StepAfter => LineType::WithSteps,
            Self::StepBefore => LineType::StepBefore,
            Self::StepMiddle => LineType::StepMiddle,
            Self::Curved => LineType::Curved,
            Self::Monotone => LineType::Monotone,
            Self::Natural => LineType::Natural,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeneralSeriesId(NonZeroU32);

impl GeneralSeriesId {
    pub fn get(self) -> u32 {
        self.0.get()
    }

    #[doc(hidden)]
    pub fn from_raw(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSeriesOptions {
    pub kind: GeneralSeriesKind,
    pub pane: usize,
    pub dataset: GeneralDatasetId,
    pub x_axis_id: String,
    pub y_axis_id: String,
    pub visible: bool,
    pub title: String,
    pub color: Option<String>,
    pub point_radius: f64,
    pub point_markers: bool,
    pub point_symbol: GeneralPointSymbol,
    pub line_width: f64,
    pub line_style: GeneralLineStyle,
    pub interpolation: GeneralInterpolation,
    pub connect_missing: bool,
    pub fill_opacity: f64,
    pub polar: GeneralPolarOptions,
    /// Explicit top and bottom CSS colors for an unstacked area fill.
    pub fill_gradient: Option<[String; 2]>,
    pub baseline_value: Option<f64>,
    pub baseline_policy: GeneralAreaBaseline,
    pub data_labels: bool,
    /// Total empty space within a bar slot, split equally between its sides, in CSS pixels.
    pub bar_gap: f64,
    /// Maximum bar thickness in CSS pixels; `None` uses the available slot.
    pub bar_max_width: Option<f64>,
    /// Radius of exposed bar corners in CSS pixels.
    pub bar_corner_radius: f64,
    /// Fixed color normalization domain for heatmap cells; omitted uses visible values.
    pub heatmap_value_domain: Option<[f64; 2]>,
    pub heatmap_low_color: Option<String>,
    pub heatmap_high_color: Option<String>,
    pub box_fill_color: Option<String>,
    pub box_median_color: Option<String>,
    pub group_id: Option<String>,
    pub stack_id: Option<String>,
    pub stack_mode: GeneralStackMode,
    /// Binning policy; non-default values require a histogram series.
    pub histogram: GeneralHistogramOptions,
}

impl GeneralSeriesOptions {
    pub fn xy_line(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::XyLine,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn xy_area(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::XyArea,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn range_area(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::RangeArea,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn range_bar(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        let mut options = Self::range_area(pane, dataset, x_axis_id, y_axis_id);
        options.kind = GeneralSeriesKind::RangeBar;
        options
    }

    pub fn error_bar(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::ErrorBar,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 4.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn column(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::Column,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn horizontal_bar(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::HorizontalBar,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn box_plot(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::BoxPlot,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    /// Histogram of one-dimensional samples (`GeneralXyInput::Values`) binned by the engine onto a
    /// linear X axis, with bin heights on a linear Y axis.
    pub fn histogram(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        let mut options = Self::column(pane, dataset, x_axis_id, y_axis_id);
        options.kind = GeneralSeriesKind::Histogram;
        options
    }

    pub fn heatmap_grid(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::HeatmapGrid,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn scatter(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::Scatter,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn bubble(
        pane: usize,
        dataset: GeneralDatasetId,
        x_axis_id: impl Into<String>,
        y_axis_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: GeneralSeriesKind::Bubble,
            pane,
            dataset,
            x_axis_id: x_axis_id.into(),
            y_axis_id: y_axis_id.into(),
            visible: true,
            title: String::new(),
            color: None,
            point_radius: 3.0,
            point_markers: false,
            point_symbol: GeneralPointSymbol::Circle,
            line_width: 2.0,
            line_style: GeneralLineStyle::Solid,
            interpolation: GeneralInterpolation::Linear,
            connect_missing: false,
            fill_opacity: DEFAULT_GENERAL_FILL_OPACITY,
            polar: GeneralPolarOptions::default(),
            fill_gradient: None,
            baseline_value: None,
            baseline_policy: GeneralAreaBaseline::Zero,
            data_labels: false,
            bar_gap: 0.0,
            bar_max_width: None,
            bar_corner_radius: 0.0,
            heatmap_value_domain: None,
            heatmap_low_color: None,
            heatmap_high_color: None,
            box_fill_color: None,
            box_median_color: None,
            group_id: None,
            stack_id: None,
            stack_mode: GeneralStackMode::Normal,
            histogram: GeneralHistogramOptions::default(),
        }
    }

    pub fn pie(
        pane: usize,
        dataset: GeneralDatasetId,
        angle_axis_id: impl Into<String>,
        radius_axis_id: impl Into<String>,
    ) -> Self {
        let mut options = Self::xy_area(pane, dataset, angle_axis_id, radius_axis_id);
        options.kind = GeneralSeriesKind::Pie;
        options.fill_opacity = 1.0;
        options
    }

    pub fn radar(
        pane: usize,
        dataset: GeneralDatasetId,
        angle_axis_id: impl Into<String>,
        radius_axis_id: impl Into<String>,
    ) -> Self {
        let mut options = Self::xy_area(pane, dataset, angle_axis_id, radius_axis_id);
        options.kind = GeneralSeriesKind::Radar;
        options
    }

    pub fn polar_area(
        pane: usize,
        dataset: GeneralDatasetId,
        angle_axis_id: impl Into<String>,
        radius_axis_id: impl Into<String>,
    ) -> Self {
        let mut options = Self::xy_area(pane, dataset, angle_axis_id, radius_axis_id);
        options.kind = GeneralSeriesKind::PolarArea;
        options.fill_opacity = 1.0;
        options
    }

    pub fn radial_bar(
        pane: usize,
        dataset: GeneralDatasetId,
        angle_axis_id: impl Into<String>,
        radius_axis_id: impl Into<String>,
    ) -> Self {
        let mut options = Self::polar_area(pane, dataset, angle_axis_id, radius_axis_id);
        options.kind = GeneralSeriesKind::RadialBar;
        options
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSeries {
    id: GeneralSeriesId,
    kind: GeneralSeriesKind,
    pane_id: PaneId,
    dataset: GeneralDatasetId,
    x_axis_id: String,
    y_axis_id: String,
    visible: bool,
    title: String,
    color: Option<String>,
    point_radius: f64,
    point_markers: bool,
    point_symbol: GeneralPointSymbol,
    line_width: f64,
    line_style: GeneralLineStyle,
    interpolation: GeneralInterpolation,
    connect_missing: bool,
    fill_opacity: f64,
    polar: GeneralPolarOptions,
    fill_gradient: Option<[String; 2]>,
    baseline_value: Option<f64>,
    baseline_policy: GeneralAreaBaseline,
    data_labels: bool,
    bar_gap: f64,
    bar_max_width: Option<f64>,
    bar_corner_radius: f64,
    heatmap_value_domain: Option<[f64; 2]>,
    heatmap_low_color: Option<String>,
    heatmap_high_color: Option<String>,
    box_fill_color: Option<String>,
    box_median_color: Option<String>,
    group_id: Option<String>,
    stack_id: Option<String>,
    stack_mode: GeneralStackMode,
    histogram: GeneralHistogramOptions,
    composition_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralColumnGeometry {
    pub(crate) row: usize,
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
    /// Whether the value end is the minimum coordinate on its value axis.
    pub(crate) end_at_min: bool,
    /// Whether this bar has an exposed value end (or is unstacked).
    pub(crate) exposed_end: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralScatterGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) radius: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GeneralPolarSectorGeometry {
    pub(crate) row: usize,
    pub(crate) sector: PolarSector,
}

#[derive(Clone)]
struct GeneralPolarCategoryContext {
    scene_generation: u64,
    plot: PolarPlot,
    angles: AngularCategoryScale,
    sector_bounds: Option<Vec<(f64, f64)>>,
    radar_slot_morph: bool,
    radar_shape_angles: Option<Arc<[(f64, f64)]>>,
    radar_marker_rows: Option<Vec<Option<usize>>>,
    radius_scale: NumericAxisScale,
    rows: Vec<Option<usize>>,
    radar_radii: Option<Vec<f64>>,
}

#[derive(Clone, Copy)]
struct GeneralPieSlice {
    row: usize,
    from: f64,
    to: f64,
}

#[derive(Clone, Copy)]
pub(crate) struct PieOrderRow {
    from: [f64; 2],
    to: [f64; 2],
}

pub(crate) struct RadarSlotMotion {
    pub(crate) series_id: GeneralSeriesId,
    source_radii: Vec<f64>,
    target_values: Vec<Option<f64>>,
    target_rows: Vec<Option<usize>>,
}

pub(crate) struct RadarShapeMotion {
    pub(crate) series_id: GeneralSeriesId,
    angles: Arc<[(f64, f64)]>,
    source_radii: Vec<f64>,
    source_rows: Vec<Option<usize>>,
    source_marker_rows: Vec<Option<usize>>,
    target_values: Vec<Option<f64>>,
    target_rows: Vec<Option<usize>>,
    target_marker_rows: Vec<Option<usize>>,
    target_angles: AngularCategoryScale,
}

impl RadarShapeMotion {
    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.angles.len() * std::mem::size_of::<(f64, f64)>()
            + self.source_radii.capacity() * std::mem::size_of::<f64>()
            + self.source_rows.capacity() * std::mem::size_of::<Option<usize>>()
            + self.source_marker_rows.capacity() * std::mem::size_of::<Option<usize>>()
            + self.target_values.capacity() * std::mem::size_of::<Option<f64>>()
            + self.target_rows.capacity() * std::mem::size_of::<Option<usize>>()
            + self.target_marker_rows.capacity() * std::mem::size_of::<Option<usize>>()
    }
}

impl RadarSlotMotion {
    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.source_radii.capacity() * std::mem::size_of::<f64>()
            + self.target_values.capacity() * std::mem::size_of::<Option<f64>>()
            + self.target_rows.capacity() * std::mem::size_of::<Option<usize>>()
    }
}

impl PieOrderRow {
    fn bounds(self, progress: f64) -> [f64; 2] {
        [
            self.from[0] * (1.0 - progress) + self.to[0] * progress,
            self.from[1] * (1.0 - progress) + self.to[1] * progress,
        ]
    }
}

fn pie_normalized_bounds(dataset: &crate::general_data::GeneralDataset) -> Vec<Option<[f64; 2]>> {
    let mut bounds = vec![None; dataset.len()];
    let maximum = dataset
        .y()
        .iter()
        .enumerate()
        .filter(|(row, value)| dataset.y_is_valid(*row) && **value > 0.0)
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::max);
    if maximum == 0.0 {
        return bounds;
    }
    let total: f64 = dataset
        .y()
        .iter()
        .enumerate()
        .filter(|(row, value)| dataset.y_is_valid(*row) && **value > 0.0)
        .map(|(_, value)| *value / maximum)
        .sum();
    let mut accumulated = 0.0;
    let mut last = None;
    for (row, value) in dataset.y().iter().enumerate() {
        if !dataset.y_is_valid(row) || *value <= 0.0 {
            continue;
        }
        let from = accumulated;
        accumulated += (*value / maximum) / total;
        bounds[row] = Some([from, accumulated.min(1.0)]);
        last = Some(row);
    }
    if let Some(last) = last {
        bounds[last].as_mut().unwrap()[1] = 1.0;
    }
    bounds
}

fn radar_rows_by_category(dataset: &crate::general_data::GeneralDataset) -> HashMap<&str, usize> {
    let mut rows = HashMap::new();
    if let (Some(categories), Some(indices)) = (dataset.categories(), dataset.category_indices()) {
        for (row, index) in indices.iter().enumerate() {
            if let Some(category) = categories.get(*index as usize) {
                rows.insert(category.as_str(), row);
            }
        }
    }
    rows
}

fn radar_angular_scale(count: usize, reverse: bool) -> Option<AngularCategoryScale> {
    if count == 0 {
        return None;
    }
    let step = std::f64::consts::TAU / count as f64;
    let start = if reverse { step / 2.0 } else { -step / 2.0 };
    AngularCategoryScale::new(count, start, !reverse).ok()
}

fn same_polar_angle(a: f64, b: f64) -> bool {
    let delta = (a - b).rem_euclid(std::f64::consts::TAU);
    delta.min(std::f64::consts::TAU - delta) < 1e-12
}

impl GeneralPieSlice {
    fn geometry(
        self,
        plot: PolarPlot,
        options: GeneralPolarOptions,
    ) -> Option<GeneralPolarSectorGeometry> {
        let pad = options.pad_angle.min(self.to - self.from);
        let sector = PolarSector::new(
            plot,
            self.from + pad / 2.0,
            self.to - self.from - pad,
            options.inner_radius,
            options.outer_radius,
        )
        .ok()?;
        Some(GeneralPolarSectorGeometry {
            row: self.row,
            sector,
        })
    }
}

#[derive(Clone)]
struct GeneralPieContext {
    scene_generation: u64,
    plot: PolarPlot,
    start: f64,
    end: f64,
    slices: Vec<GeneralPieSlice>,
}

fn polar_value_sector(
    context: &GeneralPolarCategoryContext,
    dataset: &crate::general_data::GeneralDataset,
    series: &GeneralSeries,
    index: usize,
    sweep_fraction: f64,
) -> Option<GeneralPolarSectorGeometry> {
    let row = context.rows.get(index).copied().flatten()?;
    if !dataset.y_is_valid(row) || dataset.y()[row] <= 0.0 {
        return None;
    }
    let options = series.polar;
    let fraction = context
        .radius_scale
        .coordinate(dataset.y()[row])?
        .clamp(0.0, 1.0);
    let outer = if series.kind == GeneralSeriesKind::PolarArea {
        (options.inner_radius.powi(2)
            + (options.outer_radius.powi(2) - options.inner_radius.powi(2)) * fraction)
            .sqrt()
    } else {
        options.inner_radius + (options.outer_radius - options.inner_radius) * fraction
    };
    if outer <= options.inner_radius {
        return None;
    }
    let (from, to) = context
        .sector_bounds
        .as_ref()
        .and_then(|bounds| bounds.get(index).copied())
        .or_else(|| context.angles.sector(index))?;
    let signed_sweep = (to - from) * sweep_fraction;
    let from = options.start_angle + from * sweep_fraction;
    let pad = options.pad_angle.min(signed_sweep.abs());
    let signed_pad = pad.copysign(signed_sweep);
    let sector = PolarSector::new(
        context.plot,
        from + signed_pad / 2.0,
        signed_sweep - signed_pad,
        options.inner_radius,
        outer,
    )
    .ok()?;
    Some(GeneralPolarSectorGeometry { row, sector })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralLinePointGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) starts_new_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralRangePointGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) low_y: f64,
    pub(crate) high_y: f64,
    pub(crate) starts_new_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum GeneralStackXKey {
    Numeric(u64),
    Temporal(i64),
    Category(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralErrorBarGeometry {
    pub(crate) row: usize,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) x_low: Option<f64>,
    pub(crate) x_high: Option<f64>,
    pub(crate) y_low: Option<f64>,
    pub(crate) y_high: Option<f64>,
    pub(crate) cap_half_size: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralBoxPlotGeometry {
    pub(crate) row: usize,
    pub(crate) center_x: f64,
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) min_y: f64,
    pub(crate) q1_y: f64,
    pub(crate) median_y: f64,
    pub(crate) q3_y: f64,
    pub(crate) max_y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralHeatmapGeometry {
    pub(crate) row: usize,
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
    pub(crate) value: f64,
    pub(crate) intensity: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScatterGeometryKey {
    dataset_generation: u64,
    plot_width: u64,
    plot_y: u64,
    plot_height: u64,
    x_domain: [u64; 2],
    y_domain: [u64; 2],
    x_scale: GeneralScaleType,
    y_scale: GeneralScaleType,
    x_reverse: bool,
    y_reverse: bool,
    radius: u64,
}

#[derive(Clone, Copy)]
struct ScatterGeometryContext {
    key: ScatterGeometryKey,
    x_scale: NumericAxisScale,
    y_scale: NumericAxisScale,
    plot_width: f64,
    plot_y: f64,
    plot_bottom: f64,
}

/// Memoized O(rows) contribution of one series to one automatic numeric axis domain. It is valid
/// while the series keeps the same dataset generation, kind, and stacking; hit tests and frame
/// construction resolve auto domains repeatedly, so rescanning every row per call is not bounded.
#[derive(Clone, Copy)]
struct AxisScanEntry {
    dimension: AxisDimension,
    scale: GeneralScaleType,
    dataset: GeneralDatasetId,
    generation: u64,
    kind: GeneralSeriesKind,
    stacked: bool,
    value: (Option<(f64, f64)>, bool),
}

/// Bins of one histogram series, valid while its dataset generation and binning policy match.
#[derive(Clone)]
struct GeneralHistogramCacheEntry {
    dataset: GeneralDatasetId,
    generation: u64,
    options: GeneralHistogramOptions,
    bins: Arc<GeneralHistogramBins>,
}

/// A series binds one X and one Y axis; a small bound still covers scale-type changes.
const MAX_AXIS_SCANS_PER_SERIES: usize = 4;

#[derive(Clone)]
struct ScatterSpatialIndex {
    key: ScatterGeometryKey,
    origin_y: f64,
    cell_size: f64,
    columns: usize,
    rows: usize,
    offsets: Vec<usize>,
    point_indices: Vec<u32>,
    points: Vec<GeneralScatterGeometry>,
}

impl ScatterSpatialIndex {
    fn new(
        key: ScatterGeometryKey,
        origin_y: f64,
        plot_width: f64,
        plot_height: f64,
        points: Vec<GeneralScatterGeometry>,
    ) -> Self {
        let mut cell_size = SCATTER_GRID_BASE_CELL_CSS;
        let (mut columns, mut rows) = grid_dimensions(plot_width, plot_height, cell_size);
        while columns.saturating_mul(rows) > MAX_SCATTER_GRID_CELLS {
            cell_size *= 2.0;
            (columns, rows) = grid_dimensions(plot_width, plot_height, cell_size);
        }
        let cell_count = columns.saturating_mul(rows).max(1);
        let mut counts = vec![0usize; cell_count];
        for point in &points {
            let cell = scatter_cell(point.x, point.y, origin_y, cell_size, columns, rows);
            counts[cell] += 1;
        }
        let mut offsets = vec![0usize; cell_count + 1];
        for (index, count) in counts.into_iter().enumerate() {
            offsets[index + 1] = offsets[index] + count;
        }
        let mut cursors = offsets[..cell_count].to_vec();
        let mut point_indices = vec![0u32; points.len()];
        for (point_index, point) in points.iter().enumerate() {
            let cell = scatter_cell(point.x, point.y, origin_y, cell_size, columns, rows);
            let slot = cursors[cell];
            point_indices[slot] = u32::try_from(point_index)
                .expect("general scatter rows stay below the u32 index ceiling");
            cursors[cell] += 1;
        }
        Self {
            key,
            origin_y,
            cell_size,
            columns,
            rows,
            offsets,
            point_indices,
            points,
        }
    }

    fn estimated_bytes(&self) -> usize {
        self.offsets.capacity() * std::mem::size_of::<usize>()
            + self.point_indices.capacity() * std::mem::size_of::<u32>()
            + self.points.capacity() * std::mem::size_of::<GeneralScatterGeometry>()
    }

    fn visit_candidates<F>(&self, x: f64, y: f64, expansion: f64, mut visit: F)
    where
        F: FnMut(GeneralScatterGeometry),
    {
        let Some((min_col, max_col)) = grid_query_range(
            x - expansion,
            x + expansion,
            0.0,
            self.cell_size,
            self.columns,
        ) else {
            return;
        };
        let Some((min_row, max_row)) = grid_query_range(
            y - expansion,
            y + expansion,
            self.origin_y,
            self.cell_size,
            self.rows,
        ) else {
            return;
        };
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let cell = row * self.columns + col;
                for &point_index in &self.point_indices[self.offsets[cell]..self.offsets[cell + 1]]
                {
                    if let Some(point) = self.points.get(point_index as usize) {
                        visit(*point);
                    }
                }
            }
        }
    }
}

fn grid_dimensions(width: f64, height: f64, cell_size: f64) -> (usize, usize) {
    let columns = (width.max(1.0) / cell_size).ceil().max(1.0) as usize;
    let rows = (height.max(1.0) / cell_size).ceil().max(1.0) as usize;
    (columns, rows)
}

fn scatter_cell(
    x: f64,
    y: f64,
    origin_y: f64,
    cell_size: f64,
    columns: usize,
    rows: usize,
) -> usize {
    let col = ((x / cell_size).floor() as isize).clamp(0, columns as isize - 1) as usize;
    let row = (((y - origin_y) / cell_size).floor() as isize).clamp(0, rows as isize - 1) as usize;
    row * columns + col
}

fn grid_query_range(
    from: f64,
    to: f64,
    range_start: f64,
    cell_size: f64,
    count: usize,
) -> Option<(usize, usize)> {
    let first = ((from - range_start) / cell_size).floor() as isize;
    let last = ((to - range_start) / cell_size).floor() as isize;
    if last < 0 || first >= count as isize {
        return None;
    }
    Some((
        first.clamp(0, count as isize - 1) as usize,
        last.clamp(0, count as isize - 1) as usize,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeneralHitMode {
    Exact,
    Nearest { max_distance: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSeriesHit {
    pub series: GeneralSeriesId,
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub distance: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct GeneralInteractionTarget {
    series: GeneralSeriesId,
    row: usize,
    row_id: GeneralRowIdentity,
    distance: f64,
}

impl From<&GeneralSeriesHit> for GeneralInteractionTarget {
    fn from(hit: &GeneralSeriesHit) -> Self {
        Self {
            series: hit.series,
            row: hit.row,
            row_id: hit.row_id.clone(),
            distance: hit.distance,
        }
    }
}

impl GeneralInteractionTarget {
    fn hit(&self) -> GeneralSeriesHit {
        GeneralSeriesHit {
            series: self.series,
            row: self.row,
            row_id: self.row_id.clone(),
            distance: self.distance,
        }
    }

    fn estimated_bytes(&self) -> usize {
        match &self.row_id {
            GeneralRowIdentity::Explicit(crate::GeneralRowId::Text(value)) => value.capacity(),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralTooltipSnapshot {
    pub series: GeneralSeriesId,
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub y_label: Option<String>,
    pub label: Option<String>,
    pub value: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub x_low: Option<f64>,
    pub x_high: Option<f64>,
    pub q1: Option<f64>,
    pub q3: Option<f64>,
    pub size: Option<f64>,
    pub title: String,
    /// Opaque identity color of the row's mark: the sector fill for polar series, else the row
    /// color, the series color or the default line color.
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSharedTooltipSnapshot {
    pub pane: usize,
    pub anchor_series: GeneralSeriesId,
    pub anchor_row: usize,
    pub items: Vec<GeneralTooltipSnapshot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAccessibilityItem {
    pub row: usize,
    pub row_id: GeneralRowIdentity,
    pub x_label: String,
    pub y_label: Option<String>,
    pub label: Option<String>,
    pub value: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub x_low: Option<f64>,
    pub x_high: Option<f64>,
    pub q1: Option<f64>,
    pub q3: Option<f64>,
    pub size: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAccessibilitySnapshot {
    pub series: GeneralSeriesId,
    pub title: String,
    pub total_rows: usize,
    pub offset: usize,
    pub items: Vec<GeneralAccessibilityItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralLegendItem {
    pub series: GeneralSeriesId,
    pub pane: usize,
    pub kind: GeneralSeriesKind,
    pub title: String,
    pub color: Option<String>,
    pub visible: bool,
    /// Bounded positive sector rows for pie, polar area and radial bar series.
    pub slices: Vec<GeneralLegendSlice>,
    pub slices_truncated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralLegendSlice {
    pub row: usize,
    pub label: String,
    pub value: f64,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralLegendSnapshot {
    pub items: Vec<GeneralLegendItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeneralBrushRange {
    Numeric([f64; 2]),
    Temporal([i64; 2]),
    Category([String; 2]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneralBrushEndpoint {
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralBrushSnapshot {
    pub pane: usize,
    pub axis_id: String,
    pub dimension: AxisDimension,
    pub range: GeneralBrushRange,
    pub items: Vec<GeneralSeriesHit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeneralReferenceId(NonZeroU32);

impl GeneralReferenceId {
    pub fn get(self) -> u32 {
        self.0.get()
    }

    #[doc(hidden)]
    pub fn from_raw(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GeneralReferenceValue {
    Numeric(f64),
    Temporal(i64),
    Category(String),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeneralReferenceOptions {
    Line {
        pane: usize,
        axis_id: String,
        value: GeneralReferenceValue,
        #[serde(default)]
        color: Option<String>,
        #[serde(default = "default_reference_line_width")]
        line_width: f64,
        #[serde(default)]
        extend_domain: bool,
    },
    Dot {
        pane: usize,
        x_axis_id: String,
        y_axis_id: String,
        x: GeneralReferenceValue,
        y: GeneralReferenceValue,
        #[serde(default)]
        color: Option<String>,
        #[serde(default = "default_reference_dot_radius")]
        radius: f64,
        #[serde(default)]
        extend_domain: bool,
    },
    Region {
        pane: usize,
        x_axis_id: String,
        y_axis_id: String,
        x_from: GeneralReferenceValue,
        x_to: GeneralReferenceValue,
        y_from: GeneralReferenceValue,
        y_to: GeneralReferenceValue,
        #[serde(default)]
        fill_color: Option<String>,
        #[serde(default)]
        extend_domain: bool,
    },
}

fn default_reference_line_width() -> f64 {
    1.0
}

fn default_reference_dot_radius() -> f64 {
    4.0
}

impl GeneralReferenceOptions {
    pub fn pane(&self) -> usize {
        match self {
            Self::Line { pane, .. } | Self::Dot { pane, .. } | Self::Region { pane, .. } => *pane,
        }
    }

    fn set_pane(&mut self, pane: usize) {
        match self {
            Self::Line { pane: value, .. }
            | Self::Dot { pane: value, .. }
            | Self::Region { pane: value, .. } => *value = pane,
        }
    }

    pub fn extend_domain(&self) -> bool {
        match self {
            Self::Line { extend_domain, .. }
            | Self::Dot { extend_domain, .. }
            | Self::Region { extend_domain, .. } => *extend_domain,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralReference {
    id: GeneralReferenceId,
    pane_id: PaneId,
    options: GeneralReferenceOptions,
    composition_key: Option<String>,
}

impl GeneralReference {
    pub fn id(&self) -> GeneralReferenceId {
        self.id
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    pub fn options(&self) -> &GeneralReferenceOptions {
        &self.options
    }

    fn estimated_bytes(&self) -> usize {
        fn value_bytes(value: &GeneralReferenceValue) -> usize {
            match value {
                GeneralReferenceValue::Category(value) => value.capacity(),
                GeneralReferenceValue::Numeric(_) | GeneralReferenceValue::Temporal(_) => 0,
            }
        }
        self.composition_key.as_ref().map_or(0, String::capacity)
            + match &self.options {
                GeneralReferenceOptions::Line {
                    axis_id,
                    value,
                    color,
                    ..
                } => {
                    axis_id.capacity()
                        + value_bytes(value)
                        + color.as_ref().map_or(0, String::capacity)
                }
                GeneralReferenceOptions::Dot {
                    x_axis_id,
                    y_axis_id,
                    x,
                    y,
                    color,
                    ..
                } => {
                    x_axis_id.capacity()
                        + y_axis_id.capacity()
                        + value_bytes(x)
                        + value_bytes(y)
                        + color.as_ref().map_or(0, String::capacity)
                }
                GeneralReferenceOptions::Region {
                    x_axis_id,
                    y_axis_id,
                    x_from,
                    x_to,
                    y_from,
                    y_to,
                    fill_color,
                    ..
                } => {
                    x_axis_id.capacity()
                        + y_axis_id.capacity()
                        + value_bytes(x_from)
                        + value_bytes(x_to)
                        + value_bytes(y_from)
                        + value_bytes(y_to)
                        + fill_color.as_ref().map_or(0, String::capacity)
                }
            }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GeneralBrushSelection {
    pane_id: PaneId,
    axis_id: String,
    dimension: AxisDimension,
    range: GeneralBrushRange,
}

impl GeneralSeries {
    pub fn id(&self) -> GeneralSeriesId {
        self.id
    }

    pub fn kind(&self) -> GeneralSeriesKind {
        self.kind
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    pub fn dataset(&self) -> GeneralDatasetId {
        self.dataset
    }

    pub fn x_axis_id(&self) -> &str {
        &self.x_axis_id
    }

    pub fn y_axis_id(&self) -> &str {
        &self.y_axis_id
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }

    pub fn point_radius(&self) -> f64 {
        self.point_radius
    }

    pub fn point_markers(&self) -> bool {
        self.point_markers
    }

    pub fn point_symbol(&self) -> GeneralPointSymbol {
        self.point_symbol
    }

    pub fn line_width(&self) -> f64 {
        self.line_width
    }

    pub fn line_style(&self) -> GeneralLineStyle {
        self.line_style
    }

    pub fn interpolation(&self) -> GeneralInterpolation {
        self.interpolation
    }

    pub fn connect_missing(&self) -> bool {
        self.connect_missing
    }

    pub fn fill_opacity(&self) -> f64 {
        self.fill_opacity
    }

    pub fn polar(&self) -> GeneralPolarOptions {
        self.polar
    }

    pub fn fill_gradient(&self) -> Option<&[String; 2]> {
        self.fill_gradient.as_ref()
    }

    pub fn baseline_value(&self) -> Option<f64> {
        self.baseline_value
    }

    pub fn baseline_policy(&self) -> GeneralAreaBaseline {
        self.baseline_policy
    }

    pub fn data_labels(&self) -> bool {
        self.data_labels
    }

    pub fn bar_gap(&self) -> f64 {
        self.bar_gap
    }

    pub fn bar_max_width(&self) -> Option<f64> {
        self.bar_max_width
    }

    pub fn bar_corner_radius(&self) -> f64 {
        self.bar_corner_radius
    }

    pub fn heatmap_value_domain(&self) -> Option<[f64; 2]> {
        self.heatmap_value_domain
    }

    pub fn heatmap_low_color(&self) -> Option<&str> {
        self.heatmap_low_color.as_deref()
    }

    pub fn heatmap_high_color(&self) -> Option<&str> {
        self.heatmap_high_color.as_deref()
    }

    pub fn box_fill_color(&self) -> Option<&str> {
        self.box_fill_color.as_deref()
    }

    pub fn box_median_color(&self) -> Option<&str> {
        self.box_median_color.as_deref()
    }

    pub fn group_id(&self) -> Option<&str> {
        self.group_id.as_deref()
    }

    pub fn stack_id(&self) -> Option<&str> {
        self.stack_id.as_deref()
    }

    pub fn stack_mode(&self) -> GeneralStackMode {
        self.stack_mode
    }

    pub fn histogram(&self) -> GeneralHistogramOptions {
        self.histogram
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.x_axis_id.capacity()
            + self.y_axis_id.capacity()
            + self.title.capacity()
            + self.color.as_ref().map_or(0, String::capacity)
            + self.heatmap_low_color.as_ref().map_or(0, String::capacity)
            + self.heatmap_high_color.as_ref().map_or(0, String::capacity)
            + self.box_fill_color.as_ref().map_or(0, String::capacity)
            + self.box_median_color.as_ref().map_or(0, String::capacity)
            + self.group_id.as_ref().map_or(0, String::capacity)
            + self.stack_id.as_ref().map_or(0, String::capacity)
            + self.composition_key.as_ref().map_or(0, String::capacity)
    }
}

#[derive(Clone)]
pub(crate) struct GeneralSeriesRegistry {
    series: Vec<GeneralSeries>,
    next_id: u32,
    references: Vec<GeneralReference>,
    next_reference_id: u32,
    scatter_spatial: RefCell<HashMap<GeneralSeriesId, ScatterSpatialIndex>>,
    polar_contexts: RefCell<HashMap<GeneralSeriesId, GeneralPolarCategoryContext>>,
    pie_contexts: RefCell<HashMap<GeneralSeriesId, GeneralPieContext>>,
    axis_scans: RefCell<HashMap<GeneralSeriesId, Vec<AxisScanEntry>>>,
    histogram_bins: RefCell<HashMap<GeneralSeriesId, GeneralHistogramCacheEntry>>,
    hovered: Option<GeneralInteractionTarget>,
    /// `hovered` is the anchor of an index-axis hover rather than an exact mark hit.
    hover_axis: bool,
    selected: Option<GeneralInteractionTarget>,
    accessibility_focused: Option<GeneralInteractionTarget>,
    brush: Option<GeneralBrushSelection>,
}

impl GeneralSeriesRegistry {
    pub(crate) fn new() -> Self {
        Self {
            series: Vec::new(),
            next_id: 1,
            references: Vec::new(),
            next_reference_id: 1,
            scatter_spatial: RefCell::new(HashMap::new()),
            polar_contexts: RefCell::new(HashMap::new()),
            pie_contexts: RefCell::new(HashMap::new()),
            axis_scans: RefCell::new(HashMap::new()),
            histogram_bins: RefCell::new(HashMap::new()),
            hovered: None,
            hover_axis: false,
            selected: None,
            accessibility_focused: None,
            brush: None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.series.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.series.is_empty() && self.references.is_empty() && self.brush.is_none()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &GeneralSeries> {
        self.series.iter()
    }

    pub(crate) fn get(&self, id: GeneralSeriesId) -> Option<&GeneralSeries> {
        self.series.iter().find(|series| series.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: GeneralSeriesId) -> Option<&mut GeneralSeries> {
        self.series.iter_mut().find(|series| series.id == id)
    }

    fn ids(&self, pane_id: Option<PaneId>) -> Vec<GeneralSeriesId> {
        self.series
            .iter()
            .filter(|series| pane_id.is_none_or(|pane_id| series.pane_id == pane_id))
            .map(GeneralSeries::id)
            .collect()
    }

    fn set_order(&mut self, pane_id: Option<PaneId>, ids: &[GeneralSeriesId]) -> bool {
        let positions = self
            .series
            .iter()
            .enumerate()
            .filter(|(_, series)| pane_id.is_none_or(|pane_id| series.pane_id == pane_id))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if positions.len() != ids.len()
            || ids.iter().copied().collect::<HashSet<_>>().len() != ids.len()
        {
            return false;
        }
        let mut current = positions
            .iter()
            .map(|&index| {
                let series = self.series[index].clone();
                (series.id, series)
            })
            .collect::<HashMap<_, _>>();
        if ids.iter().any(|id| !current.contains_key(id)) {
            return false;
        }

        let ordered = ids
            .iter()
            .map(|id| {
                current
                    .remove(id)
                    .expect("a validated general series order contains only live ids")
            })
            .collect::<Vec<_>>();
        for (index, series) in positions.into_iter().zip(ordered) {
            self.series[index] = series;
        }
        true
    }

    fn set_reference_order(&mut self, pane_id: PaneId, ids: &[GeneralReferenceId]) -> bool {
        let positions = self
            .references
            .iter()
            .enumerate()
            .filter(|(_, reference)| reference.pane_id == pane_id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if positions.len() != ids.len() {
            return false;
        }
        let mut current = positions
            .iter()
            .map(|&index| {
                let reference = self.references[index].clone();
                (reference.id, reference)
            })
            .collect::<HashMap<_, _>>();
        if current.len() != ids.len() || ids.iter().any(|id| !current.contains_key(id)) {
            return false;
        }
        for (index, id) in positions.into_iter().zip(ids) {
            self.references[index] = current
                .remove(id)
                .expect("a validated general reference order contains only live ids");
        }
        true
    }

    pub(crate) fn reference_iter(&self) -> impl Iterator<Item = &GeneralReference> {
        self.references.iter()
    }

    pub(crate) fn reference_get(&self, id: GeneralReferenceId) -> Option<&GeneralReference> {
        self.references.iter().find(|reference| reference.id == id)
    }

    pub(crate) fn reference_get_mut(
        &mut self,
        id: GeneralReferenceId,
    ) -> Option<&mut GeneralReference> {
        self.references
            .iter_mut()
            .find(|reference| reference.id == id)
    }

    pub(crate) fn reference_insert(
        &mut self,
        pane_id: PaneId,
        options: GeneralReferenceOptions,
    ) -> Result<GeneralReferenceId, ChartError> {
        if self.references.len() >= MAX_GENERAL_REFERENCES {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_REFERENCES} general references"
            )));
        }
        let id = NonZeroU32::new(self.next_reference_id)
            .map(GeneralReferenceId)
            .ok_or_else(|| resource("general reference identity space is exhausted"))?;
        self.next_reference_id = self
            .next_reference_id
            .checked_add(1)
            .ok_or_else(|| resource("general reference identity space is exhausted"))?;
        self.references.push(GeneralReference {
            id,
            pane_id,
            options,
            composition_key: None,
        });
        Ok(id)
    }

    pub(crate) fn reference_remove(&mut self, id: GeneralReferenceId) -> bool {
        let Some(index) = self
            .references
            .iter()
            .position(|reference| reference.id == id)
        else {
            return false;
        };
        self.references.remove(index);
        true
    }

    pub(crate) fn insert(
        &mut self,
        pane_id: PaneId,
        options: GeneralSeriesOptions,
    ) -> Result<GeneralSeriesId, ChartError> {
        if self.series.len() >= MAX_GENERAL_SERIES {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_SERIES} general series"
            )));
        }
        validate_presentation(&options)?;
        let id = NonZeroU32::new(self.next_id)
            .map(GeneralSeriesId)
            .ok_or_else(|| resource("general series identity space is exhausted"))?;
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| resource("general series identity space is exhausted"))?;
        self.series.push(GeneralSeries {
            id,
            kind: options.kind,
            pane_id,
            dataset: options.dataset,
            x_axis_id: options.x_axis_id,
            y_axis_id: options.y_axis_id,
            visible: options.visible,
            title: options.title,
            color: options.color,
            point_radius: options.point_radius,
            point_markers: options.point_markers,
            point_symbol: options.point_symbol,
            line_width: options.line_width,
            line_style: options.line_style,
            interpolation: options.interpolation,
            connect_missing: options.connect_missing,
            fill_opacity: options.fill_opacity,
            polar: options.polar,
            fill_gradient: options.fill_gradient,
            baseline_value: options.baseline_value,
            baseline_policy: options.baseline_policy,
            data_labels: options.data_labels,
            bar_gap: options.bar_gap,
            bar_max_width: options.bar_max_width,
            bar_corner_radius: options.bar_corner_radius,
            heatmap_value_domain: options.heatmap_value_domain,
            heatmap_low_color: options.heatmap_low_color,
            heatmap_high_color: options.heatmap_high_color,
            box_fill_color: options.box_fill_color,
            box_median_color: options.box_median_color,
            group_id: options.group_id,
            stack_id: options.stack_id,
            stack_mode: options.stack_mode,
            histogram: options.histogram,
            composition_key: None,
        });
        self.next_id = next_id;
        Ok(id)
    }

    pub(crate) fn remove(&mut self, id: GeneralSeriesId) -> bool {
        let Some(index) = self.series.iter().position(|series| series.id == id) else {
            return false;
        };
        self.series.remove(index);
        self.scatter_spatial.get_mut().remove(&id);
        self.polar_contexts.get_mut().remove(&id);
        self.pie_contexts.get_mut().remove(&id);
        self.axis_scans.get_mut().remove(&id);
        self.histogram_bins.get_mut().remove(&id);
        if self
            .hovered
            .as_ref()
            .is_some_and(|target| target.series == id)
        {
            self.hovered = None;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|target| target.series == id)
        {
            self.selected = None;
        }
        if self
            .accessibility_focused
            .as_ref()
            .is_some_and(|target| target.series == id)
        {
            self.accessibility_focused = None;
        }
        true
    }

    pub(crate) fn uses_axis(&self, id: &str) -> bool {
        self.series
            .iter()
            .any(|series| series.x_axis_id == id || series.y_axis_id == id)
            || self
                .references
                .iter()
                .any(|reference| match &reference.options {
                    GeneralReferenceOptions::Line { axis_id, .. } => axis_id == id,
                    GeneralReferenceOptions::Dot {
                        x_axis_id,
                        y_axis_id,
                        ..
                    }
                    | GeneralReferenceOptions::Region {
                        x_axis_id,
                        y_axis_id,
                        ..
                    } => x_axis_id == id || y_axis_id == id,
                })
            || self.brush.as_ref().is_some_and(|brush| brush.axis_id == id)
    }

    pub(crate) fn uses_dataset(&self, id: GeneralDatasetId) -> bool {
        self.series.iter().any(|series| series.dataset == id)
    }

    pub(crate) fn uses_pane(&self, pane_id: PaneId) -> bool {
        self.series.iter().any(|series| series.pane_id == pane_id)
            || self
                .references
                .iter()
                .any(|reference| reference.pane_id == pane_id)
            || self
                .brush
                .as_ref()
                .is_some_and(|brush| brush.pane_id == pane_id)
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        let scatter_bytes = self.scatter_spatial.try_borrow().map_or(0, |cache| {
            cache.capacity()
                * (std::mem::size_of::<GeneralSeriesId>()
                    + std::mem::size_of::<ScatterSpatialIndex>())
                + cache
                    .values()
                    .map(ScatterSpatialIndex::estimated_bytes)
                    .sum::<usize>()
        });
        let axis_scan_bytes = self.axis_scans.try_borrow().map_or(0, |cache| {
            cache.capacity() * std::mem::size_of::<(GeneralSeriesId, Vec<AxisScanEntry>)>()
                + cache
                    .values()
                    .map(|entries| entries.capacity() * std::mem::size_of::<AxisScanEntry>())
                    .sum::<usize>()
        });
        let polar_bytes = self.polar_contexts.try_borrow().map_or(0, |cache| {
            cache.capacity() * std::mem::size_of::<(GeneralSeriesId, GeneralPolarCategoryContext)>()
                + cache
                    .values()
                    .map(|entry| {
                        entry.rows.capacity() * std::mem::size_of::<Option<usize>>()
                            + entry.radar_marker_rows.as_ref().map_or(0, |rows| {
                                rows.capacity() * std::mem::size_of::<Option<usize>>()
                            })
                            + entry.sector_bounds.as_ref().map_or(0, |bounds| {
                                bounds.capacity() * std::mem::size_of::<(f64, f64)>()
                            })
                            + entry
                                .radar_radii
                                .as_ref()
                                .map_or(0, |radii| radii.capacity() * std::mem::size_of::<f64>())
                    })
                    .sum::<usize>()
        });
        let pie_bytes = self.pie_contexts.try_borrow().map_or(0, |cache| {
            cache.capacity() * std::mem::size_of::<(GeneralSeriesId, GeneralPieContext)>()
                + cache
                    .values()
                    .map(|entry| entry.slices.capacity() * std::mem::size_of::<GeneralPieSlice>())
                    .sum::<usize>()
        });
        let histogram_bytes = self.histogram_bins.try_borrow().map_or(0, |cache| {
            cache.capacity() * std::mem::size_of::<(GeneralSeriesId, GeneralHistogramCacheEntry)>()
                + cache
                    .values()
                    .map(|entry| entry.bins.estimated_bytes())
                    .sum::<usize>()
        });
        let brush_bytes = self.brush.as_ref().map_or(0, |brush| {
            brush.axis_id.capacity()
                + match &brush.range {
                    GeneralBrushRange::Category(values) => {
                        values[0].capacity() + values[1].capacity()
                    }
                    GeneralBrushRange::Numeric(_) | GeneralBrushRange::Temporal(_) => 0,
                }
        });
        self.series.capacity() * std::mem::size_of::<GeneralSeries>()
            + self
                .series
                .iter()
                .map(GeneralSeries::estimated_bytes)
                .sum::<usize>()
            + scatter_bytes
            + axis_scan_bytes
            + polar_bytes
            + pie_bytes
            + histogram_bytes
            + self.references.capacity() * std::mem::size_of::<GeneralReference>()
            + self
                .references
                .iter()
                .map(GeneralReference::estimated_bytes)
                .sum::<usize>()
            + self
                .hovered
                .as_ref()
                .map_or(0, GeneralInteractionTarget::estimated_bytes)
            + self
                .selected
                .as_ref()
                .map_or(0, GeneralInteractionTarget::estimated_bytes)
            + self
                .accessibility_focused
                .as_ref()
                .map_or(0, GeneralInteractionTarget::estimated_bytes)
            + brush_bytes
    }
}

impl Default for GeneralSeriesRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn reference_axis_ids(options: &GeneralReferenceOptions) -> Vec<&str> {
    match options {
        GeneralReferenceOptions::Line { axis_id, .. } => vec![axis_id],
        GeneralReferenceOptions::Dot {
            x_axis_id,
            y_axis_id,
            ..
        }
        | GeneralReferenceOptions::Region {
            x_axis_id,
            y_axis_id,
            ..
        } => vec![x_axis_id, y_axis_id],
    }
}

fn validate_composed_layout(series: &[GeneralPaneSeriesComposition]) -> Result<(), ChartError> {
    for (index, proposed) in series.iter().enumerate() {
        let options = &proposed.options;
        for sibling in series.iter().skip(index + 1).map(|item| &item.options) {
            if options.kind != sibling.kind {
                continue;
            }
            if matches!(
                options.kind,
                GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar
            ) && options.group_id.is_some()
                && options.group_id == sibling.group_id
            {
                let same_category_axis = match options.kind {
                    GeneralSeriesKind::Column => options.x_axis_id == sibling.x_axis_id,
                    GeneralSeriesKind::HorizontalBar => options.y_axis_id == sibling.y_axis_id,
                    _ => true,
                };
                if !same_category_axis {
                    return Err(invalid(
                        "grouped bar series must share the same category axis",
                    ));
                }
            }
            if options.stack_id.is_some()
                && options.stack_id == sibling.stack_id
                && options.group_id == sibling.group_id
                && (options.x_axis_id != sibling.x_axis_id
                    || options.y_axis_id != sibling.y_axis_id
                    || options.stack_mode != sibling.stack_mode
                    || (matches!(
                        options.kind,
                        GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar
                    ) && (options.bar_gap != sibling.bar_gap
                        || options.bar_max_width != sibling.bar_max_width))
                    || (options.kind == GeneralSeriesKind::XyArea
                        && (options.interpolation != sibling.interpolation
                            || options.connect_missing != sibling.connect_missing)))
            {
                return Err(invalid(
                    "stacked series must share X/Y axes, group ID, stack mode, bar thickness, and area path policy",
                ));
            }
        }
    }
    Ok(())
}

fn apply_composed_series_options(
    series: &mut GeneralSeries,
    pane_id: PaneId,
    options: GeneralSeriesOptions,
) {
    series.kind = options.kind;
    series.pane_id = pane_id;
    series.dataset = options.dataset;
    series.x_axis_id = options.x_axis_id;
    series.y_axis_id = options.y_axis_id;
    series.visible = options.visible;
    series.title = options.title;
    series.color = options.color;
    series.point_radius = options.point_radius;
    series.point_markers = options.point_markers;
    series.point_symbol = options.point_symbol;
    series.line_width = options.line_width;
    series.line_style = options.line_style;
    series.interpolation = options.interpolation;
    series.connect_missing = options.connect_missing;
    series.fill_opacity = options.fill_opacity;
    series.polar = options.polar;
    series.fill_gradient = options.fill_gradient;
    series.baseline_value = options.baseline_value;
    series.baseline_policy = options.baseline_policy;
    series.data_labels = options.data_labels;
    series.bar_gap = options.bar_gap;
    series.bar_max_width = options.bar_max_width;
    series.bar_corner_radius = options.bar_corner_radius;
    series.heatmap_value_domain = options.heatmap_value_domain;
    series.heatmap_low_color = options.heatmap_low_color;
    series.heatmap_high_color = options.heatmap_high_color;
    series.box_fill_color = options.box_fill_color;
    series.box_median_color = options.box_median_color;
    series.group_id = options.group_id;
    series.stack_id = options.stack_id;
    series.stack_mode = options.stack_mode;
    series.histogram = options.histogram;
}

impl ChartEngine {
    pub(crate) fn invalidate_general_dataset_geometry_caches(&self, dataset: GeneralDatasetId) {
        let Some(registry) = self.general_series.as_ref() else {
            return;
        };
        for series in registry
            .series
            .iter()
            .filter(|series| series.dataset == dataset)
        {
            registry.scatter_spatial.borrow_mut().remove(&series.id);
            registry.polar_contexts.borrow_mut().remove(&series.id);
            registry.pie_contexts.borrow_mut().remove(&series.id);
            registry.axis_scans.borrow_mut().remove(&series.id);
            registry.histogram_bins.borrow_mut().remove(&series.id);
        }
    }
    /// Validate a complete proposed general pane without changing live state.
    pub fn validate_general_pane_proposal(
        &self,
        proposal: &GeneralPaneProposal,
    ) -> Result<(), ChartError> {
        let pane = proposal.pane;
        let pane_id = self
            .pane_stable_id(pane)
            .ok_or_else(|| invalid("general pane proposal references a stale pane"))?;
        let pane_domain = self
            .pane_horizontal_domain(pane)
            .ok_or_else(|| invalid("general pane proposal has no horizontal domain"))?;
        let mut axis_ids = std::collections::HashSet::with_capacity(proposal.axes.len());
        for axis in &proposal.axes {
            if axis.pane != pane {
                return Err(invalid("proposed general axes must belong to their pane"));
            }
            if !axis_ids.insert(axis.id.as_str()) {
                return Err(invalid("general axis IDs must be unique"));
            }
            self.validate_general_axis_definition(axis)?;
        }

        let axis = |id: &str| {
            proposal
                .axes
                .iter()
                .find(|axis| axis.id == id)
                .ok_or_else(|| invalid(format!("general series axis {id:?} does not exist")))
        };
        let mut data = crate::general_data::GeneralDataStore::new();
        for series in &proposal.series {
            let x_axis = axis(&series.x_axis_id)?;
            let y_axis = axis(&series.y_axis_id)?;
            let (expected_x, expected_y) = series_axis_dimensions(series.kind);
            if x_axis.dimension != expected_x || y_axis.dimension != expected_y {
                return Err(invalid("general series axis dimensions are incompatible"));
            }
            validate_general_series_axis_compatibility(
                series.kind,
                pane_domain,
                series.data.x_kind(),
                x_axis.scale,
                y_axis.scale,
                matches!(series.data, GeneralXyInput::HeatmapCategoryCategory { .. }),
                matches!(
                    series.data,
                    GeneralXyInput::HeatmapNumericNumeric { .. }
                        | GeneralXyInput::HeatmapTemporalNumeric { .. }
                ),
            )?;
            let dataset_id = data.insert(series.data.clone())?;
            let dataset = data
                .get(dataset_id)
                .ok_or_else(|| invalid("proposed general dataset could not be validated"))?;
            validate_dataset_for_series(series.kind, dataset, x_axis.scale, y_axis.scale)?;
        }

        let axis_info = |id: &str| {
            let axis = proposal
                .axes
                .iter()
                .find(|axis| axis.id == id)
                .ok_or_else(|| invalid(format!("general reference axis {id:?} does not exist")))?;
            Ok((pane_id, axis.dimension, axis.scale))
        };
        for reference in &proposal.references {
            validate_general_reference_options(pane_id, reference, axis_info)?;
        }
        Ok(())
    }

    /// Atomically replace the complete general graph owned by one pane.
    pub fn compose_general_pane(
        &mut self,
        composition: GeneralPaneComposition,
    ) -> Result<GeneralPaneCompositionResult, ChartError> {
        self.validate_general_pane_composition(&composition)?;

        // Only the three general-domain registries are staged. Financial data, panes, drawings,
        // interaction, retained frames, and every other engine owner remain untouched.
        let staged_axes = self.general_axes.clone();
        let staged_data = self.general_data.clone();
        let staged_series = self.general_series.clone();
        let original_axes = std::mem::replace(&mut self.general_axes, staged_axes);
        let original_data = std::mem::replace(&mut self.general_data, staged_data);
        let original_series = std::mem::replace(&mut self.general_series, staged_series);
        let original_invalidation = self.frame_invalidation.clone();

        match self.apply_general_pane_composition(composition) {
            Ok((result, structurally_removed_axes)) => {
                self.general_dataset_handles_issued |= !result.series.is_empty();
                for axis_id in structurally_removed_axes {
                    self.disarm_general_brush_input_for_axis(&axis_id);
                }
                self.invalidate_frame_all();
                Ok(result)
            }
            Err(error) => {
                self.general_axes = original_axes;
                self.general_data = original_data;
                self.general_series = original_series;
                self.frame_invalidation = original_invalidation;
                Err(error)
            }
        }
    }

    fn validate_general_pane_composition(
        &self,
        composition: &GeneralPaneComposition,
    ) -> Result<(), ChartError> {
        let pane = composition.pane;
        let pane_id = self
            .pane_stable_id(pane)
            .ok_or_else(|| invalid("general pane composition references a stale pane"))?;
        let pane_domain = self
            .pane_horizontal_domain(pane)
            .ok_or_else(|| invalid("general pane composition has no horizontal domain"))?;
        let mut axis_ids = HashSet::with_capacity(composition.axes.len());
        for axis in &composition.axes {
            if axis.pane != pane {
                return Err(invalid("composed general axes must belong to their pane"));
            }
            if !axis_ids.insert(axis.id.as_str()) {
                return Err(invalid("general axis IDs must be unique"));
            }
            if self
                .general_axes
                .iter()
                .any(|live| live.id() == axis.id && live.pane_id() != pane_id)
            {
                return Err(invalid(format!(
                    "general axis id {:?} already exists",
                    axis.id
                )));
            }
            self.validate_general_axis_definition(axis)?;
        }
        let axis = |id: &str| {
            composition
                .axes
                .iter()
                .find(|axis| axis.id == id)
                .ok_or_else(|| invalid(format!("general series axis {id:?} does not exist")))
        };

        let mut series_keys = HashSet::with_capacity(composition.series.len());
        let mut current_series = HashSet::with_capacity(composition.series.len());
        let mut validated_data = crate::general_data::GeneralDataStore::new();
        for proposed in &composition.series {
            if proposed.key.is_empty()
                || proposed.key.len() > MAX_GENERAL_SERIES_TITLE_BYTES
                || !series_keys.insert(proposed.key.as_str())
            {
                return Err(invalid(
                    "general series composition keys must be bounded, nonempty, and unique",
                ));
            }
            if proposed.options.pane != pane {
                return Err(invalid("composed general series must belong to their pane"));
            }
            if let Some(current) = proposed.current {
                if !current_series.insert(current) {
                    return Err(invalid(
                        "a current general series may be composed only once",
                    ));
                }
                let live = self.general_series(current).ok_or_else(|| {
                    ChartError::new(ErrorCode::InvalidHandle, "current general series is stale")
                })?;
                if live.pane_id() != pane_id {
                    return Err(invalid("current general series belongs to another pane"));
                }
            }
            let x_axis = axis(&proposed.options.x_axis_id)?;
            let y_axis = axis(&proposed.options.y_axis_id)?;
            let (expected_x, expected_y) = series_axis_dimensions(proposed.options.kind);
            if x_axis.dimension != expected_x || y_axis.dimension != expected_y {
                return Err(invalid("general series axis dimensions are incompatible"));
            }
            validate_presentation(&proposed.options)?;
            validate_general_series_axis_compatibility(
                proposed.options.kind,
                pane_domain,
                proposed.data.x_kind(),
                x_axis.scale,
                y_axis.scale,
                matches!(
                    proposed.data,
                    GeneralXyInput::HeatmapCategoryCategory { .. }
                ),
                matches!(
                    proposed.data,
                    GeneralXyInput::HeatmapNumericNumeric { .. }
                        | GeneralXyInput::HeatmapTemporalNumeric { .. }
                ),
            )?;
            let dataset = validated_data.insert_styled(
                proposed.data.clone(),
                proposed.labels.clone(),
                proposed.colors.clone(),
                proposed.symbols.clone(),
            )?;
            let validated_dataset = validated_data
                .get(dataset)
                .expect("newly validated composition data remains present");
            validate_dataset_for_series(
                proposed.options.kind,
                validated_dataset,
                x_axis.scale,
                y_axis.scale,
            )?;
            validate_histogram_dataset(&proposed.options, validated_dataset)?;
        }
        validate_composed_layout(&composition.series)?;

        let mut reference_keys = HashSet::with_capacity(composition.references.len());
        let mut current_references = HashSet::with_capacity(composition.references.len());
        for proposed in &composition.references {
            if proposed.key.is_empty()
                || proposed.key.len() > MAX_GENERAL_SERIES_TITLE_BYTES
                || !reference_keys.insert(proposed.key.as_str())
            {
                return Err(invalid(
                    "general reference composition keys must be bounded, nonempty, and unique",
                ));
            }
            if proposed.options.pane() != pane {
                return Err(invalid(
                    "composed general references must belong to their pane",
                ));
            }
            if let Some(current) = proposed.current {
                if !current_references.insert(current) {
                    return Err(invalid(
                        "a current general reference may be composed only once",
                    ));
                }
                let live = self
                    .general_series
                    .as_ref()
                    .and_then(|registry| registry.reference_get(current))
                    .ok_or_else(|| {
                        ChartError::new(
                            ErrorCode::InvalidHandle,
                            "current general reference is stale",
                        )
                    })?;
                if live.pane_id() != pane_id {
                    return Err(invalid("current general reference belongs to another pane"));
                }
            }
            validate_general_reference_options(pane_id, &proposed.options, |id| {
                let axis = axis(id)?;
                Ok((pane_id, axis.dimension, axis.scale))
            })?;
        }
        Ok(())
    }

    fn apply_general_pane_composition(
        &mut self,
        composition: GeneralPaneComposition,
    ) -> Result<(GeneralPaneCompositionResult, Vec<String>), ChartError> {
        let pane = composition.pane;
        let pane_id = self
            .pane_stable_id(pane)
            .expect("a validated composition pane remains present");
        let structural_axes = composition
            .axes
            .iter()
            .filter_map(|options| {
                self.general_axis(&options.id)
                    .filter(|axis| {
                        axis.pane_id() == pane_id
                            && (axis.dimension() != options.dimension
                                || axis.scale() != options.scale)
                    })
                    .map(|_| options.id.clone())
            })
            .collect::<HashSet<_>>();

        let retain_series = composition
            .series
            .iter()
            .filter_map(|proposed| {
                let id = proposed.current?;
                let live = self.general_series(id)?;
                (live
                    .composition_key
                    .as_deref()
                    .is_none_or(|key| key == proposed.key)
                    && live.kind == proposed.options.kind
                    && !structural_axes.contains(&live.x_axis_id)
                    && !structural_axes.contains(&live.y_axis_id)
                    && !structural_axes.contains(&proposed.options.x_axis_id)
                    && !structural_axes.contains(&proposed.options.y_axis_id))
                .then_some(id)
            })
            .collect::<HashSet<_>>();
        let retain_references = composition
            .references
            .iter()
            .filter_map(|proposed| {
                let id = proposed.current?;
                let live = self.general_series.as_ref()?.reference_get(id)?;
                (live
                    .composition_key
                    .as_deref()
                    .is_none_or(|key| key == proposed.key)
                    && std::mem::discriminant(&live.options)
                        == std::mem::discriminant(&proposed.options)
                    && !reference_axis_ids(&live.options)
                        .iter()
                        .any(|axis| structural_axes.contains(*axis)))
                .then_some(id)
            })
            .collect::<HashSet<_>>();

        let (remove_series, remove_references) = self.general_series.as_ref().map_or_else(
            || (Vec::new(), Vec::new()),
            |registry| {
                (
                    registry
                        .series
                        .iter()
                        .filter(|series| {
                            series.pane_id == pane_id && !retain_series.contains(&series.id)
                        })
                        .map(|series| (series.id, series.dataset))
                        .collect(),
                    registry
                        .references
                        .iter()
                        .filter(|reference| {
                            reference.pane_id == pane_id
                                && !retain_references.contains(&reference.id)
                        })
                        .map(|reference| reference.id)
                        .collect(),
                )
            },
        );
        if let Some(registry) = self.general_series.as_mut() {
            for id in remove_references {
                registry.reference_remove(id);
            }
            for &(id, _) in &remove_series {
                registry.remove(id);
            }
        }
        for (_, dataset) in remove_series {
            if !self.general_series_uses_dataset(dataset)
                && let Some(store) = self.general_data.as_mut()
            {
                store.remove(dataset);
            }
        }

        let desired_axes = composition
            .axes
            .iter()
            .map(|axis| axis.id.as_str())
            .collect::<HashSet<_>>();
        let axis_order = composition
            .axes
            .iter()
            .map(|axis| axis.id.clone())
            .collect::<Vec<_>>();
        let removed_axes = self
            .general_axes(Some(pane))
            .into_iter()
            .filter(|axis| !desired_axes.contains(axis.id()) || structural_axes.contains(axis.id()))
            .map(|axis| axis.id().to_owned())
            .collect::<Vec<_>>();
        for id in &removed_axes {
            self.general_axes.remove(id);
        }
        // A semantic range cannot survive a removed axis or a dimension/scale replacement.
        let brush_cleared = self.general_series.as_mut().is_some_and(|registry| {
            let cleared = registry
                .brush
                .as_ref()
                .is_some_and(|brush| removed_axes.contains(&brush.axis_id));
            if cleared {
                registry.brush = None;
            }
            cleared
        });
        for options in composition.axes {
            if self.general_axis(&options.id).is_some() {
                self.update_general_axis_options(options)?;
            } else {
                self.add_general_axis(options)?;
            }
        }
        if !self.general_axes.set_pane_order(pane_id, &axis_order) {
            return Err(invalid("composed general axis order is invalid"));
        }

        let mut series_result = Vec::with_capacity(composition.series.len());
        for mut proposed in composition.series {
            let retained = proposed.current.filter(|id| retain_series.contains(id));
            let (id, dataset) = if let Some(id) = retained {
                let dataset = self
                    .general_series(id)
                    .expect("retained composition series remains present")
                    .dataset;
                self.general_data
                    .as_mut()
                    .expect("retained composition series has a data store")
                    .replace_styled(
                        dataset,
                        proposed.data,
                        proposed.labels,
                        proposed.colors,
                        proposed.symbols,
                    )?;
                proposed.options.dataset = dataset;
                let registry = self
                    .general_series
                    .as_mut()
                    .expect("retained composition series has a registry");
                let series = registry
                    .get_mut(id)
                    .expect("retained composition series remains present");
                apply_composed_series_options(series, pane_id, proposed.options);
                series.composition_key = Some(proposed.key.clone());
                registry.scatter_spatial.get_mut().remove(&id);
                registry.axis_scans.get_mut().remove(&id);
                registry.histogram_bins.get_mut().remove(&id);
                (id, dataset)
            } else {
                let (data, labels, colors, symbols) = (
                    proposed.data,
                    proposed.labels,
                    proposed.colors,
                    proposed.symbols,
                );
                let dataset = if let Some(store) = self.general_data.as_mut() {
                    store.insert_styled(data, labels, colors, symbols)?
                } else {
                    let mut store = crate::general_data::GeneralDataStore::new();
                    let dataset = store.insert_styled(data, labels, colors, symbols)?;
                    self.general_data = Some(store);
                    dataset
                };
                proposed.options.dataset = dataset;
                let id = if let Some(registry) = self.general_series.as_mut() {
                    registry.insert(pane_id, proposed.options)?
                } else {
                    let mut registry = GeneralSeriesRegistry::new();
                    let id = registry.insert(pane_id, proposed.options)?;
                    self.general_series = Some(registry);
                    id
                };
                self.general_series
                    .as_mut()
                    .and_then(|registry| registry.get_mut(id))
                    .expect("new composition series remains present")
                    .composition_key = Some(proposed.key.clone());
                (id, dataset)
            };
            series_result.push(GeneralPaneComposedSeries {
                key: proposed.key,
                id,
                dataset,
            });
        }
        let order = series_result.iter().map(|item| item.id).collect::<Vec<_>>();
        if let Some(registry) = self.general_series.as_mut()
            && !registry.set_order(Some(pane_id), &order)
        {
            return Err(invalid("composed general series order is invalid"));
        }

        let mut reference_result = Vec::with_capacity(composition.references.len());
        for proposed in composition.references {
            let retained = proposed.current.filter(|id| retain_references.contains(id));
            let id = if let Some(id) = retained {
                let reference = self
                    .general_series
                    .as_mut()
                    .and_then(|registry| registry.reference_get_mut(id))
                    .expect("retained composition reference remains present");
                reference.options = proposed.options;
                reference.pane_id = pane_id;
                reference.composition_key = Some(proposed.key.clone());
                id
            } else {
                let id = if let Some(registry) = self.general_series.as_mut() {
                    registry.reference_insert(pane_id, proposed.options)?
                } else {
                    let mut registry = GeneralSeriesRegistry::new();
                    let id = registry.reference_insert(pane_id, proposed.options)?;
                    self.general_series = Some(registry);
                    id
                };
                self.general_series
                    .as_mut()
                    .and_then(|registry| registry.reference_get_mut(id))
                    .expect("new composition reference remains present")
                    .composition_key = Some(proposed.key.clone());
                id
            };
            reference_result.push(GeneralPaneComposedReference {
                key: proposed.key,
                id,
            });
        }
        let reference_order = reference_result
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        if let Some(registry) = self.general_series.as_mut()
            && !registry.set_reference_order(pane_id, &reference_order)
        {
            return Err(invalid("composed general reference order is invalid"));
        }

        if self
            .general_data
            .as_ref()
            .is_some_and(|store| store.is_empty())
        {
            self.general_data = None;
        }
        if self
            .general_series
            .as_ref()
            .is_some_and(GeneralSeriesRegistry::is_empty)
        {
            self.general_series = None;
        }
        Ok((
            GeneralPaneCompositionResult {
                series: series_result,
                references: reference_result,
                brush_cleared,
            },
            removed_axes,
        ))
    }

    #[doc(hidden)]
    pub fn add_general_series(
        &mut self,
        options: GeneralSeriesOptions,
    ) -> Result<GeneralSeriesId, ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane)
            .ok_or_else(|| invalid("general series references a stale pane"))?;
        let pane_domain = self
            .pane_horizontal_domain(options.pane)
            .ok_or_else(|| invalid("general series references a stale pane"))?;
        let dataset = self.general_dataset(options.dataset).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general dataset handle is stale")
        })?;
        let dataset_kind = dataset.x_kind();
        let x_axis = self
            .general_axis(&options.x_axis_id)
            .cloned()
            .ok_or_else(|| invalid("general series X axis does not exist"))?;
        let y_axis = self
            .general_axis(&options.y_axis_id)
            .cloned()
            .ok_or_else(|| invalid("general series Y axis does not exist"))?;

        if x_axis.pane_id() != pane_id || y_axis.pane_id() != pane_id {
            return Err(invalid("general series axes must belong to its pane"));
        }
        let (expected_x, expected_y) = series_axis_dimensions(options.kind);
        if x_axis.dimension() != expected_x || y_axis.dimension() != expected_y {
            return Err(invalid("general series axis dimensions are incompatible"));
        }
        validate_general_series_axis_compatibility(
            options.kind,
            pane_domain,
            dataset_kind,
            x_axis.scale(),
            y_axis.scale(),
            dataset.heatmap_y_categories().is_some(),
            dataset.heatmap_y_numeric().is_some(),
        )?;
        validate_dataset_for_series(options.kind, dataset, x_axis.scale(), y_axis.scale())?;
        validate_presentation(&options)?;
        validate_histogram_dataset(&options, dataset)?;
        self.validate_layout_compatibility(pane_id, &options, None)?;
        let id = if let Some(registry) = self.general_series.as_mut() {
            registry.insert(pane_id, options)?
        } else {
            let mut registry = GeneralSeriesRegistry::new();
            let id = registry.insert(pane_id, options)?;
            self.general_series = Some(registry);
            id
        };
        self.invalidate_frame_all();
        Ok(id)
    }

    fn validate_layout_compatibility(
        &self,
        pane_id: PaneId,
        options: &GeneralSeriesOptions,
        ignored: Option<GeneralSeriesId>,
    ) -> Result<(), ChartError> {
        if matches!(
            options.kind,
            GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar
        ) && let Some(group_id) = options.group_id.as_deref()
        {
            for sibling in self.general_series_iter().filter(|series| {
                Some(series.id) != ignored
                    && series.kind == options.kind
                    && series.pane_id == pane_id
                    && series.group_id() == Some(group_id)
            }) {
                let same_category_axis = match options.kind {
                    GeneralSeriesKind::Column => sibling.x_axis_id == options.x_axis_id,
                    GeneralSeriesKind::HorizontalBar => sibling.y_axis_id == options.y_axis_id,
                    _ => true,
                };
                if !same_category_axis {
                    return Err(invalid(
                        "grouped bar series must share the same category axis",
                    ));
                }
            }
        }

        if let Some(stack_id) = options.stack_id.as_deref() {
            for sibling in self.general_series_iter().filter(|series| {
                Some(series.id) != ignored
                    && series.kind == options.kind
                    && series.pane_id == pane_id
                    && series.group_id() == options.group_id.as_deref()
                    && series.stack_id() == Some(stack_id)
            }) {
                if sibling.x_axis_id != options.x_axis_id
                    || sibling.y_axis_id != options.y_axis_id
                    || sibling.stack_mode != options.stack_mode
                    || (matches!(
                        options.kind,
                        GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar
                    ) && (sibling.bar_gap != options.bar_gap
                        || sibling.bar_max_width != options.bar_max_width))
                    || (options.kind == GeneralSeriesKind::XyArea
                        && (sibling.interpolation != options.interpolation
                            || sibling.connect_missing != options.connect_missing))
                {
                    return Err(invalid(
                        "stacked series must share X/Y axes, group ID, stack mode, bar thickness, and area path policy",
                    ));
                }
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn update_general_series_options(
        &mut self,
        id: GeneralSeriesId,
        options: GeneralSeriesOptions,
    ) -> Result<(), ChartError> {
        let current = self.general_series(id).cloned().ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general series handle is stale")
        })?;
        let pane = self
            .pane_index_for_id(current.pane_id)
            .ok_or_else(|| invalid("general series references a stale pane"))?;
        if options.kind != current.kind || options.dataset != current.dataset {
            return Err(invalid("general series kind and dataset are structural"));
        }
        let current_domain = self
            .pane_horizontal_domain(pane)
            .ok_or_else(|| invalid("general series references a stale pane"))?;
        let target_pane_id = self
            .pane_stable_id(options.pane)
            .ok_or_else(|| invalid("general series references a stale target pane"))?;
        let target_domain = self
            .pane_horizontal_domain(options.pane)
            .ok_or_else(|| invalid("general series target pane has no horizontal domain"))?;
        let current_x = self
            .general_axis(&current.x_axis_id)
            .ok_or_else(|| invalid("general series current X axis does not exist"))?;
        let current_y = self
            .general_axis(&current.y_axis_id)
            .ok_or_else(|| invalid("general series current Y axis does not exist"))?;
        let target_x = self
            .general_axis(&options.x_axis_id)
            .ok_or_else(|| invalid("general series target X axis does not exist"))?;
        let target_y = self
            .general_axis(&options.y_axis_id)
            .ok_or_else(|| invalid("general series target Y axis does not exist"))?;
        let (expected_x, expected_y) = series_axis_dimensions(options.kind);
        if target_x.pane_id() != target_pane_id
            || target_y.pane_id() != target_pane_id
            || target_x.dimension() != expected_x
            || target_y.dimension() != expected_y
        {
            return Err(invalid(
                "general series target axes must be X/Y axes in its target pane",
            ));
        }
        if target_domain != current_domain
            || target_x.scale() != current_x.scale()
            || target_y.scale() != current_y.scale()
        {
            return Err(invalid(
                "general series rebinding requires equivalent pane and axis scale semantics",
            ));
        }
        validate_presentation(&options)?;
        if let Some(dataset) = self.general_dataset(current.dataset) {
            validate_histogram_dataset(&options, dataset)?;
        }
        self.validate_layout_compatibility(target_pane_id, &options, Some(id))?;

        let registry = self
            .general_series
            .as_mut()
            .expect("a resolved general series has a registry");
        let series = registry
            .get_mut(id)
            .expect("a resolved general series remains live during one mutation");
        let geometry_binding_changed = series.pane_id != target_pane_id
            || series.x_axis_id != options.x_axis_id
            || series.y_axis_id != options.y_axis_id
            || series.point_radius != options.point_radius;
        series.pane_id = target_pane_id;
        series.x_axis_id = options.x_axis_id;
        series.y_axis_id = options.y_axis_id;
        series.visible = options.visible;
        series.title = options.title;
        series.color = options.color;
        series.point_radius = options.point_radius;
        series.point_markers = options.point_markers;
        series.point_symbol = options.point_symbol;
        series.line_width = options.line_width;
        series.line_style = options.line_style;
        series.interpolation = options.interpolation;
        series.connect_missing = options.connect_missing;
        series.fill_opacity = options.fill_opacity;
        series.polar = options.polar;
        series.fill_gradient = options.fill_gradient;
        series.baseline_value = options.baseline_value;
        series.baseline_policy = options.baseline_policy;
        series.data_labels = options.data_labels;
        series.bar_gap = options.bar_gap;
        series.bar_max_width = options.bar_max_width;
        series.bar_corner_radius = options.bar_corner_radius;
        series.heatmap_value_domain = options.heatmap_value_domain;
        series.heatmap_low_color = options.heatmap_low_color;
        series.heatmap_high_color = options.heatmap_high_color;
        series.box_fill_color = options.box_fill_color;
        series.box_median_color = options.box_median_color;
        series.group_id = options.group_id;
        series.stack_id = options.stack_id;
        series.stack_mode = options.stack_mode;
        series.histogram = options.histogram;
        if geometry_binding_changed {
            registry.scatter_spatial.get_mut().remove(&id);
        }
        self.invalidate_frame_all();
        Ok(())
    }

    #[doc(hidden)]
    pub fn general_series(&self, id: GeneralSeriesId) -> Option<&GeneralSeries> {
        self.general_series.as_ref()?.get(id)
    }

    #[doc(hidden)]
    pub fn general_series_pane_index(&self, id: GeneralSeriesId) -> Option<usize> {
        self.pane_index_for_id(self.general_series(id)?.pane_id)
    }

    #[doc(hidden)]
    pub fn general_series_count(&self) -> usize {
        self.general_series
            .as_ref()
            .map_or(0, GeneralSeriesRegistry::len)
    }

    #[doc(hidden)]
    pub fn general_series_ids_in_pane(&self, pane_index: usize) -> Vec<GeneralSeriesId> {
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return Vec::new();
        };
        self.general_series
            .as_ref()
            .into_iter()
            .flat_map(GeneralSeriesRegistry::iter)
            .filter(|series| series.pane_id == pane_id)
            .map(GeneralSeries::id)
            .collect()
    }

    #[doc(hidden)]
    pub fn general_series_order(&self, pane_index: Option<usize>) -> Vec<GeneralSeriesId> {
        let pane_id = match pane_index {
            Some(index) => match self.pane_stable_id(index) {
                Some(pane_id) => Some(pane_id),
                None => return Vec::new(),
            },
            None => None,
        };
        self.general_series
            .as_ref()
            .map_or_else(Vec::new, |registry| registry.ids(pane_id))
    }

    #[doc(hidden)]
    pub fn set_general_series_order(
        &mut self,
        pane_index: Option<usize>,
        ids: Vec<GeneralSeriesId>,
    ) -> bool {
        let pane_id = match pane_index {
            Some(index) => match self.pane_stable_id(index) {
                Some(pane_id) => Some(pane_id),
                None => return false,
            },
            None => None,
        };
        let accepted = match self.general_series.as_mut() {
            Some(registry) => registry.set_order(pane_id, &ids),
            None => ids.is_empty(),
        };
        if accepted {
            self.invalidate_frame_all();
        }
        accepted
    }

    #[doc(hidden)]
    pub fn remove_general_series(&mut self, id: GeneralSeriesId) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        if !registry.remove(id) {
            return false;
        }
        if registry.is_empty() {
            self.general_series = None;
        }
        self.prune_general_transitions();
        self.invalidate_frame_all();
        true
    }

    #[doc(hidden)]
    pub fn set_general_series_visible(&mut self, id: GeneralSeriesId, visible: bool) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        let Some(series) = registry.get_mut(id) else {
            return false;
        };
        if series.visible == visible {
            return true;
        }
        series.visible = visible;
        self.invalidate_frame_all();
        true
    }

    pub(crate) fn general_series_iter(&self) -> impl Iterator<Item = &GeneralSeries> {
        self.general_series
            .as_ref()
            .into_iter()
            .flat_map(GeneralSeriesRegistry::iter)
    }

    #[doc(hidden)]
    pub fn add_general_reference(
        &mut self,
        options: GeneralReferenceOptions,
    ) -> Result<GeneralReferenceId, ChartError> {
        self.validate_general_reference_definition(&options)?;
        let pane_id = self
            .pane_stable_id(options.pane())
            .expect("validated general reference pane remains present");
        let id = if let Some(registry) = self.general_series.as_mut() {
            registry.reference_insert(pane_id, options)?
        } else {
            let mut registry = GeneralSeriesRegistry::new();
            let id = registry.reference_insert(pane_id, options)?;
            self.general_series = Some(registry);
            id
        };
        self.invalidate_frame_all();
        Ok(id)
    }

    /// Validate a proposed reference against current axes without mutating the chart.
    pub fn validate_general_reference_definition(
        &self,
        options: &GeneralReferenceOptions,
    ) -> Result<(), ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane())
            .ok_or_else(|| invalid("general reference references a stale pane"))?;
        validate_general_reference_options(pane_id, options, |id| {
            let axis = self
                .general_axis(id)
                .ok_or_else(|| invalid(format!("general reference axis {id:?} is stale")))?;
            Ok((axis.pane_id(), axis.dimension(), axis.scale()))
        })
    }

    /// Validate a reference against the proposed axes for a pane before replacing live axes.
    pub fn validate_general_reference_with_axes(
        &self,
        options: &GeneralReferenceOptions,
        axes: &[crate::GeneralAxisOptions],
    ) -> Result<(), ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane())
            .ok_or_else(|| invalid("general reference references a stale pane"))?;
        for axis in axes {
            self.validate_general_axis_definition(axis)?;
        }
        validate_general_reference_options(pane_id, options, |id| {
            let axis = axes
                .iter()
                .find(|axis| axis.id == id)
                .ok_or_else(|| invalid(format!("general reference axis {id:?} is stale")))?;
            let axis_pane = self
                .pane_stable_id(axis.pane)
                .ok_or_else(|| invalid("general reference axis pane is stale"))?;
            Ok((axis_pane, axis.dimension, axis.scale))
        })
    }

    #[doc(hidden)]
    pub fn general_reference_options(
        &self,
        id: GeneralReferenceId,
    ) -> Option<GeneralReferenceOptions> {
        let reference = self.general_series.as_ref()?.reference_get(id)?;
        let pane = self.pane_index_for_id(reference.pane_id)?;
        let mut options = reference.options.clone();
        options.set_pane(pane);
        Some(options)
    }

    #[doc(hidden)]
    pub fn update_general_reference_options(
        &mut self,
        id: GeneralReferenceId,
        options: GeneralReferenceOptions,
    ) -> Result<(), ChartError> {
        if self
            .general_series
            .as_ref()
            .and_then(|registry| registry.reference_get(id))
            .is_none()
        {
            return Err(ChartError::new(
                ErrorCode::InvalidHandle,
                "general reference is stale",
            ));
        }
        self.validate_general_reference_definition(&options)?;
        let pane_id = self
            .pane_stable_id(options.pane())
            .expect("validated general reference pane remains present");
        let reference = self
            .general_series
            .as_mut()
            .and_then(|registry| registry.reference_get_mut(id))
            .expect("validated live general reference remains present");
        if reference.pane_id != pane_id || reference.options != options {
            reference.pane_id = pane_id;
            reference.options = options;
            self.invalidate_frame_all();
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn general_reference_ids(&self, pane: Option<usize>) -> Vec<GeneralReferenceId> {
        let pane_id = pane.and_then(|pane| self.pane_stable_id(pane));
        if pane.is_some() && pane_id.is_none() {
            return Vec::new();
        }
        self.general_series
            .as_ref()
            .into_iter()
            .flat_map(GeneralSeriesRegistry::reference_iter)
            .filter(|reference| pane_id.is_none_or(|pane_id| reference.pane_id == pane_id))
            .map(GeneralReference::id)
            .collect()
    }

    #[doc(hidden)]
    pub fn general_reference_count(&self) -> usize {
        self.general_series
            .as_ref()
            .map_or(0, |registry| registry.references.len())
    }

    #[doc(hidden)]
    pub fn remove_general_reference(&mut self, id: GeneralReferenceId) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        if !registry.reference_remove(id) {
            return false;
        }
        if registry.is_empty() {
            self.general_series = None;
        }
        self.invalidate_frame_all();
        true
    }

    pub(crate) fn general_reference_iter(&self) -> impl Iterator<Item = &GeneralReference> {
        self.general_series
            .as_ref()
            .into_iter()
            .flat_map(GeneralSeriesRegistry::reference_iter)
    }

    pub(crate) fn general_series_uses_axis(&self, id: &str) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.uses_axis(id))
    }

    pub(crate) fn general_series_uses_dataset(&self, id: GeneralDatasetId) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.uses_dataset(id))
    }

    pub(crate) fn general_series_uses_pane(&self, pane_id: PaneId) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.uses_pane(pane_id))
    }

    pub(crate) fn validate_general_dataset_replacement(
        &self,
        dataset_id: GeneralDatasetId,
        input: &GeneralXyInput,
    ) -> Result<(), ChartError> {
        let Some(registry) = self.general_series.as_ref() else {
            return Ok(());
        };
        for series in registry
            .series
            .iter()
            .filter(|series| series.dataset == dataset_id)
        {
            let x_axis = self
                .general_axis(&series.x_axis_id)
                .ok_or_else(|| invalid("bound general series X axis is stale"))?;
            let y_axis = self
                .general_axis(&series.y_axis_id)
                .ok_or_else(|| invalid("bound general series Y axis is stale"))?;
            validate_input_for_series(series.kind, input, x_axis, y_axis)?;
        }
        Ok(())
    }

    pub(crate) fn visit_general_columns<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralColumnGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::Column {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let (Some(categories), Some(category_indices)) =
            (dataset.categories(), dataset.category_indices())
        else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric([y_from, y_to])) =
            self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(x_scale) = BandScale::new(
            axis_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Ok(y_scale) = LinearScale::new(y_from, y_to, y_range.0, y_range.1) else {
            return;
        };
        let axis_lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();

        let (slot_index, slot_count) = self.general_column_group_slot(series);
        let mut stack_base = series
            .stack_id
            .is_some()
            .then(|| vec![(0.0_f64, 0.0_f64); axis_categories.len()]);
        let mut stack_totals = (matches!(
            series.stack_mode,
            GeneralStackMode::Percent | GeneralStackMode::Cumulative | GeneralStackMode::Silhouette
        ) || series.bar_corner_radius > 0.0)
            .then(|| vec![(0.0_f64, 0.0_f64); axis_categories.len()]);
        let mut stack_last = (matches!(
            series.stack_mode,
            GeneralStackMode::Cumulative | GeneralStackMode::Silhouette | GeneralStackMode::Wiggle
        ))
        .then(|| vec![None; axis_categories.len()]);
        if series.stack_id.is_some()
            && let Some(registry) = self.general_series.as_ref()
        {
            if let Some(totals) = stack_totals.as_mut() {
                for sibling in registry.series.iter().filter(|candidate| {
                    candidate.visible && column_stack_matches(series, candidate)
                }) {
                    accumulate_column_values(
                        self,
                        sibling,
                        &axis_lookup,
                        |axis_index, row, value| {
                            if value >= 0.0 {
                                totals[axis_index].0 += value;
                            } else {
                                totals[axis_index].1 += -value;
                            }
                            if value != 0.0
                                && let Some(last) = stack_last.as_mut()
                            {
                                last[axis_index] = Some((sibling.id, row));
                            }
                        },
                    );
                }
            }
            for sibling in registry
                .series
                .iter()
                .filter(|candidate| candidate.visible && column_stack_matches(series, candidate))
            {
                if sibling.id == series.id {
                    break;
                }
                let Some(base) = stack_base.as_mut() else {
                    break;
                };
                accumulate_column_values(self, sibling, &axis_lookup, |axis_index, _, value| {
                    if value >= 0.0
                        || matches!(
                            series.stack_mode,
                            GeneralStackMode::Cumulative
                                | GeneralStackMode::Silhouette
                                | GeneralStackMode::Wiggle
                        )
                    {
                        base[axis_index].0 += value;
                    } else if series.stack_mode != GeneralStackMode::Positive {
                        base[axis_index].1 += value;
                    }
                });
            }
            if series.stack_mode == GeneralStackMode::Percent
                && let (Some(base), Some(totals)) = (stack_base.as_mut(), stack_totals.as_ref())
            {
                for (base, total) in base.iter_mut().zip(totals) {
                    if total.0 > 0.0 {
                        base.0 /= total.0;
                    }
                    if total.1 > 0.0 {
                        base.1 /= total.1;
                    }
                }
            }
            if series.stack_mode == GeneralStackMode::Silhouette
                && let (Some(base), Some(totals)) = (stack_base.as_mut(), stack_totals.as_ref())
            {
                for (base, total) in base.iter_mut().zip(totals) {
                    base.0 -= (total.0 - total.1) * 0.5;
                }
            }
            if series.stack_mode == GeneralStackMode::Wiggle
                && let Some(base) = stack_base.as_mut()
            {
                let layers =
                    self.general_wiggle_category_layers(series, &axis_lookup, column_stack_matches);
                for (base, baseline) in base.iter_mut().zip(wiggle_baselines(&layers)) {
                    base.0 += baseline;
                }
            }
        }

        for (row, &category_index) in category_indices.iter().enumerate() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let Some(category) = usize::try_from(category_index)
                .ok()
                .and_then(|index| categories.get(index))
            else {
                continue;
            };
            let Some(&axis_index) = axis_lookup.get(category.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(x_axis, category, x_range.0, x_range.1)
                .or_else(|| x_scale.bounds(axis_index))
            else {
                continue;
            };
            let (left, right) = self
                .category_row_motion_bounds(x_axis, series.dataset, row, bounds, x_range)
                .unwrap_or(bounds);
            let full_left = left.min(right).clamp(0.0, plot.width);
            let full_right = left.max(right).clamp(0.0, plot.width);
            let slot_width = (full_right - full_left) / slot_count.max(1) as f64;
            let slot_left = full_left + slot_width * slot_index as f64;
            let slot_right = if slot_index + 1 == slot_count {
                full_right
            } else {
                slot_left + slot_width
            };
            let Some((left, right)) =
                bar_thickness_bounds(slot_left, slot_right, series.bar_gap, series.bar_max_width)
            else {
                continue;
            };
            let raw_value = dataset.y()[row];
            if series.stack_mode == GeneralStackMode::Positive && raw_value < 0.0 {
                continue;
            }
            let (from_value, to_value) = if series.stack_id.is_some() {
                let normalized_value = match stack_totals.as_ref() {
                    Some(totals)
                        if series.stack_mode == GeneralStackMode::Percent && raw_value >= 0.0 =>
                    {
                        let total = totals[axis_index].0;
                        if total > 0.0 { raw_value / total } else { 0.0 }
                    }
                    Some(totals) if series.stack_mode == GeneralStackMode::Percent => {
                        let total = totals[axis_index].1;
                        if total > 0.0 { raw_value / total } else { 0.0 }
                    }
                    _ => raw_value,
                };
                let Some(stack_base) = stack_base.as_mut() else {
                    continue;
                };
                let base = if raw_value >= 0.0
                    || matches!(
                        series.stack_mode,
                        GeneralStackMode::Cumulative
                            | GeneralStackMode::Silhouette
                            | GeneralStackMode::Wiggle
                    ) {
                    &mut stack_base[axis_index].0
                } else {
                    &mut stack_base[axis_index].1
                };
                let from = *base;
                let to = from + normalized_value;
                *base = to;
                (from, to)
            } else {
                (0.0, raw_value)
            };
            let Some(from_y) = y_scale.coordinate_clamped(from_value) else {
                continue;
            };
            let Some(to_y) = y_scale.coordinate_clamped(to_value) else {
                continue;
            };
            let top = from_y.min(to_y).clamp(plot.y, plot_bottom);
            let bottom = from_y.max(to_y).clamp(plot.y, plot_bottom);
            if right <= left || bottom <= top {
                continue;
            }
            visit(GeneralColumnGeometry {
                row,
                left,
                right,
                top,
                bottom,
                end_at_min: to_y < from_y,
                exposed_end: if matches!(
                    series.stack_mode,
                    GeneralStackMode::Cumulative
                        | GeneralStackMode::Silhouette
                        | GeneralStackMode::Wiggle
                ) {
                    stack_last.as_ref().and_then(|last| last[axis_index]) == Some((series.id, row))
                } else {
                    bar_end_exposed(
                        series,
                        stack_totals.as_deref(),
                        axis_index,
                        raw_value,
                        to_value,
                    )
                },
            });
        }
    }

    /// Resolve the bins of a histogram series, rebinning only when its dataset generation or
    /// binning policy changed since the cached result.
    pub(crate) fn general_histogram_bins(
        &self,
        series: &GeneralSeries,
    ) -> Option<Arc<GeneralHistogramBins>> {
        if series.kind != GeneralSeriesKind::Histogram {
            return None;
        }
        let registry = self.general_series.as_ref()?;
        let dataset = self.general_dataset(series.dataset)?;
        let current = |entry: &&GeneralHistogramCacheEntry| {
            entry.dataset == series.dataset
                && entry.generation == dataset.generation()
                && entry.options == series.histogram
        };
        if let Some(entry) = registry
            .histogram_bins
            .borrow()
            .get(&series.id)
            .filter(current)
        {
            return Some(Arc::clone(&entry.bins));
        }
        let bins = Arc::new(compute_histogram_bins(
            &series.histogram,
            dataset.y(),
            |row| dataset.y_is_valid(row),
        ));
        registry.histogram_bins.borrow_mut().insert(
            series.id,
            GeneralHistogramCacheEntry {
                dataset: series.dataset,
                generation: dataset.generation(),
                options: series.histogram,
                bins: Arc::clone(&bins),
            },
        );
        Some(bins)
    }

    pub(crate) fn visit_general_histogram_bins<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralColumnGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::Histogram {
            return;
        }
        let Some(bins) = self.general_histogram_bins(series) else {
            return;
        };
        let Some(plot) = self
            .pane_index_for_id(series.pane_id)
            .and_then(|pane_index| self.general_plot_rect(pane_index))
        else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let (
            Some(GeneralAxisDomain::Numeric([x_from, x_to])),
            Some(GeneralAxisDomain::Numeric([y_from, y_to])),
        ) = (
            self.effective_general_axis_domain(x_axis),
            self.effective_general_axis_domain(y_axis),
        )
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let (Ok(x_scale), Ok(y_scale)) = (
            LinearScale::new(x_from, x_to, x_range.0, x_range.1),
            LinearScale::new(y_from, y_to, y_range.0, y_range.1),
        ) else {
            return;
        };
        let Some(base_y) = y_scale.coordinate_clamped(0.0) else {
            return;
        };
        for (bin, &height) in bins.heights().iter().enumerate() {
            let Some((from, to)) = bins.bounds(bin) else {
                continue;
            };
            let (Some(from_x), Some(to_x), Some(value_y)) = (
                x_scale.coordinate_clamped(from),
                x_scale.coordinate_clamped(to),
                y_scale.coordinate_clamped(height),
            ) else {
                continue;
            };
            let Some((left, right)) = bar_thickness_bounds(
                from_x.min(to_x),
                from_x.max(to_x),
                series.bar_gap,
                series.bar_max_width,
            ) else {
                continue;
            };
            let left = left.clamp(0.0, plot.width);
            let right = right.clamp(0.0, plot.width);
            let top = base_y.min(value_y).clamp(plot.y, plot_bottom);
            let bottom = base_y.max(value_y).clamp(plot.y, plot_bottom);
            if right <= left || bottom <= top {
                continue;
            }
            visit(GeneralColumnGeometry {
                row: bin,
                left,
                right,
                top,
                bottom,
                end_at_min: value_y < base_y,
                exposed_end: true,
            });
        }
    }

    fn validate_general_histogram_samples(
        &self,
        dataset_id: GeneralDatasetId,
        values: &[f64],
        valid: impl Fn(usize) -> bool,
    ) -> Result<(), ChartError> {
        let Some(registry) = self.general_series.as_ref() else {
            return Ok(());
        };
        for series in registry.series.iter().filter(|series| {
            series.dataset == dataset_id && series.kind == GeneralSeriesKind::Histogram
        }) {
            validate_histogram_samples(&series.histogram, values, &valid)?;
        }
        Ok(())
    }

    /// Reject replacement samples that would leave a bound histogram unbinnable.
    pub(crate) fn validate_general_histogram_replacement(
        &self,
        dataset_id: GeneralDatasetId,
        input: &GeneralXyInput,
    ) -> Result<(), ChartError> {
        let valid = input.y_valid_values();
        self.validate_general_histogram_samples(dataset_id, input.y_values(), |row| {
            valid.is_none_or(|flags| flags.get(row).is_some_and(|flag| *flag != 0))
        })
    }

    /// Reject an upsert whose merged samples would leave a bound histogram unbinnable.
    pub(crate) fn validate_general_histogram_upsert(
        &self,
        dataset_id: GeneralDatasetId,
        input: &GeneralXyInput,
        max_rows: Option<usize>,
    ) -> Result<(), ChartError> {
        let bound = self.general_series.as_ref().is_some_and(|registry| {
            registry.series.iter().any(|series| {
                series.dataset == dataset_id && series.kind == GeneralSeriesKind::Histogram
            })
        });
        if !bound {
            return Ok(());
        }
        let store = self.general_data.as_ref().ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general dataset handle is stale")
        })?;
        let preview = store.preview_upsert(dataset_id, input.clone(), max_rows)?;
        self.validate_general_histogram_samples(dataset_id, preview.y(), |row| {
            preview.y_is_valid(row)
        })
    }
    pub(crate) fn visit_general_horizontal_bars<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralColumnGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::HorizontalBar {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let (Some(categories), Some(category_indices)) =
            (dataset.categories(), dataset.category_indices())
        else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric([x_from, x_to])) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(x_scale) = LinearScale::new(x_from, x_to, x_range.0, x_range.1) else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Ok(y_scale) = BandScale::new(
            axis_categories.len(),
            y_range.0,
            y_range.1,
            y_axis.band_padding_inner(),
            y_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let axis_lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();

        let (slot_index, slot_count) = self.general_horizontal_bar_group_slot(series);
        let mut stack_base = series
            .stack_id
            .is_some()
            .then(|| vec![(0.0_f64, 0.0_f64); axis_categories.len()]);
        let mut stack_totals = (matches!(
            series.stack_mode,
            GeneralStackMode::Percent | GeneralStackMode::Cumulative | GeneralStackMode::Silhouette
        ) || series.bar_corner_radius > 0.0)
            .then(|| vec![(0.0_f64, 0.0_f64); axis_categories.len()]);
        let mut stack_last = (matches!(
            series.stack_mode,
            GeneralStackMode::Cumulative | GeneralStackMode::Silhouette | GeneralStackMode::Wiggle
        ))
        .then(|| vec![None; axis_categories.len()]);
        if series.stack_id.is_some()
            && let Some(registry) = self.general_series.as_ref()
        {
            if let Some(totals) = stack_totals.as_mut() {
                for sibling in registry.series.iter().filter(|candidate| {
                    candidate.visible && horizontal_bar_stack_matches(series, candidate)
                }) {
                    accumulate_column_values(
                        self,
                        sibling,
                        &axis_lookup,
                        |axis_index, row, value| {
                            if value >= 0.0 {
                                totals[axis_index].0 += value;
                            } else {
                                totals[axis_index].1 += -value;
                            }
                            if value != 0.0
                                && let Some(last) = stack_last.as_mut()
                            {
                                last[axis_index] = Some((sibling.id, row));
                            }
                        },
                    );
                }
            }
            for sibling in registry.series.iter().filter(|candidate| {
                candidate.visible && horizontal_bar_stack_matches(series, candidate)
            }) {
                if sibling.id == series.id {
                    break;
                }
                let Some(base) = stack_base.as_mut() else {
                    break;
                };
                accumulate_column_values(self, sibling, &axis_lookup, |axis_index, _, value| {
                    if value >= 0.0
                        || matches!(
                            series.stack_mode,
                            GeneralStackMode::Cumulative
                                | GeneralStackMode::Silhouette
                                | GeneralStackMode::Wiggle
                        )
                    {
                        base[axis_index].0 += value;
                    } else if series.stack_mode != GeneralStackMode::Positive {
                        base[axis_index].1 += value;
                    }
                });
            }
            if series.stack_mode == GeneralStackMode::Percent
                && let (Some(base), Some(totals)) = (stack_base.as_mut(), stack_totals.as_ref())
            {
                for (base, total) in base.iter_mut().zip(totals) {
                    if total.0 > 0.0 {
                        base.0 /= total.0;
                    }
                    if total.1 > 0.0 {
                        base.1 /= total.1;
                    }
                }
            }
            if series.stack_mode == GeneralStackMode::Silhouette
                && let (Some(base), Some(totals)) = (stack_base.as_mut(), stack_totals.as_ref())
            {
                for (base, total) in base.iter_mut().zip(totals) {
                    base.0 -= (total.0 - total.1) * 0.5;
                }
            }
            if series.stack_mode == GeneralStackMode::Wiggle
                && let Some(base) = stack_base.as_mut()
            {
                let layers = self.general_wiggle_category_layers(
                    series,
                    &axis_lookup,
                    horizontal_bar_stack_matches,
                );
                for (base, baseline) in base.iter_mut().zip(wiggle_baselines(&layers)) {
                    base.0 += baseline;
                }
            }
        }

        for (row, &category_index) in category_indices.iter().enumerate() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let Some(category) = usize::try_from(category_index)
                .ok()
                .and_then(|index| categories.get(index))
            else {
                continue;
            };
            let Some(&axis_index) = axis_lookup.get(category.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(y_axis, category, y_range.0, y_range.1)
                .or_else(|| y_scale.bounds(axis_index))
            else {
                continue;
            };
            let (from_y, to_y) = self
                .category_row_motion_bounds(y_axis, series.dataset, row, bounds, y_range)
                .unwrap_or(bounds);
            let full_top = from_y.min(to_y).clamp(plot.y, plot_bottom);
            let full_bottom = from_y.max(to_y).clamp(plot.y, plot_bottom);
            let slot_height = (full_bottom - full_top) / slot_count.max(1) as f64;
            let slot_top = full_top + slot_height * slot_index as f64;
            let slot_bottom = if slot_index + 1 == slot_count {
                full_bottom
            } else {
                slot_top + slot_height
            };
            let Some((top, bottom)) =
                bar_thickness_bounds(slot_top, slot_bottom, series.bar_gap, series.bar_max_width)
            else {
                continue;
            };
            let raw_value = dataset.y()[row];
            if series.stack_mode == GeneralStackMode::Positive && raw_value < 0.0 {
                continue;
            }
            let (from_value, to_value) = if series.stack_id.is_some() {
                let normalized_value = match stack_totals.as_ref() {
                    Some(totals)
                        if series.stack_mode == GeneralStackMode::Percent && raw_value >= 0.0 =>
                    {
                        let total = totals[axis_index].0;
                        if total > 0.0 { raw_value / total } else { 0.0 }
                    }
                    Some(totals) if series.stack_mode == GeneralStackMode::Percent => {
                        let total = totals[axis_index].1;
                        if total > 0.0 { raw_value / total } else { 0.0 }
                    }
                    _ => raw_value,
                };
                let Some(stack_base) = stack_base.as_mut() else {
                    continue;
                };
                let base = if raw_value >= 0.0
                    || matches!(
                        series.stack_mode,
                        GeneralStackMode::Cumulative
                            | GeneralStackMode::Silhouette
                            | GeneralStackMode::Wiggle
                    ) {
                    &mut stack_base[axis_index].0
                } else {
                    &mut stack_base[axis_index].1
                };
                let from = *base;
                let to = from + normalized_value;
                *base = to;
                (from, to)
            } else {
                (0.0, raw_value)
            };
            let Some(from_x) = x_scale.coordinate_clamped(from_value) else {
                continue;
            };
            let Some(to_x) = x_scale.coordinate_clamped(to_value) else {
                continue;
            };
            let left = from_x.min(to_x).clamp(0.0, plot.width);
            let right = from_x.max(to_x).clamp(0.0, plot.width);
            if right <= left || bottom <= top {
                continue;
            }
            visit(GeneralColumnGeometry {
                row,
                left,
                right,
                top,
                bottom,
                end_at_min: to_x < from_x,
                exposed_end: if matches!(
                    series.stack_mode,
                    GeneralStackMode::Cumulative
                        | GeneralStackMode::Silhouette
                        | GeneralStackMode::Wiggle
                ) {
                    stack_last.as_ref().and_then(|last| last[axis_index]) == Some((series.id, row))
                } else {
                    bar_end_exposed(
                        series,
                        stack_totals.as_deref(),
                        axis_index,
                        raw_value,
                        to_value,
                    )
                },
            });
        }
    }

    pub(crate) fn visit_general_range_bars<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralColumnGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::RangeBar {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let (Some(categories), Some(category_indices), Some(low)) = (
            dataset.categories(),
            dataset.category_indices(),
            dataset.low(),
        ) else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric([y_from, y_to])) =
            self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(x_scale) = BandScale::new(
            axis_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) =
            NumericAxisScale::new(y_axis.scale(), [y_from, y_to], y_range.0, y_range.1)
        else {
            return;
        };
        let axis_lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();
        for row in 0..dataset.len() {
            if !dataset.low_is_valid(row) || !dataset.y_is_valid(row) || low[row] > dataset.y()[row]
            {
                continue;
            }
            let Some(category) = usize::try_from(category_indices[row])
                .ok()
                .and_then(|i| categories.get(i))
            else {
                continue;
            };
            let Some(&axis_index) = axis_lookup.get(category.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(x_axis, category, x_range.0, x_range.1)
                .or_else(|| x_scale.bounds(axis_index))
            else {
                continue;
            };
            let (left, right) = self
                .category_row_motion_bounds(x_axis, series.dataset, row, bounds, x_range)
                .unwrap_or(bounds);
            let Some((bar_left, bar_right)) = bar_thickness_bounds(
                left.min(right).clamp(0.0, plot.width),
                left.max(right).clamp(0.0, plot.width),
                series.bar_gap,
                series.bar_max_width,
            ) else {
                continue;
            };
            let Some(low_y) = y_scale.coordinate(low[row]) else {
                continue;
            };
            let Some(high_y) = y_scale.coordinate(dataset.y()[row]) else {
                continue;
            };
            let top = low_y.min(high_y).clamp(plot.y, plot_bottom);
            let bottom = low_y.max(high_y).clamp(plot.y, plot_bottom);
            if bar_right > bar_left && bottom > top {
                visit(GeneralColumnGeometry {
                    row,
                    left: bar_left,
                    right: bar_right,
                    top,
                    bottom,
                    end_at_min: false,
                    exposed_end: true,
                });
            }
        }
    }

    fn general_horizontal_bar_group_slot(&self, series: &GeneralSeries) -> (usize, usize) {
        let Some(group_id) = series.group_id() else {
            return (0, 1);
        };
        let Some(registry) = self.general_series.as_ref() else {
            return (0, 1);
        };
        let mut target_slot = 0usize;
        let mut slot_count = 0usize;
        for (index, sibling) in registry.series.iter().enumerate() {
            if !sibling.visible
                || sibling.kind != GeneralSeriesKind::HorizontalBar
                || sibling.pane_id != series.pane_id
                || sibling.y_axis_id != series.y_axis_id
                || sibling.group_id() != Some(group_id)
            {
                continue;
            }
            let representative = sibling.stack_id().is_none()
                || !registry.series[..index].iter().any(|previous| {
                    previous.visible
                        && previous.kind == GeneralSeriesKind::HorizontalBar
                        && previous.pane_id == series.pane_id
                        && previous.y_axis_id == series.y_axis_id
                        && previous.group_id() == Some(group_id)
                        && previous.stack_id() == sibling.stack_id()
                });
            if !representative {
                continue;
            }
            if sibling.id == series.id
                || (series.stack_id().is_some() && sibling.stack_id() == series.stack_id())
            {
                target_slot = slot_count;
            }
            slot_count += 1;
        }
        (target_slot, slot_count.max(1))
    }

    fn general_column_group_slot(&self, series: &GeneralSeries) -> (usize, usize) {
        let Some(group_id) = series.group_id() else {
            return (0, 1);
        };
        let Some(registry) = self.general_series.as_ref() else {
            return (0, 1);
        };
        let mut target_slot = 0usize;
        let mut slot_count = 0usize;
        for (index, sibling) in registry.series.iter().enumerate() {
            if !sibling.visible
                || sibling.kind != GeneralSeriesKind::Column
                || sibling.pane_id != series.pane_id
                || sibling.x_axis_id != series.x_axis_id
                || sibling.group_id() != Some(group_id)
            {
                continue;
            }
            let representative = sibling.stack_id().is_none()
                || !registry.series[..index].iter().any(|previous| {
                    previous.visible
                        && previous.kind == GeneralSeriesKind::Column
                        && previous.pane_id == series.pane_id
                        && previous.x_axis_id == series.x_axis_id
                        && previous.group_id() == Some(group_id)
                        && previous.stack_id() == sibling.stack_id()
                });
            if !representative {
                continue;
            }
            if sibling.id == series.id
                || (series.stack_id().is_some() && sibling.stack_id() == series.stack_id())
            {
                target_slot = slot_count;
            }
            slot_count += 1;
        }
        (target_slot, slot_count.max(1))
    }

    fn general_wiggle_category_layers(
        &self,
        reference: &GeneralSeries,
        axis_lookup: &HashMap<&str, usize>,
        matches: fn(&GeneralSeries, &GeneralSeries) -> bool,
    ) -> Vec<Vec<f64>> {
        let Some(registry) = self.general_series.as_ref() else {
            return Vec::new();
        };
        registry
            .series
            .iter()
            .filter(|candidate| candidate.visible && matches(reference, candidate))
            .map(|sibling| {
                let mut values = vec![0.0; axis_lookup.len()];
                accumulate_column_values(self, sibling, axis_lookup, |index, _, value| {
                    values[index] += value;
                });
                values
            })
            .collect()
    }

    fn general_wiggle_category_bounds(
        &self,
        reference: &GeneralSeries,
        category_axis_id: &str,
        matches: fn(&GeneralSeries, &GeneralSeries) -> bool,
        bounds: &mut Option<(f64, f64)>,
    ) {
        let Some(GeneralAxisDomain::Category(categories)) = self
            .general_axis(category_axis_id)
            .and_then(|axis| self.effective_general_axis_domain(axis))
        else {
            return;
        };
        let lookup: HashMap<&str, usize> = categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();
        let layers = self.general_wiggle_category_layers(reference, &lookup, matches);
        let mut prefixes = wiggle_baselines(&layers);
        for &baseline in &prefixes {
            extend_numeric_pair(bounds, baseline);
        }
        if let Some(registry) = self.general_series.as_ref() {
            for sibling in registry
                .series
                .iter()
                .filter(|candidate| candidate.visible && matches(reference, candidate))
            {
                accumulate_column_values(self, sibling, &lookup, |index, _, value| {
                    prefixes[index] += value;
                    extend_numeric_pair(bounds, prefixes[index]);
                });
            }
        }
    }

    fn general_wiggle_area_layers(
        &self,
        reference: &GeneralSeries,
    ) -> (Vec<GeneralStackXKey>, Vec<Vec<f64>>) {
        let Some(registry) = self.general_series.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let mut keys = HashSet::new();
        let mut sparse_layers = Vec::new();
        for sibling in registry
            .series
            .iter()
            .filter(|candidate| candidate.visible && area_stack_matches(reference, candidate))
        {
            let mut values = HashMap::new();
            accumulate_area_values(self, sibling, |x, value| {
                keys.insert(x.clone());
                *values.entry(x).or_insert(0.0) += value;
            });
            sparse_layers.push(values);
        }
        let category_order: HashMap<String, usize> = self
            .general_axis(&reference.x_axis_id)
            .and_then(|axis| self.effective_general_axis_domain(axis))
            .and_then(|domain| match domain {
                GeneralAxisDomain::Category(categories) => Some(categories),
                _ => None,
            })
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, category)| (category, index))
            .collect();
        let mut keys: Vec<_> = keys.into_iter().collect();
        keys.sort_by(|left, right| match (left, right) {
            (GeneralStackXKey::Numeric(a), GeneralStackXKey::Numeric(b)) => {
                f64::from_bits(*a).total_cmp(&f64::from_bits(*b))
            }
            (GeneralStackXKey::Temporal(a), GeneralStackXKey::Temporal(b)) => a.cmp(b),
            (GeneralStackXKey::Category(a), GeneralStackXKey::Category(b)) => category_order
                .get(a)
                .cmp(&category_order.get(b))
                .then_with(|| a.cmp(b)),
            _ => std::cmp::Ordering::Equal,
        });
        let layers = sparse_layers
            .into_iter()
            .map(|values: HashMap<GeneralStackXKey, f64>| {
                keys.iter()
                    .map(|key| values.get(key).copied().unwrap_or(0.0))
                    .collect()
            })
            .collect();
        (keys, layers)
    }

    pub(crate) fn general_column_axis_bounds(
        &self,
        pane_id: PaneId,
        y_axis_id: &str,
    ) -> Option<(f64, f64)> {
        let registry = self.general_series.as_ref()?;
        let mut bounds: Option<(f64, f64)> = None;
        let mut processed_stacks = HashSet::new();

        for series in registry.series.iter().filter(|series| {
            series.visible
                && series.kind == GeneralSeriesKind::Column
                && series.pane_id == pane_id
                && series.y_axis_id == y_axis_id
        }) {
            if let Some(stack_id) = series.stack_id() {
                let key = (
                    series.x_axis_id.clone(),
                    series.group_id.clone(),
                    stack_id.to_owned(),
                    series.stack_mode,
                );
                if !processed_stacks.insert(key) {
                    continue;
                }
                if series.stack_mode == GeneralStackMode::Wiggle {
                    self.general_wiggle_category_bounds(
                        series,
                        &series.x_axis_id,
                        column_stack_matches,
                        &mut bounds,
                    );
                    continue;
                }
                let mut category_totals: HashMap<String, (f64, f64)> = HashMap::new();
                let mut silhouette_prefixes: HashMap<String, (f64, f64)> = HashMap::new();
                for sibling in registry.series.iter().filter(|candidate| {
                    candidate.visible
                        && candidate.y_axis_id == y_axis_id
                        && column_stack_matches(series, candidate)
                }) {
                    let Some(dataset) = self.general_geometry_dataset(sibling.dataset) else {
                        continue;
                    };
                    let (Some(categories), Some(indices)) =
                        (dataset.categories(), dataset.category_indices())
                    else {
                        continue;
                    };
                    for (row, &category_index) in indices.iter().enumerate() {
                        if !dataset.y_is_valid(row) {
                            continue;
                        }
                        let Some(category) = usize::try_from(category_index)
                            .ok()
                            .and_then(|index| categories.get(index))
                        else {
                            continue;
                        };
                        let value = dataset.y()[row];
                        let totals = category_totals.entry(category.clone()).or_default();
                        if series.stack_mode == GeneralStackMode::Cumulative {
                            totals.0 += value;
                            extend_numeric_pair(&mut bounds, totals.0);
                        } else if series.stack_mode == GeneralStackMode::Silhouette {
                            totals.0 += value;
                            let prefix = silhouette_prefixes.entry(category.clone()).or_default();
                            prefix.0 = prefix.0.min(totals.0);
                            prefix.1 = prefix.1.max(totals.0);
                        } else if value >= 0.0 {
                            totals.0 += value;
                        } else {
                            totals.1 += value;
                        }
                    }
                }
                for (category, (positive, negative)) in category_totals {
                    match series.stack_mode {
                        GeneralStackMode::Normal => {
                            extend_numeric_pair(&mut bounds, positive);
                            extend_numeric_pair(&mut bounds, negative);
                        }
                        GeneralStackMode::Percent => {
                            if positive > 0.0 {
                                extend_numeric_pair(&mut bounds, 1.0);
                            }
                            if negative < 0.0 {
                                extend_numeric_pair(&mut bounds, -1.0);
                            }
                        }
                        GeneralStackMode::Positive => extend_numeric_pair(&mut bounds, positive),
                        GeneralStackMode::Cumulative => {}
                        GeneralStackMode::Silhouette => {
                            let (min, max) = silhouette_prefixes[&category];
                            extend_numeric_pair(&mut bounds, min - positive * 0.5);
                            extend_numeric_pair(&mut bounds, max - positive * 0.5);
                        }
                        GeneralStackMode::Wiggle => unreachable!("wiggle bounds handled above"),
                    }
                }
            } else if let Some(dataset) = self.general_geometry_dataset(series.dataset) {
                for (row, &value) in dataset.y().iter().enumerate() {
                    if dataset.y_is_valid(row) {
                        extend_numeric_pair(&mut bounds, value);
                    }
                }
            }
        }
        if bounds.is_some() {
            extend_numeric_pair(&mut bounds, 0.0);
        }
        bounds
    }

    pub(crate) fn general_horizontal_bar_axis_bounds(
        &self,
        pane_id: PaneId,
        x_axis_id: &str,
    ) -> Option<(f64, f64)> {
        let registry = self.general_series.as_ref()?;
        let mut bounds: Option<(f64, f64)> = None;
        let mut processed_stacks = HashSet::new();

        for series in registry.series.iter().filter(|series| {
            series.visible
                && series.kind == GeneralSeriesKind::HorizontalBar
                && series.pane_id == pane_id
                && series.x_axis_id == x_axis_id
        }) {
            if let Some(stack_id) = series.stack_id() {
                let key = (
                    series.y_axis_id.clone(),
                    series.group_id.clone(),
                    stack_id.to_owned(),
                    series.stack_mode,
                );
                if !processed_stacks.insert(key) {
                    continue;
                }
                if series.stack_mode == GeneralStackMode::Wiggle {
                    self.general_wiggle_category_bounds(
                        series,
                        &series.y_axis_id,
                        horizontal_bar_stack_matches,
                        &mut bounds,
                    );
                    continue;
                }
                let mut category_totals: HashMap<String, (f64, f64)> = HashMap::new();
                let mut silhouette_prefixes: HashMap<String, (f64, f64)> = HashMap::new();
                for sibling in registry.series.iter().filter(|candidate| {
                    candidate.visible
                        && candidate.x_axis_id == x_axis_id
                        && horizontal_bar_stack_matches(series, candidate)
                }) {
                    let Some(dataset) = self.general_geometry_dataset(sibling.dataset) else {
                        continue;
                    };
                    let (Some(categories), Some(indices)) =
                        (dataset.categories(), dataset.category_indices())
                    else {
                        continue;
                    };
                    for (row, &category_index) in indices.iter().enumerate() {
                        if !dataset.y_is_valid(row) {
                            continue;
                        }
                        let Some(category) = usize::try_from(category_index)
                            .ok()
                            .and_then(|index| categories.get(index))
                        else {
                            continue;
                        };
                        let value = dataset.y()[row];
                        let totals = category_totals.entry(category.clone()).or_default();
                        if series.stack_mode == GeneralStackMode::Cumulative {
                            totals.0 += value;
                            extend_numeric_pair(&mut bounds, totals.0);
                        } else if series.stack_mode == GeneralStackMode::Silhouette {
                            totals.0 += value;
                            let prefix = silhouette_prefixes.entry(category.clone()).or_default();
                            prefix.0 = prefix.0.min(totals.0);
                            prefix.1 = prefix.1.max(totals.0);
                        } else if value >= 0.0 {
                            totals.0 += value;
                        } else {
                            totals.1 += value;
                        }
                    }
                }
                for (category, (positive, negative)) in category_totals {
                    match series.stack_mode {
                        GeneralStackMode::Normal => {
                            extend_numeric_pair(&mut bounds, positive);
                            extend_numeric_pair(&mut bounds, negative);
                        }
                        GeneralStackMode::Percent => {
                            if positive > 0.0 {
                                extend_numeric_pair(&mut bounds, 1.0);
                            }
                            if negative < 0.0 {
                                extend_numeric_pair(&mut bounds, -1.0);
                            }
                        }
                        GeneralStackMode::Positive => extend_numeric_pair(&mut bounds, positive),
                        GeneralStackMode::Cumulative => {}
                        GeneralStackMode::Silhouette => {
                            let (min, max) = silhouette_prefixes[&category];
                            extend_numeric_pair(&mut bounds, min - positive * 0.5);
                            extend_numeric_pair(&mut bounds, max - positive * 0.5);
                        }
                        GeneralStackMode::Wiggle => unreachable!("wiggle bounds handled above"),
                    }
                }
            } else if let Some(dataset) = self.general_geometry_dataset(series.dataset) {
                for (row, &value) in dataset.y().iter().enumerate() {
                    if dataset.y_is_valid(row) {
                        extend_numeric_pair(&mut bounds, value);
                    }
                }
            }
        }
        if bounds.is_some() {
            extend_numeric_pair(&mut bounds, 0.0);
        }
        bounds
    }

    pub(crate) fn general_area_stack_axis_bounds(
        &self,
        pane_id: PaneId,
        y_axis_id: &str,
    ) -> Option<(f64, f64)> {
        let registry = self.general_series.as_ref()?;
        let mut bounds: Option<(f64, f64)> = None;
        let mut processed_stacks = HashSet::new();

        for series in registry.series.iter().filter(|series| {
            series.visible
                && series.kind == GeneralSeriesKind::XyArea
                && series.pane_id == pane_id
                && series.y_axis_id == y_axis_id
                && series.stack_id.is_some()
        }) {
            let key = (
                series.x_axis_id.clone(),
                series.stack_id.clone().expect("stacked area stack ID"),
                series.stack_mode,
            );
            if !processed_stacks.insert(key) {
                continue;
            }
            if series.stack_mode == GeneralStackMode::Wiggle {
                let (keys, layers) = self.general_wiggle_area_layers(series);
                let baselines = wiggle_baselines(&layers);
                let mut prefixes: HashMap<_, _> = keys.into_iter().zip(baselines).collect();
                for &baseline in prefixes.values() {
                    extend_numeric_pair(&mut bounds, baseline);
                }
                for sibling in registry
                    .series
                    .iter()
                    .filter(|candidate| candidate.visible && area_stack_matches(series, candidate))
                {
                    accumulate_area_values(self, sibling, |x, value| {
                        if let Some(prefix) = prefixes.get_mut(&x) {
                            *prefix += value;
                            extend_numeric_pair(&mut bounds, *prefix);
                        }
                    });
                }
                continue;
            }
            let mut totals: HashMap<GeneralStackXKey, (f64, f64)> = HashMap::new();
            let mut silhouette_prefixes: HashMap<GeneralStackXKey, (f64, f64)> = HashMap::new();
            for sibling in registry
                .series
                .iter()
                .filter(|candidate| candidate.visible && area_stack_matches(series, candidate))
            {
                accumulate_area_values(self, sibling, |x, value| {
                    if series.stack_mode == GeneralStackMode::Silhouette {
                        let total = totals.entry(x.clone()).or_default();
                        total.0 += value;
                        let prefix = silhouette_prefixes.entry(x).or_default();
                        prefix.0 = prefix.0.min(total.0);
                        prefix.1 = prefix.1.max(total.0);
                    } else {
                        let total = totals.entry(x).or_default();
                        if series.stack_mode == GeneralStackMode::Cumulative {
                            total.0 += value;
                            extend_numeric_pair(&mut bounds, total.0);
                        } else if value >= 0.0 {
                            total.0 += value;
                        } else {
                            total.1 += -value;
                        }
                    }
                });
            }
            for (x, (positive, negative)) in totals {
                match series.stack_mode {
                    GeneralStackMode::Normal => {
                        if positive > 0.0 {
                            extend_numeric_pair(&mut bounds, positive);
                        }
                        if negative > 0.0 {
                            extend_numeric_pair(&mut bounds, -negative);
                        }
                    }
                    GeneralStackMode::Percent => {
                        if positive > 0.0 {
                            extend_numeric_pair(&mut bounds, 1.0);
                        }
                        if negative > 0.0 {
                            extend_numeric_pair(&mut bounds, -1.0);
                        }
                    }
                    GeneralStackMode::Positive => {
                        if positive > 0.0 {
                            extend_numeric_pair(&mut bounds, positive);
                        }
                    }
                    GeneralStackMode::Cumulative => {}
                    GeneralStackMode::Silhouette => {
                        let (min, max) = silhouette_prefixes[&x];
                        extend_numeric_pair(&mut bounds, min - positive * 0.5);
                        extend_numeric_pair(&mut bounds, max - positive * 0.5);
                    }
                    GeneralStackMode::Wiggle => unreachable!("wiggle bounds handled above"),
                }
            }
        }
        bounds
    }

    pub(crate) fn visit_general_stacked_area_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralRangePointGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::XyArea || series.stack_id.is_none()
        {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let Some(y_axis) = self.general_axis(&series.y_axis_id) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let Some(registry) = self.general_series.as_ref() else {
            return;
        };

        let mut totals = (matches!(
            series.stack_mode,
            GeneralStackMode::Percent | GeneralStackMode::Silhouette
        ))
        .then(HashMap::<GeneralStackXKey, (f64, f64)>::new);
        if let Some(totals) = totals.as_mut() {
            for sibling in registry
                .series
                .iter()
                .filter(|candidate| candidate.visible && area_stack_matches(series, candidate))
            {
                accumulate_area_values(self, sibling, |x, value| {
                    let total = totals.entry(x).or_default();
                    if value >= 0.0 || series.stack_mode == GeneralStackMode::Silhouette {
                        total.0 += value;
                    } else {
                        total.1 += -value;
                    }
                });
            }
        }

        let mut base: HashMap<GeneralStackXKey, (f64, f64)> = HashMap::new();
        for sibling in registry
            .series
            .iter()
            .filter(|candidate| candidate.visible && area_stack_matches(series, candidate))
        {
            if sibling.id == series.id {
                break;
            }
            accumulate_area_values(self, sibling, |x, value| {
                let entry = base.entry(x).or_default();
                if value >= 0.0
                    || matches!(
                        series.stack_mode,
                        GeneralStackMode::Cumulative
                            | GeneralStackMode::Silhouette
                            | GeneralStackMode::Wiggle
                    )
                {
                    entry.0 += value;
                } else if series.stack_mode != GeneralStackMode::Positive {
                    entry.1 += value;
                }
            });
        }
        if series.stack_mode == GeneralStackMode::Percent
            && let Some(totals) = totals.as_ref()
        {
            for (x, base) in &mut base {
                let total = totals.get(x).copied().unwrap_or_default();
                if total.0 > 0.0 {
                    base.0 /= total.0;
                }
                if total.1 > 0.0 {
                    base.1 /= total.1;
                }
            }
        }

        let wiggle_offsets = if series.stack_mode == GeneralStackMode::Wiggle {
            let (keys, layers) = self.general_wiggle_area_layers(series);
            keys.into_iter()
                .zip(wiggle_baselines(&layers))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };

        self.visit_general_path_points(series, |geometry| {
            let Some(x_key) = general_stack_x_key(dataset, geometry.row) else {
                return;
            };
            let raw_value = dataset.y()[geometry.row];
            let normalized_value =
                if series.stack_mode == GeneralStackMode::Positive && raw_value < 0.0 {
                    0.0
                } else {
                    match totals.as_ref().and_then(|values| values.get(&x_key)) {
                        _ if series.stack_mode != GeneralStackMode::Percent => raw_value,
                        Some(total) if raw_value >= 0.0 && total.0 > 0.0 => raw_value / total.0,
                        Some(total) if raw_value < 0.0 && total.1 > 0.0 => raw_value / total.1,
                        Some(_) => 0.0,
                        None => raw_value,
                    }
                };
            let stack_base = base.get(&x_key).copied().unwrap_or_default();
            let from_value = if series.stack_mode == GeneralStackMode::Wiggle {
                stack_base.0 + wiggle_offsets.get(&x_key).copied().unwrap_or(0.0)
            } else if series.stack_mode == GeneralStackMode::Silhouette {
                stack_base.0
                    - totals
                        .as_ref()
                        .and_then(|values| values.get(&x_key))
                        .map_or(0.0, |total| total.0 * 0.5)
            } else if raw_value >= 0.0 || series.stack_mode == GeneralStackMode::Cumulative {
                stack_base.0
            } else {
                stack_base.1
            };
            let to_value = from_value + normalized_value;
            let low_y = y_scale
                .coordinate(from_value)
                .or_else(|| (from_value == 0.0).then_some(y_range.0));
            let high_y = y_scale.coordinate(to_value);
            let (Some(low_y), Some(high_y)) = (low_y, high_y) else {
                return;
            };
            if !low_y.is_finite() || !high_y.is_finite() {
                return;
            }
            visit(GeneralRangePointGeometry {
                row: geometry.row,
                x: geometry.x,
                low_y,
                high_y,
                starts_new_run: geometry.starts_new_run,
            });
        });
    }

    pub(crate) fn visit_general_path_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralLinePointGeometry),
    {
        if !series.visible
            || !matches!(
                series.kind,
                GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea
            )
        {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(x_domain) = self.effective_general_axis_domain(x_axis) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
        };

        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let numeric_x_scale = match &x_domain {
            GeneralAxisDomain::Numeric(domain) => {
                NumericAxisScale::new(x_axis.scale(), *domain, x_range.0, x_range.1)
            }
            _ => None,
        };
        let temporal_x_scale = match &x_domain {
            GeneralAxisDomain::Temporal([from, to]) => {
                LinearScale::new(*from as f64, *to as f64, x_range.0, x_range.1).ok()
            }
            _ => None,
        };
        let category_lookup: Option<HashMap<&str, usize>> = match &x_domain {
            GeneralAxisDomain::Category(values) => Some(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (value.as_str(), index))
                    .collect(),
            ),
            _ => None,
        };
        let category_band_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Band => {
                BandScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_inner(),
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };
        let category_point_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Point => {
                PointScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };

        let mut starts_new_run = true;
        for row in 0..dataset.len() {
            if !dataset.y_is_valid(row) {
                if !series.connect_missing {
                    starts_new_run = true;
                }
                continue;
            }
            let Some(y) = y_scale.coordinate(dataset.y()[row]) else {
                starts_new_run = true;
                continue;
            };
            let x = match dataset.x_kind() {
                GeneralXKind::Values => None,
                GeneralXKind::Numeric => dataset
                    .numeric_x()
                    .and_then(|values| values.get(row))
                    .and_then(|value| numeric_x_scale.and_then(|scale| scale.coordinate(*value))),
                GeneralXKind::Temporal => dataset
                    .temporal_x_epoch_ms()
                    .and_then(|values| values.get(row))
                    .and_then(|value| {
                        temporal_x_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(*value as f64))
                    }),
                GeneralXKind::Category => {
                    let category = dataset
                        .category_indices()
                        .and_then(|values| values.get(row))
                        .and_then(|value| usize::try_from(*value).ok())
                        .and_then(|index| {
                            dataset.categories().and_then(|values| values.get(index))
                        });
                    category
                        .and_then(|category| {
                            self.category_motion_bounds(x_axis, category, x_range.0, x_range.1)
                                .map(|(first, second)| (first + second) / 2.0)
                                .or_else(|| {
                                    let index = category_lookup
                                        .as_ref()
                                        .and_then(|lookup| lookup.get(category.as_str()))
                                        .copied()?;
                                    match x_axis.scale() {
                                        GeneralScaleType::Band => category_band_scale
                                            .as_ref()
                                            .and_then(|scale| scale.center(index)),
                                        GeneralScaleType::Point => category_point_scale
                                            .as_ref()
                                            .and_then(|scale| scale.coordinate(index)),
                                        _ => None,
                                    }
                                })
                        })
                        .map(|x| {
                            self.category_row_motion_bounds(
                                x_axis,
                                series.dataset,
                                row,
                                (x, x),
                                x_range,
                            )
                            .map(|(first, second)| (first + second) / 2.0)
                            .unwrap_or(x)
                        })
                }
            };
            let Some(x) = x else {
                starts_new_run = true;
                continue;
            };
            if !x.is_finite() || !y.is_finite() {
                starts_new_run = true;
                continue;
            }
            visit(GeneralLinePointGeometry {
                row,
                x,
                y,
                starts_new_run,
            });
            starts_new_run = false;
        }
    }

    pub(crate) fn visit_general_range_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralRangePointGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::RangeArea {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let Some(low_values) = dataset.low() else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(x_domain) = self.effective_general_axis_domain(x_axis) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let numeric_x_scale = match &x_domain {
            GeneralAxisDomain::Numeric(domain) => {
                NumericAxisScale::new(x_axis.scale(), *domain, x_range.0, x_range.1)
            }
            _ => None,
        };
        let temporal_x_scale = match &x_domain {
            GeneralAxisDomain::Temporal([from, to]) => {
                LinearScale::new(*from as f64, *to as f64, x_range.0, x_range.1).ok()
            }
            _ => None,
        };
        let category_lookup: Option<HashMap<&str, usize>> = match &x_domain {
            GeneralAxisDomain::Category(values) => Some(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (value.as_str(), index))
                    .collect(),
            ),
            _ => None,
        };
        let category_band_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Band => {
                BandScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_inner(),
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };
        let category_point_scale = match &x_domain {
            GeneralAxisDomain::Category(values) if x_axis.scale() == GeneralScaleType::Point => {
                PointScale::new(
                    values.len(),
                    x_range.0,
                    x_range.1,
                    x_axis.band_padding_outer(),
                    0.5,
                )
                .ok()
            }
            _ => None,
        };

        let mut starts_new_run = true;
        for (row, &low) in low_values.iter().enumerate().take(dataset.len()) {
            if !dataset.y_is_valid(row) || !dataset.low_is_valid(row) {
                if !series.connect_missing {
                    starts_new_run = true;
                }
                continue;
            }
            let Some(high_y) = y_scale.coordinate(dataset.y()[row]) else {
                starts_new_run = true;
                continue;
            };
            let Some(low_y) = y_scale.coordinate(low) else {
                starts_new_run = true;
                continue;
            };
            let x = match dataset.x_kind() {
                GeneralXKind::Values => None,
                GeneralXKind::Numeric => dataset
                    .numeric_x()
                    .and_then(|values| values.get(row))
                    .and_then(|value| numeric_x_scale.and_then(|scale| scale.coordinate(*value))),
                GeneralXKind::Temporal => dataset
                    .temporal_x_epoch_ms()
                    .and_then(|values| values.get(row))
                    .and_then(|value| {
                        temporal_x_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(*value as f64))
                    }),
                GeneralXKind::Category => {
                    let category = dataset
                        .category_indices()
                        .and_then(|values| values.get(row))
                        .and_then(|value| usize::try_from(*value).ok())
                        .and_then(|index| {
                            dataset.categories().and_then(|values| values.get(index))
                        });
                    category
                        .and_then(|category| {
                            self.category_motion_bounds(x_axis, category, x_range.0, x_range.1)
                                .map(|(first, second)| (first + second) / 2.0)
                                .or_else(|| {
                                    let index = category_lookup
                                        .as_ref()
                                        .and_then(|lookup| lookup.get(category.as_str()))
                                        .copied()?;
                                    match x_axis.scale() {
                                        GeneralScaleType::Band => category_band_scale
                                            .as_ref()
                                            .and_then(|scale| scale.center(index)),
                                        GeneralScaleType::Point => category_point_scale
                                            .as_ref()
                                            .and_then(|scale| scale.coordinate(index)),
                                        _ => None,
                                    }
                                })
                        })
                        .map(|x| {
                            self.category_row_motion_bounds(
                                x_axis,
                                series.dataset,
                                row,
                                (x, x),
                                x_range,
                            )
                            .map(|(first, second)| (first + second) / 2.0)
                            .unwrap_or(x)
                        })
                }
            };
            let Some(x) = x else {
                starts_new_run = true;
                continue;
            };
            if !x.is_finite() || !low_y.is_finite() || !high_y.is_finite() {
                starts_new_run = true;
                continue;
            }
            visit(GeneralRangePointGeometry {
                row,
                x,
                low_y,
                high_y,
                starts_new_run,
            });
            starts_new_run = false;
        }
    }

    pub(crate) fn general_path_baseline_y(&self, series: &GeneralSeries) -> Option<f64> {
        if !series.visible || series.kind != GeneralSeriesKind::XyArea {
            return None;
        }
        let pane_index = self.pane_index_for_id(series.pane_id)?;
        let plot = self.general_plot_rect(pane_index)?;
        let y_axis = self.general_axis(&series.y_axis_id)?;
        let GeneralAxisDomain::Numeric(y_domain) = self.effective_general_axis_domain(y_axis)?
        else {
            return None;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let scale = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)?;
        let baseline = series
            .baseline_value
            .unwrap_or(match series.baseline_policy {
                GeneralAreaBaseline::Zero => 0.0,
                GeneralAreaBaseline::DomainMin => y_domain[0],
                GeneralAreaBaseline::DomainMax => y_domain[1],
            });
        Some(
            scale
                .coordinate(baseline)
                .unwrap_or(y_range.0)
                .clamp(plot.y, plot_bottom),
        )
    }

    pub(crate) fn visit_general_error_bars<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralErrorBarGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::ErrorBar {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let (Some(y_low_values), Some(y_high_values)) = (dataset.low(), dataset.high()) else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(x_domain) = self.effective_general_axis_domain(x_axis) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        match x_domain {
            GeneralAxisDomain::Numeric(domain) => {
                let (Some(x_values), Some(x_low_values), Some(x_high_values), Some(x_scale)) = (
                    dataset.numeric_x(),
                    dataset.x_low(),
                    dataset.x_high(),
                    NumericAxisScale::new(x_axis.scale(), domain, x_range.0, x_range.1),
                ) else {
                    return;
                };
                for row in 0..dataset.len() {
                    if !dataset.y_is_valid(row) {
                        continue;
                    }
                    let (Some(x), Some(y)) = (
                        x_scale.coordinate(x_values[row]),
                        y_scale.coordinate(dataset.y()[row]),
                    ) else {
                        continue;
                    };
                    let x_low = dataset
                        .x_low_is_valid(row)
                        .then(|| x_scale.coordinate(x_low_values[row]))
                        .flatten();
                    let x_high = dataset
                        .x_high_is_valid(row)
                        .then(|| x_scale.coordinate(x_high_values[row]))
                        .flatten();
                    let y_low = dataset
                        .low_is_valid(row)
                        .then(|| y_scale.coordinate(y_low_values[row]))
                        .flatten();
                    let y_high = dataset
                        .high_is_valid(row)
                        .then(|| y_scale.coordinate(y_high_values[row]))
                        .flatten();
                    visit(GeneralErrorBarGeometry {
                        row,
                        x,
                        y,
                        x_low,
                        x_high,
                        y_low,
                        y_high,
                        cap_half_size: series.point_radius,
                    });
                }
            }
            GeneralAxisDomain::Category(axis_categories) => {
                let (Some(categories), Some(indices)) =
                    (dataset.categories(), dataset.category_indices())
                else {
                    return;
                };
                let positions: Option<Vec<f64>> = match x_axis.scale() {
                    GeneralScaleType::Band => BandScale::new(
                        axis_categories.len(),
                        x_range.0,
                        x_range.1,
                        x_axis.band_padding_inner(),
                        x_axis.band_padding_outer(),
                        0.5,
                    )
                    .ok()
                    .and_then(|scale| {
                        (0..axis_categories.len())
                            .map(|index| {
                                self.category_motion_bounds(
                                    x_axis,
                                    &axis_categories[index],
                                    x_range.0,
                                    x_range.1,
                                )
                                .map(|(first, second)| (first + second) / 2.0)
                                .or_else(|| scale.center(index))
                            })
                            .collect()
                    }),
                    GeneralScaleType::Point => PointScale::new(
                        axis_categories.len(),
                        x_range.0,
                        x_range.1,
                        x_axis.band_padding_outer(),
                        0.5,
                    )
                    .ok()
                    .and_then(|scale| {
                        (0..axis_categories.len())
                            .map(|index| {
                                self.category_motion_bounds(
                                    x_axis,
                                    &axis_categories[index],
                                    x_range.0,
                                    x_range.1,
                                )
                                .map(|(first, second)| (first + second) / 2.0)
                                .or_else(|| scale.coordinate(index))
                            })
                            .collect()
                    }),
                    _ => None,
                };
                let Some(positions) = positions else {
                    return;
                };
                let category_x: HashMap<&str, f64> = axis_categories
                    .iter()
                    .map(String::as_str)
                    .zip(positions)
                    .collect();
                for row in 0..dataset.len() {
                    if !dataset.y_is_valid(row) {
                        continue;
                    }
                    let x = categories
                        .get(indices[row] as usize)
                        .and_then(|category| category_x.get(category.as_str()))
                        .copied()
                        .map(|x| {
                            self.category_row_motion_bounds(
                                x_axis,
                                series.dataset,
                                row,
                                (x, x),
                                x_range,
                            )
                            .map(|(first, second)| (first + second) / 2.0)
                            .unwrap_or(x)
                        });
                    let (Some(x), Some(y)) = (x, y_scale.coordinate(dataset.y()[row])) else {
                        continue;
                    };
                    let y_low = dataset
                        .low_is_valid(row)
                        .then(|| y_scale.coordinate(y_low_values[row]))
                        .flatten();
                    let y_high = dataset
                        .high_is_valid(row)
                        .then(|| y_scale.coordinate(y_high_values[row]))
                        .flatten();
                    visit(GeneralErrorBarGeometry {
                        row,
                        x,
                        y,
                        x_low: None,
                        x_high: None,
                        y_low,
                        y_high,
                        cap_half_size: series.point_radius,
                    });
                }
            }
            GeneralAxisDomain::Temporal([from, to]) => {
                let (Some(x_values), Some(x_low_values), Some(x_high_values)) = (
                    dataset.temporal_x_epoch_ms(),
                    dataset.x_low(),
                    dataset.x_high(),
                ) else {
                    return;
                };
                let Ok(x_scale) = LinearScale::new(from as f64, to as f64, x_range.0, x_range.1)
                else {
                    return;
                };
                for row in 0..dataset.len() {
                    if !dataset.y_is_valid(row) {
                        continue;
                    }
                    let (Some(x), Some(y)) = (
                        x_scale.coordinate(x_values[row] as f64),
                        y_scale.coordinate(dataset.y()[row]),
                    ) else {
                        continue;
                    };
                    let x_low = dataset
                        .x_low_is_valid(row)
                        .then(|| x_scale.coordinate(x_low_values[row]))
                        .flatten();
                    let x_high = dataset
                        .x_high_is_valid(row)
                        .then(|| x_scale.coordinate(x_high_values[row]))
                        .flatten();
                    let y_low = dataset
                        .low_is_valid(row)
                        .then(|| y_scale.coordinate(y_low_values[row]))
                        .flatten();
                    let y_high = dataset
                        .high_is_valid(row)
                        .then(|| y_scale.coordinate(y_high_values[row]))
                        .flatten();
                    visit(GeneralErrorBarGeometry {
                        row,
                        x,
                        y,
                        x_low,
                        x_high,
                        y_low,
                        y_high,
                        cap_half_size: series.point_radius,
                    });
                }
            }
            GeneralAxisDomain::Auto => {}
        }
    }

    pub(crate) fn visit_general_box_plots<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralBoxPlotGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::BoxPlot {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        let (
            Some(categories),
            Some(category_indices),
            Some(min_values),
            Some(max_values),
            Some(q1_values),
            Some(q3_values),
        ) = (
            dataset.categories(),
            dataset.category_indices(),
            dataset.low(),
            dataset.high(),
            dataset.x_low(),
            dataset.x_high(),
        )
        else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(GeneralAxisDomain::Category(axis_categories)) =
            self.effective_general_axis_domain(x_axis)
        else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(x_scale) = BandScale::new(
            axis_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let axis_lookup: HashMap<&str, usize> = axis_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();

        for (row, &category_index) in category_indices.iter().enumerate() {
            if !dataset.y_is_valid(row)
                || !dataset.low_is_valid(row)
                || !dataset.high_is_valid(row)
                || !dataset.x_low_is_valid(row)
                || !dataset.x_high_is_valid(row)
            {
                continue;
            }
            let Some(category) = usize::try_from(category_index)
                .ok()
                .and_then(|index| categories.get(index))
            else {
                continue;
            };
            let Some(&axis_index) = axis_lookup.get(category.as_str()) else {
                continue;
            };
            let Some(bounds) = self
                .category_motion_bounds(x_axis, category, x_range.0, x_range.1)
                .or_else(|| x_scale.bounds(axis_index))
            else {
                continue;
            };
            let (band_from, band_to) = self
                .category_row_motion_bounds(x_axis, series.dataset, row, bounds, x_range)
                .unwrap_or(bounds);
            let full_left = band_from.min(band_to).clamp(0.0, plot.width);
            let full_right = band_from.max(band_to).clamp(0.0, plot.width);
            let inset = (full_right - full_left) * 0.2;
            let left = full_left + inset;
            let right = full_right - inset;
            if right <= left {
                continue;
            }
            let (Some(min_y), Some(q1_y), Some(median_y), Some(q3_y), Some(max_y)) = (
                y_scale.coordinate(min_values[row]),
                y_scale.coordinate(q1_values[row]),
                y_scale.coordinate(dataset.y()[row]),
                y_scale.coordinate(q3_values[row]),
                y_scale.coordinate(max_values[row]),
            ) else {
                continue;
            };
            if [min_y, q1_y, median_y, q3_y, max_y]
                .iter()
                .any(|value| !value.is_finite())
            {
                continue;
            }
            visit(GeneralBoxPlotGeometry {
                row,
                center_x: (left + right) * 0.5,
                left,
                right,
                min_y,
                q1_y,
                median_y,
                q3_y,
                max_y,
            });
        }
    }

    pub(crate) fn visit_general_heatmap_cells<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralHeatmapGeometry),
    {
        if !series.visible || series.kind != GeneralSeriesKind::HeatmapGrid {
            return;
        }
        let Some(pane_index) = self.pane_index_for_id(series.pane_id) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
            return;
        };
        if dataset.heatmap_y_numeric().is_some() {
            self.visit_general_numeric_heatmap_cells(series, pane_index, plot, dataset, &mut visit);
            return;
        }
        let (
            Some(x_categories),
            Some(x_category_indices),
            Some(y_categories),
            Some(y_category_indices),
        ) = (
            dataset.categories(),
            dataset.category_indices(),
            dataset.heatmap_y_categories(),
            dataset.heatmap_y_category_indices(),
        )
        else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let (
            Some(GeneralAxisDomain::Category(axis_x_categories)),
            Some(GeneralAxisDomain::Category(axis_y_categories)),
        ) = (
            self.effective_general_axis_domain(x_axis),
            self.effective_general_axis_domain(y_axis),
        )
        else {
            return;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let Ok(x_scale) = BandScale::new(
            axis_x_categories.len(),
            x_range.0,
            x_range.1,
            x_axis.band_padding_inner(),
            x_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Ok(y_scale) = BandScale::new(
            axis_y_categories.len(),
            y_range.0,
            y_range.1,
            y_axis.band_padding_inner(),
            y_axis.band_padding_outer(),
            0.5,
        ) else {
            return;
        };
        let x_lookup: HashMap<&str, usize> = axis_x_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();
        let y_lookup: HashMap<&str, usize> = axis_y_categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.as_str(), index))
            .collect();

        let mut value_bounds: Option<(f64, f64)> = None;
        for row in 0..dataset.len() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let mapped = x_category_indices
                .get(row)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|index| x_categories.get(index))
                .and_then(|category| x_lookup.get(category.as_str()))
                .is_some()
                && y_category_indices
                    .get(row)
                    .and_then(|index| usize::try_from(*index).ok())
                    .and_then(|index| y_categories.get(index))
                    .and_then(|category| y_lookup.get(category.as_str()))
                    .is_some();
            if !mapped {
                continue;
            }
            let value = dataset.y()[row];
            value_bounds = Some(match value_bounds {
                Some((low, high)) => (low.min(value), high.max(value)),
                None => (value, value),
            });
        }
        let Some((value_low, value_high)) = series
            .heatmap_value_domain
            .map(|domain| (domain[0], domain[1]))
            .or(value_bounds)
        else {
            return;
        };

        for row in 0..dataset.len() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let Some(x_axis_index) = x_category_indices
                .get(row)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|index| x_categories.get(index))
                .and_then(|category| x_lookup.get(category.as_str()))
                .copied()
            else {
                continue;
            };
            let Some(y_axis_index) = y_category_indices
                .get(row)
                .and_then(|index| usize::try_from(*index).ok())
                .and_then(|index| y_categories.get(index))
                .and_then(|category| y_lookup.get(category.as_str()))
                .copied()
            else {
                continue;
            };
            let (Some((x0, x1)), Some((y0, y1))) = (
                self.category_motion_bounds(
                    x_axis,
                    &axis_x_categories[x_axis_index],
                    x_range.0,
                    x_range.1,
                )
                .or_else(|| x_scale.bounds(x_axis_index)),
                self.category_motion_bounds(
                    y_axis,
                    &axis_y_categories[y_axis_index],
                    y_range.0,
                    y_range.1,
                )
                .or_else(|| y_scale.bounds(y_axis_index)),
            ) else {
                continue;
            };
            let (x0, x1) = self
                .category_row_motion_bounds(x_axis, series.dataset, row, (x0, x1), x_range)
                .unwrap_or((x0, x1));
            let (y0, y1) = self
                .category_row_motion_bounds(y_axis, series.dataset, row, (y0, y1), y_range)
                .unwrap_or((y0, y1));
            let left = x0.min(x1).clamp(0.0, plot.width);
            let right = x0.max(x1).clamp(0.0, plot.width);
            let top = y0.min(y1).clamp(plot.y, plot_bottom);
            let bottom = y0.max(y1).clamp(plot.y, plot_bottom);
            if right <= left || bottom <= top {
                continue;
            }
            let value = dataset.y()[row];
            let intensity = if value_high > value_low {
                ((value - value_low) / (value_high - value_low)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            visit(GeneralHeatmapGeometry {
                row,
                left,
                right,
                top,
                bottom,
                value,
                intensity,
            });
        }
    }

    fn visit_general_numeric_heatmap_cells<F>(
        &self,
        series: &GeneralSeries,
        _pane_index: usize,
        plot: crate::general_axes::GeneralPlotRect,
        dataset: &crate::GeneralDataset,
        visit: &mut F,
    ) where
        F: FnMut(GeneralHeatmapGeometry),
    {
        let Some(y_coordinates) = dataset.heatmap_y_numeric() else {
            return;
        };
        let (Some(x_axis), Some(y_axis)) = (
            self.general_axis(&series.x_axis_id),
            self.general_axis(&series.y_axis_id),
        ) else {
            return;
        };
        let Some(GeneralAxisDomain::Numeric(y_domain)) = self.effective_general_axis_domain(y_axis)
        else {
            return;
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let Some(y_scale) = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)
        else {
            return;
        };
        let x_domain = self.effective_general_axis_domain(x_axis);
        let numeric_x_scale = match x_domain.as_ref() {
            Some(GeneralAxisDomain::Numeric(domain)) => {
                let range = if x_axis.reverse() {
                    (plot.width, 0.0)
                } else {
                    (0.0, plot.width)
                };
                NumericAxisScale::new(x_axis.scale(), *domain, range.0, range.1)
            }
            _ => None,
        };
        let temporal_x_scale = match x_domain.as_ref() {
            Some(GeneralAxisDomain::Temporal([from, to])) => {
                let range = if x_axis.reverse() {
                    (plot.width, 0.0)
                } else {
                    (0.0, plot.width)
                };
                LinearScale::new(*from as f64, *to as f64, range.0, range.1).ok()
            }
            _ => None,
        };

        let mut mapped = Vec::with_capacity(dataset.len());
        let mut x_centers = Vec::with_capacity(dataset.len());
        let mut y_centers = Vec::with_capacity(dataset.len());
        for row in 0..dataset.len() {
            let x = match dataset.x_kind() {
                GeneralXKind::Values => None,
                GeneralXKind::Numeric => dataset
                    .numeric_x()
                    .and_then(|values| values.get(row))
                    .and_then(|value| numeric_x_scale.and_then(|scale| scale.coordinate(*value))),
                GeneralXKind::Temporal => dataset
                    .temporal_x_epoch_ms()
                    .and_then(|values| values.get(row))
                    .and_then(|value| {
                        temporal_x_scale
                            .as_ref()
                            .and_then(|scale| scale.coordinate(*value as f64))
                    }),
                GeneralXKind::Category => None,
            };
            let y = y_coordinates
                .get(row)
                .and_then(|value| y_scale.coordinate(*value));
            let point = x.zip(y).filter(|(x, y)| x.is_finite() && y.is_finite());
            if let Some((x, y)) = point {
                x_centers.push(x);
                y_centers.push(y);
            }
            mapped.push(point);
        }
        x_centers.sort_by(f64::total_cmp);
        x_centers.dedup_by(|left, right| left.to_bits() == right.to_bits());
        y_centers.sort_by(f64::total_cmp);
        y_centers.dedup_by(|left, right| left.to_bits() == right.to_bits());
        if x_centers.is_empty() || y_centers.is_empty() {
            return;
        }

        let mut value_bounds: Option<(f64, f64)> = None;
        for (row, point) in mapped.iter().enumerate() {
            if point.is_none() || !dataset.y_is_valid(row) {
                continue;
            }
            let value = dataset.y()[row];
            value_bounds = Some(match value_bounds {
                Some((low, high)) => (low.min(value), high.max(value)),
                None => (value, value),
            });
        }
        let Some((value_low, value_high)) = series
            .heatmap_value_domain
            .map(|domain| (domain[0], domain[1]))
            .or(value_bounds)
        else {
            return;
        };

        for (row, point) in mapped.into_iter().enumerate() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let Some((x, y)) = point else {
                continue;
            };
            let Some((left, right)) = heatmap_center_bounds(x, &x_centers, 0.0, plot.width) else {
                continue;
            };
            let Some((top, bottom)) = heatmap_center_bounds(y, &y_centers, plot.y, plot_bottom)
            else {
                continue;
            };
            let value = dataset.y()[row];
            let intensity = if value_high > value_low {
                ((value - value_low) / (value_high - value_low)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            visit(GeneralHeatmapGeometry {
                row,
                left,
                right,
                top,
                bottom,
                value,
                intensity,
            });
        }
    }

    fn scatter_geometry_context(&self, series: &GeneralSeries) -> Option<ScatterGeometryContext> {
        if !series.visible
            || !matches!(
                series.kind,
                GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble
            )
        {
            return None;
        }
        let pane_index = self.pane_index_for_id(series.pane_id)?;
        let plot = self.general_plot_rect(pane_index)?;
        let dataset = self.general_geometry_dataset(series.dataset)?;
        dataset.numeric_x()?;
        let x_axis = self.general_axis(&series.x_axis_id)?;
        let y_axis = self.general_axis(&series.y_axis_id)?;
        let GeneralAxisDomain::Numeric(x_domain) = self.effective_general_axis_domain(x_axis)?
        else {
            return None;
        };
        let GeneralAxisDomain::Numeric(y_domain) = self.effective_general_axis_domain(y_axis)?
        else {
            return None;
        };
        let x_range = if x_axis.reverse() {
            (plot.width, 0.0)
        } else {
            (0.0, plot.width)
        };
        let plot_bottom = plot.y + plot.height;
        let y_range = if y_axis.range_reversed() {
            (plot.y, plot_bottom)
        } else {
            (plot_bottom, plot.y)
        };
        let x_scale = NumericAxisScale::new(x_axis.scale(), x_domain, x_range.0, x_range.1)?;
        let y_scale = NumericAxisScale::new(y_axis.scale(), y_domain, y_range.0, y_range.1)?;
        let radius = if series.kind == GeneralSeriesKind::Bubble {
            MAX_GENERAL_POINT_RADIUS
        } else {
            series.point_radius
        };
        Some(ScatterGeometryContext {
            key: ScatterGeometryKey {
                dataset_generation: dataset.generation(),
                plot_width: plot.width.to_bits(),
                plot_y: plot.y.to_bits(),
                plot_height: plot.height.to_bits(),
                x_domain: x_domain.map(f64::to_bits),
                y_domain: y_domain.map(f64::to_bits),
                x_scale: x_axis.scale(),
                y_scale: y_axis.scale(),
                x_reverse: x_axis.reverse(),
                y_reverse: y_axis.range_reversed(),
                radius: radius.to_bits(),
            },
            x_scale,
            y_scale,
            plot_width: plot.width,
            plot_y: plot.y,
            plot_bottom,
        })
    }

    fn build_scatter_spatial_index(
        &self,
        series: &GeneralSeries,
        context: ScatterGeometryContext,
    ) -> Option<ScatterSpatialIndex> {
        let dataset = self.general_geometry_dataset(series.dataset)?;
        let x_values = dataset.numeric_x()?;
        let mut points = Vec::with_capacity(dataset.len());
        for (row, (&x_value, &y_value)) in x_values.iter().zip(dataset.y()).enumerate() {
            if !dataset.y_is_valid(row) {
                continue;
            }
            let radius = match series.kind {
                GeneralSeriesKind::Scatter => series.point_radius,
                GeneralSeriesKind::Bubble => {
                    let size = dataset.size()?.get(row).copied()?;
                    if !dataset.size_is_valid(row) || size <= 0.0 {
                        continue;
                    }
                    size.sqrt()
                        .clamp(MIN_GENERAL_POINT_RADIUS, MAX_GENERAL_POINT_RADIUS)
                }
                _ => return None,
            };
            let (Some(x), Some(y)) = (
                context.x_scale.coordinate(x_value),
                context.y_scale.coordinate(y_value),
            ) else {
                continue;
            };
            if x < -radius
                || x > context.plot_width + radius
                || y < context.plot_y - radius
                || y > context.plot_bottom + radius
            {
                continue;
            }
            points.push(GeneralScatterGeometry { row, x, y, radius });
        }
        Some(ScatterSpatialIndex::new(
            context.key,
            context.plot_y,
            context.plot_width,
            context.plot_bottom - context.plot_y,
            points,
        ))
    }

    fn with_scatter_spatial_index<R, F>(&self, series: &GeneralSeries, use_index: F) -> Option<R>
    where
        F: FnOnce(&ScatterSpatialIndex) -> R,
    {
        let context = self.scatter_geometry_context(series)?;
        let registry = self.general_series.as_ref()?;
        let current = registry
            .scatter_spatial
            .borrow()
            .get(&series.id)
            .is_some_and(|index| index.key == context.key);
        if !current {
            let index = self.build_scatter_spatial_index(series, context)?;
            registry
                .scatter_spatial
                .borrow_mut()
                .insert(series.id, index);
        }
        let cache = registry.scatter_spatial.borrow();
        let index = cache.get(&series.id)?;
        Some(use_index(index))
    }

    /// Resolve one series' auto-domain contribution for an axis, rescanning its rows only when
    /// the dataset generation or the series binding that shapes the scan changed.
    pub(crate) fn cached_general_series_axis_scan<F>(
        &self,
        series: &GeneralSeries,
        dataset: &crate::general_data::GeneralDataset,
        dimension: AxisDimension,
        scale: GeneralScaleType,
        scan: F,
    ) -> (Option<(f64, f64)>, bool)
    where
        F: FnOnce() -> (Option<(f64, f64)>, bool),
    {
        let Some(registry) = self.general_series.as_ref() else {
            return scan();
        };
        let matches = |entry: &AxisScanEntry| {
            entry.dimension == dimension
                && entry.scale == scale
                && entry.dataset == series.dataset
                && entry.generation == dataset.generation()
                && entry.kind == series.kind
                && entry.stacked == series.stack_id.is_some()
        };
        if let Some(entry) = registry
            .axis_scans
            .borrow()
            .get(&series.id)
            .and_then(|entries| entries.iter().find(|entry| matches(entry)))
        {
            return entry.value;
        }
        let value = scan();
        let mut cache = registry.axis_scans.borrow_mut();
        let entries = cache.entry(series.id).or_default();
        entries.retain(|entry| !(entry.dimension == dimension && entry.scale == scale));
        if entries.len() >= MAX_AXIS_SCANS_PER_SERIES {
            entries.remove(0);
        }
        entries.push(AxisScanEntry {
            dimension,
            scale,
            dataset: series.dataset,
            generation: dataset.generation(),
            kind: series.kind,
            stacked: series.stack_id.is_some(),
            value,
        });
        value
    }

    pub(crate) fn visit_general_pie_slices(
        &self,
        series: &GeneralSeries,
        mut visit: impl FnMut(GeneralPolarSectorGeometry),
    ) {
        if series.kind != GeneralSeriesKind::Pie {
            return;
        }
        let _ = self.with_general_pie_context(series, |context| {
            for slice in &context.slices {
                if let Some(geometry) = slice.geometry(context.plot, series.polar) {
                    visit(geometry);
                }
            }
        });
    }

    pub(crate) fn pie_order_motion_for_dataset(
        &self,
        id: crate::GeneralDatasetId,
        previous: &crate::general_data::GeneralDataset,
        rows: &[crate::general_data::GeneralTransitionRow],
    ) -> Option<Vec<Option<PieOrderRow>>> {
        if rows.len() > MAX_GENERAL_PIE_SLICES
            || !self.general_series_iter().any(|series| {
                series.visible()
                    && series.dataset() == id
                    && series.kind() == GeneralSeriesKind::Pie
            })
        {
            return None;
        }
        let target = self.general_dataset(id)?;
        let prior_motion = self
            .general_transitions
            .iter()
            .find(|transition| transition.dataset == id)
            .and_then(|transition| transition.pie_order.as_ref());
        let mut last_source = None;
        let reordered = rows.iter().filter_map(|row| row.source).any(|source| {
            let moved = last_source.is_some_and(|last| source < last);
            last_source = Some(source);
            moved
        });
        if !reordered && prior_motion.is_none() {
            return None;
        }
        let source_bounds = pie_normalized_bounds(previous);
        let target_bounds = pie_normalized_bounds(target);
        let prior_progress = self
            .general_transitions
            .iter()
            .find(|transition| transition.dataset == id)
            .map(|transition| {
                ((self.general_transition_time_ms - transition.started_ms)
                    / self.general_transition_duration_ms)
                    .clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        Some(
            rows.iter()
                .map(|mapping| {
                    let source = mapping.source.and_then(|row| {
                        prior_motion
                            .and_then(|motion| motion.get(row).copied().flatten())
                            .map(|motion| motion.bounds(prior_progress))
                            .or_else(|| source_bounds.get(row).copied().flatten())
                    });
                    let target = mapping
                        .target
                        .and_then(|row| target_bounds.get(row).copied().flatten());
                    match (source, target) {
                        (Some(from), Some(to)) => Some(PieOrderRow { from, to }),
                        (None, Some(to)) => {
                            let center = (to[0] + to[1]) / 2.0;
                            Some(PieOrderRow {
                                from: [center, center],
                                to,
                            })
                        }
                        (Some(from), None) => {
                            let center = (from[0] + from[1]) / 2.0;
                            Some(PieOrderRow {
                                from,
                                to: [center, center],
                            })
                        }
                        (None, None) => None,
                    }
                })
                .collect(),
        )
    }

    pub(crate) fn radar_slot_motions_for_dataset(
        &self,
        id: GeneralDatasetId,
        previous: &crate::general_data::GeneralDataset,
        rows: &[crate::general_data::GeneralTransitionRow],
        category_axes: &[crate::general_axes::CategoryAxisMotion],
        source_domains: &[(String, [f64; 2])],
    ) -> Option<Vec<RadarSlotMotion>> {
        let Some(target) = self.general_dataset(id) else {
            return Some(Vec::new());
        };
        let mut motions = Vec::new();
        let mut retained_bytes = 0;
        for series in self.general_series_iter().filter(|series| {
            series.visible()
                && series.dataset() == id
                && series.kind() == GeneralSeriesKind::Radar
                && category_axes
                    .iter()
                    .any(|motion| motion.axis_id == series.x_axis_id())
        }) {
            let (Some(angle_axis), Some(radius_axis)) = (
                self.general_axis(series.x_axis_id()),
                self.general_axis(series.y_axis_id()),
            ) else {
                continue;
            };
            let (Some(source_categories), Some(target_categories)) = (
                self.category_axis_values(angle_axis, Some((id, previous)), false, true),
                self.category_axis_values(angle_axis, None, true, true),
            ) else {
                continue;
            };
            if source_categories.len() != target_categories.len() || source_categories.len() < 3 {
                continue;
            }
            let Some(domain) = source_domains
                .iter()
                .find(|(axis_id, _)| axis_id == series.y_axis_id())
                .map(|(_, domain)| *domain)
            else {
                continue;
            };
            let range = if radius_axis.reverse() {
                (1.0, 0.0)
            } else {
                (0.0, 1.0)
            };
            let Some(scale) = NumericAxisScale::new(radius_axis.scale(), domain, range.0, range.1)
            else {
                continue;
            };
            let source_lookup = radar_rows_by_category(previous);
            let source_radii = self
                .general_transitions
                .iter()
                .any(|transition| transition.dataset == id)
                .then(|| {
                    self.with_general_polar_category_context(series, |context| {
                        context.radar_radii.clone()
                    })
                    .flatten()
                })
                .flatten()
                .filter(|radii| radii.len() == source_categories.len())
                .unwrap_or_else(|| {
                    source_categories
                        .iter()
                        .map(|category| {
                            source_lookup
                                .get(category.as_str())
                                .and_then(|row| {
                                    previous.y_is_valid(*row).then_some(previous.y()[*row])
                                })
                                .and_then(|value| scale.coordinate(value))
                                .unwrap_or(0.0)
                                .clamp(0.0, 1.0)
                        })
                        .collect()
                });
            let target_lookup = radar_rows_by_category(target);
            let mut presentation_for_target = vec![None; target.len()];
            for (presentation_row, mapping) in rows.iter().enumerate() {
                if let Some(target_row) = mapping.target {
                    presentation_for_target[target_row] = Some(presentation_row);
                }
            }
            let mut target_values = Vec::with_capacity(target_categories.len());
            let mut target_rows = Vec::with_capacity(target_categories.len());
            for category in &target_categories {
                let row = target_lookup.get(category.as_str()).copied();
                target_values
                    .push(row.and_then(|row| target.y_is_valid(row).then_some(target.y()[row])));
                target_rows.push(row.and_then(|row| presentation_for_target[row]));
            }
            let motion = RadarSlotMotion {
                series_id: series.id(),
                source_radii,
                target_values,
                target_rows,
            };
            retained_bytes += motion.estimated_bytes();
            if retained_bytes > crate::MAX_GENERAL_TRANSITION_BYTES / 4 {
                return None;
            }
            motions.push(motion);
        }
        Some(motions)
    }

    pub(crate) fn radar_shape_motions_for_dataset(
        &self,
        id: GeneralDatasetId,
        previous: &crate::general_data::GeneralDataset,
        rows: &[crate::general_data::GeneralTransitionRow],
        category_axes: &[crate::general_axes::CategoryAxisMotion],
        source_domains: &[(String, [f64; 2])],
    ) -> Option<Vec<RadarShapeMotion>> {
        let Some(target) = self.general_dataset(id) else {
            return Some(Vec::new());
        };
        let mut motions = Vec::new();
        let mut retained_bytes = 0;
        for series in self.general_series_iter().filter(|series| {
            series.visible()
                && series.dataset() == id
                && series.kind() == GeneralSeriesKind::Radar
                && category_axes
                    .iter()
                    .any(|motion| motion.axis_id == series.x_axis_id())
        }) {
            let (Some(angle_axis), Some(radius_axis)) = (
                self.general_axis(series.x_axis_id()),
                self.general_axis(series.y_axis_id()),
            ) else {
                continue;
            };
            let (Some(source_categories), Some(target_categories)) = (
                self.category_axis_values(angle_axis, Some((id, previous)), false, true),
                self.category_axis_values(angle_axis, None, true, true),
            ) else {
                continue;
            };
            if source_categories.len() == target_categories.len() {
                continue;
            }
            let (Some(source_angles), Some(target_angles)) = (
                radar_angular_scale(source_categories.len(), angle_axis.reverse()),
                radar_angular_scale(target_categories.len(), angle_axis.reverse()),
            ) else {
                continue;
            };
            let Some(domain) = source_domains
                .iter()
                .find(|(axis_id, _)| axis_id == series.y_axis_id())
                .map(|(_, domain)| *domain)
            else {
                continue;
            };
            let range = if radius_axis.reverse() {
                (1.0, 0.0)
            } else {
                (0.0, 1.0)
            };
            let Some(scale) = NumericAxisScale::new(radius_axis.scale(), domain, range.0, range.1)
            else {
                continue;
            };
            let mut angles = source_angles
                .count()
                .checked_add(target_angles.count())
                .map(Vec::with_capacity)
                .unwrap_or_default();
            for angular in [source_angles, target_angles] {
                for index in 0..angular.count() {
                    angles.push(
                        angular
                            .center(index)
                            .unwrap()
                            .rem_euclid(std::f64::consts::TAU),
                    );
                }
            }
            angles.sort_by(f64::total_cmp);
            angles.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
            if angles.len() < 3 || angles.len() > MAX_GENERAL_RADAR_VALUES * 2 {
                continue;
            }
            let mut presentation_for_source = vec![None; previous.len()];
            let mut presentation_for_target = vec![None; target.len()];
            for (presentation_row, mapping) in rows.iter().enumerate() {
                if let Some(source_row) = mapping.source {
                    presentation_for_source[source_row] = Some(presentation_row);
                }
                if let Some(target_row) = mapping.target {
                    presentation_for_target[target_row] = Some(presentation_row);
                }
            }
            let source_lookup = radar_rows_by_category(previous);
            let target_lookup = radar_rows_by_category(target);
            let source_values = source_categories
                .iter()
                .map(|category| {
                    source_lookup
                        .get(category.as_str())
                        .and_then(|row| previous.y_is_valid(*row).then_some(previous.y()[*row]))
                        .and_then(|value| scale.coordinate(value))
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0)
                })
                .collect::<Vec<_>>();
            let unit_plot = PolarPlot::from_rect(0.0, 0.0, 2.0, 2.0).unwrap();
            let source_polygon = PolarPolygon::new(unit_plot, source_angles, &source_values).ok();
            let mut prior_shape = None;
            if self
                .general_transitions
                .iter()
                .any(|transition| transition.dataset == id)
            {
                self.with_general_radar_polygon(series, |polygon, old_rows, old_markers| {
                    prior_shape = Some(
                        angles
                            .iter()
                            .map(|&angle| {
                                let point = polygon.plot().point(angle, 0.5).unwrap();
                                let index = polygon.category_at(point.0, point.1);
                                let row = index
                                    .and_then(|index| old_rows.get(index).copied().flatten())
                                    .and_then(|source_row| presentation_for_source[source_row]);
                                let marker = index
                                    .filter(|index| {
                                        polygon
                                            .angle(*index)
                                            .is_some_and(|old| same_polar_angle(old, angle))
                                    })
                                    .and_then(|index| old_markers.get(index).copied().flatten())
                                    .and_then(|source_row| presentation_for_source[source_row]);
                                (polygon.radius_at(angle).unwrap_or(0.0), row, marker)
                            })
                            .collect::<Vec<_>>(),
                    );
                });
            }
            let mut source_radii = Vec::with_capacity(angles.len());
            let mut source_rows = Vec::with_capacity(angles.len());
            let mut source_marker_rows = Vec::with_capacity(angles.len());
            for (index, &angle) in angles.iter().enumerate() {
                let (radius, row, marker) = prior_shape
                    .as_ref()
                    .and_then(|sample| sample.get(index).copied())
                    .unwrap_or_else(|| {
                        let slot = source_angles.index_at(angle);
                        let row = slot
                            .and_then(|slot| source_categories.get(slot))
                            .and_then(|category| source_lookup.get(category.as_str()))
                            .and_then(|row| presentation_for_source[*row]);
                        let marker = slot
                            .filter(|slot| {
                                source_angles
                                    .center(*slot)
                                    .is_some_and(|center| same_polar_angle(center, angle))
                            })
                            .and(row);
                        (
                            source_polygon
                                .and_then(|polygon| polygon.radius_at(angle))
                                .unwrap_or(0.0),
                            row,
                            marker,
                        )
                    });
                source_radii.push(radius);
                source_rows.push(row);
                source_marker_rows.push(marker);
            }
            let target_values = target_categories
                .iter()
                .map(|category| {
                    target_lookup
                        .get(category.as_str())
                        .and_then(|row| target.y_is_valid(*row).then_some(target.y()[*row]))
                })
                .collect();
            let mut target_rows = Vec::with_capacity(angles.len());
            let mut target_marker_rows = Vec::with_capacity(angles.len());
            for &angle in &angles {
                let slot = target_angles.index_at(angle);
                let row = slot
                    .and_then(|slot| target_categories.get(slot))
                    .and_then(|category| target_lookup.get(category.as_str()))
                    .and_then(|row| presentation_for_target[*row]);
                let marker = slot
                    .filter(|slot| {
                        target_angles
                            .center(*slot)
                            .is_some_and(|center| same_polar_angle(center, angle))
                    })
                    .and(row);
                target_rows.push(row);
                target_marker_rows.push(marker);
            }
            let motion = RadarShapeMotion {
                series_id: series.id(),
                angles: Arc::from(
                    angles
                        .into_iter()
                        .map(|angle| (angle, angle))
                        .collect::<Vec<_>>(),
                ),
                source_radii,
                source_rows,
                source_marker_rows,
                target_values,
                target_rows,
                target_marker_rows,
                target_angles,
            };
            retained_bytes += motion.estimated_bytes();
            if retained_bytes > crate::MAX_GENERAL_TRANSITION_BYTES / 4 {
                return None;
            }
            motions.push(motion);
        }
        Some(motions)
    }

    fn build_general_pie_context(&self, series: &GeneralSeries) -> Option<GeneralPieContext> {
        let pane = self.pane_index_for_id(series.pane_id)?;
        let polar_plot = self.general_polar_plot(pane)?;
        let dataset = self.general_geometry_dataset(series.dataset)?;
        let max_value = dataset
            .y()
            .iter()
            .enumerate()
            .filter(|(row, value)| dataset.y_is_valid(*row) && **value > 0.0)
            .map(|(_, value)| *value)
            .fold(0.0_f64, f64::max);
        if max_value == 0.0 {
            return None;
        }
        let total: f64 = dataset
            .y()
            .iter()
            .enumerate()
            .filter(|(row, value)| dataset.y_is_valid(*row) && **value > 0.0)
            .map(|(_, value)| *value / max_value)
            .sum();
        let last_positive = (0..dataset.len())
            .rev()
            .find(|row| dataset.y_is_valid(*row) && dataset.y()[*row] > 0.0);
        let last_positive = last_positive?;
        let options = series.polar;
        let full_sweep = options.end_angle - options.start_angle;
        let start_angle = options.start_angle.rem_euclid(std::f64::consts::TAU);
        let end_angle = start_angle + full_sweep;
        let mut accumulated = 0.0;
        let mut slices = Vec::with_capacity(dataset.len());
        let transition = self
            .general_transitions
            .iter()
            .find(|transition| transition.dataset == series.dataset);
        let motion = transition.and_then(|transition| transition.pie_order.as_ref());
        let progress = transition.map_or(0.0, |transition| {
            ((self.general_transition_time_ms - transition.started_ms)
                / self.general_transition_duration_ms)
                .clamp(0.0, 1.0)
        });
        for (row, value) in dataset.y().iter().enumerate() {
            if !dataset.y_is_valid(row) || *value <= 0.0 {
                continue;
            }
            let from = start_angle + full_sweep * accumulated;
            accumulated += (*value / max_value) / total;
            let to = if row == last_positive {
                end_angle
            } else {
                (start_angle + full_sweep * accumulated).min(end_angle)
            };
            let (from, to) = motion
                .and_then(|motion| motion.get(row).copied().flatten())
                .map(|motion| {
                    let [first, second] = motion.bounds(progress);
                    (
                        start_angle + full_sweep * first,
                        start_angle + full_sweep * second,
                    )
                })
                .unwrap_or((from, to));
            let slice = GeneralPieSlice { row, from, to };
            if slice.geometry(polar_plot, options).is_some() {
                slices.push(slice);
            }
        }
        Some(GeneralPieContext {
            scene_generation: self.frame_invalidation.scene_generation(),
            plot: polar_plot,
            start: start_angle,
            end: end_angle,
            slices,
        })
    }

    fn with_general_pie_context<R>(
        &self,
        series: &GeneralSeries,
        use_context: impl FnOnce(&GeneralPieContext) -> R,
    ) -> Option<R> {
        let registry = self.general_series.as_ref()?;
        let scene = self.frame_invalidation.scene_generation();
        let current = registry
            .pie_contexts
            .borrow()
            .get(&series.id)
            .is_some_and(|context| context.scene_generation == scene);
        if !current {
            let context = self.build_general_pie_context(series)?;
            let mut cache = registry.pie_contexts.borrow_mut();
            cache.retain(|_, entry| entry.scene_generation == scene);
            if cache.len() >= MAX_POLAR_CONTEXT_CACHES
                && !cache.contains_key(&series.id)
                && let Some(oldest) = cache.keys().copied().min_by_key(|id| id.get())
            {
                cache.remove(&oldest);
            }
            cache.insert(series.id, context);
        }
        let cache = registry.pie_contexts.borrow();
        Some(use_context(cache.get(&series.id)?))
    }

    fn general_pie_hit(&self, series: &GeneralSeries, x: f64, y: f64) -> Option<usize> {
        self.with_general_pie_context(series, |context| {
            if self.general_transitions.iter().any(|transition| {
                transition.dataset == series.dataset && transition.pie_order.is_some()
            }) {
                return context.slices.iter().rev().find_map(|slice| {
                    let geometry = slice.geometry(context.plot, series.polar)?;
                    geometry.sector.contains(x, y).then_some(geometry.row)
                });
            }
            let (angle, _) = context.plot.invert(x, y)?;
            let unwrapped =
                context.start + (angle - context.start).rem_euclid(std::f64::consts::TAU);
            if unwrapped >= context.end {
                return None;
            }
            let index = context
                .slices
                .partition_point(|slice| slice.from <= unwrapped)
                .checked_sub(1)?;
            let slice = context.slices[index];
            let geometry = slice.geometry(context.plot, series.polar)?;
            geometry.sector.contains(x, y).then_some(geometry.row)
        })?
    }

    fn build_general_polar_category_context(
        &self,
        series: &GeneralSeries,
    ) -> Option<GeneralPolarCategoryContext> {
        let pane = self.pane_index_for_id(series.pane_id)?;
        let plot = self.general_polar_plot(pane)?;
        let dataset = self.general_geometry_dataset(series.dataset)?;
        let angle_axis = self.general_axis(&series.x_axis_id)?;
        let radius_axis = self.general_axis(&series.y_axis_id)?;
        let GeneralAxisDomain::Category(categories) =
            self.effective_general_axis_domain(angle_axis)?
        else {
            return None;
        };
        if categories.is_empty() {
            return None;
        }
        let GeneralAxisDomain::Numeric(domain) = self.effective_general_axis_domain(radius_axis)?
        else {
            return None;
        };
        let step = std::f64::consts::TAU / categories.len() as f64;
        let start = if angle_axis.reverse() {
            step / 2.0
        } else {
            -step / 2.0
        };
        let angles =
            AngularCategoryScale::new(categories.len(), start, !angle_axis.reverse()).ok()?;
        let range = if radius_axis.reverse() {
            (1.0, 0.0)
        } else {
            (0.0, 1.0)
        };
        let radius_scale = NumericAxisScale::new(radius_axis.scale(), domain, range.0, range.1)?;
        let mut axis_indices = HashMap::with_capacity(categories.len());
        for (index, category) in categories.iter().enumerate() {
            axis_indices.insert(category.as_str(), index);
        }
        let mut rows = vec![None; categories.len()];
        let dataset_categories = dataset.categories()?;
        let dataset_indices = dataset.category_indices()?;
        for (row, &index) in dataset_indices.iter().enumerate() {
            let category = dataset_categories.get(index as usize)?;
            if let Some(&axis_index) = axis_indices.get(category.as_str()) {
                rows[axis_index] = Some(row);
            }
        }
        let sector_bounds = self
            .general_transitions
            .iter()
            .any(|transition| {
                transition
                    .category_axes
                    .iter()
                    .any(|motion| motion.axis_id == angle_axis.id())
            })
            .then(|| {
                categories
                    .iter()
                    .enumerate()
                    .map(|(index, category)| {
                        let fallback = angles.sector(index)?;
                        let current = self
                            .category_motion_bounds_normalized(angle_axis.id(), category)
                            .map(|bounds| {
                                (
                                    bounds[0] * std::f64::consts::TAU,
                                    bounds[1] * std::f64::consts::TAU,
                                )
                            })
                            .unwrap_or(fallback);
                        Some(
                            rows[index]
                                .and_then(|row| {
                                    self.category_row_motion_bounds(
                                        angle_axis,
                                        series.dataset,
                                        row,
                                        current,
                                        (0.0, std::f64::consts::TAU),
                                    )
                                })
                                .unwrap_or(current),
                        )
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        let mut radar_radii = if series.kind == GeneralSeriesKind::Radar {
            let mut radii = vec![0.0; rows.len()];
            for (index, row) in rows.iter().enumerate() {
                if let Some(row) = row
                    && dataset.y_is_valid(*row)
                {
                    radii[index] = radius_scale.coordinate(dataset.y()[*row])?.clamp(0.0, 1.0);
                }
            }
            Some(radii)
        } else {
            None
        };
        let radar_motion = self
            .general_transitions
            .iter()
            .find(|transition| transition.dataset == series.dataset)
            .and_then(|transition| {
                transition
                    .radar_slots
                    .iter()
                    .find(|motion| motion.series_id == series.id)
                    .map(|motion| (transition, motion))
            });
        let mut radar_slot_morph = false;
        if let (Some((transition, motion)), Some(radii)) = (radar_motion, radar_radii.as_mut())
            && motion.source_radii.len() == radii.len()
            && motion.target_values.len() == radii.len()
        {
            let progress = ((self.general_transition_time_ms - transition.started_ms)
                / self.general_transition_duration_ms)
                .clamp(0.0, 1.0);
            for (index, radius) in radii.iter_mut().enumerate() {
                let target_radius = motion.target_values[index]
                    .and_then(|value| radius_scale.coordinate(value))
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                *radius = motion.source_radii[index] * (1.0 - progress) + target_radius * progress;
            }
            if progress >= 0.5 {
                rows.copy_from_slice(&motion.target_rows);
            }
            radar_slot_morph = true;
        }
        let radar_shape = self
            .general_transitions
            .iter()
            .find(|transition| transition.dataset == series.dataset)
            .and_then(|transition| {
                transition
                    .radar_shapes
                    .iter()
                    .find(|motion| motion.series_id == series.id)
                    .map(|motion| (transition, motion))
            });
        let mut radar_shape_angles = None;
        let mut radar_marker_rows = None;
        if let Some((transition, motion)) = radar_shape {
            let progress = ((self.general_transition_time_ms - transition.started_ms)
                / self.general_transition_duration_ms)
                .clamp(0.0, 1.0);
            let target_radii = motion
                .target_values
                .iter()
                .map(|value| {
                    value
                        .and_then(|value| radius_scale.coordinate(value))
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0)
                })
                .collect::<Vec<_>>();
            let target_polygon = PolarPolygon::new(plot, motion.target_angles, &target_radii).ok();
            let radii = motion
                .angles
                .iter()
                .zip(&motion.source_radii)
                .map(|((angle, _), source)| {
                    let target = target_polygon
                        .and_then(|polygon| polygon.radius_at(*angle))
                        .unwrap_or(0.0);
                    source * (1.0 - progress) + target * progress
                })
                .collect::<Vec<_>>();
            rows = if progress < 0.5 {
                motion.source_rows.clone()
            } else {
                motion.target_rows.clone()
            };
            radar_marker_rows = Some(if progress < 0.5 {
                motion.source_marker_rows.clone()
            } else {
                motion.target_marker_rows.clone()
            });
            radar_radii = Some(radii);
            radar_shape_angles = Some(Arc::clone(&motion.angles));
        }
        Some(GeneralPolarCategoryContext {
            scene_generation: self.frame_invalidation.scene_generation(),
            plot,
            angles,
            sector_bounds,
            radar_slot_morph,
            radar_shape_angles,
            radar_marker_rows,
            radius_scale,
            rows,
            radar_radii,
        })
    }

    fn with_general_polar_category_context<R>(
        &self,
        series: &GeneralSeries,
        use_context: impl FnOnce(&GeneralPolarCategoryContext) -> R,
    ) -> Option<R> {
        let registry = self.general_series.as_ref()?;
        let scene = self.frame_invalidation.scene_generation();
        let current = registry
            .polar_contexts
            .borrow()
            .get(&series.id)
            .is_some_and(|context| context.scene_generation == scene);
        if !current {
            let context = self.build_general_polar_category_context(series)?;
            let mut cache = registry.polar_contexts.borrow_mut();
            cache.retain(|_, entry| entry.scene_generation == scene);
            if cache.len() >= MAX_POLAR_CONTEXT_CACHES
                && !cache.contains_key(&series.id)
                && let Some(oldest) = cache.keys().copied().min_by_key(|id| id.get())
            {
                cache.remove(&oldest);
            }
            cache.insert(series.id, context);
        }
        let cache = registry.polar_contexts.borrow();
        Some(use_context(cache.get(&series.id)?))
    }

    pub(crate) fn with_general_radar_polygon(
        &self,
        series: &GeneralSeries,
        mut visit: impl FnMut(PolarPolygon<'_>, &[Option<usize>], &[Option<usize>]),
    ) {
        if series.kind != GeneralSeriesKind::Radar {
            return;
        }
        let _ = self.with_general_polar_category_context(series, |context| {
            if context.rows.len() < 3 {
                return;
            }
            let Some(radii) = context.radar_radii.as_deref() else {
                return;
            };
            let polygon = if let Some(sectors) = context.radar_shape_angles.as_deref() {
                AngularCategoryScale::new(sectors.len(), 0.0, true).and_then(|angles| {
                    PolarPolygon::new_with_sectors(context.plot, angles, sectors, radii)
                })
            } else if context.radar_slot_morph {
                PolarPolygon::new(context.plot, context.angles, radii)
            } else if let Some(sectors) = context.sector_bounds.as_deref() {
                PolarPolygon::new_with_sectors(context.plot, context.angles, sectors, radii)
            } else {
                PolarPolygon::new(context.plot, context.angles, radii)
            };
            if let Ok(polygon) = polygon {
                visit(
                    polygon,
                    &context.rows,
                    context
                        .radar_marker_rows
                        .as_deref()
                        .unwrap_or(&context.rows),
                );
            }
        });
    }

    pub(crate) fn visit_general_polar_value_sectors(
        &self,
        series: &GeneralSeries,
        mut visit: impl FnMut(GeneralPolarSectorGeometry),
    ) {
        if !matches!(
            series.kind,
            GeneralSeriesKind::PolarArea | GeneralSeriesKind::RadialBar
        ) {
            return;
        }
        let _ = self.with_general_polar_category_context(series, |context| {
            let Some(dataset) = self.general_geometry_dataset(series.dataset) else {
                return;
            };
            let options = series.polar;
            let sweep_fraction = (options.end_angle - options.start_angle) / std::f64::consts::TAU;
            for index in 0..context.rows.len() {
                if let Some(geometry) =
                    polar_value_sector(context, dataset, series, index, sweep_fraction)
                {
                    visit(geometry);
                }
            }
        });
    }

    fn general_polar_value_hit(&self, series: &GeneralSeries, x: f64, y: f64) -> Option<usize> {
        self.with_general_polar_category_context(series, |context| {
            let dataset = self.general_geometry_dataset(series.dataset)?;
            if context.sector_bounds.is_some() {
                let fraction =
                    (series.polar.end_angle - series.polar.start_angle) / std::f64::consts::TAU;
                return (0..context.rows.len()).rev().find_map(|index| {
                    let geometry = polar_value_sector(context, dataset, series, index, fraction)?;
                    geometry.sector.contains(x, y).then_some(geometry.row)
                });
            }
            let (angle, _) = context.plot.invert(x, y)?;
            let count = context.angles.count();
            let (first, _) = context.angles.sector(0)?;
            let (_, last) = context.angles.sector(count - 1)?;
            let fraction =
                (series.polar.end_angle - series.polar.start_angle) / std::f64::consts::TAU;
            let from = series.polar.start_angle + first * fraction;
            let to = series.polar.start_angle + last * fraction;
            let low = from.min(to);
            let high = from.max(to);
            let unwrapped =
                angle + ((low - angle) / std::f64::consts::TAU).ceil() * std::f64::consts::TAU;
            if unwrapped > high {
                return None;
            }
            let base_angle = (unwrapped - series.polar.start_angle) / fraction;
            let index = context.angles.index_at(base_angle)?;
            for candidate in [index, (index + count - 1) % count, (index + 1) % count] {
                if let Some(geometry) =
                    polar_value_sector(context, dataset, series, candidate, fraction)
                    && geometry.sector.contains(x, y)
                {
                    return Some(geometry.row);
                }
            }
            None
        })?
    }

    pub(crate) fn visit_general_scatter_points<F>(&self, series: &GeneralSeries, mut visit: F)
    where
        F: FnMut(GeneralScatterGeometry),
    {
        let _ = self.with_scatter_spatial_index(series, |index| {
            for &point in &index.points {
                visit(point);
            }
        });
    }

    #[doc(hidden)]
    pub fn general_hit_test(
        &self,
        pane_index: usize,
        x_css: f64,
        y_css: f64,
        mode: GeneralHitMode,
    ) -> Option<GeneralSeriesHit> {
        if !x_css.is_finite() || !y_css.is_finite() {
            return None;
        }
        let max_distance = match mode {
            GeneralHitMode::Exact => 0.0,
            GeneralHitMode::Nearest { max_distance }
                if max_distance.is_finite() && max_distance >= 0.0 =>
            {
                max_distance
            }
            GeneralHitMode::Nearest { .. } => return None,
        };
        let pane_id = self.pane_stable_id(pane_index)?;
        let plot = self.general_plot_rect(pane_index)?;
        // X is plot-local; Y is chart-space. Neither exact nor nearest hits may return ink
        // outside the plot, even when a path or its baseline extends through axis chrome.
        if x_css < 0.0 || x_css >= plot.width || y_css < plot.y || y_css >= plot.y + plot.height {
            return None;
        }
        let registry = self.general_series.as_ref()?;
        let mut best: Option<GeneralSeriesHit> = None;

        for series in registry
            .series
            .iter()
            .rev()
            .filter(|series| series.visible && series.pane_id == pane_id)
        {
            let Some(dataset) = self.general_dataset(series.dataset) else {
                continue;
            };
            if series.kind == GeneralSeriesKind::Histogram {
                self.visit_general_histogram_bins(series, |geometry| {
                    let distance = distance_to_bar(x_css, y_css, geometry, series, self, false);
                    if distance <= max_distance
                        && best
                            .as_ref()
                            .is_none_or(|current| distance < current.distance)
                    {
                        best = Some(GeneralSeriesHit {
                            series: series.id,
                            row: geometry.row,
                            row_id: histogram_bin_identity(geometry.row),
                            distance,
                        });
                    }
                });
                continue;
            }
            let mut consider = |presentation_row: usize, distance: f64| {
                if distance > max_distance {
                    return;
                }
                let Some(row) =
                    self.general_transition_target_row(series.dataset, presentation_row)
                else {
                    return;
                };
                if best
                    .as_ref()
                    .is_some_and(|current| distance >= current.distance)
                {
                    return;
                }
                let Some(row_id) = dataset.row_identity(row).cloned() else {
                    return;
                };
                best = Some(GeneralSeriesHit {
                    series: series.id,
                    row,
                    row_id,
                    distance,
                });
            };
            match series.kind {
                GeneralSeriesKind::Pie => {
                    if let Some(row) = self.general_pie_hit(series, x_css, y_css) {
                        consider(row, 0.0);
                    }
                }
                GeneralSeriesKind::PolarArea | GeneralSeriesKind::RadialBar => {
                    if let Some(row) = self.general_polar_value_hit(series, x_css, y_css) {
                        consider(row, 0.0);
                    }
                }
                GeneralSeriesKind::Radar => {
                    self.with_general_radar_polygon(series, |polygon, rows, _| {
                        if polygon.contains(x_css, y_css)
                            && let Some(row) = polygon
                                .category_at(x_css, y_css)
                                .and_then(|index| rows.get(index))
                                .copied()
                                .flatten()
                        {
                            consider(row, 0.0);
                        }
                    });
                }
                GeneralSeriesKind::XyLine => {
                    if series.interpolation != GeneralInterpolation::Linear {
                        let mut run = Vec::new();
                        self.visit_general_path_points(series, |geometry| {
                            if geometry.starts_new_run {
                                if let Some((row, distance)) = distance_to_interpolated_line(
                                    x_css,
                                    y_css,
                                    &run,
                                    series.interpolation.render_type(),
                                ) {
                                    consider(row, (distance - 3.0).max(0.0));
                                }
                                run.clear();
                            }
                            if series.point_markers {
                                consider(
                                    geometry.row,
                                    distance_to_point_symbol(
                                        x_css,
                                        y_css,
                                        geometry.x,
                                        geometry.y,
                                        series.point_radius,
                                        dataset
                                            .row_symbol(geometry.row)
                                            .unwrap_or(series.point_symbol),
                                    ),
                                );
                            }
                            run.push(geometry);
                        });
                        if let Some((row, distance)) = distance_to_interpolated_line(
                            x_css,
                            y_css,
                            &run,
                            series.interpolation.render_type(),
                        ) {
                            consider(row, (distance - 3.0).max(0.0));
                        }
                        continue;
                    }
                    let mut previous: Option<GeneralLinePointGeometry> = None;
                    self.visit_general_path_points(series, |geometry| {
                        if series.point_markers {
                            consider(
                                geometry.row,
                                distance_to_point_symbol(
                                    x_css,
                                    y_css,
                                    geometry.x,
                                    geometry.y,
                                    series.point_radius,
                                    dataset
                                        .row_symbol(geometry.row)
                                        .unwrap_or(series.point_symbol),
                                ),
                            );
                        }
                        if geometry.starts_new_run {
                            previous = Some(geometry);
                            return;
                        }
                        let Some(from) = previous else {
                            previous = Some(geometry);
                            return;
                        };
                        let (distance, position) = distance_to_segment(
                            x_css, y_css, from.x, from.y, geometry.x, geometry.y,
                        );
                        let row = if position <= 0.5 {
                            from.row
                        } else {
                            geometry.row
                        };
                        consider(row, (distance - 3.0).max(0.0));
                        previous = Some(geometry);
                    });
                }
                GeneralSeriesKind::XyArea => {
                    if series.stack_id.is_some() {
                        if series.interpolation != GeneralInterpolation::Linear {
                            let mut run = Vec::new();
                            self.visit_general_stacked_area_points(series, |geometry| {
                                if geometry.starts_new_run {
                                    if let Some((row, distance)) = distance_to_interpolated_band(
                                        x_css,
                                        y_css,
                                        &run,
                                        series.interpolation.render_type(),
                                    ) {
                                        consider(row, distance);
                                    }
                                    run.clear();
                                }
                                if series.point_markers {
                                    consider(
                                        geometry.row,
                                        distance_to_point_symbol(
                                            x_css,
                                            y_css,
                                            geometry.x,
                                            geometry.high_y,
                                            series.point_radius,
                                            dataset
                                                .row_symbol(geometry.row)
                                                .unwrap_or(series.point_symbol),
                                        ),
                                    );
                                }
                                run.push(geometry);
                            });
                            if let Some((row, distance)) = distance_to_interpolated_band(
                                x_css,
                                y_css,
                                &run,
                                series.interpolation.render_type(),
                            ) {
                                consider(row, distance);
                            }
                            continue;
                        }
                        let mut previous: Option<GeneralRangePointGeometry> = None;
                        self.visit_general_stacked_area_points(series, |geometry| {
                            if series.point_markers {
                                consider(
                                    geometry.row,
                                    distance_to_point_symbol(
                                        x_css,
                                        y_css,
                                        geometry.x,
                                        geometry.high_y,
                                        series.point_radius,
                                        dataset
                                            .row_symbol(geometry.row)
                                            .unwrap_or(series.point_symbol),
                                    ),
                                );
                            }
                            if geometry.starts_new_run {
                                previous = Some(geometry);
                                return;
                            }
                            let Some(from) = previous else {
                                previous = Some(geometry);
                                return;
                            };
                            let (distance, position) = distance_to_band_segment(
                                x_css,
                                y_css,
                                from.x,
                                from.low_y,
                                from.high_y,
                                geometry.x,
                                geometry.low_y,
                                geometry.high_y,
                            );
                            let row = if position <= 0.5 {
                                from.row
                            } else {
                                geometry.row
                            };
                            consider(row, distance);
                            previous = Some(geometry);
                        });
                        continue;
                    }
                    let Some(baseline_y) = self.general_path_baseline_y(series) else {
                        continue;
                    };
                    if series.interpolation != GeneralInterpolation::Linear {
                        let mut run = Vec::new();
                        self.visit_general_path_points(series, |geometry| {
                            if geometry.starts_new_run {
                                if let Some((row, distance)) = distance_to_interpolated_area(
                                    x_css,
                                    y_css,
                                    &run,
                                    baseline_y,
                                    series.interpolation.render_type(),
                                ) {
                                    consider(row, distance);
                                }
                                run.clear();
                            }
                            if series.point_markers {
                                consider(
                                    geometry.row,
                                    distance_to_point_symbol(
                                        x_css,
                                        y_css,
                                        geometry.x,
                                        geometry.y,
                                        series.point_radius,
                                        dataset
                                            .row_symbol(geometry.row)
                                            .unwrap_or(series.point_symbol),
                                    ),
                                );
                            }
                            run.push(geometry);
                        });
                        if let Some((row, distance)) = distance_to_interpolated_area(
                            x_css,
                            y_css,
                            &run,
                            baseline_y,
                            series.interpolation.render_type(),
                        ) {
                            consider(row, distance);
                        }
                        continue;
                    }
                    let mut previous: Option<GeneralLinePointGeometry> = None;
                    self.visit_general_path_points(series, |geometry| {
                        if series.point_markers {
                            consider(
                                geometry.row,
                                distance_to_point_symbol(
                                    x_css,
                                    y_css,
                                    geometry.x,
                                    geometry.y,
                                    series.point_radius,
                                    dataset
                                        .row_symbol(geometry.row)
                                        .unwrap_or(series.point_symbol),
                                ),
                            );
                        }
                        if geometry.starts_new_run {
                            previous = Some(geometry);
                            return;
                        }
                        let Some(from) = previous else {
                            previous = Some(geometry);
                            return;
                        };
                        let (distance, position) = distance_to_area_segment(
                            x_css, y_css, from.x, from.y, geometry.x, geometry.y, baseline_y,
                        );
                        let row = if position <= 0.5 {
                            from.row
                        } else {
                            geometry.row
                        };
                        consider(row, distance);
                        previous = Some(geometry);
                    });
                }
                GeneralSeriesKind::RangeArea => {
                    if series.interpolation != GeneralInterpolation::Linear {
                        let mut run = Vec::new();
                        self.visit_general_range_points(series, |geometry| {
                            if geometry.starts_new_run {
                                if let Some((row, distance)) = distance_to_interpolated_band(
                                    x_css,
                                    y_css,
                                    &run,
                                    series.interpolation.render_type(),
                                ) {
                                    consider(row, distance);
                                }
                                run.clear();
                            }
                            if series.point_markers {
                                consider(
                                    geometry.row,
                                    distance_to_point_symbol(
                                        x_css,
                                        y_css,
                                        geometry.x,
                                        geometry.low_y,
                                        series.point_radius,
                                        dataset
                                            .row_symbol(geometry.row)
                                            .unwrap_or(series.point_symbol),
                                    )
                                    .min(
                                        distance_to_point_symbol(
                                            x_css,
                                            y_css,
                                            geometry.x,
                                            geometry.high_y,
                                            series.point_radius,
                                            dataset
                                                .row_symbol(geometry.row)
                                                .unwrap_or(series.point_symbol),
                                        ),
                                    ),
                                );
                            }
                            run.push(geometry);
                        });
                        if let Some((row, distance)) = distance_to_interpolated_band(
                            x_css,
                            y_css,
                            &run,
                            series.interpolation.render_type(),
                        ) {
                            consider(row, distance);
                        }
                        continue;
                    }
                    let mut previous: Option<GeneralRangePointGeometry> = None;
                    self.visit_general_range_points(series, |geometry| {
                        if series.point_markers {
                            consider(
                                geometry.row,
                                distance_to_point_symbol(
                                    x_css,
                                    y_css,
                                    geometry.x,
                                    geometry.low_y,
                                    series.point_radius,
                                    dataset
                                        .row_symbol(geometry.row)
                                        .unwrap_or(series.point_symbol),
                                )
                                .min(distance_to_point_symbol(
                                    x_css,
                                    y_css,
                                    geometry.x,
                                    geometry.high_y,
                                    series.point_radius,
                                    dataset
                                        .row_symbol(geometry.row)
                                        .unwrap_or(series.point_symbol),
                                )),
                            );
                        }
                        if geometry.starts_new_run {
                            previous = Some(geometry);
                            return;
                        }
                        let Some(from) = previous else {
                            previous = Some(geometry);
                            return;
                        };
                        let (distance, position) = distance_to_band_segment(
                            x_css,
                            y_css,
                            from.x,
                            from.low_y,
                            from.high_y,
                            geometry.x,
                            geometry.low_y,
                            geometry.high_y,
                        );
                        let row = if position <= 0.5 {
                            from.row
                        } else {
                            geometry.row
                        };
                        consider(row, distance);
                        previous = Some(geometry);
                    });
                }
                GeneralSeriesKind::ErrorBar => {
                    // Every error-bar segment lies inside the bar's bounding box, so a pointer
                    // farther than the hit reach from that box can skip the segment distances.
                    let reach = max_distance + 3.0;
                    self.visit_general_error_bars(series, |geometry| {
                        let cap = geometry.cap_half_size;
                        let [left, right] = error_bar_extent(
                            geometry.x,
                            geometry.x_low,
                            geometry.x_high,
                            geometry.y_low.is_some() || geometry.y_high.is_some(),
                            cap,
                        );
                        let [top, bottom] = error_bar_extent(
                            geometry.y,
                            geometry.y_low,
                            geometry.y_high,
                            geometry.x_low.is_some() || geometry.x_high.is_some(),
                            cap,
                        );
                        if x_css < left - reach
                            || x_css > right + reach
                            || y_css < top - reach
                            || y_css > bottom + reach
                        {
                            return;
                        }
                        consider(
                            geometry.row,
                            (distance_to_error_bar(x_css, y_css, geometry) - 3.0).max(0.0),
                        );
                    });
                }
                GeneralSeriesKind::Histogram => {}
                GeneralSeriesKind::Column => self.visit_general_columns(series, |geometry| {
                    consider(
                        geometry.row,
                        distance_to_bar(x_css, y_css, geometry, series, self, false),
                    );
                }),
                GeneralSeriesKind::RangeBar => self.visit_general_range_bars(series, |geometry| {
                    consider(geometry.row, distance_to_rect(x_css, y_css, geometry));
                }),
                GeneralSeriesKind::HorizontalBar => {
                    self.visit_general_horizontal_bars(series, |geometry| {
                        consider(
                            geometry.row,
                            distance_to_bar(x_css, y_css, geometry, series, self, true),
                        );
                    })
                }
                GeneralSeriesKind::BoxPlot => self.visit_general_box_plots(series, |geometry| {
                    consider(
                        geometry.row,
                        (distance_to_box_plot(x_css, y_css, geometry) - 3.0).max(0.0),
                    );
                }),
                GeneralSeriesKind::HeatmapGrid => {
                    self.visit_general_heatmap_cells(series, |geometry| {
                        consider(
                            geometry.row,
                            distance_to_rect(
                                x_css,
                                y_css,
                                GeneralColumnGeometry {
                                    row: geometry.row,
                                    left: geometry.left,
                                    right: geometry.right,
                                    top: geometry.top,
                                    bottom: geometry.bottom,
                                    end_at_min: false,
                                    exposed_end: true,
                                },
                            ),
                        );
                    })
                }
                GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
                    let expansion = if series.kind == GeneralSeriesKind::Bubble {
                        MAX_GENERAL_POINT_RADIUS
                    } else {
                        series.point_radius
                    } + max_distance;
                    let _ = self.with_scatter_spatial_index(series, |index| {
                        index.visit_candidates(x_css, y_css, expansion, |geometry| {
                            consider(
                                geometry.row,
                                distance_to_point_symbol(
                                    x_css,
                                    y_css,
                                    geometry.x,
                                    geometry.y,
                                    geometry.radius,
                                    if series.kind == GeneralSeriesKind::Bubble {
                                        GeneralPointSymbol::Circle
                                    } else {
                                        dataset
                                            .row_symbol(geometry.row)
                                            .unwrap_or(series.point_symbol)
                                    },
                                ),
                            );
                        });
                    });
                }
            }
            if matches!(mode, GeneralHitMode::Exact)
                && best.as_ref().is_some_and(|hit| hit.distance == 0.0)
            {
                break;
            }
        }
        best
    }

    #[doc(hidden)]
    pub fn update_general_hover(
        &mut self,
        pane_index: usize,
        x_css: f64,
        y_css: f64,
    ) -> Option<GeneralSeriesHit> {
        let dimension = self.general_axis_hover_dimension(pane_index);
        let hit = match dimension {
            Some(dimension) => self.general_axis_hover_hit(pane_index, x_css, y_css, dimension),
            None => self.general_hit_test(pane_index, x_css, y_css, GeneralHitMode::Exact),
        };
        let next = hit.as_ref().map(GeneralInteractionTarget::from);
        let axis = dimension.is_some();
        let changed = self.general_series.as_ref().is_some_and(|registry| {
            registry.hovered != next || (next.is_some() && registry.hover_axis != axis)
        });
        if let Some(registry) = self.general_series.as_mut() {
            registry.hovered = next;
            registry.hover_axis = axis;
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        hit
    }

    #[doc(hidden)]
    pub fn clear_general_hover(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.hovered.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    #[doc(hidden)]
    pub fn general_hovered_hit(&self) -> Option<GeneralSeriesHit> {
        self.general_series
            .as_ref()?
            .hovered
            .as_ref()
            .map(GeneralInteractionTarget::hit)
    }

    /// Whether the hovered target is an index-axis hover anchor (cursor, dots, shared tooltip)
    /// rather than an exact mark hit (hover ring, item tooltip).
    pub(crate) fn general_hover_is_axis(&self) -> bool {
        self.general_series
            .as_ref()
            .is_some_and(|registry| registry.hover_axis && registry.hovered.is_some())
    }

    /// The rows an axis-mode hover activates in `pane_index`: every line or area row sharing
    /// the anchor's index datum. They draw an active dot like the financial crosshair marker.
    pub(crate) fn general_axis_hover_dot_rows(
        &self,
        pane_index: usize,
    ) -> Vec<(GeneralSeriesId, usize)> {
        let mut rows = Vec::new();
        if !self.general_hover_is_axis() {
            return rows;
        }
        let Some((anchor_id, anchor_row)) = self.general_hovered_key() else {
            return rows;
        };
        let Some(anchor) = self.general_series(anchor_id) else {
            return rows;
        };
        let Some(anchor_dataset) = self.general_dataset(anchor.dataset) else {
            return rows;
        };
        if self.pane_index_for_id(anchor.pane_id) != Some(pane_index) {
            return rows;
        }
        for series in self.general_series_iter().filter(|series| {
            series.visible
                && series.pane_id == anchor.pane_id
                && matches!(
                    series.kind,
                    GeneralSeriesKind::XyLine
                        | GeneralSeriesKind::XyArea
                        | GeneralSeriesKind::RangeArea
                )
        }) {
            let Some(dataset) = self.general_dataset(series.dataset) else {
                continue;
            };
            for row in 0..dataset.len() {
                if general_rows_share_horizontal_datum(anchor_dataset, anchor_row, dataset, row) {
                    rows.push((series.id, row));
                    if rows.len() >= MAX_GENERAL_SHARED_TOOLTIP_ITEMS {
                        return rows;
                    }
                }
            }
        }
        rows
    }

    pub(crate) fn general_hovered_key(&self) -> Option<(GeneralSeriesId, usize)> {
        self.general_series
            .as_ref()?
            .hovered
            .as_ref()
            .map(|target| (target.series, target.row))
    }

    #[doc(hidden)]
    pub fn select_general_hovered(&mut self) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        let next = registry.hovered.clone();
        let hit = next.is_some();
        if registry.selected != next {
            registry.selected = next;
            self.invalidate_frame_overlay();
        }
        hit
    }

    #[doc(hidden)]
    pub fn clear_general_selection(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.selected.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    #[doc(hidden)]
    pub fn general_selected_hit(&self) -> Option<GeneralSeriesHit> {
        self.general_series
            .as_ref()?
            .selected
            .as_ref()
            .map(GeneralInteractionTarget::hit)
    }

    pub(crate) fn select_general_row(&mut self, series_id: GeneralSeriesId, row: usize) -> bool {
        let Some(series) = self
            .general_series(series_id)
            .filter(|series| series.visible)
        else {
            return false;
        };
        let Some(row_id) = self.general_series_row_identity(series, row) else {
            return false;
        };
        let next = Some(GeneralInteractionTarget {
            series: series_id,
            row,
            row_id,
            distance: 0.0,
        });
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        if registry.selected != next {
            registry.selected = next;
            self.invalidate_frame_overlay();
        }
        true
    }

    #[doc(hidden)]
    pub fn set_general_brush_from_pixels(
        &mut self,
        axis_id: &str,
        from_css: f64,
        to_css: f64,
    ) -> Result<GeneralBrushSnapshot, ChartError> {
        self.update_general_brush_from_pixels(axis_id, from_css, to_css)?;
        self.general_brush_snapshot()
            .ok_or_else(|| invalid("general brush could not resolve its snapshot"))
    }

    /// Update the semantic brush during an input gesture without enumerating selected rows.
    pub(crate) fn update_general_brush_from_pixels(
        &mut self,
        axis_id: &str,
        from_css: f64,
        to_css: f64,
    ) -> Result<(), ChartError> {
        if !from_css.is_finite() || !to_css.is_finite() {
            return Err(invalid(
                "general brush coordinates must be finite CSS-pixel values",
            ));
        }
        let (pane_id, dimension, scale_type, reverse, padding_inner, padding_outer, domain, plot) = {
            let axis = self.general_axis(axis_id).ok_or_else(|| {
                ChartError::new(ErrorCode::InvalidHandle, "general axis is stale")
            })?;
            if !matches!(axis.dimension(), AxisDimension::X | AxisDimension::Y) {
                return Err(invalid("general brushes require a Cartesian X or Y axis"));
            }
            let pane_id = axis.pane_id();
            let pane = self
                .pane_index_for_id(pane_id)
                .ok_or_else(|| invalid("general brush axis references a stale pane"))?;
            let plot = self
                .general_plot_rect(pane)
                .ok_or_else(|| invalid("general brush pane has no plot rectangle"))?;
            let domain = self
                .effective_general_axis_domain(axis)
                .ok_or_else(|| invalid("general brush axis has no effective domain"))?;
            (
                pane_id,
                axis.dimension(),
                axis.scale(),
                axis.range_reversed(),
                axis.band_padding_inner(),
                axis.band_padding_outer(),
                domain,
                plot,
            )
        };
        let (range_from, range_to, coordinate_min, coordinate_max) = match dimension {
            AxisDimension::X => {
                let (from, to) = if reverse {
                    (plot.width, 0.0)
                } else {
                    (0.0, plot.width)
                };
                (from, to, 0.0, plot.width)
            }
            AxisDimension::Y => {
                let bottom = plot.y + plot.height;
                let (from, to) = if reverse {
                    (plot.y, bottom)
                } else {
                    (bottom, plot.y)
                };
                (from, to, plot.y, bottom)
            }
            AxisDimension::Angle | AxisDimension::Radius => unreachable!(),
        };
        let from_css = from_css.clamp(coordinate_min, coordinate_max);
        let to_css = to_css.clamp(coordinate_min, coordinate_max);
        let range = match domain {
            GeneralAxisDomain::Numeric(domain) => {
                let scale = NumericAxisScale::new(scale_type, domain, range_from, range_to)
                    .ok_or_else(|| invalid("general brush numeric transform is invalid"))?;
                let from = scale.invert(from_css).ok_or_else(|| {
                    invalid("general brush start is outside the numeric transform")
                })?;
                let to = scale
                    .invert(to_css)
                    .ok_or_else(|| invalid("general brush end is outside the numeric transform"))?;
                GeneralBrushRange::Numeric([from.min(to), from.max(to)])
            }
            GeneralAxisDomain::Temporal(domain) => {
                let scale =
                    LinearScale::new(domain[0] as f64, domain[1] as f64, range_from, range_to)
                        .map_err(|_| invalid("general brush temporal transform is invalid"))?;
                let from = scale
                    .invert(from_css)
                    .ok_or_else(|| {
                        invalid("general brush start is outside the temporal transform")
                    })?
                    .round() as i64;
                let to = scale
                    .invert(to_css)
                    .ok_or_else(|| invalid("general brush end is outside the temporal transform"))?
                    .round() as i64;
                GeneralBrushRange::Temporal([from.min(to), from.max(to)])
            }
            GeneralAxisDomain::Category(categories) => {
                if categories.is_empty() {
                    return Err(invalid("general brush category axis has no categories"));
                }
                let coordinate_for = |index: usize| -> Option<f64> {
                    if let Some(moved) = self
                        .general_axis(axis_id)
                        .and_then(|axis| {
                            self.category_motion_bounds(
                                axis,
                                &categories[index],
                                range_from,
                                range_to,
                            )
                        })
                        .map(|(first, second)| (first + second) / 2.0)
                    {
                        return Some(moved);
                    }
                    match scale_type {
                        GeneralScaleType::Band => BandScale::new(
                            categories.len(),
                            range_from,
                            range_to,
                            padding_inner,
                            padding_outer,
                            0.5,
                        )
                        .ok()?
                        .center(index),
                        GeneralScaleType::Point => PointScale::new(
                            categories.len(),
                            range_from,
                            range_to,
                            padding_outer,
                            0.5,
                        )
                        .ok()?
                        .coordinate(index),
                        _ => None,
                    }
                };
                let nearest = |coordinate: f64| -> Option<usize> {
                    (0..categories.len())
                        .filter_map(|index| {
                            coordinate_for(index).map(|mapped| (index, (mapped - coordinate).abs()))
                        })
                        .min_by(|left, right| left.1.total_cmp(&right.1))
                        .map(|(index, _)| index)
                };
                let from = nearest(from_css)
                    .ok_or_else(|| invalid("general brush category transform is invalid"))?;
                let to = nearest(to_css)
                    .ok_or_else(|| invalid("general brush category transform is invalid"))?;
                let (from, to) = if from <= to { (from, to) } else { (to, from) };
                GeneralBrushRange::Category([categories[from].clone(), categories[to].clone()])
            }
            GeneralAxisDomain::Auto => {
                return Err(invalid("general brush axis has no resolved domain"));
            }
        };

        let registry = self
            .general_series
            .as_mut()
            .ok_or_else(|| invalid("general brush requires at least one general series"))?;
        registry.brush = Some(GeneralBrushSelection {
            pane_id,
            axis_id: axis_id.to_owned(),
            dimension,
            range,
        });
        self.invalidate_frame_overlay();
        Ok(())
    }

    #[doc(hidden)]
    pub fn clear_general_brush(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.brush.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    pub(crate) fn general_brush_state(&self) -> Option<GeneralBrushSelection> {
        self.general_series.as_ref()?.brush.clone()
    }

    pub(crate) fn restore_general_brush_state(
        &mut self,
        prior: Option<GeneralBrushSelection>,
    ) -> bool {
        let Some(registry) = self.general_series.as_mut() else {
            return false;
        };
        if registry.brush != prior {
            registry.brush = prior;
            self.invalidate_frame_overlay();
            return true;
        }
        false
    }

    /// Start an accessible brush at the full current axis domain without host projection math.
    pub fn select_full_general_brush(
        &mut self,
        axis_id: &str,
    ) -> Result<GeneralBrushSnapshot, ChartError> {
        let axis = self.general_axis(axis_id).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general brush axis is stale")
        })?;
        if !matches!(axis.dimension(), AxisDimension::X | AxisDimension::Y) {
            return Err(invalid("general brushes require a Cartesian X or Y axis"));
        }
        let pane_id = axis.pane_id();
        let dimension = axis.dimension();
        let range = match self
            .effective_general_axis_domain(axis)
            .ok_or_else(|| invalid("general brush axis has no effective domain"))?
        {
            GeneralAxisDomain::Numeric(bounds) => GeneralBrushRange::Numeric(bounds),
            GeneralAxisDomain::Temporal(bounds) => GeneralBrushRange::Temporal(bounds),
            GeneralAxisDomain::Category(categories) => GeneralBrushRange::Category([
                categories
                    .first()
                    .ok_or_else(|| invalid("general brush category axis has no categories"))?
                    .clone(),
                categories
                    .last()
                    .expect("the first category exists")
                    .clone(),
            ]),
            GeneralAxisDomain::Auto => return Err(invalid("general brush axis has no domain")),
        };
        let registry = self
            .general_series
            .as_mut()
            .ok_or_else(|| invalid("general brush requires at least one general series"))?;
        registry.brush = Some(GeneralBrushSelection {
            pane_id,
            axis_id: axis_id.to_owned(),
            dimension,
            range,
        });
        self.invalidate_frame_overlay();
        self.general_brush_snapshot()
            .ok_or_else(|| invalid("general brush could not resolve its snapshot"))
    }

    /// Set a validated semantic range without host pixel projection. Useful for controlled
    /// authoring and for restoring a range after a viewport change.
    pub fn set_general_brush_range(
        &mut self,
        axis_id: &str,
        range: GeneralBrushRange,
    ) -> Result<GeneralBrushSnapshot, ChartError> {
        let axis = self.general_axis(axis_id).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general brush axis is stale")
        })?;
        if !matches!(axis.dimension(), AxisDimension::X | AxisDimension::Y) {
            return Err(invalid("general brushes require a Cartesian X or Y axis"));
        }
        let domain = self
            .effective_general_axis_domain(axis)
            .ok_or_else(|| invalid("general brush axis has no effective domain"))?;
        let valid = match (&range, domain) {
            (GeneralBrushRange::Numeric([from, to]), GeneralAxisDomain::Numeric(_)) => {
                from.is_finite()
                    && to.is_finite()
                    && from <= to
                    && (axis.scale() != GeneralScaleType::Logarithmic || *from > 0.0)
            }
            (GeneralBrushRange::Temporal([from, to]), GeneralAxisDomain::Temporal(_)) => {
                from <= to
                    && from.unsigned_abs() <= crate::MAX_GENERAL_TEMPORAL_MILLISECONDS as u64
                    && to.unsigned_abs() <= crate::MAX_GENERAL_TEMPORAL_MILLISECONDS as u64
            }
            (GeneralBrushRange::Category([from, to]), GeneralAxisDomain::Category(_)) => {
                let Some(GeneralAxisDomain::Category(categories)) =
                    self.base_general_axis_domain(axis)
                else {
                    return Err(invalid("general brush category axis has no base domain"));
                };
                let start = categories.iter().position(|value| value == from);
                let end = categories.iter().position(|value| value == to);
                matches!((start, end), (Some(start), Some(end)) if start <= end)
            }
            _ => false,
        };
        if !valid {
            return Err(invalid(
                "general brush range must match its axis and be ordered",
            ));
        }
        let next = Some(GeneralBrushSelection {
            pane_id: axis.pane_id(),
            axis_id: axis_id.to_owned(),
            dimension: axis.dimension(),
            range,
        });
        self.restore_general_brush_state(next);
        self.general_brush_snapshot()
            .ok_or_else(|| invalid("general brush could not resolve its snapshot"))
    }

    /// Move one brush endpoint by one percent of the current transformed span, or one category.
    pub fn nudge_general_brush(
        &mut self,
        endpoint: GeneralBrushEndpoint,
        direction: i8,
    ) -> Result<GeneralBrushSnapshot, ChartError> {
        if !matches!(direction, -1 | 1) {
            return Err(invalid("general brush nudge direction must be -1 or 1"));
        }
        let mut brush = self
            .general_brush_state()
            .ok_or_else(|| invalid("general brush has no active range"))?;
        let axis = self.general_axis(&brush.axis_id).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general brush axis is stale")
        })?;
        let domain = self
            .effective_general_axis_domain(axis)
            .ok_or_else(|| invalid("general brush axis has no effective domain"))?;
        match (&mut brush.range, domain) {
            (GeneralBrushRange::Numeric(range), GeneralAxisDomain::Numeric(domain)) => {
                let scale = NumericAxisScale::new(axis.scale(), domain, 0.0, 1.0)
                    .ok_or_else(|| invalid("general brush numeric transform is invalid"))?;
                let index = usize::from(endpoint == GeneralBrushEndpoint::End);
                let position = scale.coordinate(range[index]).ok_or_else(|| {
                    invalid("general brush endpoint is outside the numeric transform")
                })?;
                let next = scale
                    .invert((position + f64::from(direction) * 0.01).clamp(0.0, 1.0))
                    .ok_or_else(|| invalid("general brush nudge exceeds the numeric transform"))?;
                range[index] = if index == 0 {
                    next.min(range[1])
                } else {
                    next.max(range[0])
                };
            }
            (GeneralBrushRange::Temporal(range), GeneralAxisDomain::Temporal(domain)) => {
                let scale = LinearScale::new(domain[0] as f64, domain[1] as f64, 0.0, 1.0)
                    .map_err(|_| invalid("general brush temporal transform is invalid"))?;
                let index = usize::from(endpoint == GeneralBrushEndpoint::End);
                let position = scale.coordinate(range[index] as f64).ok_or_else(|| {
                    invalid("general brush endpoint is outside the temporal transform")
                })?;
                let next = scale
                    .invert((position + f64::from(direction) * 0.01).clamp(0.0, 1.0))
                    .ok_or_else(|| invalid("general brush nudge exceeds the temporal transform"))?
                    .round() as i64;
                range[index] = if index == 0 {
                    next.min(range[1])
                } else {
                    next.max(range[0])
                };
            }
            (GeneralBrushRange::Category(range), GeneralAxisDomain::Category(_)) => {
                let GeneralAxisDomain::Category(categories) =
                    self.base_general_axis_domain(axis)
                        .ok_or_else(|| invalid("general brush category axis has no base domain"))?
                else {
                    return Err(invalid("general brush category axis has no base domain"));
                };
                let index = usize::from(endpoint == GeneralBrushEndpoint::End);
                let current = categories
                    .iter()
                    .position(|value| value == &range[index])
                    .ok_or_else(|| {
                        invalid("general brush endpoint category is no longer present")
                    })?;
                let other = categories
                    .iter()
                    .position(|value| value == &range[1 - index])
                    .ok_or_else(|| {
                        invalid("general brush endpoint category is no longer present")
                    })?;
                let next = current
                    .saturating_add_signed(isize::from(direction))
                    .min(categories.len() - 1);
                let next = if index == 0 {
                    next.min(other)
                } else {
                    next.max(other)
                };
                range[index] = categories[next].clone();
            }
            _ => return Err(invalid("general brush range no longer matches its axis")),
        }
        self.restore_general_brush_state(Some(brush));
        self.general_brush_snapshot()
            .ok_or_else(|| invalid("general brush could not resolve its snapshot"))
    }

    #[doc(hidden)]
    pub fn general_brush_snapshot(&self) -> Option<GeneralBrushSnapshot> {
        let registry = self.general_series.as_ref()?;
        let brush = registry.brush.as_ref()?;
        let pane = self.pane_index_for_id(brush.pane_id)?;
        let axis = self.general_axis(&brush.axis_id)?;
        let domain = self.effective_general_axis_domain(axis)?;
        let mut items = Vec::new();
        for series in registry.series.iter().filter(|series| {
            series.visible
                && series.pane_id == brush.pane_id
                && match brush.dimension {
                    AxisDimension::X => series.x_axis_id == brush.axis_id,
                    AxisDimension::Y => series.y_axis_id == brush.axis_id,
                    AxisDimension::Angle | AxisDimension::Radius => false,
                }
        }) {
            let Some(dataset) = self.general_dataset(series.dataset) else {
                continue;
            };
            let histogram = self.general_histogram_bins(series);
            let rows = histogram.as_ref().map_or(dataset.len(), |bins| bins.len());
            for row in 0..rows {
                let matched = match histogram.as_ref() {
                    Some(bins) => histogram_bin_matches_brush(bins, row, brush),
                    None => general_row_matches_brush(series, dataset, row, brush, &domain),
                };
                if !matched {
                    continue;
                }
                let Some(row_id) = (match histogram {
                    Some(_) => Some(histogram_bin_identity(row)),
                    None => dataset.row_identity(row).cloned(),
                }) else {
                    continue;
                };
                items.push(GeneralSeriesHit {
                    series: series.id,
                    row,
                    row_id,
                    distance: 0.0,
                });
                if items.len() >= MAX_GENERAL_BRUSH_ITEMS {
                    return Some(GeneralBrushSnapshot {
                        pane,
                        axis_id: brush.axis_id.clone(),
                        dimension: brush.dimension,
                        range: brush.range.clone(),
                        items,
                    });
                }
            }
        }
        Some(GeneralBrushSnapshot {
            pane,
            axis_id: brush.axis_id.clone(),
            dimension: brush.dimension,
            range: brush.range.clone(),
            items,
        })
    }

    pub(crate) fn general_brush_axis_bounds(
        &self,
        pane_index: usize,
    ) -> Option<(AxisDimension, f64, f64)> {
        let registry = self.general_series.as_ref()?;
        let brush = registry.brush.as_ref()?;
        if self.pane_index_for_id(brush.pane_id)? != pane_index {
            return None;
        }
        let axis = self.general_axis(&brush.axis_id)?;
        let domain = self.effective_general_axis_domain(axis)?;
        let plot = self.general_plot_rect(pane_index)?;
        let (range_from, range_to) = match brush.dimension {
            AxisDimension::X => {
                if axis.range_reversed() {
                    (plot.width, 0.0)
                } else {
                    (0.0, plot.width)
                }
            }
            AxisDimension::Y => {
                let bottom = plot.y + plot.height;
                if axis.range_reversed() {
                    (plot.y, bottom)
                } else {
                    (bottom, plot.y)
                }
            }
            AxisDimension::Angle | AxisDimension::Radius => return None,
        };
        let bounds = match (&brush.range, domain) {
            (GeneralBrushRange::Numeric(values), GeneralAxisDomain::Numeric(domain)) => {
                let scale = NumericAxisScale::new(axis.scale(), domain, range_from, range_to)?;
                (scale.coordinate(values[0])?, scale.coordinate(values[1])?)
            }
            (GeneralBrushRange::Temporal(values), GeneralAxisDomain::Temporal(domain)) => {
                let scale =
                    LinearScale::new(domain[0] as f64, domain[1] as f64, range_from, range_to)
                        .ok()?;
                (
                    scale.coordinate(values[0] as f64)?,
                    scale.coordinate(values[1] as f64)?,
                )
            }
            (GeneralBrushRange::Category(values), GeneralAxisDomain::Category(categories)) => {
                let from = categories.iter().position(|value| value == &values[0])?;
                let to = categories.iter().position(|value| value == &values[1])?;
                match axis.scale() {
                    GeneralScaleType::Band => {
                        let scale = BandScale::new(
                            categories.len(),
                            range_from,
                            range_to,
                            axis.band_padding_inner(),
                            axis.band_padding_outer(),
                            0.5,
                        )
                        .ok()?;
                        let from = self
                            .category_motion_bounds(axis, &values[0], range_from, range_to)
                            .or_else(|| scale.bounds(from))?;
                        let to = self
                            .category_motion_bounds(axis, &values[1], range_from, range_to)
                            .or_else(|| scale.bounds(to))?;
                        (
                            from.0.min(from.1).min(to.0.min(to.1)),
                            from.0.max(from.1).max(to.0.max(to.1)),
                        )
                    }
                    GeneralScaleType::Point => {
                        let scale = PointScale::new(
                            categories.len(),
                            range_from,
                            range_to,
                            axis.band_padding_outer(),
                            0.5,
                        )
                        .ok()?;
                        let center = |index: usize, category: &str| {
                            self.category_motion_bounds(axis, category, range_from, range_to)
                                .map(|(first, second)| (first + second) / 2.0)
                                .or_else(|| scale.coordinate(index))
                        };
                        (center(from, &values[0])?, center(to, &values[1])?)
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some((
            brush.dimension,
            bounds.0.min(bounds.1),
            bounds.0.max(bounds.1),
        ))
    }

    #[doc(hidden)]
    pub fn set_general_accessibility_focus(
        &mut self,
        series_id: GeneralSeriesId,
        row: usize,
    ) -> bool {
        let Some(series) = self.general_series(series_id) else {
            return false;
        };
        let Some(row_id) = self.general_series_row_identity(series, row) else {
            return false;
        };
        let next = Some(GeneralInteractionTarget {
            series: series_id,
            row,
            row_id,
            distance: 0.0,
        });
        let changed = self
            .general_series
            .as_ref()
            .is_some_and(|registry| registry.accessibility_focused != next);
        if let Some(registry) = self.general_series.as_mut() {
            registry.accessibility_focused = next;
        }
        if changed {
            self.invalidate_frame_overlay();
        }
        true
    }

    #[doc(hidden)]
    pub fn clear_general_accessibility_focus(&mut self) {
        let changed = self
            .general_series
            .as_mut()
            .is_some_and(|registry| registry.accessibility_focused.take().is_some());
        if changed {
            self.invalidate_frame_overlay();
        }
    }

    #[doc(hidden)]
    pub fn general_accessibility_focused_hit(&self) -> Option<GeneralSeriesHit> {
        self.general_series
            .as_ref()?
            .accessibility_focused
            .as_ref()
            .map(GeneralInteractionTarget::hit)
    }

    pub(crate) fn general_row_interaction(
        &self,
        series: GeneralSeriesId,
        row: usize,
    ) -> (bool, bool) {
        let Some(registry) = self.general_series.as_ref() else {
            return (false, false);
        };
        let matches =
            |target: &GeneralInteractionTarget| target.series == series && target.row == row;
        (
            (!registry.hover_axis && registry.hovered.as_ref().is_some_and(matches))
                || registry.accessibility_focused.as_ref().is_some_and(matches),
            registry.selected.as_ref().is_some_and(matches),
        )
    }

    pub(crate) fn reconcile_general_interaction_for_dataset(
        &mut self,
        dataset_id: GeneralDatasetId,
        removed_front: usize,
    ) {
        let Some(registry) = self.general_series.as_ref() else {
            return;
        };
        let dataset_series: Vec<GeneralSeriesId> = registry
            .series
            .iter()
            .filter(|series| series.dataset == dataset_id)
            .map(GeneralSeries::id)
            .collect();
        // Histogram targets name engine bins rather than dataset rows; a bin survives a data
        // change while its index still exists.
        let histogram_bins: HashMap<GeneralSeriesId, usize> = registry
            .series
            .iter()
            .filter(|series| {
                series.dataset == dataset_id && series.kind == GeneralSeriesKind::Histogram
            })
            .map(|series| {
                (
                    series.id,
                    self.general_histogram_bins(series)
                        .map_or(0, |bins| bins.len()),
                )
            })
            .collect();
        let previous_hovered = registry.hovered.clone();
        let previous_selected = registry.selected.clone();
        let previous_accessibility_focused = registry.accessibility_focused.clone();
        let Some(dataset) = self.general_dataset(dataset_id) else {
            return;
        };
        let reconcile = |target: Option<GeneralInteractionTarget>| {
            let mut current = target?;
            if let Some(&bins) = histogram_bins.get(&current.series) {
                return (current.row < bins).then_some(current);
            }
            if dataset_series.contains(&current.series) {
                let shifted = current.row.checked_sub(removed_front);
                current.row = shifted
                    .filter(|&row| dataset.row_identity(row) == Some(&current.row_id))
                    .or_else(|| {
                        (0..dataset.len())
                            .find(|&row| dataset.row_identity(row) == Some(&current.row_id))
                    })?;
            }
            Some(current)
        };
        let next_hovered = reconcile(previous_hovered.clone());
        let next_selected = reconcile(previous_selected.clone());
        let next_accessibility_focused = reconcile(previous_accessibility_focused.clone());
        if previous_hovered != next_hovered
            || previous_selected != next_selected
            || previous_accessibility_focused != next_accessibility_focused
        {
            if let Some(registry) = self.general_series.as_mut() {
                registry.hovered = next_hovered;
                registry.selected = next_selected;
                registry.accessibility_focused = next_accessibility_focused;
            }
            self.invalidate_frame_overlay();
        }
    }

    #[cfg(test)]
    pub(crate) fn general_scatter_hit_candidate_count(
        &self,
        series_id: GeneralSeriesId,
        x_css: f64,
        y_css: f64,
        max_distance: f64,
    ) -> Option<usize> {
        let series = self.general_series(series_id)?;
        if !matches!(
            series.kind,
            GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble
        ) || !x_css.is_finite()
            || !y_css.is_finite()
            || !max_distance.is_finite()
            || max_distance < 0.0
        {
            return None;
        }
        self.with_scatter_spatial_index(series, |index| {
            let mut count = 0usize;
            let expansion = if series.kind == GeneralSeriesKind::Bubble {
                MAX_GENERAL_POINT_RADIUS
            } else {
                series.point_radius
            } + max_distance;
            index.visit_candidates(x_css, y_css, expansion, |_| count += 1);
            count
        })
    }

    /// Identity of an interaction row: a dataset row, or an engine bin for a histogram.
    fn general_series_row_identity(
        &self,
        series: &GeneralSeries,
        row: usize,
    ) -> Option<GeneralRowIdentity> {
        if series.kind == GeneralSeriesKind::Histogram {
            return (row < self.general_histogram_bins(series)?.len())
                .then(|| histogram_bin_identity(row));
        }
        self.general_dataset(series.dataset)?
            .row_identity(row)
            .cloned()
    }

    fn general_histogram_tooltip_snapshot(
        &self,
        series: &GeneralSeries,
        bin: usize,
    ) -> Option<GeneralTooltipSnapshot> {
        let bins = self.general_histogram_bins(series)?;
        let (from, to) = bins.bounds(bin)?;
        let color = series
            .color()
            .and_then(Color::parse_css)
            .unwrap_or(crate::DEFAULT_LINE_COLOR);
        Some(GeneralTooltipSnapshot {
            series: series.id,
            row: bin,
            row_id: histogram_bin_identity(bin),
            x_label: bins.label(bin)?,
            y_label: None,
            label: None,
            value: bins.height(bin),
            low: None,
            high: None,
            x_low: Some(from),
            x_high: Some(to),
            q1: None,
            q3: None,
            size: None,
            title: series.title.clone(),
            color: Color::rgb(color.r(), color.g(), color.b()),
        })
    }

    #[doc(hidden)]
    pub fn general_tooltip_snapshot(
        &self,
        series_id: GeneralSeriesId,
        row: usize,
    ) -> Option<GeneralTooltipSnapshot> {
        let series = self.general_series(series_id)?;
        if series.kind == GeneralSeriesKind::Histogram {
            return self.general_histogram_tooltip_snapshot(series, row);
        }
        let dataset = self.general_dataset(series.dataset)?;
        let row_id = dataset.row_identity(row)?.clone();
        let x_label = general_x_label(dataset, row)?;
        let y_label = (series.kind == GeneralSeriesKind::HeatmapGrid)
            .then(|| general_heatmap_y_label(dataset, row))
            .flatten();
        let is_box_plot = series.kind == GeneralSeriesKind::BoxPlot;
        let color = match series.kind {
            GeneralSeriesKind::Pie
            | GeneralSeriesKind::PolarArea
            | GeneralSeriesKind::RadialBar => general_polar_fill_color(series, dataset, row),
            _ => dataset.row_color(row).unwrap_or_else(|| {
                series
                    .color()
                    .and_then(Color::parse_css)
                    .unwrap_or(crate::DEFAULT_LINE_COLOR)
            }),
        };
        Some(GeneralTooltipSnapshot {
            series: series_id,
            row,
            row_id,
            x_label,
            y_label,
            label: dataset.row_label(row).map(str::to_owned),
            value: dataset.y_is_valid(row).then(|| dataset.y()[row]),
            low: dataset
                .low()
                .and_then(|values| dataset.low_is_valid(row).then(|| values[row])),
            high: dataset.high().map_or_else(
                || {
                    dataset
                        .low()
                        .and_then(|_| dataset.y_is_valid(row).then(|| dataset.y()[row]))
                },
                |values| dataset.high_is_valid(row).then(|| values[row]),
            ),
            x_low: (!is_box_plot)
                .then(|| {
                    dataset
                        .x_low()
                        .and_then(|values| dataset.x_low_is_valid(row).then(|| values[row]))
                })
                .flatten(),
            x_high: (!is_box_plot)
                .then(|| {
                    dataset
                        .x_high()
                        .and_then(|values| dataset.x_high_is_valid(row).then(|| values[row]))
                })
                .flatten(),
            q1: is_box_plot
                .then(|| {
                    dataset
                        .x_low()
                        .and_then(|values| dataset.x_low_is_valid(row).then(|| values[row]))
                })
                .flatten(),
            q3: is_box_plot
                .then(|| {
                    dataset
                        .x_high()
                        .and_then(|values| dataset.x_high_is_valid(row).then(|| values[row]))
                })
                .flatten(),
            size: dataset
                .size()
                .and_then(|values| dataset.size_is_valid(row).then(|| values[row])),
            title: series.title.clone(),
            color: Color::rgb(color.r(), color.g(), color.b()),
        })
    }

    #[doc(hidden)]
    pub fn general_shared_tooltip_snapshot(
        &self,
        series_id: GeneralSeriesId,
        row: usize,
    ) -> Option<GeneralSharedTooltipSnapshot> {
        let anchor_series = self.general_series(series_id)?;
        if !anchor_series.visible {
            return None;
        }
        let pane = self.pane_index_for_id(anchor_series.pane_id)?;
        // Histogram bins share no row datum with any other series.
        if anchor_series.kind == GeneralSeriesKind::Histogram {
            return Some(GeneralSharedTooltipSnapshot {
                pane,
                anchor_series: series_id,
                anchor_row: row,
                items: vec![self.general_histogram_tooltip_snapshot(anchor_series, row)?],
            });
        }
        let anchor_dataset = self.general_dataset(anchor_series.dataset)?;
        anchor_dataset.row_identity(row)?;
        let mut items = Vec::new();

        for candidate in self
            .general_series_iter()
            .filter(|candidate| candidate.visible && candidate.pane_id == anchor_series.pane_id)
        {
            let Some(dataset) = self.general_dataset(candidate.dataset) else {
                continue;
            };
            for candidate_row in 0..dataset.len() {
                if !general_rows_share_horizontal_datum(anchor_dataset, row, dataset, candidate_row)
                {
                    continue;
                }
                if let Some(snapshot) = self.general_tooltip_snapshot(candidate.id, candidate_row) {
                    items.push(snapshot);
                    if items.len() >= MAX_GENERAL_SHARED_TOOLTIP_ITEMS {
                        return Some(GeneralSharedTooltipSnapshot {
                            pane,
                            anchor_series: series_id,
                            anchor_row: row,
                            items,
                        });
                    }
                }
            }
        }

        Some(GeneralSharedTooltipSnapshot {
            pane,
            anchor_series: series_id,
            anchor_row: row,
            items,
        })
    }

    #[doc(hidden)]
    pub fn general_legend_snapshot(&self, pane: Option<usize>) -> GeneralLegendSnapshot {
        let pane_id = pane.and_then(|index| self.pane_stable_id(index));
        if pane.is_some() && pane_id.is_none() {
            return GeneralLegendSnapshot { items: Vec::new() };
        }
        let mut items = Vec::with_capacity(self.general_series_count().min(MAX_GENERAL_SERIES));
        let mut retained_slices = 0;
        for series in self.general_series_iter() {
            if pane_id.is_some_and(|id| series.pane_id != id) {
                continue;
            }
            let Some(pane) = self.pane_index_for_id(series.pane_id) else {
                continue;
            };
            let mut slices = Vec::new();
            let mut slices_truncated = false;
            if matches!(
                series.kind,
                GeneralSeriesKind::Pie
                    | GeneralSeriesKind::PolarArea
                    | GeneralSeriesKind::RadialBar
            ) && let Some(dataset) = self.general_dataset(series.dataset)
            {
                for row in 0..dataset.len() {
                    if !dataset.y_is_valid(row) || dataset.y()[row] <= 0.0 {
                        continue;
                    }
                    if slices.len() == MAX_GENERAL_LEGEND_SLICES_PER_SERIES
                        || retained_slices == MAX_GENERAL_LEGEND_SLICES_PER_SNAPSHOT
                    {
                        slices_truncated = true;
                        break;
                    }
                    slices.push(GeneralLegendSlice {
                        row,
                        label: dataset
                            .row_label(row)
                            .map(str::to_owned)
                            .or_else(|| general_x_label(dataset, row))
                            .unwrap_or_default(),
                        value: dataset.y()[row],
                        color: general_polar_fill_color(series, dataset, row).to_css(),
                    });
                    retained_slices += 1;
                }
            }
            items.push(GeneralLegendItem {
                series: series.id,
                pane,
                kind: series.kind,
                title: series.title.clone(),
                color: series.color.clone(),
                visible: series.visible,
                slices,
                slices_truncated,
            });
        }
        GeneralLegendSnapshot { items }
    }

    #[doc(hidden)]
    pub fn general_accessibility_snapshot(
        &self,
        series_id: GeneralSeriesId,
        offset: usize,
        limit: usize,
    ) -> Option<GeneralAccessibilitySnapshot> {
        let series = self.general_series(series_id)?;
        if series.kind == GeneralSeriesKind::Histogram {
            let bins = self.general_histogram_bins(series)?;
            let total_rows = bins.len();
            let offset = offset.min(total_rows);
            let end = offset
                .saturating_add(limit.min(MAX_GENERAL_ACCESSIBILITY_ITEMS))
                .min(total_rows);
            let items = (offset..end)
                .map(|bin| {
                    let (from, to) = bins.bounds(bin)?;
                    Some(GeneralAccessibilityItem {
                        row: bin,
                        row_id: histogram_bin_identity(bin),
                        x_label: bins.label(bin)?,
                        y_label: None,
                        label: None,
                        value: bins.height(bin),
                        low: None,
                        high: None,
                        x_low: Some(from),
                        x_high: Some(to),
                        q1: None,
                        q3: None,
                        size: None,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            return Some(GeneralAccessibilitySnapshot {
                series: series_id,
                title: series.title.clone(),
                total_rows,
                offset,
                items,
            });
        }
        let dataset = self.general_dataset(series.dataset)?;
        let total_rows = dataset.len();
        let offset = offset.min(total_rows);
        let end = offset
            .saturating_add(limit.min(MAX_GENERAL_ACCESSIBILITY_ITEMS))
            .min(total_rows);
        let mut items = Vec::with_capacity(end - offset);
        for row in offset..end {
            let row_id = dataset.row_identity(row)?.clone();
            let x_label = general_x_label(dataset, row)?;
            let y_label = (series.kind == GeneralSeriesKind::HeatmapGrid)
                .then(|| general_heatmap_y_label(dataset, row))
                .flatten();
            let is_box_plot = series.kind == GeneralSeriesKind::BoxPlot;
            items.push(GeneralAccessibilityItem {
                row,
                row_id,
                x_label,
                y_label,
                label: dataset.row_label(row).map(str::to_owned),
                value: dataset.y_is_valid(row).then(|| dataset.y()[row]),
                low: dataset
                    .low()
                    .and_then(|values| dataset.low_is_valid(row).then(|| values[row])),
                high: dataset.high().map_or_else(
                    || {
                        dataset
                            .low()
                            .and_then(|_| dataset.y_is_valid(row).then(|| dataset.y()[row]))
                    },
                    |values| dataset.high_is_valid(row).then(|| values[row]),
                ),
                x_low: (!is_box_plot)
                    .then(|| {
                        dataset
                            .x_low()
                            .and_then(|values| dataset.x_low_is_valid(row).then(|| values[row]))
                    })
                    .flatten(),
                x_high: (!is_box_plot)
                    .then(|| {
                        dataset
                            .x_high()
                            .and_then(|values| dataset.x_high_is_valid(row).then(|| values[row]))
                    })
                    .flatten(),
                q1: is_box_plot
                    .then(|| {
                        dataset
                            .x_low()
                            .and_then(|values| dataset.x_low_is_valid(row).then(|| values[row]))
                    })
                    .flatten(),
                q3: is_box_plot
                    .then(|| {
                        dataset
                            .x_high()
                            .and_then(|values| dataset.x_high_is_valid(row).then(|| values[row]))
                    })
                    .flatten(),
                size: dataset
                    .size()
                    .and_then(|values| dataset.size_is_valid(row).then(|| values[row])),
            });
        }
        Some(GeneralAccessibilitySnapshot {
            series: series_id,
            title: series.title.clone(),
            total_rows,
            offset,
            items,
        })
    }
}

fn distance_to_rect(x: f64, y: f64, geometry: GeneralColumnGeometry) -> f64 {
    let dx = if x < geometry.left {
        geometry.left - x
    } else if x > geometry.right {
        x - geometry.right
    } else {
        0.0
    };
    let dy = if y < geometry.top {
        geometry.top - y
    } else if y > geometry.bottom {
        y - geometry.bottom
    } else {
        0.0
    };
    dx.hypot(dy)
}

pub(crate) fn general_bar_radii(
    series: &GeneralSeries,
    geometry: GeneralColumnGeometry,
    hpr: f64,
    vpr: f64,
    horizontal: bool,
) -> Option<[f32; 4]> {
    if !geometry.exposed_end {
        return None;
    }
    let width = ((geometry.right * hpr).round() - (geometry.left * hpr).round()).max(1.0);
    let height = ((geometry.bottom * vpr).round() - (geometry.top * vpr).round()).max(1.0);
    let radius = (series.bar_corner_radius() * hpr.min(vpr)).min(width.min(height) * 0.5) as f32;
    if radius <= 0.0 {
        return None;
    }
    // Only the value end is rounded; the baseline end stays square against its axis or segment.
    Some(if horizontal {
        if geometry.end_at_min {
            [radius, 0.0, 0.0, radius]
        } else {
            [0.0, radius, radius, 0.0]
        }
    } else if geometry.end_at_min {
        [radius, radius, 0.0, 0.0]
    } else {
        [0.0, 0.0, radius, radius]
    })
}

fn distance_to_bar(
    x: f64,
    y: f64,
    geometry: GeneralColumnGeometry,
    series: &GeneralSeries,
    chart: &ChartEngine,
    horizontal: bool,
) -> f64 {
    let hpr = (chart.pane_w * chart.dpr).round().max(1.0) / chart.pane_w.max(1.0);
    let vpr = (chart.pane_h * chart.dpr).round().max(1.0) / chart.pane_h.max(1.0);
    let Some(radii) = general_bar_radii(series, geometry, hpr, vpr, horizontal) else {
        return distance_to_rect(x, y, geometry);
    };
    let left = (geometry.left * hpr).round();
    let right = (geometry.right * hpr).round();
    let top = (geometry.top * vpr).round();
    let bottom = (geometry.bottom * vpr).round();
    let px = x * hpr;
    let py = y * vpr;
    let rect_distance = (left - px)
        .max(0.0)
        .max(px - right)
        .hypot((top - py).max(0.0).max(py - bottom));
    let corner = if px < left + f64::from(radii[0]) && py < top + f64::from(radii[0]) {
        Some((
            left + f64::from(radii[0]),
            top + f64::from(radii[0]),
            radii[0],
        ))
    } else if px > right - f64::from(radii[1]) && py < top + f64::from(radii[1]) {
        Some((
            right - f64::from(radii[1]),
            top + f64::from(radii[1]),
            radii[1],
        ))
    } else if px > right - f64::from(radii[2]) && py > bottom - f64::from(radii[2]) {
        Some((
            right - f64::from(radii[2]),
            bottom - f64::from(radii[2]),
            radii[2],
        ))
    } else if px < left + f64::from(radii[3]) && py > bottom - f64::from(radii[3]) {
        Some((
            left + f64::from(radii[3]),
            bottom - f64::from(radii[3]),
            radii[3],
        ))
    } else {
        None
    };
    let corner_distance = corner.map_or(0.0, |(cx, cy, radius)| {
        ((px - cx).hypot(py - cy) - f64::from(radius)).max(0.0)
    });
    rect_distance.max(corner_distance) / hpr.min(vpr)
}

fn column_stack_matches(reference: &GeneralSeries, candidate: &GeneralSeries) -> bool {
    reference.kind == GeneralSeriesKind::Column
        && candidate.kind == GeneralSeriesKind::Column
        && reference.pane_id == candidate.pane_id
        && reference.x_axis_id == candidate.x_axis_id
        && reference.y_axis_id == candidate.y_axis_id
        && reference.group_id == candidate.group_id
        && reference.stack_id.is_some()
        && reference.stack_id == candidate.stack_id
        && reference.stack_mode == candidate.stack_mode
}

fn bar_end_exposed(
    series: &GeneralSeries,
    totals: Option<&[(f64, f64)]>,
    category: usize,
    raw_value: f64,
    end_value: f64,
) -> bool {
    if series.stack_id.is_none() {
        return true;
    }
    let Some(&(positive, negative_magnitude)) = totals.and_then(|values| values.get(category))
    else {
        return false;
    };
    let magnitude = if raw_value >= 0.0 {
        positive
    } else {
        negative_magnitude
    };
    let target = if series.stack_mode == GeneralStackMode::Percent {
        1.0
    } else {
        magnitude
    };
    target > 0.0 && end_value.abs() >= target - target.max(1.0) * 1e-12
}

fn bar_thickness_bounds(
    from: f64,
    to: f64,
    gap: f64,
    max_width: Option<f64>,
) -> Option<(f64, f64)> {
    let available = (to - from - gap).max(0.0);
    let width = max_width.map_or(available, |limit| available.min(limit));
    if width <= 0.0 {
        return None;
    }
    let center = from + (to - from) * 0.5;
    Some((center - width * 0.5, center + width * 0.5))
}

fn horizontal_bar_stack_matches(reference: &GeneralSeries, candidate: &GeneralSeries) -> bool {
    reference.kind == GeneralSeriesKind::HorizontalBar
        && candidate.kind == GeneralSeriesKind::HorizontalBar
        && reference.pane_id == candidate.pane_id
        && reference.x_axis_id == candidate.x_axis_id
        && reference.y_axis_id == candidate.y_axis_id
        && reference.group_id == candidate.group_id
        && reference.stack_id.is_some()
        && reference.stack_id == candidate.stack_id
        && reference.stack_mode == candidate.stack_mode
}

fn area_stack_matches(reference: &GeneralSeries, candidate: &GeneralSeries) -> bool {
    reference.kind == GeneralSeriesKind::XyArea
        && candidate.kind == GeneralSeriesKind::XyArea
        && reference.pane_id == candidate.pane_id
        && reference.x_axis_id == candidate.x_axis_id
        && reference.y_axis_id == candidate.y_axis_id
        && reference.stack_id.is_some()
        && reference.stack_id == candidate.stack_id
        && reference.stack_mode == candidate.stack_mode
}

fn general_stack_x_key(dataset: &crate::GeneralDataset, row: usize) -> Option<GeneralStackXKey> {
    match dataset.x_kind() {
        GeneralXKind::Numeric => {
            dataset
                .numeric_x()
                .and_then(|values| values.get(row))
                .map(|value| {
                    GeneralStackXKey::Numeric(if *value == 0.0 {
                        0.0f64.to_bits()
                    } else {
                        value.to_bits()
                    })
                })
        }
        GeneralXKind::Temporal => dataset
            .temporal_x_epoch_ms()
            .and_then(|values| values.get(row))
            .copied()
            .map(GeneralStackXKey::Temporal),
        GeneralXKind::Category => {
            let category_index = dataset
                .category_indices()
                .and_then(|values| values.get(row))
                .and_then(|value| usize::try_from(*value).ok())?;
            dataset
                .categories()
                .and_then(|values| values.get(category_index))
                .cloned()
                .map(GeneralStackXKey::Category)
        }
        GeneralXKind::Values => None,
    }
}

fn accumulate_area_values<F>(engine: &ChartEngine, series: &GeneralSeries, mut visit: F)
where
    F: FnMut(GeneralStackXKey, f64),
{
    let Some(dataset) = engine.general_dataset(series.dataset) else {
        return;
    };
    for (row, &value) in dataset.y().iter().enumerate() {
        if !dataset.y_is_valid(row) {
            continue;
        }
        let Some(x) = general_stack_x_key(dataset, row) else {
            continue;
        };
        visit(x, value);
    }
}

fn wiggle_baselines(layers: &[Vec<f64>]) -> Vec<f64> {
    let Some(width) = layers.first().map(Vec::len) else {
        return Vec::new();
    };
    let mut baselines = vec![0.0; width];
    for column in 1..width {
        let mut total = 0.0;
        let mut weighted_change = 0.0;
        let mut preceding_change = 0.0;
        for layer in layers {
            let current = layer[column];
            let change = current - layer[column - 1];
            total += current;
            weighted_change += current * (preceding_change + change * 0.5);
            preceding_change += change;
        }
        if total != 0.0 && total.is_finite() && weighted_change.is_finite() {
            let next = baselines[column - 1] - weighted_change / total;
            if next.is_finite() {
                baselines[column] = next;
                continue;
            }
        }
        baselines[column] = baselines[column - 1];
    }
    baselines
}

fn accumulate_column_values<F>(
    engine: &ChartEngine,
    series: &GeneralSeries,
    axis_lookup: &HashMap<&str, usize>,
    mut visit: F,
) where
    F: FnMut(usize, usize, f64),
{
    let Some(dataset) = engine.general_dataset(series.dataset) else {
        return;
    };
    let (Some(categories), Some(indices)) = (dataset.categories(), dataset.category_indices())
    else {
        return;
    };
    for (row, &category_index) in indices.iter().enumerate() {
        if !dataset.y_is_valid(row) {
            continue;
        }
        let Some(category) = usize::try_from(category_index)
            .ok()
            .and_then(|index| categories.get(index))
        else {
            continue;
        };
        let Some(&axis_index) = axis_lookup.get(category.as_str()) else {
            continue;
        };
        visit(axis_index, row, dataset.y()[row]);
    }
}

fn extend_numeric_pair(bounds: &mut Option<(f64, f64)>, value: f64) {
    *bounds = Some(match *bounds {
        Some((low, high)) => (low.min(value), high.max(value)),
        None => (value, value),
    });
}

fn distance_to_point_symbol(
    x: f64,
    y: f64,
    center_x: f64,
    center_y: f64,
    radius: f64,
    symbol: GeneralPointSymbol,
) -> f64 {
    let dx = x - center_x;
    let dy = y - center_y;
    match symbol {
        GeneralPointSymbol::Circle => (dx.hypot(dy) - radius).max(0.0),
        GeneralPointSymbol::Square => (dx.abs() - radius)
            .max(0.0)
            .hypot((dy.abs() - radius).max(0.0)),
        GeneralPointSymbol::Diamond => {
            if dx.abs() + dy.abs() <= radius {
                return 0.0;
            }
            let vertices = [
                (center_x, center_y - radius),
                (center_x + radius, center_y),
                (center_x, center_y + radius),
                (center_x - radius, center_y),
            ];
            vertices
                .iter()
                .zip(vertices.iter().cycle().skip(1))
                .take(vertices.len())
                .map(|(&(x0, y0), &(x1, y1))| distance_to_segment(x, y, x0, y0, x1, y1).0)
                .fold(f64::INFINITY, f64::min)
        }
        GeneralPointSymbol::Triangle => {
            let vertices = [
                (center_x, center_y - radius),
                (center_x + radius, center_y + radius),
                (center_x - radius, center_y + radius),
            ];
            let cross =
                |a: (f64, f64), b: (f64, f64)| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
            let signs = [
                cross(vertices[0], vertices[1]),
                cross(vertices[1], vertices[2]),
                cross(vertices[2], vertices[0]),
            ];
            if signs.iter().all(|value| *value >= 0.0) || signs.iter().all(|value| *value <= 0.0) {
                return 0.0;
            }
            vertices
                .iter()
                .zip(vertices.iter().cycle().skip(1))
                .take(vertices.len())
                .map(|(&(x0, y0), &(x1, y1))| distance_to_segment(x, y, x0, y0, x1, y1).0)
                .fold(f64::INFINITY, f64::min)
        }
    }
}

/// The span of an error bar along one axis: the center, its own error bounds, and the caps drawn
/// across it by the other axis's error bounds.
fn error_bar_extent(
    center: f64,
    low: Option<f64>,
    high: Option<f64>,
    crossed_by_caps: bool,
    cap_half_size: f64,
) -> [f64; 2] {
    let mut from = center;
    let mut to = center;
    for bound in [low, high].into_iter().flatten() {
        from = from.min(bound);
        to = to.max(bound);
    }
    if crossed_by_caps {
        from = from.min(center - cap_half_size);
        to = to.max(center + cap_half_size);
    }
    [from, to]
}

fn distance_to_error_bar(x: f64, y: f64, geometry: GeneralErrorBarGeometry) -> f64 {
    let mut distance = (x - geometry.x).hypot(y - geometry.y);
    if geometry.x_low.is_some() || geometry.x_high.is_some() {
        let from = geometry.x_low.unwrap_or(geometry.x);
        let to = geometry.x_high.unwrap_or(geometry.x);
        distance = distance.min(distance_to_segment(x, y, from, geometry.y, to, geometry.y).0);
        for bound in [geometry.x_low, geometry.x_high].into_iter().flatten() {
            distance = distance.min(
                distance_to_segment(
                    x,
                    y,
                    bound,
                    geometry.y - geometry.cap_half_size,
                    bound,
                    geometry.y + geometry.cap_half_size,
                )
                .0,
            );
        }
    }
    if geometry.y_low.is_some() || geometry.y_high.is_some() {
        let from = geometry.y_low.unwrap_or(geometry.y);
        let to = geometry.y_high.unwrap_or(geometry.y);
        distance = distance.min(distance_to_segment(x, y, geometry.x, from, geometry.x, to).0);
        for bound in [geometry.y_low, geometry.y_high].into_iter().flatten() {
            distance = distance.min(
                distance_to_segment(
                    x,
                    y,
                    geometry.x - geometry.cap_half_size,
                    bound,
                    geometry.x + geometry.cap_half_size,
                    bound,
                )
                .0,
            );
        }
    }
    distance
}

fn distance_to_box_plot(x: f64, y: f64, geometry: GeneralBoxPlotGeometry) -> f64 {
    let box_top = geometry.q1_y.min(geometry.q3_y);
    let box_bottom = geometry.q1_y.max(geometry.q3_y);
    if x >= geometry.left && x <= geometry.right && y >= box_top && y <= box_bottom {
        return 0.0;
    }
    [
        distance_to_segment(
            x,
            y,
            geometry.left,
            geometry.q1_y,
            geometry.right,
            geometry.q1_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.left,
            geometry.q3_y,
            geometry.right,
            geometry.q3_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.left,
            geometry.median_y,
            geometry.right,
            geometry.median_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.left,
            geometry.q1_y,
            geometry.left,
            geometry.q3_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.right,
            geometry.q1_y,
            geometry.right,
            geometry.q3_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.center_x,
            geometry.min_y,
            geometry.center_x,
            geometry.q1_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.center_x,
            geometry.q3_y,
            geometry.center_x,
            geometry.max_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.left,
            geometry.min_y,
            geometry.right,
            geometry.min_y,
        )
        .0,
        distance_to_segment(
            x,
            y,
            geometry.left,
            geometry.max_y,
            geometry.right,
            geometry.max_y,
        )
        .0,
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
}

fn distance_to_segment(x: f64, y: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if !length_squared.is_finite() || length_squared <= f64::EPSILON {
        return ((x - x0).hypot(y - y0), 0.0);
    }
    let position = (((x - x0) * dx + (y - y0) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest_x = x0 + dx * position;
    let nearest_y = y0 + dy * position;
    ((x - nearest_x).hypot(y - nearest_y), position)
}

fn nearest_line_row(points: &[GeneralLinePointGeometry], x: f64, y: f64) -> Option<usize> {
    points
        .iter()
        .min_by(|left, right| {
            (left.x - x)
                .hypot(left.y - y)
                .total_cmp(&(right.x - x).hypot(right.y - y))
        })
        .map(|point| point.row)
}

fn nearest_band_row(points: &[GeneralRangePointGeometry], x: f64, y: f64) -> Option<usize> {
    points
        .iter()
        .min_by(|left, right| {
            let left_y = (left.low_y + left.high_y) * 0.5;
            let right_y = (right.low_y + right.high_y) * 0.5;
            (left.x - x)
                .hypot(left_y - y)
                .total_cmp(&(right.x - x).hypot(right_y - y))
        })
        .map(|point| point.row)
}

fn distance_to_interpolated_line(
    x: f64,
    y: f64,
    points: &[GeneralLinePointGeometry],
    line_type: LineType,
) -> Option<(usize, f64)> {
    if points.len() < 2 {
        return None;
    }
    let source = points
        .iter()
        .map(|point| LinePoint {
            x: point.x,
            y: point.y,
        })
        .collect::<Vec<_>>();
    let mut expanded = Vec::new();
    expand_line_into(&source, line_type, 1.0, 1.0, &mut expanded);
    let mut best = (f64::INFINITY, 0.0, 0.0);
    for pair in expanded.windows(2) {
        let (distance, position) =
            distance_to_segment(x, y, pair[0].x, pair[0].y, pair[1].x, pair[1].y);
        if distance < best.0 {
            best = (
                distance,
                pair[0].x + (pair[1].x - pair[0].x) * position,
                pair[0].y + (pair[1].y - pair[0].y) * position,
            );
        }
    }
    nearest_line_row(points, best.1, best.2).map(|row| (row, best.0))
}

fn distance_to_interpolated_area(
    x: f64,
    y: f64,
    points: &[GeneralLinePointGeometry],
    baseline_y: f64,
    line_type: LineType,
) -> Option<(usize, f64)> {
    if points.len() < 2 {
        return None;
    }
    let source = points
        .iter()
        .map(|point| LinePoint {
            x: point.x,
            y: point.y,
        })
        .collect::<Vec<_>>();
    let mut expanded = Vec::new();
    expand_line_into(&source, line_type, 1.0, 1.0, &mut expanded);
    let mut best = (f64::INFINITY, 0.0, 0.0);
    for pair in expanded.windows(2) {
        let (distance, position) =
            distance_to_area_segment(x, y, pair[0].x, pair[0].y, pair[1].x, pair[1].y, baseline_y);
        if distance < best.0 {
            best = (
                distance,
                pair[0].x + (pair[1].x - pair[0].x) * position,
                pair[0].y + (pair[1].y - pair[0].y) * position,
            );
        }
    }
    nearest_line_row(points, best.1, best.2).map(|row| (row, best.0))
}

fn distance_to_interpolated_band(
    x: f64,
    y: f64,
    points: &[GeneralRangePointGeometry],
    line_type: LineType,
) -> Option<(usize, f64)> {
    if points.len() < 2 {
        return None;
    }
    let upper = points
        .iter()
        .map(|point| LinePoint {
            x: point.x,
            y: point.high_y,
        })
        .collect::<Vec<_>>();
    let lower = points
        .iter()
        .map(|point| LinePoint {
            x: point.x,
            y: point.low_y,
        })
        .collect::<Vec<_>>();
    let mut expanded_upper = Vec::new();
    let mut expanded_lower = Vec::new();
    expand_band_into(
        &upper,
        &lower,
        line_type,
        1.0,
        1.0,
        &mut expanded_upper,
        &mut expanded_lower,
    );
    let mut best = (f64::INFINITY, 0.0, 0.0);
    for index in 0..expanded_upper
        .len()
        .min(expanded_lower.len())
        .saturating_sub(1)
    {
        let upper_from = expanded_upper[index];
        let upper_to = expanded_upper[index + 1];
        let lower_from = expanded_lower[index];
        let lower_to = expanded_lower[index + 1];
        let (distance, position) = distance_to_band_segment(
            x,
            y,
            upper_from.x,
            lower_from.y,
            upper_from.y,
            upper_to.x,
            lower_to.y,
            upper_to.y,
        );
        if distance < best.0 {
            let low_y = lower_from.y + (lower_to.y - lower_from.y) * position;
            let high_y = upper_from.y + (upper_to.y - upper_from.y) * position;
            best = (
                distance,
                upper_from.x + (upper_to.x - upper_from.x) * position,
                (low_y + high_y) * 0.5,
            );
        }
    }
    nearest_band_row(points, best.1, best.2).map(|row| (row, best.0))
}

fn heatmap_center_bounds(
    center: f64,
    centers: &[f64],
    range_min: f64,
    range_max: f64,
) -> Option<(f64, f64)> {
    if !center.is_finite() || centers.is_empty() {
        return None;
    }
    let index = centers
        .binary_search_by(|candidate| candidate.total_cmp(&center))
        .ok()?;
    let low = if centers.len() == 1 {
        range_min
    } else if index == 0 {
        center - (centers[1] - center) * 0.5
    } else {
        (centers[index - 1] + center) * 0.5
    };
    let high = if centers.len() == 1 {
        range_max
    } else if index + 1 == centers.len() {
        center + (center - centers[index - 1]) * 0.5
    } else {
        (center + centers[index + 1]) * 0.5
    };
    let low = low.clamp(range_min, range_max);
    let high = high.clamp(range_min, range_max);
    (high > low).then_some((low, high))
}

/// Frame points are encoded as `f32`, so a pointer on a displayed fill edge can land a few
/// ulps outside the `f64` geometry; such points still count as inside the fill.
const FILL_EDGE_TOLERANCE: f64 = 1e-3;

fn distance_to_area_segment(
    x: f64,
    y: f64,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    baseline_y: f64,
) -> (f64, f64) {
    let (_, projected) = distance_to_segment(x, y, x0, y0, x1, y1);
    let position = if (x1 - x0).abs() > f64::EPSILON {
        ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
    } else {
        projected
    };
    let top_y = y0 + (y1 - y0) * position;
    if x >= x0.min(x1)
        && x <= x0.max(x1)
        && y >= top_y.min(baseline_y) - FILL_EDGE_TOLERANCE
        && y <= top_y.max(baseline_y) + FILL_EDGE_TOLERANCE
    {
        return (0.0, position);
    }
    let distance = [
        distance_to_segment(x, y, x0, y0, x1, y1).0,
        distance_to_segment(x, y, x0, baseline_y, x1, baseline_y).0,
        distance_to_segment(x, y, x0, y0, x0, baseline_y).0,
        distance_to_segment(x, y, x1, y1, x1, baseline_y).0,
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min);
    (distance, position)
}

#[allow(clippy::too_many_arguments)]
fn distance_to_band_segment(
    x: f64,
    y: f64,
    x0: f64,
    low0: f64,
    high0: f64,
    x1: f64,
    low1: f64,
    high1: f64,
) -> (f64, f64) {
    let (_, projected) = distance_to_segment(x, y, x0, high0, x1, high1);
    let position = if (x1 - x0).abs() > f64::EPSILON {
        ((x - x0) / (x1 - x0)).clamp(0.0, 1.0)
    } else {
        projected
    };
    let low_y = low0 + (low1 - low0) * position;
    let high_y = high0 + (high1 - high0) * position;
    if x >= x0.min(x1)
        && x <= x0.max(x1)
        && y >= low_y.min(high_y) - FILL_EDGE_TOLERANCE
        && y <= low_y.max(high_y) + FILL_EDGE_TOLERANCE
    {
        return (0.0, position);
    }
    let distance = [
        distance_to_segment(x, y, x0, low0, x1, low1).0,
        distance_to_segment(x, y, x0, high0, x1, high1).0,
        distance_to_segment(x, y, x0, low0, x0, high0).0,
        distance_to_segment(x, y, x1, low1, x1, high1).0,
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min);
    (distance, position)
}

fn general_x_label(dataset: &crate::GeneralDataset, row: usize) -> Option<String> {
    if let Some(values) = dataset.numeric_x() {
        return values.get(row).map(ToString::to_string);
    }
    if let Some(values) = dataset.temporal_x_epoch_ms() {
        return values.get(row).map(ToString::to_string);
    }
    let category_index = usize::try_from(*dataset.category_indices()?.get(row)?).ok()?;
    dataset.categories()?.get(category_index).cloned()
}

fn general_rows_share_horizontal_datum(
    left: &crate::GeneralDataset,
    left_row: usize,
    right: &crate::GeneralDataset,
    right_row: usize,
) -> bool {
    if left.x_kind() != right.x_kind() {
        return false;
    }
    match left.x_kind() {
        GeneralXKind::Numeric => left
            .numeric_x()
            .and_then(|values| values.get(left_row))
            .zip(right.numeric_x().and_then(|values| values.get(right_row)))
            .is_some_and(|(left, right)| left.to_bits() == right.to_bits()),
        GeneralXKind::Temporal => left
            .temporal_x_epoch_ms()
            .and_then(|values| values.get(left_row))
            .zip(
                right
                    .temporal_x_epoch_ms()
                    .and_then(|values| values.get(right_row)),
            )
            .is_some_and(|(left, right)| left == right),
        GeneralXKind::Category => left
            .category_indices()
            .and_then(|indices| indices.get(left_row))
            .and_then(|index| left.categories()?.get(*index as usize))
            .zip(
                right
                    .category_indices()
                    .and_then(|indices| indices.get(right_row))
                    .and_then(|index| right.categories()?.get(*index as usize)),
            )
            .is_some_and(|(left, right)| left == right),
        GeneralXKind::Values => false,
    }
}

fn histogram_bin_identity(bin: usize) -> GeneralRowIdentity {
    GeneralRowIdentity::Generated(bin as u64)
}

/// A bin matches an X brush when its center lies in the range and a Y brush by its height.
fn histogram_bin_matches_brush(
    bins: &GeneralHistogramBins,
    bin: usize,
    brush: &GeneralBrushSelection,
) -> bool {
    let GeneralBrushRange::Numeric([from, to]) = brush.range else {
        return false;
    };
    let value = match brush.dimension {
        AxisDimension::X => bins.bounds(bin).map(|(left, right)| (left + right) * 0.5),
        AxisDimension::Y => bins.height(bin),
        AxisDimension::Angle | AxisDimension::Radius => None,
    };
    value.is_some_and(|value| value >= from && value <= to)
}

fn general_row_matches_brush(
    series: &GeneralSeries,
    dataset: &crate::GeneralDataset,
    row: usize,
    brush: &GeneralBrushSelection,
    domain: &GeneralAxisDomain,
) -> bool {
    match (&brush.range, domain) {
        (GeneralBrushRange::Numeric([from, to]), GeneralAxisDomain::Numeric(_)) => {
            let value = match brush.dimension {
                AxisDimension::X => {
                    if series.kind == GeneralSeriesKind::HorizontalBar {
                        dataset.y_is_valid(row).then(|| dataset.y()[row])
                    } else {
                        dataset
                            .numeric_x()
                            .and_then(|values| values.get(row))
                            .copied()
                    }
                }
                AxisDimension::Y => {
                    if series.kind == GeneralSeriesKind::HorizontalBar {
                        None
                    } else if series.kind == GeneralSeriesKind::HeatmapGrid {
                        dataset
                            .heatmap_y_numeric()
                            .and_then(|values| values.get(row))
                            .copied()
                    } else {
                        dataset.y_is_valid(row).then(|| dataset.y()[row])
                    }
                }
                AxisDimension::Angle | AxisDimension::Radius => None,
            };
            value.is_some_and(|value| value >= *from && value <= *to)
        }
        (GeneralBrushRange::Temporal([from, to]), GeneralAxisDomain::Temporal(_)) => {
            if brush.dimension != AxisDimension::X {
                return false;
            }
            dataset
                .temporal_x_epoch_ms()
                .and_then(|values| values.get(row))
                .is_some_and(|value| value >= from && value <= to)
        }
        (GeneralBrushRange::Category([from, to]), GeneralAxisDomain::Category(categories)) => {
            let Some(from_index) = categories.iter().position(|value| value == from) else {
                return false;
            };
            let Some(to_index) = categories.iter().position(|value| value == to) else {
                return false;
            };
            let label = match brush.dimension {
                AxisDimension::X => {
                    if series.kind == GeneralSeriesKind::HorizontalBar {
                        None
                    } else {
                        general_x_label(dataset, row)
                    }
                }
                AxisDimension::Y => match series.kind {
                    GeneralSeriesKind::HorizontalBar => general_x_label(dataset, row),
                    GeneralSeriesKind::HeatmapGrid => general_heatmap_y_label(dataset, row),
                    _ => None,
                },
                AxisDimension::Angle | AxisDimension::Radius => None,
            };
            label
                .and_then(|label| categories.iter().position(|value| value == &label))
                .is_some_and(|index| {
                    let low = from_index.min(to_index);
                    let high = from_index.max(to_index);
                    index >= low && index <= high
                })
        }
        _ => false,
    }
}

fn general_heatmap_y_label(dataset: &crate::GeneralDataset, row: usize) -> Option<String> {
    if let Some(value) = dataset
        .heatmap_y_numeric()
        .and_then(|values| values.get(row))
    {
        return Some(value.to_string());
    }
    let category_index = usize::try_from(*dataset.heatmap_y_category_indices()?.get(row)?).ok()?;
    dataset.heatmap_y_categories()?.get(category_index).cloned()
}

fn series_axis_dimensions(kind: GeneralSeriesKind) -> (AxisDimension, AxisDimension) {
    if matches!(
        kind,
        GeneralSeriesKind::Pie
            | GeneralSeriesKind::Radar
            | GeneralSeriesKind::RadialBar
            | GeneralSeriesKind::PolarArea
    ) {
        (AxisDimension::Angle, AxisDimension::Radius)
    } else {
        (AxisDimension::X, AxisDimension::Y)
    }
}

fn validate_general_series_axis_compatibility(
    kind: GeneralSeriesKind,
    pane_domain: HorizontalDomain,
    dataset_kind: GeneralXKind,
    x_scale: GeneralScaleType,
    y_scale: GeneralScaleType,
    heatmap_y_category: bool,
    heatmap_y_numeric: bool,
) -> Result<(), ChartError> {
    match kind {
        GeneralSeriesKind::Pie
        | GeneralSeriesKind::Radar
        | GeneralSeriesKind::RadialBar
        | GeneralSeriesKind::PolarArea => {
            if pane_domain != HorizontalDomain::Polar
                || dataset_kind != GeneralXKind::Category
                || x_scale != GeneralScaleType::AngularCategory
                || y_scale != GeneralScaleType::RadialLinear
            {
                return Err(invalid(
                    "polar series require a polar pane, category/value data, and angular/radial axes",
                ));
            }
        }
        GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea | GeneralSeriesKind::RangeArea => {
            let compatible_x = matches!(
                (pane_domain, dataset_kind, x_scale),
                (
                    HorizontalDomain::Continuous { .. },
                    GeneralXKind::Numeric,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                ) | (
                    HorizontalDomain::Temporal,
                    GeneralXKind::Temporal,
                    GeneralScaleType::Temporal
                ) | (
                    HorizontalDomain::Category {
                        scale: crate::CategoryScaleType::Band
                    },
                    GeneralXKind::Category,
                    GeneralScaleType::Band
                ) | (
                    HorizontalDomain::Category {
                        scale: crate::CategoryScaleType::Point
                    },
                    GeneralXKind::Category,
                    GeneralScaleType::Point
                )
            );
            if !compatible_x
                || !matches!(
                    y_scale,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                )
            {
                return Err(invalid(
                    "xy_line/xy_area/range_area requires X data/axis semantics matching its continuous, temporal, or category pane and a numeric Y axis",
                ));
            }
        }
        GeneralSeriesKind::RangeBar => {
            if pane_domain
                != (HorizontalDomain::Category {
                    scale: crate::CategoryScaleType::Band,
                })
                || dataset_kind != GeneralXKind::Category
                || x_scale != GeneralScaleType::Band
                || !matches!(
                    y_scale,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                )
            {
                return Err(invalid(
                    "range_bar requires a category-band pane, category X data, a band X axis, and a numeric Y axis",
                ));
            }
        }
        GeneralSeriesKind::Column => {
            if pane_domain
                != (HorizontalDomain::Category {
                    scale: crate::CategoryScaleType::Band,
                })
                || dataset_kind != GeneralXKind::Category
                || x_scale != GeneralScaleType::Band
                || y_scale != GeneralScaleType::Linear
            {
                return Err(invalid(
                    "the initial column series requires a category-band pane, category X data, a band X axis, and a linear Y axis",
                ));
            }
        }
        GeneralSeriesKind::Histogram => {
            if pane_domain
                != (HorizontalDomain::Continuous {
                    scale: crate::ContinuousScaleType::Linear,
                })
                || dataset_kind != GeneralXKind::Values
                || x_scale != GeneralScaleType::Linear
                || y_scale != GeneralScaleType::Linear
            {
                return Err(invalid(
                    "histogram requires a continuous-linear pane, sample values data, and linear X/Y axes",
                ));
            }
        }
        GeneralSeriesKind::HorizontalBar => {
            if pane_domain
                != (HorizontalDomain::Continuous {
                    scale: crate::ContinuousScaleType::Linear,
                })
                || dataset_kind != GeneralXKind::Category
                || x_scale != GeneralScaleType::Linear
                || y_scale != GeneralScaleType::Band
            {
                return Err(invalid(
                    "horizontal_bar requires a continuous-linear pane, category/value data, a linear X axis, and a band Y axis",
                ));
            }
        }
        GeneralSeriesKind::BoxPlot => {
            if pane_domain
                != (HorizontalDomain::Category {
                    scale: crate::CategoryScaleType::Band,
                })
                || dataset_kind != GeneralXKind::Category
                || x_scale != GeneralScaleType::Band
                || !matches!(
                    y_scale,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                )
            {
                return Err(invalid(
                    "box_plot requires a category-band pane, category data, a band X axis, and a numeric Y axis",
                ));
            }
        }
        GeneralSeriesKind::HeatmapGrid => {
            let compatible = match dataset_kind {
                GeneralXKind::Category => {
                    pane_domain
                        == (HorizontalDomain::Category {
                            scale: crate::CategoryScaleType::Band,
                        })
                        && x_scale == GeneralScaleType::Band
                        && y_scale == GeneralScaleType::Band
                        && heatmap_y_category
                }
                GeneralXKind::Numeric => {
                    matches!(pane_domain, HorizontalDomain::Continuous { .. })
                        && matches!(
                            x_scale,
                            GeneralScaleType::Linear
                                | GeneralScaleType::Logarithmic
                                | GeneralScaleType::SymmetricLog
                        )
                        && matches!(
                            y_scale,
                            GeneralScaleType::Linear
                                | GeneralScaleType::Logarithmic
                                | GeneralScaleType::SymmetricLog
                        )
                        && heatmap_y_numeric
                }
                GeneralXKind::Temporal => {
                    pane_domain == HorizontalDomain::Temporal
                        && x_scale == GeneralScaleType::Temporal
                        && matches!(
                            y_scale,
                            GeneralScaleType::Linear
                                | GeneralScaleType::Logarithmic
                                | GeneralScaleType::SymmetricLog
                        )
                        && heatmap_y_numeric
                }
                GeneralXKind::Values => false,
            };
            if !compatible {
                return Err(invalid(
                    "heatmap_grid requires category/category band axes, numeric/numeric axes, or temporal-X/numeric-Y axes matching its data",
                ));
            }
        }
        GeneralSeriesKind::Scatter | GeneralSeriesKind::Bubble => {
            if !matches!(pane_domain, HorizontalDomain::Continuous { .. })
                || dataset_kind != GeneralXKind::Numeric
                || !matches!(
                    x_scale,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                )
                || !matches!(
                    y_scale,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                )
            {
                return Err(invalid(
                    "scatter/bubble requires a continuous pane, numeric X data, and numeric X/Y axes",
                ));
            }
        }
        GeneralSeriesKind::ErrorBar => {
            let compatible_x = matches!(
                (pane_domain, dataset_kind, x_scale),
                (
                    HorizontalDomain::Continuous { .. },
                    GeneralXKind::Numeric,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                ) | (
                    HorizontalDomain::Temporal,
                    GeneralXKind::Temporal,
                    GeneralScaleType::Temporal
                ) | (
                    HorizontalDomain::Category {
                        scale: crate::CategoryScaleType::Band
                    },
                    GeneralXKind::Category,
                    GeneralScaleType::Band
                ) | (
                    HorizontalDomain::Category {
                        scale: crate::CategoryScaleType::Point
                    },
                    GeneralXKind::Category,
                    GeneralScaleType::Point
                )
            );
            if !compatible_x
                || !matches!(
                    y_scale,
                    GeneralScaleType::Linear
                        | GeneralScaleType::Logarithmic
                        | GeneralScaleType::SymmetricLog
                )
            {
                return Err(invalid(
                    "error_bar requires matching numeric/temporal/category X pane, data, and axis semantics with a numeric Y axis",
                ));
            }
        }
    }
    Ok(())
}

fn validate_histogram_dataset(
    options: &GeneralSeriesOptions,
    dataset: &crate::GeneralDataset,
) -> Result<(), ChartError> {
    if options.kind != GeneralSeriesKind::Histogram {
        return Ok(());
    }
    validate_histogram_options(&options.histogram)?;
    validate_histogram_samples(&options.histogram, dataset.y(), |row| {
        dataset.y_is_valid(row)
    })
}

fn validate_dataset_for_series(
    kind: GeneralSeriesKind,
    dataset: &crate::GeneralDataset,
    x_scale: GeneralScaleType,
    y_scale: GeneralScaleType,
) -> Result<(), ChartError> {
    match kind {
        GeneralSeriesKind::PolarArea | GeneralSeriesKind::RadialBar => {
            let (Some(categories), Some(indices)) =
                (dataset.categories(), dataset.category_indices())
            else {
                return Err(invalid("polar area requires category/value data"));
            };
            validate_polar_area_rows(categories.len(), indices, dataset.y(), |row| {
                dataset.y_is_valid(row)
            })?;
        }
        GeneralSeriesKind::Radar => {
            let (Some(categories), Some(indices)) =
                (dataset.categories(), dataset.category_indices())
            else {
                return Err(invalid("radar requires category/value data"));
            };
            validate_radar_rows(categories.len(), indices, dataset.y(), |row| {
                dataset.y_is_valid(row)
            })?;
        }
        GeneralSeriesKind::Pie => {
            if dataset.x_kind() != GeneralXKind::Category {
                return Err(invalid("pie requires category/value data"));
            }
            if dataset.len() > MAX_GENERAL_PIE_SLICES {
                return Err(resource(format!(
                    "pie exceeds {MAX_GENERAL_PIE_SLICES} slices"
                )));
            }
            if dataset
                .y()
                .iter()
                .enumerate()
                .any(|(row, value)| dataset.y_is_valid(row) && *value < 0.0)
            {
                return Err(invalid("pie values must be nonnegative"));
            }
        }
        GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar => {
            if dataset.x_kind() != GeneralXKind::Category {
                return Err(invalid("bar series require category/value data"));
            }
        }
        GeneralSeriesKind::Histogram => {
            if dataset.x_kind() != GeneralXKind::Values {
                return Err(invalid("histogram requires sample values data"));
            }
        }
        GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
            let expected_x = match x_scale {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("general path X axis scale is incompatible"));
                }
            };
            if dataset.x_kind() != expected_x {
                return Err(invalid(
                    "general path dataset X kind must match its bound X axis",
                ));
            }
        }
        GeneralSeriesKind::RangeArea | GeneralSeriesKind::RangeBar => {
            let expected_x = match x_scale {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("range-area X axis scale is incompatible"));
                }
            };
            if dataset.x_kind() != expected_x {
                return Err(invalid(
                    "range-area dataset X kind must match its bound X axis",
                ));
            }
            dataset
                .low()
                .ok_or_else(|| invalid("range-area series require a low-value channel"))?;
        }
        GeneralSeriesKind::ErrorBar => {
            match dataset.x_kind() {
                GeneralXKind::Numeric => {
                    dataset
                        .x_low()
                        .ok_or_else(|| invalid("numeric error bars require an X-low channel"))?;
                    dataset
                        .x_high()
                        .ok_or_else(|| invalid("numeric error bars require an X-high channel"))?;
                }
                GeneralXKind::Category => {
                    if dataset.x_low().is_some() || dataset.x_high().is_some() {
                        return Err(invalid(
                            "category error bars cannot have numeric X-bound channels",
                        ));
                    }
                }
                GeneralXKind::Temporal => {
                    dataset
                        .x_low()
                        .ok_or_else(|| invalid("temporal error bars require an X-low channel"))?;
                    dataset
                        .x_high()
                        .ok_or_else(|| invalid("temporal error bars require an X-high channel"))?;
                }
                GeneralXKind::Values => {
                    return Err(invalid("error bars require X coordinates"));
                }
            }
            dataset
                .low()
                .ok_or_else(|| invalid("error-bar series require a Y-low channel"))?;
            dataset
                .high()
                .ok_or_else(|| invalid("error-bar series require a Y-high channel"))?;
        }
        GeneralSeriesKind::BoxPlot => {
            if dataset.x_kind() != GeneralXKind::Category {
                return Err(invalid("box-plot series require category X data"));
            }
            let min = dataset
                .low()
                .ok_or_else(|| invalid("box-plot series require a min channel"))?;
            let max = dataset
                .high()
                .ok_or_else(|| invalid("box-plot series require a max channel"))?;
            let q1 = dataset
                .x_low()
                .ok_or_else(|| invalid("box-plot series require a q1 channel"))?;
            let q3 = dataset
                .x_high()
                .ok_or_else(|| invalid("box-plot series require a q3 channel"))?;
            if y_scale == GeneralScaleType::Logarithmic {
                for row in 0..dataset.len() {
                    for (valid, value) in [
                        (dataset.low_is_valid(row), min[row]),
                        (dataset.x_low_is_valid(row), q1[row]),
                        (dataset.y_is_valid(row), dataset.y()[row]),
                        (dataset.x_high_is_valid(row), q3[row]),
                        (dataset.high_is_valid(row), max[row]),
                    ] {
                        if valid && value <= 0.0 {
                            return Err(invalid(
                                "logarithmic box-plot statistics must be positive",
                            ));
                        }
                    }
                }
            }
        }
        GeneralSeriesKind::HeatmapGrid => match dataset.x_kind() {
            GeneralXKind::Values => {
                return Err(invalid("heatmap_grid requires X coordinates"));
            }
            GeneralXKind::Category => {
                let y_categories = dataset
                    .heatmap_y_categories()
                    .ok_or_else(|| invalid("category heatmaps require Y-category labels"))?;
                let y_indices = dataset
                    .heatmap_y_category_indices()
                    .ok_or_else(|| invalid("category heatmaps require Y-category indices"))?;
                if y_indices.len() != dataset.len()
                    || (y_categories.is_empty() && !dataset.is_empty())
                {
                    return Err(invalid(
                        "heatmap-grid Y-category columns must stay aligned with the dataset",
                    ));
                }
            }
            GeneralXKind::Numeric | GeneralXKind::Temporal => {
                let y_coordinates = dataset.heatmap_y_numeric().ok_or_else(|| {
                    invalid("numeric/temporal heatmaps require numeric Y coordinates")
                })?;
                if y_coordinates.len() != dataset.len() {
                    return Err(invalid(
                        "heatmap-grid numeric Y coordinates must stay aligned with the dataset",
                    ));
                }
                if y_scale == GeneralScaleType::Logarithmic
                    && y_coordinates.iter().any(|value| *value <= 0.0)
                {
                    return Err(invalid(
                        "logarithmic heatmap Y coordinates must be positive",
                    ));
                }
                if dataset.x_kind() == GeneralXKind::Numeric
                    && x_scale == GeneralScaleType::Logarithmic
                    && dataset
                        .numeric_x()
                        .is_some_and(|values| values.iter().any(|value| *value <= 0.0))
                {
                    return Err(invalid(
                        "logarithmic heatmap X coordinates must be positive",
                    ));
                }
            }
        },
        GeneralSeriesKind::Scatter => {
            let values = dataset
                .numeric_x()
                .ok_or_else(|| invalid("scatter series require numeric X data"))?;
            if x_scale == GeneralScaleType::Logarithmic && values.iter().any(|value| *value <= 0.0)
            {
                return Err(invalid("logarithmic scatter X values must be positive"));
            }
            validate_logarithmic_general_y(dataset, y_scale)?;
        }
        GeneralSeriesKind::Bubble => {
            dataset
                .numeric_x()
                .ok_or_else(|| invalid("bubble series require numeric X data"))?;
            dataset
                .size()
                .ok_or_else(|| invalid("bubble series require a size channel"))?;
        }
    }
    Ok(())
}

fn validate_radar_rows(
    category_count: usize,
    indices: &[u32],
    values: &[f64],
    is_valid: impl Fn(usize) -> bool,
) -> Result<(), ChartError> {
    if category_count == 0 && values.is_empty() {
        return Ok(());
    }
    if category_count < 3 || values.len() < 3 {
        return Err(invalid("radar requires at least three categories and rows"));
    }
    if values.len() > MAX_GENERAL_RADAR_VALUES {
        return Err(resource(format!(
            "radar exceeds {MAX_GENERAL_RADAR_VALUES} values"
        )));
    }
    let mut seen = HashSet::with_capacity(indices.len());
    for (row, &index) in indices.iter().enumerate() {
        if !seen.insert(index) {
            return Err(invalid("radar requires at most one row per category"));
        }
        if is_valid(row) && values[row] < 0.0 {
            return Err(invalid("radar values must be nonnegative"));
        }
    }
    Ok(())
}

fn validate_polar_area_rows(
    category_count: usize,
    indices: &[u32],
    values: &[f64],
    is_valid: impl Fn(usize) -> bool,
) -> Result<(), ChartError> {
    if category_count == 0 && values.is_empty() {
        return Ok(());
    }
    if category_count == 0 || values.is_empty() {
        return Err(invalid("polar area requires category/value rows"));
    }
    if values.len() > MAX_GENERAL_PIE_SLICES {
        return Err(resource(format!(
            "polar area exceeds {MAX_GENERAL_PIE_SLICES} sectors"
        )));
    }
    let mut seen = HashSet::with_capacity(indices.len());
    for (row, &index) in indices.iter().enumerate() {
        if !seen.insert(index) {
            return Err(invalid("polar area requires at most one row per category"));
        }
        if is_valid(row) && values[row] < 0.0 {
            return Err(invalid("polar area values must be nonnegative"));
        }
    }
    Ok(())
}

fn validate_logarithmic_general_y(
    dataset: &crate::GeneralDataset,
    y_scale: GeneralScaleType,
) -> Result<(), ChartError> {
    if y_scale == GeneralScaleType::Logarithmic
        && dataset
            .y()
            .iter()
            .enumerate()
            .any(|(index, value)| dataset.y_is_valid(index) && *value <= 0.0)
    {
        return Err(invalid(
            "logarithmic general-series Y values must be positive",
        ));
    }
    Ok(())
}

fn validate_general_reference_options(
    pane_id: PaneId,
    options: &GeneralReferenceOptions,
    axis_info: impl Fn(&str) -> Result<(PaneId, AxisDimension, GeneralScaleType), ChartError>,
) -> Result<(), ChartError> {
    let validate_axis = |id: &str,
                         dimension: AxisDimension|
     -> Result<GeneralScaleType, ChartError> {
        let (axis_pane, axis_dimension, scale) = axis_info(id)?;
        if axis_pane != pane_id || axis_dimension != dimension {
            return Err(invalid(format!(
                "general reference axis {id:?} must belong to its pane and have {dimension:?} dimension"
            )));
        }
        Ok(scale)
    };
    let validate_color = |color: &Option<String>| -> Result<(), ChartError> {
        if let Some(color) = color {
            if color.len() > MAX_GENERAL_SERIES_COLOR_BYTES {
                return Err(resource(format!(
                    "general reference color exceeds {MAX_GENERAL_SERIES_COLOR_BYTES} bytes"
                )));
            }
            if Color::parse_css(color).is_none() {
                return Err(invalid(
                    "general reference color is not a supported CSS color",
                ));
            }
        }
        Ok(())
    };

    match options {
        GeneralReferenceOptions::Line {
            axis_id,
            value,
            color,
            line_width,
            ..
        } => {
            let (axis_pane, axis_dimension, scale) = axis_info(axis_id)?;
            if axis_pane != pane_id
                || !matches!(axis_dimension, AxisDimension::X | AxisDimension::Y)
            {
                return Err(invalid(
                    "general reference line axis must be an X or Y axis on its pane",
                ));
            }
            validate_general_reference_value(value, scale)?;
            validate_color(color)?;
            if !line_width.is_finite() || *line_width <= 0.0 || *line_width > 64.0 {
                return Err(invalid(
                    "general reference line_width must be finite and in (0, 64]",
                ));
            }
        }
        GeneralReferenceOptions::Dot {
            x_axis_id,
            y_axis_id,
            x,
            y,
            color,
            radius,
            ..
        } => {
            let x_axis = validate_axis(x_axis_id, AxisDimension::X)?;
            let y_axis = validate_axis(y_axis_id, AxisDimension::Y)?;
            validate_general_reference_value(x, x_axis)?;
            validate_general_reference_value(y, y_axis)?;
            validate_color(color)?;
            if !radius.is_finite()
                || *radius < MIN_GENERAL_POINT_RADIUS
                || *radius > MAX_GENERAL_POINT_RADIUS
            {
                return Err(invalid(format!(
                    "general reference radius must be finite and in [{MIN_GENERAL_POINT_RADIUS}, {MAX_GENERAL_POINT_RADIUS}]"
                )));
            }
        }
        GeneralReferenceOptions::Region {
            x_axis_id,
            y_axis_id,
            x_from,
            x_to,
            y_from,
            y_to,
            fill_color,
            ..
        } => {
            let x_axis = validate_axis(x_axis_id, AxisDimension::X)?;
            let y_axis = validate_axis(y_axis_id, AxisDimension::Y)?;
            for value in [x_from, x_to] {
                validate_general_reference_value(value, x_axis)?;
            }
            for value in [y_from, y_to] {
                validate_general_reference_value(value, y_axis)?;
            }
            validate_color(fill_color)?;
        }
    }
    Ok(())
}

fn validate_general_reference_value(
    value: &GeneralReferenceValue,
    scale: GeneralScaleType,
) -> Result<(), ChartError> {
    match (value, scale) {
        (
            GeneralReferenceValue::Numeric(value),
            GeneralScaleType::Linear
            | GeneralScaleType::Logarithmic
            | GeneralScaleType::SymmetricLog,
        ) => {
            if !value.is_finite() {
                return Err(invalid("general reference numeric values must be finite"));
            }
            if scale == GeneralScaleType::Logarithmic && *value <= 0.0 {
                return Err(invalid(
                    "general reference values on logarithmic axes must be positive",
                ));
            }
            Ok(())
        }
        (GeneralReferenceValue::Temporal(value), GeneralScaleType::Temporal) => {
            if value.unsigned_abs() > crate::MAX_GENERAL_TEMPORAL_MILLISECONDS as u64 {
                return Err(invalid(format!(
                    "general reference temporal values must stay within +/-{} epoch milliseconds",
                    crate::MAX_GENERAL_TEMPORAL_MILLISECONDS
                )));
            }
            Ok(())
        }
        (
            GeneralReferenceValue::Category(value),
            GeneralScaleType::Band | GeneralScaleType::Point,
        ) => {
            if value.is_empty() {
                return Err(invalid(
                    "general reference category values must not be empty",
                ));
            }
            if value.len() > crate::MAX_GENERAL_AXIS_CATEGORY_BYTES {
                return Err(resource(format!(
                    "general reference category exceeds {} bytes",
                    crate::MAX_GENERAL_AXIS_CATEGORY_BYTES
                )));
            }
            Ok(())
        }
        _ => Err(invalid(
            "general reference value type must match the bound axis scale",
        )),
    }
}

fn validate_input_for_series(
    kind: GeneralSeriesKind,
    input: &GeneralXyInput,
    x_axis: &crate::GeneralAxis,
    y_axis: &crate::GeneralAxis,
) -> Result<(), ChartError> {
    match kind {
        GeneralSeriesKind::PolarArea | GeneralSeriesKind::RadialBar => {
            let GeneralXyInput::Category {
                categories,
                category_indices,
                y,
                y_valid,
                ..
            } = input
            else {
                return Err(invalid(
                    "a polar area dataset must remain category/value data",
                ));
            };
            validate_polar_area_rows(categories.len(), category_indices, y, |row| {
                y_valid
                    .as_ref()
                    .is_none_or(|valid| valid.get(row).is_some_and(|flag| *flag != 0))
            })?;
        }
        GeneralSeriesKind::Radar => {
            let GeneralXyInput::Category {
                categories,
                category_indices,
                y,
                y_valid,
                ..
            } = input
            else {
                return Err(invalid("a radar dataset must remain category/value data"));
            };
            validate_radar_rows(categories.len(), category_indices, y, |row| {
                y_valid
                    .as_ref()
                    .is_none_or(|valid| valid.get(row).is_some_and(|flag| *flag != 0))
            })?;
        }
        GeneralSeriesKind::Pie => {
            if input.x_kind() != GeneralXKind::Category {
                return Err(invalid("a pie dataset must remain category/value data"));
            }
            if input.y_values().len() > MAX_GENERAL_PIE_SLICES {
                return Err(resource(format!(
                    "pie exceeds {MAX_GENERAL_PIE_SLICES} slices"
                )));
            }
            if input.y_values().iter().enumerate().any(|(row, value)| {
                input
                    .y_valid_values()
                    .is_none_or(|valid| valid.get(row).is_some_and(|value| *value != 0))
                    && *value < 0.0
            }) {
                return Err(invalid("pie values must be nonnegative"));
            }
        }
        GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar => {
            if input.x_kind() != GeneralXKind::Category {
                return Err(invalid(
                    "a dataset bound to a bar series must remain category/value data",
                ));
            }
        }
        GeneralSeriesKind::Histogram => {
            if input.x_kind() != GeneralXKind::Values {
                return Err(invalid(
                    "a dataset bound to a histogram must remain sample values data",
                ));
            }
        }
        GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("general path X axis scale is incompatible"));
                }
            };
            if input.x_kind() != expected_x {
                return Err(invalid(
                    "a dataset bound to a general path series must keep the X kind required by its X axis",
                ));
            }
        }
        GeneralSeriesKind::RangeArea | GeneralSeriesKind::RangeBar => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("range-area X axis scale is incompatible"));
                }
            };
            if input.x_kind() != expected_x {
                return Err(invalid(
                    "a dataset bound to a range-area series must keep the X kind required by its X axis",
                ));
            }
            input.low_values().ok_or_else(|| {
                invalid("a dataset bound to a range-area series must retain its low-value channel")
            })?;
        }
        GeneralSeriesKind::ErrorBar => {
            let expected_x = match x_axis.scale() {
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog => GeneralXKind::Numeric,
                GeneralScaleType::Temporal => GeneralXKind::Temporal,
                GeneralScaleType::Band | GeneralScaleType::Point => GeneralXKind::Category,
                GeneralScaleType::RadialLinear | GeneralScaleType::AngularCategory => {
                    return Err(invalid("error-bar X axis scale is incompatible"));
                }
            };
            if input.x_kind() != expected_x {
                return Err(invalid(
                    "a dataset bound to an error-bar series must keep the X kind required by its X axis",
                ));
            }
            input.low_values().ok_or_else(|| {
                invalid("a dataset bound to an error-bar series must retain its Y-low channel")
            })?;
            input.high_values().ok_or_else(|| {
                invalid("a dataset bound to an error-bar series must retain its Y-high channel")
            })?;
            if matches!(expected_x, GeneralXKind::Numeric | GeneralXKind::Temporal) {
                input.x_low_values().ok_or_else(|| {
                    invalid("numeric/temporal error bars must retain an X-low channel")
                })?;
                input.x_high_values().ok_or_else(|| {
                    invalid("numeric/temporal error bars must retain an X-high channel")
                })?;
            } else if input.x_low_values().is_some() || input.x_high_values().is_some() {
                return Err(invalid(
                    "category error bars cannot have numeric X-bound channels",
                ));
            }
        }
        GeneralSeriesKind::BoxPlot => {
            let GeneralXyInput::BoxCategory {
                min,
                min_valid,
                q1,
                q1_valid,
                median,
                median_valid,
                q3,
                q3_valid,
                max,
                max_valid,
                ..
            } = input
            else {
                return Err(invalid(
                    "a dataset bound to a box-plot series must remain category box-plot data",
                ));
            };
            if y_axis.scale() == GeneralScaleType::Logarithmic {
                for row in 0..median.len() {
                    for (validity, value) in [
                        (min_valid.as_deref(), min[row]),
                        (q1_valid.as_deref(), q1[row]),
                        (median_valid.as_deref(), median[row]),
                        (q3_valid.as_deref(), q3[row]),
                        (max_valid.as_deref(), max[row]),
                    ] {
                        if validity.is_none_or(|values| values[row] != 0) && value <= 0.0 {
                            return Err(invalid(
                                "logarithmic box-plot statistics must be positive",
                            ));
                        }
                    }
                }
            }
        }
        GeneralSeriesKind::HeatmapGrid => match (input, x_axis.scale(), y_axis.scale()) {
            (
                GeneralXyInput::HeatmapCategoryCategory {
                    y_categories,
                    y_category_indices,
                    value,
                    ..
                },
                GeneralScaleType::Band,
                GeneralScaleType::Band,
            ) => {
                if y_category_indices.len() != value.len()
                    || (y_categories.is_empty() && !value.is_empty())
                {
                    return Err(invalid_data(
                        "heatmap-grid Y-category columns must stay aligned with the value channel",
                    ));
                }
            }
            (
                GeneralXyInput::HeatmapNumericNumeric {
                    x,
                    y_coordinate,
                    value,
                    ..
                },
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog,
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog,
            ) => {
                if x.len() != value.len() || y_coordinate.len() != value.len() {
                    return Err(invalid_data(
                        "numeric heatmap coordinates must stay aligned with the value channel",
                    ));
                }
                if x_axis.scale() == GeneralScaleType::Logarithmic
                    && x.iter().any(|value| *value <= 0.0)
                {
                    return Err(invalid(
                        "logarithmic heatmap X coordinates must be positive",
                    ));
                }
                if y_axis.scale() == GeneralScaleType::Logarithmic
                    && y_coordinate.iter().any(|value| *value <= 0.0)
                {
                    return Err(invalid(
                        "logarithmic heatmap Y coordinates must be positive",
                    ));
                }
            }
            (
                GeneralXyInput::HeatmapTemporalNumeric {
                    x_epoch_ms,
                    y_coordinate,
                    value,
                    ..
                },
                GeneralScaleType::Temporal,
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog,
            ) => {
                if x_epoch_ms.len() != value.len() || y_coordinate.len() != value.len() {
                    return Err(invalid_data(
                        "temporal heatmap coordinates must stay aligned with the value channel",
                    ));
                }
                if y_axis.scale() == GeneralScaleType::Logarithmic
                    && y_coordinate.iter().any(|value| *value <= 0.0)
                {
                    return Err(invalid(
                        "logarithmic heatmap Y coordinates must be positive",
                    ));
                }
            }
            _ => {
                return Err(invalid(
                    "a dataset bound to a heatmap-grid series must keep the coordinate shape required by its axes",
                ));
            }
        },
        GeneralSeriesKind::Scatter => {
            let values = input.numeric_x_values().ok_or_else(|| {
                invalid("a dataset bound to a scatter series must remain numeric X data")
            })?;
            if x_axis.scale() == GeneralScaleType::Logarithmic
                && values.iter().any(|value| *value <= 0.0)
            {
                return Err(invalid("logarithmic scatter X values must be positive"));
            }
            validate_logarithmic_input_y(input, y_axis)?;
        }
        GeneralSeriesKind::Bubble => {
            input.numeric_x_values().ok_or_else(|| {
                invalid("a dataset bound to a bubble series must remain numeric X data")
            })?;
            input.size_values().ok_or_else(|| {
                invalid("a dataset bound to a bubble series must retain its size channel")
            })?;
        }
    }
    Ok(())
}

fn validate_logarithmic_input_y(
    input: &GeneralXyInput,
    y_axis: &crate::GeneralAxis,
) -> Result<(), ChartError> {
    if y_axis.scale() == GeneralScaleType::Logarithmic {
        let validity = input.y_valid_values();
        if input.y_values().iter().enumerate().any(|(index, value)| {
            validity
                .and_then(|values| values.get(index))
                .is_none_or(|valid| *valid != 0)
                && *value <= 0.0
        }) {
            return Err(invalid(
                "logarithmic general-series Y values must be positive",
            ));
        }
    }
    Ok(())
}

fn validate_presentation(options: &GeneralSeriesOptions) -> Result<(), ChartError> {
    if matches!(
        options.kind,
        GeneralSeriesKind::Pie | GeneralSeriesKind::PolarArea | GeneralSeriesKind::RadialBar
    ) {
        let polar = options.polar;
        let sweep = polar.end_angle - polar.start_angle;
        if ![
            polar.start_angle,
            polar.end_angle,
            polar.inner_radius,
            polar.outer_radius,
            polar.pad_angle,
            sweep,
        ]
        .iter()
        .all(|value| value.is_finite())
            || sweep <= 0.0
            || sweep > std::f64::consts::TAU
            || polar.inner_radius < 0.0
            || polar.outer_radius > 1.0
            || polar.inner_radius >= polar.outer_radius
            || polar.pad_angle < 0.0
            || polar.pad_angle >= sweep
        {
            return Err(invalid(
                "polar sector bounds require a finite positive sweep of at most one turn, 0 <= inner < outer <= 1, and 0 <= pad < sweep",
            ));
        }
    } else if options.polar != GeneralPolarOptions::default() {
        return Err(invalid("polar options require a sector series"));
    }
    if options.kind == GeneralSeriesKind::Histogram {
        validate_histogram_options(&options.histogram)?;
    } else if options.histogram != GeneralHistogramOptions::default() {
        return Err(invalid("histogram options require a histogram series"));
    }
    if let Some(colors) = options.fill_gradient.as_ref() {
        if !matches!(
            options.kind,
            GeneralSeriesKind::XyArea
                | GeneralSeriesKind::RangeArea
                | GeneralSeriesKind::Column
                | GeneralSeriesKind::Histogram
                | GeneralSeriesKind::HorizontalBar
                | GeneralSeriesKind::RangeBar
        ) {
            return Err(invalid(
                "fill_gradient requires an area, column, histogram, horizontal_bar, or range_bar series",
            ));
        }
        for color in colors {
            if color.len() > MAX_GENERAL_SERIES_COLOR_BYTES {
                return Err(resource(format!(
                    "general gradient color exceeds {MAX_GENERAL_SERIES_COLOR_BYTES} UTF-8 bytes"
                )));
            }
            if Color::parse_css(color).is_none() {
                return Err(invalid("fill_gradient requires two valid CSS colors"));
            }
        }
    }
    for color in [
        options.heatmap_low_color.as_deref(),
        options.heatmap_high_color.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if options.kind != GeneralSeriesKind::HeatmapGrid || Color::parse_css(color).is_none() {
            return Err(invalid(
                "heatmap colors require a heatmap and valid CSS colors",
            ));
        }
        if color.len() > MAX_GENERAL_SERIES_COLOR_BYTES {
            return Err(resource(format!(
                "heatmap color exceeds {MAX_GENERAL_SERIES_COLOR_BYTES} UTF-8 bytes"
            )));
        }
    }
    for color in [
        options.box_fill_color.as_deref(),
        options.box_median_color.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if options.kind != GeneralSeriesKind::BoxPlot || Color::parse_css(color).is_none() {
            return Err(invalid(
                "box colors require a box_plot and valid CSS colors",
            ));
        }
        if color.len() > MAX_GENERAL_SERIES_COLOR_BYTES {
            return Err(resource(format!(
                "box color exceeds {MAX_GENERAL_SERIES_COLOR_BYTES} UTF-8 bytes"
            )));
        }
    }
    if let Some([low, high]) = options.heatmap_value_domain
        && (options.kind != GeneralSeriesKind::HeatmapGrid
            || !low.is_finite()
            || !high.is_finite()
            || low >= high)
    {
        return Err(invalid(
            "heatmap_value_domain requires a heatmap and finite increasing bounds",
        ));
    }
    if !options.bar_gap.is_finite() || !(0.0..=64.0).contains(&options.bar_gap) {
        return Err(invalid(
            "general series bar_gap must be finite and in [0, 64] CSS px",
        ));
    }
    if options
        .bar_max_width
        .is_some_and(|width| !width.is_finite() || !(1.0..=512.0).contains(&width))
    {
        return Err(invalid(
            "general series bar_max_width must be finite and in [1, 512] CSS px",
        ));
    }
    if !matches!(
        options.kind,
        GeneralSeriesKind::Column
            | GeneralSeriesKind::Histogram
            | GeneralSeriesKind::HorizontalBar
            | GeneralSeriesKind::RangeBar
    ) && (options.bar_gap != 0.0 || options.bar_max_width.is_some())
    {
        return Err(invalid(
            "bar_gap and bar_max_width are supported only by column, histogram, horizontal_bar, and range_bar",
        ));
    }
    if !options.bar_corner_radius.is_finite()
        || !(0.0..=64.0).contains(&options.bar_corner_radius)
        || (!matches!(
            options.kind,
            GeneralSeriesKind::Column
                | GeneralSeriesKind::Histogram
                | GeneralSeriesKind::HorizontalBar
        ) && options.bar_corner_radius != 0.0)
    {
        return Err(invalid(
            "bar_corner_radius requires a column, histogram, or horizontal_bar series and a value in [0, 64] CSS px",
        ));
    }
    if !options.fill_opacity.is_finite() || !(0.0..=1.0).contains(&options.fill_opacity) {
        return Err(invalid(
            "general series fill_opacity must be finite and in [0, 1]",
        ));
    }
    if options.fill_opacity != DEFAULT_GENERAL_FILL_OPACITY
        && !matches!(
            options.kind,
            GeneralSeriesKind::XyArea
                | GeneralSeriesKind::RangeArea
                | GeneralSeriesKind::Pie
                | GeneralSeriesKind::Radar
                | GeneralSeriesKind::PolarArea
                | GeneralSeriesKind::RadialBar
        )
    {
        return Err(invalid(
            "general series fill_opacity requires an area or polar fill series",
        ));
    }
    if options.point_symbol != GeneralPointSymbol::Circle
        && !matches!(
            options.kind,
            GeneralSeriesKind::XyLine
                | GeneralSeriesKind::XyArea
                | GeneralSeriesKind::RangeArea
                | GeneralSeriesKind::Scatter
                | GeneralSeriesKind::Radar
        )
    {
        return Err(invalid(
            "general series point_symbol requires a path, scatter, or radar series",
        ));
    }
    if options.connect_missing
        && !matches!(
            options.kind,
            GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea | GeneralSeriesKind::RangeArea
        )
    {
        return Err(invalid(
            "general series connect_missing is supported only by xy_line, xy_area, and range_area",
        ));
    }
    if options.interpolation != GeneralInterpolation::Linear
        && !matches!(
            options.kind,
            GeneralSeriesKind::XyLine | GeneralSeriesKind::XyArea | GeneralSeriesKind::RangeArea
        )
    {
        return Err(invalid(
            "general series interpolation is supported only by xy_line, xy_area, and range_area",
        ));
    }
    if options.point_markers
        && !matches!(
            options.kind,
            GeneralSeriesKind::XyLine
                | GeneralSeriesKind::XyArea
                | GeneralSeriesKind::RangeArea
                | GeneralSeriesKind::Radar
        )
    {
        return Err(invalid(
            "general series point_markers requires a path or radar series",
        ));
    }
    if !options.line_width.is_finite() || !(0.5..=32.0).contains(&options.line_width) {
        return Err(invalid(
            "general series line_width must be finite and in [0.5, 32]",
        ));
    }
    // Preserve valid dormant styles across partial updates and older V2 documents.
    // Only the active series kind decides whether a style contributes to paint.
    if let Some(value) = options.baseline_value {
        if !value.is_finite() {
            return Err(invalid("general series baseline_value must be finite"));
        }
        if options.kind != GeneralSeriesKind::XyArea {
            return Err(invalid(
                "general series baseline_value is supported only by xy_area",
            ));
        }
    }
    if options.baseline_policy != GeneralAreaBaseline::Zero
        && options.kind != GeneralSeriesKind::XyArea
    {
        return Err(invalid("baseline_policy is supported only by xy_area"));
    }
    if options.baseline_value.is_some() && options.baseline_policy != GeneralAreaBaseline::Zero {
        return Err(invalid(
            "baseline_value and baseline_policy cannot both be set",
        ));
    }
    if options.title.len() > MAX_GENERAL_SERIES_TITLE_BYTES {
        return Err(resource(format!(
            "general series title exceeds {MAX_GENERAL_SERIES_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    if let Some(color) = options.color.as_deref() {
        if color.len() > MAX_GENERAL_SERIES_COLOR_BYTES {
            return Err(resource(format!(
                "general series color exceeds {MAX_GENERAL_SERIES_COLOR_BYTES} UTF-8 bytes"
            )));
        }
        if Color::parse_css(color).is_none() {
            return Err(invalid("general series color must be a valid CSS color"));
        }
    }
    if options
        .group_id
        .as_ref()
        .is_some_and(|value| value.len() > MAX_GENERAL_SERIES_GROUP_ID_BYTES)
    {
        return Err(resource(format!(
            "general series group ID exceeds {MAX_GENERAL_SERIES_GROUP_ID_BYTES} UTF-8 bytes"
        )));
    }
    if options
        .stack_id
        .as_ref()
        .is_some_and(|value| value.len() > MAX_GENERAL_SERIES_STACK_ID_BYTES)
    {
        return Err(resource(format!(
            "general series stack ID exceeds {MAX_GENERAL_SERIES_STACK_ID_BYTES} UTF-8 bytes"
        )));
    }
    if !matches!(
        options.kind,
        GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar
    ) && options.group_id.is_some()
    {
        return Err(invalid(
            "grouping is supported only by column and horizontal_bar series",
        ));
    }
    if !matches!(
        options.kind,
        GeneralSeriesKind::Column | GeneralSeriesKind::HorizontalBar | GeneralSeriesKind::XyArea
    ) && (options.stack_id.is_some() || options.stack_mode != GeneralStackMode::Normal)
    {
        return Err(invalid(
            "stacking is supported only by column, horizontal_bar, and xy_area series",
        ));
    }
    if options.stack_id.is_none() && options.stack_mode != GeneralStackMode::Normal {
        return Err(invalid("nondefault stack mode requires a stack ID"));
    }
    // Earlier V2 writers retained any finite dormant radius, including zero.
    // A zero error-bar cap is meaningful (a stem without caps), even when painted.
    if !options.point_radius.is_finite()
        || ((options.kind == GeneralSeriesKind::Scatter || options.point_markers)
            && !(MIN_GENERAL_POINT_RADIUS..=MAX_GENERAL_POINT_RADIUS)
                .contains(&options.point_radius))
        || (options.kind == GeneralSeriesKind::ErrorBar
            && !(0.0..=MAX_GENERAL_POINT_RADIUS).contains(&options.point_radius))
    {
        return Err(invalid(format!(
            "general series point radius must be finite and in {MIN_GENERAL_POINT_RADIUS}..={MAX_GENERAL_POINT_RADIUS} CSS px"
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidOptions, message)
}

fn invalid_data(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

#[cfg(test)]
mod composition_tests {
    use super::*;
    use crate::{ContinuousScaleType, GeneralAxisOptions, GeneralScaleType};

    fn chart() -> ChartEngine {
        ChartEngine::new_with_initial_domain(
            640.0,
            400.0,
            1.0,
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            },
        )
        .unwrap()
    }

    fn axes(y_scale: GeneralScaleType) -> Vec<GeneralAxisOptions> {
        vec![
            GeneralAxisOptions::new("x", 0, AxisDimension::X, GeneralScaleType::Linear),
            GeneralAxisOptions::new("y", 0, AxisDimension::Y, y_scale),
        ]
    }

    fn data(value: f64) -> GeneralXyInput {
        GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 2.0],
            y: vec![value, value + 1.0],
            y_valid: None,
        }
    }

    fn series(
        key: &str,
        current: Option<GeneralSeriesId>,
        value: f64,
    ) -> GeneralPaneSeriesComposition {
        GeneralPaneSeriesComposition {
            key: key.into(),
            current,
            options: GeneralSeriesOptions::xy_line(
                0,
                GeneralDatasetId::from_raw(1).unwrap(),
                "x",
                "y",
            ),
            data: data(value),
            labels: None,
            colors: None,
            symbols: None,
        }
    }

    fn reference(
        key: &str,
        current: Option<GeneralReferenceId>,
    ) -> GeneralPaneReferenceComposition {
        GeneralPaneReferenceComposition {
            key: key.into(),
            current,
            options: GeneralReferenceOptions::Line {
                pane: 0,
                axis_id: "y".into(),
                value: GeneralReferenceValue::Numeric(2.0),
                color: None,
                line_width: 1.0,
                extend_domain: false,
            },
        }
    }

    #[test]
    fn composition_invalid_graph_is_atomic_including_identity_counters_and_invalidation() {
        let mut chart = chart();
        let before_clock = chart.frame_invalidation.clock();
        let mut invalid = series("bad", None, -1.0);
        invalid.data = GeneralXyInput::Numeric {
            ids: None,
            x: vec![1.0, 2.0],
            y: vec![1.0],
            y_valid: None,
        };
        assert!(
            chart
                .compose_general_pane(GeneralPaneComposition {
                    pane: 0,
                    axes: axes(GeneralScaleType::Logarithmic),
                    series: vec![invalid],
                    references: vec![],
                })
                .is_err()
        );
        assert_eq!(chart.frame_invalidation.clock(), before_clock);
        assert_eq!(chart.general_series_count(), 0);
        assert_eq!(chart.general_dataset_count(), 0);
        assert!(chart.general_axes(Some(0)).is_empty());

        let result = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![series("ok", None, 1.0)],
                references: vec![reference("mark", None)],
            })
            .unwrap();
        assert_eq!(chart.general_axis("x").unwrap().handle_token(), 1);
        assert_eq!(result.series[0].id.get(), 1);
        assert_eq!(result.series[0].dataset.get(), 1);
        assert_eq!(result.references[0].id.get(), 1);
    }

    #[test]
    fn composition_removes_dependents_before_axes_and_releases_datasets() {
        let mut chart = chart();
        chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![series("line", None, 1.0)],
                references: vec![reference("mark", None)],
            })
            .unwrap();
        chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: vec![],
                series: vec![],
                references: vec![],
            })
            .unwrap();
        assert_eq!(chart.general_series_count(), 0);
        assert_eq!(chart.general_reference_count(), 0);
        assert_eq!(chart.general_dataset_count(), 0);
        assert!(chart.general_axes(Some(0)).is_empty());
    }

    #[test]
    fn composition_retains_reordered_objects_and_replaces_key_or_structural_axis_changes() {
        let mut chart = chart();
        let first = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![series("a", None, 1.0), series("b", None, 10.0)],
                references: vec![reference("mark", None)],
            })
            .unwrap();
        let a = first.series[0].clone();
        let b = first.series[1].clone();
        let mark = first.references[0].clone();
        let mut updated_b = series("b", Some(b.id), 20.0);
        updated_b.options.visible = false;
        let second = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![updated_b, series("a", Some(a.id), 2.0)],
                references: vec![reference("mark", Some(mark.id))],
            })
            .unwrap();
        assert_eq!(second.series[0].id, b.id);
        assert_eq!(second.series[0].dataset, b.dataset);
        assert_eq!(second.series[1].id, a.id);
        assert_eq!(chart.general_series_order(Some(0)), vec![b.id, a.id]);
        assert!(!chart.general_series(b.id).unwrap().visible());
        assert_eq!(second.references[0].id, mark.id);

        let x_handle = chart.general_axis("x").unwrap().handle_token();
        let third = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::SymmetricLog),
                series: vec![
                    series("renamed", Some(b.id), 30.0),
                    series("a", Some(a.id), 3.0),
                ],
                references: vec![reference("mark", Some(mark.id))],
            })
            .unwrap();
        assert_ne!(third.series[0].id, b.id);
        assert_ne!(third.series[1].id, a.id);
        assert_ne!(third.references[0].id, mark.id);
        assert_eq!(chart.general_axis("x").unwrap().handle_token(), x_handle);
        assert_eq!(chart.general_series_count(), 2);
        assert_eq!(chart.general_dataset_count(), 2);
    }

    #[test]
    fn composition_removing_a_used_axis_fails_naming_the_dependency() {
        let mut chart = chart();
        let first = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![series("line", None, 1.0)],
                references: vec![reference("mark", None)],
            })
            .unwrap();
        let before_clock = chart.frame_invalidation.clock();
        let error = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear)
                    .into_iter()
                    .filter(|axis| axis.id != "y")
                    .collect(),
                series: vec![series("line", Some(first.series[0].id), 2.0)],
                references: vec![],
            })
            .unwrap_err();
        assert!(
            error.message().contains("\"y\" does not exist"),
            "{error:?}"
        );
        assert_eq!(chart.frame_invalidation.clock(), before_clock);
        assert!(chart.general_axis("y").is_some());
        assert_eq!(chart.general_reference_count(), 1);
        assert_eq!(
            chart.general_series_order(Some(0)),
            vec![first.series[0].id]
        );

        let error = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear)
                    .into_iter()
                    .filter(|axis| axis.id != "y")
                    .collect(),
                series: vec![],
                references: vec![reference("mark", Some(first.references[0].id))],
            })
            .unwrap_err();
        assert!(
            error.message().contains("\"y\" does not exist"),
            "{error:?}"
        );
        assert_eq!(chart.general_series_count(), 1);
        assert_eq!(chart.general_reference_count(), 1);
    }

    #[test]
    fn composition_installs_and_replaces_row_labels_with_the_data() {
        let mut chart = chart();
        let mut labeled = series("line", None, 1.0);
        labeled.labels = Some(vec![Some("first".into()), None]);
        let first = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![labeled],
                references: vec![],
            })
            .unwrap();
        let id = first.series[0].id;
        let label = |chart: &ChartEngine, row| {
            chart
                .general_tooltip_snapshot(id, row)
                .and_then(|snapshot| snapshot.label)
        };
        assert_eq!(label(&chart, 0).as_deref(), Some("first"));
        assert_eq!(label(&chart, 1), None);

        let mut relabeled = series("line", Some(id), 2.0);
        relabeled.labels = Some(vec![None, Some("second".into())]);
        chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![relabeled],
                references: vec![],
            })
            .unwrap();
        assert_eq!(label(&chart, 0), None);
        assert_eq!(label(&chart, 1).as_deref(), Some("second"));

        let mut mismatched = series("line", Some(id), 3.0);
        mismatched.labels = Some(vec![Some("only one".into())]);
        assert!(
            chart
                .compose_general_pane(GeneralPaneComposition {
                    pane: 0,
                    axes: axes(GeneralScaleType::Linear),
                    series: vec![mismatched],
                    references: vec![],
                })
                .is_err()
        );
        assert_eq!(label(&chart, 1).as_deref(), Some("second"));
    }

    #[test]
    fn composition_clears_a_brush_only_when_its_axis_is_removed_or_structural() {
        let mut chart = chart();
        let first = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: axes(GeneralScaleType::Linear),
                series: vec![series("line", None, 1.0)],
                references: vec![],
            })
            .unwrap();
        chart.select_full_general_brush("y").unwrap();
        let mut titled = axes(GeneralScaleType::Linear);
        titled[1].title = Some("Retitled".into());
        let retained = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: titled,
                series: vec![series("line", Some(first.series[0].id), 2.0)],
                references: vec![],
            })
            .unwrap();
        assert!(!retained.brush_cleared);
        assert_eq!(chart.general_brush_snapshot().unwrap().axis_id, "y");

        let mut structural = axes(GeneralScaleType::Linear);
        structural[1].scale = GeneralScaleType::Logarithmic;
        let replaced = chart
            .compose_general_pane(GeneralPaneComposition {
                pane: 0,
                axes: structural,
                series: vec![series("line", Some(first.series[0].id), 2.0)],
                references: vec![],
            })
            .unwrap();
        assert!(replaced.brush_cleared);
        assert!(chart.general_brush_snapshot().is_none());
        chart.select_full_general_brush("y").unwrap();
        assert_eq!(chart.general_brush_snapshot().unwrap().axis_id, "y");
    }
}

#[cfg(test)]
mod wiggle_tests {
    use super::wiggle_baselines;

    #[test]
    fn weighted_changes_shift_the_baseline_and_empty_columns_hold_it() {
        let offsets = wiggle_baselines(&[vec![2.0, 4.0, 0.0], vec![2.0, 2.0, 0.0]]);
        assert_eq!(offsets[0], 0.0);
        assert!((offsets[1] + 4.0 / 3.0).abs() < 1e-12);
        assert_eq!(offsets[2], offsets[1]);
        let signed = wiggle_baselines(&[vec![10.0, -3.0], vec![-15.0, 5.0], vec![2.0, -4.0]]);
        assert!((signed[1] + 5.75).abs() < 1e-12);
    }
}
