//! Drawing tools (trend line, horizontal line/ray, vertical line, rectangle, text) as
//! engine-owned drawing objects — TradingView's drawing tools in the spirit of the reference's
//! plugin-examples (trend-line.ts, rectangle-drawing-tool.ts, vertical-line.ts, anchored-text.ts),
//! but with all state, hit-testing, and anchor-dragging math living headless here: hosts only
//! forward gestures and render the frame, exactly like the interaction-model split
//! (ARCHITECTURE.md §6.2).
//!
//! Anchor model: a drawing is defined by 1 or 2 [`DrawingPoint`]s in `{logical, price}` space
//! (fractional logical bar index + price — the time scale's interpolation space, so an anchor
//! may sit between bars, like the reference examples' `timeToCoordinate`/`priceToCoordinate`
//! inputs). Coordinates resolve against the pane's RIGHT price scale each frame, mirroring the
//! pane-primitive converters' default target (chart/primitives.rs).
//!
//! Hit-testing and dragging reuse the same media-px coordinate space as the series hit tests
//! (hit_test.rs): x from the pane's left edge, y from the chart's top. A body drag translates
//! every anchor in COORDINATE space and converts back per anchor, so the shape stays pixel-rigid
//! under the cursor on any scale mode (normal/log/percentage).

use aion_render::color::Color;
use aion_render::draw_list::{LineStyle, LineType};

use super::*;

/// Chart-unique drawing id (never reused within a chart; 0 is the "no drawing" sentinel).
pub type DrawingId = u32;

/// The drawing-tool kinds. Wire values cross the wasm boundary as `u8`; names cross as the
/// snake_case strings [`DrawingKind::name`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Freehand path (TradingView's brush): a variable-length point list drawn as a smooth
    /// interpolating curve, with anchor handles at the two ENDS when selected.
    Brush,
}

impl DrawingKind {
    pub fn from_u8(kind: u8) -> Option<Self> {
        Some(match kind {
            0 => Self::TrendLine,
            1 => Self::HorizontalLine,
            2 => Self::HorizontalRay,
            3 => Self::VerticalLine,
            4 => Self::Rectangle,
            5 => Self::Text,
            6 => Self::Brush,
            _ => return None,
        })
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::TrendLine => 0,
            Self::HorizontalLine => 1,
            Self::HorizontalRay => 2,
            Self::VerticalLine => 3,
            Self::Rectangle => 4,
            Self::Text => 5,
            Self::Brush => 6,
        }
    }

    /// The public snake_case wire name (TS `drawing_kind`).
    pub fn name(self) -> &'static str {
        match self {
            Self::TrendLine => "trend_line",
            Self::HorizontalLine => "horizontal_line",
            Self::HorizontalRay => "horizontal_ray",
            Self::VerticalLine => "vertical_line",
            Self::Rectangle => "rectangle",
            Self::Text => "text",
            Self::Brush => "brush",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "trend_line" => Self::TrendLine,
            "horizontal_line" => Self::HorizontalLine,
            "horizontal_ray" => Self::HorizontalRay,
            "vertical_line" => Self::VerticalLine,
            "rectangle" => Self::Rectangle,
            "text" => Self::Text,
            "brush" => Self::Brush,
            _ => return None,
        })
    }

    /// The number of defining anchors the kind is placed with (and its handles show). The brush
    /// is variable-length: this is its MINIMUM (two ends).
    pub fn anchor_count(self) -> usize {
        match self {
            Self::TrendLine | Self::Rectangle | Self::Brush => 2,
            Self::HorizontalLine | Self::HorizontalRay | Self::VerticalLine | Self::Text => 1,
        }
    }

    /// Whether `count` is a valid point count for a stored drawing of this kind (the brush
    /// takes any count at or above its two-end minimum).
    pub fn valid_point_count(self, count: usize) -> bool {
        match self {
            Self::Brush => count >= self.anchor_count(),
            _ => count == self.anchor_count(),
        }
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

/// Horizontal text alignment shared by every tool's label (TS `drawing_text_h_align`; maps to
/// the IR's `TextAlign`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingTextHAlign {
    Left,
    Center,
    Right,
}

impl DrawingTextHAlign {
    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
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

impl DrawingTextVAlign {
    fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Middle => "middle",
            Self::Bottom => "bottom",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "top" => Self::Top,
            "middle" => Self::Middle,
            "bottom" => Self::Bottom,
            _ => return None,
        })
    }
}

/// TradingView's drawing default color (its trend-line/ray blue).
pub const DRAWING_DEFAULT_COLOR: &str = "#2962ff";

/// An engine-owned drawing. Colors follow the series pattern: stored verbatim as CSS strings
/// and parsed at render time (`None`/unparseable falls back to the follow behavior documented
/// per field).
#[derive(Clone, Debug)]
pub struct Drawing {
    pub id: DrawingId,
    pub kind: DrawingKind,
    pub pane_index: usize,
    pub points: Vec<DrawingPoint>,
    /// Line/border color CSS string (default [`DRAWING_DEFAULT_COLOR`]).
    pub color: String,
    /// Stroke width in CSS px (default 2; 1 for a rectangle's border).
    pub width: f64,
    pub style: LineStyle,
    /// Rectangle fill CSS string; `None` fills with the border color at 20% alpha. Unused by
    /// the line kinds and the text tool.
    pub fill_color: Option<String>,
    /// The tool's text label (`""` = none).
    pub text: String,
    /// Label color CSS string; `None` follows the chart's `layout.textColor`.
    pub text_color: Option<String>,
    /// Label glyph size in CSS px; `None` follows the chart's `layout.fontSize`.
    pub text_size: Option<f64>,
    pub text_bold: bool,
    pub text_h_align: DrawingTextHAlign,
    pub text_v_align: DrawingTextVAlign,
}

impl Drawing {
    fn new(id: DrawingId, kind: DrawingKind, pane_index: usize, points: Vec<DrawingPoint>) -> Self {
        Self {
            id,
            kind,
            pane_index,
            points,
            color: DRAWING_DEFAULT_COLOR.to_string(),
            width: if kind == DrawingKind::Rectangle {
                1.0
            } else {
                2.0
            },
            style: LineStyle::Solid,
            fill_color: None,
            text: String::new(),
            text_color: None,
            text_size: None,
            text_bold: false,
            text_h_align: DrawingTextHAlign::Center,
            text_v_align: DrawingTextVAlign::Middle,
        }
    }
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

/// Modifier keys the host gesture layer forwards with pointer positions (TradingView modifier
/// semantics): `magnet` snaps anchors to the nearest bar — x to the bar's center, the price to
/// the closest of its open/high/low/close (Ctrl/Cmd — the same key that magnets the crosshair);
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
/// `{bold} {size}px {family}` and returning the width in the same px units as `size`). Without
/// one the engine estimates `chars × size × 0.6` (deterministic for native tests).
pub type TextMeasureFn = Box<dyn Fn(&str, f64, &str, bool) -> f64>;

/// Active anchor/body drag session (the interaction.rs session pattern: the engine owns the
/// start snapshot and the math; hosts forward drag positions).
pub(crate) struct DrawingDrag {
    pub(crate) id: DrawingId,
    pub(crate) part: DrawingDragPart,
    /// Press point in pane-relative media px (x from the pane's left, y from the chart's top).
    pub(crate) start_x: f64,
    pub(crate) start_y: f64,
    /// Anchor definitions at drag start.
    pub(crate) start_points: Vec<DrawingPoint>,
    /// Anchors converted to media px at drag start (the body-drag translation base).
    pub(crate) start_px: Vec<(f64, f64)>,
}

/// Interactive creation in progress (the reference rectangle-drawing-tool's `_drawing` state,
/// engine-owned): committed anchors plus a preview point that follows the mouse until the kind's
/// anchor count is reached.
pub(crate) struct PendingDrawing {
    pub(crate) drawing: Drawing,
    pub(crate) preview: Option<DrawingPoint>,
}

/// Freehand brush capture in progress (TradingView's brush drag, engine-owned): the points
/// collected so far plus the options template for the committed drawing. Input is decimated by
/// distance on the way in (media px) and RDP-simplified at commit — the stored path is already
/// the smooth one.
pub(crate) struct BrushCapture {
    pub(crate) pane_index: usize,
    pub(crate) points: Vec<DrawingPoint>,
    /// The last captured point in media px (the decimation reference).
    pub(crate) last_px: (f64, f64),
    pub(crate) options: Drawing,
}

/// Minimum spacing between captured brush points in media px (input decimation — anything
/// closer is pointer noise, not intent).
pub(crate) const BRUSH_MIN_POINT_DISTANCE: f64 = 1.5;
/// Ramer–Douglas–Peucker tolerance in media px applied at brush commit: drops collinear/noise
/// points so the stored path is the smooth centerline of the stroke.
pub(crate) const BRUSH_SIMPLIFY_TOLERANCE: f64 = 1.0;

/// Ramer–Douglas–Peucker polyline simplification in media-px space (iterative — no recursion
/// depth limit on long strokes). Both endpoints are always kept.
pub(crate) fn rdp_simplify(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        if last <= first + 1 {
            continue;
        }
        let (a, b) = (points[first], points[last]);
        let mut max_distance = 0.0_f64;
        let mut max_index = first;
        for (index, &point) in points.iter().enumerate().take(last).skip(first + 1) {
            let distance = distance_to_segment(point.0, point.1, a.0, a.1, b.0, b.1);
            if distance > max_distance {
                max_distance = distance;
                max_index = index;
            }
        }
        if max_distance > epsilon {
            keep[max_index] = true;
            stack.push((first, max_index));
            stack.push((max_index, last));
        }
    }
    points
        .iter()
        .zip(keep.iter())
        .filter_map(|(&point, &keep)| keep.then_some(point))
        .collect()
}

/// reference default `hitTestTolerance` shared with the series hit tests (hit_test.rs).
const HIT_TOLERANCE: f64 = 3.0;
/// Anchor-handle hit radius in media px (the drawn handle spans radius 4 + 1.5 border).
const ANCHOR_HIT_RADIUS: f64 = 6.5;
/// Padding between a tool's reference box and its text label, in CSS px.
pub(crate) const TEXT_PAD: f64 = 4.0;

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

/// JSON patch accepted by [`ChartEngine::drawing_apply_options`] and carried by
/// `add_drawing`/creation. Snake_case keys are canonical (matching the TS `drawing_options`);
/// the reference camelCase forms are accepted as aliases. Every field is optional — absent keys
/// keep their current values (reference merge semantics).
#[derive(serde::Deserialize, Default)]
pub(crate) struct DrawingPatch {
    color: Option<String>,
    width: Option<f64>,
    #[serde(alias = "lineStyle")]
    style: Option<serde_json::Value>,
    #[serde(alias = "fillColor")]
    fill_color: Option<String>,
    text: Option<String>,
    #[serde(alias = "textColor")]
    text_color: Option<String>,
    #[serde(alias = "textSize")]
    text_size: Option<f64>,
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
    fn apply_patch(&mut self, patch: DrawingPatch) {
        if let Some(css) = patch.color {
            if Color::parse_css(&css).is_some() {
                self.color = css;
            }
        }
        if let Some(width) = patch.width {
            if width.is_finite() && width > 0.0 {
                self.width = width;
            }
        }
        if let Some(style) = patch.style.as_ref().and_then(parse_drawing_style) {
            self.style = style;
        }
        if let Some(css) = patch.fill_color {
            update_css_slot(&mut self.fill_color, css);
        }
        if let Some(text) = patch.text {
            self.text = text;
        }
        if let Some(css) = patch.text_color {
            update_css_slot(&mut self.text_color, css);
        }
        if let Some(size) = patch.text_size {
            if size.is_finite() && size > 0.0 {
                self.text_size = Some(size);
            }
        }
        if let Some(bold) = patch.text_bold {
            self.text_bold = bold;
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
    }

    fn options_json(&self) -> serde_json::Value {
        serde_json::json!({
            "color": self.color,
            "width": self.width,
            "style": style_name(self.style),
            "fill_color": self.fill_color.as_deref().unwrap_or(""),
            "text": self.text,
            "text_color": self.text_color.as_deref().unwrap_or(""),
            "text_size": self.text_size,
            "text_bold": self.text_bold,
            "text_h_align": self.text_h_align.name(),
            "text_v_align": self.text_v_align.name(),
        })
    }
}

/// The label's reference box in the caller's coordinate units (media px at hit-test time,
/// bitmap px at render): the tool's bounding geometry the 3×3 alignment resolves against.
/// Horizontal lines span the pane (or the ray's extent); a vertical line spans the pane's
/// height; the text tool's box degenerates to its anchor point.
pub(crate) struct TextBox {
    pub(crate) left: f64,
    pub(crate) right: f64,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
}

impl ChartEngine {
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
        let scale = &self.panes.get(pane_index)?.price_scale;
        (!scale.is_empty()).then_some(scale)
    }

    /// The pane's primary series' base value for percentage/indexed scale modes — the first
    /// visible, non-overlay series bound to the right scale (mirrors the pane-primitive
    /// converters' `pane_scale_base_value`, chart/primitives.rs). Unused by normal/log scales.
    pub(crate) fn drawing_scale_base(&self, pane_index: usize) -> f64 {
        self.visible_range()
            .and_then(|(from, _)| {
                let series = self.series.iter().find(|s| {
                    s.visible
                        && !s.removed
                        && !s.overlay
                        && s.pane_index == pane_index
                        && !s.left_scale
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
        let scale = self.drawing_scale(pane_index)?;
        if self.data.merged_times().is_empty() {
            return None;
        }
        let base = self.drawing_scale_base(pane_index);
        Some((
            self.time_scale.logical_to_coordinate(point.logical),
            scale.price_to_coordinate(point.price, base),
        ))
    }

    /// The anchor under a media-px position on `pane_index` (the drag/creation conversion).
    fn drawing_from_px(&self, pane_index: usize, x: f64, y: f64) -> Option<DrawingPoint> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let scale = self.drawing_scale(pane_index)?;
        if self.data.merged_times().is_empty() {
            return None;
        }
        let base = self.drawing_scale_base(pane_index);
        Some(DrawingPoint {
            // The time scale's float-index space is offset a half bar from its logical space
            // (bar `i` owns float indexes `(i-1, i]` — coordinate_to_index ceils), so the
            // inverse of `logical_to_coordinate` is `coordinate_to_float_index + 0.5`.
            logical: self.time_scale.coordinate_to_float_index(x) + 0.5,
            price: scale.coordinate_to_price(y, base),
        })
    }

    /// TradingView magnet (Ctrl held): snap an anchor to the nearest bar — the logical index
    /// to the bar's center and the price to the closest of the bar's open/high/low/close,
    /// resolved on the pane's primary series (the first visible, non-overlay series — the same
    /// primary rule the crosshair/axis use; the snap returns a price, so the series' scale
    /// binding is irrelevant and left-scale charts snap too). A snapped index with no real bar
    /// there (whitespace, or off the data) keeps the raw point.
    fn magnet_snap_point(&self, pane_index: usize, point: DrawingPoint) -> DrawingPoint {
        let Some(primary) = self
            .series
            .iter()
            .find(|s| s.visible && !s.removed && !s.overlay && s.pane_index == pane_index)
        else {
            return point;
        };
        let logical = point.logical.round() as i64;
        let plot = self.data.plot(primary.id);
        let Some(row) = plot.search(logical, MismatchDirection::None) else {
            return point;
        };
        if plot.is_whitespace_row(row) {
            return point;
        }
        let mut best_price = point.price;
        let mut best_distance = f64::INFINITY;
        for column in [
            PlotValueIndex::Open,
            PlotValueIndex::High,
            PlotValueIndex::Low,
            PlotValueIndex::Close,
        ] {
            let value = plot.value_at(row, column);
            if !value.is_finite() {
                continue;
            }
            let distance = (value - point.price).abs();
            if distance < best_distance {
                best_distance = distance;
                best_price = value;
            }
        }
        if !best_distance.is_finite() {
            return point;
        }
        DrawingPoint {
            logical: logical as f64,
            price: best_price,
        }
    }

    /// TradingView straighten (Shift held): recompute the dragged anchor of a two-anchor tool
    /// so the segment from the FIXED anchor snaps to the nearest 0°/45°/90° direction at the
    /// dragged pixel distance; a rectangle instead becomes a square (the larger dragged side
    /// wins, the drag quadrant's signs kept). Angles are visual, so the math runs in media-px
    /// space (screen coordinates, y down — the 45° snap set is symmetric). One-anchor kinds
    /// pass through (already straight). `None` when either anchor fails to convert.
    fn straighten_point(
        &self,
        pane_index: usize,
        kind: DrawingKind,
        fixed: DrawingPoint,
        dragged: DrawingPoint,
    ) -> Option<DrawingPoint> {
        let (fx, fy) = self.drawing_to_px(pane_index, fixed)?;
        let (dx, dy) = self.drawing_to_px(pane_index, dragged)?;
        let (mut vx, mut vy) = (dx - fx, dy - fy);
        match kind {
            DrawingKind::TrendLine => {
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
            DrawingKind::Rectangle => {
                let side = vx.abs().max(vy.abs());
                vx = if vx < 0.0 { -side } else { side };
                vy = if vy < 0.0 { -side } else { side };
            }
            _ => return Some(dragged),
        }
        self.drawing_from_px(pane_index, fx + vx, fy + vy)
    }

    /// The anchors of `drawing` in media px (`None` when any anchor fails to resolve), with the
    /// pending preview appended when `preview` holds (the interactive-creation geometry).
    pub(crate) fn drawing_px(&self, drawing: &Drawing) -> Option<Vec<(f64, f64)>> {
        drawing
            .points
            .iter()
            .map(|&point| self.drawing_to_px(drawing.pane_index, point))
            .collect()
    }

    // --- text placement (shared by render and hit-testing) ---

    /// The label's reference box for a drawing in the caller's units. `px` holds the anchors
    /// already converted (bitmap px at render, media px at hit-test); `pane_w`/`pane_h` bound
    /// the full-width/full-height kinds in the same units. `pane_top` is the pane's vertical
    /// origin (0 for pane-local bitmap x media y are both chart-top-relative — see hit_test.rs).
    pub(crate) fn text_box(
        kind: DrawingKind,
        px: &[(f64, f64)],
        pane_w: f64,
        pane_top: f64,
        pane_h: f64,
    ) -> TextBox {
        match kind {
            DrawingKind::HorizontalLine => TextBox {
                left: 0.0,
                right: pane_w,
                top: px[0].1,
                bottom: px[0].1,
            },
            DrawingKind::HorizontalRay => TextBox {
                left: px[0].0,
                right: pane_w,
                top: px[0].1,
                bottom: px[0].1,
            },
            DrawingKind::VerticalLine => TextBox {
                left: px[0].0,
                right: px[0].0,
                top: pane_top,
                bottom: pane_top + pane_h,
            },
            DrawingKind::TrendLine | DrawingKind::Rectangle => {
                let (a, b) = (px[0], px[1]);
                TextBox {
                    left: a.0.min(b.0),
                    right: a.0.max(b.0),
                    top: a.1.min(b.1),
                    bottom: a.1.max(b.1),
                }
            }
            DrawingKind::Brush => {
                // The whole stroke's bounding box.
                let (mut left, mut right, mut top, mut bottom) = (
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                );
                for &(x, y) in px {
                    left = left.min(x);
                    right = right.max(x);
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
                TextBox {
                    left,
                    right,
                    top,
                    bottom,
                }
            }
            DrawingKind::Text => TextBox {
                left: px[0].0,
                right: px[0].0,
                top: px[0].1,
                bottom: px[0].1,
            },
        }
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
    ) -> (f64, f64, DrawingTextHAlign) {
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
        (x, y, h)
    }

    /// Measure (or estimate) a label's width in the same px units as `size`.
    pub(crate) fn measure_drawing_text(&self, drawing: &Drawing, size: f64) -> f64 {
        let layout = &self.options.get().layout;
        let family = layout.font_family.as_str();
        match &self.text_measure_fn {
            Some(measure) => measure(drawing.text.as_str(), size, family, drawing.text_bold),
            None => drawing.text.chars().count() as f64 * size * 0.6,
        }
    }

    /// Install (or clear with `None`) the host text-measure callback for drawing-label hit
    /// boxes (see [`TextMeasureFn`]).
    pub fn set_text_measure(&mut self, f: Option<TextMeasureFn>) {
        self.text_measure_fn = f;
    }

    // --- CRUD ---

    /// Add a drawing to a pane; returns its chart-unique id, or `None` for a stale pane index,
    /// a wrong anchor count for the kind, or non-finite anchors. `options_json` is a
    /// [`DrawingPatch`] — absent keys take the documented defaults.
    pub fn add_drawing(
        &mut self,
        kind: DrawingKind,
        pane_index: usize,
        points: Vec<DrawingPoint>,
        options_json: Option<&str>,
    ) -> Option<DrawingId> {
        if pane_index >= self.panes.len() || !kind.valid_point_count(points.len()) {
            return None;
        }
        if points
            .iter()
            .any(|p| !p.logical.is_finite() || !p.price.is_finite())
        {
            return None;
        }
        let id = self.next_drawing_id;
        self.next_drawing_id += 1;
        let mut drawing = Drawing::new(id, kind, pane_index, points);
        if let Some(json) = options_json {
            if let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) {
                drawing.apply_patch(patch);
            }
        }
        self.drawings.push(drawing);
        Some(id)
    }

    /// Merge a JSON options patch into the drawing with `id` (reference `applyOptions`): absent
    /// keys keep their current values. Returns false for a malformed patch or an unknown id.
    pub fn drawing_apply_options(&mut self, id: DrawingId, json: &str) -> bool {
        let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) else {
            return false;
        };
        let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) else {
            return false;
        };
        drawing.apply_patch(patch);
        true
    }

    /// Replace a drawing's anchors from a JSON `[{logical, price}, ...]` array. Returns false
    /// for malformed JSON, a wrong count for the kind, non-finite values, or an unknown id.
    pub fn drawing_set_points(&mut self, id: DrawingId, json: &str) -> bool {
        let Ok(points) = serde_json::from_str::<Vec<DrawingPoint>>(json) else {
            return false;
        };
        let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) else {
            return false;
        };
        if !drawing.kind.valid_point_count(points.len())
            || points
                .iter()
                .any(|p| !p.logical.is_finite() || !p.price.is_finite())
        {
            return false;
        }
        drawing.points = points;
        true
    }

    /// Remove a drawing by id. Returns false for an unknown id. Any selection or drag session
    /// pointing at it is released.
    pub fn remove_drawing(&mut self, id: DrawingId) -> bool {
        let before = self.drawings.len();
        self.drawings.retain(|d| d.id != id);
        let removed = self.drawings.len() != before;
        if removed {
            if self.selected_drawing == Some(id) {
                self.selected_drawing = None;
            }
            if self.drawing_drag.as_ref().is_some_and(|drag| drag.id == id) {
                self.drawing_drag = None;
            }
        }
        removed
    }

    /// Remove every drawing (the demo's "clear all") and release the selection/drag state.
    pub fn clear_drawings(&mut self) {
        self.drawings.clear();
        self.selected_drawing = None;
        self.drawing_drag = None;
    }

    /// The drawing's full options as a snake_case JSON object (reference `options`). `None` for
    /// an unknown id.
    pub fn drawing_options_json(&self, id: DrawingId) -> Option<String> {
        Some(self.drawing(id)?.options_json().to_string())
    }

    /// The drawing's anchors as a JSON `[{logical, price}, ...]` array. `None` for an unknown id.
    pub fn drawing_points_json(&self, id: DrawingId) -> Option<String> {
        Some(serde_json::to_string(&self.drawing(id)?.points).unwrap_or_default())
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

    /// The selected drawing (TradingView-style click-to-select): while set, the frame build
    /// paints anchor handles at its defining points and its anchors accept drags. An unknown id
    /// never sticks.
    pub fn set_selected_drawing(&mut self, id: Option<DrawingId>) {
        self.selected_drawing = id.filter(|&sid| self.drawings.iter().any(|d| d.id == sid));
    }

    pub fn selected_drawing(&self) -> Option<DrawingId> {
        self.selected_drawing
    }

    /// Select the drawing under pane-relative media px `(x, y)` (the host click pipeline):
    /// the topmost hit is selected; a miss clears the selection. Returns whether a drawing was
    /// hit (the host then skips its series-selection path).
    pub fn select_drawing_at(&mut self, x: f64, y: f64) -> bool {
        self.selected_drawing = self.hit_test_drawing(x, y).map(|hit| hit.id);
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
        if !x.is_finite() || !y.is_finite() || x < 0.0 || x > self.pane_w {
            return None;
        }
        let pane = self.pane_at_y(y)?;
        // The selected drawing's anchor handles win over every body (they paint above all).
        // The brush shows handles at its two ENDS only; the fixed kinds show one per anchor.
        if let Some(selected) = self.selected_drawing {
            if let Some(drawing) = self.drawing(selected) {
                if drawing.pane_index == pane {
                    if let Some(px) = self.drawing_px(drawing) {
                        let anchor_indexes: &[usize] = match drawing.kind {
                            DrawingKind::Brush if px.len() > 2 => &[0, px.len() - 1],
                            _ => &[],
                        };
                        if anchor_indexes.is_empty() {
                            for (index, &(ax, ay)) in px.iter().enumerate() {
                                if (x - ax).hypot(y - ay) <= ANCHOR_HIT_RADIUS {
                                    return Some(DrawingHit {
                                        id: selected,
                                        part: DrawingDragPart::Anchor(index),
                                        cursor: "pointer",
                                    });
                                }
                            }
                        } else {
                            for &index in anchor_indexes {
                                let (ax, ay) = px[index];
                                if (x - ax).hypot(y - ay) <= ANCHOR_HIT_RADIUS {
                                    return Some(DrawingHit {
                                        id: selected,
                                        part: DrawingDragPart::Anchor(index),
                                        cursor: "pointer",
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        for drawing in self.drawings.iter().rev() {
            if drawing.pane_index != pane {
                continue;
            }
            let Some(px) = self.drawing_px(drawing) else {
                continue;
            };
            if self.drawing_body_hit(drawing, &px, x, y) {
                return Some(DrawingHit {
                    id: drawing.id,
                    part: DrawingDragPart::Body,
                    cursor: "move",
                });
            }
        }
        None
    }

    /// The per-kind body test at media px `(x, y)` against the converted anchors `px`.
    fn drawing_body_hit(&self, drawing: &Drawing, px: &[(f64, f64)], x: f64, y: f64) -> bool {
        let tolerance = drawing.width / 2.0 + HIT_TOLERANCE;
        match drawing.kind {
            DrawingKind::TrendLine => {
                let (a, b) = (px[0], px[1]);
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }
            DrawingKind::HorizontalLine => (y - px[0].1).abs() <= tolerance,
            DrawingKind::HorizontalRay => {
                (y - px[0].1).abs() <= tolerance && x >= px[0].0 - HIT_TOLERANCE
            }
            DrawingKind::VerticalLine => (x - px[0].0).abs() <= tolerance,
            DrawingKind::Rectangle => {
                let (a, b) = (px[0], px[1]);
                let (left, right) = (a.0.min(b.0), a.0.max(b.0));
                let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
                x >= left - HIT_TOLERANCE
                    && x <= right + HIT_TOLERANCE
                    && y >= top - HIT_TOLERANCE
                    && y <= bottom + HIT_TOLERANCE
            }
            DrawingKind::Brush => {
                // The stroke the user sees IS the smooth curve: test against the same curved
                // geometry the series line hit test uses (hit_test.rs `hit_test_line_series`
                // with LineType::Curved), so a hit lands exactly on the painted stroke.
                crate::hit_test::hit_test_line_series(
                    px,
                    x,
                    y,
                    LineType::Curved,
                    drawing.width,
                    None,
                    self.time_scale.bar_spacing(),
                    HIT_TOLERANCE,
                )
                .is_some()
            }
            DrawingKind::Text => {
                if drawing.text.is_empty() {
                    // An unlabeled text tool's click target is its anchor disc.
                    return (x - px[0].0).hypot(y - px[0].1) <= ANCHOR_HIT_RADIUS;
                }
                let layout = &self.options.get().layout;
                let size = drawing.text_size.unwrap_or(layout.font_size);
                let reference =
                    Self::text_box(drawing.kind, px, self.pane_w, 0.0, self.pane_h.max(1.0));
                let (tx, ty, align) = Self::text_placement(drawing, &reference, size, TEXT_PAD);
                let width = self.measure_drawing_text(drawing, size);
                let height = size * 1.2;
                let left = match align {
                    DrawingTextHAlign::Left => tx,
                    DrawingTextHAlign::Center => tx - width / 2.0,
                    DrawingTextHAlign::Right => tx - width,
                };
                x >= left && x <= left + width && y >= ty - height / 2.0 && y <= ty + height / 2.0
            }
        }
    }

    // --- drag session (anchor re-anchoring / whole-body move) ---

    /// Hit-test `(x, y)` and open a drag session on the hit part (body move or anchor
    /// re-anchor). A successful grab also selects the drawing (TradingView parity). Returns
    /// false on a miss — the host falls through to its pan/scroll handling.
    pub fn drawing_drag_start_at(&mut self, x: f64, y: f64) -> bool {
        let Some(hit) = self.hit_test_drawing(x, y) else {
            return false;
        };
        let Some(drawing) = self.drawing(hit.id) else {
            return false;
        };
        let start_points = drawing.points.clone();
        let Some(start_px) = self.drawing_px(drawing) else {
            return false;
        };
        self.selected_drawing = Some(hit.id);
        self.drawing_drag = Some(DrawingDrag {
            id: hit.id,
            part: hit.part,
            start_x: x,
            start_y: y,
            start_points,
            start_px,
        });
        true
    }

    /// Apply the drag to `(x, y)`: an anchor drag re-anchors that point under the cursor; a
    /// body drag translates every anchor in coordinate space and converts back per anchor, so
    /// the shape stays pixel-rigid on any scale mode. The unused coordinate of the full-span
    /// kinds is frozen (a horizontal line moves only vertically, a vertical line only
    /// horizontally). `modifiers` applies magnet (OHLC snap) before straighten (0°/45°/90° for
    /// a trend anchor, square for a rectangle corner, dominant-axis for a body move) — the
    /// session recomputes from its start snapshot each call, so toggling a modifier mid-drag
    /// responds live (TradingView parity).
    pub fn drawing_drag_to(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) {
        let Some(drag) = &self.drawing_drag else {
            return;
        };
        let (dx, dy) = (x - drag.start_x, y - drag.start_y);
        let (id, part) = (drag.id, drag.part);
        let (start_points, start_px) = (drag.start_points.clone(), drag.start_px.clone());
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        let (kind, pane) = (drawing.kind, drawing.pane_index);
        let mut points = start_points;
        let convert = |index: usize, dx: f64, dy: f64| -> Option<DrawingPoint> {
            let (px, py) = start_px.get(index)?;
            self.drawing_from_px(pane, px + dx, py + dy)
        };
        match part {
            DrawingDragPart::Anchor(index) => {
                if index >= points.len() {
                    return;
                }
                let (dx, dy) = match kind {
                    DrawingKind::HorizontalLine => (0.0, dy),
                    DrawingKind::VerticalLine => (dx, 0.0),
                    _ => (dx, dy),
                };
                let Some(mut point) = convert(index, dx, dy) else {
                    return;
                };
                if modifiers.magnet {
                    point = self.magnet_snap_point(pane, point);
                }
                if modifiers.straighten && points.len() == 2 {
                    // The other anchor is the fixed one (only the dragged anchor moves).
                    let fixed = points[1 - index];
                    if let Some(snapped) = self.straighten_point(pane, kind, fixed, point) {
                        point = snapped;
                    }
                }
                points[index] = point;
            }
            DrawingDragPart::Body => {
                // TradingView shift-move: constrain the translation to the dominant axis.
                let (dx, dy) = if modifiers.straighten {
                    if dx.abs() >= dy.abs() {
                        (dx, 0.0)
                    } else {
                        (0.0, dy)
                    }
                } else {
                    (dx, dy)
                };
                for (index, slot) in points.iter_mut().enumerate() {
                    let (dx, dy) = match kind {
                        DrawingKind::HorizontalLine => (0.0, dy),
                        DrawingKind::VerticalLine => (dx, 0.0),
                        _ => (dx, dy),
                    };
                    let Some(point) = convert(index, dx, dy) else {
                        return;
                    };
                    *slot = point;
                }
            }
        }
        if let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) {
            drawing.points = points;
        }
    }

    /// Close the drag session (pointer up/cancel).
    pub fn drawing_drag_end(&mut self) {
        self.drawing_drag = None;
    }

    pub fn drawing_drag_active(&self) -> bool {
        self.drawing_drag.is_some()
    }

    // --- interactive creation (click-place anchors, move previews) ---

    /// Arm interactive creation of `kind` (the reference rectangle-drawing-tool's
    /// `startDrawing`): subsequent clicks place anchors through
    /// [`ChartEngine::drawing_create_click`], mouse moves update the preview through
    /// [`ChartEngine::drawing_create_move`]. `options_json` templates the new drawing
    /// ([`DrawingPatch`]). Replaces any in-progress creation. Returns false for a malformed
    /// options template.
    pub fn drawing_create_begin(&mut self, kind: DrawingKind, options_json: Option<&str>) -> bool {
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
        self.pending_drawing = Some(PendingDrawing {
            drawing,
            preview: None,
        });
        true
    }

    /// Place the next anchor at pane-relative media px `(x, y)`. The first click also binds the
    /// pending drawing to the pane under the cursor; later clicks in a different pane are
    /// ignored. `modifiers` snaps the placed anchor (magnet always; straighten against the
    /// already-placed anchor for two-anchor kinds — TradingView's shift-snapped second point).
    /// Returns 0 while no creation is armed, -1 while more anchors are needed, or the
    /// committed drawing's id (> 0) once the kind's anchor count is reached — the new drawing is
    /// left selected, TradingView-style.
    pub fn drawing_create_click(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) -> i64 {
        let Some(pending) = &self.pending_drawing else {
            return 0;
        };
        let bound_pane = (!pending.drawing.points.is_empty()).then_some(pending.drawing.pane_index);
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
        let Some(mut point) = self.drawing_from_px(pane, x, y) else {
            return -1;
        };
        let (anchor_count, kind, fixed) = {
            let Some(pending) = &self.pending_drawing else {
                return 0;
            };
            (
                pending.drawing.kind.anchor_count(),
                pending.drawing.kind,
                pending.drawing.points.last().copied(),
            )
        };
        if modifiers.magnet {
            point = self.magnet_snap_point(pane, point);
        }
        if modifiers.straighten {
            if let Some(fixed) = fixed {
                if let Some(snapped) = self.straighten_point(pane, kind, fixed, point) {
                    point = snapped;
                }
            }
        }
        let Some(pending) = self.pending_drawing.as_mut() else {
            return -1;
        };
        pending.drawing.pane_index = pane;
        pending.drawing.points.push(point);
        pending.preview = None;
        if pending.drawing.points.len() < anchor_count {
            pending.preview = Some(point);
            return -1;
        }
        let Some(pending) = self.pending_drawing.take() else {
            return -1;
        };
        let id = self.next_drawing_id;
        self.next_drawing_id += 1;
        let mut drawing = pending.drawing;
        drawing.id = id;
        self.drawings.push(drawing);
        self.selected_drawing = Some(id);
        i64::from(id)
    }

    /// Update the creation preview point from a mouse move (no-op while unarmed or off the data).
    /// `modifiers` snaps the preview exactly as a click would snap the placed anchor.
    pub fn drawing_create_move(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) {
        let Some(pending) = &self.pending_drawing else {
            return;
        };
        let unplaced = pending.drawing.points.is_empty();
        let bound_pane = pending.drawing.pane_index;
        // With nothing placed yet the preview follows the cursor in whichever pane it is over;
        // afterwards it stays bound to the first click's pane.
        let pane = if unplaced {
            match self.pane_at_y(y) {
                Some(pane) => pane,
                None => return,
            }
        } else {
            bound_pane
        };
        let Some(mut point) = self.drawing_from_px(pane, x, y) else {
            return;
        };
        if modifiers.magnet {
            point = self.magnet_snap_point(pane, point);
        }
        if modifiers.straighten {
            if let Some(pending) = &self.pending_drawing {
                if let Some(&fixed) = pending.drawing.points.last() {
                    if let Some(snapped) =
                        self.straighten_point(pane, pending.drawing.kind, fixed, point)
                    {
                        point = snapped;
                    }
                }
            }
        }
        if let Some(pending) = self.pending_drawing.as_mut() {
            pending.drawing.pane_index = pane;
            pending.preview = Some(point);
        }
    }

    /// Abandon the in-progress creation (Escape / tool disarm).
    pub fn drawing_create_cancel(&mut self) {
        self.pending_drawing = None;
    }

    pub fn drawing_create_active(&self) -> bool {
        self.pending_drawing.is_some()
    }

    /// The in-progress creation for the frame build (its committed anchors plus the preview
    /// point render as a tentative drawing).
    pub(crate) fn pending_drawing(&self) -> Option<&PendingDrawing> {
        self.pending_drawing.as_ref()
    }

    // --- freehand brush capture (press-drag-release, TradingView's brush) ---

    /// Begin a brush stroke at pane-relative media px `(x, y)` (pointer-down with the brush
    /// tool armed): binds the stroke to the pane under the cursor and captures the first
    /// point. `options_json` templates the committed drawing ([`DrawingPatch`]). Returns false
    /// off the panes/data or for a malformed template.
    pub fn brush_create_start(&mut self, options_json: Option<&str>, x: f64, y: f64) -> bool {
        let Some(pane) = self.pane_at_y(y) else {
            return false;
        };
        let Some(point) = self.drawing_from_px(pane, x, y) else {
            return false;
        };
        let mut options = Drawing::new(0, DrawingKind::Brush, pane, Vec::new());
        if let Some(json) = options_json {
            let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) else {
                return false;
            };
            options.apply_patch(patch);
        }
        self.brush_capture = Some(BrushCapture {
            pane_index: pane,
            points: vec![point],
            last_px: (x, y),
            options,
        });
        true
    }

    /// Capture the next stroke point from a pointer move, decimated by distance
    /// ([`BRUSH_MIN_POINT_DISTANCE`] in media px — closer samples are pointer noise).
    pub fn brush_create_add(&mut self, x: f64, y: f64) {
        let Some(capture) = &self.brush_capture else {
            return;
        };
        if (x - capture.last_px.0).hypot(y - capture.last_px.1) < BRUSH_MIN_POINT_DISTANCE {
            return;
        }
        let pane = capture.pane_index;
        let Some(point) = self.drawing_from_px(pane, x, y) else {
            return;
        };
        if let Some(capture) = self.brush_capture.as_mut() {
            capture.points.push(point);
            capture.last_px = (x, y);
        }
    }

    /// Commit the stroke (pointer-up): the captured path is RDP-simplified in media-px space
    /// ([`BRUSH_SIMPLIFY_TOLERANCE`] — the stored path is the smooth centerline of the stroke,
    /// rendered as a curved polyline) and committed as a selected drawing. Returns the id, or
    /// 0 discarding a degenerate stroke (fewer than two surviving points / no capture active).
    pub fn brush_create_end(&mut self) -> DrawingId {
        let Some(capture) = self.brush_capture.take() else {
            return 0;
        };
        if capture.points.len() < 2 {
            return 0;
        }
        // Simplify in pixel space, then convert the surviving corners back to anchors.
        let px: Option<Vec<(f64, f64)>> = capture
            .points
            .iter()
            .map(|&point| self.drawing_to_px(capture.pane_index, point))
            .collect();
        let Some(px) = px else {
            return 0;
        };
        let simplified = rdp_simplify(&px, BRUSH_SIMPLIFY_TOLERANCE);
        let points: Option<Vec<DrawingPoint>> = simplified
            .iter()
            .map(|&(x, y)| self.drawing_from_px(capture.pane_index, x, y))
            .collect();
        let Some(points) = points else {
            return 0;
        };
        if points.len() < 2 {
            return 0;
        }
        let id = self.next_drawing_id;
        self.next_drawing_id += 1;
        let mut drawing = capture.options;
        drawing.id = id;
        drawing.pane_index = capture.pane_index;
        drawing.points = points;
        self.drawings.push(drawing);
        self.selected_drawing = Some(id);
        id
    }

    /// Abandon the in-progress stroke (Escape / tool disarm).
    pub fn brush_create_cancel(&mut self) {
        self.brush_capture = None;
    }

    pub fn brush_create_active(&self) -> bool {
        self.brush_capture.is_some()
    }

    /// The in-progress stroke for the frame build (paints as a live curved polyline).
    pub(crate) fn brush_capture(&self) -> Option<&BrushCapture> {
        self.brush_capture.as_ref()
    }
}

#[cfg(test)]
mod tests;
