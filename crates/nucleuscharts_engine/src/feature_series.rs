//! Engine-owned advanced series.
//!
//! These series use the same ordered `ChartFrame` as every built-in series. Hosts only translate
//! typed data and options; price semantics, autoscale projections, geometry, and lifecycle stay in
//! the headless engine so WebGPU, Canvas2D, GPUI, and native rendering cannot diverge.

use crate::{ChartEngine, SeriesKind};
use nucleuscharts_core::model::data_layer::{SeriesId, SeriesIdError};
use nucleuscharts_core::model::data_validation::{
    ValidationError, ValidationReport, MAX_SAFE_VALUE, MIN_SAFE_VALUE,
};
use nucleuscharts_core::model::plot_list::MismatchDirection;
use nucleuscharts_render::color::Color;
use std::mem::size_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureSeriesKind {
    BrushableArea,
    DualRangeHistogram,
    GroupedBars,
    Heatmap,
    HlcArea,
    PrettyHistogram,
    RoundedCandles,
    BackgroundShade,
    StackedArea,
    StackedBars,
    WhiskerBox,
}

impl FeatureSeriesKind {
    pub fn from_u8(kind: u8) -> Option<Self> {
        Some(match kind {
            0 => Self::BrushableArea,
            1 => Self::DualRangeHistogram,
            2 => Self::GroupedBars,
            3 => Self::Heatmap,
            4 => Self::HlcArea,
            5 => Self::PrettyHistogram,
            6 => Self::RoundedCandles,
            7 => Self::BackgroundShade,
            8 => Self::StackedArea,
            9 => Self::StackedBars,
            10 => Self::WhiskerBox,
            _ => return None,
        })
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::BrushableArea => 0,
            Self::DualRangeHistogram => 1,
            Self::GroupedBars => 2,
            Self::Heatmap => 3,
            Self::HlcArea => 4,
            Self::PrettyHistogram => 5,
            Self::RoundedCandles => 6,
            Self::BackgroundShade => 7,
            Self::StackedArea => 8,
            Self::StackedBars => 9,
            Self::WhiskerBox => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatmapCell {
    pub low: f64,
    pub high: f64,
    pub amount: f64,
    /// Host-resolved `cellShader` output. `None` uses the official default shader.
    pub color: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushStyle {
    pub line_color: Color,
    pub top_color: Color,
    pub bottom_color: Color,
    pub line_width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushRange {
    /// Inclusive logical start. The renderer follows the reference and excludes `to`.
    pub from: f64,
    pub to: f64,
    pub style: BrushStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackedAreaColor {
    pub line: Color,
    pub area: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FeatureValue {
    BrushableArea {
        value: f64,
    },
    DualRangeHistogram {
        values: Vec<f64>,
    },
    GroupedBars {
        values: Vec<f64>,
    },
    Heatmap {
        cells: Vec<HeatmapCell>,
    },
    HlcArea {
        high: f64,
        low: f64,
        close: f64,
    },
    PrettyHistogram {
        value: f64,
        color: Option<Color>,
    },
    RoundedCandles {
        open: f64,
        high: f64,
        low: f64,
        close: f64,
    },
    BackgroundShade {
        value: f64,
    },
    StackedArea {
        values: Vec<f64>,
    },
    StackedBars {
        values: Vec<f64>,
    },
    WhiskerBox {
        quartiles: [f64; 5],
        outliers: Vec<f64>,
    },
}

impl FeatureValue {
    fn kind(&self) -> FeatureSeriesKind {
        match self {
            Self::BrushableArea { .. } => FeatureSeriesKind::BrushableArea,
            Self::DualRangeHistogram { .. } => FeatureSeriesKind::DualRangeHistogram,
            Self::GroupedBars { .. } => FeatureSeriesKind::GroupedBars,
            Self::Heatmap { .. } => FeatureSeriesKind::Heatmap,
            Self::HlcArea { .. } => FeatureSeriesKind::HlcArea,
            Self::PrettyHistogram { .. } => FeatureSeriesKind::PrettyHistogram,
            Self::RoundedCandles { .. } => FeatureSeriesKind::RoundedCandles,
            Self::BackgroundShade { .. } => FeatureSeriesKind::BackgroundShade,
            Self::StackedArea { .. } => FeatureSeriesKind::StackedArea,
            Self::StackedBars { .. } => FeatureSeriesKind::StackedBars,
            Self::WhiskerBox { .. } => FeatureSeriesKind::WhiskerBox,
        }
    }

    fn finite_and_safe(&self) -> bool {
        let safe =
            |value: f64| value.is_finite() && (MIN_SAFE_VALUE..=MAX_SAFE_VALUE).contains(&value);
        match self {
            Self::BrushableArea { value }
            | Self::PrettyHistogram { value, .. }
            | Self::BackgroundShade { value } => safe(*value),
            Self::DualRangeHistogram { values }
            | Self::GroupedBars { values }
            | Self::StackedArea { values }
            | Self::StackedBars { values } => {
                !values.is_empty() && values.iter().copied().all(safe)
            }
            Self::Heatmap { cells } => {
                !cells.is_empty()
                    && cells
                        .iter()
                        .all(|cell| safe(cell.low) && safe(cell.high) && safe(cell.amount))
            }
            Self::HlcArea { high, low, close } => safe(*high) && safe(*low) && safe(*close),
            Self::RoundedCandles {
                open,
                high,
                low,
                close,
            } => safe(*open) && safe(*high) && safe(*low) && safe(*close),
            Self::WhiskerBox {
                quartiles,
                outliers,
            } => quartiles.iter().copied().all(safe) && outliers.iter().copied().all(safe),
        }
    }

    fn semantic_anomaly(&self) -> bool {
        match self {
            Self::HlcArea { high, low, close } => high < low || close < low || close > high,
            Self::RoundedCandles {
                open,
                high,
                low,
                close,
            } => high < low || high < open || high < close || low > open || low > close,
            Self::Heatmap { cells } => cells.iter().any(|cell| cell.high < cell.low),
            Self::WhiskerBox { quartiles, .. } => {
                quartiles.windows(2).any(|pair| pair[0] > pair[1])
            }
            _ => false,
        }
    }

    /// OHLC-shaped projection used by the canonical scale/query layer. The complete payload stays
    /// in [`FeatureSeriesState`]; this projection is a derived index, not renderer-owned data.
    pub(crate) fn projection(&self) -> [f64; 4] {
        match self {
            Self::BrushableArea { value } | Self::PrettyHistogram { value, .. } => [*value; 4],
            Self::DualRangeHistogram { .. } => [0.0; 4],
            Self::GroupedBars { values } => {
                range_projection(values, *values.last().unwrap_or(&0.0))
            }
            Self::Heatmap { cells } => {
                let low = cells
                    .iter()
                    .map(|cell| cell.low)
                    .fold(f64::INFINITY, f64::min);
                let high = cells
                    .iter()
                    .map(|cell| cell.high)
                    .fold(f64::NEG_INFINITY, f64::max);
                let mid = low + (high - low) / 2.0;
                [mid, high, low, mid]
            }
            Self::HlcArea { high, low, close } => [*close, *high, *low, *close],
            Self::RoundedCandles {
                open,
                high,
                low,
                close,
            } => [*open, *high, *low, *close],
            // The reference intentionally returns NaN so this visual never owns a price scale.
            Self::BackgroundShade { .. } => [f64::NAN; 4],
            Self::StackedArea { values } | Self::StackedBars { values } => {
                let total = values.iter().sum::<f64>();
                [0.0, total.max(0.0), total.min(0.0), total]
            }
            Self::WhiskerBox { quartiles, .. } => {
                [quartiles[2], quartiles[4], quartiles[0], quartiles[2]]
            }
        }
    }
}

fn range_projection(values: &[f64], close: f64) -> [f64; 4] {
    let low = values.iter().copied().fold(0.0, f64::min);
    let high = values.iter().copied().fold(0.0, f64::max);
    [0.0, high, low, close]
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeatureDataPoint {
    pub time: f64,
    /// `None` is an explicit whitespace row.
    pub value: Option<FeatureValue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureSeriesOptionsPatch {
    pub color: Option<Color>,
    pub colors: Option<Vec<Color>>,
    pub stacked_area_colors: Option<Vec<StackedAreaColor>>,
    pub line_color: Option<Color>,
    pub top_color: Option<Color>,
    pub bottom_color: Option<Color>,
    pub line_width: Option<f64>,
    pub base_price: Option<f64>,
    pub brush_ranges: Option<Vec<BrushRange>>,
    pub border_radius: Option<Vec<f64>>,
    pub max_height: Option<f64>,
    pub cell_border_width: Option<f64>,
    pub cell_border_color: Option<Color>,
    pub high_line_color: Option<Color>,
    pub low_line_color: Option<Color>,
    pub close_line_color: Option<Color>,
    pub area_top_color: Option<Color>,
    pub area_bottom_color: Option<Color>,
    pub high_line_width: Option<f64>,
    pub low_line_width: Option<f64>,
    pub close_line_width: Option<f64>,
    pub width_percent: Option<f64>,
    pub radius: Option<f64>,
    pub up_color: Option<Color>,
    pub down_color: Option<Color>,
    pub wick_up_color: Option<Color>,
    pub wick_down_color: Option<Color>,
    pub wick_visible: Option<bool>,
    pub low_color: Option<Color>,
    pub high_color: Option<Color>,
    pub low_value: Option<f64>,
    pub high_value: Option<f64>,
    pub opacity: Option<f64>,
    pub whisker_color: Option<Color>,
    pub lower_quartile_fill: Option<Color>,
    pub upper_quartile_fill: Option<Color>,
    pub outlier_color: Option<Color>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FeatureSeriesOptions {
    pub color: Color,
    pub colors: Vec<Color>,
    pub stacked_area_colors: Vec<StackedAreaColor>,
    pub line_color: Color,
    pub top_color: Color,
    pub bottom_color: Color,
    pub line_width: f64,
    pub base_price: f64,
    pub brush_ranges: Vec<BrushRange>,
    pub border_radius: Vec<f64>,
    pub max_height: f64,
    pub cell_border_width: f64,
    pub cell_border_color: Color,
    pub high_line_color: Color,
    pub low_line_color: Color,
    pub close_line_color: Color,
    pub area_top_color: Color,
    pub area_bottom_color: Color,
    pub high_line_width: f64,
    pub low_line_width: f64,
    pub close_line_width: f64,
    pub width_percent: f64,
    pub radius: Option<f64>,
    pub up_color: Color,
    pub down_color: Color,
    pub wick_up_color: Color,
    pub wick_down_color: Color,
    pub wick_visible: bool,
    pub low_color: Color,
    pub high_color: Color,
    pub low_value: f64,
    pub high_value: f64,
    pub opacity: f64,
    pub whisker_color: Color,
    pub lower_quartile_fill: Color,
    pub upper_quartile_fill: Color,
    pub outlier_color: Color,
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::rgba(r, g, b, a)
}

impl Default for FeatureSeriesOptions {
    fn default() -> Self {
        let palette = vec![
            rgb(0x29, 0x62, 0xff),
            rgb(0xe1, 0x57, 0x5a),
            rgb(0xf2, 0x8e, 0x2c),
            rgb(0xa4, 0x59, 0xd1),
            rgb(0x1b, 0x9c, 0x85),
        ];
        Self {
            color: rgb(0xd6, 0x38, 0x64),
            colors: palette.clone(),
            stacked_area_colors: palette
                .iter()
                .copied()
                .map(|line| StackedAreaColor {
                    line,
                    area: Color::rgba(line.r(), line.g(), line.b(), 51),
                })
                .collect(),
            line_color: rgb(40, 98, 255),
            top_color: rgba(40, 98, 255, 102),
            bottom_color: rgba(40, 98, 255, 0),
            line_width: 2.0,
            base_price: 0.0,
            brush_ranges: Vec::new(),
            border_radius: vec![2.0, 0.0, 2.0, 0.0],
            max_height: 130.0,
            cell_border_width: 1.0,
            cell_border_color: rgba(0, 0, 0, 0),
            high_line_color: rgb(0x04, 0x99, 0x81),
            low_line_color: rgb(0xf2, 0x36, 0x45),
            close_line_color: rgb(0x87, 0x89, 0x93),
            area_top_color: rgba(4, 153, 129, 51),
            area_bottom_color: rgba(242, 54, 69, 51),
            high_line_width: 2.0,
            low_line_width: 2.0,
            close_line_width: 2.0,
            width_percent: 50.0,
            radius: None,
            up_color: rgb(0x26, 0xa6, 0x9a),
            down_color: rgb(0xef, 0x53, 0x50),
            wick_up_color: rgb(0x26, 0xa6, 0x9a),
            wick_down_color: rgb(0xef, 0x53, 0x50),
            wick_visible: true,
            low_color: rgb(50, 50, 255),
            high_color: rgb(255, 50, 50),
            low_value: 0.0,
            high_value: 100.0,
            opacity: 0.8,
            whisker_color: rgb(106, 27, 154),
            lower_quartile_fill: rgb(103, 58, 183),
            upper_quartile_fill: rgb(233, 30, 99),
            outlier_color: rgb(149, 152, 161),
        }
    }
}

impl FeatureSeriesOptions {
    fn for_kind(kind: FeatureSeriesKind) -> Self {
        let mut options = Self::default();
        if kind == FeatureSeriesKind::DualRangeHistogram {
            options.colors = vec![
                rgb(0xac, 0xe5, 0xdc),
                rgb(0x42, 0xbd, 0xa8),
                rgb(0xfc, 0xca, 0xcd),
                rgb(0xf7, 0x7c, 0x80),
            ];
        }
        options
    }

    fn apply(&mut self, patch: FeatureSeriesOptionsPatch) {
        macro_rules! set {
            ($field:ident) => {
                if let Some(value) = patch.$field {
                    self.$field = value;
                }
            };
        }
        set!(color);
        if patch
            .colors
            .as_ref()
            .is_some_and(|colors| !colors.is_empty())
        {
            self.colors = patch.colors.unwrap_or_default();
        }
        if patch
            .stacked_area_colors
            .as_ref()
            .is_some_and(|colors| !colors.is_empty())
        {
            self.stacked_area_colors = patch.stacked_area_colors.unwrap_or_default();
        }
        set!(line_color);
        set!(top_color);
        set!(bottom_color);
        if patch
            .line_width
            .is_some_and(|value| value.is_finite() && value > 0.0)
        {
            self.line_width = patch.line_width.unwrap_or(self.line_width);
        }
        if patch.base_price.is_some_and(f64::is_finite) {
            self.base_price = patch.base_price.unwrap_or(self.base_price);
        }
        if let Some(ranges) = patch.brush_ranges {
            self.brush_ranges = ranges
                .into_iter()
                .filter(|range| {
                    range.from.is_finite()
                        && range.to.is_finite()
                        && range.style.line_width.is_finite()
                        && range.style.line_width > 0.0
                })
                .collect();
        }
        if patch
            .border_radius
            .as_ref()
            .is_some_and(|radii| !radii.is_empty())
        {
            self.border_radius = patch
                .border_radius
                .unwrap_or_default()
                .into_iter()
                .map(|radius| radius.max(0.0))
                .collect();
        }
        if patch
            .max_height
            .is_some_and(|value| value.is_finite() && value > 0.0)
        {
            self.max_height = patch.max_height.unwrap_or(self.max_height);
        }
        if patch
            .cell_border_width
            .is_some_and(|value| value.is_finite() && value >= 0.0)
        {
            self.cell_border_width = patch.cell_border_width.unwrap_or(self.cell_border_width);
        }
        set!(cell_border_color);
        set!(high_line_color);
        set!(low_line_color);
        set!(close_line_color);
        set!(area_top_color);
        set!(area_bottom_color);
        if patch
            .high_line_width
            .is_some_and(|value| value.is_finite() && value > 0.0)
        {
            self.high_line_width = patch.high_line_width.unwrap_or(self.high_line_width);
        }
        if patch
            .low_line_width
            .is_some_and(|value| value.is_finite() && value > 0.0)
        {
            self.low_line_width = patch.low_line_width.unwrap_or(self.low_line_width);
        }
        if patch
            .close_line_width
            .is_some_and(|value| value.is_finite() && value > 0.0)
        {
            self.close_line_width = patch.close_line_width.unwrap_or(self.close_line_width);
        }
        if patch
            .width_percent
            .is_some_and(|value| value.is_finite() && value > 0.0 && value <= 100.0)
        {
            self.width_percent = patch.width_percent.unwrap_or(self.width_percent);
        }
        if let Some(radius) = patch.radius {
            if radius.is_finite() && radius >= 0.0 {
                self.radius = Some(radius);
            }
        }
        set!(up_color);
        set!(down_color);
        set!(wick_up_color);
        set!(wick_down_color);
        set!(wick_visible);
        set!(low_color);
        set!(high_color);
        if patch.low_value.is_some_and(f64::is_finite) {
            self.low_value = patch.low_value.unwrap_or(self.low_value);
        }
        if patch.high_value.is_some_and(f64::is_finite) {
            self.high_value = patch.high_value.unwrap_or(self.high_value);
        }
        if patch.opacity.is_some_and(f64::is_finite) {
            self.opacity = patch.opacity.unwrap_or(self.opacity).clamp(0.0, 1.0);
        }
        set!(whisker_color);
        set!(lower_quartile_fill);
        set!(upper_quartile_fill);
        set!(outlier_color);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FeatureRow {
    pub time: i64,
    pub value: Option<FeatureValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FeatureSeriesState {
    pub kind: FeatureSeriesKind,
    pub options: FeatureSeriesOptions,
    pub rows: Vec<FeatureRow>,
}

impl FeatureSeriesState {
    fn new(kind: FeatureSeriesKind, patch: FeatureSeriesOptionsPatch) -> Self {
        let mut options = FeatureSeriesOptions::for_kind(kind);
        options.apply(patch);
        Self {
            kind,
            options,
            rows: Vec::new(),
        }
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        let row_payload = self
            .rows
            .iter()
            .filter_map(|row| row.value.as_ref())
            .map(|value| match value {
                FeatureValue::DualRangeHistogram { values }
                | FeatureValue::GroupedBars { values }
                | FeatureValue::StackedArea { values }
                | FeatureValue::StackedBars { values } => values.capacity() * size_of::<f64>(),
                FeatureValue::Heatmap { cells } => cells.capacity() * size_of::<HeatmapCell>(),
                FeatureValue::WhiskerBox { outliers, .. } => outliers.capacity() * size_of::<f64>(),
                _ => 0,
            })
            .sum::<usize>();
        self.rows.capacity() * size_of::<FeatureRow>()
            + row_payload
            + self.options.colors.capacity() * size_of::<Color>()
            + self.options.stacked_area_colors.capacity() * size_of::<StackedAreaColor>()
            + self.options.brush_ranges.capacity() * size_of::<BrushRange>()
            + self.options.border_radius.capacity() * size_of::<f64>()
    }
}

impl ChartEngine {
    pub fn add_feature_series(
        &mut self,
        kind: FeatureSeriesKind,
        options: FeatureSeriesOptionsPatch,
    ) -> SeriesId {
        let id = self.add_series(SeriesKind::Feature);
        self.configure_feature_series(id, kind, options);
        id
    }

    pub fn configure_feature_series(
        &mut self,
        id: SeriesId,
        kind: FeatureSeriesKind,
        options: FeatureSeriesOptionsPatch,
    ) -> bool {
        let Some(series) = self
            .series
            .iter_mut()
            .find(|series| series.id == id && !series.removed)
        else {
            return false;
        };
        series.kind = SeriesKind::Feature;
        series.feature = Some(FeatureSeriesState::new(kind, options));
        series.custom_frame = Default::default();
        self.data.set_rows_count_as_data(id, true);
        self.invalidate_frame_series(id);
        true
    }

    pub fn feature_series_kind(&self, id: SeriesId) -> Option<FeatureSeriesKind> {
        self.series_entry(id)?
            .feature
            .as_ref()
            .map(|feature| feature.kind)
    }

    pub fn feature_series_options_json(&self, id: SeriesId) -> Option<String> {
        let feature = self.series_entry(id)?.feature.as_ref()?;
        let options = &feature.options;
        let colors = if feature.kind == FeatureSeriesKind::StackedArea {
            serde_json::Value::Array(
                options
                    .stacked_area_colors
                    .iter()
                    .map(|color| {
                        serde_json::json!({
                            "line": color.line.to_css(),
                            "area": color.area.to_css(),
                        })
                    })
                    .collect(),
            )
        } else {
            serde_json::Value::Array(
                options
                    .colors
                    .iter()
                    .map(|color| serde_json::Value::String(color.to_css()))
                    .collect(),
            )
        };
        let mut value = serde_json::json!({
            "color": options.color.to_css(),
            "colors": colors,
            "line_color": options.line_color.to_css(),
            "top_color": options.top_color.to_css(),
            "bottom_color": options.bottom_color.to_css(),
            "line_width": options.line_width,
            "base_price": options.base_price,
            "brush_ranges": options.brush_ranges.iter().map(|range| serde_json::json!({
                "range": { "from": range.from, "to": range.to },
                "style": {
                    "line_color": range.style.line_color.to_css(),
                    "top_color": range.style.top_color.to_css(),
                    "bottom_color": range.style.bottom_color.to_css(),
                    "line_width": range.style.line_width,
                }
            })).collect::<Vec<_>>(),
            "border_radius": options.border_radius,
            "max_height": options.max_height,
            "cell_border_width": options.cell_border_width,
            "cell_border_color": options.cell_border_color.to_css(),
            "high_line_color": options.high_line_color.to_css(),
            "low_line_color": options.low_line_color.to_css(),
            "close_line_color": options.close_line_color.to_css(),
            "area_top_color": options.area_top_color.to_css(),
            "area_bottom_color": options.area_bottom_color.to_css(),
            "high_line_width": options.high_line_width,
            "low_line_width": options.low_line_width,
            "close_line_width": options.close_line_width,
            "width_percent": options.width_percent,
            "up_color": options.up_color.to_css(),
            "down_color": options.down_color.to_css(),
            "wick_up_color": options.wick_up_color.to_css(),
            "wick_down_color": options.wick_down_color.to_css(),
            "wick_visible": options.wick_visible,
            "low_color": options.low_color.to_css(),
            "high_color": options.high_color.to_css(),
            "low_value": options.low_value,
            "high_value": options.high_value,
            "opacity": options.opacity,
            "whisker_color": options.whisker_color.to_css(),
            "lower_quartile_fill": options.lower_quartile_fill.to_css(),
            "upper_quartile_fill": options.upper_quartile_fill.to_css(),
            "outlier_color": options.outlier_color.to_css(),
        });
        if let Some(radius) = options.radius {
            value["radius"] = serde_json::json!(radius);
        }
        serde_json::to_string(&value).ok()
    }

    pub fn apply_feature_series_options(
        &mut self,
        id: SeriesId,
        patch: FeatureSeriesOptionsPatch,
    ) -> bool {
        let Some(feature) = self
            .series_entry_mut(id)
            .and_then(|series| series.feature.as_mut())
        else {
            return false;
        };
        feature.options.apply(patch);
        self.invalidate_frame_series(id);
        true
    }

    pub fn set_feature_series_data(
        &mut self,
        id: SeriesId,
        input: Vec<FeatureDataPoint>,
    ) -> Result<ValidationReport, ValidationError> {
        let kind = self
            .series_entry(id)
            .and_then(|series| series.feature.as_ref())
            .map(|feature| feature.kind)
            .ok_or_else(|| match self.data.validate_series_id(id) {
                Err(SeriesIdError::Stale(id)) => ValidationError::StaleSeries(id),
                _ => ValidationError::UnknownSeries(id),
            })?;
        let input_was_empty = input.is_empty();
        let (mut rows, report) = sanitize_feature_rows(kind, input);
        // An explicitly empty set clears the series. A non-empty payload with no valid rows is a
        // rejected transaction and must not erase previously accepted chart data.
        if !input_was_empty && rows.is_empty() && report.dropped_invalid > 0 {
            return Ok(report);
        }
        let mut times = Vec::with_capacity(rows.len());
        let mut open = Vec::with_capacity(rows.len());
        let mut high = Vec::with_capacity(rows.len());
        let mut low = Vec::with_capacity(rows.len());
        let mut close = Vec::with_capacity(rows.len());
        for row in &rows {
            let values = row
                .value
                .as_ref()
                .map_or([f64::NAN; 4], FeatureValue::projection);
            times.push(row.time);
            open.push(values[0]);
            high.push(values[1]);
            low.push(values[2]);
            close.push(values[3]);
        }
        if !self.install_series_data(id, times, open, high, low, close) {
            return Err(ValidationError::UnknownSeries(id));
        }
        let retained = self
            .data
            .series_data(id)
            .map_or(0, |(times, _)| times.len());
        if retained < rows.len() {
            rows.drain(..rows.len() - retained);
        }
        if let Some(feature) = self
            .series_entry_mut(id)
            .and_then(|series| series.feature.as_mut())
        {
            feature.rows = rows;
        }
        self.invalidate_frame_series(id);
        Ok(report)
    }

    /// Append or replace one engine-owned advanced-series row. The raw payload and its canonical
    /// OHLC projection are updated together, so queries, autoscale, indicators, and every backend
    /// observe the same state.
    pub fn update_feature_series_data(
        &mut self,
        id: SeriesId,
        point: FeatureDataPoint,
    ) -> Result<ValidationReport, ValidationError> {
        let kind = self
            .series_entry(id)
            .and_then(|series| series.feature.as_ref())
            .map(|feature| feature.kind)
            .ok_or_else(|| match self.data.validate_series_id(id) {
                Err(SeriesIdError::Stale(id)) => ValidationError::StaleSeries(id),
                _ => ValidationError::UnknownSeries(id),
            })?;
        let (mut rows, report) = sanitize_feature_rows(kind, vec![point]);
        let Some(row) = rows.pop() else {
            return Ok(report);
        };
        let projection = row
            .value
            .as_ref()
            .map_or([f64::NAN; 4], FeatureValue::projection);
        let previous_generation = self.data.series_generation(id).unwrap_or(0);
        let from = self
            .data
            .series_data(id)
            .map(|(times, _)| {
                times
                    .binary_search(&row.time)
                    .unwrap_or_else(|position| position)
            })
            .unwrap_or_default();
        self.data.update_styled(id, row.time, projection, [None; 3]);
        if let Some(feature) = self
            .series_entry_mut(id)
            .and_then(|series| series.feature.as_mut())
        {
            match feature
                .rows
                .binary_search_by_key(&row.time, |item| item.time)
            {
                Ok(position) => feature.rows[position] = row,
                Err(position) => feature.rows.insert(position, row),
            }
        }
        let trimmed = self.enforce_series_cap(id);
        self.sync_time_points();
        self.update_indicators_after_change(
            id,
            crate::IndicatorChange {
                from: if trimmed { 0 } else { from },
                previous_generation,
                full_replace: trimmed,
            },
        );
        self.invalidate_frame_series(id);
        Ok(report)
    }

    /// Canonical feature payloads in post-sanitize engine order.
    pub fn feature_series_data(&self, id: SeriesId) -> Option<Vec<FeatureDataPoint>> {
        let feature = self.series_entry(id)?.feature.as_ref()?;
        Some(
            feature
                .rows
                .iter()
                .map(|row| FeatureDataPoint {
                    time: row.time as f64,
                    value: row.value.clone(),
                })
                .collect(),
        )
    }

    /// Canonical feature payload at a logical index, honoring the standard mismatch direction.
    pub fn feature_series_data_by_index(
        &self,
        id: SeriesId,
        logical_index: i64,
        mismatch: MismatchDirection,
    ) -> Option<FeatureDataPoint> {
        let feature = self.series_entry(id)?.feature.as_ref()?;
        let row = self.data.plot(id).search(logical_index, mismatch)?;
        let value = feature.rows.get(row)?;
        Some(FeatureDataPoint {
            time: value.time as f64,
            value: value.value.clone(),
        })
    }

    pub(crate) fn feature_series_capacity_bytes(&self) -> usize {
        self.series
            .iter()
            .filter_map(|series| series.feature.as_ref())
            .map(FeatureSeriesState::capacity_bytes)
            .sum()
    }

    pub(crate) fn truncate_feature_rows(&mut self, id: SeriesId, len: usize) {
        if let Some(feature) = self
            .series_entry_mut(id)
            .and_then(|series| series.feature.as_mut())
        {
            feature.rows.truncate(len);
        }
    }

    pub(crate) fn trim_feature_rows_front(&mut self, id: SeriesId, keep: usize) {
        if let Some(feature) = self
            .series_entry_mut(id)
            .and_then(|series| series.feature.as_mut())
        {
            let drop = feature.rows.len().saturating_sub(keep);
            feature.rows.drain(..drop);
        }
    }
}

fn sanitize_feature_rows(
    kind: FeatureSeriesKind,
    input: Vec<FeatureDataPoint>,
) -> (Vec<FeatureRow>, ValidationReport) {
    let mut report = ValidationReport::default();
    let mut rows = Vec::with_capacity(input.len());
    for (source, point) in input.into_iter().enumerate() {
        if !point.time.is_finite() {
            report.dropped_invalid += 1;
            report.dropped_non_finite += 1;
            continue;
        }
        if let Some(value) = &point.value {
            if value.kind() != kind || !value.finite_and_safe() {
                report.dropped_invalid += 1;
                report.dropped_non_finite += 1;
                continue;
            }
            if value.semantic_anomaly() {
                report.semantic_anomalies += 1;
            }
        }
        rows.push((point.time as i64, source, point.value));
    }
    report.reordered = rows.windows(2).any(|pair| pair[0].0 > pair[1].0);
    rows.sort_by_key(|(time, _, _)| *time);
    let mut sanitized: Vec<FeatureRow> = Vec::with_capacity(rows.len());
    for (time, _, value) in rows {
        if sanitized.last().is_some_and(|row| row.time == time) {
            sanitized.pop();
            report.dropped_duplicate += 1;
        }
        sanitized.push(FeatureRow { time, value });
    }
    report.accepted = sanitized.len();
    (sanitized, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_value(kind: FeatureSeriesKind, index: usize) -> FeatureValue {
        let value = 10.0 + index as f64;
        match kind {
            FeatureSeriesKind::BrushableArea => FeatureValue::BrushableArea { value },
            FeatureSeriesKind::DualRangeHistogram => FeatureValue::DualRangeHistogram {
                values: vec![value, value / 2.0, -value, -value / 2.0],
            },
            FeatureSeriesKind::GroupedBars => FeatureValue::GroupedBars {
                values: vec![value, value + 2.0, value - 2.0],
            },
            FeatureSeriesKind::Heatmap => FeatureValue::Heatmap {
                cells: vec![HeatmapCell {
                    low: value - 2.0,
                    high: value + 2.0,
                    amount: index as f64 / 2.0,
                    color: None,
                }],
            },
            FeatureSeriesKind::HlcArea => FeatureValue::HlcArea {
                high: value + 2.0,
                low: value - 2.0,
                close: value,
            },
            FeatureSeriesKind::PrettyHistogram => {
                FeatureValue::PrettyHistogram { value, color: None }
            }
            FeatureSeriesKind::RoundedCandles => FeatureValue::RoundedCandles {
                open: value - 1.0,
                high: value + 2.0,
                low: value - 2.0,
                close: value + 1.0,
            },
            FeatureSeriesKind::BackgroundShade => FeatureValue::BackgroundShade { value },
            FeatureSeriesKind::StackedArea => FeatureValue::StackedArea {
                values: vec![value, value / 2.0, value / 4.0],
            },
            FeatureSeriesKind::StackedBars => FeatureValue::StackedBars {
                values: vec![value, value / 2.0, value / 4.0],
            },
            FeatureSeriesKind::WhiskerBox => FeatureValue::WhiskerBox {
                quartiles: [value - 4.0, value - 2.0, value, value + 2.0, value + 4.0],
                outliers: vec![value + 5.0],
            },
        }
    }

    #[test]
    fn feature_data_is_engine_owned_sorted_and_last_wins() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::HlcArea,
            FeatureSeriesOptionsPatch::default(),
        );
        let report = chart
            .set_feature_series_data(
                0,
                vec![
                    FeatureDataPoint {
                        time: 2.0,
                        value: Some(FeatureValue::HlcArea {
                            high: 12.0,
                            low: 8.0,
                            close: 10.0,
                        }),
                    },
                    FeatureDataPoint {
                        time: 1.0,
                        value: Some(FeatureValue::HlcArea {
                            high: 11.0,
                            low: 7.0,
                            close: 9.0,
                        }),
                    },
                    FeatureDataPoint {
                        time: 2.0,
                        value: Some(FeatureValue::HlcArea {
                            high: 14.0,
                            low: 6.0,
                            close: 13.0,
                        }),
                    },
                ],
            )
            .unwrap();
        assert!(report.reordered);
        assert_eq!(report.dropped_duplicate, 1);
        let feature = chart.series_entry(0).unwrap().feature.as_ref().unwrap();
        assert_eq!(feature.rows.len(), 2);
        assert!(matches!(
            feature.rows[1].value,
            Some(FeatureValue::HlcArea { close: 13.0, .. })
        ));
        let (times, values) = chart.data.series_data(0).unwrap();
        assert_eq!(times, [1, 2]);
        assert_eq!(values[3], [9.0, 13.0]);
    }

    #[test]
    fn wrong_payload_kind_is_rejected_without_poisoning_scale_data() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::Heatmap,
            FeatureSeriesOptionsPatch::default(),
        );
        let report = chart
            .set_feature_series_data(
                0,
                vec![FeatureDataPoint {
                    time: 1.0,
                    value: Some(FeatureValue::BrushableArea { value: 10.0 }),
                }],
            )
            .unwrap();
        assert_eq!(report.dropped_invalid, 1);
        assert!(chart.data.plot(0).is_empty());
    }

    #[test]
    fn feature_streaming_query_and_retention_stay_aligned() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::GroupedBars,
            FeatureSeriesOptionsPatch::default(),
        );
        chart.set_series_max_points(0, Some(2));
        for index in 0..3 {
            chart
                .update_feature_series_data(
                    0,
                    FeatureDataPoint {
                        time: index as f64,
                        value: Some(sample_value(FeatureSeriesKind::GroupedBars, index)),
                    },
                )
                .unwrap();
        }
        let data = chart.feature_series_data(0).unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].time, 1.0);
        assert_eq!(data[1].time, 2.0);
        let point = chart
            .feature_series_data_by_index(0, 1, MismatchDirection::None)
            .unwrap();
        assert_eq!(point.time, 2.0);
        assert!(chart.memory_usage().feature_series_capacity_bytes > 0);
    }

    #[test]
    fn every_feature_kind_builds_shared_frame_geometry() {
        let kinds = [
            FeatureSeriesKind::BrushableArea,
            FeatureSeriesKind::DualRangeHistogram,
            FeatureSeriesKind::GroupedBars,
            FeatureSeriesKind::Heatmap,
            FeatureSeriesKind::HlcArea,
            FeatureSeriesKind::PrettyHistogram,
            FeatureSeriesKind::RoundedCandles,
            FeatureSeriesKind::BackgroundShade,
            FeatureSeriesKind::StackedArea,
            FeatureSeriesKind::StackedBars,
            FeatureSeriesKind::WhiskerBox,
        ];
        for kind in kinds {
            let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
            chart.configure_feature_series(0, kind, FeatureSeriesOptionsPatch::default());
            chart
                .set_feature_series_data(
                    0,
                    (0..3)
                        .map(|index| FeatureDataPoint {
                            time: index as f64,
                            value: Some(sample_value(kind, index)),
                        })
                        .collect(),
                )
                .unwrap();
            chart.time_scale.set_width(800.0);
            chart.fit_content();
            let frame = chart.build_frame();
            let segment = chart
                .frame_series_segments(0)
                .iter()
                .find(|segment| segment.series_id == Some(0))
                .copied()
                .unwrap_or_else(|| panic!("{kind:?} emitted no retained series segment"));
            assert!(segment.end > segment.start, "{kind:?} emitted no geometry");
            assert!(frame.panes[0].main.len() >= segment.end);
        }
    }

    #[test]
    fn heatmap_emits_every_full_width_price_cell_with_host_shader_colors() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::Heatmap,
            FeatureSeriesOptionsPatch::default(),
        );
        let color = Color::rgb(12, 34, 56);
        chart
            .set_feature_series_data(
                0,
                (0..4)
                    .map(|time| FeatureDataPoint {
                        time: time as f64,
                        value: Some(FeatureValue::Heatmap {
                            cells: (0..3)
                                .map(|cell| HeatmapCell {
                                    low: cell as f64 * 10.0,
                                    high: (cell + 1) as f64 * 10.0,
                                    amount: 50.0,
                                    color: Some(color),
                                })
                                .collect(),
                        }),
                    })
                    .collect(),
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        let frame = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .copied()
            .unwrap();
        let cells = frame.panes[0].main[segment.start..segment.end]
            .iter()
            .filter_map(|primitive| match primitive {
                nucleuscharts_render::draw_list::Prim::Rect { rect, color: fill }
                    if *fill == color =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(cells.len(), 12);
        assert!(cells.iter().all(|cell| cell.w > 1 && cell.h > 1));
        let time_columns = cells
            .chunks_exact(3)
            .map(|group| group[0].x)
            .collect::<Vec<_>>();
        assert_eq!(time_columns.len(), 4);
        assert!(time_columns.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn dual_range_columns_and_background_shade_match_official_bar_geometry() {
        let mut dual = ChartEngine::new(800.0, 500.0, 1.0);
        dual.configure_feature_series(
            0,
            FeatureSeriesKind::DualRangeHistogram,
            FeatureSeriesOptionsPatch::default(),
        );
        dual.set_feature_series_data(
            0,
            (0..6)
                .map(|time| FeatureDataPoint {
                    time: time as f64,
                    value: Some(FeatureValue::DualRangeHistogram {
                        values: vec![20.0, 10.0, -20.0, -10.0],
                    }),
                })
                .collect(),
        )
        .unwrap();
        dual.time_scale.set_width(800.0);
        dual.fit_content();
        let frame = dual.build_frame();
        let segment = dual
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .copied()
            .unwrap();
        let columns = frame.panes[0].main[segment.start..segment.end]
            .iter()
            .filter_map(|primitive| match primitive {
                nucleuscharts_render::draw_list::Prim::RoundRect { x, w, .. } => {
                    Some((*x as i32, *w as i32))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(columns.len(), 24);
        assert!(columns.chunks_exact(4).all(|group| {
            group
                .iter()
                .all(|column| *column == group[0] && column.1 > 1)
        }));

        let mut background = ChartEngine::new(800.0, 500.0, 2.0);
        background.configure_feature_series(
            0,
            FeatureSeriesKind::BackgroundShade,
            FeatureSeriesOptionsPatch {
                low_value: Some(0.0),
                high_value: Some(100.0),
                ..FeatureSeriesOptionsPatch::default()
            },
        );
        background
            .set_feature_series_data(
                0,
                vec![
                    FeatureDataPoint {
                        time: 0.0,
                        value: Some(FeatureValue::BackgroundShade { value: 0.0 }),
                    },
                    FeatureDataPoint {
                        time: 1.0,
                        value: Some(FeatureValue::BackgroundShade { value: 50.0 }),
                    },
                    FeatureDataPoint {
                        time: 2.0,
                        value: Some(FeatureValue::BackgroundShade { value: 100.0 }),
                    },
                    FeatureDataPoint {
                        time: 3.0,
                        value: None,
                    },
                    FeatureDataPoint {
                        time: 4.0,
                        value: Some(FeatureValue::BackgroundShade { value: 25.0 }),
                    },
                ],
            )
            .unwrap();
        background.time_scale.set_width(800.0);
        background.fit_content();
        background.build_frame();
        assert!(background.series_base_value(0, 0).is_none());
        assert!(background.panes[0].price_scale.price_range().is_none());
        assert!(background.hit_test_one_series(0, 400.0, 250.0).is_none());

        let line = background.add_series(SeriesKind::Line);
        let times = [0.0, 1.0, 2.0, 3.0, 4.0];
        let values = [0.0, 50.0, 100.0, 75.0, 25.0];
        background
            .set_series_data(line, &times, &values, &values, &values, &values)
            .unwrap();
        background.time_scale.set_width(800.0);
        background.fit_content();
        let frame = background.build_frame();
        let segment = background
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .copied()
            .unwrap();
        let fields = frame.panes[0].main[segment.start..segment.end]
            .iter()
            .filter_map(|primitive| match primitive {
                nucleuscharts_render::draw_list::Prim::Rect { rect, color } => {
                    Some((*rect, *color))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            fields.len(),
            4,
            "one full-height strip per non-whitespace bar"
        );
        assert_eq!(
            fields.iter().map(|(_, color)| *color).collect::<Vec<_>>(),
            vec![
                Color::rgb(50, 50, 255),
                Color::rgb(153, 50, 153),
                Color::rgb(255, 50, 50),
                Color::rgb(101, 50, 204),
            ]
        );
        assert!(fields
            .iter()
            .all(|(rect, _)| rect.y == 0 && rect.h == 1_000));
        let spacing = background.time_scale.bar_spacing();
        let expected = [0, 1, 2, 4]
            .map(|logical| {
                let x = background.time_scale.index_to_coordinate(logical);
                let left = ((x - spacing / 2.0) * 2.0).round() as i32;
                let right = ((x + spacing / 2.0) * 2.0).round() as i32;
                (left, (right - left).max(1))
            })
            .to_vec();
        assert_eq!(
            fields
                .iter()
                .map(|(rect, _)| (rect.x, rect.w))
                .collect::<Vec<_>>(),
            expected,
            "each value owns exactly its upstream full-bar-width interval"
        );
        assert!(
            fields[2].0.x + fields[2].0.w < fields[3].0.x,
            "whitespace must remain unshaded"
        );
        assert!(background.series_base_value(0, 0).is_none());
    }

    #[test]
    fn background_shade_sparse_rows_stay_time_aligned_after_pan() {
        let mut chart = ChartEngine::new(480.0, 260.0, 2.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::BackgroundShade,
            FeatureSeriesOptionsPatch::default(),
        );
        let values = (0..60)
            .map(|index| match index {
                0 => 0.0,
                1 => 50.0,
                2 => 100.0,
                _ => ((index * 37 + (index % 5) * 11) % 101) as f64,
            })
            .collect::<Vec<_>>();
        chart
            .set_feature_series_data(
                0,
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| FeatureDataPoint {
                        time: index as f64,
                        value: (![9, 31].contains(&index))
                            .then_some(FeatureValue::BackgroundShade { value: *value }),
                    })
                    .collect(),
            )
            .unwrap();
        let line = chart.add_series(SeriesKind::Line);
        let times = (0..60).map(|index| index as f64).collect::<Vec<_>>();
        chart
            .set_series_data(line, &times, &values, &values, &values, &values)
            .unwrap();
        chart.time_scale.set_width(480.0);
        chart.set_visible_logical_range(20.0, 35.0);
        let frame = chart.build_frame();
        let segment = chart
            .frame_series_segments(0)
            .iter()
            .find(|segment| segment.series_id == Some(0))
            .copied()
            .unwrap();
        let colors = frame.panes[0].main[segment.start..segment.end]
            .iter()
            .filter_map(|primitive| match primitive {
                nucleuscharts_render::draw_list::Prim::Rect { color, .. } => Some(*color),
                _ => None,
            })
            .collect::<Vec<_>>();
        let expected = (20..=35)
            .filter(|index| *index != 31)
            .map(|index| {
                let amount = values[index] / 100.0;
                Color::rgb(
                    (50.0 + 205.0 * amount).round() as u8,
                    50,
                    (255.0 - 205.0 * amount).round() as u8,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(colors, expected);
    }

    #[test]
    fn official_feature_defaults_and_shader_colors_are_preserved() {
        let mut heatmap = ChartEngine::new(800.0, 500.0, 1.0);
        heatmap.configure_feature_series(
            0,
            FeatureSeriesKind::Heatmap,
            FeatureSeriesOptionsPatch::default(),
        );
        heatmap
            .set_feature_series_data(
                0,
                (0..2)
                    .map(|time| FeatureDataPoint {
                        time: time as f64,
                        value: Some(FeatureValue::Heatmap {
                            cells: vec![HeatmapCell {
                                low: 10.0,
                                high: 11.0,
                                amount: 0.5,
                                color: (time == 1).then_some(Color::rgb(12, 34, 56)),
                            }],
                        }),
                    })
                    .collect(),
            )
            .unwrap();
        heatmap.time_scale.set_width(800.0);
        heatmap.fit_content();
        assert!(heatmap.build_frame().panes[0].main.iter().any(|primitive| {
            matches!(primitive, nucleuscharts_render::draw_list::Prim::Rect { color, .. }
                if *color == Color::rgba(0, 101, 1, 153))
        }));
        assert!(heatmap.build_frame().panes[0].main.iter().any(|primitive| {
            matches!(primitive, nucleuscharts_render::draw_list::Prim::Rect { color, .. }
                if *color == Color::rgb(12, 34, 56))
        }));

        let mut background = ChartEngine::new(800.0, 500.0, 1.0);
        background.configure_feature_series(
            0,
            FeatureSeriesKind::BackgroundShade,
            FeatureSeriesOptionsPatch {
                opacity: Some(0.01),
                ..FeatureSeriesOptionsPatch::default()
            },
        );
        background
            .set_feature_series_data(
                0,
                (0..2)
                    .map(|time| FeatureDataPoint {
                        time: time as f64,
                        value: Some(FeatureValue::BackgroundShade { value: 50.0 }),
                    })
                    .collect(),
            )
            .unwrap();
        background.time_scale.set_width(800.0);
        background.fit_content();
        assert!(background.build_frame().panes[0]
            .main
            .iter()
            .any(|primitive| {
                matches!(primitive, nucleuscharts_render::draw_list::Prim::Rect { color, .. }
                if color.a() == 255 && color.r() == 153 && color.g() == 50 && color.b() == 153)
            }));
    }

    #[test]
    fn pretty_histogram_paints_the_official_one_bar_right_edge_overscan() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::PrettyHistogram,
            FeatureSeriesOptionsPatch::default(),
        );
        chart
            .set_feature_series_data(
                0,
                (0..8)
                    .map(|time| FeatureDataPoint {
                        time: time as f64,
                        value: Some(FeatureValue::PrettyHistogram {
                            value: 10.0 + time as f64,
                            color: None,
                        }),
                    })
                    .collect(),
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.set_visible_logical_range(2.0, 4.0);
        let frame = chart.build_frame();
        let columns = frame.panes[0]
            .main
            .iter()
            .filter(|primitive| {
                matches!(primitive, nucleuscharts_render::draw_list::Prim::RoundRect {
                    fill,
                    ..
                } if *fill == Color::rgb(0xd6, 0x38, 0x64))
            })
            .count();
        assert_eq!(
            columns, 4,
            "strict bars 2..=4 plus the official right overscan"
        );
    }

    #[test]
    fn rounded_candles_preserve_official_wicks_and_canvas_radius_normalization() {
        let wick = Color::rgb(9, 8, 7);
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.configure_feature_series(
            0,
            FeatureSeriesKind::RoundedCandles,
            FeatureSeriesOptionsPatch {
                radius: Some(100.0),
                wick_visible: Some(false),
                wick_up_color: Some(wick),
                ..FeatureSeriesOptionsPatch::default()
            },
        );
        chart
            .set_feature_series_data(
                0,
                vec![FeatureDataPoint {
                    time: 1.0,
                    value: Some(FeatureValue::RoundedCandles {
                        open: 10.0,
                        high: 12.0,
                        low: 8.0,
                        close: 10.0,
                    }),
                }],
            )
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        let frame = chart.build_frame();
        assert!(frame.panes[0].main.iter().any(|primitive| {
            matches!(primitive, nucleuscharts_render::draw_list::Prim::Rect { color, .. }
                if *color == wick)
        }));
        let body = frame.panes[0]
            .main
            .iter()
            .find_map(|primitive| match primitive {
                nucleuscharts_render::draw_list::Prim::RoundRect { h, radii, .. } => {
                    Some((*h, *radii))
                }
                _ => None,
            })
            .expect("rounded candle body");
        assert!(body.1.iter().all(|radius| *radius <= body.0 / 2.0));
    }
}
