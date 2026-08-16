//! Backend-neutral frame production for the headless chart model.
//!
//! This is intentionally independent of WebGPU, Canvas2D, and DOM types. Hosts may convert the
//! returned primitives into any raster backend, or inspect them in tests.

use crate::drawings::DrawingKind;
use crate::{
    ChartEngine, PriceFormatKind, PriceScaleTarget, SeriesKind, SeriesPriceFormat, PANE_SEPARATOR,
};
use nucleuscharts_core::format::percentage_formatter::PercentageFormatter;
use nucleuscharts_core::format::price_formatter::PriceFormatter;
use nucleuscharts_core::format::time_formatter::{
    format_crosshair_time_with, format_date_pattern, format_tick_label_with,
    weight_to_tick_mark_type, TickMarkType,
};
use nucleuscharts_core::format::volume_formatter::VolumeFormatter;
use nucleuscharts_core::model::data_layer::{PointColorChannel, SeriesId};
use nucleuscharts_core::model::magnet::{magnet_snap_coordinate, CrosshairMode};
use nucleuscharts_core::model::plot_list::{MismatchDirection, PlotListView, PlotValueIndex};
use nucleuscharts_core::model::price_range::PriceRange;
use nucleuscharts_core::scale::price_scale_core::{PriceScaleCore, PriceScaleMode};
use nucleuscharts_core::style::{
    DEFAULT_BORDER_RGB, DEFAULT_CROSSHAIR_LABEL_RGB, DEFAULT_CROSSHAIR_LINE_RGB,
    DEFAULT_PRIMARY_RGB, MARKET_DOWN_RGB, MARKET_UP_RGB, MARKET_VOLUME_ALPHA,
};
use nucleuscharts_render::bars::{build_bars, BarItem, BarsParams};
use nucleuscharts_render::candles::{build_candles, CandleItem, CandlesParams};
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{Gradient, IRect, LineStyle, LineType, Prim, TextAlign};
use nucleuscharts_render::histogram::{build_histogram, HistogramItem, HistogramParams};
use nucleuscharts_render::line::{dash_split, expand_line, LinePoint};

mod axis;
pub(crate) mod conflation;
mod crosshair;
mod drawings;
mod feature_geometry;
mod native_primitive_geometry;
mod series_geometry;
#[cfg(test)]
mod tests;
mod trading_geometry;

#[cfg(test)]
use conflation::{visible_histogram_rows, visible_ohlc};
use conflation::{
    visible_histogram_rows_with_work, visible_line_rows, visible_line_rows_with_work,
    visible_ohlc_with_work,
};

const UP: Color = Color::rgb(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2);
const DOWN: Color = Color::rgb(MARKET_DOWN_RGB.0, MARKET_DOWN_RGB.1, MARKET_DOWN_RGB.2);
const PRIMARY: Color = Color::rgb(
    DEFAULT_PRIMARY_RGB.0,
    DEFAULT_PRIMARY_RGB.1,
    DEFAULT_PRIMARY_RGB.2,
);
const GRID: Color = Color::rgb(
    DEFAULT_BORDER_RGB.0,
    DEFAULT_BORDER_RGB.1,
    DEFAULT_BORDER_RGB.2,
);
const LINE: Color = Color::rgb(0x21, 0x96, 0xf3);
const AREA_LINE: Color = UP;
const AREA_TOP: Color = Color::rgba(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2, 102);
const AREA_BOTTOM: Color = Color::rgba(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2, 0);
const HISTOGRAM: Color = Color::rgba(
    MARKET_UP_RGB.0,
    MARKET_UP_RGB.1,
    MARKET_UP_RGB.2,
    MARKET_VOLUME_ALPHA,
);
const VOLUME_UP: Color = Color::rgba(
    MARKET_UP_RGB.0,
    MARKET_UP_RGB.1,
    MARKET_UP_RGB.2,
    MARKET_VOLUME_ALPHA,
);
const VOLUME_DOWN: Color = Color::rgba(
    MARKET_DOWN_RGB.0,
    MARKET_DOWN_RGB.1,
    MARKET_DOWN_RGB.2,
    MARKET_VOLUME_ALPHA,
);
const BASELINE_TOP_LINE: Color = UP;
const BASELINE_BOTTOM_LINE: Color = DOWN;
/// reference baseline quadrant fill defaults (model/series/baseline-series.ts): two-stop gradients
/// from the line to the baseline. Alphas are the CSS 0..1 values quantized to bytes
/// (0.28 -> 71, 0.05 -> 13, matching `Color::parse_css`).
const BASELINE_TOP_FILL1: Color =
    Color::rgba(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2, 71);
const BASELINE_TOP_FILL2: Color =
    Color::rgba(MARKET_UP_RGB.0, MARKET_UP_RGB.1, MARKET_UP_RGB.2, 13);
const BASELINE_BOTTOM_FILL1: Color =
    Color::rgba(MARKET_DOWN_RGB.0, MARKET_DOWN_RGB.1, MARKET_DOWN_RGB.2, 13);
const BASELINE_BOTTOM_FILL2: Color =
    Color::rgba(MARKET_DOWN_RGB.0, MARKET_DOWN_RGB.1, MARKET_DOWN_RGB.2, 71);
pub(crate) const LINE_WIDTH: f64 = 3.0;
const CROSSHAIR_COLOR: Color = Color::rgb(
    DEFAULT_CROSSHAIR_LINE_RGB.0,
    DEFAULT_CROSSHAIR_LINE_RGB.1,
    DEFAULT_CROSSHAIR_LINE_RGB.2,
);
const CROSSHAIR_LABEL_BG: Color = Color::rgb(
    DEFAULT_CROSSHAIR_LABEL_RGB.0,
    DEFAULT_CROSSHAIR_LABEL_RGB.1,
    DEFAULT_CROSSHAIR_LABEL_RGB.2,
);

fn ceiled_odd(value: f64) -> f64 {
    let ceiled = value.ceil() as i64;
    if ceiled % 2 == 0 {
        (ceiled - 1) as f64
    } else {
        ceiled as f64
    }
}

fn ceiled_even(value: f64) -> f64 {
    let ceiled = value.ceil() as i64;
    if ceiled % 2 != 0 {
        (ceiled - 1) as f64
    } else {
        ceiled as f64
    }
}

fn marker_envelope_size(bar_spacing: f64) -> f64 {
    ceiled_even(ceiled_odd(bar_spacing.clamp(12.0, 30.0)))
}

fn marker_shape_size(envelope: f64, coefficient: f64) -> f64 {
    ceiled_odd(envelope.max(12.0) * coefficient)
}

fn marker_margin(bar_spacing: f64) -> f64 {
    ceiled_odd(bar_spacing.clamp(12.0, 30.0) * 0.1).max(3.0)
}

fn marker_auto_scale_margins(markers: &[crate::Marker], bar_spacing: f64) -> (f64, f64) {
    if markers.is_empty() {
        return (0.0, 0.0);
    }
    let margin_value = marker_envelope_size(bar_spacing) * 1.5 + marker_margin(bar_spacing) * 2.0;
    let has_above = markers
        .iter()
        .any(|marker| marker.position == crate::marker_pos::ABOVE);
    let has_below = markers
        .iter()
        .any(|marker| marker.position == crate::marker_pos::BELOW);
    let has_in_bar = markers
        .iter()
        .any(|marker| marker.position == crate::marker_pos::IN_BAR);
    let adjusted = || (margin_value / 2.0).ceil();
    (
        if has_above {
            margin_value
        } else if has_in_bar {
            adjusted()
        } else {
            0.0
        },
        if has_below {
            margin_value
        } else if has_in_bar {
            adjusted()
        } else {
            0.0
        },
    )
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FramePane {
    pub top: f64,
    pub height: f64,
    pub scissor: [u32; 4],
    pub under: Vec<Prim>,
    pub main: Vec<Prim>,
    /// Primitive z-order `top` layer (reference `PrimitivePaneViewZOrder` "top" — above everything,
    /// crosshair included). The engine emits nothing here today; hosts append plugin prims
    /// after frame construction. Kept beside `under`/`main` so both backends execute it with
    /// the pane's scissor and point pool. (`FramePane.top` is already the pane's CSS y offset,
    /// hence the `_prims` suffix.)
    pub top_prims: Vec<Prim>,
    /// Series paint-order marks (Phase C-c): `(series_id, main.len())` recorded right after
    /// each visible series' slot in the pane's paint loop. A custom series paints nothing
    /// here, so its mark equals the previous series'; hosts splice custom-series prims into
    /// `main` at the mark, preserving the chart z-order instead of always painting on top.
    pub series_paint_marks: Vec<(SeriesId, usize)>,
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartFrame {
    pub width: f64,
    pub height: f64,
    pub pixel_ratio: f64,
    pub panes: Vec<FramePane>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameBuildStats {
    pub layout_rebuilds: u64,
    pub autoscale_runs: u64,
    pub grid_rebuilds: u64,
    pub series_rebuilds: u64,
    pub drawing_rebuilds: u64,
    pub overlay_rebuilds: u64,
    pub trading_rebuilds: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FramePaneSegments {
    pub under_end: usize,
    pub series_end: usize,
    pub trading_regions_end: usize,
    pub drawings_end: usize,
    pub trading_end: usize,
    pub overlay_end: usize,
    pub under_revision: u64,
    pub drawings_revision: u64,
    pub trading_revision: u64,
    pub overlay_revision: u64,
    pub top_revision: u64,
    /// Canonical coordinate revision shared by every coordinate-dependent segment in this pane.
    pub coordinate_revision: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSeriesSegment {
    pub series_id: Option<SeriesId>,
    pub start: usize,
    pub end: usize,
    pub revision: u64,
    pub coordinate_revision: u64,
}

#[derive(Default)]
pub(crate) struct FrameInvalidation {
    clock: u64,
    layout: u64,
    coordinate: u64,
    scene: u64,
    drawings: u64,
    trading: u64,
    overlay: u64,
    axis: u64,
    autoscale: u64,
    chrome: u64,
    series: Vec<(SeriesId, u64)>,
}

impl FrameInvalidation {
    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1).max(1);
        self.clock
    }

    fn all(&mut self) {
        let generation = self.tick();
        self.layout = generation;
        self.coordinate = generation;
        self.scene = generation;
        self.chrome = generation;
        self.drawings = generation;
        self.trading = generation;
        self.overlay = generation;
        self.axis = generation;
        self.autoscale = generation;
    }

    fn scene(&mut self) {
        let generation = self.tick();
        self.scene = generation;
        self.chrome = generation;
        self.drawings = generation;
        self.trading = generation;
        self.overlay = generation;
        self.axis = generation;
        self.autoscale = generation;
    }

    fn coordinates(&mut self) {
        let generation = self.tick();
        self.coordinate = generation;
        self.scene = generation;
        self.chrome = generation;
        self.drawings = generation;
        self.trading = generation;
        self.overlay = generation;
        self.axis = generation;
    }

    fn time_coordinates(&mut self) {
        self.coordinates();
        self.autoscale = self.clock;
    }

    fn series(&mut self, id: SeriesId) {
        let generation = self.tick();
        match self.series.iter_mut().find(|entry| entry.0 == id) {
            Some(entry) => entry.1 = generation,
            None => self.series.push((id, generation)),
        }
        self.chrome = generation;
        self.overlay = generation;
        self.autoscale = generation;
        self.axis = generation;
    }

    fn series_generation(&self, id: SeriesId) -> u64 {
        self.series
            .iter()
            .find_map(|entry| (entry.0 == id).then_some(entry.1))
            .unwrap_or(0)
    }

    fn drawings(&mut self) {
        let generation = self.tick();
        self.drawings = generation;
        self.overlay = generation;
        self.axis = generation;
    }

    fn trading(&mut self) {
        let generation = self.tick();
        self.trading = generation;
        self.axis = generation;
    }

    fn overlay(&mut self) {
        let generation = self.tick();
        self.overlay = generation;
        self.axis = generation;
    }

    fn axis(&mut self) {
        self.axis = self.tick();
    }
}

#[derive(Clone, Default)]
struct RetainedLayer {
    prims: Vec<Prim>,
    points: Vec<[f32; 2]>,
    revision: u64,
    coordinate_revision: u64,
}

#[derive(Clone, Default)]
struct RetainedPane {
    top: f64,
    height: f64,
    scissor: [u32; 4],
    under: RetainedLayer,
    /// Cursor-driven primitives with `bottom` z-order. Kept separate so pointer movement does not
    /// rebuild static grids or any series geometry.
    cursor_under: RetainedLayer,
    series_layers: Vec<RetainedSeriesLayer>,
    chrome: RetainedLayer,
    trading_regions: RetainedLayer,
    drawings: RetainedLayer,
    trading: RetainedLayer,
    overlay: RetainedLayer,
    top_layer: RetainedLayer,
}

#[derive(Clone, Default)]
struct RetainedSeriesLayer {
    id: SeriesId,
    scene_generation: u64,
    source_generation: u64,
    layer: RetainedLayer,
}

#[derive(Default)]
pub(crate) struct RetainedFrame {
    initialized: bool,
    panes: Vec<RetainedPane>,
    segments: Vec<FramePaneSegments>,
    series_segments: Vec<Vec<FrameSeriesSegment>>,
    layout_generation: u64,
    scene_generation: u64,
    chrome_generation: u64,
    drawings_generation: u64,
    trading_generation: u64,
    overlay_generation: u64,
    autoscale_generation: u64,
    axis_generation: u64,
    coordinate_generation: u64,
    last_layout_key: Option<[u64; 9]>,
    last_overlay_key: Option<[u64; 6]>,
    last_options_generation: u64,
    last_series_revision: u64,
    last_time_scale_revision: u64,
    last_price_scale_revisions: Vec<[u64; 3]>,
}

impl RetainedFrame {
    pub(crate) fn capacity_bytes(&self) -> usize {
        fn layer_bytes(layer: &RetainedLayer) -> usize {
            layer.prims.capacity() * std::mem::size_of::<Prim>()
                + layer.points.capacity() * std::mem::size_of::<[f32; 2]>()
        }

        self.panes
            .iter()
            .map(|pane| {
                layer_bytes(&pane.under)
                    + layer_bytes(&pane.cursor_under)
                    + layer_bytes(&pane.chrome)
                    + layer_bytes(&pane.trading_regions)
                    + layer_bytes(&pane.drawings)
                    + layer_bytes(&pane.trading)
                    + layer_bytes(&pane.overlay)
                    + layer_bytes(&pane.top_layer)
                    + pane.series_layers.capacity() * std::mem::size_of::<RetainedSeriesLayer>()
                    + pane
                        .series_layers
                        .iter()
                        .map(|series| layer_bytes(&series.layer))
                        .sum::<usize>()
            })
            .sum::<usize>()
            + self.panes.capacity() * std::mem::size_of::<RetainedPane>()
            + self.segments.capacity() * std::mem::size_of::<FramePaneSegments>()
            + self.series_segments.capacity() * std::mem::size_of::<Vec<FrameSeriesSegment>>()
            + self
                .series_segments
                .iter()
                .map(|segments| segments.capacity() * std::mem::size_of::<FrameSeriesSegment>())
                .sum::<usize>()
            + self.last_price_scale_revisions.capacity() * std::mem::size_of::<[u64; 3]>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisTextAlign {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisTextMidpoint {
    /// Canvas `middle` baseline without an actual-glyph correction (time ticks and markers).
    None,
    /// Correct using this label's glyph bounds (price-axis labels).
    Label,
    /// Correct using the reference's stable representative time-label sample (crosshair time label).
    StableTime,
}

/// Per-corner rounding selection for a boxed axis label's background (painted with a 2 CSS px
/// radius by the host). A boxed label rounds only its axis-facing side — the corners pointing
/// away from the pane — and keeps the chart-facing side sharp: right-strip labels round their
/// right corners, left-strip labels their left corners, time-strip labels their bottom corners.
/// The last-value cluster selects per row instead: the axis-facing corners of the cluster's
/// top and bottom edges only, with sharp internal boundaries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AxisLabelCorners {
    pub top_left: bool,
    pub top_right: bool,
    pub bottom_left: bool,
    pub bottom_right: bool,
}

impl AxisLabelCorners {
    pub const NONE: Self = Self {
        top_left: false,
        top_right: false,
        bottom_left: false,
        bottom_right: false,
    };
    /// Right side (right price strip: the axis-facing side is the label's right edge).
    pub const RIGHT: Self = Self {
        top_left: false,
        top_right: true,
        bottom_left: false,
        bottom_right: true,
    };
    /// Left side (left price strip).
    pub const LEFT: Self = Self {
        top_left: true,
        top_right: false,
        bottom_left: true,
        bottom_right: false,
    };
    /// Bottom side (time strip below the pane).
    pub const BOTTOM: Self = Self {
        top_left: false,
        top_right: false,
        bottom_left: true,
        bottom_right: true,
    };

    /// The whole-side selection for a single-row boxed label from its text alignment: `Left`
    /// (text starting at the right strip's left edge) rounds the right corners, `Right` the
    /// left corners, and `Center` (time-axis boxes below the pane) the bottom corners.
    pub fn for_align(align: AxisTextAlign) -> Self {
        match align {
            AxisTextAlign::Left => Self::RIGHT,
            AxisTextAlign::Right => Self::LEFT,
            AxisTextAlign::Center => Self::BOTTOM,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::NONE
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AxisLabel {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub color: Color,
    pub align: AxisTextAlign,
    pub midpoint: AxisTextMidpoint,
    /// Scale relative to the chart layout font size. Axis labels use `1.0`; secondary rows such
    /// as the candle countdown may be smaller while retaining the same family and metrics.
    pub font_scale: f64,
    pub bold: bool,
    pub background: Option<(f64, f64, f64, f64, Color)>,
    /// Rounded-corner selection for `background` (see [`AxisLabelCorners`]); `NONE` paints the
    /// plain sharp rectangle.
    pub background_corners: AxisLabelCorners,
    /// Extra width (media px) this label contributes to the axis-width negotiation beyond its
    /// own text. The last-value cluster puts it on the price-area label so the negotiated
    /// strip covers the title chip + price row (each label is otherwise measured alone).
    pub measure_extra: f64,
    /// Attachment group: boxed labels sharing a group id are painted with SHARED edges — each
    /// box's top edge is the previous box's exact bottom (no per-box rounding gaps between
    /// attached rows like the price chip and its countdown chip).
    pub attach_group: Option<u32>,
    /// Optional inside border for an axis-label background, in media px.
    pub border: Option<(f64, Color)>,
}

/// A backend-neutral rectangle painted beneath axis chrome and labels. Rectangle drawings use
/// this for the official plugin's 15 CSS px price/time-axis pane shading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisBand {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: Color,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisFrame {
    pub bands: Vec<AxisBand>,
    pub labels: Vec<AxisLabel>,
    pub separators: Vec<f64>,
    /// Price-axis tick stubs (reference `ticksVisible`): 5 css px horizontal marks painted from the
    /// pane edge into the axis strip at each tick coordinate, in the strip's border color.
    /// Emitted only for scales whose `ticksVisible` option is on.
    pub price_ticks: Vec<PriceAxisTick>,
    /// Time-axis tick x positions (media px, relative to the chart offset) for the
    /// `ticksVisible` stubs; empty while `timeScale.ticksVisible` is off.
    pub time_ticks: Vec<f64>,
    /// Index (into `separators`) of the hovered pane separator, if any — the host paints the
    /// `layout.panes.separatorHoverColor` band over it (reference pane-separator.ts).
    pub separator_hover: Option<usize>,
}

/// One price-axis tick stub (reference price-axis-widget.ts `_drawTickMarks`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceAxisTick {
    /// Media-y of the tick mark (same coordinate as its label).
    pub y: f64,
    /// Which strip the tick belongs to (`true` = left axis, `false` = right).
    pub left: bool,
}

#[derive(Clone, Copy)]
struct ResolvedSeries {
    id: SeriesId,
    kind: SeriesKind,
    color: Color,
    up: Color,
    down: Color,
    wick_up: Color,
    wick_down: Color,
    border_up: Color,
    border_down: Color,
    wick_visible: bool,
    border_visible: bool,
    line_width: f64,
    line_style: LineStyle,
    line_visible: bool,
    area_top: Color,
    area_bottom: Color,
    invert_filled_area: bool,
    point_markers: bool,
    point_markers_radius: Option<f64>,
    visible: bool,
    line_type: LineType,
    open_visible: bool,
    thin_bars: bool,
    base: f64,
    top_fill1: Color,
    top_fill2: Color,
    top_line: Color,
    top_line_width: f64,
    top_line_style: LineStyle,
    bottom_fill1: Color,
    bottom_fill2: Color,
    bottom_line: Color,
    bottom_line_width: f64,
    bottom_line_style: LineStyle,
    scale_target: PriceScaleTarget,
    /// The pane this series renders on; `None` when its pane was removed (reference `removePane`
    /// orphans the pane's series) — it draws and scales nowhere until re-assigned.
    pane: Option<usize>,
    base_value: f64,
}

pub(crate) fn series_scale_target(series: &crate::SeriesEntry) -> PriceScaleTarget {
    if series.overlay {
        PriceScaleTarget::Overlay
    } else if series.left_scale {
        PriceScaleTarget::Left
    } else {
        PriceScaleTarget::Right
    }
}

pub(crate) fn pane_scale(pane: &crate::Pane, target: PriceScaleTarget) -> &PriceScaleCore {
    match target {
        PriceScaleTarget::Right => &pane.price_scale,
        PriceScaleTarget::Left => &pane.left_scale,
        PriceScaleTarget::Overlay => &pane.overlay_scale,
    }
}

fn css_color(value: &str, fallback: Color) -> Color {
    Color::parse_css(value).unwrap_or(fallback)
}

/// Resolve a verbatim CSS color slot at render time (the wave-1 pattern): the stored string
/// parses, an unset slot or an unparseable string falls back to `fallback` (the reference's default —
/// a user string the renderer cannot parse degrades to the default rather than vanishing).
pub(crate) fn verbatim_color(value: &Option<String>, fallback: Color) -> Color {
    value
        .as_deref()
        .and_then(Color::parse_css)
        .unwrap_or(fallback)
}

impl ChartEngine {
    /// The Baseline series' effective baseline price: the pinned `baseline_value` option, or
    /// the visible-range close midpoint (the engine's auto mode when the option is unset).
    /// Shared by the baseline geometry builder and the bar-color resolution so both agree on
    /// which side of the baseline a bar sits.
    pub(crate) fn resolved_baseline_price(&self, id: SeriesId, from: i64, to: i64) -> Option<f64> {
        let series = self.series_entry(id)?;
        if let Some(price) = series.baseline {
            return Some(price);
        }
        let plot = self.data.plot(id);
        let close = plot.column(PlotValueIndex::Close);
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut any = false;
        for row in plot.visible_rows(from, to) {
            let value = close[row];
            if value.is_finite() {
                min = min.min(value);
                max = max.max(value);
                any = true;
            }
        }
        any.then_some((min + max) / 2.0)
    }

    /// reference `SeriesBarColorer.barColor` (model/series-bar-colorer.ts) for the series' bar at
    /// `row`: the color the built-in last-price line, the last-value axis label, and the
    /// crosshair marker background all follow when their own color option is unset.
    /// `baseline_price` is the resolved baseline for Baseline series (`None` for other kinds).
    pub(crate) fn series_bar_color(
        &self,
        series: &crate::SeriesEntry,
        row: usize,
        baseline_price: Option<f64>,
    ) -> Color {
        let plot = self.data.plot(series.id);
        // reference data-item colors: a per-point `color` (area reads `lineColor`, mapped onto the
        // body channel here) wins over the series-level resolution for every kind that reads
        // it (bar/candlestick/line/area/histogram); Baseline's barColor ignores data-item
        // colors (series-bar-colorer.ts Baseline arm).
        if !matches!(series.kind, SeriesKind::Baseline) {
            if let Some(c) = self
                .data
                .point_color(series.id, PointColorChannel::Body, row)
            {
                return Color(c);
            }
        }
        match series.kind {
            // A line_color still holding the default placeholder resolves to the kind default,
            // exactly like the geometry builders.
            SeriesKind::Line => verbatim_color(&series.line_color, crate::DEFAULT_LINE_COLOR),
            SeriesKind::Area => {
                let color = verbatim_color(&series.line_color, crate::DEFAULT_LINE_COLOR);
                if color != LINE {
                    color
                } else {
                    AREA_LINE
                }
            }
            SeriesKind::Histogram => {
                let color = verbatim_color(&series.line_color, crate::DEFAULT_LINE_COLOR);
                if color != LINE {
                    color
                } else {
                    HISTOGRAM
                }
            }
            // reference baseline colorer: top line color at/above the baseline, bottom below it.
            SeriesKind::Baseline => {
                let close = plot.value_at(row, PlotValueIndex::Close);
                match baseline_price {
                    Some(base) if close < base => series
                        .bottom_line_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(BASELINE_BOTTOM_LINE),
                    _ => series
                        .top_line_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(BASELINE_TOP_LINE),
                }
            }
            // reference bar/candlestick colorer: up when open <= close.
            SeriesKind::Candlestick | SeriesKind::Bar => {
                let open = plot.value_at(row, PlotValueIndex::Open);
                let close = plot.value_at(row, PlotValueIndex::Close);
                if open <= close {
                    verbatim_color(&series.up_color, UP)
                } else {
                    verbatim_color(&series.down_color, DOWN)
                }
            }
            // reference custom-series colorer (series-bar-colorer.ts Custom arm): the series `color`
            // option (the data-item color wins in reference; the host folds those into the custom
            // frame values, so this arm is only the exhaustiveness fallback).
            SeriesKind::Custom | SeriesKind::Feature => {
                verbatim_color(&series.line_color, crate::DEFAULT_LINE_COLOR)
            }
        }
    }
}

fn translate_prims_x(prims: &mut [Prim], dx: i32) {
    let dxf = dx as f32;
    for prim in prims {
        match prim {
            Prim::Rect { rect, .. } | Prim::RectFrame { rect, .. } => rect.x += dx,
            Prim::HLine { x0, x1, .. } => {
                *x0 += dx;
                *x1 += dx;
            }
            Prim::VLine { x, .. } => *x += dx,
            Prim::RoundRect { x, .. } => *x += dxf,
            Prim::Circle { cx, .. } => *cx += dxf,
            Prim::Triangle { a, b, c, .. } => {
                a[0] += dxf;
                b[0] += dxf;
                c[0] += dxf;
            }
            Prim::Text { x, .. } => *x += dxf,
            Prim::Image { rect, .. } => rect[0] += dxf,
            Prim::Polyline { .. }
            | Prim::AreaFill { .. }
            | Prim::BandFill { .. }
            | Prim::Background { .. } => {}
        }
    }
}

fn append_retained_layer(layer: &RetainedLayer, prims: &mut Vec<Prim>, points: &mut Vec<[f32; 2]>) {
    let point_base = points.len() as u32;
    points.extend_from_slice(&layer.points);
    prims.reserve(layer.prims.len());
    for prim in &layer.prims {
        let mut prim = prim.clone();
        match &mut prim {
            Prim::Polyline { first_point, .. } | Prim::AreaFill { first_point, .. } => {
                *first_point += point_base;
            }
            Prim::BandFill {
                upper_first,
                lower_first,
                ..
            } => {
                *upper_first += point_base;
                *lower_first += point_base;
            }
            _ => {}
        }
        prims.push(prim);
    }
}

impl ChartEngine {
    pub(crate) fn invalidate_frame_all(&mut self) {
        self.frame_invalidation.all();
    }

    pub(crate) fn invalidate_frame_scene(&mut self) {
        self.frame_invalidation.scene();
    }

    pub(crate) fn invalidate_frame_series(&mut self, id: SeriesId) {
        self.frame_invalidation.series(id);
    }

    pub(crate) fn invalidate_frame_drawings(&mut self) {
        self.frame_invalidation.drawings();
    }

    pub(crate) fn invalidate_frame_trading(&mut self) {
        self.frame_invalidation.trading();
    }

    pub(crate) fn invalidate_frame_overlay(&mut self) {
        self.frame_invalidation.overlay();
    }

    pub(crate) fn invalidate_frame_axis(&mut self) {
        self.frame_invalidation.axis();
    }

    pub fn frame_build_stats(&self) -> FrameBuildStats {
        self.frame_build_stats
    }

    pub fn frame_pane_segments(&self, pane: usize) -> Option<FramePaneSegments> {
        self.retained_frame.segments.get(pane).copied()
    }

    pub fn frame_series_segments(&self, pane: usize) -> &[FrameSeriesSegment] {
        self.retained_frame
            .series_segments
            .get(pane)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn frame_coordinate_revision(&self) -> u64 {
        self.retained_frame.coordinate_generation
    }

    pub fn frame_requires_layout(&self) -> bool {
        self.retained_frame.layout_generation != self.frame_invalidation.layout
            || self.retained_frame.autoscale_generation != self.frame_invalidation.autoscale
    }

    pub(crate) fn frame_layout_prepared(&mut self) {
        self.retained_frame.layout_generation = self.frame_invalidation.layout;
    }

    pub fn frame_requires_axis(&self) -> bool {
        self.retained_frame.axis_generation != self.frame_invalidation.axis
    }

    /// Force the next axis build to start from engine-owned labels. Browser extensions use this
    /// after mutating their transient axis views so detached plugin labels cannot be retained.
    pub fn invalidate_axis_frame(&mut self) {
        self.invalidate_frame_axis();
    }

    pub fn set_crosshair_at(&mut self, x: f64, y: f64) {
        let next = Some((x, y));
        if self.crosshair != next {
            self.crosshair = next;
            self.invalidate_frame_overlay();
        }
    }

    pub fn clear_crosshair_at(&mut self) {
        if self.crosshair.take().is_some() {
            self.invalidate_frame_overlay();
        }
    }

    /// Recompute pane price ranges for the current visible time window.
    /// Hosts that need scale-dependent layout measurements may call this before building a frame;
    /// `build_frame` calls it as well so standalone backends remain correct.
    pub fn autoscale_visible(&mut self) {
        self.frame_build_stats.autoscale_runs += 1;
        let mut before = std::mem::take(&mut self.retained_frame.last_price_scale_revisions);
        before.clear();
        before.extend(self.panes.iter().map(|pane| {
            [
                pane.price_scale.revision(),
                pane.left_scale.revision(),
                pane.overlay_scale.revision(),
            ]
        }));
        if let Some((from, to)) = self.visible_range_for_frame() {
            self.autoscale_for_frame(from, to);
        }
        if self.panes.iter().zip(&before).any(|(pane, before)| {
            [
                pane.price_scale.revision(),
                pane.left_scale.revision(),
                pane.overlay_scale.revision(),
            ] != *before
        }) {
            self.frame_invalidation.coordinates();
        }
        before.clear();
        before.extend(self.panes.iter().map(|pane| {
            [
                pane.price_scale.revision(),
                pane.left_scale.revision(),
                pane.overlay_scale.revision(),
            ]
        }));
        self.retained_frame.last_price_scale_revisions = before;
        self.retained_frame.autoscale_generation = self.frame_invalidation.autoscale;
    }

    /// Build the visible chart geometry as backend-neutral primitives.
    ///
    /// The frame owns no GPU buffers and performs no browser calls. It is suitable for WebGPU,
    /// Canvas2D, tiny-skia, screenshots, and golden tests alike.
    pub fn build_frame(&mut self) -> ChartFrame {
        let mut frame = ChartFrame::default();
        self.build_frame_into(&mut frame);
        frame
    }

    /// Rebuild a frame while retaining its pane, primitive, and point allocations. Hosts that
    /// repaint repeatedly should keep one `ChartFrame` and call this method instead of allocating
    /// a fresh tree for every cursor/animation frame.
    pub fn build_frame_into(&mut self, output: &mut ChartFrame) {
        self.frame_build_stats = FrameBuildStats::default();
        self.reset_lod_work();
        self.build_frame_into_accumulating(output);
    }

    /// Start one host-coordinated frame whose layout/axis preparation occurs before pane-frame
    /// construction. The browser uses this to keep diagnostics for the complete operation.
    pub fn begin_frame_build(&mut self) {
        self.frame_build_stats = FrameBuildStats::default();
        self.reset_lod_work();
    }

    fn sync_frame_input_invalidation(&mut self) {
        let layout_key = [
            self.css_width.to_bits(),
            self.css_height.to_bits(),
            self.dpr.to_bits(),
            self.pane_w.to_bits(),
            self.pane_h.to_bits(),
            self.pane_left.to_bits(),
            self.left_axis_w.to_bits(),
            self.axis_w.to_bits(),
            self.panes.len() as u64,
        ];
        let crosshair = self.crosshair.unwrap_or((f64::NAN, f64::NAN));
        let overlay_key = [
            crosshair.0.to_bits(),
            crosshair.1.to_bits(),
            self.crosshair_mode as u64,
            u64::from(self.crosshair_ohlc_magnet),
            self.animation_time.to_bits(),
            self.separator_hover.map_or(u64::MAX, |index| index as u64),
        ];
        let options_generation = self.options.generation();
        let series_revision = self.series.revision();
        if self.retained_frame.last_layout_key != Some(layout_key)
            || self.retained_frame.last_options_generation != options_generation
        {
            self.frame_invalidation.all();
        } else if self.retained_frame.last_series_revision != series_revision {
            self.frame_invalidation.scene();
        } else if self.retained_frame.last_overlay_key != Some(overlay_key) {
            self.frame_invalidation.overlay();
        }
        let time_scale_revision = self.time_scale.revision();
        if self.retained_frame.last_time_scale_revision != time_scale_revision {
            self.frame_invalidation.time_coordinates();
        }
        let price_scales_changed = self.retained_frame.last_price_scale_revisions.len()
            != self.panes.len()
            || self
                .panes
                .iter()
                .zip(&self.retained_frame.last_price_scale_revisions)
                .any(|(pane, revisions)| {
                    *revisions
                        != [
                            pane.price_scale.revision(),
                            pane.left_scale.revision(),
                            pane.overlay_scale.revision(),
                        ]
                });
        if price_scales_changed {
            self.frame_invalidation.coordinates();
        }
        self.retained_frame.last_layout_key = Some(layout_key);
        self.retained_frame.last_overlay_key = Some(overlay_key);
        self.retained_frame.last_options_generation = options_generation;
        self.retained_frame.last_series_revision = series_revision;
        self.retained_frame.last_time_scale_revision = time_scale_revision;
        self.retained_frame.last_price_scale_revisions.clear();
        self.retained_frame
            .last_price_scale_revisions
            .extend(self.panes.iter().map(|pane| {
                [
                    pane.price_scale.revision(),
                    pane.left_scale.revision(),
                    pane.overlay_scale.revision(),
                ]
            }));
    }

    /// Build pane geometry without resetting work already recorded by host layout preparation.
    pub fn build_frame_into_accumulating(&mut self, output: &mut ChartFrame) {
        self.sync_frame_input_invalidation();

        let layout_dirty = self.retained_frame.layout_generation != self.frame_invalidation.layout;
        let autoscale_dirty =
            self.retained_frame.autoscale_generation != self.frame_invalidation.autoscale;

        if layout_dirty {
            self.layout_for_frame();
            self.frame_build_stats.layout_rebuilds += 1;
        }
        let visible = self.visible_range_for_frame();
        if autoscale_dirty {
            self.autoscale_visible();
        }
        let scene_dirty = self.retained_frame.scene_generation != self.frame_invalidation.scene;
        let drawings_dirty =
            self.retained_frame.drawings_generation != self.frame_invalidation.drawings;
        let trading_dirty = !self.retained_frame.initialized
            || self.retained_frame.trading_generation != self.frame_invalidation.trading;
        let overlay_dirty =
            self.retained_frame.overlay_generation != self.frame_invalidation.overlay;
        let chrome_dirty = self.retained_frame.chrome_generation != self.frame_invalidation.chrome;

        // reference/fancy-canvas renders each pane with its actual bitmap/media ratio, which can differ
        // slightly from devicePixelRatio when a fractional-DPR pane dimension rounds. Using DPR
        // directly shifts bars and grid lines relative to the independently rounded pane bitmap.
        let nominal_dpr = self.dpr.max(0.01);
        let hpr = (self.pane_w * nominal_dpr).round().max(1.0) / self.pane_w.max(1.0);
        let vpr = (self.pane_h * nominal_dpr).round().max(1.0) / self.pane_h.max(1.0);
        let pane_count = self.panes.len().max(1);
        let pane_w_px = (self.pane_w * hpr).round().max(1.0) as u32;
        let pane_left_px = (self.pane_left * nominal_dpr).round().max(0.0) as u32;
        let mut resolved = Vec::with_capacity(self.series.len());
        // Paint in the chart's series order (reference z-order, pane.ts orderedSources): ids run
        // bottom to top so a later entry overpaints the earlier ones within its pane. With
        // `hoveredSeriesOnTop` the hovered series repaints topmost for the frame (reference
        // `hoveredSourceOnTopOrder` — render order only; the stable `series_order` and hit
        // arbitration are untouched). The bump is global across panes, which only reorders
        // within the hovered series' own pane since each pane paints its own series.
        let order = match self
            .hovered_series
            .filter(|_| self.options.get().hovered_series_on_top)
        {
            Some(hovered)
                if self.series_order.contains(&hovered)
                    && self.series_order.last() != Some(&hovered) =>
            {
                let mut bumped: Vec<SeriesId> = self
                    .series_order
                    .iter()
                    .copied()
                    .filter(|&id| id != hovered)
                    .collect();
                bumped.push(hovered);
                bumped
            }
            _ => self.series_order.clone(),
        };
        for &id in &order {
            let Some(s) = self.series_entry(id) else {
                continue;
            };
            let base_value = visible
                .and_then(|(from, _)| self.series_base_value(s.id, from))
                .unwrap_or(0.0);
            let up = verbatim_color(&s.up_color, UP);
            let down = verbatim_color(&s.down_color, DOWN);
            resolved.push(ResolvedSeries {
                id: s.id,
                kind: s.kind,
                color: verbatim_color(&s.line_color, crate::DEFAULT_LINE_COLOR),
                up,
                down,
                // reference parity: an unset wick/border color follows the body color of its direction.
                wick_up: verbatim_color(&s.wick_up_color, up),
                wick_down: verbatim_color(&s.wick_down_color, down),
                border_up: verbatim_color(&s.border_up_color, up),
                border_down: verbatim_color(&s.border_down_color, down),
                wick_visible: s.wick_visible.unwrap_or(true),
                border_visible: s.border_visible.unwrap_or(true),
                line_width: s.line_width.unwrap_or(LINE_WIDTH),
                line_style: crate::line_style_from_u8(s.line_style),
                line_visible: s.line_visible,
                area_top: verbatim_color(&s.area_top_color, AREA_TOP),
                area_bottom: verbatim_color(&s.area_bottom_color, AREA_BOTTOM),
                invert_filled_area: s.invert_filled_area,
                point_markers: s.point_markers,
                point_markers_radius: s.point_markers_radius,
                visible: s.visible,
                line_type: s.line_type,
                open_visible: s.open_visible,
                thin_bars: s.thin_bars,
                base: s.base,
                // reference baselineStyleDefaults; an unset quadrant line width follows the series'
                // line width (the reference's single baseline lineWidth). Quadrant colors are verbatim
                // CSS strings parsed here, with the reference default when unset/unparseable.
                top_fill1: s
                    .top_fill_color1
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(BASELINE_TOP_FILL1),
                top_fill2: s
                    .top_fill_color2
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(BASELINE_TOP_FILL2),
                top_line: s
                    .top_line_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(BASELINE_TOP_LINE),
                top_line_width: s.top_line_width.or(s.line_width).unwrap_or(LINE_WIDTH),
                top_line_style: crate::line_style_from_u8(s.top_line_style),
                bottom_fill1: s
                    .bottom_fill_color1
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(BASELINE_BOTTOM_FILL1),
                bottom_fill2: s
                    .bottom_fill_color2
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(BASELINE_BOTTOM_FILL2),
                bottom_line: s
                    .bottom_line_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(BASELINE_BOTTOM_LINE),
                bottom_line_width: s.bottom_line_width.or(s.line_width).unwrap_or(LINE_WIDTH),
                bottom_line_style: crate::line_style_from_u8(s.bottom_line_style),
                scale_target: series_scale_target(s),
                pane: (s.pane_index < pane_count).then_some(s.pane_index),
                base_value,
            });
        }

        output.width = self.pane_left + self.pane_w;
        output.height = self.pane_h;
        output.pixel_ratio = self.dpr;
        output.panes.resize_with(pane_count, FramePane::default);
        output.panes.truncate(pane_count);
        let time_marks = if layout_dirty || scene_dirty {
            self.time_marks_for_frame()
        } else {
            Vec::new()
        };
        let mut retained = std::mem::take(&mut self.retained_frame);
        retained
            .panes
            .resize_with(pane_count, RetainedPane::default);
        retained.panes.truncate(pane_count);
        retained
            .segments
            .resize(pane_count, FramePaneSegments::default());
        retained.segments.truncate(pane_count);
        retained.series_segments.resize_with(pane_count, Vec::new);
        retained.series_segments.truncate(pane_count);
        let initial_build = !retained.initialized;
        for (pi, pane) in self.panes.iter().enumerate() {
            let top_px = (pane.top * vpr).round().max(0.0) as u32;
            let height_px = (pane.height * vpr).round().max(0.0) as u32;
            let cache = &mut retained.panes[pi];
            cache.top = pane.top;
            cache.height = pane.height;
            cache.scissor = [pane_left_px, top_px, pane_w_px, height_px];

            if layout_dirty || scene_dirty {
                cache.under.prims.clear();
                cache.under.points.clear();
                if let Some(background) =
                    self.background_gradient_prim(pane_left_px, top_px, pane_w_px, height_px)
                {
                    cache.under.prims.push(background);
                }
                if let Some((from, to)) = visible {
                    let (grid_scale, grid_target) = if pane.price_scale.is_empty() {
                        (&pane.left_scale, PriceScaleTarget::Left)
                    } else {
                        (&pane.price_scale, PriceScaleTarget::Right)
                    };
                    self.build_grid_frame(
                        &mut cache.under.prims,
                        &time_marks,
                        from,
                        to,
                        pane_w_px as i32,
                        top_px as i32,
                        height_px as i32,
                        hpr,
                        vpr,
                        grid_scale,
                        self.scale_tick_base(pi, grid_target),
                    );
                    self.build_native_session_highlighting_frame(
                        pi,
                        from,
                        to,
                        hpr,
                        vpr,
                        &mut cache.under.prims,
                    );
                }
                self.build_native_image_watermark_frame(pi, hpr, vpr, &mut cache.under.prims);
                cache.under.revision = self.frame_invalidation.scene;
                cache.under.coordinate_revision = self.frame_invalidation.coordinate;
                self.frame_build_stats.grid_rebuilds += 1;
            }

            let series_layers_dirty = scene_dirty
                || resolved
                    .iter()
                    .filter(|rs| rs.pane == Some(pi) && rs.visible)
                    .any(|rs| {
                        let source_generation = self.frame_invalidation.series_generation(rs.id);
                        cache
                            .series_layers
                            .iter()
                            .find(|layer| layer.id == rs.id)
                            .is_none_or(|layer| {
                                layer.scene_generation != self.frame_invalidation.scene
                                    || layer.source_generation != source_generation
                            })
                    });
            if series_layers_dirty || chrome_dirty {
                cache
                    .series_layers
                    .retain(|layer| resolved.iter().any(|rs| rs.id == layer.id));
                cache.chrome.prims.clear();
                cache.chrome.points.clear();
                cache.top_layer.prims.clear();
                cache.top_layer.points.clear();
                if let Some((from, to)) = visible {
                    for rs in &resolved {
                        if rs.pane != Some(pi) || !rs.visible {
                            continue;
                        }
                        let source_generation = self.frame_invalidation.series_generation(rs.id);
                        let layer_index = match cache
                            .series_layers
                            .iter()
                            .position(|layer| layer.id == rs.id)
                        {
                            Some(index) => index,
                            None => {
                                cache.series_layers.push(RetainedSeriesLayer {
                                    id: rs.id,
                                    ..RetainedSeriesLayer::default()
                                });
                                cache.series_layers.len() - 1
                            }
                        };
                        let series_layer = &mut cache.series_layers[layer_index];
                        let rebuild_series = scene_dirty
                            || series_layer.scene_generation != self.frame_invalidation.scene
                            || series_layer.source_generation != source_generation;
                        if !rebuild_series {
                            continue;
                        }
                        series_layer.layer.prims.clear();
                        series_layer.layer.points.clear();
                        let scale = pane_scale(pane, rs.scale_target);
                        self.build_native_series_background_primitives_frame(
                            *rs,
                            from,
                            to,
                            hpr,
                            vpr,
                            &mut series_layer.layer.prims,
                            &mut series_layer.layer.points,
                            scale,
                        );
                        match rs.kind {
                            SeriesKind::Candlestick => self.build_candles_frame(
                                *rs,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut series_layer.layer.prims,
                                scale,
                            ),
                            SeriesKind::Bar => self.build_bars_frame(
                                *rs,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut series_layer.layer.prims,
                                scale,
                            ),
                            SeriesKind::Histogram => self.build_histogram_frame(
                                *rs,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut series_layer.layer.prims,
                                scale,
                            ),
                            SeriesKind::Line | SeriesKind::Area => {
                                if let Some((lower_level, upper_level)) =
                                    self.oscillator_channel(rs.id)
                                {
                                    let y_upper =
                                        (scale.price_to_coordinate(upper_level, rs.base_value)
                                            * vpr) as f32;
                                    let y_lower =
                                        (scale.price_to_coordinate(lower_level, rs.base_value)
                                            * vpr) as f32;
                                    let top = y_upper.min(y_lower);
                                    let height = (y_lower - y_upper).abs();
                                    if height >= 1.0 {
                                        series_layer.layer.prims.push(Prim::Rect {
                                            rect: IRect {
                                                x: 0,
                                                y: top.round() as i32,
                                                w: pane_w_px as i32,
                                                h: height.round() as i32,
                                            },
                                            color: Color::rgba(0x78, 0x7B, 0x86, 0x33),
                                        });
                                    }
                                }
                                self.build_line_frame(
                                    *rs,
                                    from,
                                    to,
                                    hpr,
                                    vpr,
                                    pane.top,
                                    pane.top + pane.height,
                                    &mut series_layer.layer.prims,
                                    &mut series_layer.layer.points,
                                    scale,
                                )
                            }
                            SeriesKind::Baseline => self.build_baseline_frame(
                                *rs,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut series_layer.layer.prims,
                                &mut series_layer.layer.points,
                                scale,
                            ),
                            SeriesKind::Feature => self.build_feature_series_frame(
                                *rs,
                                from,
                                to,
                                hpr,
                                vpr,
                                pane.top,
                                pane.height,
                                &mut series_layer.layer.prims,
                                &mut series_layer.layer.points,
                                scale,
                            ),
                            SeriesKind::Custom => {}
                        }
                        self.build_native_series_primitives_frame(
                            *rs,
                            from,
                            to,
                            hpr,
                            vpr,
                            &mut series_layer.layer.prims,
                            &mut series_layer.layer.points,
                            scale,
                        );
                        if self
                            .series
                            .iter()
                            .find(|series| series.id == rs.id)
                            .is_some_and(|series| {
                                series.markers_z_order == crate::marker_z_order::NORMAL
                            })
                        {
                            self.build_series_markers_frame(
                                rs.id,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut series_layer.layer.prims,
                            );
                        }
                        series_layer.scene_generation = self.frame_invalidation.scene;
                        series_layer.source_generation = source_generation;
                        series_layer.layer.revision =
                            self.frame_invalidation.scene.max(source_generation);
                        series_layer.layer.coordinate_revision = self.frame_invalidation.coordinate;
                        self.frame_build_stats.series_rebuilds += 1;
                    }
                    for rs in &resolved {
                        if rs.pane != Some(pi) || !rs.visible {
                            continue;
                        }
                        let Some(series) = self.series.iter().find(|series| series.id == rs.id)
                        else {
                            continue;
                        };
                        match series.markers_z_order {
                            crate::marker_z_order::ABOVE_SERIES => self.build_series_markers_frame(
                                rs.id,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut cache.chrome.prims,
                            ),
                            crate::marker_z_order::TOP => self.build_series_markers_frame(
                                rs.id,
                                from,
                                to,
                                hpr,
                                vpr,
                                &mut cache.top_layer.prims,
                            ),
                            _ => {}
                        }
                    }
                    self.build_price_lines_frame(
                        pi,
                        &mut cache.chrome.prims,
                        pane_w_px as i32,
                        vpr,
                    );
                    self.build_last_value_line_frame(
                        pi,
                        from,
                        to,
                        &mut cache.chrome.prims,
                        pane_w_px as i32,
                        hpr,
                        vpr,
                    );
                    self.build_bid_ask_lines_frame(
                        pi,
                        from,
                        &mut cache.chrome.prims,
                        pane_w_px as i32,
                        hpr,
                        vpr,
                    );
                    if pi == 0 {
                        self.build_last_pulse_frame(&mut cache.chrome.prims, hpr, vpr);
                    }
                }
                self.build_native_anchored_text_frame(pi, hpr, vpr, &mut cache.chrome.prims);
                self.build_native_text_watermark_frame(pi, hpr, vpr, &mut cache.chrome.prims);
                cache.chrome.revision = self.frame_invalidation.chrome;
                cache.chrome.coordinate_revision = self.frame_invalidation.coordinate;
                cache.top_layer.revision = self.frame_invalidation.chrome;
                cache.top_layer.coordinate_revision = self.frame_invalidation.coordinate;
            }

            if drawings_dirty {
                cache.drawings.prims.clear();
                cache.drawings.points.clear();
                self.build_drawings_frame(
                    pi,
                    pane_w_px as i32,
                    hpr,
                    vpr,
                    &mut cache.drawings.prims,
                    &mut cache.drawings.points,
                );
                cache.drawings.revision = self.frame_invalidation.drawings;
                cache.drawings.coordinate_revision = self.frame_invalidation.coordinate;
                self.frame_build_stats.drawing_rebuilds += 1;
            }

            if trading_dirty {
                cache.trading_regions.prims.clear();
                cache.trading_regions.points.clear();
                cache.trading.prims.clear();
                cache.trading.points.clear();
                self.build_trading_frame(
                    pi,
                    hpr,
                    vpr,
                    &mut cache.trading_regions.prims,
                    &mut cache.trading.prims,
                );
                cache.trading_regions.revision = self.frame_invalidation.trading;
                cache.trading_regions.coordinate_revision = self.frame_invalidation.coordinate;
                cache.trading.revision = self.frame_invalidation.trading;
                cache.trading.coordinate_revision = self.frame_invalidation.coordinate;
                self.frame_build_stats.trading_rebuilds += 1;
            }

            if overlay_dirty {
                cache.cursor_under.prims.clear();
                cache.cursor_under.points.clear();
                self.build_native_crosshair_highlight_frame(
                    pi,
                    hpr,
                    vpr,
                    &mut cache.cursor_under.prims,
                );
                self.build_native_tooltip_crosshair_frame(
                    pi,
                    hpr,
                    vpr,
                    &mut cache.cursor_under.prims,
                );
                cache.cursor_under.revision = self.frame_invalidation.overlay;
                cache.cursor_under.coordinate_revision = self.frame_invalidation.coordinate;
                cache.overlay.prims.clear();
                cache.overlay.points.clear();
                self.build_native_accessibility_focus_frame(pi, hpr, vpr, &mut cache.overlay.prims);
                self.build_selected_drawing_handles_frame(pi, hpr, vpr, &mut cache.overlay.prims);
                self.build_crosshair_frame(
                    pi,
                    pane_w_px as i32,
                    hpr,
                    vpr,
                    &mut cache.overlay.prims,
                );
                self.build_native_user_price_lines_button_frame(
                    pi,
                    hpr,
                    vpr,
                    &mut cache.overlay.prims,
                );
                self.build_native_user_price_alerts_frame(
                    pi,
                    hpr,
                    vpr,
                    &mut cache.overlay.prims,
                    &mut cache.overlay.points,
                );
                self.build_native_delta_tooltip_frame(pi, hpr, vpr, &mut cache.overlay.prims);
                if let Some((from, _)) = visible {
                    self.build_selection_anchors_frame(
                        pi,
                        from,
                        hpr,
                        vpr,
                        &mut cache.overlay.prims,
                    );
                }
                cache.overlay.revision = self.frame_invalidation.overlay;
                cache.overlay.coordinate_revision = self.frame_invalidation.coordinate;
                self.frame_build_stats.overlay_rebuilds += 1;
            }

            let out = &mut output.panes[pi];
            debug_assert_eq!(
                cache.under.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.cursor_under.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.chrome.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.drawings.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.trading_regions.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.trading.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.overlay.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert_eq!(
                cache.top_layer.coordinate_revision,
                self.frame_invalidation.coordinate
            );
            debug_assert!(cache
                .series_layers
                .iter()
                .filter(|layer| resolved.iter().any(|series| {
                    series.id == layer.id && series.pane == Some(pi) && series.visible
                }))
                .all(|layer| {
                    layer.layer.coordinate_revision == self.frame_invalidation.coordinate
                }));
            out.top = cache.top;
            out.height = cache.height;
            out.scissor = cache.scissor;
            out.under.clear();
            out.main.clear();
            out.top_prims.clear();
            out.series_paint_marks.clear();
            out.points.clear();
            // The first retained build knows its exact assembled size. Reserve once so initial
            // historical installs pay one retained-to-contract copy, not repeated Vec growth.
            if initial_build {
                let mut main_prims = cache.chrome.prims.len()
                    + cache.trading_regions.prims.len()
                    + cache.drawings.prims.len()
                    + cache.trading.prims.len()
                    + cache.overlay.prims.len();
                let mut point_count = cache.under.points.len()
                    + cache.cursor_under.points.len()
                    + cache.chrome.points.len()
                    + cache.trading_regions.points.len()
                    + cache.drawings.points.len()
                    + cache.trading.points.len()
                    + cache.overlay.points.len();
                point_count += cache.top_layer.points.len();
                for rs in &resolved {
                    if rs.pane == Some(pi) && rs.visible {
                        if let Some(layer) =
                            cache.series_layers.iter().find(|layer| layer.id == rs.id)
                        {
                            main_prims += layer.layer.prims.len();
                            point_count += layer.layer.points.len();
                        }
                    }
                }
                out.under
                    .reserve(cache.under.prims.len() + cache.cursor_under.prims.len());
                out.main.reserve(main_prims);
                out.points.reserve(point_count);
            }
            append_retained_layer(&cache.under, &mut out.under, &mut out.points);
            append_retained_layer(&cache.cursor_under, &mut out.under, &mut out.points);
            retained.series_segments[pi].clear();
            for rs in &resolved {
                if rs.pane != Some(pi) || !rs.visible {
                    continue;
                }
                let start = out.main.len();
                if let Some(layer) = cache.series_layers.iter().find(|layer| layer.id == rs.id) {
                    append_retained_layer(&layer.layer, &mut out.main, &mut out.points);
                    retained.series_segments[pi].push(FrameSeriesSegment {
                        series_id: Some(rs.id),
                        start,
                        end: out.main.len(),
                        revision: layer.layer.revision,
                        coordinate_revision: layer.layer.coordinate_revision,
                    });
                }
                out.series_paint_marks.push((rs.id, out.main.len()));
            }
            let chrome_start = out.main.len();
            append_retained_layer(&cache.chrome, &mut out.main, &mut out.points);
            retained.series_segments[pi].push(FrameSeriesSegment {
                series_id: None,
                start: chrome_start,
                end: out.main.len(),
                revision: cache.chrome.revision,
                coordinate_revision: cache.chrome.coordinate_revision,
            });
            let series_end = out.main.len();
            append_retained_layer(&cache.trading_regions, &mut out.main, &mut out.points);
            let trading_regions_end = out.main.len();
            append_retained_layer(&cache.drawings, &mut out.main, &mut out.points);
            let drawings_end = out.main.len();
            append_retained_layer(&cache.trading, &mut out.main, &mut out.points);
            let trading_end = out.main.len();
            append_retained_layer(&cache.overlay, &mut out.main, &mut out.points);
            let overlay_end = out.main.len();
            append_retained_layer(&cache.top_layer, &mut out.top_prims, &mut out.points);
            if pane_left_px != 0 {
                translate_prims_x(&mut out.under, pane_left_px as i32);
                translate_prims_x(&mut out.main, pane_left_px as i32);
                translate_prims_x(&mut out.top_prims, pane_left_px as i32);
                for point in &mut out.points {
                    point[0] += pane_left_px as f32;
                }
            }
            retained.segments[pi] = FramePaneSegments {
                under_end: out.under.len(),
                series_end,
                trading_regions_end,
                drawings_end,
                trading_end,
                overlay_end,
                under_revision: cache.under.revision.max(cache.cursor_under.revision),
                drawings_revision: cache.drawings.revision,
                trading_revision: cache.trading.revision,
                overlay_revision: cache.overlay.revision,
                top_revision: cache.top_layer.revision,
                coordinate_revision: self.frame_invalidation.coordinate,
            };
        }
        retained.layout_generation = self.frame_invalidation.layout;
        retained.scene_generation = self.frame_invalidation.scene;
        retained.chrome_generation = self.frame_invalidation.chrome;
        retained.drawings_generation = self.frame_invalidation.drawings;
        retained.trading_generation = self.frame_invalidation.trading;
        retained.overlay_generation = self.frame_invalidation.overlay;
        retained.autoscale_generation = self.frame_invalidation.autoscale;
        retained.coordinate_generation = self.frame_invalidation.coordinate;
        retained.last_price_scale_revisions.clear();
        retained
            .last_price_scale_revisions
            .extend(self.panes.iter().map(|pane| {
                [
                    pane.price_scale.revision(),
                    pane.left_scale.revision(),
                    pane.overlay_scale.revision(),
                ]
            }));
        retained.initialized = true;
        self.retained_frame = retained;
    }

    fn layout_for_frame(&mut self) {
        // Hosts may negotiate an inner content width (for example after measuring the price axis).
        // Preserve that negotiated viewport; standalone/native callers start with pane_w/pane_h
        // equal to the CSS size.
        self.pane_w = if self.pane_w > 0.0 {
            self.pane_w
        } else {
            self.css_width.max(1.0)
        };
        self.pane_h = if self.pane_h > 0.0 {
            self.pane_h
        } else {
            self.css_height.max(1.0)
        };
        self.layout_panes(self.pane_h);
    }

    /// The pane's `layout.background` gradient prim (reference VerticalGradient,
    /// pane-widget.ts `_drawBackground`): a two-stop vertical gradient covering the pane's
    /// bitmap rect. `None` for the solid variant — the backends' clear color paints that.
    /// Accepts the reference's `"gradient"` wire value (and the `"vertical_gradient"` alias).
    fn background_gradient_prim(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Prim> {
        let background = &self.options.get().layout.background;
        if background.kind != "gradient" && background.kind != "vertical_gradient" {
            return None;
        }
        let fallback = Color::rgb(
            nucleuscharts_core::style::DEFAULT_SURFACE_RGB.0,
            nucleuscharts_core::style::DEFAULT_SURFACE_RGB.1,
            nucleuscharts_core::style::DEFAULT_SURFACE_RGB.2,
        );
        Some(Prim::Background {
            rect: [x as f32, y as f32, w as f32, h as f32],
            gradient: Gradient {
                top: Color::parse_css(&background.top_color).unwrap_or(fallback),
                bottom: Color::parse_css(&background.bottom_color).unwrap_or(fallback),
            },
        })
    }

    pub(crate) fn visible_range_for_frame(&self) -> Option<(i64, i64)> {
        let n = self.data.merged_times().len() as i64;
        let r = self.time_scale.visible_strict_range()?;
        if n == 0 {
            return None;
        }
        let from = r.left().max(0);
        let to = r.right().min(n - 1);
        (from <= to).then_some((from, to))
    }

    /// Visible merged-time indices for host-side axis labels and hit-testing.
    pub fn visible_range(&self) -> Option<(i64, i64)> {
        self.visible_range_for_frame()
    }

    fn autoscale_for_frame(&mut self, from: i64, to: i64) {
        let n = self.panes.len().max(1);
        let scale_min_moves: Vec<[f64; 3]> = (0..n)
            .map(|pane| {
                [
                    self.scale_autoscale_min_move(pane, PriceScaleTarget::Right),
                    self.scale_autoscale_min_move(pane, PriceScaleTarget::Left),
                    self.scale_autoscale_min_move(pane, PriceScaleTarget::Overlay),
                ]
            })
            .collect();
        let mut main: Vec<Option<PriceRange>> = vec![None; n];
        let mut left: Vec<Option<PriceRange>> = vec![None; n];
        let mut overlay: Vec<Option<PriceRange>> = vec![None; n];
        let mut main_marker_margins = vec![(0.0_f64, 0.0_f64); n];
        let mut left_marker_margins = vec![(0.0_f64, 0.0_f64); n];
        let mut overlay_marker_margins = vec![(0.0_f64, 0.0_f64); n];
        for s in &self.series {
            // Hidden series remain engine-owned so they can be toggled back on, but—matching
            // reference—they must not contribute to the active price-scale autoscale range. A
            // pane-less series (its pane was removed) scales nowhere either.
            if !s.visible {
                continue;
            }
            let Some(pane_index) = (s.pane_index < n).then_some(s.pane_index) else {
                continue;
            };
            let mm = self.data.min_max_on_range_cached(
                s.id,
                from,
                to,
                &[PlotValueIndex::Low, PlotValueIndex::High],
            );
            let Some(mm) = mm else {
                continue;
            };
            let Some(base_value) = self.series_base_value(s.id, from) else {
                continue;
            };
            let scale_target = series_scale_target(s);
            let scale = pane_scale(&self.panes[pane_index], scale_target);
            let Some(range) =
                scale.price_range_to_logical(&PriceRange::new(mm.min, mm.max), base_value)
            else {
                continue;
            };
            let slot = match scale_target {
                PriceScaleTarget::Right => &mut main[pane_index],
                PriceScaleTarget::Left => &mut left[pane_index],
                PriceScaleTarget::Overlay => &mut overlay[pane_index],
            };
            *slot = Some(match slot.take() {
                Some(old) => old.merge(Some(&range)),
                None => range,
            });
            // Engine-owned volume profiles participate only while their anchored bar span overlaps
            // the visible logical range, matching the official primitive's autoscaleInfo gate.
            for primitive in &s.native_primitives {
                let range = match &primitive.kind {
                    crate::native_primitives::NativeSeriesPrimitiveKind::BandsIndicator(_) => {
                        let plot = self.data.plot(s.id);
                        let mut minimum = f64::INFINITY;
                        let mut maximum = f64::NEG_INFINITY;
                        for row in plot.visible_rows(from, to) {
                            if plot.is_whitespace_row(row) {
                                continue;
                            }
                            let price = plot.value_at(row, PlotValueIndex::Close);
                            if !price.is_finite() {
                                continue;
                            }
                            let first = price * 0.9;
                            let second = price * 1.1;
                            minimum = minimum.min(first.min(second));
                            maximum = maximum.max(first.max(second));
                        }
                        if !minimum.is_finite() || !maximum.is_finite() {
                            continue;
                        }
                        PriceRange::new(minimum, maximum)
                    }
                    crate::native_primitives::NativeSeriesPrimitiveKind::VolumeProfile {
                        data,
                        ..
                    } => {
                        let Some(logical) = self.time_to_index(data.time as f64, false) else {
                            continue;
                        };
                        if to < logical || from as f64 > logical as f64 + data.width {
                            continue;
                        }
                        let (minimum, maximum) = data.profile.iter().fold(
                            (f64::INFINITY, f64::NEG_INFINITY),
                            |(minimum, maximum), point| {
                                (minimum.min(point.price), maximum.max(point.price))
                            },
                        );
                        PriceRange::new(minimum, maximum)
                    }
                    crate::native_primitives::NativeSeriesPrimitiveKind::TrendLine {
                        first_time,
                        first_price,
                        second_time,
                        second_price,
                        ..
                    } => {
                        let (Some(first), Some(second)) = (
                            self.time_to_index(*first_time as f64, false),
                            self.time_to_index(*second_time as f64, false),
                        ) else {
                            continue;
                        };
                        if to < first.min(second) || from > first.max(second) {
                            continue;
                        }
                        PriceRange::new(
                            first_price.min(*second_price),
                            first_price.max(*second_price),
                        )
                    }
                    crate::native_primitives::NativeSeriesPrimitiveKind::ExpiringPriceAlerts(
                        state,
                    ) => {
                        let Some(first) = state.alerts.first() else {
                            continue;
                        };
                        let (minimum, maximum) = state.alerts.iter().skip(1).fold(
                            (first.price, first.price),
                            |(minimum, maximum), alert| {
                                (minimum.min(alert.price), maximum.max(alert.price))
                            },
                        );
                        PriceRange::new(minimum, maximum)
                    }
                    _ => continue,
                };
                let Some(profile_range) = scale.price_range_to_logical(&range, base_value) else {
                    continue;
                };
                *slot = Some(match slot.take() {
                    Some(old) => old.merge(Some(&profile_range)),
                    None => profile_range,
                });
            }
            if s.markers_auto_scale {
                let margins = marker_auto_scale_margins(&s.markers, self.time_scale.bar_spacing());
                let target = match scale_target {
                    PriceScaleTarget::Right => &mut main_marker_margins[pane_index],
                    PriceScaleTarget::Left => &mut left_marker_margins[pane_index],
                    PriceScaleTarget::Overlay => &mut overlay_marker_margins[pane_index],
                };
                target.0 = target.0.max(margins.0);
                target.1 = target.1.max(margins.1);
            }
        }
        // Series-primitive autoscale contributions (plugin platform Phase C-b): reference merges a
        // series primitive's `autoscaleInfo` into its owning series' autoscale info (series.ts
        // `_autoscaleInfoImpl`), and price-scale.ts `_recalculatePriceRangeImpl` only consults
        // sources that are visible with a first value — so a hidden, data-less, or pane-less
        // owning series silences its primitives' contributions too.
        for contribution in &self.primitive_autoscale {
            let Some(series) = self
                .series
                .iter()
                .find(|s| s.id == contribution.series && !s.removed)
            else {
                continue;
            };
            if !series.visible || contribution.pane != series.pane_index || contribution.pane >= n {
                continue;
            }
            let Some(base_value) = self.series_base_value(series.id, from) else {
                continue;
            };
            let scale = pane_scale(&self.panes[contribution.pane], contribution.target);
            let Some(range) = scale.price_range_to_logical(
                &PriceRange::new(contribution.min, contribution.max),
                base_value,
            ) else {
                continue;
            };
            let slot = match contribution.target {
                PriceScaleTarget::Right => &mut main[contribution.pane],
                PriceScaleTarget::Left => &mut left[contribution.pane],
                PriceScaleTarget::Overlay => &mut overlay[contribution.pane],
            };
            *slot = Some(match slot.take() {
                Some(old) => old.merge(Some(&range)),
                None => range,
            });
        }
        for (i, pane) in self.panes.iter_mut().enumerate() {
            let main_auto = pane.price_scale.is_auto_scale();
            let left_auto = pane.left_scale.is_auto_scale();
            let overlay_auto = pane.overlay_scale.is_auto_scale();
            pane.marker_margin_above = if main_auto {
                main_marker_margins[i].0
            } else {
                0.0
            };
            pane.marker_margin_below = if main_auto {
                main_marker_margins[i].1
            } else {
                0.0
            };
            pane.left_marker_margin_above = if left_auto {
                left_marker_margins[i].0
            } else {
                0.0
            };
            pane.left_marker_margin_below = if left_auto {
                left_marker_margins[i].1
            } else {
                0.0
            };
            pane.overlay_marker_margin_above = if overlay_auto {
                overlay_marker_margins[i].0
            } else {
                0.0
            };
            pane.overlay_marker_margin_below = if overlay_auto {
                overlay_marker_margins[i].1
            } else {
                0.0
            };
            pane.refresh_internal_margins();
            if main_auto {
                if let Some(range) = main[i].take() {
                    pane.price_scale
                        .apply_autoscale_range(Some(range), scale_min_moves[i][0]);
                }
            }
            if left_auto {
                if let Some(range) = left[i].take() {
                    pane.left_scale
                        .apply_autoscale_range(Some(range), scale_min_moves[i][1]);
                }
            }
            if overlay_auto {
                if let Some(range) = overlay[i].take() {
                    pane.overlay_scale.apply_autoscale_range(
                        Some(range.merge(Some(&PriceRange::new(0.0, 0.0)))),
                        scale_min_moves[i][2],
                    );
                }
            }
        }
    }

    fn time_marks_for_frame(&mut self) -> Vec<(i64, u8)> {
        // reference time-scale.ts:635 — `(fontSize + 4) * 5 / 8 * tickMarkMaxCharacterLength` with
        // the grid's fixed 12px estimate; the option widens/narrows the mark spacing.
        let max_width = (12.0 + 4.0) * 5.0 / 8.0 * f64::from(self.tick_mark_max_character_length);
        self.time_marks(max_width)
    }

    /// Build the time marks used by both the frame grid and host axis labels.
    pub fn time_marks(&mut self, max_label_width: f64) -> Vec<(i64, u8)> {
        self.tick_marks
            .build(self.time_scale.bar_spacing(), max_label_width)
            .iter()
            .map(|m| (m.index, m.weight))
            .collect()
    }
}
