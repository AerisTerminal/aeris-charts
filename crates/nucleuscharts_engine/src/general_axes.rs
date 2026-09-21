use std::collections::HashSet;
use std::num::NonZeroU32;

use nucleuscharts_core::scale::general_scale::{BandScale, LinearScale, PointScale};
use nucleuscharts_render::color::Color;

use crate::{
    axis_metrics::{AxisMetrics, AXIS_FONT_SCALE},
    AxisBand, AxisFrame, AxisLabel, AxisLabelCorners, AxisTextAlign, AxisTextMidpoint,
    CategoryScaleType, ChartEngine, ChartError, ContinuousScaleType, ErrorCode, HorizontalDomain,
    PaneId, PriceScaleSide,
};

pub const MAX_GENERAL_AXES: usize = 128;
pub const MAX_GENERAL_AXIS_ID_BYTES: usize = 128;
pub const MAX_GENERAL_AXIS_TITLE_BYTES: usize = 4_096;
pub const MAX_GENERAL_AXIS_TICKS: u16 = 512;
pub const MAX_GENERAL_AXIS_CATEGORIES: usize = 65_536;
pub const MAX_GENERAL_AXIS_CATEGORY_BYTES: usize = 1_048_576;
pub const MAX_GENERAL_TEMPORAL_MILLISECONDS: i64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisDimension {
    X,
    Y,
    Angle,
    Radius,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisPosition {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default, PartialEq)]
pub enum GeneralAxisDomain {
    #[default]
    Auto,
    Numeric([f64; 2]),
    Temporal([i64; 2]),
    Category(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Negotiated strip width for vertical axes. Horizontal strip heights are derived from the
    /// shared font metrics because they do not depend on glyph advance.
    layout_thickness: f64,
}

impl GeneralAxis {
    #[cfg(test)]
    pub(crate) fn handle(&self) -> GeneralAxisHandle {
        self.handle
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
            layout_thickness: 0.0,
        });
        Ok(handle)
    }

    fn get(&self, id: &str) -> Option<&GeneralAxis> {
        self.axes.iter().find(|axis| axis.id == id)
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
}

impl ChartEngine {
    pub(crate) fn measure_general_axis_widths<F>(&mut self, measure: &F, allow_shrink: bool)
    where
        F: Fn(&str, bool) -> f64,
    {
        let fallback = AxisMetrics::price_strip_width(AxisMetrics::DEFAULT_TEXT_WIDTH, 0.0);
        for axis in self.general_axes.iter_mut() {
            if !axis.visible || !matches!(axis.dimension, AxisDimension::Y) {
                axis.layout_thickness = 0.0;
                continue;
            }
            let widest_tick = explicit_tick_labels(axis)
                .into_iter()
                .map(|label| measure(&label, false))
                .fold(0.0_f64, f64::max);
            let title_width = axis
                .title
                .as_deref()
                .map_or(0.0, |title| measure(title, false));
            let measured = AxisMetrics::price_strip_width(
                widest_tick
                    .max(title_width)
                    .max(AxisMetrics::DEFAULT_TEXT_WIDTH),
                0.0,
            )
            .max(fallback);
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
                let ticks = axis_ticks(axis, range.0, range.1, metrics);
                let ticks = collision_filtered_ticks(axis, ticks, measure, metrics);
                for tick in ticks {
                    if axis.dimension == AxisDimension::Y
                        && axis.title.is_some()
                        && (tick.coordinate - label_y).abs() < metrics.axis + axis.min_tick_gap
                    {
                        continue;
                    }
                    let (x, y) = match axis.dimension {
                        AxisDimension::X => (tick.coordinate, label_y),
                        AxisDimension::Y => (label_x, tick.coordinate),
                        AxisDimension::Angle | AxisDimension::Radius => unreachable!(),
                    };
                    out.labels
                        .push(plain_axis_label(tick.label, x, y, text_color, align));
                }
                if let Some(title) = axis.title.as_ref() {
                    let (x, y) = match position {
                        AxisPosition::Top => (label_x, strip_y + metrics.axis / 2.0 + 2.0),
                        AxisPosition::Bottom => {
                            (label_x, strip_y + strip_h - metrics.axis / 2.0 - 2.0)
                        }
                        AxisPosition::Left | AxisPosition::Right => (label_x, label_y),
                    };
                    out.labels
                        .push(plain_axis_label(title.clone(), x, y, text_color, align));
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

fn horizontal_axis_thickness(axis: &GeneralAxis, metrics: AxisMetrics) -> f64 {
    let rows = if axis.title.is_some() { 2.0 } else { 1.0 };
    (1.0 + AxisMetrics::TICK_LENGTH + 4.0 + rows * (metrics.axis + 4.0)).ceil()
}

fn explicit_tick_labels(axis: &GeneralAxis) -> Vec<String> {
    match (&axis.scale, &axis.domain) {
        (GeneralScaleType::Linear, GeneralAxisDomain::Numeric([from, to])) => {
            let Ok(scale) = LinearScale::new(*from, *to, 0.0, 1.0) else {
                return Vec::new();
            };
            scale
                .ticks(axis.tick_count.unwrap_or(6) as usize)
                .into_iter()
                .map(format_numeric_tick)
                .collect()
        }
        (GeneralScaleType::Band | GeneralScaleType::Point, GeneralAxisDomain::Category(values)) => {
            values
                .iter()
                .take(MAX_GENERAL_AXIS_TICKS as usize)
                .cloned()
                .collect()
        }
        _ => Vec::new(),
    }
}

fn axis_ticks(
    axis: &GeneralAxis,
    range_from: f64,
    range_to: f64,
    metrics: AxisMetrics,
) -> Vec<GeneralAxisTick> {
    match (&axis.scale, &axis.domain) {
        (GeneralScaleType::Linear, GeneralAxisDomain::Numeric([from, to])) => {
            let Ok(scale) = LinearScale::new(*from, *to, range_from, range_to) else {
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
            scale
                .ticks(target)
                .into_iter()
                .filter_map(|value| {
                    scale.coordinate(value).map(|coordinate| GeneralAxisTick {
                        coordinate,
                        label: format_numeric_tick(value),
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
            })
        })
        .collect()
}

fn collision_filtered_ticks<F>(
    axis: &GeneralAxis,
    mut ticks: Vec<GeneralAxisTick>,
    measure: &F,
    metrics: AxisMetrics,
) -> Vec<GeneralAxisTick>
where
    F: Fn(&str, bool) -> f64,
{
    ticks.sort_by(|left, right| left.coordinate.total_cmp(&right.coordinate));
    let mut previous_end = f64::NEG_INFINITY;
    ticks.retain(|tick| {
        let extent = if axis.dimension == AxisDimension::X {
            measure(&tick.label, false) / 2.0
        } else {
            metrics.axis / 2.0
        };
        let start = tick.coordinate - extent;
        let keep = start >= previous_end + axis.min_tick_gap;
        if keep {
            previous_end = tick.coordinate + extent;
        }
        keep
    });
    ticks
}

fn format_numeric_tick(value: f64) -> String {
    value.to_string()
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

    /// Remove an unpopulated general axis. General-series ownership will add the populated-axis
    /// guard at the same registry boundary when those series are introduced.
    pub fn remove_general_axis(&mut self, id: &str) -> bool {
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
            | GeneralScaleType::SymmetricLog,
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
}
