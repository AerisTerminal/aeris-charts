use std::collections::HashSet;
use std::num::NonZeroU32;

use nucleuscharts_core::format::time_formatter::MonthNames;
use nucleuscharts_core::scale::general_scale::{
    BandScale, LinearScale, LogScale, PointScale, SymLogScale, DEFAULT_SYMLOG_CONSTANT,
};
use nucleuscharts_core::scale::time_tick_marks::{civil_from_timestamp, days_from_civil};
use nucleuscharts_core::style::DEFAULT_BORDER_RGB;
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{LineStyle, Prim};

use crate::{
    axis_metrics::{AxisMetrics, AXIS_FONT_SCALE},
    AxisBand, AxisFrame, AxisLabel, AxisLabelCorners, AxisRotatedLabel, AxisTextAlign,
    AxisTextMidpoint, CategoryScaleType, ChartEngine, ChartError, ContinuousScaleType, ErrorCode,
    HorizontalDomain, PaneId, PriceScaleSide,
};

pub const MAX_GENERAL_AXES: usize = 128;
pub const MAX_GENERAL_AXIS_ID_BYTES: usize = 128;
pub const MAX_GENERAL_AXIS_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_AXIS_TICKS: u16 = 512;
pub const MAX_GENERAL_AXIS_CATEGORIES: usize = 65_536;
pub const MAX_GENERAL_AXIS_CATEGORY_BYTES: usize = 1_048_576;
pub const MAX_GENERAL_TEMPORAL_MILLISECONDS: i64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AxisDimension {
    X,
    Y,
    Angle,
    Radius,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AxisPosition {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralScaleType {
    Linear,
    Logarithmic,
    SymmetricLog,
    Temporal,
    Band,
    Point,
    RadialLinear,
    AngularCategory,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GeneralAxisDomain {
    #[default]
    Auto,
    Numeric([f64; 2]),
    Temporal([i64; 2]),
    Category(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeneralAxisOptions {
    pub id: String,
    pub pane: usize,
    pub dimension: AxisDimension,
    pub position: Option<AxisPosition>,
    pub scale: GeneralScaleType,
    pub domain: GeneralAxisDomain,
    pub reverse: bool,
    pub visible: bool,
    pub title: Option<String>,
    pub tick_count: Option<u16>,
    pub min_tick_gap: f64,
    pub band_padding_inner: f64,
    pub band_padding_outer: f64,
    pub zero_line: bool,
    pub grid_visible: bool,
}

impl GeneralAxisOptions {
    pub fn new(
        id: impl Into<String>,
        pane: usize,
        dimension: AxisDimension,
        scale: GeneralScaleType,
    ) -> Self {
        Self {
            id: id.into(),
            pane,
            dimension,
            position: None,
            scale,
            domain: GeneralAxisDomain::Auto,
            reverse: false,
            visible: true,
            title: None,
            tick_count: None,
            min_tick_gap: 4.0,
            band_padding_inner: 0.1,
            band_padding_outer: 0.1,
            zero_line: true,
            grid_visible: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GeneralAxisHandle(NonZeroU32);

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralAxis {
    handle: GeneralAxisHandle,
    id: String,
    pane_id: PaneId,
    dimension: AxisDimension,
    position: Option<AxisPosition>,
    scale: GeneralScaleType,
    domain: GeneralAxisDomain,
    reverse: bool,
    visible: bool,
    title: Option<String>,
    tick_count: Option<u16>,
    min_tick_gap: f64,
    band_padding_inner: f64,
    band_padding_outer: f64,
    zero_line: bool,
    grid_visible: bool,
    /// Runtime viewport for continuous axes. Configured/auto domain remains canonical and this is
    /// reset independently, matching the financial distinction between data range and visible range.
    view_domain: Option<GeneralAxisDomain>,
    /// Negotiated strip width for vertical axes. Horizontal strip heights are derived from the
    /// shared font metrics because they do not depend on glyph advance.
    layout_thickness: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum NumericAxisScale {
    Linear(LinearScale),
    Logarithmic(LogScale),
    SymmetricLog(SymLogScale),
}

impl NumericAxisScale {
    pub(crate) fn new(
        scale: GeneralScaleType,
        domain: [f64; 2],
        range_from: f64,
        range_to: f64,
    ) -> Option<Self> {
        Some(match scale {
            GeneralScaleType::Linear => {
                Self::Linear(LinearScale::new(domain[0], domain[1], range_from, range_to).ok()?)
            }
            GeneralScaleType::Logarithmic => {
                Self::Logarithmic(LogScale::new(domain[0], domain[1], range_from, range_to).ok()?)
            }
            GeneralScaleType::SymmetricLog => Self::SymmetricLog(
                SymLogScale::new(
                    domain[0],
                    domain[1],
                    range_from,
                    range_to,
                    DEFAULT_SYMLOG_CONSTANT,
                )
                .ok()?,
            ),
            _ => return None,
        })
    }

    pub(crate) fn coordinate(self, value: f64) -> Option<f64> {
        match self {
            Self::Linear(scale) => scale.coordinate(value),
            Self::Logarithmic(scale) => scale.coordinate(value),
            Self::SymmetricLog(scale) => scale.coordinate(value),
        }
    }

    pub(crate) fn invert(self, coordinate: f64) -> Option<f64> {
        match self {
            Self::Linear(scale) => scale.invert(coordinate),
            Self::Logarithmic(scale) => scale.invert(coordinate),
            Self::SymmetricLog(scale) => scale.invert(coordinate),
        }
    }

    fn ticks(self, target_count: usize) -> Vec<f64> {
        match self {
            Self::Linear(scale) => scale.ticks(target_count),
            Self::Logarithmic(scale) => scale.ticks(target_count),
            Self::SymmetricLog(scale) => scale.ticks(target_count),
        }
    }
}

impl GeneralAxis {
    #[cfg(test)]
    pub(crate) fn handle(&self) -> GeneralAxisHandle {
        self.handle
    }

    #[doc(hidden)]
    pub fn handle_token(&self) -> u32 {
        self.handle.0.get()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    pub fn dimension(&self) -> AxisDimension {
        self.dimension
    }

    pub fn position(&self) -> Option<AxisPosition> {
        self.position
    }

    pub fn scale(&self) -> GeneralScaleType {
        self.scale
    }

    pub fn domain(&self) -> &GeneralAxisDomain {
        &self.domain
    }

    pub fn reverse(&self) -> bool {
        self.reverse
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn tick_count(&self) -> Option<u16> {
        self.tick_count
    }

    pub fn min_tick_gap(&self) -> f64 {
        self.min_tick_gap
    }

    pub fn band_padding_inner(&self) -> f64 {
        self.band_padding_inner
    }

    pub fn band_padding_outer(&self) -> f64 {
        self.band_padding_outer
    }

    pub fn zero_line(&self) -> bool {
        self.zero_line
    }

    pub fn grid_visible(&self) -> bool {
        self.grid_visible
    }

    fn estimated_bytes(&self) -> usize {
        self.id.capacity()
            + self.title.as_ref().map_or(0, String::capacity)
            + match &self.domain {
                GeneralAxisDomain::Category(values) => {
                    values.capacity() * std::mem::size_of::<String>()
                        + values.iter().map(String::capacity).sum::<usize>()
                }
                _ => 0,
            }
    }
}

pub(crate) struct GeneralAxisRegistry {
    axes: Vec<GeneralAxis>,
    next_handle: u32,
}

impl GeneralAxisRegistry {
    pub(crate) fn new() -> Self {
        Self {
            axes: Vec::new(),
            next_handle: 1,
        }
    }

    pub(crate) fn has_issued_handles(&self) -> bool {
        self.next_handle != 1
    }

    fn insert(
        &mut self,
        pane_id: PaneId,
        options: GeneralAxisOptions,
    ) -> Result<GeneralAxisHandle, ChartError> {
        if self.axes.len() >= MAX_GENERAL_AXES {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_AXES} general axes"
            )));
        }
        if self.axes.iter().any(|axis| axis.id == options.id) {
            return Err(invalid(format!(
                "general axis id {:?} already exists",
                options.id
            )));
        }
        validate_options(&options)?;
        let handle = NonZeroU32::new(self.next_handle)
            .map(GeneralAxisHandle)
            .ok_or_else(|| resource("general axis identity space is exhausted"))?;
        self.next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or_else(|| resource("general axis identity space is exhausted"))?;
        let position = resolved_position(options.dimension, options.position);
        self.axes.push(GeneralAxis {
            handle,
            id: options.id,
            pane_id,
            dimension: options.dimension,
            position,
            scale: options.scale,
            domain: options.domain,
            reverse: options.reverse,
            visible: options.visible,
            title: options.title,
            tick_count: options.tick_count,
            min_tick_gap: options.min_tick_gap,
            band_padding_inner: options.band_padding_inner,
            band_padding_outer: options.band_padding_outer,
            zero_line: options.zero_line,
            grid_visible: options.grid_visible,
            view_domain: None,
            layout_thickness: 0.0,
        });
        Ok(handle)
    }

    fn get(&self, id: &str) -> Option<&GeneralAxis> {
        self.axes.iter().find(|axis| axis.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut GeneralAxis> {
        self.axes.iter_mut().find(|axis| axis.id == id)
    }

    fn remove(&mut self, id: &str) -> bool {
        let Some(index) = self.axes.iter().position(|axis| axis.id == id) else {
            return false;
        };
        self.axes.remove(index);
        true
    }

    pub(crate) fn remove_pane(&mut self, pane_id: Option<PaneId>) {
        let Some(pane_id) = pane_id else {
            return;
        };
        self.axes.retain(|axis| axis.pane_id != pane_id);
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &GeneralAxis> {
        self.axes.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut GeneralAxis> {
        self.axes.iter_mut()
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.axes.capacity() * std::mem::size_of::<GeneralAxis>()
            + self
                .axes
                .iter()
                .map(GeneralAxis::estimated_bytes)
                .sum::<usize>()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.axes.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneralPlotRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct GeneralAxisTick {
    coordinate: f64,
    label: String,
    align: AxisTextAlign,
}

impl ChartEngine {
    pub(crate) fn measure_general_axis_widths<F>(&mut self, measure: &F, allow_shrink: bool)
    where
        F: Fn(&str, bool) -> f64,
    {
        let metrics = self.axis_metrics();
        let month_names = &self.month_names;
        let fallback = AxisMetrics::price_strip_width(AxisMetrics::DEFAULT_TEXT_WIDTH, 0.0);
        let effective_domains: Vec<_> = self
            .general_axes
            .iter()
            .filter_map(|axis| {
                self.effective_general_axis_domain(axis)
                    .map(|domain| (axis.handle, domain))
            })
            .collect();
        for axis in self.general_axes.iter_mut() {
            if !axis.visible || !matches!(axis.dimension, AxisDimension::Y) {
                axis.layout_thickness = 0.0;
                continue;
            }
            let widest_tick = effective_domains
                .iter()
                .find(|(handle, _)| *handle == axis.handle)
                .map(|(_, domain)| tick_labels_for_domain(axis, domain, month_names))
                .unwrap_or_default()
                .into_iter()
                .map(|label| measure(&label, false))
                .fold(0.0_f64, f64::max);
            let tick_strip = AxisMetrics::price_strip_width(
                widest_tick.max(AxisMetrics::DEFAULT_TEXT_WIDTH),
                0.0,
            )
            .max(fallback);
            // A vertical general-axis title owns a dedicated rotated-text lane rather than
            // competing with tick labels for the same horizontal strip. This keeps long titles
            // from inflating the tick area and prevents the title from covering the center tick.
            let measured = tick_strip + vertical_axis_title_lane(axis, metrics);
            axis.layout_thickness = if allow_shrink || axis.layout_thickness <= 0.0 {
                measured
            } else {
                axis.layout_thickness.max(measured)
            };
        }
    }

    pub(crate) fn general_axis_side_width(&self, pane_index: usize, side: PriceScaleSide) -> f64 {
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return 0.0;
        };
        let position = match side {
            PriceScaleSide::Left => AxisPosition::Left,
            PriceScaleSide::Right => AxisPosition::Right,
        };
        let budget = self.general_vertical_axis_budget(pane_index);
        fitting_thickness(
            self.general_axes
                .iter()
                .filter(|axis| axis.pane_id == pane_id && axis.visible)
                .filter(|axis| axis.position == Some(position))
                .map(vertical_axis_thickness),
            budget,
        )
    }

    pub(crate) fn general_plot_rect(&self, pane_index: usize) -> Option<GeneralPlotRect> {
        let pane = self.panes.get(pane_index)?;
        let pane_id = pane.stable_id?;
        let metrics = self.axis_metrics();
        let side_height = |position| {
            fitting_thickness(
                self.general_axes
                    .iter()
                    .filter(|axis| axis.pane_id == pane_id && axis.visible)
                    .filter(|axis| axis.position == Some(position))
                    .map(|axis| horizontal_axis_thickness(axis, metrics)),
                (pane.height - 1.0).max(0.0) * 0.45,
            )
        };
        let top = side_height(AxisPosition::Top);
        let bottom = side_height(AxisPosition::Bottom);
        Some(GeneralPlotRect {
            x: self.pane_left,
            y: pane.top + top,
            width: self.pane_w,
            height: (pane.height - top - bottom).max(1.0),
        })
    }

    pub(crate) fn append_general_grid_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return;
        };
        let grid = &self.options.get().grid;
        let has_rules = self.general_axes.iter().any(|axis| {
            axis.pane_id == pane_id
                && axis.visible
                && (axis.zero_line
                    || axis.grid_visible
                        && match axis.dimension {
                            AxisDimension::X => grid.vert_lines.visible,
                            AxisDimension::Y => grid.horz_lines.visible,
                            AxisDimension::Angle | AxisDimension::Radius => false,
                        })
        });
        if !has_rules {
            return;
        }
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let metrics = self.axis_metrics();
        let fallback = Color::rgb(
            DEFAULT_BORDER_RGB.0,
            DEFAULT_BORDER_RGB.1,
            DEFAULT_BORDER_RGB.2,
        );
        let vertical_color = Color::parse_css(&grid.vert_lines.color).unwrap_or(fallback);
        let horizontal_color = Color::parse_css(&grid.horz_lines.color).unwrap_or(fallback);
        let zero_color = Color::parse_css(&self.options.get().right_price_scale.border_color)
            .unwrap_or(fallback);
        let vertical_style = crate::line_style_from_u8(grid.vert_lines.style);
        let horizontal_style = crate::line_style_from_u8(grid.horz_lines.style);
        let mut vertical_grid = Vec::new();
        let mut horizontal_grid = Vec::new();
        let mut vertical_zero = Vec::new();
        let mut horizontal_zero = Vec::new();
        let mut seen_vertical_grid = HashSet::new();
        let mut seen_horizontal_grid = HashSet::new();
        let mut seen_vertical_zero = HashSet::new();
        let mut seen_horizontal_zero = HashSet::new();

        for axis in self
            .general_axes
            .iter()
            .filter(|axis| axis.pane_id == pane_id && axis.visible)
        {
            let range = match axis.dimension {
                AxisDimension::X => {
                    if axis.reverse {
                        (plot.width, 0.0)
                    } else {
                        (0.0, plot.width)
                    }
                }
                AxisDimension::Y => {
                    let bottom = plot.y + plot.height;
                    if axis.reverse {
                        (plot.y, bottom)
                    } else {
                        (bottom, plot.y)
                    }
                }
                AxisDimension::Angle | AxisDimension::Radius => continue,
            };
            let Some(domain) = self.effective_general_axis_domain(axis) else {
                continue;
            };

            if axis.grid_visible
                && match axis.dimension {
                    AxisDimension::X => grid.vert_lines.visible,
                    AxisDimension::Y => grid.horz_lines.visible,
                    AxisDimension::Angle | AxisDimension::Radius => false,
                }
            {
                for tick in axis_ticks(axis, &domain, range.0, range.1, metrics, &self.month_names)
                {
                    match axis.dimension {
                        AxisDimension::X => {
                            let coordinate = (tick.coordinate * hpr).round() as i32;
                            if seen_vertical_grid.insert(coordinate) {
                                vertical_grid.push(coordinate);
                            }
                        }
                        AxisDimension::Y => {
                            let coordinate = (tick.coordinate * vpr).round() as i32;
                            if seen_horizontal_grid.insert(coordinate) {
                                horizontal_grid.push(coordinate);
                            }
                        }
                        AxisDimension::Angle | AxisDimension::Radius => unreachable!(),
                    }
                }
            }

            if axis.zero_line {
                let GeneralAxisDomain::Numeric(domain) = domain else {
                    continue;
                };
                if domain[0] > 0.0 || domain[1] < 0.0 {
                    continue;
                }
                let Some(coordinate) = NumericAxisScale::new(axis.scale, domain, range.0, range.1)
                    .and_then(|scale| scale.coordinate(0.0))
                else {
                    continue;
                };
                match axis.dimension {
                    AxisDimension::X => {
                        let coordinate = (coordinate * hpr).round() as i32;
                        if seen_vertical_zero.insert(coordinate) {
                            vertical_zero.push(coordinate);
                        }
                    }
                    AxisDimension::Y => {
                        let coordinate = (coordinate * vpr).round() as i32;
                        if seen_horizontal_zero.insert(coordinate) {
                            horizontal_zero.push(coordinate);
                        }
                    }
                    AxisDimension::Angle | AxisDimension::Radius => unreachable!(),
                }
            }
        }

        let top = (plot.y * vpr).round() as i32;
        let bottom = ((plot.y + plot.height) * vpr).round() as i32;
        let right = (plot.width * hpr).round() as i32;
        let vertical_width = hpr.floor().max(1.0) as i32;
        let horizontal_width = vpr.floor().max(1.0) as i32;
        for x in vertical_grid {
            if !seen_vertical_zero.contains(&x) {
                out.push(Prim::VLine {
                    x,
                    y0: top,
                    y1: bottom,
                    width: vertical_width,
                    style: vertical_style,
                    color: vertical_color,
                });
            }
        }
        for y in horizontal_grid {
            if !seen_horizontal_zero.contains(&y) {
                out.push(Prim::HLine {
                    y,
                    x0: 0,
                    x1: right,
                    width: horizontal_width,
                    style: horizontal_style,
                    color: horizontal_color,
                });
            }
        }
        for x in vertical_zero {
            out.push(Prim::VLine {
                x,
                y0: top,
                y1: bottom,
                width: vertical_width,
                style: LineStyle::Solid,
                color: zero_color,
            });
        }
        for y in horizontal_zero {
            out.push(Prim::HLine {
                y,
                x0: 0,
                x1: right,
                width: horizontal_width,
                style: LineStyle::Solid,
                color: zero_color,
            });
        }
    }

    pub(crate) fn append_general_axis_frame<F>(&self, out: &mut AxisFrame, measure: &F)
    where
        F: Fn(&str, bool) -> f64,
    {
        if self.general_axes.iter().next().is_none() {
            return;
        }
        let metrics = self.axis_metrics();
        let text_color = self.primary_text_color();
        let border_color = Color::parse_css(&self.options.get().right_price_scale.border_color)
            .unwrap_or_else(|| Color::rgb(54, 58, 69));

        for (pane_index, pane) in self.panes.iter().enumerate() {
            let Some(pane_id) = pane.stable_id else {
                continue;
            };
            let Some(plot) = self.general_plot_rect(pane_index) else {
                continue;
            };
            let financial_left = self.financial_axis_side_width(pane_index, PriceScaleSide::Left);
            let financial_right = self.financial_axis_side_width(pane_index, PriceScaleSide::Right);
            let vertical_budget = self.general_vertical_axis_budget(pane_index);
            let horizontal_budget = (pane.height - 1.0).max(0.0) * 0.45;
            let mut top_offset = 0.0;
            let mut bottom_offset = 0.0;
            let mut left_offset = 0.0;
            let mut right_offset = 0.0;

            for axis in self
                .general_axes
                .iter()
                .filter(|axis| axis.pane_id == pane_id && axis.visible)
            {
                let Some(position) = axis.position else {
                    continue;
                };
                let (strip_x, strip_y, strip_w, strip_h, label_x, label_y, align) = match position {
                    AxisPosition::Top => {
                        let thickness = horizontal_axis_thickness(axis, metrics);
                        if top_offset + thickness > horizontal_budget {
                            continue;
                        }
                        let y = pane.top + top_offset;
                        top_offset += thickness;
                        (
                            plot.x,
                            y,
                            plot.width,
                            thickness,
                            plot.x + plot.width / 2.0,
                            y + thickness - metrics.axis / 2.0 - 2.0,
                            AxisTextAlign::Center,
                        )
                    }
                    AxisPosition::Bottom => {
                        let thickness = horizontal_axis_thickness(axis, metrics);
                        if bottom_offset + thickness > horizontal_budget {
                            continue;
                        }
                        let y = plot.y + plot.height + bottom_offset;
                        bottom_offset += thickness;
                        (
                            plot.x,
                            y,
                            plot.width,
                            thickness,
                            plot.x + plot.width / 2.0,
                            y + 1.0 + AxisMetrics::TICK_LENGTH + 4.0 + metrics.axis / 2.0,
                            AxisTextAlign::Center,
                        )
                    }
                    AxisPosition::Left => {
                        let thickness = vertical_axis_thickness(axis);
                        if left_offset + thickness > vertical_budget {
                            continue;
                        }
                        let x = self.pane_left - financial_left - left_offset - thickness;
                        left_offset += thickness;
                        (
                            x,
                            plot.y,
                            thickness,
                            plot.height,
                            x + thickness - AxisMetrics::PRICE_TEXT_INSET,
                            plot.y + plot.height / 2.0,
                            AxisTextAlign::Right,
                        )
                    }
                    AxisPosition::Right => {
                        let thickness = vertical_axis_thickness(axis);
                        if right_offset + thickness > vertical_budget {
                            continue;
                        }
                        let x = self.pane_left + self.pane_w + financial_right + right_offset;
                        right_offset += thickness;
                        (
                            x,
                            plot.y,
                            thickness,
                            plot.height,
                            x + AxisMetrics::PRICE_TEXT_INSET,
                            plot.y + plot.height / 2.0,
                            AxisTextAlign::Left,
                        )
                    }
                };

                let (line_x, line_y, line_w, line_h) = match position {
                    AxisPosition::Top => (strip_x, strip_y + strip_h - 1.0, strip_w, 1.0),
                    AxisPosition::Bottom => (strip_x, strip_y, strip_w, 1.0),
                    AxisPosition::Left => (strip_x + strip_w - 1.0, strip_y, 1.0, strip_h),
                    AxisPosition::Right => (strip_x, strip_y, 1.0, strip_h),
                };
                out.bands.push(AxisBand {
                    x: line_x,
                    y: line_y,
                    width: line_w,
                    height: line_h,
                    color: border_color,
                });

                let range = match axis.dimension {
                    AxisDimension::X => {
                        let from = plot.x;
                        let to = plot.x + plot.width;
                        if axis.reverse {
                            (to, from)
                        } else {
                            (from, to)
                        }
                    }
                    AxisDimension::Y => {
                        let from = plot.y + plot.height;
                        let to = plot.y;
                        if axis.reverse {
                            (to, from)
                        } else {
                            (from, to)
                        }
                    }
                    AxisDimension::Angle | AxisDimension::Radius => continue,
                };
                let Some(domain) = self.effective_general_axis_domain(axis) else {
                    continue;
                };
                let ticks = axis_ticks(axis, &domain, range.0, range.1, metrics, &self.month_names);
                let ticks =
                    collision_filtered_ticks(axis, ticks, measure, metrics, range.0, range.1);
                for tick in ticks {
                    let (x, y) = match axis.dimension {
                        AxisDimension::X => (tick.coordinate, label_y),
                        AxisDimension::Y => (label_x, tick.coordinate),
                        AxisDimension::Angle | AxisDimension::Radius => unreachable!(),
                    };
                    let tick_align = if axis.dimension == AxisDimension::X {
                        tick.align
                    } else {
                        align
                    };
                    out.labels
                        .push(plain_axis_label(tick.label, x, y, text_color, tick_align));
                }
                if let Some(title) = axis.title.as_ref() {
                    match position {
                        AxisPosition::Top => out.labels.push(plain_axis_label(
                            title.clone(),
                            label_x,
                            strip_y + metrics.axis / 2.0 + 2.0,
                            text_color,
                            AxisTextAlign::Center,
                        )),
                        AxisPosition::Bottom => out.labels.push(plain_axis_label(
                            title.clone(),
                            label_x,
                            strip_y + strip_h - metrics.axis / 2.0 - 2.0,
                            text_color,
                            AxisTextAlign::Center,
                        )),
                        AxisPosition::Left | AxisPosition::Right => {
                            let lane = vertical_axis_title_lane(axis, metrics);
                            let x = if position == AxisPosition::Left {
                                strip_x + lane / 2.0
                            } else {
                                strip_x + strip_w - lane / 2.0
                            };
                            out.rotated_labels.push(AxisRotatedLabel {
                                text: title.clone(),
                                x,
                                y: label_y,
                                color: text_color,
                                align: AxisTextAlign::Center,
                                font_scale: AXIS_FONT_SCALE,
                                bold: false,
                                angle: if position == AxisPosition::Left {
                                    -std::f64::consts::FRAC_PI_2
                                } else {
                                    std::f64::consts::FRAC_PI_2
                                },
                            });
                        }
                    }
                }
            }
        }
    }

    fn financial_axis_side_width(&self, pane_index: usize, side: PriceScaleSide) -> f64 {
        self.panes[pane_index]
            .ordered_side_targets(side)
            .into_iter()
            .filter(|target| self.price_scale_visible_for(pane_index, *target))
            .filter_map(|target| self.price_scale_axis_width(pane_index, target))
            .sum()
    }

    fn general_vertical_axis_budget(&self, pane_index: usize) -> f64 {
        let financial = self.financial_axis_side_width(pane_index, PriceScaleSide::Left)
            + self.financial_axis_side_width(pane_index, PriceScaleSide::Right);
        (self.css_width - financial - 1.0).max(0.0) * 0.45
    }

    pub(crate) fn effective_general_axis_domain(
        &self,
        axis: &GeneralAxis,
    ) -> Option<GeneralAxisDomain> {
        if let Some(view) = axis.view_domain.as_ref() {
            return Some(view.clone());
        }
        self.base_general_axis_domain(axis)
    }

    fn base_general_axis_domain(&self, axis: &GeneralAxis) -> Option<GeneralAxisDomain> {
        if axis.domain != GeneralAxisDomain::Auto {
            return Some(axis.domain.clone());
        }

        match (axis.dimension, axis.scale) {
            (AxisDimension::X, GeneralScaleType::Band | GeneralScaleType::Point) => {
                let mut categories = Vec::new();
                let mut seen = HashSet::new();
                for series in self.general_series_iter().filter(|series| {
                    series.visible()
                        && series.pane_id() == axis.pane_id
                        && series.x_axis_id() == axis.id
                }) {
                    let Some(dataset) = self.general_dataset(series.dataset()) else {
                        continue;
                    };
                    let Some(values) = dataset.categories() else {
                        continue;
                    };
                    for value in values {
                        if seen.insert(value.clone()) {
                            categories.push(value.clone());
                        }
                    }
                }
                for reference in self.general_reference_iter().filter(|reference| {
                    reference.pane_id() == axis.pane_id && reference.options().extend_domain()
                }) {
                    for value in general_reference_values_for_axis(reference.options(), &axis.id)
                        .into_iter()
                        .flatten()
                    {
                        if let crate::GeneralReferenceValue::Category(value) = value {
                            if seen.insert(value.clone()) {
                                categories.push(value.clone());
                            }
                        }
                    }
                }
                (!categories.is_empty()).then_some(GeneralAxisDomain::Category(categories))
            }
            (AxisDimension::Y, GeneralScaleType::Band | GeneralScaleType::Point) => {
                let mut categories = Vec::new();
                let mut seen = HashSet::new();
                for series in self.general_series_iter().filter(|series| {
                    series.visible()
                        && series.pane_id() == axis.pane_id
                        && series.y_axis_id() == axis.id
                        && matches!(
                            series.kind(),
                            crate::GeneralSeriesKind::HorizontalBar
                                | crate::GeneralSeriesKind::HeatmapGrid
                        )
                }) {
                    let Some(dataset) = self.general_dataset(series.dataset()) else {
                        continue;
                    };
                    let values = if series.kind() == crate::GeneralSeriesKind::HeatmapGrid {
                        dataset.heatmap_y_categories()
                    } else {
                        dataset.categories()
                    };
                    let Some(values) = values else {
                        continue;
                    };
                    for value in values {
                        if seen.insert(value.clone()) {
                            categories.push(value.clone());
                        }
                    }
                }
                for reference in self.general_reference_iter().filter(|reference| {
                    reference.pane_id() == axis.pane_id && reference.options().extend_domain()
                }) {
                    for value in general_reference_values_for_axis(reference.options(), &axis.id)
                        .into_iter()
                        .flatten()
                    {
                        if let crate::GeneralReferenceValue::Category(value) = value {
                            if seen.insert(value.clone()) {
                                categories.push(value.clone());
                            }
                        }
                    }
                }
                (!categories.is_empty()).then_some(GeneralAxisDomain::Category(categories))
            }
            (AxisDimension::X, GeneralScaleType::Temporal) => {
                let mut bounds: Option<(i64, i64)> = None;
                for series in self.general_series_iter().filter(|series| {
                    series.visible()
                        && series.pane_id() == axis.pane_id
                        && series.x_axis_id() == axis.id
                }) {
                    let Some(dataset) = self.general_dataset(series.dataset()) else {
                        continue;
                    };
                    let Some(values) = dataset.temporal_x_epoch_ms() else {
                        continue;
                    };
                    for (index, &value) in values.iter().enumerate() {
                        bounds = Some(match bounds {
                            Some((low, high)) => (low.min(value), high.max(value)),
                            None => (value, value),
                        });
                        if series.kind() == crate::GeneralSeriesKind::ErrorBar
                            && dataset.y_is_valid(index)
                        {
                            if let Some(low_values) = dataset.x_low() {
                                if dataset.x_low_is_valid(index) {
                                    let low = low_values[index] as i64;
                                    bounds = Some(match bounds {
                                        Some((from, to)) => (from.min(low), to.max(low)),
                                        None => (low, low),
                                    });
                                }
                            }
                            if let Some(high_values) = dataset.x_high() {
                                if dataset.x_high_is_valid(index) {
                                    let high = high_values[index] as i64;
                                    bounds = Some(match bounds {
                                        Some((from, to)) => (from.min(high), to.max(high)),
                                        None => (high, high),
                                    });
                                }
                            }
                        }
                    }
                }
                for reference in self.general_reference_iter().filter(|reference| {
                    reference.pane_id() == axis.pane_id && reference.options().extend_domain()
                }) {
                    for value in general_reference_values_for_axis(reference.options(), &axis.id)
                        .into_iter()
                        .flatten()
                    {
                        if let crate::GeneralReferenceValue::Temporal(value) = value {
                            bounds = Some(match bounds {
                                Some((low, high)) => (low.min(*value), high.max(*value)),
                                None => (*value, *value),
                            });
                        }
                    }
                }
                bounds.and_then(|(low, high)| expanded_temporal_domain(low, high))
            }
            (
                AxisDimension::X | AxisDimension::Y,
                GeneralScaleType::Linear
                | GeneralScaleType::Logarithmic
                | GeneralScaleType::SymmetricLog,
            ) => {
                let mut bounds: Option<(f64, f64)> = None;
                let mut include_zero = false;
                for series in self.general_series_iter().filter(|series| {
                    series.visible()
                        && series.pane_id() == axis.pane_id
                        && if axis.dimension == AxisDimension::X {
                            series.x_axis_id() == axis.id
                        } else {
                            series.y_axis_id() == axis.id
                        }
                }) {
                    let Some(dataset) = self.general_dataset(series.dataset()) else {
                        continue;
                    };
                    if axis.dimension == AxisDimension::Y {
                        if series.kind() == crate::GeneralSeriesKind::HeatmapGrid {
                            if let Some(values) = dataset.heatmap_y_numeric() {
                                for &value in values {
                                    if axis.scale == GeneralScaleType::Logarithmic && value <= 0.0 {
                                        continue;
                                    }
                                    extend_numeric_bounds(&mut bounds, value);
                                }
                                continue;
                            }
                        }
                        if series.kind() == crate::GeneralSeriesKind::Column {
                            include_zero = true;
                            continue;
                        }
                        if series.kind() == crate::GeneralSeriesKind::XyArea
                            && series.stack_id().is_some()
                        {
                            include_zero = true;
                            continue;
                        }
                        if series.kind() == crate::GeneralSeriesKind::RangeArea {
                            let Some(low_values) = dataset.low() else {
                                continue;
                            };
                            for (index, &high) in dataset.y().iter().enumerate() {
                                let low = low_values[index];
                                if !dataset.y_is_valid(index)
                                    || !dataset.low_is_valid(index)
                                    || (axis.scale == GeneralScaleType::Logarithmic
                                        && (low <= 0.0 || high <= 0.0))
                                {
                                    continue;
                                }
                                extend_numeric_bounds(&mut bounds, low);
                                extend_numeric_bounds(&mut bounds, high);
                            }
                            continue;
                        }
                        if series.kind() == crate::GeneralSeriesKind::BoxPlot {
                            let (
                                Some(min_values),
                                Some(max_values),
                                Some(q1_values),
                                Some(q3_values),
                            ) = (
                                dataset.low(),
                                dataset.high(),
                                dataset.x_low(),
                                dataset.x_high(),
                            )
                            else {
                                continue;
                            };
                            for (index, &median) in dataset.y().iter().enumerate() {
                                if !dataset.y_is_valid(index)
                                    || !dataset.low_is_valid(index)
                                    || !dataset.high_is_valid(index)
                                    || !dataset.x_low_is_valid(index)
                                    || !dataset.x_high_is_valid(index)
                                {
                                    continue;
                                }
                                let values = [
                                    min_values[index],
                                    q1_values[index],
                                    median,
                                    q3_values[index],
                                    max_values[index],
                                ];
                                if axis.scale == GeneralScaleType::Logarithmic
                                    && values.iter().any(|value| *value <= 0.0)
                                {
                                    continue;
                                }
                                extend_numeric_bounds(&mut bounds, min_values[index]);
                                extend_numeric_bounds(&mut bounds, max_values[index]);
                            }
                            continue;
                        }
                        if series.kind() == crate::GeneralSeriesKind::ErrorBar {
                            let (Some(low_values), Some(high_values)) =
                                (dataset.low(), dataset.high())
                            else {
                                continue;
                            };
                            for (index, &value) in dataset.y().iter().enumerate() {
                                if !dataset.y_is_valid(index)
                                    || (axis.scale == GeneralScaleType::Logarithmic && value <= 0.0)
                                {
                                    continue;
                                }
                                extend_numeric_bounds(&mut bounds, value);
                                if dataset.low_is_valid(index) {
                                    let low = low_values[index];
                                    if axis.scale != GeneralScaleType::Logarithmic || low > 0.0 {
                                        extend_numeric_bounds(&mut bounds, low);
                                    }
                                }
                                if dataset.high_is_valid(index) {
                                    let high = high_values[index];
                                    if axis.scale != GeneralScaleType::Logarithmic || high > 0.0 {
                                        extend_numeric_bounds(&mut bounds, high);
                                    }
                                }
                            }
                            continue;
                        }
                        for (index, &value) in dataset.y().iter().enumerate() {
                            if !dataset.y_is_valid(index)
                                || (axis.scale == GeneralScaleType::Logarithmic && value <= 0.0)
                            {
                                continue;
                            }
                            extend_numeric_bounds(&mut bounds, value);
                        }
                    } else if series.kind() == crate::GeneralSeriesKind::HorizontalBar {
                        include_zero = true;
                        if series.stack_id().is_some() {
                            continue;
                        }
                        for (index, &value) in dataset.y().iter().enumerate() {
                            if dataset.y_is_valid(index) {
                                extend_numeric_bounds(&mut bounds, value);
                            }
                        }
                    } else if let Some(values) = dataset.numeric_x() {
                        for (index, &value) in values.iter().enumerate() {
                            if axis.scale == GeneralScaleType::Logarithmic && value <= 0.0 {
                                continue;
                            }
                            extend_numeric_bounds(&mut bounds, value);
                            if series.kind() == crate::GeneralSeriesKind::ErrorBar
                                && dataset.y_is_valid(index)
                            {
                                if let Some(low_values) = dataset.x_low() {
                                    if dataset.x_low_is_valid(index) {
                                        let low = low_values[index];
                                        if axis.scale != GeneralScaleType::Logarithmic || low > 0.0
                                        {
                                            extend_numeric_bounds(&mut bounds, low);
                                        }
                                    }
                                }
                                if let Some(high_values) = dataset.x_high() {
                                    if dataset.x_high_is_valid(index) {
                                        let high = high_values[index];
                                        if axis.scale != GeneralScaleType::Logarithmic || high > 0.0
                                        {
                                            extend_numeric_bounds(&mut bounds, high);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                for reference in self.general_reference_iter().filter(|reference| {
                    reference.pane_id() == axis.pane_id && reference.options().extend_domain()
                }) {
                    for value in general_reference_values_for_axis(reference.options(), &axis.id)
                        .into_iter()
                        .flatten()
                    {
                        if let crate::GeneralReferenceValue::Numeric(value) = value {
                            extend_numeric_bounds(&mut bounds, *value);
                        }
                    }
                }
                if axis.dimension == AxisDimension::Y {
                    if let Some((low, high)) =
                        self.general_column_axis_bounds(axis.pane_id, axis.id())
                    {
                        extend_numeric_bounds(&mut bounds, low);
                        extend_numeric_bounds(&mut bounds, high);
                        include_zero = true;
                    }
                    if let Some((low, high)) =
                        self.general_area_stack_axis_bounds(axis.pane_id, axis.id())
                    {
                        extend_numeric_bounds(&mut bounds, low);
                        extend_numeric_bounds(&mut bounds, high);
                        include_zero = true;
                    }
                } else if let Some((low, high)) =
                    self.general_horizontal_bar_axis_bounds(axis.pane_id, axis.id())
                {
                    extend_numeric_bounds(&mut bounds, low);
                    extend_numeric_bounds(&mut bounds, high);
                    include_zero = true;
                }
                if include_zero && axis.scale != GeneralScaleType::Logarithmic {
                    extend_numeric_bounds(&mut bounds, 0.0);
                }
                bounds.and_then(|(low, high)| {
                    expanded_numeric_domain_for_scale(axis.scale, low, high)
                        .map(GeneralAxisDomain::Numeric)
                })
            }
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn general_axis_effective_domain(&self, id: &str) -> Option<GeneralAxisDomain> {
        let axis = self.general_axis(id)?;
        self.effective_general_axis_domain(axis)
    }

    #[doc(hidden)]
    pub fn pan_general_axis(&mut self, id: &str, fraction: f64) -> Result<(), ChartError> {
        if !fraction.is_finite() {
            return Err(invalid("general axis pan fraction must be finite"));
        }
        let (scale_type, domain) = {
            let axis = self.general_axis(id).ok_or_else(|| {
                ChartError::new(ErrorCode::InvalidHandle, "general axis is stale")
            })?;
            let domain = self
                .effective_general_axis_domain(axis)
                .ok_or_else(|| invalid("general axis has no domain to pan"))?;
            (axis.scale, domain)
        };
        let view =
            match domain {
                GeneralAxisDomain::Numeric(domain) => {
                    let scale = NumericAxisScale::new(scale_type, domain, 0.0, 1.0)
                        .ok_or_else(|| invalid("general axis numeric transform is invalid"))?;
                    let from = scale
                        .invert(fraction)
                        .ok_or_else(|| invalid("general axis pan exceeds the numeric transform"))?;
                    let to = scale
                        .invert(1.0 + fraction)
                        .ok_or_else(|| invalid("general axis pan exceeds the numeric transform"))?;
                    GeneralAxisDomain::Numeric([from.min(to), from.max(to)])
                }
                GeneralAxisDomain::Temporal(domain) => {
                    let scale = temporal_linear_scale(domain, 0.0, 1.0)
                        .ok_or_else(|| invalid("general axis temporal transform is invalid"))?;
                    let from = scale.invert(fraction).ok_or_else(|| {
                        invalid("general axis pan exceeds the temporal transform")
                    })?;
                    let to = scale.invert(1.0 + fraction).ok_or_else(|| {
                        invalid("general axis pan exceeds the temporal transform")
                    })?;
                    GeneralAxisDomain::Temporal(temporal_domain_from_f64(from, to).ok_or_else(
                        || invalid("general axis pan exceeds safe epoch milliseconds"),
                    )?)
                }
                _ => return Err(invalid("only numeric and temporal general axes can pan")),
            };
        let axis = self
            .general_axes
            .get_mut(id)
            .ok_or_else(|| ChartError::new(ErrorCode::InvalidHandle, "general axis is stale"))?;
        axis.view_domain = Some(view);
        self.invalidate_frame_all();
        Ok(())
    }

    #[doc(hidden)]
    pub fn zoom_general_axis(
        &mut self,
        id: &str,
        factor: f64,
        anchor_value: f64,
    ) -> Result<(), ChartError> {
        if !factor.is_finite() || factor <= 0.0 || !anchor_value.is_finite() {
            return Err(invalid(
                "general axis zoom requires a positive finite factor and finite anchor",
            ));
        }
        let (scale_type, domain) = {
            let axis = self.general_axis(id).ok_or_else(|| {
                ChartError::new(ErrorCode::InvalidHandle, "general axis is stale")
            })?;
            let domain = self
                .effective_general_axis_domain(axis)
                .ok_or_else(|| invalid("general axis has no domain to zoom"))?;
            (axis.scale, domain)
        };
        let view =
            match domain {
                GeneralAxisDomain::Numeric(domain) => {
                    let scale = NumericAxisScale::new(scale_type, domain, 0.0, 1.0)
                        .ok_or_else(|| invalid("general axis numeric transform is invalid"))?;
                    let anchor = scale
                        .coordinate(anchor_value)
                        .filter(|value| (0.0..=1.0).contains(value))
                        .ok_or_else(|| {
                            invalid("general axis zoom anchor must be inside the visible domain")
                        })?;
                    let from_unit = anchor + (0.0 - anchor) / factor;
                    let to_unit = anchor + (1.0 - anchor) / factor;
                    let from = scale.invert(from_unit).ok_or_else(|| {
                        invalid("general axis zoom exceeds the numeric transform")
                    })?;
                    let to = scale.invert(to_unit).ok_or_else(|| {
                        invalid("general axis zoom exceeds the numeric transform")
                    })?;
                    GeneralAxisDomain::Numeric([from.min(to), from.max(to)])
                }
                GeneralAxisDomain::Temporal(domain) => {
                    if anchor_value.fract() != 0.0
                        || anchor_value.abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as f64
                    {
                        return Err(invalid(
                        "general temporal axis zoom anchor must be a safe whole epoch millisecond",
                    ));
                    }
                    let scale = temporal_linear_scale(domain, 0.0, 1.0)
                        .ok_or_else(|| invalid("general axis temporal transform is invalid"))?;
                    let anchor = scale
                        .coordinate(anchor_value)
                        .filter(|value| (0.0..=1.0).contains(value))
                        .ok_or_else(|| {
                            invalid("general axis zoom anchor must be inside the visible domain")
                        })?;
                    let from_unit = anchor + (0.0 - anchor) / factor;
                    let to_unit = anchor + (1.0 - anchor) / factor;
                    let from = scale.invert(from_unit).ok_or_else(|| {
                        invalid("general axis zoom exceeds the temporal transform")
                    })?;
                    let to = scale.invert(to_unit).ok_or_else(|| {
                        invalid("general axis zoom exceeds the temporal transform")
                    })?;
                    GeneralAxisDomain::Temporal(temporal_domain_from_f64(from, to).ok_or_else(
                        || invalid("general axis zoom exceeds safe epoch milliseconds"),
                    )?)
                }
                _ => return Err(invalid("only numeric and temporal general axes can zoom")),
            };
        let axis = self
            .general_axes
            .get_mut(id)
            .ok_or_else(|| ChartError::new(ErrorCode::InvalidHandle, "general axis is stale"))?;
        axis.view_domain = Some(view);
        self.invalidate_frame_all();
        Ok(())
    }

    #[doc(hidden)]
    pub fn reset_general_axis_view(&mut self, id: &str) -> bool {
        let Some(axis) = self.general_axes.get_mut(id) else {
            return false;
        };
        if axis.view_domain.take().is_some() {
            self.invalidate_frame_all();
        }
        true
    }
}

fn general_reference_values_for_axis<'a>(
    options: &'a crate::GeneralReferenceOptions,
    axis_id: &str,
) -> [Option<&'a crate::GeneralReferenceValue>; 2] {
    match options {
        crate::GeneralReferenceOptions::Line {
            axis_id: reference_axis,
            value,
            ..
        } if reference_axis == axis_id => [Some(value), None],
        crate::GeneralReferenceOptions::Dot {
            x_axis_id,
            y_axis_id,
            x,
            y,
            ..
        } if x_axis_id == axis_id => [Some(x), None],
        crate::GeneralReferenceOptions::Dot {
            x_axis_id,
            y_axis_id,
            x,
            y,
            ..
        } if y_axis_id == axis_id => [Some(y), None],
        crate::GeneralReferenceOptions::Region {
            x_axis_id,
            x_from,
            x_to,
            ..
        } if x_axis_id == axis_id => [Some(x_from), Some(x_to)],
        crate::GeneralReferenceOptions::Region {
            y_axis_id,
            y_from,
            y_to,
            ..
        } if y_axis_id == axis_id => [Some(y_from), Some(y_to)],
        _ => [None, None],
    }
}

fn fitting_thickness<I>(thicknesses: I, budget: f64) -> f64
where
    I: Iterator<Item = f64>,
{
    thicknesses
        .scan(0.0, |used, thickness| {
            if *used + thickness <= budget {
                *used += thickness;
                Some(Some(thickness))
            } else {
                Some(None)
            }
        })
        .flatten()
        .sum()
}

fn vertical_axis_thickness(axis: &GeneralAxis) -> f64 {
    axis.layout_thickness.max(AxisMetrics::price_strip_width(
        AxisMetrics::DEFAULT_TEXT_WIDTH,
        0.0,
    ))
}

fn vertical_axis_title_lane(axis: &GeneralAxis, metrics: AxisMetrics) -> f64 {
    if axis.title.is_some() {
        // Rotated titles need one text-height lane plus breathing room from the tick column.
        metrics.axis + 8.0
    } else {
        0.0
    }
}

fn horizontal_axis_thickness(axis: &GeneralAxis, metrics: AxisMetrics) -> f64 {
    let rows = if axis.title.is_some() { 2.0 } else { 1.0 };
    (1.0 + AxisMetrics::TICK_LENGTH + 4.0 + rows * (metrics.axis + 4.0)).ceil()
}

fn tick_labels_for_domain(
    axis: &GeneralAxis,
    domain: &GeneralAxisDomain,
    month_names: &MonthNames,
) -> Vec<String> {
    match (&axis.scale, domain) {
        (
            GeneralScaleType::Linear
            | GeneralScaleType::Logarithmic
            | GeneralScaleType::SymmetricLog,
            GeneralAxisDomain::Numeric(domain),
        ) => {
            let Some(scale) = NumericAxisScale::new(axis.scale, *domain, 0.0, 1.0) else {
                return Vec::new();
            };
            format_numeric_ticks(&scale.ticks(axis.tick_count.unwrap_or(6) as usize))
        }
        (GeneralScaleType::Band | GeneralScaleType::Point, GeneralAxisDomain::Category(values)) => {
            values
                .iter()
                .take(MAX_GENERAL_AXIS_TICKS as usize)
                .cloned()
                .collect()
        }
        (GeneralScaleType::Temporal, GeneralAxisDomain::Temporal(domain)) => {
            let interval = temporal_tick_interval(*domain, axis.tick_count.unwrap_or(6) as usize);
            temporal_tick_values(*domain, interval)
                .into_iter()
                .map(|value| format_temporal_tick(value, interval, month_names))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn axis_ticks(
    axis: &GeneralAxis,
    domain: &GeneralAxisDomain,
    range_from: f64,
    range_to: f64,
    metrics: AxisMetrics,
    month_names: &MonthNames,
) -> Vec<GeneralAxisTick> {
    match (&axis.scale, domain) {
        (
            GeneralScaleType::Linear
            | GeneralScaleType::Logarithmic
            | GeneralScaleType::SymmetricLog,
            GeneralAxisDomain::Numeric(domain),
        ) => {
            let Some(scale) = NumericAxisScale::new(axis.scale, *domain, range_from, range_to)
            else {
                return Vec::new();
            };
            let span = (range_to - range_from).abs();
            let target = axis.tick_count.map_or_else(
                || {
                    (span / (metrics.axis + axis.min_tick_gap + 8.0))
                        .floor()
                        .clamp(2.0, 10.0) as usize
                },
                usize::from,
            );
            let values = scale.ticks(target);
            let labels = format_numeric_ticks(&values);
            values
                .into_iter()
                .zip(labels)
                .filter_map(|(value, label)| {
                    scale.coordinate(value).map(|coordinate| GeneralAxisTick {
                        coordinate,
                        label,
                        align: AxisTextAlign::Center,
                    })
                })
                .collect()
        }
        (GeneralScaleType::Temporal, GeneralAxisDomain::Temporal(domain)) => {
            let Some(scale) = temporal_linear_scale(*domain, range_from, range_to) else {
                return Vec::new();
            };
            let span = (range_to - range_from).abs();
            let target = axis.tick_count.map_or_else(
                || {
                    (span / (metrics.axis + axis.min_tick_gap + 8.0))
                        .floor()
                        .clamp(2.0, 10.0) as usize
                },
                usize::from,
            );
            let interval = temporal_tick_interval(*domain, target);
            temporal_tick_values(*domain, interval)
                .into_iter()
                .filter_map(|value| {
                    scale
                        .coordinate(value as f64)
                        .map(|coordinate| GeneralAxisTick {
                            coordinate,
                            label: format_temporal_tick(value, interval, month_names),
                            align: AxisTextAlign::Center,
                        })
                })
                .collect()
        }
        (GeneralScaleType::Band, GeneralAxisDomain::Category(values)) => {
            let Ok(scale) = BandScale::new(
                values.len(),
                range_from,
                range_to,
                axis.band_padding_inner,
                axis.band_padding_outer,
                0.5,
            ) else {
                return Vec::new();
            };
            sampled_category_ticks(axis, values, |index| scale.center(index))
        }
        (GeneralScaleType::Point, GeneralAxisDomain::Category(values)) => {
            let Ok(scale) = PointScale::new(
                values.len(),
                range_from,
                range_to,
                axis.band_padding_outer,
                0.5,
            ) else {
                return Vec::new();
            };
            sampled_category_ticks(axis, values, |index| scale.coordinate(index))
        }
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TemporalTickInterval {
    Fixed(i64),
    Months(i64),
    Years(i64),
}

impl TemporalTickInterval {
    fn approximate_milliseconds(self) -> i64 {
        const DAY: i64 = 86_400_000;
        match self {
            Self::Fixed(milliseconds) => milliseconds,
            Self::Months(months) => months.saturating_mul(30 * DAY),
            Self::Years(years) => years.saturating_mul(365 * DAY),
        }
    }
}

fn temporal_linear_scale(domain: [i64; 2], range_from: f64, range_to: f64) -> Option<LinearScale> {
    LinearScale::new(domain[0] as f64, domain[1] as f64, range_from, range_to).ok()
}

fn temporal_domain_from_f64(from: f64, to: f64) -> Option<[i64; 2]> {
    if !from.is_finite() || !to.is_finite() {
        return None;
    }
    let from = from.round();
    let to = to.round();
    let limit = MAX_GENERAL_TEMPORAL_MILLISECONDS as f64;
    if from.abs() > limit || to.abs() > limit || from >= to {
        return None;
    }
    Some([from as i64, to as i64])
}

fn temporal_tick_interval(domain: [i64; 2], target: usize) -> TemporalTickInterval {
    const FIXED: [i64; 22] = [
        1,
        5,
        10,
        50,
        100,
        250,
        500,
        1_000,
        5_000,
        15_000,
        30_000,
        60_000,
        300_000,
        900_000,
        1_800_000,
        3_600_000,
        10_800_000,
        21_600_000,
        43_200_000,
        86_400_000,
        172_800_000,
        1_209_600_000,
    ];
    let span = domain[1] - domain[0];
    let divisor = i64::try_from(target.saturating_sub(1).max(1)).unwrap_or(1);
    let desired = (span + divisor - 1) / divisor;
    if let Some(interval) = FIXED.into_iter().find(|interval| *interval >= desired) {
        return TemporalTickInterval::Fixed(interval);
    }
    for interval in [
        TemporalTickInterval::Months(1),
        TemporalTickInterval::Months(3),
        TemporalTickInterval::Months(6),
        TemporalTickInterval::Years(1),
        TemporalTickInterval::Years(2),
        TemporalTickInterval::Years(5),
        TemporalTickInterval::Years(10),
        TemporalTickInterval::Years(25),
        TemporalTickInterval::Years(50),
        TemporalTickInterval::Years(100),
    ] {
        if interval.approximate_milliseconds() >= desired {
            return interval;
        }
    }

    let year_milliseconds = 365 * 86_400_000;
    let desired_years = ((desired + year_milliseconds - 1) / year_milliseconds).max(1);
    let mut magnitude = 1_i64;
    while magnitude <= desired_years / 10 {
        magnitude = magnitude.saturating_mul(10);
    }
    for multiplier in [1_i64, 2, 5, 10] {
        let step = magnitude.saturating_mul(multiplier);
        if step >= desired_years {
            return TemporalTickInterval::Years(step);
        }
    }
    TemporalTickInterval::Years(desired_years)
}

fn temporal_tick_values(domain: [i64; 2], interval: TemporalTickInterval) -> Vec<i64> {
    const DAY: i64 = 86_400_000;
    let mut values = Vec::new();
    let mut push_until_end = |mut value: i64, step: i64| {
        while value <= domain[1] && values.len() < usize::from(MAX_GENERAL_AXIS_TICKS) {
            if value >= domain[0] {
                values.push(value);
            }
            let Some(next) = value.checked_add(step) else {
                break;
            };
            value = next;
        }
    };

    match interval {
        TemporalTickInterval::Fixed(step) => {
            let first = domain[0].div_euclid(step).checked_mul(step);
            if let Some(first) = first.and_then(|value| {
                if value < domain[0] {
                    value.checked_add(step)
                } else {
                    Some(value)
                }
            }) {
                push_until_end(first, step);
            }
        }
        TemporalTickInterval::Months(step) => {
            let (year, month, _) = civil_from_timestamp(domain[0].div_euclid(1_000));
            let Some(total_month) = year
                .checked_mul(12)
                .and_then(|value| value.checked_add(i64::from(month) - 1))
            else {
                return values;
            };
            let aligned = total_month.div_euclid(step).saturating_mul(step);
            let mut current_month = aligned;
            if calendar_month_milliseconds(current_month).is_none_or(|value| value < domain[0]) {
                current_month = current_month.saturating_add(step);
            }
            if calendar_month_milliseconds(current_month).is_some() {
                while values.len() < usize::from(MAX_GENERAL_AXIS_TICKS) {
                    let Some(value) = calendar_month_milliseconds(current_month) else {
                        break;
                    };
                    if value > domain[1] {
                        break;
                    }
                    values.push(value);
                    let Some(next) = current_month.checked_add(step) else {
                        break;
                    };
                    current_month = next;
                }
            }
        }
        TemporalTickInterval::Years(step) => {
            let (year, _, _) = civil_from_timestamp(domain[0].div_euclid(1_000));
            let aligned = year.div_euclid(step).saturating_mul(step);
            let first_year = days_from_civil(aligned, 1, 1)
                .and_then(|days| days.checked_mul(DAY))
                .filter(|value| *value >= domain[0])
                .map_or_else(|| aligned.saturating_add(step), |_| aligned);
            let mut current_year = first_year;
            while values.len() < usize::from(MAX_GENERAL_AXIS_TICKS) {
                let Some(value) =
                    days_from_civil(current_year, 1, 1).and_then(|days| days.checked_mul(DAY))
                else {
                    break;
                };
                if value > domain[1] {
                    break;
                }
                values.push(value);
                let Some(next) = current_year.checked_add(step) else {
                    break;
                };
                current_year = next;
            }
        }
    }
    values
}

fn calendar_month_milliseconds(total_month: i64) -> Option<i64> {
    let year = total_month.div_euclid(12);
    let month = u32::try_from(total_month.rem_euclid(12) + 1).ok()?;
    days_from_civil(year, month, 1)?.checked_mul(86_400_000)
}

fn format_temporal_tick(
    epoch_ms: i64,
    interval: TemporalTickInterval,
    month_names: &MonthNames,
) -> String {
    let seconds = epoch_ms.div_euclid(1_000);
    let (year, month, day) = civil_from_timestamp(seconds);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day.rem_euclid(3_600) / 60;
    let second = seconds_of_day.rem_euclid(60);
    let month_name = &month_names.short[(month - 1) as usize];
    match interval {
        TemporalTickInterval::Fixed(step) if step < 1_000 => {
            let millisecond = epoch_ms.rem_euclid(1_000);
            format!("{hour:02}:{minute:02}:{second:02}.{millisecond:03}")
        }
        TemporalTickInterval::Fixed(step) if step < 60_000 => {
            format!("{hour:02}:{minute:02}:{second:02}")
        }
        TemporalTickInterval::Fixed(step) if step < 86_400_000 => {
            if seconds_of_day == 0 {
                format!("{day} {month_name}")
            } else {
                format!("{hour:02}:{minute:02}")
            }
        }
        TemporalTickInterval::Fixed(_) => format!("{day} {month_name}"),
        TemporalTickInterval::Months(_) => format!("{month_name} {year}"),
        TemporalTickInterval::Years(_) => year.to_string(),
    }
}

fn extend_numeric_bounds(bounds: &mut Option<(f64, f64)>, value: f64) {
    *bounds = Some(match *bounds {
        Some((low, high)) => (low.min(value), high.max(value)),
        None => (value, value),
    });
}

fn expanded_numeric_domain(low: f64, high: f64) -> [f64; 2] {
    if low < high {
        return [low, high];
    }
    let delta = low.abs().max(1.0) * 0.01;
    [low - delta, high + delta]
}

fn expanded_numeric_domain_for_scale(
    scale: GeneralScaleType,
    low: f64,
    high: f64,
) -> Option<[f64; 2]> {
    if low < high {
        return Some([low, high]);
    }
    match scale {
        GeneralScaleType::Logarithmic => {
            if low <= 0.0 {
                return None;
            }
            let factor = 1.01;
            Some([low / factor, high * factor])
        }
        GeneralScaleType::Linear | GeneralScaleType::SymmetricLog => {
            Some(expanded_numeric_domain(low, high))
        }
        _ => None,
    }
}

fn expanded_temporal_domain(low: i64, high: i64) -> Option<GeneralAxisDomain> {
    if low < high {
        return Some(GeneralAxisDomain::Temporal([low, high]));
    }
    if low < MAX_GENERAL_TEMPORAL_MILLISECONDS {
        Some(GeneralAxisDomain::Temporal([low, low + 1]))
    } else if low > -MAX_GENERAL_TEMPORAL_MILLISECONDS {
        Some(GeneralAxisDomain::Temporal([low - 1, low]))
    } else {
        None
    }
}

fn sampled_category_ticks<F>(
    axis: &GeneralAxis,
    values: &[String],
    coordinate: F,
) -> Vec<GeneralAxisTick>
where
    F: Fn(usize) -> Option<f64>,
{
    let limit = axis
        .tick_count
        .map(usize::from)
        .unwrap_or(MAX_GENERAL_AXIS_TICKS as usize)
        .min(MAX_GENERAL_AXIS_TICKS as usize);
    if values.is_empty() || limit == 0 {
        return Vec::new();
    }
    let stride = values.len().div_ceil(limit).max(1);
    values
        .iter()
        .enumerate()
        .step_by(stride)
        .filter_map(|(index, label)| {
            coordinate(index).map(|coordinate| GeneralAxisTick {
                coordinate,
                label: label.clone(),
                align: AxisTextAlign::Center,
            })
        })
        .collect()
}

fn collision_filtered_ticks<F>(
    axis: &GeneralAxis,
    mut ticks: Vec<GeneralAxisTick>,
    measure: &F,
    metrics: AxisMetrics,
    range_from: f64,
    range_to: f64,
) -> Vec<GeneralAxisTick>
where
    F: Fn(&str, bool) -> f64,
{
    ticks.sort_by(|left, right| left.coordinate.total_cmp(&right.coordinate));
    let mut previous_end = f64::NEG_INFINITY;
    let range_start = range_from.min(range_to);
    let range_end = range_from.max(range_to);
    ticks.retain_mut(|tick| {
        let (start, end) = if axis.dimension == AxisDimension::X {
            let width = measure(&tick.label, false);
            if !width.is_finite() || width <= 0.0 || width > range_end - range_start {
                return false;
            }
            let half = width / 2.0;
            if tick.coordinate - half < range_start {
                tick.align = AxisTextAlign::Left;
                (tick.coordinate, tick.coordinate + width)
            } else if tick.coordinate + half > range_end {
                tick.align = AxisTextAlign::Right;
                (tick.coordinate - width, tick.coordinate)
            } else {
                tick.align = AxisTextAlign::Center;
                (tick.coordinate - half, tick.coordinate + half)
            }
        } else {
            let extent = metrics.axis / 2.0;
            (tick.coordinate - extent, tick.coordinate + extent)
        };
        let keep = start >= previous_end + axis.min_tick_gap;
        if keep {
            previous_end = end;
        }
        keep
    });
    ticks
}

fn format_numeric_ticks(values: &[f64]) -> Vec<String> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut positive_steps = values
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .filter(|step| step.is_finite() && *step > 0.0);
    let step = positive_steps
        .next()
        .map(|first| positive_steps.fold(first, f64::min));
    let max_abs = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    let scientific = max_abs >= 1.0e9
        || (max_abs > 0.0 && max_abs < 1.0e-4)
        || step.is_some_and(|value| value < 1.0e-6);
    if scientific {
        return values
            .iter()
            .map(|value| {
                let normalized = if value.abs() < f64::EPSILON {
                    0.0
                } else {
                    *value
                };
                format!("{normalized:.3e}")
            })
            .collect();
    }

    let precision = step.map_or(0, numeric_tick_precision);
    values
        .iter()
        .map(|value| {
            let zero_epsilon = step.unwrap_or(1.0).abs() * 1.0e-9;
            if value.abs() <= zero_epsilon {
                "0".to_owned()
            } else {
                format!("{value:.precision$}")
            }
        })
        .collect()
}

fn numeric_tick_precision(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 {
        return 0;
    }
    for precision in 0..=6 {
        let scaled = step * 10_f64.powi(precision as i32);
        if (scaled - scaled.round()).abs() <= scaled.abs().max(1.0) * 1.0e-9 {
            return precision;
        }
    }
    6
}

fn plain_axis_label(text: String, x: f64, y: f64, color: Color, align: AxisTextAlign) -> AxisLabel {
    AxisLabel {
        text,
        x,
        y,
        color,
        align,
        midpoint: AxisTextMidpoint::Label,
        font_scale: AXIS_FONT_SCALE,
        bold: false,
        background: None,
        background_corners: AxisLabelCorners::NONE,
        measure_extra: 0.0,
        attach_group: None,
        border: None,
    }
}

impl Default for GeneralAxisRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartEngine {
    pub fn add_general_axis(&mut self, options: GeneralAxisOptions) -> Result<(), ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane)
            .ok_or_else(|| invalid("general axis references a stale pane"))?;
        let horizontal_domain = self
            .pane_horizontal_domain(options.pane)
            .ok_or_else(|| invalid("general axis pane has no horizontal domain"))?;
        validate_compatibility(horizontal_domain, &options)?;
        self.general_axes.insert(pane_id, options)?;
        self.invalidate_frame_all();
        Ok(())
    }

    pub fn general_axis(&self, id: &str) -> Option<&GeneralAxis> {
        self.general_axes.get(id)
    }

    pub fn general_axis_pane_index(&self, id: &str) -> Option<usize> {
        self.pane_index_for_id(self.general_axes.get(id)?.pane_id)
    }

    /// General axes in insertion order, optionally filtered to the pane currently at `pane`.
    pub fn general_axes(&self, pane: Option<usize>) -> Vec<&GeneralAxis> {
        let pane_id = match pane {
            Some(index) => match self.pane_stable_id(index) {
                Some(id) => Some(id),
                None => return Vec::new(),
            },
            None => None,
        };
        self.general_axes
            .iter()
            .filter(|axis| pane_id.is_none_or(|id| axis.pane_id == id))
            .collect()
    }

    pub fn set_general_axis_visible(&mut self, id: &str, visible: bool) -> bool {
        let Some(axis) = self.general_axes.get_mut(id) else {
            return false;
        };
        if axis.visible == visible {
            return true;
        }
        axis.visible = visible;
        self.invalidate_frame_all();
        true
    }

    pub fn update_general_axis_options(
        &mut self,
        options: GeneralAxisOptions,
    ) -> Result<(), ChartError> {
        let current = self.general_axis(&options.id).cloned().ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidHandle, "general axis handle is stale")
        })?;
        let pane = self
            .pane_index_for_id(current.pane_id)
            .ok_or_else(|| invalid("general axis references a stale pane"))?;
        if options.pane != pane
            || options.dimension != current.dimension
            || options.scale != current.scale
        {
            return Err(invalid(
                "general axis pane, dimension, and scale are structural",
            ));
        }
        let horizontal_domain = self
            .pane_horizontal_domain(pane)
            .ok_or_else(|| invalid("general axis pane has no horizontal domain"))?;
        validate_options(&options)?;
        validate_compatibility(horizontal_domain, &options)?;

        let axis = self
            .general_axes
            .get_mut(&options.id)
            .expect("a resolved general axis remains live during one mutation");
        if axis.domain != options.domain {
            axis.view_domain = None;
        }
        axis.position = resolved_position(options.dimension, options.position);
        axis.domain = options.domain;
        axis.reverse = options.reverse;
        axis.visible = options.visible;
        axis.title = options.title;
        axis.tick_count = options.tick_count;
        axis.min_tick_gap = options.min_tick_gap;
        axis.band_padding_inner = options.band_padding_inner;
        axis.band_padding_outer = options.band_padding_outer;
        axis.zero_line = options.zero_line;
        axis.grid_visible = options.grid_visible;
        self.invalidate_frame_all();
        Ok(())
    }

    /// Remove an unpopulated general axis. General-series ownership will add the populated-axis
    /// guard at the same registry boundary when those series are introduced.
    pub fn remove_general_axis(&mut self, id: &str) -> bool {
        if self.general_series_uses_axis(id) {
            return false;
        }
        let removed = self.general_axes.remove(id);
        if removed {
            self.invalidate_frame_all();
        }
        removed
    }
}

fn validate_options(options: &GeneralAxisOptions) -> Result<(), ChartError> {
    if options.id.is_empty() || options.id.len() > MAX_GENERAL_AXIS_ID_BYTES {
        return Err(invalid(format!(
            "general axis id must contain 1..={MAX_GENERAL_AXIS_ID_BYTES} UTF-8 bytes"
        )));
    }
    if options
        .title
        .as_ref()
        .is_some_and(|title| title.len() > MAX_GENERAL_AXIS_TITLE_BYTES)
    {
        return Err(resource(format!(
            "general axis title exceeds {MAX_GENERAL_AXIS_TITLE_BYTES} UTF-8 bytes"
        )));
    }
    if options
        .tick_count
        .is_some_and(|count| count == 0 || count > MAX_GENERAL_AXIS_TICKS)
    {
        return Err(invalid(format!(
            "general axis tick_count must be in 1..={MAX_GENERAL_AXIS_TICKS}"
        )));
    }
    if !options.min_tick_gap.is_finite() || !(0.0..=10_000.0).contains(&options.min_tick_gap) {
        return Err(invalid(
            "general axis min_tick_gap must be finite and in 0..=10000",
        ));
    }
    for (name, value) in [
        ("band_padding_inner", options.band_padding_inner),
        ("band_padding_outer", options.band_padding_outer),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(invalid(format!(
                "general axis {name} must be finite and in 0..=1"
            )));
        }
    }
    validate_position(options.dimension, options.position)?;
    validate_domain(options.scale, &options.domain)
}

fn validate_position(
    dimension: AxisDimension,
    position: Option<AxisPosition>,
) -> Result<(), ChartError> {
    let valid = match (dimension, position) {
        (_, None) => true,
        (AxisDimension::X, Some(AxisPosition::Top | AxisPosition::Bottom)) => true,
        (AxisDimension::Y, Some(AxisPosition::Left | AxisPosition::Right)) => true,
        (AxisDimension::Angle | AxisDimension::Radius, Some(_)) => false,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "general axis position is incompatible with its dimension",
        ))
    }
}

fn resolved_position(
    dimension: AxisDimension,
    position: Option<AxisPosition>,
) -> Option<AxisPosition> {
    position.or(match dimension {
        AxisDimension::X => Some(AxisPosition::Bottom),
        AxisDimension::Y => Some(AxisPosition::Left),
        AxisDimension::Angle | AxisDimension::Radius => None,
    })
}

fn validate_domain(scale: GeneralScaleType, domain: &GeneralAxisDomain) -> Result<(), ChartError> {
    match domain {
        GeneralAxisDomain::Auto => Ok(()),
        GeneralAxisDomain::Numeric([from, to]) => {
            if !matches!(
                scale,
                GeneralScaleType::Linear
                    | GeneralScaleType::Logarithmic
                    | GeneralScaleType::SymmetricLog
                    | GeneralScaleType::RadialLinear
            ) {
                return Err(invalid("numeric domain requires a numeric scale"));
            }
            if !from.is_finite() || !to.is_finite() || from >= to || !(*to - *from).is_finite() {
                return Err(invalid(
                    "numeric domain bounds must be finite and strictly ascending",
                ));
            }
            if scale == GeneralScaleType::Logarithmic && *from <= 0.0 {
                return Err(invalid("logarithmic domain bounds must be positive"));
            }
            Ok(())
        }
        GeneralAxisDomain::Temporal([from, to]) => {
            if scale != GeneralScaleType::Temporal {
                return Err(invalid("temporal domain requires a temporal scale"));
            }
            if from >= to
                || from.unsigned_abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as u64
                || to.unsigned_abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as u64
            {
                return Err(invalid(
                    "temporal domain bounds must be strictly ascending safe epoch milliseconds",
                ));
            }
            Ok(())
        }
        GeneralAxisDomain::Category(values) => {
            if !matches!(
                scale,
                GeneralScaleType::Band
                    | GeneralScaleType::Point
                    | GeneralScaleType::AngularCategory
            ) {
                return Err(invalid("category domain requires a category scale"));
            }
            if values.len() > MAX_GENERAL_AXIS_CATEGORIES {
                return Err(resource(format!(
                    "general axis category domain exceeds {MAX_GENERAL_AXIS_CATEGORIES} labels"
                )));
            }
            let bytes = values.iter().try_fold(0usize, |total, value| {
                total
                    .checked_add(value.len())
                    .ok_or_else(|| resource("general axis category domain byte count overflow"))
            })?;
            if bytes > MAX_GENERAL_AXIS_CATEGORY_BYTES {
                return Err(resource(format!(
                    "general axis category domain exceeds {MAX_GENERAL_AXIS_CATEGORY_BYTES} UTF-8 bytes"
                )));
            }
            let mut unique = HashSet::with_capacity(values.len());
            if values.iter().any(|value| !unique.insert(value.as_str())) {
                return Err(invalid("general axis category labels must be unique"));
            }
            Ok(())
        }
    }
}

fn validate_compatibility(
    horizontal_domain: HorizontalDomain,
    options: &GeneralAxisOptions,
) -> Result<(), ChartError> {
    let compatible = match (horizontal_domain, options.dimension, options.scale) {
        (HorizontalDomain::FinancialTime, _, _) => false,
        (HorizontalDomain::Continuous { scale: domain }, AxisDimension::X, axis_scale) => {
            numeric_scale_for_domain(domain) == axis_scale
        }
        (HorizontalDomain::Temporal, AxisDimension::X, GeneralScaleType::Temporal) => true,
        (HorizontalDomain::Category { scale: domain }, AxisDimension::X, axis_scale) => {
            category_scale_for_domain(domain) == axis_scale
        }
        (
            HorizontalDomain::Continuous { .. }
            | HorizontalDomain::Temporal
            | HorizontalDomain::Category { .. },
            AxisDimension::Y,
            GeneralScaleType::Linear
            | GeneralScaleType::Logarithmic
            | GeneralScaleType::SymmetricLog
            | GeneralScaleType::Band
            | GeneralScaleType::Point,
        ) => true,
        (HorizontalDomain::Polar, AxisDimension::Angle, GeneralScaleType::AngularCategory) => true,
        (HorizontalDomain::Polar, AxisDimension::Radius, GeneralScaleType::RadialLinear) => true,
        _ => false,
    };
    if compatible {
        Ok(())
    } else {
        Err(invalid(
            "general axis scale or dimension is incompatible with the pane domain",
        ))
    }
}

fn numeric_scale_for_domain(scale: ContinuousScaleType) -> GeneralScaleType {
    match scale {
        ContinuousScaleType::Linear => GeneralScaleType::Linear,
        ContinuousScaleType::Logarithmic => GeneralScaleType::Logarithmic,
        ContinuousScaleType::SymmetricLog => GeneralScaleType::SymmetricLog,
    }
}

fn category_scale_for_domain(scale: CategoryScaleType) -> GeneralScaleType {
    match scale {
        CategoryScaleType::Band => GeneralScaleType::Band,
        CategoryScaleType::Point => GeneralScaleType::Point,
    }
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidOptions, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category_axis(pane: usize, id: &str) -> GeneralAxisOptions {
        GeneralAxisOptions::new(id, pane, AxisDimension::X, GeneralScaleType::Band)
    }

    #[test]
    fn compatibility_matrix_accepts_only_matching_axes() {
        let cases = [
            (
                HorizontalDomain::Continuous {
                    scale: ContinuousScaleType::Linear,
                },
                AxisDimension::X,
                GeneralScaleType::Linear,
            ),
            (
                HorizontalDomain::Temporal,
                AxisDimension::X,
                GeneralScaleType::Temporal,
            ),
            (
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Point,
                },
                AxisDimension::X,
                GeneralScaleType::Point,
            ),
            (
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Band,
                },
                AxisDimension::Y,
                GeneralScaleType::Logarithmic,
            ),
            (
                HorizontalDomain::Polar,
                AxisDimension::Angle,
                GeneralScaleType::AngularCategory,
            ),
            (
                HorizontalDomain::Polar,
                AxisDimension::Radius,
                GeneralScaleType::RadialLinear,
            ),
        ];
        for (domain, dimension, scale) in cases {
            let options = GeneralAxisOptions::new("axis", 0, dimension, scale);
            assert!(validate_compatibility(domain, &options).is_ok());
        }

        for (domain, dimension, scale) in [
            (
                HorizontalDomain::FinancialTime,
                AxisDimension::Y,
                GeneralScaleType::Linear,
            ),
            (
                HorizontalDomain::Temporal,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ),
            (
                HorizontalDomain::Category {
                    scale: CategoryScaleType::Band,
                },
                AxisDimension::X,
                GeneralScaleType::Point,
            ),
            (
                HorizontalDomain::Polar,
                AxisDimension::X,
                GeneralScaleType::Linear,
            ),
        ] {
            let options = GeneralAxisOptions::new("axis", 0, dimension, scale);
            assert_eq!(
                validate_compatibility(domain, &options).unwrap_err().code(),
                ErrorCode::InvalidOptions
            );
        }
    }

    #[test]
    fn explicit_domains_validate_scale_bounds_and_category_identity() {
        let mut numeric =
            GeneralAxisOptions::new("value", 0, AxisDimension::Y, GeneralScaleType::Logarithmic);
        numeric.domain = GeneralAxisDomain::Numeric([0.0, 10.0]);
        assert!(validate_options(&numeric).is_err());
        numeric.domain = GeneralAxisDomain::Numeric([0.1, 10.0]);
        assert!(validate_options(&numeric).is_ok());

        let mut temporal =
            GeneralAxisOptions::new("time", 0, AxisDimension::X, GeneralScaleType::Temporal);
        temporal.domain = GeneralAxisDomain::Temporal([
            -MAX_GENERAL_TEMPORAL_MILLISECONDS,
            MAX_GENERAL_TEMPORAL_MILLISECONDS,
        ]);
        assert!(validate_options(&temporal).is_ok());
        temporal.domain = GeneralAxisDomain::Temporal([0, i64::MAX]);
        assert!(validate_options(&temporal).is_err());

        let mut category = category_axis(0, "category");
        category.domain = GeneralAxisDomain::Category(vec!["".into(), "A".into(), "A".into()]);
        assert!(validate_options(&category).is_err());
        category.domain = GeneralAxisDomain::Category(vec!["".into(), "A".into()]);
        assert!(validate_options(&category).is_ok());
    }

    #[test]
    fn position_defaults_and_validation_follow_dimension() {
        assert_eq!(
            resolved_position(AxisDimension::X, None),
            Some(AxisPosition::Bottom)
        );
        assert_eq!(
            resolved_position(AxisDimension::Y, None),
            Some(AxisPosition::Left)
        );
        assert_eq!(resolved_position(AxisDimension::Angle, None), None);
        assert!(validate_position(AxisDimension::X, Some(AxisPosition::Top)).is_ok());
        assert!(validate_position(AxisDimension::X, Some(AxisPosition::Left)).is_err());
        assert!(validate_position(AxisDimension::Radius, Some(AxisPosition::Right)).is_err());
    }

    #[test]
    fn numeric_tick_labels_use_one_stable_precision_per_axis() {
        assert_eq!(
            format_numeric_ticks(&[0.0, 0.25, 0.5, 0.75, 1.0]),
            ["0", "0.25", "0.50", "0.75", "1.00"]
        );
        assert_eq!(
            format_numeric_ticks(&[40.0, 50.0, 60.0, 70.0]),
            ["40", "50", "60", "70"]
        );
        assert_eq!(
            format_numeric_ticks(&[-0.5, 0.0, 0.5]),
            ["-0.5", "0", "0.5"]
        );
        assert!(format_numeric_ticks(&[1.0e9, 2.0e9])
            .iter()
            .all(|label| label.contains('e')));
    }

    #[test]
    fn temporal_ticks_are_calendar_aligned_bounded_and_locale_aware() {
        let months = MonthNames::english();
        let quarter = TemporalTickInterval::Months(3);
        let values = temporal_tick_values([1_767_225_600_000, 1_783_036_800_000], quarter);
        assert_eq!(
            values,
            [1_767_225_600_000, 1_775_001_600_000, 1_782_864_000_000]
        );
        assert_eq!(
            values
                .iter()
                .map(|value| format_temporal_tick(*value, quarter, &months))
                .collect::<Vec<_>>(),
            ["Jan 2026", "Apr 2026", "Jul 2026"]
        );

        let full_safe_range = temporal_tick_values(
            [
                -MAX_GENERAL_TEMPORAL_MILLISECONDS,
                MAX_GENERAL_TEMPORAL_MILLISECONDS,
            ],
            temporal_tick_interval(
                [
                    -MAX_GENERAL_TEMPORAL_MILLISECONDS,
                    MAX_GENERAL_TEMPORAL_MILLISECONDS,
                ],
                usize::from(MAX_GENERAL_AXIS_TICKS),
            ),
        );
        assert!(!full_safe_range.is_empty());
        assert!(full_safe_range.len() <= usize::from(MAX_GENERAL_AXIS_TICKS));
        assert!(full_safe_range.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
