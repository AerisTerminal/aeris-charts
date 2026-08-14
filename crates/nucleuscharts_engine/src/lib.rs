//! Headless Nucleus chart engine.
//!
//! This crate owns chart state and behavior without depending on WASM, the DOM, WebGPU, or a
//! native windowing system. Hosts provide input and a viewport; rendering backends consume the
//! frame produced from this state. During the architecture recovery, frame construction is being
//! migrated here incrementally from `nucleuscharts_wasm`.

mod axis_primitives;
mod drawings;
mod frame;
mod hit_test;
mod host_layout;
mod indicators;
mod interaction;
mod price_line_api;
mod price_scale_api;
mod series_query_api;
#[cfg(test)]
mod tests;
mod workspace;

use std::cell::Cell;

pub(crate) use drawings::{BrushCapture, DrawingDrag, PendingDrawing};
pub use drawings::{
    Drawing, DrawingDragPart, DrawingHit, DrawingId, DrawingKind, DrawingModifiers, DrawingPoint,
    TextMeasureFn, DRAWING_DEFAULT_COLOR,
};
pub use frame::{
    AxisFrame, AxisLabel, AxisLabelCorners, AxisTextAlign, AxisTextMidpoint, ChartFrame,
    FrameBuildStats, FramePane, FramePaneSegments, FrameSeriesSegment,
};
pub use hit_test::{SeriesHit, SeriesHitKind};
pub use indicators::IndicatorKind;
pub(crate) use indicators::{IndicatorBinding, IndicatorChange};
pub use interaction::{
    pinch_zoom_scale, wheel_zoom_scale, ScrollAnimation, KINETIC_DUMPING, KINETIC_MAX_SPEED,
    KINETIC_MIN_MOVE, KINETIC_MIN_SPEED, PINCH_ZOOM_INTENSITY, WHEEL_SCROLL_PX_PER_DELTA,
};
pub use workspace::{SplitDirection, Workspace, WorkspaceError, WorkspaceLayout, WorkspaceUsage};

use nucleuscharts_core::format::price_formatter::PriceFormatter;
use nucleuscharts_core::format::time_formatter::{MonthNames, DEFAULT_DATE_FORMAT};
use nucleuscharts_core::model::data_layer::{
    DataLayer, DataLayerMemoryUsage, PointColorChannel, SeriesId, SeriesIdError,
};
use nucleuscharts_core::model::data_validation::{
    sanitize_ohlc, sanitize_ohlc_styled, sanitize_point, ValidationError, ValidationReport,
};
use nucleuscharts_core::model::magnet::CrosshairMode;
use nucleuscharts_core::model::plot_list::{MismatchDirection, PlotValueIndex};
use nucleuscharts_core::model::price_range::PriceRange;
use nucleuscharts_core::model::range::{LogicalRange, StrictRange};
pub use nucleuscharts_core::options::ChartTheme;
use nucleuscharts_core::options::{chart_theme_patch, ChartOptionsStore};
use nucleuscharts_core::scale::price_scale_core::{
    PriceScaleCore, PriceScaleCoreOptions, PriceScaleMargins, PriceScaleMode,
};
use nucleuscharts_core::scale::time_scale_core::{TimeScaleCore, TimeScaleOptions};
use nucleuscharts_core::scale::time_tick_marks::TimeTickMarks;
use nucleuscharts_core::TimePointIndex;
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{LineStyle, LineType};

/// Host formatting callbacks (reference localization / `tickMarkFormatter`). Each returns `None` to fall
/// back to the built-in formatter. Boxed so the headless engine carries them without a js dependency.
pub type PriceFormatterFn = Box<dyn Fn(f64) -> Option<String>>;
pub type TickMarkFormatterFn = Box<dyn Fn(i64, u8) -> Option<String>>;
pub type TimeFormatterFn = Box<dyn Fn(i64) -> Option<String>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EngineMemoryUsage {
    pub data: DataLayerMemoryUsage,
    pub tick_payload_bytes: usize,
    pub tick_capacity_bytes: usize,
    pub indicator_runtime_bytes: usize,
    pub indicator_transfer_capacity_bytes: usize,
    pub retained_frame_capacity_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[doc(hidden)]
pub struct LodWorkStats {
    pub selected_level: usize,
    pub summary_nodes: usize,
    pub raw_rows: usize,
    pub candidates: usize,
}

impl EngineMemoryUsage {
    pub fn estimated_live_bytes(self) -> usize {
        self.data.logical_payload_bytes()
            + self.tick_payload_bytes
            + self.indicator_runtime_bytes
            + self.retained_frame_capacity_bytes
    }
}

/// reference `PriceFormat` kind (model/series-options.ts): the built-in `price` (precision/minMove
/// decimals), `volume` (K/M/B suffixes), `percent` (% sign), or a host `custom` formatter fn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceFormatKind {
    Price,
    Volume,
    Percent,
    Custom,
}

/// Per-series price format (reference series option `priceFormat`; series-options-defaults.ts:26-30
/// defaults to `{type:'price', precision:2, minMove:0.01}`). The boxed host formatter is
/// consulted only for [`PriceFormatKind::Custom`] (reference `priceFormat.formatter`), with a `None`
/// return falling back to the built-in price formatter.
pub struct SeriesPriceFormat {
    pub kind: PriceFormatKind,
    pub precision: u32,
    pub min_move: f64,
    pub formatter: Option<PriceFormatterFn>,
}

impl Default for SeriesPriceFormat {
    fn default() -> Self {
        Self {
            kind: PriceFormatKind::Price,
            precision: 2,
            min_move: 0.01,
            formatter: None,
        }
    }
}

impl SeriesPriceFormat {
    /// Whether the format still holds the reference's factory default — such a series defers to the
    /// chart-level `localization.priceFormatter`/built-in formatter, exactly like a series
    /// that never set `priceFormat`.
    pub fn is_reference_default(&self) -> bool {
        self.kind == PriceFormatKind::Price && self.precision == 2 && self.min_move == 0.01
    }
}

/// the reference's shared line-family default color (line/area/baseline `lineColor`, histogram `color`,
/// and the custom-series `color` — custom-series.ts `customStyleDefaults`).
pub const DEFAULT_LINE_COLOR: Color = Color::rgb(0x21, 0x96, 0xf3);
/// Default media-coordinate height of the horizontal axis. Hosts use this value during layout;
/// it is engine policy rather than a browser/demo constant.
pub const TIME_AXIS_HEIGHT: f64 = 28.0;

/// Hysteresis for `max_points` eviction: a series over its ceiling is trimmed back to
/// `max_points - max_points / CAP_TRIM_MARGIN_DIVISOR`, so the O(total) trim runs once per that
/// many appends instead of once per append. 32 keeps the post-trim floor within ~3% of the cap
/// (a 28,800-point 8-hour window trims every 900 bars) while making the amortized cost constant.
pub const CAP_TRIM_MARGIN_DIVISOR: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeriesKind {
    Candlestick,
    Bar,
    Line,
    Area,
    Histogram,
    Baseline,
    /// A plugin-defined series type (plugin platform Phase C-c; reference `addCustomSeries`). Its
    /// data-layer rows carry times only (whitespace-style); the host renders each item through
    /// the plugin's pane view and records the frame values the built-in chrome needs.
    Custom,
}

/// One custom series' last-value record (Phase C-c): the plugin's current value for the item
/// (the LAST element of `priceValueBuilder`, mirroring the Close slot of the reference's
/// `[last, max, min, last]` custom plot-row mapping — get-series-plot-row-creator.ts), the bar
/// color the reference's custom barColorer resolves (data item `color` ?? series `color`), and the
/// item's UTC-seconds time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomSeriesLastValue {
    pub value: f64,
    pub color: Color,
    pub time: i64,
}

/// Custom-series frame values (plugin platform Phase C-c), recorded by the host each frame
/// before any layout/frame pass consumes them — the same per-frame host-recording pattern as
/// [`PrimitiveAutoscaleContribution`]. A custom series' data rows are time-only, so the
/// values the built-in chrome needs — the percentage/indexed scale anchor (reference `firstValue`),
/// the built-in last-price line, the last-value axis label — arrive here, computed host-side
/// from the plugin's `priceValueBuilder` over its stored items.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CustomSeriesFrameValues {
    /// First visible non-whitespace item's current value (reference `firstValue()`).
    pub first_value: Option<f64>,
    /// Last non-whitespace item (reference `lastValueData(true)`).
    pub last: Option<CustomSeriesLastValue>,
    /// Last non-whitespace item at or left of the visible right edge (reference
    /// `lastValueData(false)`).
    pub last_visible: Option<CustomSeriesLastValue>,
}

/// The price scale that owns a series. Left and right are visible pane axes; overlay is the
/// axis-less independent scale used by volume and other pane overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceScaleTarget {
    Right,
    Left,
    Overlay,
}

/// One series primitive's autoscale contribution for the frame being built (plugin platform
/// Phase C-b; reference `ISeriesPrimitiveBase.autoscaleInfo` merged into the owning series' price
/// scale range, series.ts `_autoscaleInfoImpl`). Hosts record these between frames; the next
/// autoscale pass unions each into the owning scale's range (gated on the owning series being
/// visible with a first value, exactly like the reference's per-source autoscale gate).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimitiveAutoscaleContribution {
    /// The primitive's owning series.
    pub series: SeriesId,
    /// Pane of the owning series at record time.
    pub pane: usize,
    /// Price scale of the owning series at record time.
    pub target: PriceScaleTarget,
    /// Raw price bounds to union into the scale's range.
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeriesDataPoint {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarsInLogicalRange {
    pub bars_before: f64,
    pub bars_after: f64,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

impl SeriesKind {
    pub fn from_u8(kind: u8) -> Self {
        match kind {
            1 => Self::Bar,
            2 => Self::Line,
            3 => Self::Area,
            4 => Self::Histogram,
            5 => Self::Baseline,
            6 => Self::Custom,
            _ => Self::Candlestick,
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Candlestick => 0,
            Self::Bar => 1,
            Self::Line => 2,
            Self::Area => 3,
            Self::Histogram => 4,
            Self::Baseline => 5,
            Self::Custom => 6,
        }
    }

    fn stores_scalar_values(self) -> bool {
        matches!(
            self,
            Self::Line | Self::Area | Self::Histogram | Self::Baseline
        )
    }
}

pub fn line_style_from_u8(style: u8) -> LineStyle {
    match style {
        1 => LineStyle::Dotted,
        2 => LineStyle::Dashed,
        // The reference's retired variants (3 LargeDashed, 4 SparseDotted) fold into their
        // renamed equivalents — 3 renders exactly what `Dashed` renders, 4 what `Dotted` does.
        3 => LineStyle::Dashed,
        4 => LineStyle::Dotted,
        _ => LineStyle::Solid,
    }
}

/// reference `CrosshairMode` from its numeric wire form (unknown values fall back to Normal).
pub fn crosshair_mode_from_u8(mode: u8) -> CrosshairMode {
    use nucleuscharts_core::options::crosshair_mode as wire;
    match mode {
        wire::MAGNET => CrosshairMode::Magnet,
        wire::HIDDEN => CrosshairMode::Hidden,
        wire::MAGNET_OHLC => CrosshairMode::MagnetOhlc,
        _ => CrosshairMode::Normal,
    }
}

pub mod marker_pos {
    pub const ABOVE: u8 = 0;
    pub const BELOW: u8 = 1;
    pub const IN_BAR: u8 = 2;
}

pub mod marker_shape {
    pub const CIRCLE: u8 = 0;
    pub const SQUARE: u8 = 1;
    pub const ARROW_UP: u8 = 2;
    pub const ARROW_DOWN: u8 = 3;
}

#[derive(Clone)]
pub struct Marker {
    pub time: i64,
    pub position: u8,
    pub shape: u8,
    pub color: Color,
    pub text: String,
}

#[derive(Clone)]
pub struct PriceLine {
    pub id: u32,
    pub price: f64,
    pub color: Color,
    pub width: i32,
    pub style: LineStyle,
    pub title: String,
    /// reference `lineVisible` (default true): draw the horizontal line across the pane.
    pub line_visible: bool,
    /// reference `axisLabelVisible` (default true): show the boxed label on the price axis.
    pub axis_label_visible: bool,
    /// reference `axisLabelColor` (default `''`): label background; `None` follows the line color.
    pub axis_label_color: Option<String>,
    /// reference `axisLabelTextColor` (default `''`): label text; `None` uses semantic foreground
    /// against the label background (as the crosshair labels do).
    pub axis_label_text_color: Option<String>,
}

pub struct SeriesEntry {
    pub id: SeriesId,
    pub kind: SeriesKind,
    /// reference line/area/baseline `lineColor` (and the histogram `color`). Stored verbatim as a
    /// CSS string (reference `series.options()` returns the applied string); `None` is the
    /// kind-default placeholder [`DEFAULT_LINE_COLOR`], parsed only at render time.
    pub line_color: Option<String>,
    /// Candlestick/bar up & down body colors; `None` = reference default (follows the engine UP/DOWN
    /// palette). Stored verbatim as CSS strings; parsed at render time.
    pub up_color: Option<String>,
    pub down_color: Option<String>,
    /// Candlestick wick colors per direction; `None` falls back to the body color (reference parity).
    /// Stored verbatim as CSS strings; parsed at render time.
    pub wick_up_color: Option<String>,
    pub wick_down_color: Option<String>,
    /// Candlestick border colors per direction; `None` falls back to the body color (reference parity).
    /// Stored verbatim as CSS strings; parsed at render time.
    pub border_up_color: Option<String>,
    pub border_down_color: Option<String>,
    /// Candlestick part visibility; `None` = visible (reference parity).
    pub wick_visible: Option<bool>,
    pub border_visible: Option<bool>,
    pub line_width: Option<f64>,
    /// Area fill gradient colors; `None` = engine defaults. Stored verbatim as CSS strings;
    /// parsed at render time.
    pub area_top_color: Option<String>,
    pub area_bottom_color: Option<String>,
    pub histogram_updown: bool,
    pub overlay: bool,
    pub left_scale: bool,
    pub pane_index: usize,
    pub line_type: LineType,
    pub point_markers: bool,
    pub visible: bool,
    pub baseline: Option<f64>,
    pub last_price_animation: bool,
    /// reference `SeriesOptionsCommon.lastValueVisible` (series-options-defaults.ts: true): draw this
    /// series' last-value label on its price scale.
    pub last_value_visible: bool,
    /// reference `title` (series-options-defaults.ts: `''`): the series' display name. Shown as a
    /// chip in a darker shade of the label color at the front of the last-value label cluster
    /// when `title_visible` holds (TradingView-style).
    pub title: String,
    /// TradingView-style title-chip toggle (default true): include the series' `title` as the
    /// darker chip of the last-value cluster. The chip renders even when the price label itself
    /// is off (`last_value_visible: false`).
    pub title_visible: bool,
    /// TradingView-style candle-close countdown (default true): stack a countdown row below the
    /// price inside the last-value cluster. Hidden when the series has no usable bar interval
    /// or the host installed no clock (`now_override`).
    pub countdown_visible: bool,
    /// reference `priceLineVisible` (default true): draw the built-in last-price line for this series.
    pub price_line_visible: bool,
    /// reference `priceLineSource` (PriceLineSource): 0 = LastBar (default), 1 = LastVisible.
    pub price_line_source: u8,
    /// reference `priceLineWidth` in CSS px (default 1).
    pub price_line_width: f64,
    /// reference `priceLineColor` (default `''`): `None` follows the last bar's color. The CSS
    /// string is stored verbatim (reference `series.options()` returns the applied string); it is
    /// parsed only at render time, falling back to the follow behavior when unparseable.
    pub price_line_color: Option<String>,
    /// reference `priceLineStyle` (default 1 = Dotted; the reference LineStyle numbering).
    pub price_line_style: u8,
    /// TradingView-style bid/ask lines + axis chips (default OFF — platforms opt in). Values
    /// are pushed by the host via `set_bid_ask`; when visible, each side with a value draws a
    /// horizontal line across the pane and a "Bid"/"Ask"-titled chip on the series' scale.
    pub bid_ask_visible: bool,
    /// Current bid price (`None` hides the bid side).
    pub bid: Option<f64>,
    /// Current ask price (`None` hides the ask side).
    pub ask: Option<f64>,
    /// Bid line/chip color (default: the canonical primary token). Stored verbatim.
    pub bid_color: String,
    /// Ask line/chip color (default: the canonical loss token). Stored verbatim.
    pub ask_color: String,
    /// Bid/ask line width in CSS px (default 1, mirrors `price_line_width`).
    pub bid_ask_line_width: f64,
    /// Bid/ask line style (default 1 = Dotted, mirrors `price_line_style`).
    pub bid_ask_line_style: u8,
    /// reference line/area/baseline `lineStyle` (default 0 = Solid; reference LineStyle numbering).
    pub line_style: u8,
    /// reference `lineVisible` (default true): hides the line stroke; an area keeps its fill and a
    /// line series keeps only its point markers.
    pub line_visible: bool,
    /// reference `pointMarkersRadius` (default `undefined`): `None` = auto (`lineWidth / 2 + 2`,
    /// line-pane-view.ts).
    pub point_markers_radius: Option<f64>,
    /// reference baseline `topFillColor1` (default `rgba(38, 166, 154, 0.28)`); `None` = reference default.
    /// Stored verbatim as a CSS string; parsed at render time.
    pub top_fill_color1: Option<String>,
    /// reference baseline `topFillColor2` (default `rgba(38, 166, 154, 0.05)`); `None` = reference default.
    /// Stored verbatim as a CSS string; parsed at render time.
    pub top_fill_color2: Option<String>,
    /// reference baseline `topLineColor` (default `rgba(38, 166, 154, 1)`); `None` = reference default.
    /// Stored verbatim as a CSS string; parsed at render time.
    pub top_line_color: Option<String>,
    /// Baseline top-quadrant line width in CSS px; `None` follows `line_width` (the reference's single
    /// baseline `lineWidth`, default 3). reference has no per-quadrant width; the option is an
    /// engine extension mirroring the quadrant colors.
    pub top_line_width: Option<f64>,
    /// Baseline top-quadrant line style (default 0 = Solid; the reference's shared `lineStyle`).
    pub top_line_style: u8,
    /// reference baseline `bottomFillColor1` (default `rgba(239, 83, 80, 0.05)`); `None` = reference
    /// default. Stored verbatim as a CSS string; parsed at render time.
    pub bottom_fill_color1: Option<String>,
    /// reference baseline `bottomFillColor2` (default `rgba(239, 83, 80, 0.28)`); `None` = reference
    /// default. Stored verbatim as a CSS string; parsed at render time.
    pub bottom_fill_color2: Option<String>,
    /// reference baseline `bottomLineColor` (default `rgba(239, 83, 80, 1)`); `None` = reference default.
    /// Stored verbatim as a CSS string; parsed at render time.
    pub bottom_line_color: Option<String>,
    /// Baseline bottom-quadrant line width; `None` follows `line_width` (see `top_line_width`).
    pub bottom_line_width: Option<f64>,
    /// Baseline bottom-quadrant line style (default 0 = Solid).
    pub bottom_line_style: u8,
    /// reference histogram `base` (default 0): the price level columns grow from.
    pub base: f64,
    /// reference area `invertFilledArea` (default false): fill above the line instead of below.
    pub invert_filled_area: bool,
    /// reference bar `openVisible` (default true): draw the open tick on OHLC bars.
    pub open_visible: bool,
    /// reference bar `thinBars` (default true): bar body width capped to the crisp line width.
    pub thin_bars: bool,
    /// reference `priceFormat` (series-options-defaults.ts: `{type:'price', precision:2, minMove:0.01}`):
    /// drives this series' last-value label, its price-line labels, the crosshair price label
    /// when this series is the label source, and the axis ticks when it is the scale's primary
    /// source.
    pub price_format: SeriesPriceFormat,
    pub price_lines: Vec<PriceLine>,
    pub markers: Vec<Marker>,
    pub markers_auto_scale: bool,
    /// Tombstone flag (reference `removeSeries`). The backing slot may later hold another opaque ID.
    /// and this vector, so a removed series keeps its slot (data emptied, hidden) rather than being
    /// compacted; every other series keeps its id. Removed slots are inert in every draw/scale path
    /// because they carry no data and are not visible.
    pub removed: bool,
    /// Custom series (Phase C-c): host-recorded frame values (first/last values for the scale
    /// anchor, the last-value label, and the built-in last-price line). Refreshed per frame by
    /// the host before any layout/frame pass consumes them; unused by other kinds.
    pub custom_frame: CustomSeriesFrameValues,
    /// Retention ceiling: the series holds at most this many rows, oldest evicted first.
    /// `None` (the default) is unbounded — a series grows for as long as the host appends to it.
    /// See [`ChartEngine::set_series_max_points`] for the eviction schedule.
    pub max_points: Option<usize>,
}

impl SeriesEntry {
    pub fn new(id: SeriesId, kind: SeriesKind) -> Self {
        Self {
            id,
            kind,
            line_color: None,
            up_color: None,
            down_color: None,
            wick_up_color: None,
            wick_down_color: None,
            border_up_color: None,
            border_down_color: None,
            wick_visible: None,
            border_visible: None,
            line_width: None,
            area_top_color: None,
            area_bottom_color: None,
            histogram_updown: false,
            overlay: false,
            left_scale: false,
            pane_index: 0,
            line_type: LineType::Simple,
            point_markers: false,
            visible: true,
            baseline: None,
            last_price_animation: false,
            // reference defaults: series-options-defaults.ts (common), line/area/baseline-series.ts
            // (line family + baseline quadrants), bar-series.ts, histogram-series.ts.
            last_value_visible: true,
            title: String::new(),
            title_visible: true,
            countdown_visible: true,
            price_line_visible: true,
            price_line_source: 0,
            price_line_width: 1.0,
            price_line_color: None,
            price_line_style: 1,
            bid_ask_visible: false,
            bid: None,
            ask: None,
            bid_color: nucleuscharts_core::style::DEFAULT_PRIMARY_CSS.to_string(),
            ask_color: nucleuscharts_core::style::MARKET_DOWN_CSS.to_string(),
            bid_ask_line_width: 1.0,
            bid_ask_line_style: 1,
            line_style: 0,
            line_visible: true,
            point_markers_radius: None,
            top_fill_color1: None,
            top_fill_color2: None,
            top_line_color: None,
            top_line_width: None,
            top_line_style: 0,
            bottom_fill_color1: None,
            bottom_fill_color2: None,
            bottom_line_color: None,
            bottom_line_width: None,
            bottom_line_style: 0,
            base: 0.0,
            invert_filled_area: false,
            open_visible: true,
            thin_bars: true,
            price_format: SeriesPriceFormat::default(),
            price_lines: Vec::new(),
            markers: Vec::new(),
            markers_auto_scale: true,
            removed: false,
            custom_frame: CustomSeriesFrameValues::default(),
            max_points: None,
        }
    }
}

pub const PANE_SEPARATOR: f64 = 1.0;

/// `pane_index` sentinel for a series whose pane was removed (reference `removePane` orphans the
/// pane's series — `paneForSource` turns null): the series keeps its data but renders and
/// scales nowhere until re-assigned to a live pane.
pub(crate) const PANELESS: usize = usize::MAX;

pub struct Pane {
    /// Chart-local identity that survives index changes and is never reused. Zero is reserved for
    /// standalone/default panes that are not owned by a [`ChartEngine`].
    stable_id: u32,
    pub price_scale: PriceScaleCore,
    pub left_scale: PriceScaleCore,
    pub overlay_scale: PriceScaleCore,
    pub stretch_factor: f64,
    pub overlay_top: f64,
    pub overlay_bottom: f64,
    /// reference pane.ts `_preserveEmptyPane` (default false): an empty pane collapses on the next
    /// series removal/move-out unless this holds it open (chart-model.ts
    /// `_cleanupIfPaneIsEmpty`).
    pub preserve_empty: bool,
    pub marker_margin_above: f64,
    pub marker_margin_below: f64,
    pub left_marker_margin_above: f64,
    pub left_marker_margin_below: f64,
    pub overlay_marker_margin_above: f64,
    pub overlay_marker_margin_below: f64,
    pub top: f64,
    pub height: f64,
}

impl Pane {
    pub fn new() -> Self {
        Self::with_stable_id(0)
    }

    fn with_stable_id(stable_id: u32) -> Self {
        let main_scale = PriceScaleCore::new(PriceScaleCoreOptions::default());
        let overlay_scale = PriceScaleCore::new(PriceScaleCoreOptions {
            scale_margins: PriceScaleMargins {
                top: 0.8,
                bottom: 0.0,
            },
            ..PriceScaleCoreOptions::default()
        });
        Self {
            stable_id,
            price_scale: main_scale,
            left_scale: PriceScaleCore::new(PriceScaleCoreOptions::default()),
            overlay_scale,
            stretch_factor: 1.0,
            overlay_top: 0.8,
            overlay_bottom: 0.0,
            preserve_empty: false,
            marker_margin_above: 0.0,
            marker_margin_below: 0.0,
            left_marker_margin_above: 0.0,
            left_marker_margin_below: 0.0,
            overlay_marker_margin_above: 0.0,
            overlay_marker_margin_below: 0.0,
            top: 0.0,
            height: 0.0,
        }
    }

    pub fn stable_id(&self) -> u32 {
        self.stable_id
    }

    pub fn layout(&mut self, content_h: f64) {
        self.price_scale.set_height(content_h);
        self.left_scale.set_height(content_h);
        self.overlay_scale.set_height(content_h);
        // Fractional scale margins (top 0.2 / bottom 0.1) resolve against the pane's OWN slot,
        // not the full content height — a small pane would otherwise go negative inside and
        // flip its coordinate mapping (a dragged-short pane inverting its scale).
        self.price_scale.set_margins_height(self.height);
        self.left_scale.set_margins_height(self.height);
        self.overlay_scale.set_margins_height(self.height);
        self.refresh_internal_margins();
    }

    pub fn refresh_internal_margins(&mut self) {
        let content_h = self.price_scale.height();
        let below = (content_h - self.top - self.height).max(0.0);
        self.price_scale.set_internal_margins(
            self.top + self.marker_margin_above,
            below + self.marker_margin_below,
        );
        self.left_scale.set_internal_margins(
            self.top + self.left_marker_margin_above,
            below + self.left_marker_margin_below,
        );
        self.overlay_scale.set_internal_margins(
            self.top + self.overlay_marker_margin_above,
            below + self.overlay_marker_margin_below,
        );
    }
}

impl Default for Pane {
    fn default() -> Self {
        Self::new()
    }
}

/// Platform-independent state for one chart instance.
/// Platform-independent state for one chart instance.
pub struct ChartEngine {
    pub time_scale: TimeScaleCore,
    pub panes: Vec<Pane>,
    pub price_formatter: PriceFormatter,
    data: DataLayer,
    pub series: Vec<SeriesEntry>,
    tick_marks: TimeTickMarks,
    next_pane_id: u32,
    pub options: ChartOptionsStore,
    pub crosshair_mode: CrosshairMode,
    /// TradingView's Ctrl-held magnet: while set, a Normal-mode crosshair snaps to the hovered
    /// bar's OHLC exactly like `CrosshairMode::MagnetOhlc` (frame/crosshair.rs
    /// `crosshair_snap`). The gesture layer forwards the live modifier state; the configured
    /// `crosshair_mode` is untouched (Magnet/MagnetOhlc stay as configured, Hidden stays
    /// hidden).
    pub crosshair_ohlc_magnet: bool,
    pub animation_time: f64,
    pub next_price_line_id: u32,
    /// reference `timeScale.timeVisible` — label semantics only: whether axis/crosshair time labels
    /// include the time of day. Strip reservation is [`Self::time_axis_visible`].
    pub time_visible: bool,
    /// reference `timeScale.visible` (default true): reserve and paint the whole time-axis strip.
    /// When false the strip collapses to zero height and its labels/ticks/border vanish.
    pub time_axis_visible: bool,
    /// reference `timeScale.ticksVisible` (default false): tick marks on the time axis.
    pub time_ticks_visible: bool,
    /// reference `timeScale.minimumHeight` (default 0 = the [`TIME_AXIS_HEIGHT`] auto height): floor
    /// for the time-axis strip height.
    pub time_axis_minimum_height: f64,
    /// reference `timeScale.tickMarkMaxCharacterLength` (default 8): tick-label width cap in
    /// characters. 0 restores the default (the reference's `|| defaultTickMarkMaxCharacterLength`).
    pub tick_mark_max_character_length: u32,
    /// Hovered pane separator index for the hover band (reference pane-separator.ts
    /// `separatorHoverColor` handle); `None` paints nothing. Mirrored into the axis frame.
    pub separator_hover: Option<usize>,
    /// reference `timeScale.secondsVisible` — include seconds in axis/crosshair time labels when
    /// `time_visible` is set. Defaults to false (reference default).
    pub seconds_visible: bool,
    pub css_width: f64,
    pub css_height: f64,
    pub dpr: f64,
    /// Host-installed clock (UTC seconds) for the candle-close countdown rows of the last-value
    /// label clusters (TradingView-style extension). The engine is headless: countdown rows stay
    /// hidden until a host supplies the time — the wasm render path feeds the browser's system
    /// time every frame unless a value is pinned; tests pin one here for determinism.
    pub now_override: Option<f64>,
    pub crosshair: Option<(f64, f64)>,
    pub pane_w: f64,
    pub pane_h: f64,
    /// Media-coordinate x offset of the pane after reserving a visible left axis.
    pub pane_left: f64,
    pub left_axis_w: f64,
    pub axis_w: f64,
    indicators: Vec<IndicatorBinding>,
    indicator_changes: Vec<(SeriesId, IndicatorChange)>,
    synced_points_len: usize,
    synced_time_points_generation: u64,
    synced_last_time: Option<i64>,
    synced_first_time: Option<i64>,
    /// reference `localization.dateFormat` (default `dd MMM \'yy`): drives the crosshair time label.
    pub date_format: String,
    /// Per-locale month-name tables (reference `localization.locale`) used by the date-format
    /// `MMM`/`MMMM` tokens and the month tick labels. Hosts inject locale-derived names (the
    /// wasm host builds them from `Intl.DateTimeFormat`); the headless default is English.
    pub month_names: MonthNames,
    /// Series ids in render order, bottom to top (topmost LAST — the reference's z-order, pane.ts
    /// `orderedSources`/`setSeriesOrder`). Live series only: removed slots leave the list.
    series_order: Vec<SeriesId>,
    /// The series under the cursor (reference `ChartModel._hoveredSource`), refreshed by hosts
    /// from their hover pipeline. When `hoveredSeriesOnTop` holds, the frame build paints
    /// this series topmost (reference `hoveredSourceOnTopOrder`) without touching `series_order`.
    hovered_series: Option<SeriesId>,
    /// The series the host last clicked (TradingView-style selection): while set, the frame
    /// build paints anchor points on its drawn data points (theme-derived fill, accent-blue
    /// border). Clicking empty pane space clears it.
    selected_series: Option<SeriesId>,
    /// Series-primitive autoscale contributions for the current frame build (Phase C-b).
    /// Hosts clear and re-record them per frame, before any layout/autoscale pass runs;
    /// `autoscale_for_frame` unions them into the owning scales.
    primitive_autoscale: Vec<PrimitiveAutoscaleContribution>,
    /// Engine-owned drawing objects (drawing tools: trend/horizontal/vertical lines, rectangle,
    /// text) in z-order, bottom first. See drawings.rs.
    drawings: Vec<Drawing>,
    /// Next chart-unique drawing id (never reused; starts at 1 — 0 is the "no drawing" sentinel).
    next_drawing_id: DrawingId,
    /// The drawing the host last clicked (TradingView-style selection): while set, the frame
    /// build paints anchor handles at its defining points and its anchors accept drags.
    selected_drawing: Option<DrawingId>,
    /// Active anchor/body drag session on a drawing (drawings.rs; the interaction.rs session
    /// pattern — the engine owns the start snapshot and the math).
    drawing_drag: Option<DrawingDrag>,
    /// Interactive drawing creation in progress (drawings.rs): committed anchors plus a
    /// preview point following the mouse.
    pending_drawing: Option<PendingDrawing>,
    /// Freehand brush capture in progress (drawings.rs): the decimated point list, simplified
    /// at commit.
    brush_capture: Option<BrushCapture>,
    /// The text drawing the host's typing-mode editor currently owns (drawings.rs): its
    /// placeholder/label is suppressed in the frame so the editor's preview is the only
    /// visual for it.
    editing_drawing: Option<DrawingId>,
    /// Optional host text-measure callback for drawing-label hit boxes (drawings.rs
    /// [`TextMeasureFn`]); without one the engine estimates widths by character count.
    text_measure_fn: Option<TextMeasureFn>,
    /// Kinetic (momentum) scroll sampler/coast for the active drag (engine interaction module,
    /// reference `KineticAnimation`); the host feeds samples and drives the coast per frame.
    kinetic: Option<nucleuscharts_core::model::kinetic_animation::KineticAnimation>,
    /// In-flight eased scroll-to-position (engine interaction module); the host schedules the
    /// ticks, the engine owns the easing and applies each step.
    scroll_animation: Option<interaction::ScrollAnimation>,
    /// Optional host formatting callbacks (reference `localization.priceFormatter`/`timeFormatter` and
    /// `timeScale.tickMarkFormatter`). The engine stays headless — the host supplies plain boxed
    /// closures; each returns `None` to fall back to the built-in formatter (e.g. the callback
    /// threw at the boundary). Kept as trait objects, so `ChartEngine` is intentionally not
    /// `Clone`/`Debug`/`Send`.
    price_formatter_fn: Option<PriceFormatterFn>,
    tick_mark_formatter_fn: Option<TickMarkFormatterFn>,
    time_formatter_fn: Option<TimeFormatterFn>,
    frame_invalidation: frame::FrameInvalidation,
    retained_frame: frame::RetainedFrame,
    frame_build_stats: FrameBuildStats,
    lod_work: Cell<LodWorkStats>,
}

impl ChartEngine {
    pub fn new(css_width: f64, css_height: f64, dpr: f64) -> Self {
        let mut data = DataLayer::new();
        let main = data.add_series();
        Self {
            time_scale: TimeScaleCore::new(TimeScaleOptions::default()),
            panes: vec![Pane::with_stable_id(1)],
            price_formatter: PriceFormatter::default(),
            data,
            series: vec![SeriesEntry::new(main, SeriesKind::Candlestick)],
            tick_marks: TimeTickMarks::new(),
            next_pane_id: 2,
            options: ChartOptionsStore::new(),
            crosshair_mode: CrosshairMode::Normal,
            crosshair_ohlc_magnet: false,
            animation_time: 0.0,
            next_price_line_id: 1,
            time_visible: true,
            time_axis_visible: true,
            time_ticks_visible: false,
            time_axis_minimum_height: 0.0,
            tick_mark_max_character_length: 8,
            separator_hover: None,
            seconds_visible: false,
            css_width,
            css_height,
            dpr,
            now_override: None,
            crosshair: None,
            pane_w: css_width,
            pane_h: css_height,
            pane_left: 0.0,
            left_axis_w: 0.0,
            axis_w: 0.0,
            indicators: Vec::new(),
            indicator_changes: Vec::new(),
            synced_points_len: 0,
            synced_time_points_generation: 0,
            synced_last_time: None,
            synced_first_time: None,
            date_format: DEFAULT_DATE_FORMAT.to_string(),
            month_names: MonthNames::default(),
            series_order: vec![main],
            hovered_series: None,
            selected_series: None,
            primitive_autoscale: Vec::new(),
            drawings: Vec::new(),
            next_drawing_id: 1,
            selected_drawing: None,
            drawing_drag: None,
            pending_drawing: None,
            brush_capture: None,
            editing_drawing: None,
            text_measure_fn: None,
            kinetic: None,
            scroll_animation: None,
            price_formatter_fn: None,
            tick_mark_formatter_fn: None,
            time_formatter_fn: None,
            frame_invalidation: frame::FrameInvalidation::default(),
            retained_frame: frame::RetainedFrame::default(),
            frame_build_stats: FrameBuildStats::default(),
            lod_work: Cell::new(LodWorkStats::default()),
        }
    }

    #[doc(hidden)]
    pub fn lod_work_stats(&self) -> LodWorkStats {
        self.lod_work.get()
    }

    pub(crate) fn reset_lod_work(&self) {
        self.lod_work.set(LodWorkStats::default());
    }

    pub(crate) fn record_lod_work(
        &self,
        selected_level: usize,
        summary_nodes: usize,
        raw_rows: usize,
        candidates: usize,
    ) {
        let mut work = self.lod_work.get();
        work.selected_level = work.selected_level.max(selected_level);
        work.summary_nodes += summary_nodes;
        work.raw_rows += raw_rows;
        work.candidates += candidates;
        self.lod_work.set(work);
    }

    /// Read-only access to canonical series data. All mutation must use engine commands so time,
    /// scale, indicator, retention, and invalidation invariants remain synchronized.
    pub fn data_layer(&self) -> &DataLayer {
        &self.data
    }

    /// Structure-level memory attribution for engineering evidence. This reports logical payload
    /// and vector capacity, not allocator metadata, committed WASM pages, or browser memory.
    pub fn memory_usage(&self) -> EngineMemoryUsage {
        let (indicator_runtime_bytes, indicator_transfer_capacity_bytes) =
            self.indicator_memory_usage();
        EngineMemoryUsage {
            data: self.data.memory_usage(),
            tick_payload_bytes: self.tick_marks.payload_bytes(),
            tick_capacity_bytes: self.tick_marks.capacity_bytes(),
            indicator_runtime_bytes,
            indicator_transfer_capacity_bytes,
            retained_frame_capacity_bytes: self.retained_frame.capacity_bytes(),
        }
    }

    /// Read-only time tick state derived from the canonical timestamp sequence.
    pub fn tick_marks(&self) -> &TimeTickMarks {
        &self.tick_marks
    }

    /// Read-only live/storage entries. Series mutation remains staged through existing engine
    /// commands; the public field is retained temporarily for host option APIs that still need a
    /// controlled migration.
    pub fn series_entries(&self) -> &[SeriesEntry] {
        &self.series
    }

    /// Install (or clear with `None`) the host price formatter (reference `localization.priceFormatter`).
    /// Applied to non-percentage price labels; a `None` return from the callback falls back to the
    /// built-in formatter.
    pub fn set_price_formatter(&mut self, f: Option<PriceFormatterFn>) {
        self.price_formatter_fn = f;
        self.invalidate_frame_all();
    }

    /// Install (or clear) the host time-axis tick formatter (reference `timeScale.tickMarkFormatter`).
    /// The callback receives the UTC-second timestamp and the tick-mark type (0 Year, 1 Month,
    /// 2 DayOfMonth, 3 Time, 4 TimeWithSeconds).
    pub fn set_tick_mark_formatter(&mut self, f: Option<TickMarkFormatterFn>) {
        self.tick_mark_formatter_fn = f;
        self.invalidate_frame_scene();
    }

    /// Pin the engine clock (UTC seconds) used by the candle-close countdown rows of the
    /// last-value label clusters. Hosts with a ticking countdown call this on every tick;
    /// per frame the wasm render path also feeds the system time when nothing was pinned.
    pub fn set_now_seconds(&mut self, now: f64) {
        if now.is_finite() {
            let changed_second = self.now_override.map(f64::floor) != Some(now.floor());
            self.now_override = Some(now);
            if changed_second {
                self.invalidate_frame_axis();
            }
        }
    }

    /// Install (or clear) the host crosshair time formatter (reference `localization.timeFormatter`).
    pub fn set_time_formatter(&mut self, f: Option<TimeFormatterFn>) {
        self.time_formatter_fn = f;
        self.invalidate_frame_overlay();
    }

    /// reference `localization.dateFormat` (default `dd MMM \'yy`): the pattern driving the
    /// crosshair time label. Ignored while a host `timeFormatter` is installed (reference parity).
    pub fn set_date_format(&mut self, pattern: &str) {
        self.date_format = pattern.to_string();
        self.invalidate_frame_overlay();
    }

    /// Inject per-locale month-name tables (reference `localization.locale`): the 12 short and 12
    /// long month names used by the date-format `MMM`/`MMMM` tokens and the month tick
    /// labels. The engine stays headless — hosts derive the names (the wasm host uses
    /// `Intl.DateTimeFormat`); the default is English.
    pub fn set_month_names(&mut self, short: [String; 12], long: [String; 12]) {
        self.month_names = MonthNames { short, long };
        self.invalidate_frame_scene();
    }

    /// Add a series to the headless chart. The returned id is stable for the instance lifetime.
    pub fn add_series(&mut self, kind: SeriesKind) -> SeriesId {
        let id = self.data.add_series();
        let slot = self
            .data
            .series_slot(id)
            .expect("newly allocated series must have a storage slot");
        if slot == self.series.len() {
            self.series.push(SeriesEntry::new(id, kind));
        } else {
            self.series[slot] = SeriesEntry::new(id, kind);
        }
        // new series paint on top (reference appends to the pane's data sources)
        self.series_order.push(id);
        // A custom series' time-only rows still count as data rows for the base index.
        self.data
            .set_rows_count_as_data(id, kind == SeriesKind::Custom);
        self.invalidate_frame_scene();
        id
    }

    /// Change a live series' kind (host `setSeriesType` and custom-series adoption, Phase C-c).
    /// Keeps the data layer's custom-series bookkeeping in sync: a Custom series' rows carry
    /// times only, yet still count as data rows for the time-scale base index.
    pub fn convert_series_kind(&mut self, id: SeriesId, kind: SeriesKind) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id && !s.removed) {
            s.kind = kind;
            self.data
                .set_rows_count_as_data(id, kind == SeriesKind::Custom);
            self.invalidate_frame_scene();
        }
    }

    /// Record a custom series' frame values (Phase C-c; hosts refresh them per frame, before
    /// the layout/autoscale passes consume them). Ignored for an unknown, removed, or
    /// non-custom id.
    pub fn set_custom_frame_values(&mut self, id: SeriesId, values: CustomSeriesFrameValues) {
        if let Some(s) = self
            .series
            .iter_mut()
            .find(|s| s.id == id && !s.removed && s.kind == SeriesKind::Custom)
        {
            s.custom_frame = values;
            self.invalidate_frame_scene();
        }
    }

    /// Remove a series (reference `removeSeries`, which accepts any series including the first).
    /// Returns false for an unknown or already-removed id. Any indicators bound to (or
    /// derived from) the series are dropped with it. Consumers that anchor on the "primary"
    /// series (crosshair defaults, the volume up/down reference, the last-price pulse, the
    /// wasm coordinate API) fall back to the first visible non-removed series.
    ///
    /// Removal permanently invalidates the opaque identity and releases its backing slot for a
    /// future series. A stale identity can therefore never alias the replacement series.
    pub fn remove_series(&mut self, id: SeriesId) -> bool {
        !self.remove_series_tracked(id).is_empty()
    }

    /// `remove_series` that reports every tombstoned id: the series itself plus any indicator
    /// output series dropped with it (empty when the id is unknown or already removed). Hosts
    /// use the report to fire per-series removal events and drop their own per-series state.
    pub fn remove_series_tracked(&mut self, id: SeriesId) -> Vec<SeriesId> {
        if !self.series.iter().any(|s| s.id == id && !s.removed) {
            return Vec::new();
        }
        // The pane losing the series may collapse afterwards (reference `_cleanupIfPaneIsEmpty`).
        let home_pane = self
            .series
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.pane_index);
        // Drop indicator bindings touching this series and collect their output series to tombstone
        // alongside it (a removed source leaves no derived data behind).
        let mut tombstones = self.drop_indicators_touching(id);
        if !tombstones.contains(&id) {
            tombstones.push(id);
        }
        for rid in &tombstones {
            let rid = *rid;
            if let Some(entry) = self.series.iter_mut().find(|s| s.id == rid) {
                entry.removed = true;
                entry.visible = false;
                entry.price_lines.clear();
                entry.markers.clear();
            }
            // Release the data slot; its opaque identity is invalid forever and the storage may
            // be reused by a different identity.
            let removed = self.data.remove_series(rid);
            debug_assert!(removed, "tracked live series must own a data slot");
        }
        self.series_order.retain(|sid| !tombstones.contains(sid));
        // A hovered series leaving the chart releases the hovered-on-top z-bump with it.
        if self
            .hovered_series
            .is_some_and(|hovered| tombstones.contains(&hovered))
        {
            self.hovered_series = None;
        }
        // A selected series leaving the chart drops its anchor points with it.
        if self
            .selected_series
            .is_some_and(|selected| tombstones.contains(&selected))
        {
            self.selected_series = None;
        }
        self.sync_time_points();
        // reference chart-model.ts `removeSeries`: prune the pane the series left when it is empty
        // and not preserved (a pane-less index — after an explicit `remove_pane` — prunes
        // nothing).
        if let Some(pane_index) = home_pane {
            self.cleanup_if_pane_is_empty(pane_index);
        }
        tombstones
    }

    /// Record a series primitive's autoscale contribution for the next autoscale pass (plugin
    /// platform Phase C-b; reference `ISeriesPrimitiveBase.autoscaleInfo`). Non-finite bounds are
    /// rejected here so a misbehaving plugin cannot poison the scale range.
    pub fn add_autoscale_contribution(&mut self, contribution: PrimitiveAutoscaleContribution) {
        if !contribution.min.is_finite() || !contribution.max.is_finite() {
            return;
        }
        self.primitive_autoscale.push(contribution);
        self.invalidate_frame_scene();
    }

    /// Drop all recorded series-primitive autoscale contributions. Hosts call this at frame
    /// build start, before re-collecting the current frame's contributions.
    pub fn clear_autoscale_contributions(&mut self) {
        if !self.primitive_autoscale.is_empty() {
            self.primitive_autoscale.clear();
            self.invalidate_frame_scene();
        }
    }

    /// reference chart-api.ts `addPane(preserveEmptyPane)` → chart-model.ts `_addPane`: append a
    /// pane and return its index. The new pane's scales inherit the chart-level
    /// `leftPriceScale`/`rightPriceScale` cosmetics, exactly like the reference's `Pane` constructor.
    pub fn add_pane(&mut self, preserve_empty: bool) -> usize {
        let mut pane = Pane::with_stable_id(self.take_pane_id());
        pane.preserve_empty = preserve_empty;
        self.apply_chart_scale_options(&mut pane);
        self.panes.push(pane);
        self.invalidate_frame_all();
        self.panes.len() - 1
    }

    fn take_pane_id(&mut self) -> u32 {
        let id = self.next_pane_id;
        self.next_pane_id = self
            .next_pane_id
            .checked_add(1)
            .expect("pane identity space exhausted");
        id
    }

    /// Stable chart-local identity for the pane currently at `index`.
    pub fn pane_stable_id(&self, index: usize) -> Option<u32> {
        self.panes.get(index).map(Pane::stable_id)
    }

    /// Current index of a live pane identity. Removed pane identities never resolve again.
    pub fn pane_index_for_id(&self, stable_id: u32) -> Option<usize> {
        self.panes
            .iter()
            .position(|pane| pane.stable_id() == stable_id)
    }

    /// reference chart-model.ts `removePane`: refuses the last remaining pane and out-of-range
    /// indices (false). The removed pane's series are NOT moved or removed — they become
    /// pane-less (reference leaves them with `paneForSource` → null): they keep their data but
    /// render and scale nowhere until re-assigned. Series below shift one pane up.
    pub fn remove_pane(&mut self, index: usize) -> bool {
        if self.panes.len() <= 1 || index >= self.panes.len() {
            return false;
        }
        self.panes.remove(index);
        for s in &mut self.series {
            if s.pane_index == index {
                s.pane_index = PANELESS;
            } else if s.pane_index != PANELESS && s.pane_index > index {
                s.pane_index -= 1;
            }
        }
        self.invalidate_frame_all();
        true
    }

    /// reference chart-model.ts `swapPanes`: the two panes trade places; their series assignments,
    /// stretch factors, scales, and preserve flags ride along with them.
    pub fn swap_panes(&mut self, first: usize, second: usize) -> bool {
        if first >= self.panes.len() || second >= self.panes.len() {
            return false;
        }
        self.panes.swap(first, second);
        for s in &mut self.series {
            if s.pane_index == first {
                s.pane_index = second;
            } else if s.pane_index == second {
                s.pane_index = first;
            }
        }
        self.invalidate_frame_all();
        true
    }

    /// reference chart-model.ts `movePane` (pane-api.ts `moveTo`): relocate the pane to a new index
    /// with its series; the panes in between shift one slot.
    pub fn move_pane(&mut self, from: usize, to: usize) -> bool {
        if from >= self.panes.len() || to >= self.panes.len() {
            return false;
        }
        if from == to {
            return true;
        }
        let pane = self.panes.remove(from);
        self.panes.insert(to, pane);
        for s in &mut self.series {
            let p = s.pane_index;
            if p == PANELESS {
                continue;
            }
            s.pane_index = if p == from {
                to
            } else if from < to && p > from && p <= to {
                p - 1
            } else if to < from && p >= to && p < from {
                p + 1
            } else {
                p
            };
        }
        self.invalidate_frame_all();
        true
    }

    /// reference pane-api.ts `preserveEmptyPane()` (false for a stale index).
    pub fn pane_preserve_empty(&self, index: usize) -> bool {
        self.panes
            .get(index)
            .map(|p| p.preserve_empty)
            .unwrap_or(false)
    }

    /// reference pane-api.ts `setPreserveEmptyPane(preserve)` (ignored for a stale index).
    pub fn pane_set_preserve_empty(&mut self, index: usize, flag: bool) {
        if let Some(pane) = self.panes.get_mut(index) {
            pane.preserve_empty = flag;
        }
    }

    /// reference pane-api.ts `getSeries()`: the pane's live series in render order (bottom first,
    /// matching the chart z-order). Empty for a stale index.
    pub fn pane_series_ids(&self, index: usize) -> Vec<SeriesId> {
        self.series_order
            .iter()
            .copied()
            .filter(|&id| {
                self.series_entry(id)
                    .is_some_and(|series| series.pane_index == index)
            })
            .collect()
    }

    /// Move a series into pane `pane_index`, creating panes (with the given stretch factor
    /// for a newly-created pane) as needed — reference `moveSeriesToPane` with `_getOrCreatePane`.
    /// The pane the series left collapses when empty and not preserved (reference
    /// `_cleanupIfPaneIsEmpty`, chart-model.ts:1135).
    pub fn set_series_pane(&mut self, id: SeriesId, pane_index: usize, stretch_factor: f64) {
        if self.series_entry(id).is_none() {
            return;
        }
        while self.panes.len() <= pane_index {
            let mut pane = Pane::with_stable_id(self.take_pane_id());
            pane.stretch_factor = stretch_factor.max(0.01);
            self.apply_chart_scale_options(&mut pane);
            self.panes.push(pane);
        }
        let Some(series) = self.series.iter_mut().find(|s| s.id == id && !s.removed) else {
            return;
        };
        let from = series.pane_index;
        if from == pane_index {
            return;
        }
        series.pane_index = pane_index;
        if from != PANELESS {
            self.cleanup_if_pane_is_empty(from);
        }
        self.invalidate_frame_all();
    }

    /// Port of reference chart-model.ts `_cleanupIfPaneIsEmpty`: a pane left without any live
    /// series collapses unless it is preserved or the last remaining pane. Series below
    /// shift one pane up. Returns true when the pane was removed.
    fn cleanup_if_pane_is_empty(&mut self, pane_index: usize) -> bool {
        if pane_index >= self.panes.len() || self.panes.len() <= 1 {
            return false;
        }
        if self.panes[pane_index].preserve_empty {
            return false;
        }
        // reference checks `pane.dataSources().length === 0`: hidden series still occupy their
        // pane; removed (tombstoned) ones are detached from it.
        if self
            .series
            .iter()
            .any(|s| !s.removed && s.pane_index == pane_index)
        {
            return false;
        }
        self.panes.remove(pane_index);
        for s in &mut self.series {
            if s.pane_index != PANELESS && s.pane_index > pane_index {
                s.pane_index -= 1;
            }
        }
        true
    }

    /// Copy the chart-level `leftPriceScale`/`rightPriceScale` scale-held cosmetics onto a
    /// new pane's scales (reference pane.ts constructor `_createPriceScale` from the chart options).
    fn apply_chart_scale_options(&self, pane: &mut Pane) {
        let options = self.options.get();
        let apply = |scale: &mut PriceScaleCore,
                     group: &nucleuscharts_core::options::PriceAxisOptions| {
            scale.set_align_labels(group.align_labels);
            scale.set_ticks_visible(group.ticks_visible);
            scale.set_entire_text_only(group.entire_text_only);
            scale.set_minimum_width(group.minimum_width);
            scale.set_text_color(group.text_color.clone());
        };
        apply(&mut pane.left_scale, &options.left_price_scale);
        apply(&mut pane.price_scale, &options.right_price_scale);
    }

    /// Whether `id` names a tombstoned (removed) series. Data mutations on such a slot are ignored
    /// so a removed series can never be silently revived.
    pub fn is_series_removed(&self, id: SeriesId) -> bool {
        matches!(
            self.data.validate_series_id(id),
            Err(SeriesIdError::Stale(_))
        )
    }

    /// Validate a public series identity without touching chart state.
    pub fn validate_series_id(&self, id: SeriesId) -> Result<(), SeriesIdError> {
        self.data.validate_series_id(id)
    }

    /// Resolve a live opaque series identity to its current storage entry.
    pub(crate) fn series_entry(&self, id: SeriesId) -> Option<&SeriesEntry> {
        let slot = self.data.series_slot(id)?;
        self.series
            .get(slot)
            .filter(|series| series.id == id && !series.removed)
    }

    pub(crate) fn series_entry_mut(&mut self, id: SeriesId) -> Option<&mut SeriesEntry> {
        let slot = self.data.series_slot(id)?;
        self.series
            .get_mut(slot)
            .filter(|series| series.id == id && !series.removed)
    }

    /// Toggle a series without destroying its data or indicator binding. A removed slot can
    /// never be revived, so visibility changes on it are ignored.
    pub fn set_series_visible(&mut self, id: SeriesId, visible: bool) {
        if let Some(series) = self
            .series
            .iter_mut()
            .find(|series| series.id == id && !series.removed)
        {
            series.visible = visible;
            self.invalidate_frame_scene();
        }
    }

    /// The effective primary series: the first visible, non-removed entry. reference lets any
    /// series be removed (`removeSeries`), so every "first series" anchor resolves through
    /// this fallback instead of assuming id 0 is alive.
    pub(crate) fn primary_series(&self) -> Option<&SeriesEntry> {
        self.series.iter().find(|s| !s.removed && s.visible)
    }

    /// Series ids in current render order (bottom to top; topmost LAST), live series only.
    pub fn series_order(&self) -> &[SeriesId] {
        &self.series_order
    }

    /// The render order as a JSON array of series ids (the reference's z-order; the last id paints on
    /// top). Backs the TS `chart.seriesOrder()`.
    pub fn series_order_json(&self) -> String {
        serde_json::to_string(&self.series_order).unwrap_or_else(|_| "[]".to_string())
    }

    /// reference `chart.setSeriesOrder`: reorder which series paints on top. The patch must name
    /// every live series id exactly once (a bad permutation — wrong length, duplicates,
    /// unknown or missing ids — is rejected with false and no state change).
    pub fn set_series_order(&mut self, ids: Vec<SeriesId>) -> bool {
        self.invalidate_frame_scene();
        if ids.len() != self.series_order.len() {
            return false;
        }
        let mut requested = ids.clone();
        requested.sort_unstable();
        let mut current = self.series_order.clone();
        current.sort_unstable();
        if requested != current {
            return false;
        }
        self.series_order = ids;
        true
    }

    /// Remove the last `count` data points of a series (reference v5.2 `ISeriesApi.pop`,
    /// iseries-api.ts:203): `count` 0 is a no-op, larger counts clamp to the data length.
    /// Per-point color channels truncate with their rows. Returns the new data length, or
    /// `None` for an unknown/removed id.
    pub fn series_pop(&mut self, id: SeriesId, count: usize) -> Option<usize> {
        self.invalidate_frame_series(id);
        if self.is_series_removed(id) || !self.series.iter().any(|s| s.id == id) {
            return None;
        }
        let previous_generation = self.data.series_generation(id).unwrap_or(0);
        let len = self.data.pop(id, count)?;
        self.sync_time_points();
        self.update_indicators_after_change(
            id,
            IndicatorChange {
                from: len,
                previous_generation,
                full_replace: true,
            },
        );
        Some(len)
    }

    pub fn set_series_markers(&mut self, id: SeriesId, markers: Vec<Marker>) {
        self.invalidate_frame_scene();
        if let Some(series) = self.series_entry_mut(id) {
            series.markers = markers;
        }
    }

    pub fn set_series_markers_auto_scale(&mut self, id: SeriesId, enabled: bool) {
        self.invalidate_frame_scene();
        if let Some(series) = self.series_entry_mut(id) {
            series.markers_auto_scale = enabled;
        }
    }

    /// Apply one streaming OHLC update after validating its time and values.
    pub fn update_series_bar(&mut self, id: SeriesId, time: f64, values: [f64; 4]) -> bool {
        self.invalidate_frame_series(id);
        self.update_series_bar_styled(id, time, values, [None; 3])
    }

    /// Apply ordered streaming rows without coordinator allocation, then synchronize shared chart
    /// state and dependent indicators once. Used by the bounded shared-ring drain; each valid row
    /// keeps the single-update append/replace semantics.
    pub fn update_series_bars<I>(&mut self, id: SeriesId, rows: I) -> usize
    where
        I: IntoIterator<Item = (f64, [f64; 4])>,
    {
        self.invalidate_frame_series(id);
        if self.validate_series_id(id).is_err() {
            return 0;
        }
        let previous_generation = self.data.series_generation(id).unwrap_or(0);
        let mut from = usize::MAX;
        let mut accepted = 0;
        for (time, values) in rows {
            let Some((time, values)) = sanitize_point(time, values) else {
                continue;
            };
            let row = self
                .data
                .series_data(id)
                .map(|(times, _)| {
                    times
                        .binary_search(&time)
                        .unwrap_or_else(|position| position)
                })
                .unwrap_or_default();
            from = from.min(row);
            self.data.update_styled(id, time, values, [None; 3]);
            accepted += 1;
        }
        if accepted == 0 {
            return 0;
        }
        let trimmed = self.enforce_series_cap(id);
        self.sync_time_points();
        self.update_indicators_after_change(
            id,
            IndicatorChange {
                from: if trimmed { 0 } else { from },
                previous_generation,
                full_replace: trimmed,
            },
        );
        accepted
    }

    /// Install an already sanitized ascending/unique batch. This is the WASM typed-array fast
    /// path: the data layer merges once, time state synchronizes once, and each dependent
    /// indicator advances once from the earliest affected row.
    pub fn update_series_bars_sanitized(
        &mut self,
        id: SeriesId,
        times: Vec<i64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
    ) -> usize {
        self.invalidate_frame_series(id);
        if self.validate_series_id(id).is_err() || times.is_empty() {
            return 0;
        }
        let previous_generation = self.data.series_generation(id).unwrap_or(0);
        let Some(from) = self
            .data
            .update_many(id, &times, [&open, &high, &low, &close])
        else {
            return 0;
        };
        let accepted = times.len();
        let trimmed = self.enforce_series_cap(id);
        self.sync_time_points();
        self.update_indicators_after_change(
            id,
            IndicatorChange {
                from,
                previous_generation,
                full_replace: trimmed,
            },
        );
        accepted
    }

    /// [`update_series_bar`] plus the target bar's per-point color channels (reference
    /// `series.update` with data-item colors; `None` = no custom color for that channel).
    /// Mirrors the plain update's semantics exactly: append-new-time vs replace-last.
    pub fn update_series_bar_styled(
        &mut self,
        id: SeriesId,
        time: f64,
        values: [f64; 4],
        colors: [Option<u32>; 3],
    ) -> bool {
        self.invalidate_frame_series(id);
        if self.validate_series_id(id).is_err() {
            return false;
        }
        let Some((time, values)) = sanitize_point(time, values) else {
            return false;
        };
        let from = self
            .data
            .series_data(id)
            .map(|(times, _)| {
                times
                    .binary_search(&time)
                    .unwrap_or_else(|position| position)
            })
            .unwrap_or_default();
        let previous_generation = self.data.series_generation(id).unwrap_or(0);
        self.data.update_styled(id, time, values, colors);
        // Retention (`max_points`): usually a no-op flag check, and an O(total) trim once every
        // `margin` appends. Runs before `sync_time_points` so the scale sees the final row set.
        if self.enforce_series_cap(id) {
            self.sync_time_points();
            self.update_indicators_after_change(
                id,
                IndicatorChange {
                    from: 0,
                    previous_generation,
                    full_replace: true,
                },
            );
            return true;
        }
        self.sync_time_points();
        self.update_indicators_after_change(
            id,
            IndicatorChange {
                from,
                previous_generation,
                full_replace: false,
            },
        );
        true
    }

    /// Install per-row color overrides for a series (reference data-item colors, packed RGBA
    /// `0xRRGGBBAA`). Channels: body = candle/bar body, line/area stroke + point marker,
    /// histogram column; wick/border = candlestick parts. Each channel is `None`/empty for
    /// absent, or must match the series' row count exactly — a mismatch rejects the whole call
    /// (false, no partial state). `set_series_data`/`install_series_data` reset these colors,
    /// so hosts install them right after setting data.
    pub fn set_series_point_colors(
        &mut self,
        id: SeriesId,
        body: Option<Vec<u32>>,
        wick: Option<Vec<u32>>,
        border: Option<Vec<u32>>,
    ) -> bool {
        if self.validate_series_id(id).is_err() {
            return false;
        }
        let changed = self.data.set_point_colors(id, [body, wick, border]);
        if changed {
            self.invalidate_frame_series(id);
        }
        changed
    }

    /// Full (re)assignment with per-row color channels run through the same repair pipeline as
    /// the OHLC columns: the colors follow their row through invalid-row drops and the stable
    /// sort, and the last-wins dedupe keeps the winning row's channels.
    #[allow(clippy::too_many_arguments)] // mirrors set_series_data plus the three reference color slots
    pub fn set_series_data_styled(
        &mut self,
        id: SeriesId,
        times: &[f64],
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        colors: [Option<Vec<u32>>; 3],
    ) -> Result<ValidationReport, ValidationError> {
        self.invalidate_frame_series(id);
        self.validate_series_id(id).map_err(|error| match error {
            SeriesIdError::Unknown(id) => ValidationError::UnknownSeries(id),
            SeriesIdError::Stale(id) => ValidationError::StaleSeries(id),
        })?;
        let s = sanitize_ohlc_styled(times, open, high, low, close, colors)?;
        let report = s.data.report.clone();
        self.install_series_columns(
            id,
            s.data.times,
            s.data.open,
            s.data.high,
            s.data.low,
            s.data.close,
        );
        let [body, wick, border] = s.colors;
        let installed = self
            .data
            .set_point_colors(id, [Some(body), Some(wick), Some(border)]);
        debug_assert!(installed, "sanitized channels are aligned by construction");
        // Retention (`max_points`): trim after the colors land so the eviction shifts rows and
        // color channels together.
        self.enforce_series_cap(id);
        self.sync_time_points();
        self.recompute_indicators_for(id);
        Ok(report)
    }

    /// Validate and install one series' parallel OHLC columns without involving a host runtime.
    /// The returned report lets browser, native, and server callers expose identical diagnostics.
    pub fn set_series_data(
        &mut self,
        id: SeriesId,
        times: &[f64],
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
    ) -> Result<ValidationReport, ValidationError> {
        self.invalidate_frame_series(id);
        // A removed slot must stay empty; ignore the data (the TS series handle rejects the call
        // before it reaches here, so this is defense-in-depth) and report a clean no-op.
        self.validate_series_id(id).map_err(|error| match error {
            SeriesIdError::Unknown(id) => ValidationError::UnknownSeries(id),
            SeriesIdError::Stale(id) => ValidationError::StaleSeries(id),
        })?;
        let sanitized = sanitize_ohlc(times, open, high, low, close)?;
        let report = sanitized.report.clone();
        self.install_series_columns(
            id,
            sanitized.times,
            sanitized.open,
            sanitized.high,
            sanitized.low,
            sanitized.close,
        );
        // Retention (`max_points`): a full install can exceed the ceiling; trim before the scale
        // and the indicators index the rows. The report still describes the caller's input.
        self.enforce_series_cap(id);
        self.sync_time_points();
        self.recompute_indicators_for(id);
        Ok(report)
    }

    /// Install columns that have already crossed the validation boundary (used by adapters that
    /// need to report the sanitization details before handing ownership to the engine).
    pub fn install_series_data(
        &mut self,
        id: SeriesId,
        times: Vec<i64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
    ) -> bool {
        self.invalidate_frame_series(id);
        if self.validate_series_id(id).is_err() {
            return false;
        }
        if !self.install_series_columns(id, times, open, high, low, close) {
            return false;
        }
        // A full install can land more rows than the retention ceiling allows; trim before the
        // scale and the indicators see the row set, so nothing downstream indexes evicted rows.
        self.enforce_series_cap(id);
        self.sync_time_points();
        self.recompute_indicators_for(id);
        true
    }

    fn install_series_columns(
        &mut self,
        id: SeriesId,
        times: Vec<i64>,
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
    ) -> bool {
        let scalar = self
            .series_entry(id)
            .is_some_and(|series| series.kind.stores_scalar_values())
            && open == high
            && open == low
            && open == close;
        if scalar {
            self.data.set_single_data(id, times, close)
        } else {
            self.data.set_data(id, times, open, high, low, close)
        }
    }

    /// Set a series' retention ceiling: at most `max_points` rows, oldest evicted first. `None`
    /// restores the default unbounded behavior. Applied immediately to the series' current rows.
    ///
    /// **Eviction schedule.** Trimming is `O(total rows)` (a row shift plus a rebuild of the shared
    /// time axis), so evicting on every append would make a long streaming session quadratic.
    /// Instead the engine trims with hysteresis: once the row count exceeds `max_points` it drops
    /// back to `max_points - margin`, where `margin` is [`CAP_TRIM_MARGIN_DIVISOR`]-th of the cap.
    /// `max_points` is therefore a **hard ceiling** — the series never holds more — while the
    /// floor right after a trim is `max_points - margin`. Amortized cost per appended point is
    /// constant.
    ///
    /// An unknown or removed id is ignored.
    pub fn set_series_max_points(&mut self, id: SeriesId, max_points: Option<usize>) -> bool {
        if self.is_series_removed(id) {
            return false;
        }
        let Some(entry) = self.series_entry_mut(id) else {
            return false;
        };
        entry.max_points = max_points;
        if self.enforce_series_cap(id) {
            self.sync_time_points();
            self.recompute_indicators_for(id);
        }
        true
    }

    /// This series' retention ceiling (`None` = unbounded).
    pub fn series_max_points(&self, id: SeriesId) -> Option<usize> {
        self.series_entry(id).and_then(|series| series.max_points)
    }

    /// Evict oldest rows if the series is over its ceiling. Returns whether anything was dropped,
    /// so callers can skip the follow-up scale/indicator sync in the overwhelmingly common case
    /// where nothing needed evicting.
    fn enforce_series_cap(&mut self, id: SeriesId) -> bool {
        let Some(max_points) = self
            .series
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.max_points)
        else {
            return false;
        };
        let rows = self
            .data
            .series_data(id)
            .map_or(0, |(times, _)| times.len());
        if rows <= max_points {
            return false;
        }
        // Trim past the ceiling by the hysteresis margin so the next `margin` appends are free.
        // A cap of 0 means "hold nothing"; guard the divisor rather than special-casing it.
        let margin = (max_points / CAP_TRIM_MARGIN_DIVISOR).min(max_points);
        self.data.trim_front(id, max_points - margin);
        true
    }

    /// Fit the horizontal scale to the current union of series timestamps.
    pub fn fit_content(&mut self) {
        self.time_scale.fit_content();
        self.invalidate_frame_scene();
    }

    /// Apply the public horizontal-scale spacing while keeping ownership in the headless model.
    pub fn set_bar_spacing(&mut self, spacing: f64) {
        if spacing.is_finite() && spacing > 0.0 {
            self.time_scale.set_bar_spacing(spacing);
            self.invalidate_frame_scene();
        }
    }

    /// Apply the public horizontal-scale right offset in logical bars.
    pub fn set_right_offset(&mut self, offset: f64) {
        if offset.is_finite() {
            self.time_scale.set_right_offset(offset);
            self.invalidate_frame_scene();
        }
    }

    /// reference `timeScale.timeVisible`: show the time of day in axis/crosshair labels.
    pub fn set_time_visible(&mut self, visible: bool) {
        self.time_visible = visible;
        self.invalidate_frame_scene();
    }

    /// reference `timeScale.visible`: reserve/collapse the whole time-axis strip. Distinct from
    /// [`Self::set_time_visible`], which only governs label content (reference
    /// time-scale-options-defaults.ts keeps the two flags separate).
    pub fn set_time_axis_visible(&mut self, visible: bool) {
        self.time_axis_visible = visible;
        self.invalidate_frame_all();
    }

    /// reference `timeScale.ticksVisible`: tick marks beside the time-axis labels.
    pub fn set_time_ticks_visible(&mut self, visible: bool) {
        self.time_ticks_visible = visible;
        self.invalidate_frame_scene();
    }

    /// reference `timeScale.minimumHeight` (CSS px; non-negative, finite): floor for the strip
    /// height — chart-widget.ts `Math.max(optimalHeight(), minimumHeight)`.
    pub fn set_time_axis_minimum_height(&mut self, height: f64) {
        if height.is_finite() && height >= 0.0 {
            self.time_axis_minimum_height = height;
            self.invalidate_frame_all();
        }
    }

    /// reference `timeScale.tickMarkMaxCharacterLength`: 0 restores the default 8, matching the reference's
    /// `tickMarkMaxCharacterLength || defaultTickMarkMaxCharacterLength` (time-scale.ts:635).
    pub fn set_tick_mark_max_character_length(&mut self, n: u32) {
        self.tick_mark_max_character_length = if n == 0 { 8 } else { n };
        self.invalidate_frame_scene();
    }

    /// The reserved time-axis strip height in media px (reference chart-widget.ts
    /// `_adjustSizeImpl`): zero when the strip is hidden, else the auto height floored at
    /// `timeScale.minimumHeight`. Hosts subtract this from the chart height for the pane
    /// content area and report it from their `time_scale_height()` gestures getter.
    pub fn time_axis_height(&self) -> f64 {
        if self.time_axis_visible {
            TIME_AXIS_HEIGHT.max(self.time_axis_minimum_height)
        } else {
            0.0
        }
    }

    /// Set/clear the hovered pane separator (reference pane-separator.ts hover handle). Mirrored
    /// into the next axis frame; hosts repaint to show the band.
    pub fn set_separator_hover(&mut self, index: Option<usize>) {
        self.separator_hover = index;
        self.invalidate_frame_overlay();
    }

    /// Drag the separator below pane `index` by `delta_css` logical pixels. Positive deltas grow
    /// the pane above and shrink the pane below. Current heights become stretch factors first so
    /// unaffected panes hold their size; both adjacent panes retain the reference 24px minimum.
    pub fn drag_pane_separator(&mut self, index: usize, delta_css: f64) {
        if index + 1 >= self.panes.len() || !delta_css.is_finite() {
            return;
        }
        const MIN_PANE_HEIGHT: f64 = 24.0;
        for pane in &mut self.panes {
            pane.stretch_factor = pane.height.max(1.0);
        }
        let top = self.panes[index].height;
        let bottom = self.panes[index + 1].height;
        let combined = top + bottom;
        let new_top = (top + delta_css).clamp(
            MIN_PANE_HEIGHT,
            (combined - MIN_PANE_HEIGHT).max(MIN_PANE_HEIGHT),
        );
        let applied = new_top - top;
        self.panes[index].stretch_factor = new_top;
        self.panes[index + 1].stretch_factor = bottom - applied;
        self.invalidate_frame_all();
    }

    /// reference `timeScale.secondsVisible`: include seconds when `time_visible` is set.
    pub fn set_seconds_visible(&mut self, visible: bool) {
        self.seconds_visible = visible;
        self.invalidate_frame_scene();
    }

    /// TradingView-style bid/ask: push the current quotes for a series. `None` hides that
    /// side. Lines and chips render only while the series' `bid_ask_visible` option holds.
    pub fn set_bid_ask(&mut self, id: SeriesId, bid: Option<f64>, ask: Option<f64>) {
        if let Some(series) = self.series.iter_mut().find(|s| s.id == id && !s.removed) {
            series.bid = bid.filter(|v| v.is_finite());
            series.ask = ask.filter(|v| v.is_finite());
            self.invalidate_frame_scene();
        }
    }

    /// reference `timeScale.minBarSpacing`.
    pub fn set_min_bar_spacing(&mut self, spacing: f64) {
        self.time_scale.set_min_bar_spacing(spacing);
        self.invalidate_frame_scene();
    }

    /// reference `timeScale.maxBarSpacing` (CSS px; 0 restores the default half-width cap).
    pub fn set_max_bar_spacing(&mut self, spacing: f64) {
        self.time_scale.set_max_bar_spacing(spacing);
        self.invalidate_frame_scene();
    }

    /// reference `timeScale().applyOptions({ barSpacing })`: write the option and apply it live.
    pub fn apply_bar_spacing_option(&mut self, spacing: f64) {
        self.time_scale.apply_bar_spacing_option(spacing);
        self.invalidate_frame_scene();
    }

    /// reference `timeScale().applyOptions({ rightOffset })`: write the option and apply it live.
    pub fn apply_right_offset_option(&mut self, offset: f64) {
        self.time_scale.apply_right_offset_option(offset);
        self.invalidate_frame_scene();
    }

    /// reference `timeScale.rightOffsetPixels`: pin the right offset in pixels (converted to bars
    /// through the current bar spacing, then preserved across zoom).
    pub fn set_right_offset_pixels(&mut self, pixels: f64) {
        self.time_scale.set_right_offset_pixels(pixels);
        self.invalidate_frame_scene();
    }

    /// reference `timeScale.fixLeftEdge`.
    pub fn set_fix_left_edge(&mut self, fix: bool) {
        self.time_scale.set_fix_left_edge(fix);
    }

    /// reference `timeScale.fixRightEdge`.
    pub fn set_fix_right_edge(&mut self, fix: bool) {
        self.time_scale.set_fix_right_edge(fix);
    }

    /// reference `timeScale.lockVisibleTimeRangeOnResize`.
    pub fn set_lock_visible_time_range_on_resize(&mut self, lock: bool) {
        self.time_scale.set_lock_visible_time_range_on_resize(lock);
    }

    /// reference `timeScale.rightBarStaysOnScroll`.
    pub fn set_right_bar_stays_on_scroll(&mut self, stays: bool) {
        self.time_scale.set_right_bar_stays_on_scroll(stays);
    }

    /// reference `timeScale.shiftVisibleRangeOnNewBar` (default true): when the last bar is
    /// visible, the visible range follows newly appended bars instead of compensating the
    /// right offset (chart-model.ts:968-983).
    pub fn set_shift_visible_range_on_new_bar(&mut self, shift: bool) {
        self.time_scale.set_shift_visible_range_on_new_bar(shift);
    }

    /// reference `timeScale.allowShiftVisibleRangeOnWhitespaceReplacement` (default false): also
    /// shift when the new bar replaces an existing whitespace time point.
    pub fn set_allow_shift_visible_range_on_whitespace_replacement(&mut self, allow: bool) {
        self.time_scale
            .set_allow_shift_visible_range_on_whitespace_replacement(allow);
    }

    /// reference `timeScale.allowBoldLabels` (default true): bold the major time tick labels.
    pub fn set_allow_bold_labels(&mut self, allow: bool) {
        self.time_scale.set_allow_bold_labels(allow);
    }

    /// reference `chart.setCrosshairPosition(price, time, series)` (chart-model.ts
    /// `setAndSaveSyntheticPosition`): position the crosshair at a data point without a DOM
    /// event. The time must land exactly on a merged time point (false otherwise); x is that
    /// bar's coordinate and y the price converted through the given series' price scale.
    /// Works headless — the next built frame draws it; hosts emit their crosshair event.
    pub fn set_crosshair_position(&mut self, price: f64, time: f64, series_id: SeriesId) -> bool {
        if !price.is_finite() || self.is_series_removed(series_id) {
            return false;
        }
        if !self.series.iter().any(|s| s.id == series_id) {
            return false;
        }
        // Bars live at integer-second times (ingestion truncates), so a fractional time can
        // never resolve to a bar — reject it rather than truncating onto one (reference compares
        // exact time keys and reports no match).
        if !time.is_finite() || time.fract() != 0.0 {
            return false;
        }
        let Some(index) = self.time_to_index(time, false) else {
            return false;
        };
        let Some(y) = self.series_price_to_coordinate(series_id, price) else {
            return false;
        };
        let x = self.time_scale.index_to_coordinate(index);
        self.crosshair = Some((x, y));
        self.invalidate_frame_overlay();
        true
    }

    /// reference `chart.clearCrosshairPosition`. The engine keeps a single stored position — the
    /// reference offset coords *are* the position here (a synthetic set saves (x, y) and the frame
    /// builder re-derives the snapped index from that stored x, exactly like the reference's
    /// `updateCrosshair` re-deriving from the saved offset) — so clearing it leaves nothing
    /// a scale change could resurrect.
    pub fn clear_crosshair_position(&mut self) {
        self.clear_crosshair_at();
    }

    /// Host-pushed "all scaling and scrolling disabled" aggregate (reference
    /// `_isAllScalingAndScrollingDisabled`, time-scale.ts:975-986): label alignment only in the
    /// reference — the scale math keeps reading the raw fix-edge options, so a non-interactive
    /// chart never reacts to resizes.
    pub fn set_interaction_disabled(&mut self, disabled: bool) {
        self.time_scale.set_interaction_disabled(disabled);
    }

    pub fn bar_spacing(&self) -> f64 {
        self.time_scale.bar_spacing()
    }

    pub fn right_offset(&self) -> f64 {
        self.time_scale.right_offset()
    }

    /// Current distance, in logical bars, from the latest data point to the right edge.
    pub fn scroll_position(&self) -> f64 {
        self.time_scale.right_offset()
    }

    /// Move the latest data point to `position` logical bars from the right edge. Animation is a
    /// host scheduling concern; this headless operation applies the target state immediately.
    pub fn scroll_to_position(&mut self, position: f64) {
        self.set_right_offset(position);
    }

    /// Restore the real-time edge. This intentionally targets zero rather than the configured
    /// default offset, matching the reference charting library's `scrollToRealTime` contract.
    pub fn scroll_to_real_time(&mut self) {
        self.time_scale.set_right_offset(0.0);
        self.invalidate_frame_scene();
    }

    /// Restore the configured default bar spacing and right offset.
    pub fn reset_time_scale(&mut self) {
        self.time_scale.restore_default();
        self.invalidate_frame_scene();
    }

    /// TradingView-style "reset view" button semantics in one action: the time scale returns
    /// to its configured defaults (reference `resetTimeScale`) AND every pane's price scales
    /// re-enable autoscale (reference pane `resetPriceScale`, the price-axis double-click).
    /// The next frame's autoscale pass recalculates the visible ranges, so a manually
    /// contracted or over-zoomed price scale fits the data again.
    pub fn reset_view(&mut self) {
        self.reset_time_scale();
        for pane in &mut self.panes {
            pane.price_scale.set_auto_scale(true);
            pane.left_scale.set_auto_scale(true);
        }
    }

    /// Deep-merge a JSON options patch into the chart options store (reference `applyOptions`
    /// semantics) and apply the runtime-affecting fields: the crosshair mode, plus any
    /// behavioral `timeScale` keys routed to the core scale through the same setters as the
    /// public time-scale API. Returns the parse error for a malformed patch.
    pub fn apply_options(&mut self, patch_json: &str) -> Result<(), serde_json::Error> {
        let patch: serde_json::Value = serde_json::from_str(patch_json)?;
        self.options.apply(&patch);
        // Re-derive runtime state that isn't read straight from the store each frame.
        self.crosshair_mode = crosshair_mode_from_u8(self.options.get().crosshair.mode);
        self.route_time_scale_patch(&patch);
        self.route_price_scale_patch(&patch);
        self.route_localization_patch(&patch);
        self.invalidate_frame_all();
        Ok(())
    }

    /// Switch all chart cosmetics using Nucleus's canonical style-token source.
    pub fn set_theme(&mut self, theme: ChartTheme) {
        let patch = chart_theme_patch(theme);
        self.options.apply(&patch);
        self.route_price_scale_patch(&patch);
        self.invalidate_frame_all();
    }

    /// Route the behavioral keys of a `timeScale` options patch to the core scale (reference
    /// `applyOptions({ timeScale })`, time-scale.ts:381-420). Only keys present in this patch
    /// are applied — the merged store is never re-read here, so an unrelated patch leaves the
    /// live scale state untouched.
    fn route_time_scale_patch(&mut self, patch: &serde_json::Value) {
        let Some(time_scale) = patch
            .get("timeScale")
            .and_then(serde_json::Value::as_object)
        else {
            return;
        };
        let number = |key: &str| time_scale.get(key).and_then(serde_json::Value::as_f64);
        let flag = |key: &str| time_scale.get(key).and_then(serde_json::Value::as_bool);
        // reference ordering (time-scale.ts:384-407): edge fixes first, then bar spacing, right
        // offset, and rightOffsetPixels (which converts through the just-applied spacing);
        // the spacing constraints come last since each re-corrects spacing and offset.
        if let Some(fix) = flag("fixLeftEdge") {
            self.set_fix_left_edge(fix);
        }
        if let Some(fix) = flag("fixRightEdge") {
            self.set_fix_right_edge(fix);
        }
        if let Some(spacing) = number("barSpacing") {
            self.apply_bar_spacing_option(spacing);
        }
        if let Some(offset) = number("rightOffset") {
            self.apply_right_offset_option(offset);
        }
        if let Some(pixels) = number("rightOffsetPixels") {
            self.set_right_offset_pixels(pixels);
        }
        if let Some(spacing) = number("minBarSpacing") {
            self.set_min_bar_spacing(spacing);
        }
        if let Some(spacing) = number("maxBarSpacing") {
            self.set_max_bar_spacing(spacing);
        }
        if let Some(visible) = flag("timeVisible") {
            self.set_time_visible(visible);
        }
        if let Some(visible) = flag("secondsVisible") {
            self.set_seconds_visible(visible);
        }
        // Strip cosmetics (reference timeScale options distinct from the label flags): `visible`
        // reserves the whole strip; the others shape its height and tick chrome.
        if let Some(visible) = flag("visible") {
            self.set_time_axis_visible(visible);
        }
        if let Some(visible) = flag("ticksVisible") {
            self.set_time_ticks_visible(visible);
        }
        if let Some(height) = number("minimumHeight") {
            self.set_time_axis_minimum_height(height);
        }
        if let Some(n) = time_scale
            .get("tickMarkMaxCharacterLength")
            .and_then(serde_json::Value::as_u64)
        {
            self.set_tick_mark_max_character_length(n.min(u32::MAX as u64) as u32);
        }
        if let Some(lock) = flag("lockVisibleTimeRangeOnResize") {
            self.set_lock_visible_time_range_on_resize(lock);
        }
        if let Some(stays) = flag("rightBarStaysOnScroll") {
            self.set_right_bar_stays_on_scroll(stays);
        }
        if let Some(shift) = flag("shiftVisibleRangeOnNewBar") {
            self.set_shift_visible_range_on_new_bar(shift);
        }
        if let Some(allow) = flag("allowShiftVisibleRangeOnWhitespaceReplacement") {
            self.set_allow_shift_visible_range_on_whitespace_replacement(allow);
        }
        if let Some(allow) = flag("allowBoldLabels") {
            self.set_allow_bold_labels(allow);
        }
    }

    /// Route a `localization` options patch: `dateFormat` drives the crosshair time label
    /// (reference chart-options-defaults.ts:34-37). `locale` is handled by hosts that can resolve
    /// month names (the wasm layer intercepts it before delegating here); the store keeps
    /// both keys for the options round-trip either way.
    /// Route the scale-held keys of a `leftPriceScale`/`rightPriceScale` patch to every
    /// pane's corresponding scale — reference pane.ts `applyScaleOptions` applies the chart-level
    /// groups to all panes. The strip cosmetics (`visible`, borders) stay in the options
    /// store and are read at render time. Only keys present in this patch are applied.
    fn route_price_scale_patch(&mut self, patch: &serde_json::Value) {
        for (group_key, target) in [
            ("leftPriceScale", PriceScaleTarget::Left),
            ("rightPriceScale", PriceScaleTarget::Right),
        ] {
            let Some(group) = patch.get(group_key).and_then(serde_json::Value::as_object) else {
                continue;
            };
            let flag = |key: &str| group.get(key).and_then(serde_json::Value::as_bool);
            for pane in &mut self.panes {
                let scale = match target {
                    PriceScaleTarget::Left => &mut pane.left_scale,
                    PriceScaleTarget::Right => &mut pane.price_scale,
                    PriceScaleTarget::Overlay => continue,
                };
                if let Some(align) = flag("alignLabels") {
                    scale.set_align_labels(align);
                }
                if let Some(visible) = flag("ticksVisible") {
                    scale.set_ticks_visible(visible);
                }
                if let Some(entire) = flag("entireTextOnly") {
                    scale.set_entire_text_only(entire);
                }
                if let Some(width) = group
                    .get("minimumWidth")
                    .and_then(serde_json::Value::as_f64)
                {
                    scale.set_minimum_width(width);
                }
                if let Some(value) = group.get("textColor") {
                    if value.is_null() {
                        scale.set_text_color(None);
                    } else if let Some(css) = value.as_str() {
                        scale.set_text_color((!css.is_empty()).then(|| css.to_string()));
                    }
                }
                if let Some(bold) = flag("boldRoundLabels") {
                    scale.set_bold_round_labels(bold);
                }
            }
        }
    }

    fn route_localization_patch(&mut self, patch: &serde_json::Value) {
        let Some(localization) = patch
            .get("localization")
            .and_then(serde_json::Value::as_object)
        else {
            return;
        };
        if let Some(pattern) = localization.get("dateFormat").and_then(|v| v.as_str()) {
            self.set_date_format(pattern);
        }
    }

    /// All time-scale options as a snake_case JSON object: the `TimeScaleOptions` fields from
    /// the core scale plus the engine-held `timeVisible`/`secondsVisible` label flags and the
    /// strip cosmetics (`visible`, `ticks_visible`, `minimum_height`,
    /// `tick_mark_max_character_length`). Backs
    /// the TS time-scale handle's `options()`. Values are the *configured* options (reference
    /// `timeScale().options()` semantics): `applyOptions` writes them, scroll/zoom gestures
    /// only move the live scale.
    pub fn time_scale_options_json(&self) -> String {
        let options = self.time_scale.options();
        serde_json::json!({
            "bar_spacing": options.bar_spacing,
            "right_offset": options.right_offset,
            "min_bar_spacing": options.min_bar_spacing,
            "max_bar_spacing": options.max_bar_spacing,
            "right_offset_pixels": options.right_offset_pixels,
            "time_visible": self.time_visible,
            "seconds_visible": self.seconds_visible,
            "visible": self.time_axis_visible,
            "ticks_visible": self.time_ticks_visible,
            "minimum_height": self.time_axis_minimum_height,
            "tick_mark_max_character_length": self.tick_mark_max_character_length,
            "fix_left_edge": options.fix_left_edge,
            "fix_right_edge": options.fix_right_edge,
            "lock_visible_time_range_on_resize": options.lock_visible_time_range_on_resize,
            "right_bar_stays_on_scroll": options.right_bar_stays_on_scroll,
            "shift_visible_range_on_new_bar": options.shift_visible_range_on_new_bar,
            "allow_shift_visible_range_on_whitespace_replacement": options.allow_shift_visible_range_on_whitespace_replacement,
            "allow_bold_labels": options.allow_bold_labels,
        })
        .to_string()
    }

    /// X coordinate for an integer logical index, or `None` when the scale has no points.
    pub fn logical_to_coordinate(&self, logical: f64) -> Option<f64> {
        if !logical.is_finite() || self.data.merged_times().is_empty() {
            return None;
        }
        // the reference's internal indexToCoordinate returns zero for non-integer runtime input. The public
        // Logical nominal type normally prevents this, but preserving it makes the JS boundary
        // deterministic for untyped callers too.
        if logical.fract() != 0.0 {
            return Some(0.0);
        }
        Some(
            self.time_scale
                .index_to_coordinate(logical as TimePointIndex),
        )
    }

    /// Integer logical bar owning an X coordinate. Values may extend outside the data.
    pub fn coordinate_to_logical(&self, x: f64) -> Option<f64> {
        if !x.is_finite() || self.data.merged_times().is_empty() {
            return None;
        }
        Some(self.time_scale.coordinate_to_index(x) as f64)
    }

    /// Logical index for a UTC-seconds timestamp. With `find_nearest`, select the first point at
    /// or after the timestamp and clamp timestamps beyond the last point to that final point,
    /// matching the reference's lower-bound behavior.
    pub fn time_to_index(&self, time: f64, find_nearest: bool) -> Option<TimePointIndex> {
        if !time.is_finite() {
            return None;
        }
        let times = self.data.merged_times();
        if times.is_empty() {
            return None;
        }
        let time = time as i64;
        let index = times.partition_point(|&point| point < time);
        if index < times.len() && times[index] == time {
            return Some(index as TimePointIndex);
        }
        if !find_nearest {
            return None;
        }
        Some(index.min(times.len() - 1) as TimePointIndex)
    }

    /// X coordinate for an exact UTC-seconds timestamp.
    pub fn time_to_coordinate(&self, time: f64) -> Option<f64> {
        let index = self.time_to_index(time, false)?;
        Some(self.time_scale.index_to_coordinate(index))
    }

    /// UTC-seconds timestamp at the rounded logical index under X.
    pub fn coordinate_to_time(&self, x: f64) -> Option<f64> {
        if !x.is_finite() {
            return None;
        }
        let times = self.data.merged_times();
        let index = self.time_scale.coordinate_to_index(x);
        if index < 0 || index as usize >= times.len() {
            return None;
        }
        Some(times[index as usize] as f64)
    }

    pub fn visible_logical_range(&self) -> Option<(f64, f64)> {
        self.time_scale
            .visible_logical_range()
            .map(|range| (range.left(), range.right()))
    }

    pub fn set_visible_logical_range(&mut self, from: f64, to: f64) {
        if from.is_finite() && to.is_finite() && from <= to {
            self.time_scale
                .set_logical_range(LogicalRange::new(from, to));
            self.invalidate_frame_scene();
        }
    }

    /// Visible data timestamps nearest the logical window edges.
    pub fn visible_time_range(&self) -> Option<(f64, f64)> {
        let times = self.data.merged_times();
        let range = self.time_scale.visible_strict_range()?;
        if times.is_empty() {
            return None;
        }
        let last = times.len() as i64 - 1;
        let left = range.left().clamp(0, last) as usize;
        let right = range.right().clamp(0, last) as usize;
        Some((times[left] as f64, times[right] as f64))
    }

    /// Set the visible window to the points bracketing a UTC-seconds range.
    pub fn set_visible_time_range(&mut self, from: f64, to: f64) {
        if !from.is_finite() || !to.is_finite() || from > to {
            return;
        }
        let times = self.data.merged_times();
        if times.is_empty() {
            return;
        }
        let left = times.partition_point(|&time| (time as f64) < from);
        let right = times.partition_point(|&time| (time as f64) <= to);
        if right == 0 || left >= times.len() {
            return;
        }
        let last = times.len() - 1;
        let left = left.min(last) as i64;
        let right = (right - 1).min(last) as i64;
        if left <= right {
            self.time_scale
                .set_visible_range(StrictRange::new(left, right), false);
            self.invalidate_frame_scene();
        }
    }

    /// Lay out stacked panes inside the chart content area. This is shared by hosts that need
    /// pane bounds before frame submission (for example, to draw axis separators).
    pub fn layout_panes(&mut self, content_h: f64) {
        let usable =
            (content_h - PANE_SEPARATOR * self.panes.len().saturating_sub(1) as f64).max(1.0);
        let total: f64 = self.panes.iter().map(|p| p.stretch_factor.max(0.01)).sum();
        let mut top = 0.0;
        let pane_count = self.panes.len();
        for (i, pane) in self.panes.iter_mut().enumerate() {
            pane.top = top;
            pane.height = usable * pane.stretch_factor.max(0.01) / total;
            pane.layout(content_h);
            top += pane.height;
            if i + 1 < pane_count {
                top += PANE_SEPARATOR;
            }
        }
    }

    fn sync_time_points(&mut self) {
        let sequence_changed =
            self.data.time_points_generation() != self.synced_time_points_generation;
        if sequence_changed {
            self.invalidate_frame_scene();
        }
        // Port of reference `ChartModel.updateTimeScale` (chart-model.ts:953-984): decide the
        // right-offset compensation BEFORE the new points/base index land on the scale.
        let old_first_time = self.synced_first_time;
        let new_first_time = self.data.merged_times().first().copied();
        let current_base_index = self.time_scale.base_index();
        let visible_bars = self.time_scale.visible_strict_range();
        let new_base_index = self.data.base_index();
        // the reference's `replacedExistingWhitespace` (firstChangedPointIndex === undefined): the time
        // scale points did not change, so a base-index move comes from a real bar replacing a
        // whitespace point (or a same-length data swap) rather than from new points.
        let points_unchanged =
            self.data.time_points_generation() == self.synced_time_points_generation;
        let replaced_existing_whitespace = points_unchanged;

        if let (Some(visible_bars), Some(old_first), Some(new_first)) =
            (visible_bars, old_first_time, new_first_time)
        {
            let is_last_series_bar_visible = visible_bars.contains(current_base_index);
            let is_left_bar_shift_to_left = old_first > new_first;
            let is_series_points_added =
                new_base_index.is_some_and(|new_base| new_base > current_base_index);
            let is_series_points_added_to_right =
                is_series_points_added && !is_left_bar_shift_to_left;

            let allow_shift_when_replacing_whitespace = self
                .time_scale
                .options()
                .allow_shift_visible_range_on_whitespace_replacement;
            let need_shift_visible_range_on_new_bar = is_last_series_bar_visible
                && (!replaced_existing_whitespace || allow_shift_when_replacing_whitespace)
                && self.time_scale.options().shift_visible_range_on_new_bar;
            if is_series_points_added_to_right && !need_shift_visible_range_on_new_bar {
                if let Some(new_base_index) = new_base_index {
                    let compensation_shift = new_base_index - current_base_index;
                    self.time_scale.set_right_offset(
                        self.time_scale.right_offset() - compensation_shift as f64,
                    );
                }
            }
        }

        let times = self.data.merged_times();
        let time_points_changed =
            self.data.time_points_generation() != self.synced_time_points_generation;
        let appended = time_points_changed
            && times.len() > self.synced_points_len
            && self.synced_points_len > 0
            && self.synced_last_time.is_some_and(|last| {
                times
                    .get(self.synced_points_len)
                    .is_some_and(|&time| time > last)
            });
        if appended {
            for index in self.synced_points_len..times.len() {
                let weight = nucleuscharts_core::scale::time_tick_marks::weight_by_time(
                    times[index],
                    times[index - 1],
                ) as u8;
                self.tick_marks.push_weight(index as i64, weight);
            }
        } else if time_points_changed {
            let mut weights = vec![0u8; times.len()];
            nucleuscharts_core::scale::time_tick_marks::fill_weights_for_points(
                times,
                &mut weights,
                0,
            );
            self.tick_marks.set_weights(&weights);
        }
        self.synced_points_len = times.len();
        self.synced_time_points_generation = self.data.time_points_generation();
        self.synced_last_time = times.last().copied();
        self.synced_first_time = times.first().copied();
        self.time_scale.set_points_len(times.len());
        self.time_scale.set_base_index(self.data.base_index());
    }
}
