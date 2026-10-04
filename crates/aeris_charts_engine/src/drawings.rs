//! Drawing tools (trend line, horizontal line/ray, vertical line, rectangle, Long/Short Position,
//! text, path, brush) are independently implemented as engine-owned objects. Public examples and
//! observed charting conventions inform the interaction expectations, while all state, hit testing,
//! and anchor-dragging math live headless here. Hosts only forward gestures and render the frame
//! across the engine boundary.
//!
//! Anchor model: a drawing is defined by one or more [`DrawingPoint`]s in `{logical, price}` space
//! (fractional logical bar index + price — the time scale's interpolation space, so an anchor
//! may sit between bars, like the reference examples' `timeToCoordinate`/`priceToCoordinate`
//! inputs). Coordinates resolve against the pane's RIGHT price scale each frame, mirroring the
//! pane-primitive converters' default target (chart/primitives.rs).
//!
//! Hit-testing and dragging reuse the same media-px coordinate space as the series hit tests
//! (hit_test.rs): x from the pane's left edge, y from the chart's top. A body drag translates
//! every anchor in COORDINATE space and converts back per anchor, so the shape stays pixel-rigid
//! under the cursor on any scale mode (normal/log/percentage).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, RasterImage};

use super::*;

mod geometry;
mod tools;

pub(crate) use geometry::{
    DrawingBodyGeometry, DrawingGeometryOptions, FibonacciGeometry, MeasureAxes, MeasureGeometry,
    PositionGeometry, PositionZone, resolve_drawing_geometry,
};
pub(crate) use tools::{
    DRAWING_TOOL_SPECS, DrawingHandleMode, DrawingLogicalExtent, DrawingMovementAxis,
    DrawingPlacement, DrawingPriceExtent, DrawingStraightenMode,
};

/// Chart-unique drawing id (never reused within a chart; 0 is the "no drawing" sentinel).
pub type DrawingId = u32;
/// Hard cap shared by live drawing APIs and persistence so variable-point tools remain bounded.
pub(crate) const MAX_DRAWING_POINTS: usize = 100_000;
pub const MAX_DRAWING_ICONS: usize = 32;
pub const MAX_DRAWING_ICON_SIZE: u32 = 96;
pub const MAX_DRAWING_ICON_NAME_BYTES: usize = 64;
pub const MAX_BARS_PATTERN_BARS: usize = 512;

/// Frozen source bar stored with a bars-pattern drawing. Offsets preserve whitespace gaps.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BarsPatternBar {
    pub offset: u16,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

impl BarsPatternBar {
    pub(crate) fn valid(self) -> bool {
        [self.open, self.high, self.low, self.close]
            .iter()
            .all(|value| value.is_finite())
            && self.high >= self.low
            && self.high >= self.open.max(self.close)
            && self.low <= self.open.min(self.close)
    }

    pub(crate) fn project(self, drawing: &Drawing) -> [DrawingPoint; 4] {
        let first = drawing.bars_pattern.first().copied().unwrap_or(self);
        let last = drawing.bars_pattern.last().copied().unwrap_or(self);
        let base = if drawing.bars_pattern_mirror_x {
            last.close
        } else {
            first.close
        };
        let logical_offset = if drawing.bars_pattern_mirror_x {
            last.offset.saturating_sub(self.offset)
        } else {
            self.offset
        };
        let target = drawing.points[2];
        let map_price = |price: f64| {
            target.price
                + (price - base)
                    * if drawing.bars_pattern_mirror_y {
                        -1.0
                    } else {
                        1.0
                    }
        };
        let logical = target.logical + f64::from(logical_offset);
        let open = if drawing.bars_pattern_mirror_x {
            self.close
        } else {
            self.open
        };
        let close = if drawing.bars_pattern_mirror_x {
            self.open
        } else {
            self.close
        };
        [open, self.high, self.low, close].map(|price| DrawingPoint {
            logical,
            price: map_price(price),
        })
    }
}

fn valid_bars_pattern(bars: &[BarsPatternBar]) -> bool {
    !bars.is_empty()
        && bars.len() <= MAX_BARS_PATTERN_BARS
        && bars.iter().all(|bar| bar.valid())
        && bars.windows(2).all(|pair| pair[0].offset < pair[1].offset)
        && bars
            .last()
            .is_some_and(|bar| usize::from(bar.offset) < MAX_BARS_PATTERN_BARS)
}

pub(crate) struct DrawingIconRegistry {
    images: HashMap<String, RasterImage>,
    next_key: u64,
}

impl Default for DrawingIconRegistry {
    fn default() -> Self {
        Self {
            images: HashMap::new(),
            next_key: 1_u64 << 62,
        }
    }
}

impl DrawingIconRegistry {
    pub(crate) fn get(&self, name: &str) -> Option<&RasterImage> {
        self.images.get(name)
    }

    fn set(&mut self, name: &str, width: u32, height: u32, pixels: Arc<[u8]>) -> bool {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(height as usize))
            .and_then(|value| value.checked_mul(4));
        if name.is_empty()
            || name.len() > MAX_DRAWING_ICON_NAME_BYTES
            || width == 0
            || height == 0
            || width > MAX_DRAWING_ICON_SIZE
            || height > MAX_DRAWING_ICON_SIZE
            || expected != Some(pixels.len())
            || (self.images.len() >= MAX_DRAWING_ICONS && !self.images.contains_key(name))
        {
            return false;
        }
        let Some(next_key) = self.next_key.checked_add(1) else {
            return false;
        };
        self.images.insert(
            name.to_string(),
            RasterImage {
                key: self.next_key,
                width,
                height,
                pixels,
            },
        );
        self.next_key = next_key;
        true
    }

    fn remove(&mut self, name: &str) -> bool {
        self.images.remove(name).is_some()
    }
}

impl ChartEngine {
    pub(crate) fn drawing_fibonacci_level_segment(
        &self,
        drawing: &Drawing,
        fib: FibonacciGeometry,
        value: f64,
        vpr: f64,
    ) -> ((f64, f64), (f64, f64)) {
        let effective = drawing.level_value(value);
        let mut segment = fib.segment(effective);
        if !drawing.level_log_scale || !drawing.kind.supports_log_levels() {
            return segment;
        }
        let Some(scale) = self.drawing_scale_for(drawing.pane_index, drawing.price_scale) else {
            return segment;
        };
        let base = self.drawing_scale_base_for(drawing.pane_index, drawing.price_scale);
        let zero = fib.segment(0.0);
        let one = fib.segment(1.0);
        for (index, point) in [&mut segment.0, &mut segment.1].into_iter().enumerate() {
            let (start, end) = if index == 0 {
                (zero.0, one.0)
            } else {
                (zero.1, one.1)
            };
            let p0 = scale.coordinate_to_price(start.1 / vpr, base);
            let p1 = scale.coordinate_to_price(end.1 / vpr, base);
            if p0 > 0.0 && p1 > 0.0 {
                let price = p0 * (p1 / p0).powf(effective);
                let y = scale.price_to_coordinate(price, base) * vpr;
                if y.is_finite() {
                    point.1 = y;
                }
            }
        }
        segment
    }
    /// Register an immutable RGBA8 image under a chart-local name. Replacements use fresh keys.
    pub fn set_drawing_icon(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        pixels: Arc<[u8]>,
    ) -> bool {
        if !self.drawing_icons.set(name, width, height, pixels) {
            return false;
        }
        self.invalidate_frame_drawings();
        true
    }

    pub fn remove_drawing_icon(&mut self, name: &str) -> bool {
        if !self.drawing_icons.remove(name) {
            return false;
        }
        self.invalidate_frame_drawings();
        true
    }
}

/// Open terminal chevron for the multi-click Path, ordered wing-tip-wing. `px` and `scale` are in
/// the caller's coordinate space, allowing hit testing to use media px and frame emission to use
/// bitmap px.
pub(crate) fn path_arrow_points(
    px: &[(f64, f64)],
    line_width: f64,
    scale: f64,
) -> Option<[(f64, f64); 3]> {
    let &tip = px.last()?;
    let previous = px[..px.len().saturating_sub(1)]
        .iter()
        .rev()
        .copied()
        .find(|point| (tip.0 - point.0).hypot(tip.1 - point.1) > f64::EPSILON)?;
    let distance = (tip.0 - previous.0).hypot(tip.1 - previous.1);
    let direction = (
        (tip.0 - previous.0) / distance,
        (tip.1 - previous.1) / distance,
    );
    let wing_length = (15.0 + line_width).clamp(16.0, 22.0) * scale;
    // Each wing opens 40 degrees from the reverse shaft direction.
    let back = wing_length * 0.766_044_443_118_978;
    let side = wing_length * 0.642_787_609_686_539_4;
    let base = (tip.0 - direction.0 * back, tip.1 - direction.1 * back);
    let perpendicular = (-direction.1 * side, direction.0 * side);
    Some([
        (base.0 + perpendicular.0, base.1 + perpendicular.1),
        tip,
        (base.0 - perpendicular.0, base.1 - perpendicular.1),
    ])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[doc(hidden)]
pub struct DrawingWorkStats {
    pub drawings_total: usize,
    pub bounds_tests: usize,
    pub candidates: usize,
    pub visible: usize,
    pub precise_hit_tests: usize,
    pub bounds_rebuilds: usize,
    pub geometry_rebuilds: usize,
    pub index_updates: usize,
}

#[derive(Clone, Copy, Debug)]
enum LogicalBounds {
    Full,
    From(f64),
    Ray { start: f64, towards_right: bool },
    Finite { min: f64, max: f64 },
}

#[derive(Clone, Copy, Debug)]
struct DrawingBounds {
    logical: LogicalBounds,
    min_price: Option<f64>,
    max_price: Option<f64>,
}

impl DrawingBounds {
    fn for_drawing(drawing: &Drawing) -> Self {
        let mut min_logical = f64::INFINITY;
        let mut max_logical = f64::NEG_INFINITY;
        let mut min_price = f64::INFINITY;
        let mut max_price = f64::NEG_INFINITY;
        for point in &drawing.points {
            min_logical = min_logical.min(point.logical);
            max_logical = max_logical.max(point.logical);
            min_price = min_price.min(point.price);
            max_price = max_price.max(point.price);
        }
        if drawing.kind == DrawingKind::BarsPattern {
            for &bar in &drawing.bars_pattern {
                for projected in bar.project(drawing) {
                    min_logical = min_logical.min(projected.logical);
                    max_logical = max_logical.max(projected.logical);
                    min_price = min_price.min(projected.price);
                    max_price = max_price.max(projected.price);
                }
            }
        }
        if matches!(
            drawing.kind,
            DrawingKind::FibonacciRetracement | DrawingKind::FibonacciExtension
        ) && drawing.points.len() == drawing.kind.anchor_count()
        {
            let (start, end) = if drawing.kind == DrawingKind::FibonacciRetracement {
                (drawing.points[0].price, drawing.points[1].price)
            } else {
                (
                    drawing.points[2].price,
                    drawing.points[2].price + drawing.points[1].price - drawing.points[0].price,
                )
            };
            for level in drawing.levels.iter().filter(|level| level.visible) {
                let value = drawing.level_value(level.value);
                let price = if drawing.level_log_scale && start > 0.0 && end > 0.0 {
                    start * (end / start).powf(value)
                } else {
                    start + (end - start) * value
                };
                if price.is_finite() {
                    min_price = min_price.min(price);
                    max_price = max_price.max(price);
                }
            }
        }
        if matches!(
            drawing.kind,
            DrawingKind::GannBox | DrawingKind::GannSquare | DrawingKind::GannSquareFixed
        ) && drawing.points.len() == 2
        {
            for level in drawing.levels.iter().filter(|level| level.visible) {
                let value = drawing.level_value(level.value);
                let logical = drawing.points[0].logical
                    + (drawing.points[1].logical - drawing.points[0].logical) * value;
                let price = drawing.points[0].price
                    + (drawing.points[1].price - drawing.points[0].price) * value;
                if logical.is_finite() && price.is_finite() {
                    min_logical = min_logical.min(logical);
                    max_logical = max_logical.max(logical);
                    min_price = min_price.min(price);
                    max_price = max_price.max(price);
                }
            }
        }
        if drawing.kind == DrawingKind::FibonacciExtension && drawing.points.len() == 3 {
            let projected =
                drawing.points[2].logical + (drawing.points[1].logical - drawing.points[0].logical);
            min_logical = min_logical.min(projected);
            max_logical = max_logical.max(projected);
        }
        if matches!(
            drawing.kind,
            DrawingKind::FibonacciTimeZones | DrawingKind::FibonacciTrendTime
        ) && drawing.points.len() == drawing.kind.anchor_count()
        {
            let origin = if drawing.kind == DrawingKind::FibonacciTimeZones {
                drawing.points[0].logical
            } else {
                drawing.points[2].logical
            };
            let step = drawing.points[1].logical - drawing.points[0].logical;
            for level in drawing.levels.iter().filter(|level| level.visible) {
                let projected = origin + step * drawing.level_value(level.value);
                if projected.is_finite() {
                    min_logical = min_logical.min(projected);
                    max_logical = max_logical.max(projected);
                }
            }
        }
        let spec = drawing.kind.spec();
        if spec.bounds_padding_ratio > 0.0 {
            let logical_pad = (max_logical - min_logical).abs() * spec.bounds_padding_ratio;
            let price_pad = (max_price - min_price).abs() * spec.bounds_padding_ratio;
            min_logical -= logical_pad;
            max_logical += logical_pad;
            min_price -= price_pad;
            max_price += price_pad;
        }
        let line_extension = matches!(
            drawing.kind,
            DrawingKind::TrendLine
                | DrawingKind::InfoLine
                | DrawingKind::TrendAngle
                | DrawingKind::ArrowLine
                | DrawingKind::FibonacciRetracement
                | DrawingKind::FibonacciExtension
                | DrawingKind::FibonacciChannel
        ) && (drawing.extend_left || drawing.extend_right);
        let logical = match spec.logical_extent {
            DrawingLogicalExtent::Full => LogicalBounds::Full,
            DrawingLogicalExtent::FromFirst => LogicalBounds::From(min_logical),
            DrawingLogicalExtent::Ray => LogicalBounds::Ray {
                start: drawing.points[0].logical,
                towards_right: drawing.points[1].logical >= drawing.points[0].logical,
            },
            DrawingLogicalExtent::Finite if line_extension => LogicalBounds::Full,
            DrawingLogicalExtent::Finite => LogicalBounds::Finite {
                min: min_logical,
                max: max_logical,
            },
        };
        let (min_price, max_price) = match spec.price_extent {
            DrawingPriceExtent::Full => (None, None),
            DrawingPriceExtent::Finite if line_extension => (None, None),
            DrawingPriceExtent::Finite => (Some(min_price), Some(max_price)),
        };
        Self {
            logical,
            min_price,
            max_price,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScreenBounds {
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
}

impl ScreenBounds {
    fn intersects(self, other: Self) -> bool {
        self.left <= other.right
            && self.right >= other.left
            && self.top <= other.bottom
            && self.bottom >= other.top
    }

    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

#[derive(Debug)]
struct DrawingCache {
    pane_index: usize,
    bounds: DrawingBounds,
    bounds_key: [u64; 12],
    geometry_key: [u64; 12],
    media_px: Vec<(f64, f64)>,
    screen_bounds: ScreenBounds,
    text_key: u64,
    text_width: f64,
    text_size: f64,
    screen_valid: bool,
    geometry_valid: bool,
}

impl DrawingCache {
    fn new(drawing: &Drawing) -> Self {
        Self {
            pane_index: drawing.pane_index,
            bounds: DrawingBounds::for_drawing(drawing),
            bounds_key: [0; 12],
            geometry_key: [0; 12],
            media_px: Vec::new(),
            screen_bounds: ScreenBounds::default(),
            text_key: u64::MAX,
            text_width: 0.0,
            text_size: 0.0,
            screen_valid: false,
            geometry_valid: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct DrawingRuntime {
    entries: HashMap<DrawingId, DrawingCache>,
    positions: HashMap<DrawingId, usize>,
    panes: Vec<Vec<DrawingId>>,
    scratch: Vec<DrawingId>,
    layout_generation: u64,
    font_size: f64,
    font_family: Rc<str>,
    stats: DrawingWorkStats,
}

impl DrawingRuntime {
    pub(crate) fn position(&self, id: DrawingId) -> Option<usize> {
        self.positions.get(&id).copied()
    }

    pub(crate) fn record_visible(&mut self) {
        self.stats.visible += 1;
    }

    pub(crate) fn record_precise_hit(&mut self) {
        self.stats.precise_hit_tests += 1;
    }

    pub(crate) fn pane_count(&self, pane_index: usize) -> usize {
        self.panes.get(pane_index).map_or(0, Vec::len)
    }

    fn ensure_panes(&mut self, pane_count: usize) {
        self.panes.resize_with(pane_count, Vec::new);
    }

    fn insert(&mut self, drawing: &Drawing, position: usize, pane_count: usize) {
        self.ensure_panes(pane_count);
        self.entries.insert(drawing.id, DrawingCache::new(drawing));
        self.positions.insert(drawing.id, position);
        if let Some(pane) = self.panes.get_mut(drawing.pane_index) {
            pane.push(drawing.id);
        }
        self.stats.bounds_rebuilds += 1;
        self.stats.index_updates += 1;
    }

    fn update(&mut self, drawing: &Drawing, pane_count: usize) {
        self.ensure_panes(pane_count);
        let old_pane = self.entries.get(&drawing.id).map(|entry| entry.pane_index);
        if old_pane != Some(drawing.pane_index) {
            if let Some(old) = old_pane.and_then(|pane| self.panes.get_mut(pane)) {
                old.retain(|&id| id != drawing.id);
            }
            if let Some(pane) = self.panes.get_mut(drawing.pane_index) {
                pane.push(drawing.id);
            }
            self.stats.index_updates += 1;
        }
        let entry = self
            .entries
            .entry(drawing.id)
            .or_insert_with(|| DrawingCache::new(drawing));
        entry.pane_index = drawing.pane_index;
        entry.bounds = DrawingBounds::for_drawing(drawing);
        entry.screen_valid = false;
        entry.text_key = u64::MAX;
        entry.geometry_valid = false;
        self.stats.bounds_rebuilds += 1;
    }

    fn remove(&mut self, id: DrawingId, drawings: &[Drawing]) {
        if let Some(entry) = self.entries.remove(&id) {
            if let Some(pane) = self.panes.get_mut(entry.pane_index) {
                pane.retain(|&candidate| candidate != id);
            }
            self.stats.index_updates += 1;
        }
        self.positions.clear();
        self.positions.extend(
            drawings
                .iter()
                .enumerate()
                .map(|(index, drawing)| (drawing.id, index)),
        );
    }

    fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.stats.index_updates += self.entries.len();
        }
        self.entries.clear();
        self.positions.clear();
        for pane in &mut self.panes {
            pane.clear();
        }
        self.scratch.clear();
    }

    pub(crate) fn rebuild_panes(&mut self, drawings: &[Drawing], pane_count: usize) {
        self.panes.clear();
        self.ensure_panes(pane_count);
        self.positions.clear();
        for (position, drawing) in drawings.iter().enumerate() {
            self.positions.insert(drawing.id, position);
            if let Some(entry) = self.entries.get_mut(&drawing.id) {
                entry.pane_index = drawing.pane_index;
                entry.screen_valid = false;
                entry.geometry_valid = false;
            } else {
                self.entries.insert(drawing.id, DrawingCache::new(drawing));
                self.stats.bounds_rebuilds += 1;
            }
            if let Some(pane) = self.panes.get_mut(drawing.pane_index) {
                pane.push(drawing.id);
            }
        }
        self.stats.index_updates += drawings.len();
    }

    pub(crate) fn rebuild_all(&mut self, drawings: &[Drawing], pane_count: usize) {
        self.clear();
        for (position, drawing) in drawings.iter().enumerate() {
            self.insert(drawing, position, pane_count);
        }
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        self.entries.capacity()
            * (std::mem::size_of::<DrawingId>() + std::mem::size_of::<DrawingCache>())
            + self.positions.capacity()
                * (std::mem::size_of::<DrawingId>() + std::mem::size_of::<usize>())
            + self
                .entries
                .values()
                .map(|entry| entry.media_px.capacity() * std::mem::size_of::<(f64, f64)>())
                .sum::<usize>()
            + self.panes.capacity() * std::mem::size_of::<Vec<DrawingId>>()
            + self
                .panes
                .iter()
                .map(|pane| pane.capacity() * std::mem::size_of::<DrawingId>())
                .sum::<usize>()
            + self.scratch.capacity() * std::mem::size_of::<DrawingId>()
            + self.font_family.len()
    }
}

/// The drawing-tool kinds. Wire values cross the wasm boundary as `u8`; names cross as the
/// snake_case strings [`DrawingKind::name`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingKind {
    /// Two-anchor segment (reference plugin-examples trend-line).
    TrendLine,
    /// One-anchor horizontal line spanning the pane at the anchor's price.
    HorizontalLine,
    /// One-anchor horizontal line from the anchor to the pane's right edge.
    HorizontalRay,
    /// One-anchor vertical line spanning the pane at the anchor's logical index (reference
    /// plugin-examples vertical-line).
    VerticalLine,
    /// Two-corner filled rectangle (reference plugin-examples rectangle-drawing-tool).
    Rectangle,
    /// One-anchor text label (reference plugin-examples anchored-text, anchored on a point).
    Text,
    /// Freehand path (the public reference's brush): a variable-length point list drawn as a smooth
    /// interpolating curve, with anchor handles at the two ENDS when selected.
    Brush,
    /// Multi-click path: a variable-length point list joined by straight segments, with every
    /// vertex exposed as an editable anchor.
    Path,
    /// Three-anchor Long Position annotation: entry, target, stop.
    LongPosition,
    /// Three-anchor Short Position annotation: entry, target, stop.
    ShortPosition,
    /// Two-anchor fixed-range volume profile. Statistics are bound to an engine data source.
    FixedRangeVolumeProfile,
    /// One-anchor volume profile extending from its anchor through the latest source row.
    AnchoredVolumeProfile,
    /// One-anchor VWAP with optional deviation bands through the latest source row.
    AnchoredVwap,
    /// Two-anchor price measurement: signed price change, percentage, and ticks.
    PriceRange,
    /// Two-anchor time measurement: signed bar count and elapsed time.
    DateRange,
    /// Two-anchor combined price and time measurement (also the Shift-click quick measure).
    DatePriceRange,
    /// Two-anchor line projected from its first anchor through the second.
    Ray,
    /// Two-anchor line projected to both pane edges.
    ExtendedLine,
    /// Two-anchor segment carrying price and time measurements.
    InfoLine,
    /// Two-anchor segment carrying its visual angle.
    TrendAngle,
    /// One-anchor horizontal and vertical reference lines.
    CrossLine,
    /// Two-anchor segment with an arrow at its end.
    ArrowLine,
    /// Three-anchor channel with a parallel second boundary.
    ParallelChannel,
    /// Three-anchor channel with a horizontal upper boundary.
    FlatTopChannel,
    /// Three-anchor channel with a horizontal lower boundary.
    FlatBottomChannel,
    /// Four-anchor channel whose two boundaries are independently placed.
    DisjointChannel,
    /// Least-squares trend channel over the selected source's closes.
    RegressionTrend,
    /// Multi-click straight polyline without a terminal arrow.
    Polyline,
    /// Broad translucent freehand mark.
    Highlighter,
    /// Rectangle defined by one edge and an orthogonal depth handle.
    RotatedRectangle,
    /// Ellipse fitted inside two opposite corners.
    Ellipse,
    /// Circle from center to perimeter.
    Circle,
    /// Three-anchor filled triangle.
    Triangle,
    /// Circular arc through three anchors.
    Arc,
    /// Three-anchor quadratic curve.
    Curve,
    /// Four-anchor cubic curve.
    DoubleCurve,
    /// Two-anchor Fibonacci price retracement with editable per-level styling.
    FibonacciRetracement,
    /// Three-anchor Fibonacci projection of a prior price and time swing.
    FibonacciExtension,
    /// Three-anchor sloped Fibonacci channel with editable parallel levels.
    FibonacciChannel,
    /// Vertical Fibonacci time zones projected from a two-anchor interval.
    FibonacciTimeZones,
    /// Vertical Fibonacci time projection from a third anchor.
    FibonacciTrendTime,
    /// Sloped speed resistance levels fanning from the first anchor.
    FibonacciSpeedFan,
    /// Concentric speed resistance arcs centered on the second anchor.
    FibonacciSpeedArcs,
    /// Full concentric Fibonacci circles centered on the second anchor.
    FibonacciCircles,
    /// Two-turn spiral scaled from the first anchor toward the second.
    FibonacciSpiral,
    /// Three-anchor wedge with rays interpolated between its two edges.
    FibonacciWedge,
    /// Andrews median line with parallel outer tines.
    AndrewsPitchfork,
    /// Pitchfork with the median origin shifted halfway in price.
    SchiffPitchfork,
    /// Pitchfork with the median origin shifted halfway in time and price.
    ModifiedSchiffPitchfork,
    /// Pitchfork with a median from the first two anchors' midpoint through the third.
    InsidePitchfork,
    /// Rays fanning from the first anchor through levels on the other two anchors.
    Pitchfan,
    /// Five-point harmonic XABCD path.
    PatternXabcd,
    /// Five-point cypher harmonic path.
    PatternCypher,
    /// Four-point AB=CD path.
    PatternAbcd,
    /// Left shoulder, head, right shoulder and neckline path.
    PatternHeadShoulders,
    /// Five-point converging triangle path.
    PatternTriangle,
    /// Three-drive reversal path.
    PatternThreeDrives,
    /// Elliott impulse path, origin through waves one to five.
    ElliottImpulse,
    /// Elliott correction path, origin through waves A to C.
    ElliottCorrection,
    /// Elliott triangle path, origin through waves A to E.
    ElliottTriangle,
    /// Elliott double combination path.
    ElliottDoubleCombination,
    /// Elliott triple combination path.
    ElliottTripleCombination,
    /// Repeated vertical lines at the interval between two anchors.
    CyclicLines,
    /// Forward time-cycle markers and alternating cycle bands.
    TimeCycles,
    /// Repeating sine wave with the second anchor as its first quarter-cycle peak.
    SineLine,
    /// Directional chart marker pointing up.
    ArrowMarkerUp,
    /// Directional chart marker pointing down.
    ArrowMarkerDown,
    /// Directional chart marker pointing left.
    ArrowMarkerLeft,
    /// Directional chart marker pointing right.
    ArrowMarkerRight,
    /// Flag marker at one chart anchor.
    FlagMark,
    /// Two-anchor signpost with a stem and marker head.
    Signpost,
    /// One-anchor boxed note.
    Note,
    /// One-anchor boxed comment.
    Comment,
    /// Two-anchor callout leader and editable text.
    Callout,
    /// Horizontal price guide with editable note.
    PriceNote,
    /// One-anchor price badge at the pane edge.
    PriceLabel,
    /// Text fixed to a normalized position in the pane viewport.
    AnchoredText,
    /// Host supplied bounded raster icon at one chart anchor.
    IconStamp,
    /// Price and time partition grid between two anchors.
    GannBox,
    /// Gann square with grid, fan and arc geometry.
    GannSquare,
    /// Gann square whose display box remains square in media pixels.
    GannSquareFixed,
    /// Nine proportionate Gann angle rays from a pivot.
    GannFan,
    /// Three-anchor future price sector: origin, time horizon, projected price.
    Projection,
    /// Trade target projection with a data-derived result after its time horizon.
    Forecast,
    /// Frozen, movable copy of a bounded source OHLC range.
    BarsPattern,
}

impl DrawingKind {
    pub(crate) const fn has_levels(self) -> bool {
        matches!(
            self,
            Self::FibonacciRetracement
                | Self::FibonacciExtension
                | Self::FibonacciChannel
                | Self::FibonacciTimeZones
                | Self::FibonacciTrendTime
                | Self::FibonacciSpeedFan
                | Self::FibonacciSpeedArcs
                | Self::FibonacciCircles
                | Self::FibonacciSpiral
                | Self::FibonacciWedge
                | Self::AndrewsPitchfork
                | Self::SchiffPitchfork
                | Self::ModifiedSchiffPitchfork
                | Self::InsidePitchfork
                | Self::Pitchfan
                | Self::GannBox
                | Self::GannSquare
                | Self::GannSquareFixed
                | Self::GannFan
        )
    }

    pub(crate) const fn supports_log_levels(self) -> bool {
        matches!(
            self,
            Self::FibonacciRetracement | Self::FibonacciExtension | Self::FibonacciChannel
        )
    }
    pub fn from_u8(kind: u8) -> Option<Self> {
        DRAWING_TOOL_SPECS
            .iter()
            .find(|spec| spec.wire_id == kind)
            .map(|spec| spec.kind)
    }

    pub fn to_u8(self) -> u8 {
        self.spec().wire_id
    }

    /// The public snake_case wire name (TS `drawing_kind`).
    pub fn name(self) -> &'static str {
        self.spec().name
    }

    pub fn from_name(name: &str) -> Option<Self> {
        DRAWING_TOOL_SPECS
            .iter()
            .find(|spec| spec.name == name)
            .map(|spec| spec.kind)
    }

    /// The number of defining anchors the kind is placed with (and its handles show). Brush and
    /// path are variable-length: this is their minimum.
    pub fn anchor_count(self) -> usize {
        self.spec().placement.minimum_points()
    }

    /// Whether `count` is a valid point count for a stored drawing of this kind.
    pub fn valid_point_count(self, count: usize) -> bool {
        self.spec().placement.valid_point_count(count)
    }
}

/// One defining anchor of a drawing: a fractional logical bar index (integer values sit at bar
/// centers — the time scale's `logical_to_coordinate` space) plus a price. The unused
/// coordinate of one-anchor kinds is stored but never read (a horizontal line's `logical`, a
/// vertical line's `price`).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingPoint {
    pub logical: f64,
    pub price: f64,
}

/// Full-resolution identity persisted alongside a logical drawing anchor on a non-time chart.
/// The live geometry remains in logical/price space; this sidecar lets a restored drawing find
/// the same bar after the host installs a different retained sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DrawingAnchorTime {
    pub(crate) open_timestamp_micros: i64,
    pub(crate) close_timestamp_micros: i64,
}

/// Horizontal text alignment shared by every tool's label (TS `drawing_text_h_align`; maps to
/// the IR's `TextAlign`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingTextHAlign {
    Left,
    Center,
    Right,
}

impl DrawingTextHAlign {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "left" => Self::Left,
            "center" => Self::Center,
            "right" => Self::Right,
            _ => return None,
        })
    }
}

/// Vertical text alignment shared by every tool's label (TS `drawing_text_v_align`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingTextVAlign {
    Top,
    Middle,
    Bottom,
}

/// Price scale used to convert a drawing's price anchors. Official series-bound primitives use
/// the attached series' scale; generic drawings keep the right-scale default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingPriceScale {
    Right,
    Left,
    Overlay,
}

impl DrawingPriceScale {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Overlay => "overlay",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "right" => Self::Right,
            "left" => Self::Left,
            "overlay" | "" => Self::Overlay,
            _ => return None,
        })
    }
}

impl DrawingTextVAlign {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Middle => "middle",
            Self::Bottom => "bottom",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "top" => Self::Top,
            "middle" => Self::Middle,
            "bottom" => Self::Bottom,
            _ => return None,
        })
    }
}

/// Canonical interactive primary used by drawing strokes and handles.
pub const DRAWING_DEFAULT_COLOR: &str = aeris_charts_core::style::DEFAULT_PRIMARY_CSS;

/// An engine-owned drawing. Colors follow the series pattern: stored verbatim as CSS strings
/// and parsed at render time (`None`/unparseable falls back to the follow behavior documented
/// per field).
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub id: DrawingId,
    pub kind: DrawingKind,
    pub pane_index: usize,
    pub points: Vec<DrawingPoint>,
    /// Stable host-facing identity. Empty names are allowed and mean "use the tool name".
    pub name: String,
    /// Optional host-owned group identity used by the object tree and bulk operations.
    pub group_id: Option<String>,
    /// Monotonic semantic revision. Geometry/style changes increment it; render-cache changes do
    /// not. Hosts use it to route cross-cell drawing sync without echo loops.
    pub revision: u64,
    /// Hidden drawings remain in persistence and the object tree but do not render or hit-test.
    pub visible: bool,
    /// Locked drawings remain selectable but cannot be dragged or edited.
    pub locked: bool,
    /// Stable layer order within the drawing layer. The vector order remains the deterministic
    /// tie-breaker for equal values.
    pub z_order: i32,
    pub interval_visibility: crate::DrawingIntervalVisibility,
    pub stroke_start: crate::DrawingLineCap,
    pub stroke_end: crate::DrawingLineCap,
    pub extend_left: bool,
    pub extend_right: bool,
    pub fill_enabled: bool,
    pub magnet: crate::DrawingMagnetMode,
    pub labels: Vec<crate::DrawingLabelOptions>,
    pub levels: Vec<crate::DrawingLevel>,
    /// Independently styled angle rays and quarter arcs on Gann squares.
    pub gann_fans: Vec<crate::DrawingLevel>,
    pub gann_arcs: Vec<crate::DrawingLevel>,
    pub level_reverse: bool,
    pub level_log_scale: bool,
    pub level_show_prices: bool,
    pub level_show_values: bool,
    pub level_show_percents: bool,
    /// `left`, `center`, or `right`.
    pub level_label_align: String,
    /// Elliott wave label degree; used only by the five Elliott tools.
    pub wave_degree: String,
    /// Normalized pane position of screen anchored text, independent of time and price scales.
    pub screen_x: f64,
    pub screen_y: f64,
    /// Stable host asset key for an icon stamp; pixels are chart-local and never serialized.
    pub icon_name: Option<String>,
    /// Icon stamp destination size in CSS pixels.
    pub icon_size: f64,
    /// Source OHLC snapshot for a movable bars-pattern ghost copy.
    pub bars_pattern: Vec<BarsPatternBar>,
    pub bars_pattern_mirror_x: bool,
    pub bars_pattern_mirror_y: bool,
    /// `bars`, `line_open`, `line_high`, `line_low`, or `line_close`.
    pub bars_pattern_mode: String,
    pub price_scale: DrawingPriceScale,
    /// Optional engine-data binding for volume-profile and anchored-VWAP drawings.
    pub profile: Option<crate::ProfileDrawingOptions>,
    /// Hypothetical account balance for position drawing statistics; never a broker balance.
    pub position_account_size: f64,
    /// Percentage of the hypothetical balance risked at the stop (0..=100).
    pub position_risk_percent: f64,
    /// Optional explicit source for regression trend; `None` follows the pane's primary series.
    pub regression_source_id: Option<u32>,
    /// Number of residual standard deviations on each side of the regression center.
    pub regression_deviations: f64,
    /// Line/border color CSS string (default [`DRAWING_DEFAULT_COLOR`]).
    pub color: String,
    /// Stroke width in CSS px (default 2; 1 for a rectangle's border).
    pub width: f64,
    pub style: LineStyle,
    /// Rectangle fill CSS string; `None` fills with the border color at 20% alpha. Unused by
    /// the line kinds and the text tool.
    pub fill_color: Option<String>,
    /// Rectangle creation-preview fill; `None` follows `fill_color`. Stored with the drawing so
    /// re-arming a tool from its options preserves the official preview color.
    pub preview_fill_color: Option<String>,
    /// Whether a rectangle paints its outline. The generic drawing defaults to `true`; the
    /// official rectangle-drawing plugin disables it and paints only the fill.
    pub border_visible: bool,
    /// Rectangle endpoint labels on the price and time axes.
    pub show_labels: bool,
    /// Whether the rectangle paints the official 15 CSS px bands into both axis panes.
    pub axis_bands_visible: bool,
    /// Rectangle endpoint-label background; `None` follows the drawing color.
    pub label_color: Option<String>,
    /// Rectangle endpoint-label text; `None` follows the chart foreground.
    pub label_text_color: Option<String>,
    /// Snap rectangle time anchors to canonical data times, matching the official plugin's
    /// `MouseEventParams.time` placement instead of retaining a fractional x coordinate.
    pub snap_time_to_data: bool,
    /// The tool's text label (`""` = none). Empty text tools paint nothing on the chart; the
    /// host typing-mode editor is the empty-state UI, and leaving it without typed text removes
    /// the drawing.
    pub text: String,
    /// Label color CSS string. `None` follows the drawing stroke for trend lines and the chart's
    /// `layout.textColor` for the standalone text tool.
    pub text_color: Option<String>,
    /// Label glyph size in CSS px; `None` follows the chart's `layout.fontSize`.
    pub text_size: Option<f64>,
    /// Label font weight (numeric CSS weight 100–900; `None` = normal 400, 700 = bold).
    pub text_weight: Option<u16>,
    pub text_italic: bool,
    pub text_h_align: DrawingTextHAlign,
    pub text_v_align: DrawingTextVAlign,
    /// Text-tool container background CSS string (the public reference's text-box background); `None`
    /// draws no box. Text tool only.
    pub box_color: Option<String>,
    /// Text-tool container border CSS string; `None` draws no border. Text tool only.
    pub box_border_color: Option<String>,
    /// Text-tool container border width in CSS px (default 1).
    pub box_border_width: f64,
}

/// Default glyph size for the TEXT TOOL's label in CSS px when `text_size` is unset
/// (the public reference's default text-tool size). Other tools' labels keep following the chart's
/// `layout.font_size`.
pub const TEXT_TOOL_DEFAULT_SIZE: f64 = 14.0;

/// The text tool's interaction-chrome padding in CSS px: the editing border (2 px) plus its
/// padding (4 px), mirrored by the host's `#aeris_charts-text-editor` wrap (impl.ts). The
/// hit area and the hover/focus borders all use this box so selection, hovering, and typing
/// mode land on exactly the same outline.
pub(crate) const TEXT_CHROME_PAD: f64 = TEXT_PAD + 2.0;
pub(crate) const TREND_TEXT_PLACEHOLDER: &str = "+ Add text";

fn default_gann_level(value: f64) -> crate::DrawingLevel {
    crate::DrawingLevel {
        value,
        color: String::new(),
        visible: true,
        style: "solid".to_string(),
        fill_between: false,
        fill_color: None,
        label_visible: false,
    }
}

pub(crate) fn valid_gann_family(levels: &[crate::DrawingLevel], fan: bool) -> bool {
    levels.len() <= crate::MAX_DRAWING_LEVELS
        && levels.iter().all(|level| {
            level.validate()
                && if fan {
                    (0.01..=100.0).contains(&level.value)
                } else {
                    (0.0..=1.0).contains(&level.value)
                }
        })
}

impl Drawing {
    pub(crate) fn new(
        id: DrawingId,
        kind: DrawingKind,
        pane_index: usize,
        mut points: Vec<DrawingPoint>,
    ) -> Self {
        Self::normalize_position_points(kind, &mut points);
        let (text_h_align, text_v_align) = if matches!(
            kind,
            DrawingKind::TrendLine | DrawingKind::Callout | DrawingKind::PriceNote
        ) {
            (DrawingTextHAlign::Right, DrawingTextVAlign::Top)
        } else {
            (DrawingTextHAlign::Center, DrawingTextVAlign::Middle)
        };
        Self {
            id,
            kind,
            pane_index,
            points,
            name: String::new(),
            group_id: None,
            revision: 1,
            visible: true,
            locked: false,
            z_order: id as i32,
            interval_visibility: Default::default(),
            stroke_start: if kind == DrawingKind::Callout {
                crate::DrawingLineCap::Arrow
            } else {
                Default::default()
            },
            stroke_end: if kind == DrawingKind::ArrowLine {
                crate::DrawingLineCap::Arrow
            } else {
                Default::default()
            },
            extend_left: false,
            extend_right: false,
            fill_enabled: kind == DrawingKind::Rectangle
                || kind.is_measure()
                || matches!(
                    kind,
                    DrawingKind::ParallelChannel
                        | DrawingKind::RegressionTrend
                        | DrawingKind::FlatTopChannel
                        | DrawingKind::FlatBottomChannel
                        | DrawingKind::DisjointChannel
                        | DrawingKind::RotatedRectangle
                        | DrawingKind::Ellipse
                        | DrawingKind::Circle
                        | DrawingKind::Triangle
                        | DrawingKind::FibonacciRetracement
                        | DrawingKind::FibonacciExtension
                        | DrawingKind::FibonacciChannel
                        | DrawingKind::FibonacciTimeZones
                        | DrawingKind::FibonacciTrendTime
                        | DrawingKind::FibonacciSpeedFan
                        | DrawingKind::FibonacciSpeedArcs
                        | DrawingKind::FibonacciCircles
                        | DrawingKind::FibonacciSpiral
                        | DrawingKind::FibonacciWedge
                        | DrawingKind::AndrewsPitchfork
                        | DrawingKind::SchiffPitchfork
                        | DrawingKind::ModifiedSchiffPitchfork
                        | DrawingKind::InsidePitchfork
                        | DrawingKind::Pitchfan
                        | DrawingKind::TimeCycles
                        | DrawingKind::GannBox
                        | DrawingKind::GannSquare
                        | DrawingKind::GannSquareFixed
                        | DrawingKind::GannFan
                        | DrawingKind::Projection
                ),
            magnet: Default::default(),
            labels: match kind {
                DrawingKind::InfoLine => [
                    crate::DrawingLabelMetric::PriceChange,
                    crate::DrawingLabelMetric::PercentChange,
                    crate::DrawingLabelMetric::BarCount,
                    crate::DrawingLabelMetric::Angle,
                ]
                .into_iter()
                .map(|metric| crate::DrawingLabelOptions {
                    metric,
                    visible: true,
                    position: crate::DrawingLabelPosition::Above,
                    text: None,
                })
                .collect(),
                DrawingKind::TrendAngle => vec![crate::DrawingLabelOptions {
                    metric: crate::DrawingLabelMetric::Angle,
                    visible: true,
                    position: crate::DrawingLabelPosition::Above,
                    text: None,
                }],
                _ => Vec::new(),
            },
            levels: if matches!(
                kind,
                DrawingKind::FibonacciRetracement
                    | DrawingKind::FibonacciExtension
                    | DrawingKind::FibonacciChannel
                    | DrawingKind::FibonacciTimeZones
                    | DrawingKind::FibonacciTrendTime
                    | DrawingKind::FibonacciSpeedFan
                    | DrawingKind::FibonacciSpeedArcs
                    | DrawingKind::FibonacciCircles
                    | DrawingKind::FibonacciSpiral
                    | DrawingKind::FibonacciWedge
                    | DrawingKind::AndrewsPitchfork
                    | DrawingKind::SchiffPitchfork
                    | DrawingKind::ModifiedSchiffPitchfork
                    | DrawingKind::InsidePitchfork
                    | DrawingKind::Pitchfan
                    | DrawingKind::GannBox
                    | DrawingKind::GannSquare
                    | DrawingKind::GannSquareFixed
                    | DrawingKind::GannFan
            ) {
                let values: &[f64] = if matches!(
                    kind,
                    DrawingKind::FibonacciTimeZones | DrawingKind::FibonacciTrendTime
                ) {
                    &[0.0, 1.0, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0]
                } else if kind == DrawingKind::FibonacciExtension {
                    &[0.0, 0.618, 1.0, 1.618, 2.0, 2.618]
                } else if kind == DrawingKind::GannFan {
                    &[0.125, 0.25, 0.333333333333, 0.5, 1.0, 2.0, 3.0, 4.0, 8.0]
                } else if matches!(
                    kind,
                    DrawingKind::GannBox | DrawingKind::GannSquare | DrawingKind::GannSquareFixed
                ) {
                    &[0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
                } else if matches!(
                    kind,
                    DrawingKind::AndrewsPitchfork
                        | DrawingKind::SchiffPitchfork
                        | DrawingKind::ModifiedSchiffPitchfork
                        | DrawingKind::InsidePitchfork
                ) {
                    &[0.0, 0.5, 1.0]
                } else if kind == DrawingKind::Pitchfan {
                    &[0.0, 0.25, 0.5, 0.75, 1.0]
                } else {
                    &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0]
                };
                values
                    .iter()
                    .map(|&value| crate::DrawingLevel {
                        value,
                        color: String::new(),
                        visible: true,
                        style: "solid".to_string(),
                        fill_between: false,
                        fill_color: None,
                        label_visible: true,
                    })
                    .collect()
            } else {
                Vec::new()
            },
            gann_fans: if matches!(kind, DrawingKind::GannSquare | DrawingKind::GannSquareFixed) {
                [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0]
                    .into_iter()
                    .map(default_gann_level)
                    .collect()
            } else {
                Vec::new()
            },
            gann_arcs: if matches!(kind, DrawingKind::GannSquare | DrawingKind::GannSquareFixed) {
                [0.25, 0.5, 0.75, 1.0]
                    .into_iter()
                    .map(default_gann_level)
                    .collect()
            } else {
                Vec::new()
            },
            wave_degree: "minor".to_string(),
            level_reverse: false,
            level_log_scale: false,
            level_show_prices: matches!(
                kind,
                DrawingKind::FibonacciRetracement
                    | DrawingKind::FibonacciExtension
                    | DrawingKind::FibonacciChannel
            ),
            level_show_values: matches!(
                kind,
                DrawingKind::FibonacciTimeZones | DrawingKind::FibonacciTrendTime
            ),
            level_show_percents: !matches!(
                kind,
                DrawingKind::FibonacciTimeZones | DrawingKind::FibonacciTrendTime
            ),
            level_label_align: if matches!(
                kind,
                DrawingKind::FibonacciTimeZones | DrawingKind::FibonacciTrendTime
            ) {
                "left"
            } else if matches!(
                kind,
                DrawingKind::FibonacciSpeedArcs
                    | DrawingKind::FibonacciCircles
                    | DrawingKind::FibonacciSpiral
                    | DrawingKind::FibonacciWedge
                    | DrawingKind::AndrewsPitchfork
                    | DrawingKind::SchiffPitchfork
                    | DrawingKind::ModifiedSchiffPitchfork
                    | DrawingKind::InsidePitchfork
                    | DrawingKind::Pitchfan
                    | DrawingKind::GannBox
                    | DrawingKind::GannSquare
                    | DrawingKind::GannSquareFixed
            ) {
                "center"
            } else {
                "right"
            }
            .to_string(),
            screen_x: 0.5,
            screen_y: 0.5,
            icon_name: None,
            icon_size: 24.0,
            bars_pattern: Vec::new(),
            bars_pattern_mirror_x: false,
            bars_pattern_mirror_y: false,
            bars_pattern_mode: "bars".to_string(),
            price_scale: DrawingPriceScale::Right,
            profile: None,
            position_account_size: 1000.0,
            position_risk_percent: 25.0,
            regression_source_id: None,
            regression_deviations: 2.0,
            color: DRAWING_DEFAULT_COLOR.to_string(),
            width: kind.spec().default_width,
            style: LineStyle::Solid,
            fill_color: None,
            preview_fill_color: None,
            border_visible: kind != DrawingKind::Rectangle,
            show_labels: false,
            axis_bands_visible: false,
            label_color: None,
            label_text_color: None,
            snap_time_to_data: false,
            text: String::new(),
            text_color: None,
            text_size: None,
            text_weight: None,
            text_italic: false,
            text_h_align,
            text_v_align,
            box_color: match kind {
                DrawingKind::Note => Some("#facc1533".to_string()),
                DrawingKind::Comment | DrawingKind::Callout => Some("#2962ff22".to_string()),
                _ => None,
            },
            box_border_color: match kind {
                DrawingKind::Note => Some("#eab308".to_string()),
                DrawingKind::Comment | DrawingKind::Callout => {
                    Some(DRAWING_DEFAULT_COLOR.to_string())
                }
                _ => None,
            },
            box_border_width: aeris_charts_core::style::BORDER_WIDTH,
        }
    }

    /// Long/Short Position has semantic levels, not three unrelated corners. Keep the stop on the
    /// origin edge and project target/stop to the correct side of entry while preserving each
    /// supplied distance. This also repairs older malformed persisted/programmatic values.
    fn normalize_position_points(kind: DrawingKind, points: &mut [DrawingPoint]) {
        if points.len() != 3 {
            return;
        }
        if !matches!(kind, DrawingKind::LongPosition | DrawingKind::ShortPosition) {
            return;
        }
        let entry = points[0];
        let reward_distance = (points[1].price - entry.price).abs();
        let risk_distance = (points[2].price - entry.price).abs();
        points[2].logical = entry.logical;
        match kind {
            DrawingKind::LongPosition => {
                points[1].price = entry.price + reward_distance;
                points[2].price = entry.price - risk_distance;
            }
            DrawingKind::ShortPosition => {
                points[1].price = entry.price - reward_distance;
                points[2].price = entry.price + risk_distance;
            }
            _ => unreachable!(),
        }
    }

    /// The label a drawing actually renders. Empty text tools render nothing — the host's
    /// typing-mode editor is the only empty-state UI, and leaving that editor without typed
    /// text removes the drawing (the public reference: no lingering "Add text" ghost on the chart).
    pub fn display_text(&self) -> &str {
        self.text.as_str()
    }

    /// The glyph size the label actually renders at in CSS px: `text_size` when set, else
    /// [`TEXT_TOOL_DEFAULT_SIZE`] for the text tool and the chart's `layout.font_size` for
    /// every other tool's label. Rendering, hit-testing, and host editing chrome must all
    /// resolve through this so they never disagree.
    pub fn resolved_text_size(&self, layout_font_size: f64) -> f64 {
        self.text_size.unwrap_or(match self.kind {
            DrawingKind::Text | DrawingKind::AnchoredText => TEXT_TOOL_DEFAULT_SIZE,
            _ => layout_font_size,
        })
    }

    /// Typed common-contract view used by property panels and sync adapters.  It is derived from
    /// the authoritative live drawing, so the view cannot drift from render or hit-test state.
    pub fn common_snapshot(&self) -> crate::DrawingCommonSnapshot {
        crate::DrawingCommonSnapshot {
            id: self.id,
            kind: self.kind,
            name: self.name.clone(),
            group_id: self.group_id.clone(),
            revision: self.revision,
            visible: self.visible,
            locked: self.locked,
            z_order: self.z_order,
            pane_index: self.pane_index,
            price_scale: self.price_scale,
            magnet: self.magnet,
            interval_visibility: self.interval_visibility.clone(),
            stroke_start: self.stroke_start,
            stroke_end: self.stroke_end,
            extend_left: self.extend_left,
            extend_right: self.extend_right,
            fill_enabled: self.fill_enabled,
            labels: self.labels.clone(),
            levels: self.levels.clone(),
        }
    }

    /// Typed kind-specific option block derived from the authoritative live drawing.
    pub fn kind_options(&self) -> crate::DrawingKindOptions {
        match self.kind {
            DrawingKind::Rectangle => crate::DrawingKindOptions::Rectangle {
                fill_color: self.fill_color.clone(),
                preview_fill_color: self.preview_fill_color.clone(),
                border_visible: self.border_visible,
                show_labels: self.show_labels,
                axis_bands_visible: self.axis_bands_visible,
                label_color: self.label_color.clone(),
                label_text_color: self.label_text_color.clone(),
                snap_time_to_data: self.snap_time_to_data,
            },
            DrawingKind::Text
            | DrawingKind::Note
            | DrawingKind::Comment
            | DrawingKind::Callout
            | DrawingKind::PriceNote => crate::DrawingKindOptions::Text {
                box_color: self.box_color.clone(),
                box_border_color: self.box_border_color.clone(),
                box_border_width: self.box_border_width,
            },
            DrawingKind::AnchoredText => crate::DrawingKindOptions::AnchoredText {
                screen_x: self.screen_x,
                screen_y: self.screen_y,
                box_color: self.box_color.clone(),
                box_border_color: self.box_border_color.clone(),
                box_border_width: self.box_border_width,
            },
            DrawingKind::IconStamp => crate::DrawingKindOptions::IconStamp {
                icon_name: self.icon_name.clone(),
                icon_size: self.icon_size,
            },
            DrawingKind::BarsPattern => crate::DrawingKindOptions::BarsPattern {
                mirror_x: self.bars_pattern_mirror_x,
                mirror_y: self.bars_pattern_mirror_y,
                mode: self.bars_pattern_mode.clone(),
                bar_count: self.bars_pattern.len(),
            },
            DrawingKind::LongPosition | DrawingKind::ShortPosition => {
                crate::DrawingKindOptions::Position {
                    levels: self.levels.clone(),
                    account_size: self.position_account_size,
                    risk_percent: self.position_risk_percent,
                }
            }
            DrawingKind::RegressionTrend => crate::DrawingKindOptions::RegressionTrend {
                source_id: self.regression_source_id,
                deviations: self.regression_deviations,
            },
            kind if kind.is_elliott() => crate::DrawingKindOptions::Elliott {
                wave_degree: self.wave_degree.clone(),
            },
            DrawingKind::GannSquare | DrawingKind::GannSquareFixed => {
                crate::DrawingKindOptions::GannSquare {
                    levels: self.levels.clone(),
                    fans: self.gann_fans.clone(),
                    arcs: self.gann_arcs.clone(),
                    reverse: self.level_reverse,
                    show_prices: self.level_show_prices,
                    show_values: self.level_show_values,
                    show_percents: self.level_show_percents,
                    label_align: self.level_label_align.clone(),
                }
            }
            kind if kind.has_levels() => crate::DrawingKindOptions::Levels {
                levels: self.levels.clone(),
                reverse: self.level_reverse,
                log_scale: self.level_log_scale,
                show_prices: self.level_show_prices,
                show_values: self.level_show_values,
                show_percents: self.level_show_percents,
                label_align: self.level_label_align.clone(),
            },
            _ => crate::DrawingKindOptions::Generic,
        }
    }

    fn rebase_logical_with<F>(&mut self, map: F) -> bool
    where
        F: Fn(f64) -> f64 + Copy,
    {
        rebase_points_with(&mut self.points, map)
    }
}

fn rebase_points_with<F>(points: &mut [DrawingPoint], map: F) -> bool
where
    F: Fn(f64) -> f64 + Copy,
{
    let mut changed = false;
    for point in points {
        let logical = map(point.logical);
        changed |= logical != point.logical;
        point.logical = logical;
    }
    changed
}

/// The part of a drawing a hit/drag landed on: the whole shape (a move drag) or one defining
/// anchor (a re-anchor drag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingDragPart {
    Body,
    Anchor(usize),
}

/// A drawing hit-test outcome (cf. [`SeriesHit`]): the drawing, the part under the cursor, and
/// the CSS cursor the host should apply while the hit holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawingHit {
    pub id: DrawingId,
    pub part: DrawingDragPart,
    pub cursor: &'static str,
}

/// Modifier keys the host gesture layer forwards with pointer positions (the public reference modifier
/// semantics): `magnet` snaps anchors to the nearest bar — x to the bar's center, the price to
/// its closest rendered field (OHLC for candles/bars, value for scalar series; Ctrl/Cmd — the
/// same key that magnets the crosshair);
/// `straighten` constrains the dragged anchor of a two-anchor tool so the segment snaps to
/// 0°/45°/90° (a rectangle to a square) and body drags to the dominant axis (Shift only —
/// Ctrl never straightens). Where the two compose, straighten wins for the dragged anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawingModifiers {
    pub magnet: bool,
    pub straighten: bool,
}

/// Host text-measure callback for drawing-label hit boxes (the formatter-hook pattern: the
/// engine stays headless; hosts inject a plain boxed closure measuring `text` at
/// `{italic} {weight} {size}px {family}` and returning the width in the same px units as
/// `size`). Without one the engine estimates `chars × size × 0.6` (deterministic for native
/// tests).
pub type TextMeasureFn = Box<dyn Fn(&str, f64, &str, u16, bool) -> f64>;

/// Host vertical glyph metric for `{italic} {weight} {size}px {family}`: the offset, in the same px
/// units as `size`, that moves a `Prim::Text` anchor (Canvas `textBaseline: "middle"`, the em-box
/// center) so cap-height ink is centered on the intended line instead. Browser hosts derive it
/// from `measureText` font bounds and cap ink; native hosts use the font's cap height.
/// Without one the engine uses no correction (deterministic for native tests).
pub type TextCapCenterFn = Box<dyn Fn(f64, &str, u16, bool) -> f64>;

/// Active anchor/body drag session (the interaction.rs session pattern: the engine owns the
/// start snapshot and the math; hosts forward drag positions).
pub(crate) struct DrawingDrag {
    pub(crate) id: DrawingId,
    pub(crate) part: DrawingDragPart,
    /// Press point in pane-relative media px (x from the pane's left, y from the chart's top).
    pub(crate) start_x: f64,
    pub(crate) start_y: f64,
    /// Latest pointer position, used to rebase the interaction without changing the active drag.
    pub(crate) current_x: f64,
    pub(crate) current_y: f64,
    /// Anchor definitions at the current interaction baseline.
    pub(crate) start_points: Vec<DrawingPoint>,
    /// Anchors converted to media px at drag start (the body-drag translation base).
    pub(crate) start_px: Vec<(f64, f64)>,
    /// Original semantic snapshot retained for cancellation and the one committed history entry.
    pub(crate) history_points: Vec<DrawingPoint>,
    pub(crate) history_screen_position: (f64, f64),
    pub(crate) history_bars_pattern: Vec<BarsPatternBar>,
}

const DRAWING_HISTORY_LIMIT: usize = 100;

#[derive(Clone)]
enum DrawingCommand {
    Create {
        drawing: Drawing,
        index: usize,
    },
    Delete {
        drawing: Drawing,
        index: usize,
    },
    Update {
        before: Drawing,
        after: Box<Drawing>,
    },
    Clear {
        drawings: Vec<Drawing>,
    },
    Reorder {
        before: Vec<Drawing>,
        after: Vec<Drawing>,
    },
    BatchUpdate {
        before: Vec<Drawing>,
        after: Vec<Drawing>,
    },
}

impl DrawingCommand {
    fn rebase_logical_with<F>(&mut self, map: F)
    where
        F: Fn(f64) -> f64 + Copy,
    {
        match self {
            Self::Create { drawing, .. } | Self::Delete { drawing, .. } => {
                drawing.rebase_logical_with(map);
            }
            Self::Update { before, after } => {
                before.rebase_logical_with(map);
                after.rebase_logical_with(map);
            }
            Self::Clear { drawings } => {
                for drawing in drawings {
                    drawing.rebase_logical_with(map);
                }
            }
            Self::Reorder { before, after } => {
                for drawing in before.iter_mut().chain(after.iter_mut()) {
                    drawing.rebase_logical_with(map);
                }
            }
            Self::BatchUpdate { before, after } => {
                for drawing in before.iter_mut().chain(after.iter_mut()) {
                    drawing.rebase_logical_with(map);
                }
            }
        }
    }
}

/// Runtime-only, chart-local drawing history. Commands carry only the semantic drawing state
/// needed to reverse one committed action; previews, hit indexes, selection, and render caches
/// never enter the stack.
#[derive(Default)]
pub(crate) struct DrawingHistory {
    undo: Vec<DrawingCommand>,
    redo: Vec<DrawingCommand>,
}

impl DrawingHistory {
    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn rebase_logical_with<F>(&mut self, map: F)
    where
        F: Fn(f64) -> f64 + Copy,
    {
        for command in self.undo.iter_mut().chain(&mut self.redo) {
            command.rebase_logical_with(map);
        }
    }
}

/// Interactive creation in progress (the reference rectangle-drawing-tool's `_drawing` state,
/// engine-owned): committed anchors plus a preview point that follows the mouse until the kind's
/// anchor count is reached.
pub(crate) struct PendingDrawing {
    pub(crate) drawing: Drawing,
    pub(crate) preview: Option<DrawingPoint>,
    /// Optional pane selected when the host armed the tool. Legacy low-level creation leaves this
    /// `None` and binds to the first placed anchor.
    pub(crate) pane_constraint: Option<usize>,
}

/// Freehand brush capture in progress (the public reference's brush drag, engine-owned): the points
/// collected so far plus the options template for the committed drawing. Input is decimated by
/// distance on the way in (media px) and committed as-is — the stored path is exactly the curve
/// the live stroke painted.
pub(crate) struct BrushCapture {
    pub(crate) pane_index: usize,
    pub(crate) points: Vec<DrawingPoint>,
    /// The last captured point in media px (the decimation reference).
    pub(crate) last_px: (f64, f64),
    pub(crate) options: Drawing,
}

/// An armed drawing tool and its immutable-at-arm-time defaults.  Hosts choose the tool and may
/// update this template, but placement semantics stay engine-owned.  `pane_index` optionally pins
/// creation to one pane (browser split-grid/tool routing); `None` binds on the first point.
#[derive(Clone)]
pub(crate) struct ArmedDrawingTool {
    pub(crate) kind: DrawingKind,
    pub(crate) pane_index: Option<usize>,
    pub(crate) template: Drawing,
}

/// The transient Shift-click measure: a date-and-price range that follows the pointer, freezes
/// on release (after a drag) or on the next click, and is dismissed by the click after that or
/// by Escape. It never enters the committed drawings, history, persistence, or sync.
pub(crate) struct MeasureSession {
    /// Start/end anchors plus the bound pane and price scale, in drawing form so the frame and
    /// axis paths reuse the date-and-price range geometry unchanged.
    pub(crate) drawing: Drawing,
    /// Press position in pane-relative media px (the drag threshold reference).
    pub(crate) press: (f64, f64),
    /// Whether the end anchor still follows the pointer.
    pub(crate) following: bool,
}

/// All transient drawing-tool creation state.  Keeping arming, click/multi-click placement and
/// freehand capture under one owner prevents browser and native hosts from growing independent
/// per-tool state machines.
#[derive(Default)]
pub(crate) struct DrawingController {
    pub(crate) armed: Option<ArmedDrawingTool>,
    pub(crate) pending: Option<PendingDrawing>,
    pub(crate) brush: Option<BrushCapture>,
    pub(crate) measure: Option<MeasureSession>,
}

/// Result of forwarding a platform drawing-creation event into the engine.  Platform hosts use
/// this only for effects they alone can perform (pointer capture, repaint scheduling, opening a
/// text editor); tool behavior and committed state have already been resolved by the engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawingCreationUpdate {
    pub consumed: bool,
    pub changed: bool,
    pub created: Option<DrawingId>,
    pub request_text_edit: bool,
    pub pointer_capture: bool,
}

/// Minimum spacing between captured brush points in media px (input decimation — anything
/// closer is pointer noise, not intent).
pub(crate) const BRUSH_MIN_POINT_DISTANCE: f64 = 1.5;

/// Padding between a tool's reference box and its text label, in CSS px.
pub(crate) const TEXT_PAD: f64 = 4.0;

/// Gap between a measuring tool's measured area and its statistics label, in CSS px.
pub(crate) const MEASURE_LABEL_GAP: f64 = 6.0;
/// Conservative horizontal reach of a measuring label beyond its area for viewport culling.
const MEASURE_LABEL_REACH_X: f64 = 400.0;
/// A press-release farther than the shared 5 px Manhattan click slop is a drag-measure.
const MEASURE_DRAG_SLOP: f64 = 5.0;

/// reference `distanceToSegment` (renderers/hit-test-common.ts), duplicated from hit_test.rs so
/// the drawing geometry stays self-contained.
fn distance_to_segment(x: f64, y: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    if dx == 0.0 && dy == 0.0 {
        return (x - x1).hypot(y - y1);
    }
    let projection = ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy);
    let clamped = projection.clamp(0.0, 1.0);
    (x - (x1 + dx * clamped)).hypot(y - (y1 + dy * clamped))
}

fn point_in_polygon(point: (f64, f64), vertices: &[(f64, f64)]) -> bool {
    let mut inside = false;
    for index in 0..vertices.len() {
        let a = vertices[index];
        let b = vertices[(index + 1) % vertices.len()];
        if (a.1 > point.1) != (b.1 > point.1)
            && point.0 < (b.0 - a.0) * (point.1 - a.1) / (b.1 - a.1) + a.0
        {
            inside = !inside;
        }
    }
    inside
}

/// JSON patch accepted by [`ChartEngine::drawing_apply_options`] and carried by
/// `add_drawing`/creation. Snake_case keys are canonical (matching the TS `drawing_options`);
/// the reference camelCase forms are accepted as aliases. Every field is optional — absent keys
/// keep their current values (reference merge semantics).
#[derive(Clone, serde::Deserialize, Default)]
pub(crate) struct DrawingPatch {
    name: Option<String>,
    #[serde(alias = "groupId")]
    group_id: Option<String>,
    revision: Option<u64>,
    visible: Option<bool>,
    locked: Option<bool>,
    #[serde(alias = "zOrder")]
    z_order: Option<i32>,
    #[serde(alias = "intervalVisibility")]
    interval_visibility: Option<crate::DrawingIntervalVisibility>,
    #[serde(alias = "strokeStart")]
    stroke_start: Option<crate::DrawingLineCap>,
    #[serde(alias = "strokeEnd")]
    stroke_end: Option<crate::DrawingLineCap>,
    #[serde(alias = "extendLeft")]
    extend_left: Option<bool>,
    #[serde(alias = "extendRight")]
    extend_right: Option<bool>,
    #[serde(alias = "fillEnabled")]
    fill_enabled: Option<bool>,
    magnet: Option<crate::DrawingMagnetMode>,
    labels: Option<Vec<crate::DrawingLabelOptions>>,
    levels: Option<Vec<crate::DrawingLevel>>,
    gann_fans: Option<Vec<crate::DrawingLevel>>,
    gann_arcs: Option<Vec<crate::DrawingLevel>>,
    #[serde(alias = "levelReverse")]
    level_reverse: Option<bool>,
    #[serde(alias = "levelLogScale")]
    level_log_scale: Option<bool>,
    #[serde(alias = "levelShowPrices")]
    level_show_prices: Option<bool>,
    #[serde(alias = "levelShowValues")]
    level_show_values: Option<bool>,
    #[serde(alias = "levelShowPercents")]
    level_show_percents: Option<bool>,
    #[serde(alias = "levelLabelAlign")]
    level_label_align: Option<String>,
    #[serde(alias = "waveDegree")]
    wave_degree: Option<String>,
    #[serde(alias = "screenX")]
    screen_x: Option<f64>,
    #[serde(alias = "screenY")]
    screen_y: Option<f64>,
    #[serde(alias = "iconName")]
    icon_name: Option<String>,
    #[serde(alias = "iconSize")]
    icon_size: Option<f64>,
    #[serde(alias = "barsPatternMirrorX")]
    bars_pattern_mirror_x: Option<bool>,
    #[serde(alias = "barsPatternMirrorY")]
    bars_pattern_mirror_y: Option<bool>,
    #[serde(alias = "barsPatternMode")]
    bars_pattern_mode: Option<String>,
    #[serde(alias = "priceScaleId")]
    price_scale_id: Option<String>,
    color: Option<String>,
    width: Option<f64>,
    #[serde(alias = "lineStyle")]
    style: Option<serde_json::Value>,
    #[serde(alias = "fillColor")]
    fill_color: Option<String>,
    #[serde(alias = "previewFillColor")]
    preview_fill_color: Option<String>,
    #[serde(alias = "borderVisible")]
    border_visible: Option<bool>,
    #[serde(alias = "showLabels")]
    show_labels: Option<bool>,
    #[serde(alias = "axisBandsVisible")]
    axis_bands_visible: Option<bool>,
    #[serde(alias = "labelColor")]
    label_color: Option<String>,
    #[serde(alias = "labelTextColor")]
    label_text_color: Option<String>,
    #[serde(alias = "snapTimeToData")]
    snap_time_to_data: Option<bool>,
    text: Option<String>,
    #[serde(alias = "textColor")]
    text_color: Option<String>,
    #[serde(alias = "textSize")]
    text_size: Option<f64>,
    #[serde(alias = "textWeight")]
    text_weight: Option<u16>,
    #[serde(alias = "textItalic")]
    text_italic: Option<bool>,
    /// Legacy shorthand: `text_bold: true` maps to weight 700 when no explicit `text_weight` is
    /// given; `false` resets to normal (400).
    #[serde(alias = "textBold")]
    text_bold: Option<bool>,
    #[serde(
        alias = "textHAlign",
        alias = "text_horz_align",
        alias = "textHorzAlign"
    )]
    text_h_align: Option<String>,
    #[serde(
        alias = "textVAlign",
        alias = "text_vert_align",
        alias = "textVertAlign"
    )]
    text_v_align: Option<String>,
    #[serde(alias = "boxColor")]
    box_color: Option<String>,
    #[serde(alias = "boxBorderColor")]
    box_border_color: Option<String>,
    #[serde(alias = "boxBorderWidth")]
    box_border_width: Option<f64>,
    profile: Option<crate::ProfileDrawingOptions>,
    #[serde(alias = "positionAccountSize")]
    position_account_size: Option<f64>,
    #[serde(alias = "positionRiskPercent")]
    position_risk_percent: Option<f64>,
    #[serde(
        default,
        alias = "regressionSourceId",
        deserialize_with = "deserialize_regression_source_patch"
    )]
    regression_source_id: Option<Option<u32>>,
    #[serde(alias = "regressionDeviations")]
    regression_deviations: Option<f64>,
}

fn deserialize_regression_source_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    <Option<u32> as serde::Deserialize>::deserialize(deserializer).map(Some)
}

/// A patch's `style`: the TS string form (`solid`/`dotted`/`dashed`), or the reference numeric
/// enum for untyped callers. The retired names/values fold into their renamed equivalents
/// (large_dashed → dashed, sparse_dotted → dotted), as in price_line_api.rs.
fn parse_drawing_style(value: &serde_json::Value) -> Option<LineStyle> {
    match value {
        serde_json::Value::String(s) => Some(match s.as_str() {
            "dotted" | "sparse_dotted" => LineStyle::Dotted,
            "dashed" | "large_dashed" => LineStyle::Dashed,
            _ => LineStyle::Solid,
        }),
        serde_json::Value::Number(n) => n.as_u64().map(|v| line_style_from_u8(v as u8)),
        _ => None,
    }
}

fn style_name(style: LineStyle) -> &'static str {
    match style {
        LineStyle::Dotted => "dotted",
        LineStyle::Dashed => "dashed",
        LineStyle::Solid => "solid",
    }
}

/// Apply an optional CSS color input to a stored slot: `""` clears the override, a parseable
/// color pins the string verbatim, and an unparseable one is ignored — the price-line
/// keep/clear/pin contract (price_line_api.rs `update_css_slot`).
fn update_css_slot(slot: &mut Option<String>, value: String) {
    if value.is_empty() {
        *slot = None;
    } else if Color::parse_css(&value).is_some() {
        *slot = Some(value);
    }
}

impl Drawing {
    pub(crate) fn level_value(&self, value: f64) -> f64 {
        if !self.level_reverse {
            return value;
        }
        match self.kind {
            DrawingKind::GannFan if value > 0.0 => 1.0 / value,
            DrawingKind::FibonacciTimeZones | DrawingKind::FibonacciTrendTime => -value,
            _ => 1.0 - value,
        }
    }
    fn apply_patch(&mut self, patch: DrawingPatch) -> bool {
        if patch
            .position_account_size
            .is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 1e15)
            || patch
                .position_risk_percent
                .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
            || patch
                .regression_deviations
                .is_some_and(|value| !value.is_finite() || !(0.0..=10.0).contains(&value))
        {
            return false;
        }
        if let Some(name) = patch.name.as_ref()
            && name.len() > crate::MAX_DRAWING_NAME_BYTES
        {
            return false;
        }
        if let Some(group_id) = patch.group_id.as_ref()
            && group_id.len() > crate::MAX_DRAWING_GROUP_BYTES
        {
            return false;
        }
        if let Some(intervals) = patch.interval_visibility.as_ref()
            && !intervals.validate()
        {
            return false;
        }
        if let Some(labels) = patch.labels.as_ref()
            && (labels.len() > crate::MAX_DRAWING_LABELS
                || !labels.iter().all(|label| label.validate()))
        {
            return false;
        }
        if let Some(levels) = patch.levels.as_ref()
            && (levels.len() > crate::MAX_DRAWING_LEVELS
                || !levels.iter().all(|level| level.validate()))
        {
            return false;
        }
        for (family, fan) in [(&patch.gann_fans, true), (&patch.gann_arcs, false)] {
            if let Some(levels) = family
                && (!matches!(
                    self.kind,
                    DrawingKind::GannSquare | DrawingKind::GannSquareFixed
                ) || !valid_gann_family(levels, fan))
            {
                return false;
            }
        }
        if (patch.level_reverse.is_some()
            || patch.level_log_scale.is_some()
            || patch.level_show_prices.is_some()
            || patch.level_show_values.is_some()
            || patch.level_show_percents.is_some()
            || patch.level_label_align.is_some())
            && (!self.kind.has_levels()
                || (patch.level_log_scale == Some(true) && !self.kind.supports_log_levels())
                || patch
                    .level_label_align
                    .as_deref()
                    .is_some_and(|align| !matches!(align, "left" | "center" | "right")))
        {
            return false;
        }
        if let Some(degree) = patch.wave_degree.as_deref()
            && (!self.kind.is_elliott() || !DrawingKind::valid_wave_degree(degree))
        {
            return false;
        }
        if (patch.screen_x.is_some() || patch.screen_y.is_some())
            && (self.kind != DrawingKind::AnchoredText
                || patch
                    .screen_x
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                || patch
                    .screen_y
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)))
        {
            return false;
        }
        if (patch.icon_name.is_some() || patch.icon_size.is_some())
            && (self.kind != DrawingKind::IconStamp
                || patch
                    .icon_name
                    .as_ref()
                    .is_some_and(|name| name.len() > MAX_DRAWING_ICON_NAME_BYTES)
                || patch
                    .icon_size
                    .is_some_and(|size| !size.is_finite() || !(8.0..=96.0).contains(&size)))
        {
            return false;
        }
        if (patch.bars_pattern_mirror_x.is_some()
            || patch.bars_pattern_mirror_y.is_some()
            || patch.bars_pattern_mode.is_some())
            && (self.kind != DrawingKind::BarsPattern
                || patch.bars_pattern_mode.as_deref().is_some_and(|mode| {
                    !matches!(
                        mode,
                        "bars" | "line_open" | "line_high" | "line_low" | "line_close"
                    )
                }))
        {
            return false;
        }
        if let Some(name) = patch.name {
            self.name = name;
        }
        if let Some(group_id) = patch.group_id {
            self.group_id = (!group_id.is_empty()).then_some(group_id);
        }
        if let Some(revision) = patch.revision {
            self.revision = revision;
        }
        if let Some(visible) = patch.visible {
            self.visible = visible;
        }
        if let Some(locked) = patch.locked {
            self.locked = locked;
        }
        if let Some(z_order) = patch.z_order {
            self.z_order = z_order;
        }
        if let Some(interval_visibility) = patch.interval_visibility {
            self.interval_visibility = interval_visibility;
        }
        if let Some(stroke_start) = patch.stroke_start {
            self.stroke_start = stroke_start;
        }
        if let Some(stroke_end) = patch.stroke_end {
            self.stroke_end = stroke_end;
        }
        if let Some(extend_left) = patch.extend_left {
            self.extend_left = extend_left;
        }
        if let Some(extend_right) = patch.extend_right {
            self.extend_right = extend_right;
        }
        if let Some(fill_enabled) = patch.fill_enabled {
            self.fill_enabled = fill_enabled;
        }
        if let Some(magnet) = patch.magnet {
            self.magnet = magnet;
        }
        if let Some(labels) = patch.labels {
            self.labels = labels;
        }
        if let Some(levels) = patch.levels {
            self.levels = levels;
        }
        if let Some(levels) = patch.gann_fans {
            self.gann_fans = levels;
        }
        if let Some(levels) = patch.gann_arcs {
            self.gann_arcs = levels;
        }
        if let Some(value) = patch.level_reverse {
            self.level_reverse = value;
        }
        if let Some(value) = patch.level_log_scale {
            self.level_log_scale = value;
        }
        if let Some(value) = patch.level_show_prices {
            self.level_show_prices = value;
        }
        if let Some(value) = patch.level_show_values {
            self.level_show_values = value;
        }
        if let Some(value) = patch.level_show_percents {
            self.level_show_percents = value;
        }
        if let Some(value) = patch.level_label_align {
            self.level_label_align = value;
        }
        if let Some(degree) = patch.wave_degree {
            self.wave_degree = degree;
        }
        if let Some(value) = patch.screen_x {
            self.screen_x = value;
        }
        if let Some(value) = patch.screen_y {
            self.screen_y = value;
        }
        if let Some(name) = patch.icon_name {
            self.icon_name = (!name.is_empty()).then_some(name);
        }
        if let Some(size) = patch.icon_size {
            self.icon_size = size;
        }
        if let Some(value) = patch.bars_pattern_mirror_x {
            self.bars_pattern_mirror_x = value;
        }
        if let Some(value) = patch.bars_pattern_mirror_y {
            self.bars_pattern_mirror_y = value;
        }
        if let Some(mode) = patch.bars_pattern_mode {
            self.bars_pattern_mode = mode;
        }
        if let Some(scale) = patch
            .price_scale_id
            .as_deref()
            .and_then(DrawingPriceScale::from_name)
        {
            self.price_scale = scale;
        }
        if let Some(css) = patch.color
            && Color::parse_css(&css).is_some()
        {
            self.color = css;
        }
        if let Some(width) = patch.width
            && width.is_finite()
            && width > 0.0
        {
            self.width = width;
        }
        if let Some(style) = patch.style.as_ref().and_then(parse_drawing_style) {
            self.style = style;
        }
        if let Some(css) = patch.fill_color {
            update_css_slot(&mut self.fill_color, css);
        }
        if let Some(css) = patch.preview_fill_color {
            update_css_slot(&mut self.preview_fill_color, css);
        }
        if let Some(visible) = patch.border_visible {
            self.border_visible = visible;
        }
        if let Some(visible) = patch.show_labels {
            self.show_labels = visible;
        }
        if let Some(visible) = patch.axis_bands_visible {
            self.axis_bands_visible = visible;
        }
        if let Some(css) = patch.label_color {
            update_css_slot(&mut self.label_color, css);
        }
        if let Some(css) = patch.label_text_color {
            update_css_slot(&mut self.label_text_color, css);
        }
        if let Some(snap) = patch.snap_time_to_data {
            self.snap_time_to_data = snap;
        }
        if let Some(text) = patch.text {
            self.text = text;
        }
        if let Some(css) = patch.text_color {
            update_css_slot(&mut self.text_color, css);
        }
        if let Some(size) = patch.text_size
            && size.is_finite()
            && size > 0.0
        {
            self.text_size = Some(size);
        }
        // Explicit weight wins; the legacy `text_bold` shorthand maps onto it (true → 700,
        // false → normal).
        if let Some(weight) = patch.text_weight {
            if (100..=900).contains(&weight) {
                self.text_weight = Some(weight);
            }
        } else if let Some(bold) = patch.text_bold {
            self.text_weight = bold.then_some(700);
        }
        if let Some(italic) = patch.text_italic {
            self.text_italic = italic;
        }
        if let Some(align) = patch
            .text_h_align
            .as_deref()
            .and_then(DrawingTextHAlign::from_name)
        {
            self.text_h_align = align;
        }
        if let Some(align) = patch
            .text_v_align
            .as_deref()
            .and_then(DrawingTextVAlign::from_name)
        {
            self.text_v_align = align;
        }
        if let Some(css) = patch.box_color {
            update_css_slot(&mut self.box_color, css);
        }
        if let Some(css) = patch.box_border_color {
            update_css_slot(&mut self.box_border_color, css);
        }
        if let Some(width) = patch.box_border_width
            && width.is_finite()
            && width > 0.0
        {
            self.box_border_width = width;
        }
        if let Some(value) = patch.position_account_size {
            self.position_account_size = value;
        }
        if let Some(value) = patch.position_risk_percent {
            self.position_risk_percent = value;
        }
        if let Some(source_id) = patch.regression_source_id {
            self.regression_source_id = source_id;
        }
        if let Some(deviations) = patch.regression_deviations {
            self.regression_deviations = deviations;
        }
        if let Some(profile) = patch.profile {
            self.profile = Some(profile);
        }
        true
    }

    fn options_json(&self) -> serde_json::Value {
        let mut options = serde_json::json!({
            "name": self.name,
            "group_id": self.group_id.as_deref().unwrap_or(""),
            "revision": self.revision,
            "visible": self.visible,
            "locked": self.locked,
            "z_order": self.z_order,
            "interval_visibility": self.interval_visibility,
            "stroke_start": self.stroke_start,
            "stroke_end": self.stroke_end,
            "extend_left": self.extend_left,
            "extend_right": self.extend_right,
            "fill_enabled": self.fill_enabled,
            "magnet": self.magnet,
            "labels": self.labels,
            "levels": self.levels,
            "price_scale_id": self.price_scale.name(),
            "profile": self.profile,
            "position_account_size": self.position_account_size,
            "position_risk_percent": self.position_risk_percent,
            "color": self.color,
            "width": self.width,
            "style": style_name(self.style),
            "fill_color": self.fill_color.as_deref().unwrap_or(""),
            "preview_fill_color": self.preview_fill_color.as_deref().unwrap_or(""),
            "border_visible": self.border_visible,
            "show_labels": self.show_labels,
            "axis_bands_visible": self.axis_bands_visible,
            "label_color": self.label_color.as_deref().unwrap_or(""),
            "label_text_color": self.label_text_color.as_deref().unwrap_or(""),
            "snap_time_to_data": self.snap_time_to_data,
            "text": self.text,
            "text_color": self.text_color.as_deref().unwrap_or(""),
            "text_size": self.text_size,
            "text_weight": self.text_weight,
            "text_italic": self.text_italic,
            "text_bold": self.text_weight.unwrap_or(400) >= 600,
            "text_h_align": self.text_h_align.name(),
            "text_v_align": self.text_v_align.name(),
            "box_color": self.box_color.as_deref().unwrap_or(""),
            "box_border_color": self.box_border_color.as_deref().unwrap_or(""),
            "box_border_width": self.box_border_width,
        });
        if self.kind == DrawingKind::RegressionTrend {
            options["regression_source_id"] = serde_json::json!(self.regression_source_id);
            options["regression_deviations"] = serde_json::json!(self.regression_deviations);
        }
        if self.kind.has_levels() {
            options["level_reverse"] = serde_json::json!(self.level_reverse);
            options["level_log_scale"] = serde_json::json!(self.level_log_scale);
            options["level_show_prices"] = serde_json::json!(self.level_show_prices);
            options["level_show_values"] = serde_json::json!(self.level_show_values);
            options["level_show_percents"] = serde_json::json!(self.level_show_percents);
            options["level_label_align"] = serde_json::json!(self.level_label_align);
        }
        if matches!(
            self.kind,
            DrawingKind::GannSquare | DrawingKind::GannSquareFixed
        ) {
            options["gann_fans"] = serde_json::json!(self.gann_fans);
            options["gann_arcs"] = serde_json::json!(self.gann_arcs);
        }
        if self.kind.is_elliott() {
            options["wave_degree"] = serde_json::json!(self.wave_degree);
        }
        if self.kind == DrawingKind::AnchoredText {
            options["screen_x"] = serde_json::json!(self.screen_x);
            options["screen_y"] = serde_json::json!(self.screen_y);
        }
        if self.kind == DrawingKind::IconStamp {
            options["icon_name"] = serde_json::json!(self.icon_name);
            options["icon_size"] = serde_json::json!(self.icon_size);
        }
        if self.kind == DrawingKind::BarsPattern {
            options["bars_pattern_mirror_x"] = serde_json::json!(self.bars_pattern_mirror_x);
            options["bars_pattern_mirror_y"] = serde_json::json!(self.bars_pattern_mirror_y);
            options["bars_pattern_mode"] = serde_json::json!(self.bars_pattern_mode);
        }
        options
    }
}

/// The label's reference box in the caller's coordinate units (media px at hit-test time,
/// bitmap px at render): the tool's bounding geometry the 3×3 alignment resolves against.
/// Horizontal lines span the pane (or the ray's extent); a vertical line spans the pane's
/// height; the text tool's box degenerates to its anchor point.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TextBox {
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
}

impl ChartEngine {
    /// Set host-owned interval metadata used by per-drawing visibility ranges.  The engine does
    /// not infer a timeframe from provider or browser state.
    pub fn set_drawing_interval(&mut self, interval: Option<crate::DrawingInterval>) {
        if interval.as_ref().is_some_and(|value| !value.validate()) {
            return;
        }
        if self.drawing_interval != interval {
            self.drawing_interval = interval;
            self.invalidate_frame_drawings();
        }
    }

    pub fn drawing_is_visible(&self, id: DrawingId) -> bool {
        self.drawing(id).is_some_and(|drawing| {
            drawing.visible && drawing.interval_visibility.allows(self.drawing_interval)
        })
    }

    fn rebase_drawing_logicals_with<F>(&mut self, map: F)
    where
        F: Fn(f64) -> f64 + Copy,
    {
        let mut committed_changed = false;
        for drawing in &mut self.drawings {
            committed_changed |= drawing.rebase_logical_with(map);
        }
        let mut transient_changed = false;
        if let Some(pending) = self.drawing_controller.pending.as_mut() {
            transient_changed |= pending.drawing.rebase_logical_with(map);
            if let Some(preview) = pending.preview.as_mut() {
                transient_changed |= rebase_points_with(std::slice::from_mut(preview), map);
            }
        }
        if let Some(capture) = self.drawing_controller.brush.as_mut() {
            transient_changed |= rebase_points_with(&mut capture.points, map);
            transient_changed |= capture.options.rebase_logical_with(map);
        }
        if let Some(session) = self.drawing_controller.measure.as_mut() {
            transient_changed |= session.drawing.rebase_logical_with(map);
        }
        if let Some(drag) = self.drawing_drag.as_mut() {
            transient_changed |= rebase_points_with(&mut drag.start_points, map);
            transient_changed |= rebase_points_with(&mut drag.history_points, map);
        }
        self.drawing_history.rebase_logical_with(map);

        if committed_changed {
            self.drawing_runtime
                .borrow_mut()
                .rebuild_all(&self.drawings, self.panes.len());
        }
        if committed_changed || transient_changed {
            self.invalidate_frame_drawings();
        }
    }

    pub(crate) fn rebase_drawing_logicals(&mut self, mapping: &MergedTimeMapping) {
        self.rebase_drawing_logicals_with(|logical| mapping.map_logical(logical));
    }

    pub(crate) fn rebase_drawing_logicals_sequence(&mut self, mapping: &BarSequenceMapping) {
        self.rebase_drawing_logicals_with(|logical| mapping.map_logical(logical));
    }

    pub(crate) fn refresh_drawing_pixel_baselines(&mut self) {
        let drag_baseline = self.drawing_drag.as_ref().and_then(|drag| {
            let drawing = self.drawing(drag.id)?;
            let points = drawing.points.clone();
            let px = self.drawing_px(drawing)?;
            Some((points, px, drag.current_x, drag.current_y))
        });
        if let (Some(drag), Some((start_points, start_px, pointer_x, pointer_y))) =
            (self.drawing_drag.as_mut(), drag_baseline)
        {
            drag.start_x = pointer_x;
            drag.start_y = pointer_y;
            drag.start_points = start_points;
            drag.start_px = start_px;
        }

        let brush_px =
            self.drawing_controller.brush.as_ref().and_then(|capture| {
                self.drawing_to_px(capture.pane_index, *capture.points.last()?)
            });
        if let (Some(capture), Some(last_px)) = (self.drawing_controller.brush.as_mut(), brush_px) {
            capture.last_px = last_px;
        }
    }

    fn record_drawing_command(&mut self, command: DrawingCommand) {
        if self.drawing_history.undo.len() == DRAWING_HISTORY_LIMIT {
            self.drawing_history.undo.remove(0);
        }
        self.drawing_history.undo.push(command);
        self.drawing_history.redo.clear();
        self.drawing_revision = self.drawing_revision.wrapping_add(1);
    }

    /// Monotonic revision of committed drawing semantics: every recorded create, delete, anchor,
    /// style, lock, text, or clear operation and every undo/redo step advances it. Hover,
    /// selection, previews, and in-flight drags do not. Hosts persist when it changes.
    pub fn drawing_revision(&self) -> u64 {
        self.drawing_revision
    }

    fn bump_drawing_sync_revision(&mut self) {
        self.drawing_sync_revision = self.drawing_sync_revision.wrapping_add(1).max(1);
    }

    fn insert_drawing_snapshot(&mut self, drawing: Drawing, index: usize) {
        let id = drawing.id;
        let index = index.min(self.drawings.len());
        self.drawings.insert(index, drawing);
        self.drawing_runtime
            .borrow_mut()
            .rebuild_panes(&self.drawings, self.panes.len());
        self.selected_drawing = Some(id);
    }

    fn remove_drawing_snapshot(&mut self, id: DrawingId) -> Option<(Drawing, usize)> {
        let index = self.drawings.iter().position(|drawing| drawing.id == id)?;
        let drawing = self.drawings.remove(index);
        self.drawing_anchor_times.remove(&id);
        self.drawing_runtime.borrow_mut().remove(id, &self.drawings);
        if self.selected_drawing == Some(id) {
            self.selected_drawing = None;
        }
        if self.drawing_drag.as_ref().is_some_and(|drag| drag.id == id) {
            self.drawing_drag = None;
        }
        if self.editing_drawing == Some(id) {
            self.editing_drawing = None;
            self.drawing_text_edit = None;
        }
        if self.hovered_text == Some(id) {
            self.hovered_text = None;
        }
        if self.hovered_drawing == Some(id) {
            self.hovered_drawing = None;
        }
        Some((drawing, index))
    }

    fn replace_drawing_snapshot(&mut self, snapshot: &Drawing) -> bool {
        let Some(drawing) = self
            .drawings
            .iter_mut()
            .find(|drawing| drawing.id == snapshot.id)
        else {
            return false;
        };
        *drawing = snapshot.clone();
        self.drawing_anchor_times.remove(&snapshot.id);
        self.update_drawing_runtime(snapshot.id);
        true
    }

    fn apply_drawing_command(&mut self, command: &DrawingCommand, undo: bool) {
        match command {
            DrawingCommand::Create { drawing, index } => {
                if undo {
                    self.remove_drawing_snapshot(drawing.id);
                } else {
                    self.insert_drawing_snapshot(drawing.clone(), *index);
                }
            }
            DrawingCommand::Delete { drawing, index } => {
                if undo {
                    self.insert_drawing_snapshot(drawing.clone(), *index);
                } else {
                    self.remove_drawing_snapshot(drawing.id);
                }
            }
            DrawingCommand::Update { before, after } => {
                self.replace_drawing_snapshot(if undo { before } else { after.as_ref() });
            }
            DrawingCommand::Clear { drawings } => {
                if undo {
                    self.drawings = drawings.clone();
                    self.drawing_runtime
                        .borrow_mut()
                        .rebuild_all(&self.drawings, self.panes.len());
                } else {
                    self.drawings.clear();
                    self.drawing_runtime.borrow_mut().clear();
                }
                self.selected_drawing = None;
                self.drawing_drag = None;
                self.editing_drawing = None;
                self.drawing_text_edit = None;
                self.hovered_drawing = None;
                self.hovered_text = None;
            }
            DrawingCommand::Reorder { before, after } => {
                self.drawings = if undo { before.clone() } else { after.clone() };
                self.drawing_runtime
                    .borrow_mut()
                    .rebuild_all(&self.drawings, self.panes.len());
            }
            DrawingCommand::BatchUpdate { before, after } => {
                self.drawings = if undo { before.clone() } else { after.clone() };
                self.drawing_runtime
                    .borrow_mut()
                    .rebuild_all(&self.drawings, self.panes.len());
            }
        }
    }

    /// Undo one committed drawing-semantic operation for this chart only.
    pub fn undo_drawing(&mut self) -> bool {
        let Some(command) = self.drawing_history.undo.pop() else {
            return false;
        };
        self.invalidate_frame_drawings();
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
        self.apply_drawing_command(&command, true);
        self.bump_drawing_sync_revision();
        self.drawing_revision = self.drawing_revision.wrapping_add(1);
        self.drawing_history.redo.push(command);
        true
    }

    /// Redo one previously undone drawing-semantic operation for this chart only.
    pub fn redo_drawing(&mut self) -> bool {
        let Some(command) = self.drawing_history.redo.pop() else {
            return false;
        };
        self.invalidate_frame_drawings();
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
        self.apply_drawing_command(&command, false);
        self.bump_drawing_sync_revision();
        self.drawing_revision = self.drawing_revision.wrapping_add(1);
        self.drawing_history.undo.push(command);
        true
    }

    pub fn can_undo_drawing(&self) -> bool {
        !self.drawing_history.undo.is_empty()
    }

    pub fn can_redo_drawing(&self) -> bool {
        !self.drawing_history.redo.is_empty()
    }

    // --- store access ---

    /// Every live drawing in z-order (bottom first — a later entry overpaints the earlier ones
    /// within its pane).
    pub fn drawings(&self) -> &[Drawing] {
        &self.drawings
    }

    pub fn drawing(&self, id: DrawingId) -> Option<&Drawing> {
        self.drawings.iter().find(|d| d.id == id)
    }

    /// Whether any drawing exists (the host hover pipeline's cheap gate).
    pub fn has_drawings(&self) -> bool {
        !self.drawings.is_empty() || self.selected_drawing.is_some()
    }

    // --- coordinate conversion (media px: x pane-relative, y chart-top) ---

    /// The pane's right scale for drawing conversion, `None` for a stale pane index (the pane
    /// was removed after the drawing was placed — like a pane-less series, it draws nowhere).
    pub(crate) fn drawing_scale(&self, pane_index: usize) -> Option<&PriceScaleCore> {
        self.drawing_scale_for(pane_index, DrawingPriceScale::Right)
    }

    pub(crate) fn drawing_scale_for(
        &self,
        pane_index: usize,
        target: DrawingPriceScale,
    ) -> Option<&PriceScaleCore> {
        let pane = self.panes.get(pane_index)?;
        let scale = match target {
            DrawingPriceScale::Right => &pane.price_scale,
            DrawingPriceScale::Left => &pane.left_scale,
            DrawingPriceScale::Overlay => &pane.overlay_scale,
        };
        (!scale.is_empty()).then_some(scale)
    }

    /// The pane's primary series' base value for percentage/indexed scale modes — the first
    /// visible, non-overlay series bound to the right scale (mirrors the pane-primitive
    /// converters' `pane_scale_base_value`, chart/primitives.rs). Unused by normal/log scales.
    pub(crate) fn drawing_scale_base(&self, pane_index: usize) -> f64 {
        self.drawing_scale_base_for(pane_index, DrawingPriceScale::Right)
    }

    pub(crate) fn drawing_scale_base_for(
        &self,
        pane_index: usize,
        target: DrawingPriceScale,
    ) -> f64 {
        let target = match target {
            DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        self.visible_range()
            .and_then(|(from, _)| {
                let series = self.series.iter().find(|s| {
                    s.visible
                        && !s.removed
                        && s.pane_index == pane_index
                        && crate::frame::series_scale_target(s) == target
                })?;
                self.series_base_value(series.id, from)
            })
            .unwrap_or(0.0)
    }

    /// Media px for an anchor on `pane_index`, or `None` when the pane/scale cannot place it
    /// (stale pane, empty scale, no time points for the x conversion).
    pub(crate) fn drawing_to_px(
        &self,
        pane_index: usize,
        point: DrawingPoint,
    ) -> Option<(f64, f64)> {
        self.drawing_to_px_for(pane_index, DrawingPriceScale::Right, point)
    }

    pub(crate) fn drawing_to_px_for(
        &self,
        pane_index: usize,
        target: DrawingPriceScale,
        point: DrawingPoint,
    ) -> Option<(f64, f64)> {
        let scale = self.drawing_scale_for(pane_index, target)?;
        if self.data.merged_times().is_empty() {
            return None;
        }
        let base = self.drawing_scale_base_for(pane_index, target);
        Some((
            self.time_scale.logical_to_coordinate(point.logical),
            scale.price_to_coordinate(point.price, base),
        ))
    }

    /// The anchor under a media-px position on `pane_index` (the drag/creation conversion).
    fn drawing_from_px(&self, pane_index: usize, x: f64, y: f64) -> Option<DrawingPoint> {
        self.drawing_from_px_for(pane_index, DrawingPriceScale::Right, x, y)
    }

    fn drawing_from_px_for(
        &self,
        pane_index: usize,
        target: DrawingPriceScale,
        x: f64,
        y: f64,
    ) -> Option<DrawingPoint> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let scale = self.drawing_scale_for(pane_index, target)?;
        if self.data.merged_times().is_empty() {
            return None;
        }
        let base = self.drawing_scale_base_for(pane_index, target);
        Some(DrawingPoint {
            // The time scale's float-index space is offset a half bar from its logical space
            // (bar `i` owns float indexes `(i-1, i]` — coordinate_to_index ceils), so the
            // inverse of `logical_to_coordinate` is `coordinate_to_float_index + 0.5`.
            logical: self.time_scale.coordinate_to_float_index(x) + 0.5,
            price: scale.coordinate_to_price(y, base),
        })
    }

    /// Position levels use the instrument's orderable grid, or their bound scale's display tick.
    /// Resolve this in price space: a tick can be much smaller than a device pixel.
    pub(crate) fn position_price_tick(
        &self,
        pane_index: usize,
        price_scale: DrawingPriceScale,
    ) -> Option<f64> {
        let target = match price_scale {
            DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        self.trading_state
            .instrument
            .tick_size
            .or_else(|| {
                self.scale_formatter_source(pane_index, target)
                    .map(|series| series.price_format.min_move)
            })
            .filter(|tick| tick.is_finite() && *tick > 0.0)
    }

    fn snap_position_price(
        &self,
        pane_index: usize,
        price_scale: DrawingPriceScale,
        price: f64,
    ) -> f64 {
        let Some(tick) = self.position_price_tick(pane_index, price_scale) else {
            return price;
        };
        let snapped = (price / tick).round() * tick;
        if snapped.is_finite() { snapped } else { price }
    }

    /// Grid-snapped tools place anchors on the crosshair's time slot under `x` (unless the magnet
    /// already chose a bar) and on the price tick grid.
    fn grid_snap_point(
        &self,
        kind: DrawingKind,
        pane_index: usize,
        price_scale: DrawingPriceScale,
        x: f64,
        mut point: DrawingPoint,
        magnet: bool,
    ) -> DrawingPoint {
        if !kind.spec().grid_snap {
            return point;
        }
        if !magnet {
            point.logical = self.snapped_crosshair_index(x) as f64;
        }
        point.price = self.snap_position_price(pane_index, price_scale, point.price);
        point
    }

    fn snap_drawing_time_to_data(&self, mut point: DrawingPoint) -> Option<DrawingPoint> {
        let logical = point.logical.round() as i64;
        if logical < 0 || logical as usize >= self.data.merged_times().len() {
            return None;
        }
        point.logical = logical as f64;
        Some(point)
    }

    /// Reference-informed magnet behavior (Ctrl held): resolve the live pointer through the same pixel-space
    /// rendered-price candidate path as the crosshair, then encode the winning coordinate on the
    /// drawing's own price scale. A bar with no visible real candidate keeps the unsnapped point.
    fn magnet_snap_point_at(
        &self,
        pane_index: usize,
        price_scale: DrawingPriceScale,
        x: f64,
        y: f64,
        point: DrawingPoint,
    ) -> DrawingPoint {
        let Some((logical, snapped_y)) = self.magnet_snap_coordinate(pane_index, x, y, true) else {
            return point;
        };
        let Some(mut snapped) = self.drawing_from_px_for(pane_index, price_scale, x, snapped_y)
        else {
            return point;
        };
        snapped.logical = logical as f64;
        snapped
    }

    /// Reference-informed straighten behavior (Shift held): recompute the dragged anchor of a two-anchor tool
    /// so the segment from the FIXED anchor snaps to the nearest 0°/45°/90° direction at the
    /// dragged pixel distance; a rectangle instead becomes a square (the larger dragged side
    /// wins, the drag quadrant's signs kept). Angles are visual, so the math runs in media-px
    /// space (screen coordinates, y down — the 45° snap set is symmetric). One-anchor kinds
    /// pass through (already straight). `None` when either anchor fails to convert.
    fn straighten_point(
        &self,
        pane_index: usize,
        price_scale: DrawingPriceScale,
        kind: DrawingKind,
        fixed: DrawingPoint,
        dragged: DrawingPoint,
    ) -> Option<DrawingPoint> {
        let (fx, fy) = self.drawing_to_px_for(pane_index, price_scale, fixed)?;
        let (dx, dy) = self.drawing_to_px_for(pane_index, price_scale, dragged)?;
        let (mut vx, mut vy) = (dx - fx, dy - fy);
        match kind.spec().straighten {
            DrawingStraightenMode::Segment45 => {
                let distance = vx.hypot(vy);
                if distance == 0.0 {
                    return Some(dragged);
                }
                let angle = vy.atan2(vx);
                let snapped =
                    (angle / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
                vx = distance * snapped.cos();
                vy = distance * snapped.sin();
            }
            DrawingStraightenMode::Square => {
                let side = vx.abs().max(vy.abs());
                vx = if vx < 0.0 { -side } else { side };
                vy = if vy < 0.0 { -side } else { side };
            }
            DrawingStraightenMode::None => return Some(dragged),
        }
        self.drawing_from_px_for(pane_index, price_scale, fx + vx, fy + vy)
    }

    /// The anchors of `drawing` in media px (`None` when any anchor fails to resolve), with the
    /// pending preview appended when `preview` holds (the interactive-creation geometry).
    pub(crate) fn drawing_px(&self, drawing: &Drawing) -> Option<Vec<(f64, f64)>> {
        if drawing.kind == DrawingKind::AnchoredText {
            let pane = self.panes.get(drawing.pane_index)?;
            return Some(vec![(
                drawing.screen_x * self.pane_w,
                pane.top + drawing.screen_y * pane.height,
            )]);
        }
        drawing
            .points
            .iter()
            .map(|&point| self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point))
            .collect()
    }

    fn regression_source_id(&self, drawing: &Drawing) -> Option<u32> {
        let target = match drawing.price_scale {
            DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        drawing
            .regression_source_id
            .and_then(|id| {
                self.series
                    .iter()
                    .any(|series| {
                        series.id == id
                            && !series.removed
                            && series.pane_index == drawing.pane_index
                            && series.price_scale_target == target
                    })
                    .then_some(id)
            })
            .or_else(|| {
                if drawing.regression_source_id.is_some() {
                    return None;
                }
                self.primary_series_on_price_scale(drawing.pane_index, target)
                    .map(|source| source.series_id)
            })
    }

    /// Forecast result from actual source highs/lows between entry and the declared horizon.
    /// `None` remains pending while the horizon is in the future or source data is unavailable.
    pub(crate) fn forecast_result(&self, drawing: &Drawing) -> Option<bool> {
        if drawing.kind != DrawingKind::Forecast || drawing.points.len() != 2 {
            return None;
        }
        let [entry, target] = drawing.points.as_slice() else {
            return None;
        };
        if target.logical <= entry.logical {
            return None;
        }
        let source = self.regression_source_id(drawing)?;
        let (times, columns) = self.data.series_data(source)?;
        let merged = self.data.merged_times();
        let first = entry.logical.ceil().max(0.0) as usize;
        let last = target
            .logical
            .floor()
            .min((merged.len().saturating_sub(1)) as f64) as usize;
        if first >= merged.len() || last < first {
            return None;
        }
        let mut row = times.partition_point(|time| *time < merged[first]);
        for &time in &merged[first..=last] {
            while row < times.len() && times[row] < time {
                row += 1;
            }
            if row < times.len() && times[row] == time {
                let extremum = if target.price >= entry.price {
                    columns[1][row]
                } else {
                    columns[2][row]
                };
                if extremum.is_finite()
                    && if target.price >= entry.price {
                        extremum >= target.price
                    } else {
                        extremum <= target.price
                    }
                {
                    return Some(true);
                }
            }
        }
        (merged.len().saturating_sub(1) as f64 >= target.logical).then_some(false)
    }

    fn capture_bars_pattern(&self, drawing: &Drawing) -> Option<Vec<BarsPatternBar>> {
        if drawing.kind != DrawingKind::BarsPattern || drawing.points.len() != 3 {
            return None;
        }
        let source = self.regression_source_id(drawing)?;
        let (times, columns) = self.data.series_data(source)?;
        let merged = self.data.merged_times();
        let first = drawing.points[0]
            .logical
            .min(drawing.points[1].logical)
            .ceil()
            .max(0.0) as usize;
        let last = drawing.points[0]
            .logical
            .max(drawing.points[1].logical)
            .floor() as usize;
        if first >= merged.len()
            || last >= merged.len()
            || last < first
            || last - first >= MAX_BARS_PATTERN_BARS
        {
            return None;
        }
        let mut row = times.partition_point(|time| *time < merged[first]);
        let mut bars = Vec::with_capacity(last - first + 1);
        for (offset, &time) in merged[first..=last].iter().enumerate() {
            while row < times.len() && times[row] < time {
                row += 1;
            }
            if row < times.len() && times[row] == time {
                let bar = BarsPatternBar {
                    offset: offset as u16,
                    open: columns[0][row],
                    high: columns[1][row],
                    low: columns[2][row],
                    close: columns[3][row],
                };
                if bar.valid() {
                    bars.push(bar);
                }
            }
        }
        (!bars.is_empty()).then_some(bars)
    }

    /// Least-squares center and residual-deviation boundaries over source closes at the chart's
    /// canonical merged slots. Sparse/whitespace rows do not contribute; the two defining
    /// anchors select the logical window and remain editable independently of computed prices.
    fn regression_points(&self, drawing: &Drawing) -> Option<[DrawingPoint; 6]> {
        if drawing.kind != DrawingKind::RegressionTrend {
            return None;
        }
        let [start, end] = drawing.points.as_slice() else {
            return None;
        };
        let source = self.regression_source_id(drawing)?;
        let (times, columns) = self.data.series_data(source)?;
        let closes = columns[3];
        let merged = self.data.merged_times();
        let lo = start.logical.min(end.logical).ceil().max(0.0) as usize;
        let hi = start.logical.max(end.logical).floor() as usize;
        if lo >= merged.len() || hi <= lo {
            return None;
        }
        let hi = hi.min(merged.len() - 1);
        let mut row = times.partition_point(|time| *time < merged[lo]);
        let mut count = 0.0;
        let (mut sum_x, mut sum_y, mut sum_xx, mut sum_xy) = (0.0, 0.0, 0.0, 0.0);
        for (offset, &time) in merged[lo..=hi].iter().enumerate() {
            while row < times.len() && times[row] < time {
                row += 1;
            }
            if row < times.len() && times[row] == time {
                let value = closes[row];
                if value.is_finite() {
                    let x = offset as f64;
                    count += 1.0;
                    sum_x += x;
                    sum_y += value;
                    sum_xx += x * x;
                    sum_xy += x * value;
                }
            }
        }
        if count < 2.0 {
            return None;
        }
        let denominator = count * sum_xx - sum_x * sum_x;
        if denominator <= f64::EPSILON {
            return None;
        }
        let slope = (count * sum_xy - sum_x * sum_y) / denominator;
        let intercept = (sum_y - slope * sum_x) / count;
        let mut residual_sum = 0.0;
        row = times.partition_point(|time| *time < merged[lo]);
        for (offset, &time) in merged[lo..=hi].iter().enumerate() {
            while row < times.len() && times[row] < time {
                row += 1;
            }
            if row < times.len() && times[row] == time {
                let value = closes[row];
                if value.is_finite() {
                    let residual = value - (intercept + slope * offset as f64);
                    residual_sum += residual * residual;
                }
            }
        }
        let deviation = drawing.regression_deviations * (residual_sum / count).sqrt();
        let center = |logical: f64| intercept + slope * (logical - lo as f64);
        let make = |logical: f64, offset: f64| DrawingPoint {
            logical,
            price: center(logical) + offset,
        };
        Some([
            make(start.logical, 0.0),
            make(end.logical, 0.0),
            make(start.logical, deviation),
            make(end.logical, deviation),
            make(start.logical, -deviation),
            make(end.logical, -deviation),
        ])
    }

    pub(crate) fn drawing_render_px(&self, drawing: &Drawing) -> Option<Vec<(f64, f64)>> {
        let mut px = self.drawing_px(drawing)?;
        if drawing.kind == DrawingKind::RegressionTrend
            && let Some(derived) = self.regression_points(drawing)
        {
            for point in derived {
                px.push(self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)?);
            }
        }
        Some(px)
    }

    pub(crate) fn drawing_coordinate_key(&self, drawing: &Drawing) -> Option<[u64; 12]> {
        let pane = self.panes.get(drawing.pane_index)?;
        let scale = self.drawing_scale_for(drawing.pane_index, drawing.price_scale)?;
        let range = scale.price_range_for_api()?;
        let base = self.drawing_scale_base_for(drawing.pane_index, drawing.price_scale);
        let midpoint = (range.min_value() + range.max_value()) / 2.0;
        Some([
            scale.mode() as u64
                | (u64::from(scale.is_inverted()) << 8)
                | ((drawing.price_scale as u64) << 16),
            self.time_scale.logical_to_coordinate(0.0).to_bits(),
            self.time_scale.logical_to_coordinate(1.0).to_bits(),
            range.min_value().to_bits(),
            range.max_value().to_bits(),
            base.to_bits(),
            pane.top.to_bits(),
            pane.height.to_bits(),
            self.pane_w.to_bits(),
            scale.price_to_coordinate(midpoint, base).to_bits(),
            self.dpr.to_bits()
                ^ if drawing.kind == DrawingKind::RegressionTrend {
                    self.regression_source_id(drawing)
                        .map(|source| {
                            (u64::from(source) << 32)
                                ^ self.data.series_generation(source).unwrap_or(0)
                        })
                        .unwrap_or(0)
                        .rotate_left(17)
                } else {
                    0
                },
            self.options.generation(),
        ])
    }

    fn refresh_drawing_screen_bounds(
        &self,
        drawing: &Drawing,
        entry: &mut DrawingCache,
        key: [u64; 12],
        base: f64,
        text_metrics: Option<(f64, f64)>,
    ) -> bool {
        if entry.screen_valid && entry.bounds_key == key {
            return true;
        }
        let Some(scale) = self.drawing_scale_for(drawing.pane_index, drawing.price_scale) else {
            return false;
        };
        if self.data.merged_times().is_empty() {
            return false;
        }
        let pane = &self.panes[drawing.pane_index];
        let (mut left, mut right) = match entry.bounds.logical {
            LogicalBounds::Full => (0.0, self.pane_w),
            LogicalBounds::From(logical) => {
                let start_x = self.time_scale.logical_to_coordinate(logical);
                if start_x <= self.pane_w || !drawing.text.is_empty() {
                    (start_x, self.pane_w)
                } else {
                    (start_x, start_x)
                }
            }
            LogicalBounds::Ray {
                start,
                towards_right,
            } => {
                let start_x = self.time_scale.logical_to_coordinate(start);
                if towards_right {
                    (start_x, self.pane_w.max(start_x))
                } else {
                    (0.0_f64.min(start_x), start_x)
                }
            }
            LogicalBounds::Finite { min, max } => (
                self.time_scale.logical_to_coordinate(min),
                self.time_scale.logical_to_coordinate(max),
            ),
        };
        let (mut top, mut bottom) = match (entry.bounds.min_price, entry.bounds.max_price) {
            (Some(min), Some(max)) => (
                scale.price_to_coordinate(max, base),
                scale.price_to_coordinate(min, base),
            ),
            _ => (pane.top, pane.top + pane.height),
        };
        if drawing.kind == DrawingKind::AnchoredText {
            let x = drawing.screen_x * self.pane_w;
            let y = pane.top + drawing.screen_y * pane.height;
            left = x;
            right = x;
            top = y;
            bottom = y;
        } else if matches!(
            drawing.kind,
            DrawingKind::Circle
                | DrawingKind::Arc
                | DrawingKind::RotatedRectangle
                | DrawingKind::FibonacciSpeedArcs
                | DrawingKind::FibonacciCircles
                | DrawingKind::FibonacciSpiral
                | DrawingKind::FibonacciWedge
        ) {
            let mut px = [(0.0, 0.0); 4];
            for (slot, &point) in px.iter_mut().zip(&drawing.points) {
                let Some(converted) =
                    self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)
                else {
                    return false;
                };
                *slot = converted;
            }
            let Some(geometry) = resolve_drawing_geometry(
                drawing.kind,
                &px[..drawing.points.len().min(px.len())],
                self.pane_w,
                pane.top,
                pane.height,
                DrawingGeometryOptions::default(),
            ) else {
                return false;
            };
            if let DrawingBodyGeometry::FibonacciArcs(arcs) = geometry.body {
                let radius = arcs.radius
                    * drawing
                        .levels
                        .iter()
                        .filter(|level| level.visible)
                        .map(|level| drawing.level_value(level.value).max(0.0))
                        .fold(0.0, f64::max);
                left = arcs.center.0 - radius;
                right = arcs.center.0 + radius;
                top = arcs.center.1 - radius;
                bottom = arcs.center.1 + radius;
            } else {
                left = geometry.text_box.left;
                right = geometry.text_box.right;
                top = geometry.text_box.top;
                bottom = geometry.text_box.bottom;
            }
        }
        let mut extra_x = drawing.width / 2.0 + HitProfile::TOUCH.drawing_stroke_tolerance;
        let mut extra_y = extra_x;
        extra_x = extra_x.max(HitProfile::TOUCH.drawing_anchor_radius);
        extra_y = extra_y.max(HitProfile::TOUCH.drawing_anchor_radius);
        if drawing.kind.is_marker() {
            extra_x = extra_x.max(16.0);
            extra_y = extra_y.max(16.0);
        }
        if drawing.kind == DrawingKind::IconStamp {
            extra_x = extra_x.max(drawing.icon_size / 2.0);
            extra_y = extra_y.max(drawing.icon_size / 2.0);
        }
        if drawing.kind == DrawingKind::Forecast {
            extra_x = extra_x.max(160.0);
            extra_y = extra_y.max(24.0);
        }
        if let Some((width, size)) = text_metrics {
            extra_x = extra_x.max(width + TEXT_PAD * 2.0);
            extra_y = extra_y.max(size * 1.2 + TEXT_PAD * 2.0);
        }
        entry.screen_bounds = ScreenBounds {
            left: left.min(right) - extra_x,
            right: left.max(right) + extra_x,
            top: top.min(bottom) - extra_y,
            bottom: top.max(bottom) + extra_y,
        };
        entry.bounds_key = key;
        entry.screen_valid = true;
        true
    }

    fn drawing_semantic_might_intersect(
        &self,
        drawing: &Drawing,
        bounds: DrawingBounds,
        pane_index: usize,
        visible: (f64, f64),
        base: f64,
        text_metrics: Option<(f64, f64)>,
    ) -> bool {
        let mut extra_x = drawing.width / 2.0 + HitProfile::TOUCH.drawing_anchor_radius;
        let mut extra_y = extra_x;
        if drawing.kind.is_marker() {
            extra_x = extra_x.max(16.0);
            extra_y = extra_y.max(16.0);
        }
        if drawing.kind == DrawingKind::IconStamp {
            extra_x = extra_x.max(drawing.icon_size / 2.0);
            extra_y = extra_y.max(drawing.icon_size / 2.0);
        }
        if drawing.kind == DrawingKind::Forecast {
            extra_x = extra_x.max(160.0);
            extra_y = extra_y.max(24.0);
        }
        if let Some((width, size)) = text_metrics {
            extra_x = extra_x.max(width + TEXT_PAD * 2.0);
            extra_y = extra_y.max(size * 1.2 + TEXT_PAD * 2.0);
        }
        let logical_pad = extra_x / self.time_scale.bar_spacing().max(f64::MIN_POSITIVE);
        let logical_intersects = match bounds.logical {
            LogicalBounds::Full => true,
            // A labeled ray can anchor its right-aligned label at the pane edge even when its
            // start sits beyond that edge, so keep it conservative.
            LogicalBounds::From(_) if !drawing.text.is_empty() => true,
            LogicalBounds::From(start) => start <= visible.1 + logical_pad,
            LogicalBounds::Ray {
                start,
                towards_right,
            } => {
                if towards_right {
                    start <= visible.1 + logical_pad
                } else {
                    start >= visible.0 - logical_pad
                }
            }
            LogicalBounds::Finite { min, max } => {
                min <= visible.1 + logical_pad && max >= visible.0 - logical_pad
            }
        };
        if !logical_intersects {
            return false;
        }
        let (Some(min_price), Some(max_price)) = (bounds.min_price, bounds.max_price) else {
            return true;
        };
        let Some(scale) = self.drawing_scale_for(pane_index, drawing.price_scale) else {
            return false;
        };
        let first = scale.price_to_coordinate(min_price, base);
        let second = scale.price_to_coordinate(max_price, base);
        let pane = &self.panes[pane_index];
        first.min(second) <= pane.top + pane.height + extra_y
            && first.max(second) >= pane.top - extra_y
    }

    pub(crate) fn drawing_px_cached<'a>(
        &self,
        drawing: &Drawing,
        runtime: &'a mut DrawingRuntime,
        key: [u64; 12],
    ) -> Option<&'a [(f64, f64)]> {
        let rebuild = runtime
            .entries
            .get(&drawing.id)
            .is_some_and(|entry| !entry.geometry_valid || entry.geometry_key != key);
        if rebuild {
            let entry = runtime.entries.get_mut(&drawing.id)?;
            entry.media_px.clear();
            if drawing.kind == DrawingKind::AnchoredText {
                let pane = self.panes.get(drawing.pane_index)?;
                entry.media_px.push((
                    drawing.screen_x * self.pane_w,
                    pane.top + drawing.screen_y * pane.height,
                ));
            } else {
                for &point in &drawing.points {
                    entry.media_px.push(self.drawing_to_px_for(
                        drawing.pane_index,
                        drawing.price_scale,
                        point,
                    )?);
                }
            }
            if drawing.kind == DrawingKind::RegressionTrend
                && let Some(derived) = self.regression_points(drawing)
            {
                for point in derived {
                    entry.media_px.push(self.drawing_to_px_for(
                        drawing.pane_index,
                        drawing.price_scale,
                        point,
                    )?);
                }
            }
            entry.geometry_key = key;
            entry.geometry_valid = true;
            runtime.stats.geometry_rebuilds += 1;
        }
        Some(&runtime.entries.get(&drawing.id)?.media_px)
    }

    fn cached_drawing_layout(&self) -> (f64, Rc<str>) {
        let generation = self.options.generation();
        let mut runtime = self.drawing_runtime.borrow_mut();
        if runtime.layout_generation != generation || runtime.font_family.is_empty() {
            let layout = &self.options.get().layout;
            runtime.layout_generation = generation;
            runtime.font_size = layout.font_size;
            runtime.font_family = Rc::from(layout.font_family.as_str());
        }
        (runtime.font_size, Rc::clone(&runtime.font_family))
    }

    fn cached_drawing_text_metrics(
        &self,
        drawing: &Drawing,
        entry: &mut DrawingCache,
        key: u64,
        font_size: f64,
        font_family: &str,
    ) -> Option<(f64, f64)> {
        if drawing.kind.is_measure() {
            // The statistics label (at most two lines) sits beyond the measured area; report a
            // reach that keeps a visible label's drawing in the candidate set.
            let size = crate::frame::stat_label_height(font_size, 2) + MEASURE_LABEL_GAP;
            return Some((MEASURE_LABEL_REACH_X, size / 1.2));
        }
        if drawing.text.is_empty()
            && drawing.kind != DrawingKind::Text
            && drawing.kind != DrawingKind::TrendLine
            && !drawing.kind.is_text_annotation()
        {
            return None;
        }
        if entry.text_key != key {
            let size = drawing.resolved_text_size(font_size);
            entry.text_width = if drawing.kind == DrawingKind::TrendLine && drawing.text.is_empty()
            {
                self.measure_text_run(
                    TREND_TEXT_PLACEHOLDER,
                    size,
                    font_family,
                    drawing.text_weight.unwrap_or(400),
                    drawing.text_italic,
                )
            } else {
                self.measure_drawing_text_with_family(drawing, size, font_family)
            };
            entry.text_size = size;
            entry.text_key = key;
        }
        Some((entry.text_width, entry.text_size))
    }

    pub(crate) fn take_drawing_candidates(
        &self,
        pane_index: usize,
        point: Option<(f64, f64)>,
    ) -> Vec<DrawingId> {
        let Some(pane) = self.panes.get(pane_index) else {
            return Vec::new();
        };
        let viewport = ScreenBounds {
            left: 0.0,
            right: self.pane_w,
            top: pane.top,
            bottom: pane.top + pane.height,
        };
        let (font_size, font_family) = self.cached_drawing_layout();
        let text_key = self.options.generation();
        let first_logical = self.time_scale.coordinate_to_float_index(0.0) + 0.5;
        let last_logical = self.time_scale.coordinate_to_float_index(self.pane_w) + 0.5;
        let visible = (
            first_logical.min(last_logical),
            first_logical.max(last_logical),
        );
        let mut runtime = self.drawing_runtime.borrow_mut();
        let mut candidates = std::mem::take(&mut runtime.scratch);
        candidates.clear();
        let pane_len = runtime.panes.get(pane_index).map_or(0, Vec::len);
        runtime.stats.drawings_total += pane_len;
        runtime.stats.bounds_tests += pane_len;
        for index in 0..pane_len {
            let id = runtime.panes[pane_index][index];
            let Some(&position) = runtime.positions.get(&id) else {
                continue;
            };
            let Some(drawing) = self.drawings.get(position) else {
                continue;
            };
            let Some(key) = self.drawing_coordinate_key(drawing) else {
                continue;
            };
            let base = self.drawing_scale_base_for(pane_index, drawing.price_scale);
            let Some(entry) = runtime.entries.get_mut(&id) else {
                continue;
            };
            let text_metrics =
                self.cached_drawing_text_metrics(drawing, entry, text_key, font_size, &font_family);
            if !self.drawing_semantic_might_intersect(
                drawing,
                entry.bounds,
                pane_index,
                visible,
                base,
                text_metrics,
            ) {
                continue;
            }
            let valid = self.refresh_drawing_screen_bounds(drawing, entry, key, base, text_metrics);
            let bounds = entry.screen_bounds;
            if valid
                && point.map_or_else(
                    || bounds.intersects(viewport),
                    |(x, y)| bounds.contains(x, y),
                )
            {
                candidates.push(id);
            }
        }
        runtime.stats.candidates += candidates.len();
        candidates
    }

    pub(crate) fn recycle_drawing_candidates(&self, mut candidates: Vec<DrawingId>) {
        candidates.clear();
        self.drawing_runtime.borrow_mut().scratch = candidates;
    }

    #[cfg(test)]
    pub(crate) fn drawing_viewport_candidate_reference(&self, drawing: &Drawing) -> bool {
        let Some(key) = self.drawing_coordinate_key(drawing) else {
            return false;
        };
        let pane = &self.panes[drawing.pane_index];
        let viewport = ScreenBounds {
            left: 0.0,
            right: self.pane_w,
            top: pane.top,
            bottom: pane.top + pane.height,
        };
        let base = self.drawing_scale_base_for(drawing.pane_index, drawing.price_scale);
        let (font_size, font_family) = self.cached_drawing_layout();
        let mut entry = DrawingCache::new(drawing);
        let text_metrics = self.cached_drawing_text_metrics(
            drawing,
            &mut entry,
            self.options.generation(),
            font_size,
            &font_family,
        );
        self.refresh_drawing_screen_bounds(drawing, &mut entry, key, base, text_metrics)
            && entry.screen_bounds.intersects(viewport)
    }

    #[doc(hidden)]
    pub fn drawing_work_stats(&self) -> DrawingWorkStats {
        self.drawing_runtime.borrow().stats
    }

    #[doc(hidden)]
    pub fn reset_drawing_work_stats(&self) {
        self.drawing_runtime.borrow_mut().stats = DrawingWorkStats::default();
    }

    /// The rectangle's eight reference-informed anchors derived from its two corners (media px), in
    /// clock order from the top-left: 0 TL, 1 top-mid, 2 TR, 3 right-mid, 4 BR, 5 bottom-mid,
    /// 6 BL, 7 left-mid. Corners resize both adjacent edges, midpoints one edge — all with
    /// flip-on-cross (the opposite side stays put). Works on any px basis (media/bitmap).
    pub(crate) fn rectangle_anchors(px: &[(f64, f64)]) -> [(f64, f64); 8] {
        let (a, b) = (px[0], px[1]);
        let (l, r) = (a.0.min(b.0), a.0.max(b.0));
        let (t, bo) = (a.1.min(b.1), a.1.max(b.1));
        let (mx, my) = ((l + r) / 2.0, (t + bo) / 2.0);
        [
            (l, t),
            (mx, t),
            (r, t),
            (r, my),
            (r, bo),
            (mx, bo),
            (l, bo),
            (l, my),
        ]
    }

    /// The directional resize cursor for a rectangle anchor (reference-informed behavior): diagonal
    /// cursors on the corners, straight ones on the edge midpoints.
    pub(crate) fn rectangle_anchor_cursor(index: usize) -> &'static str {
        match index {
            0 | 4 => "nwse-resize",
            2 | 6 => "nesw-resize",
            1 | 5 => "ns-resize",
            _ => "ew-resize", // 3 | 7
        }
    }

    // --- text placement (shared by render and hit-testing) ---

    /// The label's reference box for a drawing in the caller's units. `px` holds the anchors
    /// already converted (bitmap px at render, media px at hit-test); `pane_w`/`pane_h` bound
    /// the full-width/full-height kinds in the same units. `pane_top` is the pane's vertical
    /// offset (0 for pane-local bitmap x media y are both chart-top-relative — see hit_test.rs).
    pub(crate) fn text_box(
        kind: DrawingKind,
        px: &[(f64, f64)],
        pane_w: f64,
        pane_top: f64,
        pane_h: f64,
    ) -> TextBox {
        resolve_drawing_geometry(
            kind,
            px,
            pane_w,
            pane_top,
            pane_h,
            DrawingGeometryOptions {
                line_width: 1.0,
                device_scale: 1.0,
                ..Default::default()
            },
        )
        .map(|geometry| geometry.text_box)
        .unwrap_or_default()
    }

    /// The label's draw anchor `(x, y_center)` and horizontal alignment in the caller's units,
    /// resolved from the reference box and the drawing's 3×3 alignment. `size` is the glyph size
    /// in the same units. Vertical: `Top` sits the run above the box (a line's "above" slot; for
    /// the text tool, above its anchor point), `Middle` centers it, `Bottom` below it.
    pub(crate) fn text_placement(
        drawing: &Drawing,
        reference: &TextBox,
        size: f64,
        pad: f64,
    ) -> (f64, f64, DrawingTextHAlign, f64) {
        let h = drawing.text_h_align;
        let x = match h {
            DrawingTextHAlign::Left => reference.left + pad,
            DrawingTextHAlign::Center => (reference.left + reference.right) / 2.0,
            DrawingTextHAlign::Right => reference.right - pad,
        };
        let y = match drawing.text_v_align {
            DrawingTextVAlign::Top => reference.top - pad - size / 2.0,
            DrawingTextVAlign::Middle => (reference.top + reference.bottom) / 2.0,
            DrawingTextVAlign::Bottom => reference.bottom + pad + size / 2.0,
        };
        (x, y, h, 0.0)
    }

    pub(crate) fn drawing_text_placement(
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w: f64,
        pane_top: f64,
        pane_h: f64,
        size: f64,
        pad: f64,
    ) -> (f64, f64, DrawingTextHAlign, f64) {
        if drawing.kind == DrawingKind::TrendLine && px.len() >= 2 {
            let (mut start, mut end) = (px[0], px[1]);
            let mut dx = end.0 - start.0;
            let mut dy = end.1 - start.1;
            let length = dx.hypot(dy);
            if length > f64::EPSILON {
                // Text is never upside down. Reversing the readable axis also makes left/right
                // visual slots stable when the user drags one endpoint through the other.
                if dx < 0.0 || (dx.abs() <= f64::EPSILON && dy > 0.0) {
                    std::mem::swap(&mut start, &mut end);
                    dx = -dx;
                    dy = -dy;
                }
                let ux = dx / length;
                let uy = dy / length;
                let usable_pad = pad.min(length / 2.0);
                let distance = match drawing.text_h_align {
                    DrawingTextHAlign::Left => usable_pad,
                    DrawingTextHAlign::Center => length / 2.0,
                    DrawingTextHAlign::Right => length - usable_pad,
                };
                let mut x = start.0 + ux * distance;
                let mut y = start.1 + uy * distance;
                // Placement and hit testing use the same 1.2em line box. Half a bare em can
                // put descenders against the stroke, particularly when the line is tilted.
                let half_line_height = size * 0.6;
                let normal_distance = match drawing.text_v_align {
                    DrawingTextVAlign::Top => pad + half_line_height,
                    DrawingTextVAlign::Middle => 0.0,
                    DrawingTextVAlign::Bottom => -pad - half_line_height,
                };
                // Screen y grows downward, so `(uy, -ux)` is the readable line's top normal.
                x += uy * normal_distance;
                y -= ux * normal_distance;
                return (x, y, drawing.text_h_align, dy.atan2(dx));
            }
        }
        let reference = Self::text_box(drawing.kind, px, pane_w, pane_top, pane_h);
        Self::text_placement(drawing, &reference, size, pad)
    }

    /// Measure (or estimate) a label's width in the same px units as `size`. Empty text tools
    /// use one em so the focus/hover chrome and hit target stay a caret-sized box (the host
    /// never leaves an empty text drawing on the chart after editing).
    pub(crate) fn measure_drawing_text(&self, drawing: &Drawing, size: f64) -> f64 {
        let layout = &self.options.get().layout;
        self.measure_drawing_text_with_family(drawing, size, &layout.font_family)
    }

    fn measure_drawing_text_with_family(&self, drawing: &Drawing, size: f64, family: &str) -> f64 {
        if (drawing.kind == DrawingKind::Text || drawing.kind.is_text_annotation())
            && drawing.text.is_empty()
        {
            return size;
        }
        let text = drawing.display_text();
        let weight = drawing.text_weight.unwrap_or(400);
        self.measure_text_run(text, size, family, weight, drawing.text_italic)
    }

    pub(crate) fn measure_text_run(
        &self,
        text: &str,
        size: f64,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f64 {
        match &self.text_measure_fn {
            Some(measure) => measure(text, size, family, weight, italic),
            None => text.chars().count() as f64 * size * 0.6,
        }
    }

    /// Install (or clear with `None`) the host text-measure callback for drawing-label hit
    /// boxes (see [`TextMeasureFn`]).
    pub fn set_text_measure(&mut self, f: Option<TextMeasureFn>) {
        self.text_measure_fn = f;
        for entry in self.drawing_runtime.borrow_mut().entries.values_mut() {
            entry.screen_valid = false;
            entry.text_key = u64::MAX;
        }
        self.invalidate_frame_drawings();
        // Trading marker cells size themselves from measured quantity text.
        self.invalidate_frame_trading();
    }

    /// Whether a host has installed a glyph-width measurer. Native renderers use this to avoid
    /// invalidating drawing geometry on every screenshot after their first measurement install.
    pub fn has_text_measure(&self) -> bool {
        self.text_measure_fn.is_some()
    }

    /// Cap-and-figure ink correction for a middle-anchored run (see [`TextCapCenterFn`]).
    pub(crate) fn text_cap_center(
        &self,
        size: f64,
        family: &str,
        weight: u16,
        italic: bool,
    ) -> f64 {
        self.text_cap_center_fn
            .as_ref()
            .map(|correction| correction(size, family, weight, italic))
            .filter(|correction| correction.is_finite())
            .unwrap_or(0.0)
    }

    /// Install (or clear with `None`) the host vertical glyph metric used to optically center
    /// control text inside its box (see [`TextCapCenterFn`]).
    pub fn set_text_cap_center(&mut self, f: Option<TextCapCenterFn>) {
        self.text_cap_center_fn = f;
        self.invalidate_axis_frame();
        self.invalidate_frame_trading();
    }

    fn insert_drawing_runtime(&self, id: DrawingId) {
        let Some(position) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return;
        };
        self.drawing_runtime.borrow_mut().insert(
            &self.drawings[position],
            position,
            self.panes.len(),
        );
    }

    fn take_drawing_id(&mut self) -> Option<DrawingId> {
        let id = self.next_drawing_id;
        self.next_drawing_id = self.next_drawing_id.checked_add(1)?;
        Some(id)
    }

    fn update_drawing_runtime(&self, id: DrawingId) {
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        self.drawing_runtime
            .borrow_mut()
            .update(drawing, self.panes.len());
    }

    fn invalidate_brush_drag_runtime(&self, id: DrawingId) {
        let mut runtime = self.drawing_runtime.borrow_mut();
        let Some(entry) = runtime.entries.get_mut(&id) else {
            return;
        };
        // A moving/endpoint-edited brush stays conservatively global during the active drag.
        // The exact cached path bounds are rebuilt once on pointer-up, avoiding a second walk of
        // a 10K-point path on every pointer sample while never excluding the moving stroke.
        entry.bounds.logical = LogicalBounds::Full;
        entry.bounds.min_price = None;
        entry.bounds.max_price = None;
        entry.screen_valid = false;
        entry.geometry_valid = false;
        runtime.stats.bounds_rebuilds += 1;
    }

    // --- CRUD ---

    /// Add a drawing to a financial-time pane; returns its chart-unique id, or `None` for a stale
    /// or incompatible pane, a wrong anchor count for the kind, or non-finite anchors.
    /// `options_json` is a [`DrawingPatch`] — absent keys take the documented defaults.
    pub fn add_drawing(
        &mut self,
        kind: DrawingKind,
        pane_index: usize,
        points: Vec<DrawingPoint>,
        options_json: Option<&str>,
    ) -> Option<DrawingId> {
        self.invalidate_frame_drawings();
        if pane_index >= self.panes.len()
            || !self.pane_uses_financial_time(pane_index)
            || points.len() > MAX_DRAWING_POINTS
            || !kind.valid_point_count(points.len())
        {
            return None;
        }
        if points
            .iter()
            .any(|p| !p.logical.is_finite() || !p.price.is_finite())
        {
            return None;
        }
        let id = self.take_drawing_id()?;
        let mut drawing = Drawing::new(id, kind, pane_index, points);
        if kind == DrawingKind::AnchoredText
            && let (Some(pane), Some(&point)) = (self.panes.get(pane_index), drawing.points.first())
            && let Some((x, y)) = self.drawing_to_px_for(pane_index, drawing.price_scale, point)
        {
            drawing.screen_x = (x / self.pane_w.max(1.0)).clamp(0.0, 1.0);
            drawing.screen_y = ((y - pane.top) / pane.height.max(1.0)).clamp(0.0, 1.0);
        }
        if let Some(json) = options_json {
            let patch = serde_json::from_str::<DrawingPatch>(json).ok()?;
            if !drawing.apply_patch(patch) {
                return None;
            }
        }
        if kind == DrawingKind::BarsPattern {
            drawing.bars_pattern = self.capture_bars_pattern(&drawing)?;
        }
        self.drawings.push(drawing);
        self.insert_drawing_runtime(id);
        let index = self.drawings.len() - 1;
        self.record_drawing_command(DrawingCommand::Create {
            drawing: self.drawings[index].clone(),
            index,
        });
        self.bump_drawing_sync_revision();
        Some(id)
    }

    /// Merge a JSON options patch into the drawing with `id` (reference `applyOptions`): absent
    /// keys keep their current values. Returns false for a malformed patch or an unknown id.
    pub fn drawing_apply_options(&mut self, id: DrawingId, json: &str) -> bool {
        self.invalidate_frame_drawings();
        let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) else {
            return false;
        };
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        let before = self.drawings[index].clone();
        let mut after = before.clone();
        if !after.apply_patch(patch) {
            return false;
        }
        if after != before {
            after.revision = after.revision.saturating_add(1);
        }
        self.drawings[index] = after.clone();
        self.update_drawing_runtime(id);
        if before != after {
            self.record_drawing_command(DrawingCommand::Update {
                before,
                after: Box::new(after),
            });
            self.bump_drawing_sync_revision();
        }
        true
    }

    pub(crate) fn set_drawing_profile_options(
        &mut self,
        id: DrawingId,
        profile: crate::ProfileDrawingOptions,
    ) -> bool {
        self.invalidate_frame_drawings();
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        let before = self.drawings[index].clone();
        self.drawings[index].profile = Some(profile);
        self.drawings[index].revision = self.drawings[index].revision.saturating_add(1);
        let after = self.drawings[index].clone();
        self.update_drawing_runtime(id);
        self.record_drawing_command(DrawingCommand::Update {
            before,
            after: Box::new(after),
        });
        self.bump_drawing_sync_revision();
        true
    }

    /// Replace a drawing's anchors from a JSON `[{logical, price}, ...]` array. Returns false
    /// for malformed JSON, a wrong count for the kind, non-finite values, or an unknown id.
    pub fn drawing_set_points(&mut self, id: DrawingId, json: &str) -> bool {
        self.invalidate_frame_drawings();
        let Ok(mut points) = serde_json::from_str::<Vec<DrawingPoint>>(json) else {
            return false;
        };
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        let drawing = &self.drawings[index];
        if points.len() > MAX_DRAWING_POINTS
            || !drawing.kind.valid_point_count(points.len())
            || points
                .iter()
                .any(|p| !p.logical.is_finite() || !p.price.is_finite())
        {
            return false;
        }
        Drawing::normalize_position_points(drawing.kind, &mut points);
        let before = drawing.clone();
        let pattern_bars =
            if drawing.kind == DrawingKind::BarsPattern && drawing.points[..2] != points[..2] {
                let mut next = drawing.clone();
                next.points = points.clone();
                match self.capture_bars_pattern(&next) {
                    Some(bars) => Some(bars),
                    None => return false,
                }
            } else {
                None
            };
        if drawing.kind == DrawingKind::AnchoredText {
            let point = points[0];
            let pane = &self.panes[drawing.pane_index];
            if let Some((x, y)) =
                self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)
            {
                self.drawings[index].screen_x = (x / self.pane_w.max(1.0)).clamp(0.0, 1.0);
                self.drawings[index].screen_y =
                    ((y - pane.top) / pane.height.max(1.0)).clamp(0.0, 1.0);
            }
        }
        self.drawings[index].points = points;
        if let Some(bars) = pattern_bars {
            self.drawings[index].bars_pattern = bars;
        }
        let after = self.drawings[index].clone();
        if before.points != after.points {
            self.drawing_anchor_times.remove(&id);
        }
        self.update_drawing_runtime(id);
        if before != after {
            self.record_drawing_command(DrawingCommand::Update {
                before,
                after: Box::new(after),
            });
            self.bump_drawing_sync_revision();
        }
        true
    }

    /// Remove a drawing by id. Returns false for an unknown id. Any selection or drag session
    /// pointing at it is released.
    pub fn remove_drawing(&mut self, id: DrawingId) -> bool {
        self.invalidate_frame_drawings();
        let Some((drawing, index)) = self.remove_drawing_snapshot(id) else {
            return false;
        };
        self.record_drawing_command(DrawingCommand::Delete { drawing, index });
        self.bump_drawing_sync_revision();
        true
    }

    /// Remove every drawing (the demo's "clear all") and release the selection/drag state.
    pub fn clear_drawings(&mut self) {
        self.invalidate_frame_drawings();
        if self.drawings.is_empty() {
            return;
        }
        let drawings = std::mem::take(&mut self.drawings);
        self.drawing_anchor_times.clear();
        self.drawing_runtime.borrow_mut().clear();
        self.selected_drawing = None;
        self.drawing_drag = None;
        self.hovered_drawing = None;
        self.hovered_text = None;
        self.editing_drawing = None;
        self.drawing_text_edit = None;
        self.record_drawing_command(DrawingCommand::Clear { drawings });
        self.bump_drawing_sync_revision();
    }

    /// The drawing's full options as a snake_case JSON object (reference `options`). `None` for
    /// an unknown id.
    pub fn drawing_options_json(&self, id: DrawingId) -> Option<String> {
        Some(self.drawing(id)?.options_json().to_string())
    }

    /// Typed schema for a drawing's generic property panel.  The schema is deterministic and
    /// independent of the current drawing instance.
    pub fn drawing_property_schema_json(&self, id: DrawingId) -> Option<String> {
        Some(
            serde_json::to_string(&crate::drawing_contract::drawing_property_schema(
                self.drawing(id)?.kind,
            ))
            .unwrap_or_default(),
        )
    }

    /// Typed per-kind option block for a property panel or template editor.
    pub fn drawing_kind_options_json(&self, id: DrawingId) -> Option<String> {
        serde_json::to_string(&self.drawing(id)?.kind_options()).ok()
    }

    /// The object-tree snapshot. Runtime caches, hover, selection and editor state are excluded.
    pub fn drawing_object_tree_json(&self) -> String {
        let values = self
            .drawings
            .iter()
            .map(|drawing| drawing.common_snapshot())
            .collect::<Vec<_>>();
        serde_json::to_string(&values).unwrap_or_default()
    }

    /// Set a bounded multi-selection. The first id becomes the compatibility primary selection.
    pub fn set_selected_drawings(&mut self, ids: &[DrawingId]) -> bool {
        let mut selected = Vec::with_capacity(ids.len().min(crate::MAX_DRAWING_OBJECTS));
        for &id in ids.iter().take(crate::MAX_DRAWING_OBJECTS) {
            if self.drawing(id).is_some() && !selected.contains(&id) {
                selected.push(id);
            }
        }
        let changed = selected != self.selected_drawings;
        self.selected_drawings = selected;
        self.selected_drawing = self.selected_drawings.first().copied();
        if changed {
            self.invalidate_frame_overlay();
        }
        changed
    }

    pub fn selected_drawings(&self) -> &[DrawingId] {
        &self.selected_drawings
    }

    /// Copy selected or explicitly supplied drawings to a bounded, revisioned payload.
    pub fn copy_drawings_json(&self, ids: &[DrawingId]) -> Option<String> {
        let ids = if ids.is_empty() {
            &self.selected_drawings
        } else {
            ids
        };
        let drawings = ids
            .iter()
            .take(crate::MAX_DRAWING_OBJECTS)
            .filter_map(|id| self.drawing(*id))
            .map(|drawing| crate::DrawingClipboardItem {
                id: None,
                kind: drawing.kind,
                pane_index: drawing.pane_index,
                points: drawing.points.clone(),
                options: drawing.options_json(),
                bars_pattern: (drawing.kind == DrawingKind::BarsPattern)
                    .then(|| drawing.bars_pattern.clone()),
            })
            .collect::<Vec<_>>();
        let payload = crate::DrawingClipboardPayload {
            schema: "aeris_charts-drawings".to_string(),
            revision: self.drawing_sync_revision,
            drawings,
        };
        let bytes = serde_json::to_vec(&payload).ok()?;
        (bytes.len() <= crate::MAX_DRAWING_TEMPLATE_BYTES).then(|| String::from_utf8(bytes).ok())?
    }

    /// Paste a clipboard payload into one pane with a semantic anchor offset. Validation happens
    /// before mutation, so malformed or oversized payloads cannot partially change the chart.
    pub fn paste_drawings_json(
        &mut self,
        json: &str,
        pane_index: usize,
        logical_offset: f64,
        price_offset: f64,
    ) -> Option<Vec<DrawingId>> {
        if json.len() > crate::MAX_DRAWING_TEMPLATE_BYTES
            || !logical_offset.is_finite()
            || !price_offset.is_finite()
            || pane_index >= self.panes.len()
            || !self.pane_uses_financial_time(pane_index)
        {
            return None;
        }
        let payload = serde_json::from_str::<crate::DrawingClipboardPayload>(json).ok()?;
        if payload.schema != "aeris_charts-drawings"
            || payload.drawings.is_empty()
            || payload.drawings.len() > crate::MAX_DRAWING_OBJECTS
        {
            return None;
        }
        let mut staged = Vec::with_capacity(payload.drawings.len());
        for item in payload.drawings {
            let mut points = item.points;
            for point in &mut points {
                point.logical += logical_offset;
                point.price += price_offset;
            }
            if points.len() > MAX_DRAWING_POINTS
                || !item.kind.valid_point_count(points.len())
                || points
                    .iter()
                    .any(|point| !point.logical.is_finite() || !point.price.is_finite())
            {
                return None;
            }
            if item.kind == DrawingKind::BarsPattern {
                if !item.bars_pattern.as_deref().is_some_and(valid_bars_pattern) {
                    return None;
                }
            } else if item.bars_pattern.is_some() {
                return None;
            }
            let patch = serde_json::from_value::<DrawingPatch>(item.options).ok()?;
            let mut drawing = Drawing::new(0, item.kind, pane_index, points);
            if !drawing.apply_patch(patch) {
                return None;
            }
            drawing.bars_pattern = item.bars_pattern.unwrap_or_default();
            staged.push(drawing);
        }
        let mut ids = Vec::with_capacity(staged.len());
        let final_id = self
            .next_drawing_id
            .checked_add(u32::try_from(staged.len()).ok()?)?;
        for mut drawing in staged {
            let id = self.take_drawing_id()?;
            drawing.id = id;
            drawing.z_order = id as i32;
            self.drawings.push(drawing);
            self.insert_drawing_runtime(id);
            let index = self.drawings.len() - 1;
            self.record_drawing_command(DrawingCommand::Create {
                drawing: self.drawings[index].clone(),
                index,
            });
            ids.push(id);
        }
        debug_assert_eq!(self.next_drawing_id, final_id);
        self.invalidate_frame_drawings();
        self.bump_drawing_sync_revision();
        Some(ids)
    }

    pub fn clone_drawing(
        &mut self,
        id: DrawingId,
        logical_offset: f64,
        price_offset: f64,
    ) -> Option<DrawingId> {
        let payload = self.copy_drawings_json(&[id])?;
        self.paste_drawings_json(
            &payload,
            self.drawing(id)?.pane_index,
            logical_offset,
            price_offset,
        )?
        .into_iter()
        .next()
    }

    /// Move one drawing within the layer and normalize its stable z-order values. `delta` is a
    /// bounded relative position; negative moves backward, positive moves forward.
    pub fn move_drawing_z_order(&mut self, id: DrawingId, delta: i32) -> bool {
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        let target = (index as i32 + delta.clamp(-100, 100))
            .clamp(0, self.drawings.len().saturating_sub(1) as i32) as usize;
        if target == index {
            return false;
        }
        let before = self.drawings.clone();
        let drawing = self.drawings.remove(index);
        self.drawings.insert(target, drawing);
        for (position, drawing) in self.drawings.iter_mut().enumerate() {
            drawing.z_order = position as i32;
        }
        let after = self.drawings.clone();
        self.record_drawing_command(DrawingCommand::Reorder { before, after });
        self.bump_drawing_sync_revision();
        self.drawing_runtime
            .borrow_mut()
            .rebuild_all(&self.drawings, self.panes.len());
        self.invalidate_frame_drawings();
        true
    }

    pub fn set_drawing_group(&mut self, id: DrawingId, group_id: Option<String>) -> bool {
        if group_id
            .as_ref()
            .is_some_and(|group| group.len() > crate::MAX_DRAWING_GROUP_BYTES)
        {
            return false;
        }
        let value = serde_json::json!({ "group_id": group_id.unwrap_or_default() });
        self.drawing_apply_options(id, &value.to_string())
    }

    pub fn set_drawing_visibility(&mut self, id: DrawingId, visible: bool) -> bool {
        self.drawing_apply_options(id, &serde_json::json!({ "visible": visible }).to_string())
    }

    pub fn set_drawing_locked(&mut self, id: DrawingId, locked: bool) -> bool {
        self.drawing_apply_options(id, &serde_json::json!({ "locked": locked }).to_string())
    }

    fn update_group<F>(&mut self, group_id: &str, mut update: F) -> usize
    where
        F: FnMut(&mut Drawing),
    {
        if group_id.is_empty() {
            return 0;
        }
        let before = self.drawings.clone();
        let mut changed = 0;
        for drawing in &mut self.drawings {
            if drawing.group_id.as_deref() == Some(group_id) {
                let original = drawing.clone();
                update(drawing);
                if *drawing != original {
                    drawing.revision = drawing.revision.saturating_add(1);
                    changed += 1;
                }
            }
        }
        if changed != 0 {
            let after = self.drawings.clone();
            self.record_drawing_command(DrawingCommand::BatchUpdate { before, after });
            self.bump_drawing_sync_revision();
            self.drawing_runtime
                .borrow_mut()
                .rebuild_all(&self.drawings, self.panes.len());
            self.invalidate_frame_drawings();
        }
        changed
    }

    pub fn set_drawing_group_visibility(&mut self, group_id: &str, visible: bool) -> usize {
        self.update_group(group_id, |drawing| drawing.visible = visible)
    }

    pub fn set_drawing_group_locked(&mut self, group_id: &str, locked: bool) -> usize {
        self.update_group(group_id, |drawing| drawing.locked = locked)
    }

    pub fn move_drawing_group(
        &mut self,
        group_id: &str,
        logical_delta: f64,
        price_delta: f64,
    ) -> usize {
        if !logical_delta.is_finite() || !price_delta.is_finite() {
            return 0;
        }
        self.update_group(group_id, |drawing| {
            if drawing.locked {
                return;
            }
            for point in &mut drawing.points {
                point.logical += logical_delta;
                point.price += price_delta;
            }
        })
    }

    /// Atomically remove a set of drawings in object-tree order.
    pub fn remove_drawings(&mut self, ids: &[DrawingId]) -> usize {
        let mut removed = 0;
        for id in ids
            .iter()
            .take(crate::MAX_DRAWING_OBJECTS)
            .copied()
            .collect::<Vec<_>>()
        {
            removed += usize::from(self.remove_drawing(id));
        }
        let live_ids = self
            .drawings
            .iter()
            .map(|drawing| drawing.id)
            .collect::<std::collections::HashSet<_>>();
        self.selected_drawings.retain(|id| live_ids.contains(id));
        self.selected_drawing = self.selected_drawings.first().copied();
        removed
    }

    pub fn apply_drawing_template_json(&mut self, id: DrawingId, json: &str) -> bool {
        let Ok(template) = serde_json::from_str::<crate::DrawingTemplate>(json) else {
            return false;
        };
        if !template.validate()
            || self
                .drawing(id)
                .is_none_or(|drawing| drawing.kind != template.kind)
        {
            return false;
        }
        let options = template.options.to_string();
        self.drawing_apply_options(id, &options)
    }

    pub fn drawing_template_json(&self, id: DrawingId, name: &str) -> Option<String> {
        if name.is_empty() || name.len() > crate::MAX_DRAWING_NAME_BYTES {
            return None;
        }
        let drawing = self.drawing(id)?;
        serde_json::to_string(&crate::DrawingTemplate {
            name: name.to_string(),
            kind: drawing.kind,
            options: drawing.options_json(),
        })
        .ok()
    }

    pub fn drawing_sync_payload_json(&self, source: &str) -> Option<String> {
        if source.is_empty() || source.len() > crate::MAX_DRAWING_GROUP_BYTES {
            return None;
        }
        let drawings = self
            .drawings
            .iter()
            .map(|drawing| crate::DrawingClipboardItem {
                id: Some(drawing.id),
                kind: drawing.kind,
                pane_index: drawing.pane_index,
                points: drawing.points.clone(),
                options: drawing.options_json(),
                bars_pattern: (drawing.kind == DrawingKind::BarsPattern)
                    .then(|| drawing.bars_pattern.clone()),
            })
            .collect();
        serde_json::to_string(&crate::DrawingSyncPayload {
            schema: "aeris_charts-drawing-sync".to_string(),
            source: source.to_string(),
            revision: self.drawing_sync_revision.max(1),
            drawings,
        })
        .ok()
    }

    /// Apply a complete cross-cell payload. Stale revisions and same-source echoes are ignored;
    /// all entries are validated and staged before replacing live semantic state.
    pub fn apply_drawing_sync_payload_json(&mut self, json: &str) -> bool {
        let Ok(payload) = serde_json::from_str::<crate::DrawingSyncPayload>(json) else {
            return false;
        };
        if payload.schema != "aeris_charts-drawing-sync"
            || payload.source.is_empty()
            || payload.source.len() > crate::MAX_DRAWING_GROUP_BYTES
            || payload.revision <= self.drawing_sync_revision
            || (payload.source == self.drawing_sync_source
                && payload.revision <= self.drawing_sync_revision)
            || payload.drawings.len() > crate::MAX_DRAWING_OBJECTS
        {
            return false;
        }
        let mut staged = Vec::with_capacity(payload.drawings.len());
        let mut ids = std::collections::HashSet::new();
        for item in payload.drawings {
            if item.pane_index >= self.panes.len()
                || !self.pane_uses_financial_time(item.pane_index)
                || !ids.insert(item.id.unwrap_or(0))
                || !item.kind.valid_point_count(item.points.len())
                || item
                    .points
                    .iter()
                    .any(|point| !point.logical.is_finite() || !point.price.is_finite())
            {
                return false;
            }
            let id = item.id.unwrap_or(0);
            if id == 0 {
                return false;
            }
            let mut drawing = Drawing::new(id, item.kind, item.pane_index, item.points);
            let Ok(options) = serde_json::to_string(&item.options) else {
                return false;
            };
            let Ok(patch) = serde_json::from_str::<DrawingPatch>(&options) else {
                return false;
            };
            if !drawing.apply_patch(patch) {
                return false;
            }
            if item.kind == DrawingKind::BarsPattern {
                let Some(bars) = item.bars_pattern.filter(|bars| valid_bars_pattern(bars)) else {
                    return false;
                };
                drawing.bars_pattern = bars;
            } else if item.bars_pattern.is_some() {
                return false;
            }
            staged.push(drawing);
        }
        self.drawings = staged;
        self.next_drawing_id = self
            .drawings
            .iter()
            .map(|drawing| drawing.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.drawing_sync_source = payload.source;
        self.drawing_sync_revision = payload.revision;
        self.selected_drawing = None;
        self.selected_drawings.clear();
        self.drawing_history.clear();
        self.drawing_runtime
            .borrow_mut()
            .rebuild_all(&self.drawings, self.panes.len());
        self.invalidate_frame_all();
        true
    }

    /// The drawing's anchors as a JSON `[{logical, price}, ...]` array. `None` for an unknown id.
    pub fn drawing_points_json(&self, id: DrawingId) -> Option<String> {
        Some(serde_json::to_string(&self.drawing(id)?.points).unwrap_or_default())
    }

    /// One anchor's media-px position (x pane-relative, y chart-top — the host's overlay
    /// coordinate space; the text editor positions itself with it). `None` for an unknown
    /// id/index or when the anchor cannot convert (stale pane, empty scale/data).
    pub fn drawing_point_to_coordinate(&self, id: DrawingId, index: usize) -> Option<(f64, f64)> {
        let drawing = self.drawing(id)?;
        let point = drawing.points.get(index)?;
        self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, *point)
    }

    /// The exact media-px text-run anchor shared by frame rendering and the host caret overlay.
    pub fn drawing_text_coordinate(&self, id: DrawingId) -> Option<(f64, f64)> {
        let drawing = self.drawing(id)?;
        let px = self.drawing_px(drawing)?;
        let pane = self.panes.get(drawing.pane_index)?;
        let size = drawing.resolved_text_size(self.options.get().layout.font_size);
        let (x, y, _, _) = Self::drawing_text_placement(
            drawing,
            &px,
            self.pane_w,
            pane.top,
            pane.height,
            size,
            TEXT_PAD,
        );
        Some((x, y))
    }

    /// Media-px text anchor plus clockwise radians, shared with the browser caret overlay.
    pub fn drawing_text_transform(&self, id: DrawingId) -> Option<(f64, f64, f64)> {
        let drawing = self.drawing(id)?;
        let px = self.drawing_px(drawing)?;
        let pane = self.panes.get(drawing.pane_index)?;
        let size = drawing.resolved_text_size(self.options.get().layout.font_size);
        let (x, y, _, angle) = Self::drawing_text_placement(
            drawing,
            &px,
            self.pane_w,
            pane.top,
            pane.height,
            size,
            TEXT_PAD,
        );
        Some((x, y, angle))
    }

    /// Topmost trend-line label/placeholder at a media-px point. This keeps the browser host
    /// from duplicating text measurement or 3×3 segment placement when opening inline edit.
    pub fn drawing_text_hit_at(&self, x: f64, y: f64) -> Option<DrawingId> {
        let layout = &self.options.get().layout;
        let pane_index = self.pane_at_y(y)?;
        let candidates = self.take_drawing_candidates(pane_index, Some((x, y)));
        let hit = candidates.iter().rev().find_map(|&id| {
            let drawing = self.drawing(id)?;
            if drawing.kind != DrawingKind::TrendLine
                || !drawing.visible
                || drawing.locked
                || !drawing.interval_visibility.allows(self.drawing_interval)
            {
                return None;
            }
            let text = if drawing.text.is_empty() {
                TREND_TEXT_PLACEHOLDER
            } else {
                drawing.display_text()
            };
            let px = self.drawing_px(drawing)?;
            let pane = self.panes.get(drawing.pane_index)?;
            let size = drawing.resolved_text_size(layout.font_size);
            let (tx, ty, align, angle) = Self::drawing_text_placement(
                drawing,
                &px,
                self.pane_w,
                pane.top,
                pane.height,
                size,
                TEXT_PAD,
            );
            let width = self.measure_text_run(
                text,
                size,
                &layout.font_family,
                drawing.text_weight.unwrap_or(400),
                drawing.text_italic,
            );
            let local_x = (x - tx) * angle.cos() + (y - ty) * angle.sin();
            let local_y = -(x - tx) * angle.sin() + (y - ty) * angle.cos();
            let left = match align {
                DrawingTextHAlign::Left => 0.0,
                DrawingTextHAlign::Center => -width / 2.0,
                DrawingTextHAlign::Right => -width,
            };
            let half_height = size * 0.6;
            (local_x >= left - TEXT_PAD
                && local_x <= left + width + TEXT_PAD
                && local_y >= -half_height - TEXT_PAD
                && local_y <= half_height + TEXT_PAD)
                .then_some(drawing.id)
        });
        self.recycle_drawing_candidates(candidates);
        hit
    }

    /// The drawing under a media-px pointer and the cursor it shows. A trend label or its
    /// `+ Add text` prompt wins with the text cursor, so moving from the line onto the label
    /// keeps the affordance; otherwise the body/handle hit and its drag cursor. Pure: hosts use
    /// it to resolve cursors without changing hover state.
    pub fn drawing_hover_at(&self, x: f64, y: f64) -> Option<(DrawingId, &'static str)> {
        if let Some(id) = self.drawing_text_hit_at(x, y) {
            return Some((id, "text"));
        }
        self.hit_test_drawing(x, y).map(|hit| (hit.id, hit.cursor))
    }

    /// Canonical drawing hover for every host: resolves [`Self::drawing_hover_at`] and applies
    /// the generic hover promotion plus the text hover ring / trend `+ Add text` prompt, or
    /// clears both. Returns the hovered drawing and its cursor.
    pub fn update_drawing_hover(&mut self, x: f64, y: f64) -> Option<(DrawingId, &'static str)> {
        let hover = self.drawing_hover_at(x, y);
        let id = hover.map(|(id, _)| id);
        self.set_hovered_text(id);
        self.set_hovered_drawing(id);
        hover
    }

    /// Every drawing as a JSON array of `{id, kind, pane_index, points, ...options}` in z-order.
    pub fn drawings_json(&self) -> String {
        let list: Vec<serde_json::Value> = self
            .drawings
            .iter()
            .map(|d| {
                let mut value = serde_json::json!({
                    "id": d.id,
                    "kind": d.kind.name(),
                    "pane_index": d.pane_index,
                    "points": d.points,
                });
                if let (serde_json::Value::Object(map), serde_json::Value::Object(options)) =
                    (&mut value, d.options_json())
                {
                    map.extend(options);
                }
                value
            })
            .collect();
        serde_json::to_string(&list).unwrap_or_default()
    }

    // --- selection ---

    /// The selected drawing (industry-standard click-to-select): while set, the frame build
    /// paints anchor handles at its defining points and its anchors accept drags. An unknown id
    /// never sticks.
    pub fn set_selected_drawing(&mut self, id: Option<DrawingId>) {
        self.invalidate_frame_overlay();
        self.selected_drawing = id.filter(|&sid| self.drawings.iter().any(|d| d.id == sid));
        self.selected_drawings = self.selected_drawing.into_iter().collect();
        if self.selected_drawing.is_some() {
            self.clear_volume_profile_selection();
        }
    }

    pub fn selected_drawing(&self) -> Option<DrawingId> {
        self.selected_drawing
    }

    /// Mark the text-capable drawing the host's typing-mode editor currently owns. The frame keeps
    /// painting the label and the focus border underneath the host's borderless caret overlay
    /// (the public reference's overlay-caret model); this flag is the host/query seam for that session.
    /// Cleared when the editor closes. An unknown id never sticks.
    pub fn set_editing_drawing(&mut self, id: Option<DrawingId>) {
        let valid = id.filter(|&eid| {
            self.drawings.iter().any(|d| {
                d.id == eid
                    && !d.locked
                    && d.visible
                    && d.interval_visibility.allows(self.drawing_interval)
                    && (matches!(d.kind, DrawingKind::Text | DrawingKind::TrendLine)
                        || d.kind.is_text_annotation())
            })
        });
        let changes_trend_placeholder =
            [self.editing_drawing, valid]
                .into_iter()
                .flatten()
                .any(|id| {
                    self.drawing(id).is_some_and(|drawing| {
                        drawing.kind == DrawingKind::TrendLine && drawing.text.is_empty()
                    })
                });
        if changes_trend_placeholder {
            self.invalidate_frame_drawings();
        } else {
            self.invalidate_frame_overlay();
        }
        self.editing_drawing = valid;
        // A session belongs to exactly one editing drawing; any other target ends it.
        if self
            .drawing_text_edit
            .as_ref()
            .is_some_and(|session| Some(session.id) != valid)
        {
            self.drawing_text_edit = None;
        }
    }

    pub fn editing_drawing(&self) -> Option<DrawingId> {
        self.editing_drawing
    }

    /// Mark a text drawing or trend line under the host pointer. Text drawings paint their
    /// hover ring; empty trend lines paint their inline `+ Add text` affordance.
    pub fn set_hovered_text(&mut self, id: Option<DrawingId>) {
        let valid = id.filter(|&hid| {
            self.drawings.iter().any(|d| {
                d.id == hid
                    && (matches!(d.kind, DrawingKind::Text | DrawingKind::TrendLine)
                        || d.kind.is_text_annotation())
            })
        });
        if valid != self.hovered_text {
            let changes_placeholder = [self.hovered_text, valid].into_iter().flatten().any(|id| {
                self.drawing(id).is_some_and(|drawing| {
                    drawing.kind == DrawingKind::TrendLine && drawing.text.is_empty()
                })
            });
            if changes_placeholder {
                self.invalidate_frame_drawings();
            } else {
                self.invalidate_frame_overlay();
            }
            self.hovered_text = valid;
        }
    }

    pub fn hovered_text(&self) -> Option<DrawingId> {
        self.hovered_text
    }

    /// Mark the drawing of any kind under the host's pointer for temporary hover promotion
    /// (ordering seam; no hover chrome except the text ring via `hovered_text`). An unknown
    /// id never sticks. Ordering-only: frame assembly reassembles retained geometry in the
    /// new order without rebuilding it, and hit testing keeps the stable z-order so
    /// promotion cannot oscillate hover. Cleared on hover leave, deselection-safe (selection
    /// is separate), cancellation, removal, and `clear_hover`.
    pub fn set_hovered_drawing(&mut self, id: Option<DrawingId>) {
        let valid = id.filter(|&hid| self.drawings.iter().any(|d| d.id == hid));
        if valid != self.hovered_drawing {
            // Ordering-only change: retained drawing geometry is reassembled, not rebuilt.
            // Overlay invalidation covers the text ring transition when the hovered drawing
            // is text; non-text hover has no chrome but still needs a repaint for promotion.
            self.invalidate_frame_overlay();
            self.hovered_drawing = valid;
        }
    }

    pub fn hovered_drawing(&self) -> Option<DrawingId> {
        self.hovered_drawing
    }

    /// Select the drawing under pane-relative media px `(x, y)` (the host click pipeline):
    /// the topmost hit is selected; a miss clears the selection. Returns whether a drawing was
    /// hit (the host then skips its series-selection path).
    pub fn select_drawing_at(&mut self, x: f64, y: f64) -> bool {
        self.invalidate_frame_overlay();
        self.selected_drawing = self.hit_test_drawing(x, y).map(|hit| hit.id);
        self.selected_drawings = self.selected_drawing.into_iter().collect();
        self.selected_drawing.is_some()
    }

    /// Remove the selected drawing (Delete/Backspace). Returns false while nothing is selected.
    pub fn remove_selected_drawing(&mut self) -> bool {
        let Some(id) = self.selected_drawing else {
            return false;
        };
        self.remove_drawing(id)
    }

    // --- hit testing ---

    /// The drawing under pane-relative media px `(x, y)`, or `None` off the panes/drawings.
    /// Anchor handles are hittable only on the SELECTED drawing (they are only painted then);
    /// bodies hit topmost-first in z-order within the pane under the cursor (hit_test.rs
    /// restricts hits to the hovered pane the same way).
    pub fn hit_test_drawing(&self, x: f64, y: f64) -> Option<DrawingHit> {
        self.hit_test_drawing_with_profile(x, y, HitProfile::PRECISION)
    }

    pub fn hit_test_drawing_with_profile(
        &self,
        x: f64,
        y: f64,
        profile: HitProfile,
    ) -> Option<DrawingHit> {
        self.hit_test_drawing_impl(x, y, true, profile)
    }

    /// Brute-force reference used by deterministic and randomized parity tests.
    #[doc(hidden)]
    pub fn hit_test_drawing_bruteforce(&self, x: f64, y: f64) -> Option<DrawingHit> {
        self.hit_test_drawing_impl(x, y, false, HitProfile::PRECISION)
    }

    fn hit_test_drawing_impl(
        &self,
        x: f64,
        y: f64,
        indexed: bool,
        profile: HitProfile,
    ) -> Option<DrawingHit> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || x > self.pane_w {
            return None;
        }
        let pane = self.pane_at_y(y)?;
        // The selected drawing's anchor handles win over every body (they paint above all).
        // The brush shows handles at its two ENDS only; the rectangle shows its eight
        // Eight conventional anchors (four corners + four edge midpoints); the rest show one per
        // defining anchor.
        if let Some(selected) = self.selected_drawing
            && let Some(drawing) = self.drawing(selected)
            && drawing.pane_index == pane
            && drawing.visible
            && drawing.interval_visibility.allows(self.drawing_interval)
            && let Some(px) = self.drawing_px(drawing)
        {
            match drawing.kind.spec().handles {
                DrawingHandleMode::None => {}
                DrawingHandleMode::Endpoints if !px.is_empty() => {
                    let last = px.len() - 1;
                    for index in [0, last] {
                        let (ax, ay) = px[index];
                        if (x - ax).hypot(y - ay) <= profile.drawing_anchor_radius {
                            return Some(DrawingHit {
                                id: selected,
                                part: DrawingDragPart::Anchor(index),
                                cursor: "pointer",
                            });
                        }
                    }
                }
                DrawingHandleMode::RectangleBounds if px.len() == 2 => {
                    let anchors = Self::rectangle_anchors(&px);
                    for (index, &(ax, ay)) in anchors.iter().enumerate() {
                        if (x - ax).hypot(y - ay) <= profile.drawing_anchor_radius {
                            return Some(DrawingHit {
                                id: selected,
                                part: DrawingDragPart::Anchor(index),
                                cursor: Self::rectangle_anchor_cursor(index),
                            });
                        }
                    }
                }
                DrawingHandleMode::Position if px.len() == 3 => {
                    let entry = px[0];
                    let target = px[1];
                    let stop = px[2];
                    let handles = [
                        (entry.0, target.1, "ns-resize"),
                        (entry.0, entry.1, "move"),
                        (target.0, entry.1, "ew-resize"),
                        (entry.0, stop.1, "ns-resize"),
                    ];
                    for (index, &(ax, ay, cursor)) in handles.iter().enumerate() {
                        if (x - ax).hypot(y - ay) <= profile.drawing_anchor_radius {
                            return Some(DrawingHit {
                                id: selected,
                                part: DrawingDragPart::Anchor(index),
                                cursor,
                            });
                        }
                    }
                }
                DrawingHandleMode::Anchors | DrawingHandleMode::Endpoints => {
                    for (index, &(ax, ay)) in px.iter().enumerate() {
                        if (x - ax).hypot(y - ay) <= profile.drawing_anchor_radius {
                            return Some(DrawingHit {
                                id: selected,
                                part: DrawingDragPart::Anchor(index),
                                cursor: "pointer",
                            });
                        }
                    }
                }
                DrawingHandleMode::RectangleBounds | DrawingHandleMode::Position => {}
            }
        }
        if !indexed {
            for drawing in self.drawings.iter().rev() {
                if drawing.pane_index != pane
                    || !drawing.visible
                    || !drawing.interval_visibility.allows(self.drawing_interval)
                {
                    continue;
                }
                let Some(px) = self.drawing_render_px(drawing) else {
                    continue;
                };
                if self.drawing_body_hit(drawing, &px, x, y, profile) {
                    return Some(DrawingHit {
                        id: drawing.id,
                        part: DrawingDragPart::Body,
                        cursor: "move",
                    });
                }
            }
            return None;
        }

        let candidates = self.take_drawing_candidates(pane, Some((x, y)));
        let mut hit = None;
        {
            let mut runtime = self.drawing_runtime.borrow_mut();
            for &id in candidates.iter().rev() {
                let Some(&position) = runtime.positions.get(&id) else {
                    continue;
                };
                let Some(drawing) = self.drawings.get(position) else {
                    continue;
                };
                if !drawing.visible || !drawing.interval_visibility.allows(self.drawing_interval) {
                    continue;
                }
                let Some(key) = self.drawing_coordinate_key(drawing) else {
                    continue;
                };
                let Some(px) = self.drawing_px_cached(drawing, &mut runtime, key) else {
                    continue;
                };
                let body_hit = self.drawing_body_hit(drawing, px, x, y, profile);
                runtime.record_precise_hit();
                if body_hit {
                    hit = Some(DrawingHit {
                        id,
                        part: DrawingDragPart::Body,
                        cursor: "move",
                    });
                    break;
                }
            }
        }
        self.recycle_drawing_candidates(candidates);
        hit
    }

    /// The per-kind body test at media px `(x, y)` against the converted anchors `px`.
    fn drawing_body_hit(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        x: f64,
        y: f64,
        profile: HitProfile,
    ) -> bool {
        let hit_tolerance = profile.drawing_stroke_tolerance;
        let tolerance = drawing.width / 2.0 + hit_tolerance;
        if drawing.kind == DrawingKind::BarsPattern && !drawing.bars_pattern.is_empty() {
            let tick = (self.time_scale.bar_spacing() * 0.25).clamp(2.0, 6.0);
            let mut previous = None;
            for &bar in &drawing.bars_pattern {
                let projected = bar.project(drawing);
                let mut encoded = [(0.0, 0.0); 4];
                let mut valid = true;
                for (slot, point) in encoded.iter_mut().zip(projected) {
                    if let Some(value) =
                        self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)
                    {
                        *slot = value;
                    } else {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    continue;
                }
                if drawing.bars_pattern_mode == "bars" {
                    let bar_x = encoded[0].0;
                    if distance_to_segment(x, y, bar_x, encoded[1].1, bar_x, encoded[2].1)
                        <= tolerance
                        || distance_to_segment(
                            x,
                            y,
                            bar_x - tick,
                            encoded[0].1,
                            bar_x,
                            encoded[0].1,
                        ) <= tolerance
                        || distance_to_segment(
                            x,
                            y,
                            bar_x,
                            encoded[3].1,
                            bar_x + tick,
                            encoded[3].1,
                        ) <= tolerance
                    {
                        return true;
                    }
                } else {
                    let index = match drawing.bars_pattern_mode.as_str() {
                        "line_open" => 0,
                        "line_high" => 1,
                        "line_low" => 2,
                        _ => 3,
                    };
                    let current = encoded[index];
                    if previous.is_some_and(|(ax, ay)| {
                        distance_to_segment(x, y, ax, ay, current.0, current.1) <= tolerance
                    }) {
                        return true;
                    }
                    previous = Some(current);
                }
            }
            return false;
        }
        let Some(pane) = self.panes.get(drawing.pane_index) else {
            return false;
        };
        let Some(geometry) = resolve_drawing_geometry(
            drawing.kind,
            px,
            self.pane_w,
            pane.top,
            pane.height,
            DrawingGeometryOptions {
                line_width: drawing.width,
                device_scale: 1.0,
                extend_left: drawing.extend_left,
                icon_size: drawing.icon_size,
                extend_right: drawing.extend_right,
            },
        ) else {
            return false;
        };
        match geometry.body {
            DrawingBodyGeometry::Segment { a, b } => {
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }
            DrawingBodyGeometry::Horizontal { y: line_y, x0, x1 } => {
                (y - line_y).abs() <= tolerance
                    && x1 >= x0
                    && x >= x0 - hit_tolerance
                    && x <= x1 + hit_tolerance
            }
            DrawingBodyGeometry::Vertical { x: line_x, y0, y1 } => {
                (x - line_x).abs() <= tolerance
                    && y >= y0.min(y1) - hit_tolerance
                    && y <= y0.max(y1) + hit_tolerance
            }
            DrawingBodyGeometry::Cross {
                x: line_x,
                y: line_y,
                pane_w,
                pane_top,
                pane_bottom,
            } => {
                ((y - line_y).abs() <= tolerance
                    && x >= -hit_tolerance
                    && x <= pane_w + hit_tolerance)
                    || ((x - line_x).abs() <= tolerance
                        && y >= pane_top - hit_tolerance
                        && y <= pane_bottom + hit_tolerance)
            }
            DrawingBodyGeometry::Channel { first, second } => {
                (drawing.fill_enabled
                    && self.selected_drawing == Some(drawing.id)
                    && point_in_polygon((x, y), &[first[0], first[1], second[1], second[0]]))
                    || distance_to_segment(x, y, first[0].0, first[0].1, first[1].0, first[1].1)
                        <= tolerance
                    || distance_to_segment(x, y, second[0].0, second[0].1, second[1].0, second[1].1)
                        <= tolerance
            }
            DrawingBodyGeometry::Regression {
                center,
                upper,
                lower,
            } => [center, upper, lower].into_iter().any(|segment| {
                distance_to_segment(x, y, segment[0].0, segment[0].1, segment[1].0, segment[1].1)
                    <= tolerance
            }),
            DrawingBodyGeometry::Fibonacci(fib) => drawing.levels.iter().any(|level| {
                if !level.visible {
                    return false;
                }
                let (a, b) = self.drawing_fibonacci_level_segment(drawing, fib, level.value, 1.0);
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }),
            DrawingBodyGeometry::TimeLevels(time) => {
                y >= time.pane_top - hit_tolerance
                    && y <= time.pane_bottom + hit_tolerance
                    && drawing.levels.iter().any(|level| {
                        level.visible
                            && (x - time.x(drawing.level_value(level.value))).abs() <= tolerance
                    })
            }
            DrawingBodyGeometry::FibonacciArcs(arcs) => {
                (arcs.kind == DrawingKind::FibonacciWedge
                    && px[1..3].iter().any(|side| {
                        distance_to_segment(x, y, px[0].0, px[0].1, side.0, side.1) <= tolerance
                    }))
                    || drawing.levels.iter().any(|level| {
                        level.visible
                            && drawing.level_value(level.value) > 0.0
                            && (0..arcs.segments()).any(|step| {
                                let segments = f64::from(arcs.segments());
                                let a = arcs.point(
                                    drawing.level_value(level.value),
                                    f64::from(step) / segments,
                                );
                                let b = arcs.point(
                                    drawing.level_value(level.value),
                                    f64::from(step + 1) / segments,
                                );
                                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                            })
                    })
            }
            DrawingBodyGeometry::Pitchfork(fork) => drawing.levels.iter().any(|level| {
                if !level.visible {
                    return false;
                }
                let (a, b) = fork.segment(drawing.level_value(level.value));
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }),
            DrawingBodyGeometry::Cycles(cycles) => {
                y >= cycles.pane_top - hit_tolerance && y <= cycles.pane_bottom + hit_tolerance && {
                    let mut hit = false;
                    cycles.for_each_visible_line(|_, line_x| {
                        hit |= (x - line_x).abs() <= tolerance;
                    });
                    hit
                }
            }
            DrawingBodyGeometry::Sine(sine) => {
                let Some((left, right)) = sine.visible_x() else {
                    return false;
                };
                if x < left - tolerance || x > right + tolerance {
                    return false;
                }
                let count = sine.sample_count();
                let center = (((x - left) / (right - left) * f64::from(count)).floor() as u32)
                    .min(count - 1);
                (center.saturating_sub(1)..=(center + 1).min(count - 1)).any(|step| {
                    let ax = left + (right - left) * f64::from(step) / f64::from(count);
                    let bx = left + (right - left) * f64::from(step + 1) / f64::from(count);
                    distance_to_segment(x, y, ax, sine.y(ax), bx, sine.y(bx)) <= tolerance
                })
            }
            DrawingBodyGeometry::Marker(marker) => {
                let triangle = marker.triangle();
                point_in_polygon((x, y), &triangle)
                    || (0..3).any(|index| {
                        let a = triangle[index];
                        let b = triangle[(index + 1) % 3];
                        distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                    })
                    || marker.stem().is_some_and(|(a, b)| {
                        distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                    })
            }
            DrawingBodyGeometry::PriceLabel {
                x: label_x,
                y: label_y,
            } => {
                let label = if drawing.text.is_empty() {
                    self.price_formatter.format(drawing.points[0].price)
                } else {
                    drawing.text.clone()
                };
                let layout = &self.options.get().layout;
                let size = drawing.resolved_text_size(layout.font_size);
                let width = self.measure_text_run(
                    &label,
                    size,
                    &layout.font_family,
                    drawing.text_weight.unwrap_or(400),
                    drawing.text_italic,
                );
                x >= label_x - width - 8.0 - hit_tolerance
                    && x <= label_x + hit_tolerance
                    && y >= label_y - size * 0.6 - 4.0 - hit_tolerance
                    && y <= label_y + size * 0.6 + 4.0 + hit_tolerance
            }
            DrawingBodyGeometry::IconStamp { center, size } => {
                (x - center.0).abs() <= size / 2.0 + hit_tolerance
                    && (y - center.1).abs() <= size / 2.0 + hit_tolerance
            }
            DrawingBodyGeometry::GannGrid(grid) => {
                let bounds = grid.bounds();
                let corners = [
                    (bounds.left, bounds.top),
                    (bounds.right, bounds.top),
                    (bounds.right, bounds.bottom),
                    (bounds.left, bounds.bottom),
                ];
                (drawing.fill_enabled
                    && self.selected_drawing == Some(drawing.id)
                    && x >= bounds.left
                    && x <= bounds.right
                    && y >= bounds.top
                    && y <= bounds.bottom)
                    || (0..4).any(|index| {
                        let a = corners[index];
                        let b = corners[(index + 1) % 4];
                        distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                    })
                    || drawing
                        .levels
                        .iter()
                        .filter(|level| level.visible)
                        .any(|level| {
                            grid.level_lines(drawing.level_value(level.value))
                                .iter()
                                .any(|&(a, b)| {
                                    distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                                })
                        })
                    || drawing
                        .gann_fans
                        .iter()
                        .filter(|level| level.visible)
                        .any(|level| {
                            let (a, b) = grid.fan_segment(level.value, drawing.level_reverse);
                            distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                        })
                    || drawing
                        .gann_arcs
                        .iter()
                        .filter(|level| level.visible)
                        .any(|level| {
                            (0..32).any(|step| {
                                let a = grid.arc_point(
                                    level.value,
                                    f64::from(step) / 32.0,
                                    drawing.level_reverse,
                                );
                                let b = grid.arc_point(
                                    level.value,
                                    f64::from(step + 1) / 32.0,
                                    drawing.level_reverse,
                                );
                                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                            })
                        })
            }
            DrawingBodyGeometry::Quad { corners } => {
                (drawing.fill_enabled
                    && self.selected_drawing == Some(drawing.id)
                    && point_in_polygon((x, y), &corners))
                    || (0..4).any(|index| {
                        let a = corners[index];
                        let b = corners[(index + 1) % 4];
                        distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                    })
            }
            DrawingBodyGeometry::Ellipse { center, rx, ry } => {
                if rx <= 0.0 || ry <= 0.0 {
                    false
                } else if drawing.fill_enabled
                    && self.selected_drawing == Some(drawing.id)
                    && ((x - center.0) / rx).powi(2) + ((y - center.1) / ry).powi(2) <= 1.0
                {
                    true
                } else {
                    (0..64).any(|step| {
                        let theta0 = std::f64::consts::TAU * step as f64 / 64.0;
                        let theta1 = std::f64::consts::TAU * (step + 1) as f64 / 64.0;
                        let a = (center.0 + rx * theta0.cos(), center.1 + ry * theta0.sin());
                        let b = (center.0 + rx * theta1.cos(), center.1 + ry * theta1.sin());
                        distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                    })
                }
            }
            DrawingBodyGeometry::Circle { center, radius } => {
                let distance = (x - center.0).hypot(y - center.1);
                (distance - radius).abs() <= tolerance
                    || (drawing.fill_enabled
                        && self.selected_drawing == Some(drawing.id)
                        && distance <= radius)
            }
            DrawingBodyGeometry::Triangle { corners } => {
                (drawing.fill_enabled
                    && self.selected_drawing == Some(drawing.id)
                    && point_in_polygon((x, y), &corners))
                    || (0..3).any(|index| {
                        let a = corners[index];
                        let b = corners[(index + 1) % 3];
                        distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
                    })
            }
            DrawingBodyGeometry::Arc(arc) => (0..64).any(|step| {
                let a = arc.point(step as f64 / 64.0);
                let b = arc.point((step + 1) as f64 / 64.0);
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }),
            DrawingBodyGeometry::Curve(curve) => (0..64).any(|step| {
                let a = curve.point(step as f64 / 64.0);
                let b = curve.point((step + 1) as f64 / 64.0);
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }),
            DrawingBodyGeometry::Rectangle {
                left,
                right,
                top,
                bottom,
            } => {
                // the public reference: the fill is a drag surface only while the drawing is SELECTED
                // (a border click selects first); unselected, the body hits just the band
                // around the border frame and the middle pans the chart.
                let within_x = x >= left - tolerance && x <= right + tolerance;
                let within_y = y >= top - tolerance && y <= bottom + tolerance;
                if !within_x || !within_y {
                    return false;
                }
                if self.selected_drawing == Some(drawing.id) {
                    return true;
                }
                let near_v_edge = (x - left).abs() <= tolerance || (x - right).abs() <= tolerance;
                let near_h_edge = (y - top).abs() <= tolerance || (y - bottom).abs() <= tolerance;
                near_v_edge || near_h_edge
            }
            DrawingBodyGeometry::Position(position) => {
                x >= position.left - tolerance
                    && x <= position.right + tolerance
                    && y >= position.top() - tolerance
                    && y <= position.bottom() + tolerance
            }
            // The measured area is the drag surface, matching the filled box the tool paints.
            DrawingBodyGeometry::Measure(measure) => {
                x >= measure.left() - tolerance
                    && x <= measure.right() + tolerance
                    && y >= measure.top() - tolerance
                    && y <= measure.bottom() + tolerance
            }
            DrawingBodyGeometry::Polyline {
                points,
                line_type,
                terminal,
            } => {
                if crate::hit_test::hit_test_line_series(
                    points,
                    x,
                    y,
                    line_type,
                    drawing.width,
                    None,
                    self.time_scale.bar_spacing(),
                    hit_tolerance,
                )
                .is_some()
                {
                    return true;
                }
                let Some(terminal) = terminal else {
                    return false;
                };
                crate::hit_test::hit_test_line_series(
                    &terminal,
                    x,
                    y,
                    LineType::Simple,
                    drawing.width,
                    None,
                    self.time_scale.bar_spacing(),
                    hit_tolerance,
                )
                .is_some()
            }
            DrawingBodyGeometry::Empty => {
                if drawing.kind != DrawingKind::Text && !drawing.kind.is_text_annotation() {
                    return false;
                }
                // The click/hover target is the interaction-chrome box (the label run while
                // non-empty, else a one-em caret box — empty text paints nothing on the chart)
                // plus the editing chrome's border+padding, so the painted hover/focus border
                // is itself hittable.
                let layout = &self.options.get().layout;
                let size = drawing.resolved_text_size(layout.font_size);
                let reference = geometry.text_box;
                let (tx, ty, align, _) = Self::text_placement(drawing, &reference, size, TEXT_PAD);
                let width = self.measure_drawing_text(drawing, size);
                let height = size * 1.2;
                let left = match align {
                    DrawingTextHAlign::Left => tx,
                    DrawingTextHAlign::Center => tx - width / 2.0,
                    DrawingTextHAlign::Right => tx - width,
                };
                x >= left - TEXT_CHROME_PAD
                    && x <= left + width + TEXT_CHROME_PAD
                    && y >= ty - height / 2.0 - TEXT_CHROME_PAD
                    && y <= ty + height / 2.0 + TEXT_CHROME_PAD
            }
        }
    }

    // --- drag session (anchor re-anchoring / whole-body move) ---

    /// Hit-test `(x, y)` and open a drag session on the hit part (body move or anchor
    /// re-anchor). A successful grab also selects the drawing (reference-informed behavior). Returns
    /// false on a miss — the host falls through to its pan/scroll handling.
    pub fn drawing_drag_start_at(&mut self, x: f64, y: f64) -> bool {
        self.drawing_drag_start_at_with_profile(x, y, HitProfile::PRECISION)
    }

    pub fn drawing_drag_start_at_with_profile(
        &mut self,
        x: f64,
        y: f64,
        profile: HitProfile,
    ) -> bool {
        self.invalidate_frame_overlay();
        let Some(hit) = self.hit_test_drawing_with_profile(x, y, profile) else {
            return false;
        };
        let Some(drawing) = self.drawing(hit.id) else {
            return false;
        };
        if drawing.locked {
            // Locked objects remain selectable/hit-testable but never open a mutation session.
            self.selected_drawing = Some(hit.id);
            self.selected_drawings = vec![hit.id];
            return false;
        }
        let start_points = drawing.points.clone();
        let screen_position = (drawing.screen_x, drawing.screen_y);
        let history_bars_pattern = drawing.bars_pattern.clone();
        let Some(start_px) = self.drawing_px(drawing) else {
            return false;
        };
        self.selected_drawing = Some(hit.id);
        self.drawing_drag = Some(DrawingDrag {
            id: hit.id,
            part: hit.part,
            start_x: x,
            start_y: y,
            current_x: x,
            current_y: y,
            history_points: start_points.clone(),
            history_screen_position: screen_position,
            history_bars_pattern,
            start_points,
            start_px,
        });
        true
    }

    /// Apply the drag to `(x, y)`: an anchor drag re-anchors that point under the cursor; a
    /// body drag translates every anchor in coordinate space and converts back per anchor, so
    /// the shape stays pixel-rigid on any scale mode. The unused coordinate of the full-span
    /// kinds is frozen (a horizontal line moves only vertically, a vertical line only
    /// horizontally). `modifiers` applies magnet (rendered-price snap) before straighten
    /// (0°/45°/90° for a trend anchor, square for a rectangle corner, dominant-axis for a body
    /// move) — the session recomputes from its start snapshot each call, so toggling a modifier
    /// mid-drag responds live (reference-informed behavior).
    pub fn drawing_drag_to(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) {
        self.invalidate_frame_drawings();
        let Some(drag) = self.drawing_drag.as_mut() else {
            return;
        };
        drag.current_x = x;
        drag.current_y = y;
        let start_x = drag.start_x;
        let (dx, dy) = (x - start_x, y - drag.start_y);
        let (id, part) = (drag.id, drag.part);
        let (start_points, start_px) = (drag.start_points.clone(), drag.start_px.clone());
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        let (kind, pane, price_scale, snap_time_to_data) = (
            drawing.kind,
            drawing.pane_index,
            drawing.price_scale,
            drawing.snap_time_to_data,
        );
        if kind == DrawingKind::AnchoredText {
            let (dx, dy) = if modifiers.straighten && part == DrawingDragPart::Body {
                if dx.abs() >= dy.abs() {
                    (dx, 0.0)
                } else {
                    (0.0, dy)
                }
            } else {
                (dx, dy)
            };
            let Some(&(start_px_x, start_px_y)) = start_px.first() else {
                return;
            };
            let Some(pane_bounds) = self.panes.get(pane) else {
                return;
            };
            let (pane_top, pane_height) = (pane_bounds.top, pane_bounds.height);
            let new_x = (start_px_x + dx).clamp(0.0, self.pane_w);
            let new_y = (start_px_y + dy).clamp(pane_top, pane_top + pane_height);
            let market_point = self.drawing_from_px_for(pane, price_scale, new_x, new_y);
            if let Some(drawing) = self.drawings.iter_mut().find(|drawing| drawing.id == id) {
                drawing.screen_x = (new_x / self.pane_w.max(1.0)).clamp(0.0, 1.0);
                drawing.screen_y = ((new_y - pane_top) / pane_height.max(1.0)).clamp(0.0, 1.0);
                if let Some(point) = market_point {
                    drawing.points[0] = point;
                }
            }
            self.update_drawing_runtime(id);
            return;
        }
        let mut points = start_points;
        let convert = |index: usize, dx: f64, dy: f64| -> Option<DrawingPoint> {
            let (px, py) = start_px.get(index)?;
            self.drawing_from_px_for(pane, price_scale, px + dx, py + dy)
        };
        match part {
            DrawingDragPart::Anchor(index) => {
                if kind.spec().handles == DrawingHandleMode::Position
                    && points.len() == 3
                    && index < 4
                {
                    let Some(mut cursor_pt) = self.drawing_from_px_for(pane, price_scale, x, y)
                    else {
                        return;
                    };
                    if modifiers.magnet && index != 2 {
                        cursor_pt = self.magnet_snap_point_at(pane, price_scale, x, y, cursor_pt);
                    }
                    if matches!(index, 1 | 2) {
                        cursor_pt.logical = self.snapped_crosshair_index(x) as f64;
                    }
                    cursor_pt.price = self.snap_position_price(pane, price_scale, cursor_pt.price);
                    match index {
                        // Target: vertical level only.
                        0 => {
                            points[1].price = match kind {
                                DrawingKind::LongPosition => cursor_pt.price.max(points[0].price),
                                DrawingKind::ShortPosition => cursor_pt.price.min(points[0].price),
                                _ => cursor_pt.price,
                            };
                        }
                        // Entry/origin: move the entry level and the origin edge. Keep the stop
                        // point on that edge so its x never becomes an independent corner.
                        1 => {
                            let low = points[1].price.min(points[2].price);
                            let high = points[1].price.max(points[2].price);
                            points[0] = DrawingPoint {
                                logical: cursor_pt.logical,
                                price: cursor_pt.price.clamp(low, high),
                            };
                            points[2].logical = cursor_pt.logical;
                        }
                        // Horizontal extent: x only.
                        2 => points[1].logical = cursor_pt.logical,
                        // Stop: vertical level only.
                        3 => {
                            points[2].price = match kind {
                                DrawingKind::LongPosition => cursor_pt.price.min(points[0].price),
                                DrawingKind::ShortPosition => cursor_pt.price.max(points[0].price),
                                _ => cursor_pt.price,
                            };
                        }
                        _ => unreachable!(),
                    }
                    if let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) {
                        drawing.points = points;
                    }
                    self.update_drawing_runtime(id);
                    return;
                }
                if kind.spec().handles == DrawingHandleMode::RectangleBounds
                    && points.len() == 2
                    && index < 8
                {
                    // Rectangle anchors (drawings.rs `rectangle_anchors` clock order): a corner
                    // drag moves that corner (Shift squares against the fixed opposite corner),
                    // an edge-midpoint drag moves only that edge. Every slot KEEPS its corner
                    // identity — crossing the opposite side flips visually at render (the box
                    // normalizes), it never reorders the anchors or slides the fixed side
                    // (reference-informed behavior).
                    let Some(mut cursor_pt) = self.drawing_from_px_for(pane, price_scale, x, y)
                    else {
                        return;
                    };
                    if snap_time_to_data {
                        let Some(snapped) = self.snap_drawing_time_to_data(cursor_pt) else {
                            return;
                        };
                        cursor_pt = snapped;
                    }
                    if modifiers.magnet {
                        cursor_pt = self.magnet_snap_point_at(pane, price_scale, x, y, cursor_pt);
                    }
                    let Some((mx, my)) = self.drawing_to_px_for(pane, price_scale, cursor_pt)
                    else {
                        return;
                    };
                    let (a, b) = (start_px[0], start_px[1]);
                    let (mut xs, mut ys) = ([a.0, b.0], [a.1, b.1]);
                    let (l, r) = (a.0.min(b.0), a.0.max(b.0));
                    let (t, bo) = (a.1.min(b.1), a.1.max(b.1));
                    match index {
                        0 | 2 | 4 | 6 => {
                            let (fx, fy) = match index {
                                0 => (r, bo),
                                2 => (l, bo),
                                4 => (l, t),
                                _ => (r, t),
                            };
                            let (nx, ny) = if modifiers.straighten {
                                // Shift-square: the larger dragged side wins, the drag quadrant's
                                // signs kept (straighten_point's rect arm).
                                let side = (mx - fx).abs().max((my - fy).abs());
                                (
                                    fx + if mx < fx { -side } else { side },
                                    fy + if my < fy { -side } else { side },
                                )
                            } else {
                                (mx, my)
                            };
                            // The dragged corner's x/y stay on their slots (identity
                            // preserved through flips; the fixed corner's slots untouched).
                            let x_slot = match index {
                                0 | 6 => usize::from(a.0 > b.0), // left corners
                                _ => usize::from(a.0 < b.0),     // right corners (2 | 4)
                            };
                            let y_slot = match index {
                                0 | 2 => usize::from(a.1 > b.1), // top corners
                                _ => usize::from(a.1 < b.1),     // bottom corners (4 | 6)
                            };
                            xs[x_slot] = nx;
                            ys[y_slot] = ny;
                        }
                        1 => ys[usize::from(a.1 > b.1)] = my, // top edge
                        3 => xs[usize::from(a.0 < b.0)] = mx, // right edge
                        5 => ys[usize::from(a.1 < b.1)] = my, // bottom edge
                        _ => xs[usize::from(a.0 > b.0)] = mx, // left edge (7)
                    }
                    let (Some(mut p0), Some(mut p1)) = (
                        self.drawing_from_px_for(pane, price_scale, xs[0], ys[0]),
                        self.drawing_from_px_for(pane, price_scale, xs[1], ys[1]),
                    ) else {
                        return;
                    };
                    if snap_time_to_data {
                        let (Some(snapped0), Some(snapped1)) = (
                            self.snap_drawing_time_to_data(p0),
                            self.snap_drawing_time_to_data(p1),
                        ) else {
                            return;
                        };
                        p0 = snapped0;
                        p1 = snapped1;
                    }
                    points[0] = p0;
                    points[1] = p1;
                    if let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) {
                        drawing.points = points;
                    }
                    if kind.spec().placement.is_freehand() {
                        self.invalidate_brush_drag_runtime(id);
                    } else {
                        self.update_drawing_runtime(id);
                    }
                    return;
                }
                if index >= points.len() {
                    return;
                }
                let (dx, dy) = kind.spec().movement_axis.constrain(dx, dy);
                let Some(mut point) = convert(index, dx, dy) else {
                    return;
                };
                if snap_time_to_data {
                    let Some(snapped) = self.snap_drawing_time_to_data(point) else {
                        return;
                    };
                    point = snapped;
                }
                if modifiers.magnet {
                    let snapped = self.magnet_snap_point_at(pane, price_scale, x, y, point);
                    point = match kind.spec().movement_axis {
                        DrawingMovementAxis::VerticalOnly => DrawingPoint {
                            price: snapped.price,
                            ..point
                        },
                        DrawingMovementAxis::HorizontalOnly => DrawingPoint {
                            logical: snapped.logical,
                            ..point
                        },
                        DrawingMovementAxis::Both => snapped,
                    };
                }
                if modifiers.straighten && points.len() == 2 {
                    // The other anchor is the fixed one (only the dragged anchor moves).
                    let fixed = points[1 - index];
                    if let Some(snapped) =
                        self.straighten_point(pane, price_scale, kind, fixed, point)
                    {
                        point = snapped;
                    }
                }
                let anchor_x = start_px.get(index).map_or(x, |&(px, _)| px + dx);
                point = self.grid_snap_point(
                    kind,
                    pane,
                    price_scale,
                    anchor_x,
                    point,
                    modifiers.magnet,
                );
                points[index] = point;
            }
            DrawingDragPart::Body => {
                // Reference-informed shift-move: constrain the translation to the dominant axis.
                let (dx, dy) = if modifiers.straighten {
                    if dx.abs() >= dy.abs() {
                        (dx, 0.0)
                    } else {
                        (0.0, dy)
                    }
                } else {
                    (dx, dy)
                };
                let single_anchor = points.len() == 1;
                let grid = kind.spec().grid_snap;
                // Follow the crosshair's slot changes from the grabbed point. One shared
                // logical delta moves the body rigidly and preserves the grab offset/width.
                let time_steps = if grid {
                    (self.snapped_crosshair_index(start_x + dx)
                        - self.snapped_crosshair_index(start_x)) as f64
                } else {
                    0.0
                };
                for (index, slot) in points.iter_mut().enumerate() {
                    let (dx, dy) = kind.spec().movement_axis.constrain(dx, dy);
                    let Some(mut point) = convert(index, dx, dy) else {
                        return;
                    };
                    if grid {
                        point.logical = slot.logical + time_steps;
                    }
                    if snap_time_to_data {
                        let Some(snapped) = self.snap_drawing_time_to_data(point) else {
                            return;
                        };
                        point = snapped;
                    }
                    // Single-anchor kinds drag by their line, not a handle — the body drag IS
                    // the anchor drag, so the magnet applies here too (a Ctrl-dragged vertical
                    // line snaps to bar centers, a horizontal one to the nearest rendered price).
                    if modifiers.magnet && single_anchor {
                        let snapped = self.magnet_snap_point_at(pane, price_scale, x, y, point);
                        point = match kind.spec().movement_axis {
                            DrawingMovementAxis::VerticalOnly => DrawingPoint {
                                price: snapped.price,
                                ..point
                            },
                            DrawingMovementAxis::HorizontalOnly => DrawingPoint {
                                logical: snapped.logical,
                                ..point
                            },
                            DrawingMovementAxis::Both => snapped,
                        };
                    }
                    if grid {
                        point.price = self.snap_position_price(pane, price_scale, point.price);
                    }
                    *slot = point;
                }
            }
        }
        if let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) {
            drawing.points = points;
        }
        if kind.spec().placement.is_freehand() {
            self.invalidate_brush_drag_runtime(id);
        } else {
            self.update_drawing_runtime(id);
        }
    }

    /// Close the drag session (pointer up/cancel).
    pub fn drawing_drag_end(&mut self) {
        self.invalidate_frame_overlay();
        if let Some(drag) = self.drawing_drag.take() {
            let id = drag.id;
            if matches!(drag.part, DrawingDragPart::Anchor(0 | 1))
                && let Some(drawing) = self
                    .drawing(id)
                    .cloned()
                    .filter(|drawing| drawing.kind == DrawingKind::BarsPattern)
            {
                if let Some(bars) = self.capture_bars_pattern(&drawing) {
                    if let Some(current) = self.drawings.iter_mut().find(|item| item.id == id) {
                        current.bars_pattern = bars;
                    }
                } else if let Some(current) = self.drawings.iter_mut().find(|item| item.id == id) {
                    current.points = drag.history_points.clone();
                }
            }
            self.update_drawing_runtime(id);
            if let Some(after) = self.drawing(id).cloned() {
                let mut before = after.clone();
                before.points = drag.history_points;
                before.screen_x = drag.history_screen_position.0;
                before.screen_y = drag.history_screen_position.1;
                before.bars_pattern = drag.history_bars_pattern;
                if before != after {
                    self.drawing_anchor_times.remove(&id);
                    self.record_drawing_command(DrawingCommand::Update {
                        before,
                        after: Box::new(after),
                    });
                }
            }
        }
    }

    /// Abort an interrupted host drag and restore its exact semantic start snapshot. Capture loss,
    /// window blur, visibility loss, and pointer cancellation must never commit a partial edit.
    pub fn drawing_drag_cancel(&mut self) {
        self.invalidate_frame_overlay();
        let Some(drag) = self.drawing_drag.take() else {
            return;
        };
        if let Some(drawing) = self
            .drawings
            .iter_mut()
            .find(|drawing| drawing.id == drag.id)
        {
            drawing.points = drag.history_points;
            drawing.screen_x = drag.history_screen_position.0;
            drawing.screen_y = drag.history_screen_position.1;
            drawing.bars_pattern = drag.history_bars_pattern;
            self.update_drawing_runtime(drag.id);
            self.invalidate_frame_drawings();
        }
    }

    pub fn drawing_drag_active(&self) -> bool {
        self.drawing_drag.is_some()
    }

    /// Keyboard-equivalent movement through the same drag/history path as pointer input.
    /// `anchor` selects one defining anchor; `None` moves the whole drawing.
    pub fn nudge_selected_drawing(
        &mut self,
        dx_css: f64,
        dy_css: f64,
        anchor: Option<usize>,
    ) -> bool {
        self.nudge_selected_drawing_with_history(dx_css, dy_css, anchor, true)
    }

    /// [`Self::nudge_selected_drawing`]; without `record`, the step joins an open keyboard edit
    /// that commits through [`Self::record_drawing_edit`].
    pub(crate) fn nudge_selected_drawing_with_history(
        &mut self,
        dx_css: f64,
        dy_css: f64,
        anchor: Option<usize>,
        record: bool,
    ) -> bool {
        if !dx_css.is_finite() || !dy_css.is_finite() || (dx_css == 0.0 && dy_css == 0.0) {
            return false;
        }
        let Some(id) = self.selected_drawing else {
            return false;
        };
        let Some(drawing) = self.drawing(id) else {
            return false;
        };
        if anchor.is_some_and(|index| index >= drawing.points.len()) {
            return false;
        }
        let start_points = drawing.points.clone();
        let screen_position = (drawing.screen_x, drawing.screen_y);
        let Some(start_px) = self.drawing_px(drawing) else {
            return false;
        };
        self.drawing_drag = Some(DrawingDrag {
            id,
            part: anchor.map_or(DrawingDragPart::Body, DrawingDragPart::Anchor),
            start_x: 0.0,
            start_y: 0.0,
            current_x: 0.0,
            current_y: 0.0,
            history_points: start_points.clone(),
            history_screen_position: screen_position,
            history_bars_pattern: drawing.bars_pattern.clone(),
            start_points,
            start_px,
        });
        self.drawing_drag_to(dx_css, dy_css, DrawingModifiers::default());
        if record {
            self.drawing_drag_end();
        } else if let Some(drag) = self.drawing_drag.take() {
            self.update_drawing_runtime(drag.id);
            self.invalidate_frame_overlay();
        }
        true
    }

    /// Record the change from `before` to the drawing's current state as one undo entry.
    pub(crate) fn record_drawing_edit(&mut self, before: Drawing) {
        let Some(after) = self.drawing(before.id).cloned() else {
            return;
        };
        if before != after {
            self.drawing_anchor_times.remove(&before.id);
            self.record_drawing_command(DrawingCommand::Update {
                before,
                after: Box::new(after),
            });
        }
    }

    /// Restore a drawing's anchors from `before` without recording history.
    pub(crate) fn restore_drawing_points(&mut self, before: Drawing) {
        let id = before.id;
        if let Some(drawing) = self.drawings.iter_mut().find(|drawing| drawing.id == id) {
            drawing.points = before.points;
            drawing.screen_x = before.screen_x;
            drawing.screen_y = before.screen_y;
            self.update_drawing_runtime(id);
            self.invalidate_frame_drawings();
        }
    }

    // --- drawing-tool controller --------------------------------------------------------------

    fn drawing_template(kind: DrawingKind, options_json: Option<&str>) -> Option<Drawing> {
        let mut template = Drawing::new(0, kind, 0, Vec::new());
        if let Some(json) = options_json {
            let patch = serde_json::from_str::<DrawingPatch>(json).ok()?;
            template.apply_patch(patch);
        }
        Some(template)
    }

    /// Arm or disarm the chart's canonical drawing tool. The host may optionally constrain the
    /// next drawing to one pane; placement, sequence completion, freehand capture and one-shot
    /// disarming remain engine-owned.
    pub fn set_drawing_tool(
        &mut self,
        kind: Option<DrawingKind>,
        options_json: Option<&str>,
        pane_index: Option<usize>,
    ) -> bool {
        let template = match kind {
            Some(kind) => match Self::drawing_template(kind, options_json) {
                Some(template) => Some(template),
                None => return false,
            },
            None => None,
        };
        self.invalidate_frame_drawings();
        self.invalidate_frame_overlay();
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
        self.drawing_controller.measure = None;
        self.drawing_controller.armed = match (kind, template) {
            (Some(kind), Some(template)) => Some(ArmedDrawingTool {
                kind,
                pane_index,
                template,
            }),
            _ => None,
        };
        true
    }

    pub fn active_drawing_tool(&self) -> Option<DrawingKind> {
        self.drawing_controller
            .armed
            .as_ref()
            .map(|armed| armed.kind)
    }

    pub fn active_drawing_tool_pane(&self) -> Option<usize> {
        self.drawing_controller
            .armed
            .as_ref()
            .and_then(|armed| armed.pane_index)
    }

    /// Whether the active tool currently owns a captured pointer stream. Hosts use this only to
    /// route subsequent normalized samples and platform capture lifecycle; the concrete placement
    /// mode remains private to the engine catalog.
    pub fn drawing_tool_capture_active(&self) -> bool {
        self.drawing_controller.brush.is_some()
    }

    /// Whether the armed tool is an explicitly finished variable sequence. Hosts use this for
    /// generic double-activation and Backspace routing without naming a concrete tool kind.
    pub fn drawing_tool_sequence_active(&self) -> bool {
        self.active_drawing_tool()
            .is_some_and(|kind| kind.spec().placement.is_sequence())
    }

    /// Whether a newly created drawing requests the platform text editor. The editor remains a
    /// host surface, while the decision to enter it is part of the canonical tool definition.
    pub fn drawing_requests_text_edit(&self, id: DrawingId) -> bool {
        self.drawing(id)
            .is_some_and(|drawing| drawing.kind.spec().requests_text_editor)
    }

    /// Merge options into the armed template and any in-flight creation. Browser and native
    /// toolbars both use this path, keeping preview and committed style identical.
    pub fn drawing_tool_apply_options(&mut self, json: &str) -> bool {
        let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) else {
            return false;
        };
        let Some(armed) = self.drawing_controller.armed.as_mut() else {
            return false;
        };
        armed.template.apply_patch(patch.clone());
        if let Some(pending) = self.drawing_controller.pending.as_mut() {
            pending.drawing.apply_patch(patch.clone());
        }
        if let Some(capture) = self.drawing_controller.brush.as_mut() {
            capture.options.apply_patch(patch);
        }
        self.invalidate_frame_drawings();
        true
    }

    fn begin_pending_from_armed(&mut self) -> bool {
        let Some(armed) = self.drawing_controller.armed.clone() else {
            return false;
        };
        if armed.kind.spec().placement.is_freehand() {
            return false;
        }
        let mut drawing = armed.template;
        drawing.points.clear();
        self.drawing_controller.pending = Some(PendingDrawing {
            drawing,
            preview: None,
            pane_constraint: armed.pane_index,
        });
        true
    }

    fn creation_update_for_commit(
        &mut self,
        kind: DrawingKind,
        id: DrawingId,
        pointer_capture: bool,
    ) -> DrawingCreationUpdate {
        if id == 0 {
            return DrawingCreationUpdate {
                consumed: true,
                changed: pointer_capture,
                pointer_capture,
                ..DrawingCreationUpdate::default()
            };
        }
        self.drawing_controller.armed = None;
        DrawingCreationUpdate {
            consumed: true,
            changed: true,
            created: Some(id),
            request_text_edit: kind.spec().requests_text_editor,
            pointer_capture,
        }
    }

    /// Forward pointer press while a drawing tool is armed. Placement classes decide what a
    /// press means; hosts do not branch on concrete tool kinds.
    pub fn drawing_tool_pointer_down(
        &mut self,
        x: f64,
        y: f64,
        modifiers: DrawingModifiers,
    ) -> DrawingCreationUpdate {
        let Some(armed) = self.drawing_controller.armed.clone() else {
            return DrawingCreationUpdate::default();
        };
        if armed
            .pane_index
            .is_some_and(|pane| self.pane_at_y(y) != Some(pane))
        {
            return DrawingCreationUpdate {
                consumed: true,
                ..DrawingCreationUpdate::default()
            };
        }
        match armed.kind.spec().placement {
            DrawingPlacement::Freehand { .. } => {
                let started =
                    self.brush_create_start_with_template(armed.template, armed.pane_index, x, y);
                DrawingCreationUpdate {
                    consumed: true,
                    changed: started,
                    pointer_capture: started,
                    ..DrawingCreationUpdate::default()
                }
            }
            placement if placement.places_on_press() => {
                if self.drawing_controller.pending.is_none() && !self.begin_pending_from_armed() {
                    return DrawingCreationUpdate {
                        consumed: true,
                        ..DrawingCreationUpdate::default()
                    };
                }
                let result = self.drawing_create_click(x, y, modifiers);
                let id = u32::try_from(result).unwrap_or(0);
                self.creation_update_for_commit(armed.kind, id, false)
            }
            _ => DrawingCreationUpdate {
                consumed: true,
                ..DrawingCreationUpdate::default()
            },
        }
    }

    /// Forward pointer movement. Only freehand placement samples an active drag; anchored tools
    /// update the same pending preview that frame construction consumes.
    pub fn drawing_tool_pointer_move(
        &mut self,
        x: f64,
        y: f64,
        modifiers: DrawingModifiers,
        pressed: bool,
    ) -> DrawingCreationUpdate {
        let Some(kind) = self.active_drawing_tool() else {
            return DrawingCreationUpdate::default();
        };
        if kind.spec().placement.is_freehand() {
            let changed = pressed && self.brush_create_add(x, y);
            let active = self.brush_create_active();
            return DrawingCreationUpdate {
                consumed: active,
                changed,
                pointer_capture: active,
                ..DrawingCreationUpdate::default()
            };
        }
        if self.drawing_create_active() {
            self.drawing_create_move(x, y, modifiers);
            return DrawingCreationUpdate {
                consumed: true,
                changed: true,
                ..DrawingCreationUpdate::default()
            };
        }
        DrawingCreationUpdate {
            consumed: true,
            ..DrawingCreationUpdate::default()
        }
    }

    /// Forward pointer release. Freehand commits here; other placement classes wait for the
    /// platform's click/tap activation so click-cancellation rules remain host-native.
    pub fn drawing_tool_pointer_up(
        &mut self,
        x: f64,
        y: f64,
        _modifiers: DrawingModifiers,
    ) -> DrawingCreationUpdate {
        let Some(kind) = self.active_drawing_tool() else {
            return DrawingCreationUpdate::default();
        };
        if !kind.spec().placement.is_freehand() || !self.brush_create_active() {
            return DrawingCreationUpdate {
                consumed: true,
                ..DrawingCreationUpdate::default()
            };
        }
        // The release point is part of canonical freehand semantics. Hosts may coalesce motion at
        // different cadences, but the final pointer position must never depend on whether a last
        // move event happened to arrive before pointer-up.
        let changed = self.brush_create_add(x, y);
        let id = self.brush_create_end();
        let mut update = self.creation_update_for_commit(kind, id, false);
        update.changed |= changed;
        update
    }

    /// Forward one click/tap activation. Fixed-anchor and sequence tools share this path;
    /// freehand and press-anchored tools have already consumed their pointer lifecycle.
    pub fn drawing_tool_activate(
        &mut self,
        x: f64,
        y: f64,
        modifiers: DrawingModifiers,
    ) -> DrawingCreationUpdate {
        let Some(armed) = self.drawing_controller.armed.as_ref() else {
            return DrawingCreationUpdate::default();
        };
        let kind = armed.kind;
        if armed
            .pane_index
            .is_some_and(|pane| self.pane_at_y(y) != Some(pane))
        {
            return DrawingCreationUpdate {
                consumed: true,
                ..DrawingCreationUpdate::default()
            };
        }
        match kind.spec().placement {
            DrawingPlacement::ClickAnchors { .. }
            | DrawingPlacement::SingleClickPreset { .. }
            | DrawingPlacement::MultiClick { .. } => {}
            _ => {
                return DrawingCreationUpdate {
                    consumed: true,
                    ..DrawingCreationUpdate::default()
                };
            }
        }
        if self.drawing_controller.pending.is_none() && !self.begin_pending_from_armed() {
            return DrawingCreationUpdate {
                consumed: true,
                ..DrawingCreationUpdate::default()
            };
        }
        let result = self.drawing_create_click(x, y, modifiers);
        let id = u32::try_from(result).unwrap_or(0);
        if id > 0 {
            return self.creation_update_for_commit(kind, id, false);
        }
        DrawingCreationUpdate {
            consumed: true,
            changed: true,
            ..DrawingCreationUpdate::default()
        }
    }

    /// Explicitly finish a variable-sequence tool (double-click/Enter). Other placement modes are
    /// no-ops, so hosts never branch on a concrete drawing kind.
    pub fn drawing_tool_finish(&mut self) -> DrawingCreationUpdate {
        let Some(kind) = self.active_drawing_tool() else {
            return DrawingCreationUpdate::default();
        };
        if !kind.spec().placement.is_sequence() {
            return DrawingCreationUpdate::default();
        }
        let id = self.drawing_create_finish();
        self.creation_update_for_commit(kind, id, false)
    }

    pub fn drawing_tool_pop_anchor(&mut self) -> bool {
        self.active_drawing_tool()
            .is_some_and(|kind| kind.spec().placement.is_sequence())
            && self.drawing_create_pop_anchor()
    }

    /// Cancel creation and disarm the tool as one atomic controller operation. Escape routes
    /// here, so it also dismisses a transient measure.
    pub fn cancel_drawing_tool(&mut self) {
        self.invalidate_frame_drawings();
        self.invalidate_frame_overlay();
        self.drawing_controller.armed = None;
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
        self.drawing_controller.measure = None;
    }

    // --- transient Shift-click measure ------------------------------------------------------

    /// The measure anchor under a pointer: clamped into the bound pane, magnet-snapped on
    /// request, then placed on the crosshair time slot and price tick like the measure tools.
    fn measure_point_at(
        &self,
        pane_index: usize,
        price_scale: DrawingPriceScale,
        x: f64,
        y: f64,
        magnet: bool,
    ) -> Option<DrawingPoint> {
        let pane = self.panes.get(pane_index)?;
        let x = x.clamp(0.0, self.pane_w);
        let y = y.clamp(pane.top, pane.top + pane.height);
        let mut point = self.drawing_from_px_for(pane_index, price_scale, x, y)?;
        if magnet {
            point = self.magnet_snap_point_at(pane_index, price_scale, x, y, point);
        }
        Some(self.grid_snap_point(
            DrawingKind::DatePriceRange,
            pane_index,
            price_scale,
            x,
            point,
            magnet,
        ))
    }

    /// Forward a primary press to the transient measure. A live measure always consumes it: a
    /// following measure freezes at the press, a frozen one is dismissed. Otherwise `begin`
    /// (the host's Shift state, after its own object hit tests) starts a measure in the pane
    /// under the pointer on that pane's default price scale. Returns whether the press was
    /// consumed; an armed drawing tool keeps ownership of its presses.
    pub fn measure_pointer_down(
        &mut self,
        x: f64,
        y: f64,
        begin: bool,
        modifiers: DrawingModifiers,
    ) -> bool {
        if let Some(session) = self.drawing_controller.measure.as_ref() {
            if session.following {
                let (pane, scale) = (session.drawing.pane_index, session.drawing.price_scale);
                let end = self.measure_point_at(pane, scale, x, y, modifiers.magnet);
                if let Some(session) = self.drawing_controller.measure.as_mut() {
                    if let Some(end) = end {
                        session.drawing.points[1] = end;
                    }
                    session.following = false;
                }
            } else {
                self.drawing_controller.measure = None;
            }
            self.invalidate_frame_drawings();
            return true;
        }
        if !begin || !x.is_finite() || self.drawing_controller.armed.is_some() {
            return false;
        }
        let Some(pane) = self.pane_at_y(y) else {
            return false;
        };
        if !self.pane_uses_financial_time(pane) || x < 0.0 || x > self.pane_w {
            return false;
        }
        let price_scale = match self.pane_default_scale_target(pane) {
            PriceScaleTarget::Left => DrawingPriceScale::Left,
            PriceScaleTarget::Overlay => DrawingPriceScale::Overlay,
            PriceScaleTarget::Right | PriceScaleTarget::Named(_) => DrawingPriceScale::Right,
        };
        let Some(start) = self.measure_point_at(pane, price_scale, x, y, modifiers.magnet) else {
            return false;
        };
        let mut drawing = Drawing::new(0, DrawingKind::DatePriceRange, pane, vec![start, start]);
        drawing.price_scale = price_scale;
        self.drawing_controller.measure = Some(MeasureSession {
            drawing,
            press: (x, y),
            following: true,
        });
        self.invalidate_frame_drawings();
        true
    }

    /// Follow the pointer with a live measure's end anchor (button held or not). Returns whether
    /// the measure changed.
    pub fn measure_pointer_move(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) -> bool {
        let Some(session) = self
            .drawing_controller
            .measure
            .as_ref()
            .filter(|session| session.following)
        else {
            return false;
        };
        let (pane, scale) = (session.drawing.pane_index, session.drawing.price_scale);
        let Some(end) = self.measure_point_at(pane, scale, x, y, modifiers.magnet) else {
            return false;
        };
        let Some(session) = self.drawing_controller.measure.as_mut() else {
            return false;
        };
        if session.drawing.points[1] == end {
            return false;
        }
        session.drawing.points[1] = end;
        self.invalidate_frame_drawings();
        true
    }

    /// Release after a measure press. A release beyond the click slop ends a press-drag
    /// measure; a click leaves it following until the next press. Returns whether it changed.
    pub fn measure_pointer_up(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) -> bool {
        let Some(session) = self
            .drawing_controller
            .measure
            .as_ref()
            .filter(|session| session.following)
        else {
            return false;
        };
        if (x - session.press.0).abs() + (y - session.press.1).abs() < MEASURE_DRAG_SLOP {
            return false;
        }
        self.measure_pointer_move(x, y, modifiers);
        if let Some(session) = self.drawing_controller.measure.as_mut() {
            session.following = false;
        }
        self.invalidate_frame_drawings();
        true
    }

    /// Dismiss the transient measure (Escape, focus/capture loss). Returns whether one existed.
    pub fn cancel_measure(&mut self) -> bool {
        if self.drawing_controller.measure.take().is_none() {
            return false;
        }
        self.invalidate_frame_drawings();
        true
    }

    pub fn measure_active(&self) -> bool {
        self.drawing_controller.measure.is_some()
    }

    /// Whether the measure's end anchor still follows the pointer.
    pub fn measure_following(&self) -> bool {
        self.drawing_controller
            .measure
            .as_ref()
            .is_some_and(|session| session.following)
    }

    /// The transient measure's start/end anchors, for hosts and tests.
    pub fn measure_points(&self) -> Option<[DrawingPoint; 2]> {
        let points = &self.drawing_controller.measure.as_ref()?.drawing.points;
        Some([points[0], points[1]])
    }

    pub(crate) fn measure_session(&self) -> Option<&MeasureSession> {
        self.drawing_controller.measure.as_ref()
    }

    /// Abort only the in-flight placement/capture while leaving the currently armed tool intact.
    /// Platform capture loss and gesture cancellation use this; explicit Escape/tool deselection
    /// uses [`ChartEngine::cancel_drawing_tool`] instead.
    pub fn cancel_drawing_creation(&mut self) {
        self.invalidate_frame_drawings();
        self.invalidate_frame_overlay();
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
    }

    // --- interactive creation (click-place anchors, move previews) ---

    /// Arm interactive creation of `kind` (the reference rectangle-drawing-tool's
    /// `startDrawing`): subsequent clicks place anchors through
    /// [`ChartEngine::drawing_create_click`], mouse moves update the preview through
    /// [`ChartEngine::drawing_create_move`]. `options_json` templates the new drawing
    /// ([`DrawingPatch`]). Replaces any in-progress creation. Returns false for a malformed
    /// options template.
    pub fn drawing_create_begin(&mut self, kind: DrawingKind, options_json: Option<&str>) -> bool {
        self.invalidate_frame_drawings();
        let patch = match options_json {
            Some(json) => match serde_json::from_str::<DrawingPatch>(json) {
                Ok(patch) => Some(patch),
                Err(_) => return false,
            },
            None => None,
        };
        let mut drawing = Drawing::new(0, kind, 0, Vec::new());
        if let Some(patch) = patch {
            drawing.apply_patch(patch);
        }
        self.drawing_controller.pending = Some(PendingDrawing {
            drawing,
            preview: None,
            pane_constraint: None,
        });
        self.invalidate_frame_overlay();
        true
    }

    fn single_click_position_points(
        &self,
        kind: DrawingKind,
        pane: usize,
        price_scale: DrawingPriceScale,
        entry: DrawingPoint,
        snap_time_to_data: bool,
    ) -> Option<Vec<DrawingPoint>> {
        if !matches!(kind, DrawingKind::LongPosition | DrawingKind::ShortPosition) {
            return None;
        }
        let pane_geometry = self.panes.get(pane)?;
        let entry = DrawingPoint {
            logical: self
                .snapped_crosshair_index(self.time_scale.logical_to_coordinate(entry.logical))
                as f64,
            price: self.snap_position_price(pane, price_scale, entry.price),
        };
        let (entry_x, entry_y) = self.drawing_to_px_for(pane, price_scale, entry)?;

        // A position is born at a useful editable size from one click. Horizontal extent prefers
        // the right (industry-standard) and flips left only when the click is too close to the
        // price scale. Seed a compact risk leg, then derive reward in PRICE space so the preset is
        // deliberately asymmetric and keeps an exact 2:1 reward/risk ratio even on log/inverted
        // scales.
        let desired_width = (self.pane_w * 0.32).clamp(180.0, 270.0);
        let edge_pad = 12.0;
        let extent_x = if entry_x + desired_width <= self.pane_w - edge_pad {
            entry_x + desired_width
        } else {
            (entry_x - desired_width).max(edge_pad)
        };
        let risk_pixels = (pane_geometry.height * 0.12).clamp(56.0, 90.0);
        let upper_y = entry_y - risk_pixels;
        let lower_y = entry_y + risk_pixels;

        let upper = self.drawing_from_px_for(pane, price_scale, entry_x, upper_y)?;
        let lower = self.drawing_from_px_for(pane, price_scale, entry_x, lower_y)?;
        let mut extent = self.drawing_from_px_for(pane, price_scale, extent_x, entry_y)?;
        extent.logical = self.snapped_crosshair_index(extent_x) as f64;
        if snap_time_to_data {
            extent = self.snap_drawing_time_to_data(extent)?;
        }
        let low = upper.price.min(lower.price);
        let high = upper.price.max(lower.price);
        let (stop_price, reward_sign) = match kind {
            DrawingKind::LongPosition => (low, 1.0),
            DrawingKind::ShortPosition => (high, -1.0),
            _ => return None,
        };
        let mut risk_distance =
            (entry.price - self.snap_position_price(pane, price_scale, stop_price)).abs();
        if risk_distance <= f64::EPSILON {
            risk_distance = self.position_price_tick(pane, price_scale).unwrap_or(0.0);
        }
        if !risk_distance.is_finite() || risk_distance <= f64::EPSILON {
            return None;
        }
        let stop_price =
            self.snap_position_price(pane, price_scale, entry.price - reward_sign * risk_distance);
        let target_price = self.snap_position_price(
            pane,
            price_scale,
            entry.price + reward_sign * risk_distance * 2.0,
        );
        Some(vec![
            entry,
            DrawingPoint {
                logical: extent.logical,
                price: target_price,
            },
            DrawingPoint {
                logical: entry.logical,
                price: stop_price,
            },
        ])
    }

    /// Place the next activation at pane-relative media px `(x, y)`. Ordinary anchored tools add
    /// one defining anchor per click. Single-click preset tools commit their complete semantic
    /// geometry immediately around this point. The first activation also binds the pending drawing
    /// to the pane under the cursor; later activations in a different pane are ignored.
    /// Returns 0 while no creation is armed, -1 while more anchors are needed, or the
    /// committed drawing's id (> 0) once the kind's anchor count is reached — the new drawing is
    /// left selected, industry-standard.
    pub fn drawing_create_click(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) -> i64 {
        self.invalidate_frame_drawings();
        let Some(pending) = &self.drawing_controller.pending else {
            return 0;
        };
        let price_scale = pending.drawing.price_scale;
        let bound_pane = pending
            .pane_constraint
            .or_else(|| (!pending.drawing.points.is_empty()).then_some(pending.drawing.pane_index));
        let pane = match bound_pane {
            Some(pane) => pane,
            None => match self.pane_at_y(y) {
                Some(pane) => pane,
                None => return -1,
            },
        };
        if bound_pane.is_some() && self.pane_at_y(y) != Some(pane) {
            return -1;
        }
        let Some(mut point) = self.drawing_from_px_for(pane, price_scale, x, y) else {
            return -1;
        };
        let (anchor_count, kind, fixed, snap_time_to_data) = {
            let Some(pending) = &self.drawing_controller.pending else {
                return 0;
            };
            (
                pending.drawing.kind.anchor_count(),
                pending.drawing.kind,
                pending.drawing.points.last().copied(),
                pending.drawing.snap_time_to_data,
            )
        };
        if snap_time_to_data {
            let Some(snapped) = self.snap_drawing_time_to_data(point) else {
                return -1;
            };
            point = snapped;
        }
        if modifiers.magnet {
            point = self.magnet_snap_point_at(pane, price_scale, x, y, point);
        }
        if modifiers.straighten
            && let Some(fixed) = fixed
            && let Some(snapped) = self.straighten_point(pane, price_scale, kind, fixed, point)
        {
            point = snapped;
        }
        point = self.grid_snap_point(kind, pane, price_scale, x, point, modifiers.magnet);
        let preset_points = if matches!(
            kind.spec().placement,
            DrawingPlacement::SingleClickPreset { .. }
        ) {
            self.single_click_position_points(kind, pane, price_scale, point, snap_time_to_data)
        } else {
            None
        };
        let Some(pending) = self.drawing_controller.pending.as_mut() else {
            return -1;
        };
        pending.drawing.pane_index = pane;
        if let Some(points) = preset_points {
            pending.drawing.points = points;
            pending.preview = None;
            let Some(pending) = self.drawing_controller.pending.take() else {
                return -1;
            };
            return i64::from(self.commit_pending_drawing(pending));
        }
        // Native and browser double-click sequences both deliver the endpoint click twice. A
        // zero-length final segment has no semantic value, so retain one vertex before finish.
        if kind.spec().placement.is_sequence() && pending.drawing.points.last() == Some(&point) {
            pending.preview = None;
            return -1;
        }
        if kind.spec().placement.is_sequence() && pending.drawing.points.len() == MAX_DRAWING_POINTS
        {
            return -1;
        }
        pending.drawing.points.push(point);
        pending.preview = None;
        if kind.spec().placement.is_sequence() {
            return -1;
        }
        if pending.drawing.points.len() < anchor_count {
            pending.preview = Some(point);
            return -1;
        }
        let Some(pending) = self.drawing_controller.pending.take() else {
            return -1;
        };
        i64::from(self.commit_pending_drawing(pending))
    }

    fn commit_pending_drawing(&mut self, pending: PendingDrawing) -> DrawingId {
        self.invalidate_frame_overlay();
        let Some(id) = self.take_drawing_id() else {
            return 0;
        };
        let mut drawing = pending.drawing;
        drawing.id = id;
        if drawing.kind == DrawingKind::AnchoredText
            && let (Some(pane), Some(&point)) =
                (self.panes.get(drawing.pane_index), drawing.points.first())
            && let Some((x, y)) =
                self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)
        {
            drawing.screen_x = (x / self.pane_w.max(1.0)).clamp(0.0, 1.0);
            drawing.screen_y = ((y - pane.top) / pane.height.max(1.0)).clamp(0.0, 1.0);
        }
        if drawing.kind == DrawingKind::BarsPattern {
            let Some(bars) = self.capture_bars_pattern(&drawing) else {
                return 0;
            };
            drawing.bars_pattern = bars;
        }
        self.drawings.push(drawing);
        self.insert_drawing_runtime(id);
        self.selected_drawing = Some(id);
        let index = self.drawings.len() - 1;
        self.record_drawing_command(DrawingCommand::Create {
            drawing: self.drawings[index].clone(),
            index,
        });
        id
    }

    /// Commit an active multi-click path. Enter and double-click route here after at least two
    /// vertices have been placed. Other tools and degenerate paths are left unchanged.
    pub fn drawing_create_finish(&mut self) -> DrawingId {
        let ready = self
            .drawing_controller
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.drawing.kind.spec().placement.is_sequence()
                    && pending
                        .drawing
                        .kind
                        .valid_point_count(pending.drawing.points.len())
            });
        if !ready {
            return 0;
        }
        self.invalidate_frame_drawings();
        let Some(pending) = self.drawing_controller.pending.take() else {
            return 0;
        };
        self.commit_pending_drawing(pending)
    }

    /// Remove the latest committed vertex from an active multi-click path. The live preview is
    /// retained so the next segment continues following the pointer.
    pub fn drawing_create_pop_anchor(&mut self) -> bool {
        let Some(pending) = self.drawing_controller.pending.as_mut() else {
            return false;
        };
        if !pending.drawing.kind.spec().placement.is_sequence()
            || pending.drawing.points.pop().is_none()
        {
            return false;
        }
        self.invalidate_frame_drawings();
        true
    }

    /// Update the creation preview point from a mouse move (no-op while unarmed or off the data).
    /// `modifiers` snaps the preview exactly as a click would snap the placed anchor.
    pub fn drawing_create_move(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) {
        self.invalidate_frame_drawings();
        let Some(pending) = &self.drawing_controller.pending else {
            return;
        };
        let unplaced = pending.drawing.points.is_empty();
        let bound_pane = pending.drawing.pane_index;
        let pane_constraint = pending.pane_constraint;
        let price_scale = pending.drawing.price_scale;
        // With nothing placed yet the preview follows the cursor in whichever pane it is over;
        // afterwards it stays bound to the first click's pane.
        let pane = if unplaced {
            match pane_constraint.or_else(|| self.pane_at_y(y)) {
                Some(pane) if self.pane_at_y(y) == Some(pane) => pane,
                _ => return,
            }
        } else {
            bound_pane
        };
        if self.pane_at_y(y) != Some(pane) {
            return;
        }
        let Some(mut point) = self.drawing_from_px_for(pane, price_scale, x, y) else {
            return;
        };
        if pending.drawing.snap_time_to_data {
            let Some(snapped) = self.snap_drawing_time_to_data(point) else {
                return;
            };
            point = snapped;
        }
        if modifiers.magnet {
            point = self.magnet_snap_point_at(pane, price_scale, x, y, point);
        }
        if modifiers.straighten
            && let Some(pending) = &self.drawing_controller.pending
            && let Some(&fixed) = pending.drawing.points.last()
            && let Some(snapped) =
                self.straighten_point(pane, price_scale, pending.drawing.kind, fixed, point)
        {
            point = snapped;
        }
        if let Some(kind) = self
            .drawing_controller
            .pending
            .as_ref()
            .map(|pending| pending.drawing.kind)
        {
            point = self.grid_snap_point(kind, pane, price_scale, x, point, modifiers.magnet);
        }
        if let Some(pending) = self.drawing_controller.pending.as_mut() {
            pending.drawing.pane_index = pane;
            pending.preview = Some(point);
        }
    }

    /// Merge a JSON options patch into every active drawing-creation mode.
    ///
    /// Absent keys retain their current values, and placed anchors, preview state, and captured
    /// brush points are left untouched. Returns `false` when `json` is malformed or neither a
    /// pending anchored drawing nor a brush capture is active.
    pub fn drawing_create_apply_options(&mut self, json: &str) -> bool {
        self.invalidate_frame_drawings();
        let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) else {
            return false;
        };
        match (
            &mut self.drawing_controller.pending,
            &mut self.drawing_controller.brush,
        ) {
            (Some(pending), Some(capture)) => {
                let mut pending_drawing = pending.drawing.clone();
                let mut capture_options = capture.options.clone();
                if !pending_drawing.apply_patch(patch.clone())
                    || !capture_options.apply_patch(patch)
                {
                    return false;
                }
                pending.drawing = pending_drawing;
                capture.options = capture_options;
            }
            (Some(pending), None) => {
                let mut drawing = pending.drawing.clone();
                if !drawing.apply_patch(patch) {
                    return false;
                }
                pending.drawing = drawing;
            }
            (None, Some(capture)) => {
                let mut options = capture.options.clone();
                if !options.apply_patch(patch) {
                    return false;
                }
                capture.options = options;
            }
            (None, None) => return false,
        }
        true
    }

    /// Abandon the in-progress creation (Escape / tool disarm).
    pub fn drawing_create_cancel(&mut self) {
        self.invalidate_frame_drawings();
        self.invalidate_frame_overlay();
        self.drawing_controller.pending = None;
    }

    pub fn drawing_create_active(&self) -> bool {
        self.drawing_controller.pending.is_some()
    }

    /// The in-progress creation for the frame build (its committed anchors plus the preview
    /// point render as a tentative drawing).
    pub(crate) fn pending_drawing(&self) -> Option<&PendingDrawing> {
        self.drawing_controller.pending.as_ref()
    }

    // --- freehand brush capture (press-drag-release, the public reference's brush) ---

    fn brush_create_start_with_template(
        &mut self,
        mut options: Drawing,
        pane_constraint: Option<usize>,
        x: f64,
        y: f64,
    ) -> bool {
        self.invalidate_frame_drawings();
        let Some(pane) = self.pane_at_y(y) else {
            return false;
        };
        if pane_constraint.is_some_and(|expected| expected != pane) {
            return false;
        }
        let Some(point) = self.drawing_from_px_for(pane, options.price_scale, x, y) else {
            return false;
        };
        options.pane_index = pane;
        options.points.clear();
        self.drawing_controller.brush = Some(BrushCapture {
            pane_index: pane,
            points: vec![point],
            last_px: (x, y),
            options,
        });
        self.invalidate_frame_overlay();
        true
    }

    /// Begin a brush stroke at pane-relative media px `(x, y)` (pointer-down with the brush
    /// tool armed): binds the stroke to the pane under the cursor and captures the first
    /// point. `options_json` templates the committed drawing ([`DrawingPatch`]). Returns false
    /// off the panes/data or for a malformed template.
    pub fn brush_create_start(&mut self, options_json: Option<&str>, x: f64, y: f64) -> bool {
        let Some(options) = Self::drawing_template(DrawingKind::Brush, options_json) else {
            return false;
        };
        self.brush_create_start_with_template(options, None, x, y)
    }

    /// Capture the next stroke point from a pointer move, decimated by distance
    /// ([`BRUSH_MIN_POINT_DISTANCE`] in media px — closer samples are pointer noise). Returns
    /// whether a point was captured; rejected samples leave the frame untouched, so hosts can
    /// skip the repaint instead of rebuilding the drawings layer per raw pointer event.
    pub fn brush_create_add(&mut self, x: f64, y: f64) -> bool {
        let Some(capture) = &self.drawing_controller.brush else {
            return false;
        };
        if capture.points.len() == MAX_DRAWING_POINTS {
            return false;
        }
        if (x - capture.last_px.0).hypot(y - capture.last_px.1) < BRUSH_MIN_POINT_DISTANCE {
            return false;
        }
        let pane = capture.pane_index;
        let Some(point) = self.drawing_from_px(pane, x, y) else {
            return false;
        };
        if let Some(capture) = self.drawing_controller.brush.as_mut() {
            capture.points.push(point);
            capture.last_px = (x, y);
        }
        self.invalidate_frame_drawings();
        true
    }

    /// Commit the stroke (pointer-up): the decimated capture is committed as-is — the stored
    /// path is exactly the curve the live stroke painted (input decimation already bounded it),
    /// rendered as a curved polyline. Returns the id, or 0 discarding a degenerate stroke
    /// (fewer than two points / no capture active).
    pub fn brush_create_end(&mut self) -> DrawingId {
        self.invalidate_frame_drawings();
        self.invalidate_frame_overlay();
        let Some(capture) = self.drawing_controller.brush.take() else {
            return 0;
        };
        if capture.points.len() < 2 {
            return 0;
        }
        let Some(id) = self.take_drawing_id() else {
            return 0;
        };
        let mut drawing = capture.options;
        drawing.id = id;
        drawing.pane_index = capture.pane_index;
        drawing.points = capture.points;
        self.drawings.push(drawing);
        self.insert_drawing_runtime(id);
        self.selected_drawing = Some(id);
        let index = self.drawings.len() - 1;
        self.record_drawing_command(DrawingCommand::Create {
            drawing: self.drawings[index].clone(),
            index,
        });
        id
    }

    /// Abandon the in-progress stroke (Escape / tool disarm).
    pub fn brush_create_cancel(&mut self) {
        self.invalidate_frame_drawings();
        self.invalidate_frame_overlay();
        self.drawing_controller.brush = None;
    }

    pub fn brush_create_active(&self) -> bool {
        self.drawing_controller.brush.is_some()
    }

    /// The in-progress stroke for the frame build (paints as a live curved polyline).
    pub(crate) fn brush_capture(&self) -> Option<&BrushCapture> {
        self.drawing_controller.brush.as_ref()
    }
}

#[cfg(test)]
mod tests;
