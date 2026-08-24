//! Drawing tools (trend line, horizontal line/ray, vertical line, rectangle, text) as
//! engine-owned drawing objects — TradingView's drawing tools in the spirit of the reference's
//! plugin-examples (trend-line.ts, rectangle-drawing-tool.ts, vertical-line.ts, anchored-text.ts),
//! but with all state, hit-testing, and anchor-dragging math living headless here: hosts only
//! forward gestures and render the frame, exactly like the interaction-model split
//! at the headless engine boundary.
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

use std::collections::HashMap;
use std::rc::Rc;

use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{LineStyle, LineType};

use super::*;

/// Chart-unique drawing id (never reused within a chart; 0 is the "no drawing" sentinel).
pub type DrawingId = u32;

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
        if drawing.kind == DrawingKind::Brush {
            let logical_pad = (max_logical - min_logical).abs() * 0.25;
            let price_pad = (max_price - min_price).abs() * 0.25;
            min_logical -= logical_pad;
            max_logical += logical_pad;
            min_price -= price_pad;
            max_price += price_pad;
        }
        let logical = match drawing.kind {
            DrawingKind::HorizontalLine => LogicalBounds::Full,
            DrawingKind::HorizontalRay => LogicalBounds::From(min_logical),
            _ => LogicalBounds::Finite {
                min: min_logical,
                max: max_logical,
            },
        };
        let (min_price, max_price) = if drawing.kind == DrawingKind::VerticalLine {
            (None, None)
        } else {
            (Some(min_price), Some(max_price))
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
pub const DRAWING_DEFAULT_COLOR: &str = nucleuscharts_core::style::DEFAULT_PRIMARY_CSS;

/// An engine-owned drawing. Colors follow the series pattern: stored verbatim as CSS strings
/// and parsed at render time (`None`/unparseable falls back to the follow behavior documented
/// per field).
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub id: DrawingId,
    pub kind: DrawingKind,
    pub pane_index: usize,
    pub points: Vec<DrawingPoint>,
    pub price_scale: DrawingPriceScale,
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
    /// Label color CSS string; `None` follows the chart's `layout.textColor`.
    pub text_color: Option<String>,
    /// Label glyph size in CSS px; `None` follows the chart's `layout.fontSize`.
    pub text_size: Option<f64>,
    /// Label font weight (numeric CSS weight 100–900; `None` = normal 400, 700 = bold).
    pub text_weight: Option<u16>,
    pub text_italic: bool,
    pub text_h_align: DrawingTextHAlign,
    pub text_v_align: DrawingTextVAlign,
    /// Text-tool container background CSS string (TradingView's text-box background); `None`
    /// draws no box. Text tool only.
    pub box_color: Option<String>,
    /// Text-tool container border CSS string; `None` draws no border. Text tool only.
    pub box_border_color: Option<String>,
    /// Text-tool container border width in CSS px (default 1).
    pub box_border_width: f64,
}

/// Default glyph size for the TEXT TOOL's label in CSS px when `text_size` is unset
/// (TradingView's default text-tool size). Other tools' labels keep following the chart's
/// `layout.font_size`.
pub const TEXT_TOOL_DEFAULT_SIZE: f64 = 18.0;

/// The text tool's interaction-chrome padding in CSS px: the editing border (2 px) plus its
/// padding (4 px), mirrored by the host's `#nucleuscharts-text-editor` wrap (impl.ts). The
/// hit area and the hover/focus borders all use this box so selection, hovering, and typing
/// mode land on exactly the same outline.
pub(crate) const TEXT_CHROME_PAD: f64 = TEXT_PAD + 2.0;

impl Drawing {
    pub(crate) fn new(
        id: DrawingId,
        kind: DrawingKind,
        pane_index: usize,
        points: Vec<DrawingPoint>,
    ) -> Self {
        Self {
            id,
            kind,
            pane_index,
            points,
            price_scale: DrawingPriceScale::Right,
            color: DRAWING_DEFAULT_COLOR.to_string(),
            width: if kind == DrawingKind::Rectangle {
                1.0
            } else {
                2.0
            },
            style: LineStyle::Solid,
            fill_color: None,
            preview_fill_color: None,
            border_visible: true,
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
            text_h_align: DrawingTextHAlign::Center,
            text_v_align: DrawingTextVAlign::Middle,
            box_color: None,
            box_border_color: None,
            box_border_width: 1.0,
        }
    }

    /// The label a drawing actually renders. Empty text tools render nothing — the host's
    /// typing-mode editor is the only empty-state UI, and leaving that editor without typed
    /// text removes the drawing (TradingView: no lingering "Add text" ghost on the chart).
    pub fn display_text(&self) -> &str {
        self.text.as_str()
    }

    /// The glyph size the label actually renders at in CSS px: `text_size` when set, else
    /// [`TEXT_TOOL_DEFAULT_SIZE`] for the text tool and the chart's `layout.font_size` for
    /// every other tool's label. Rendering, hit-testing, and host editing chrome must all
    /// resolve through this so they never disagree.
    pub fn resolved_text_size(&self, layout_font_size: f64) -> f64 {
        self.text_size.unwrap_or(match self.kind {
            DrawingKind::Text => TEXT_TOOL_DEFAULT_SIZE,
            _ => layout_font_size,
        })
    }

    fn rebase_logical(&mut self, mapping: &MergedTimeMapping) -> bool {
        rebase_points(&mut self.points, mapping)
    }
}

fn rebase_points(points: &mut [DrawingPoint], mapping: &MergedTimeMapping) -> bool {
    let mut changed = false;
    for point in points {
        let logical = mapping.map_logical(point.logical);
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
/// `{italic} {weight} {size}px {family}` and returning the width in the same px units as
/// `size`). Without one the engine estimates `chars × size × 0.6` (deterministic for native
/// tests).
pub type TextMeasureFn = Box<dyn Fn(&str, f64, &str, u16, bool) -> f64>;

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
}

impl DrawingCommand {
    fn rebase_logical(&mut self, mapping: &MergedTimeMapping) {
        match self {
            Self::Create { drawing, .. } | Self::Delete { drawing, .. } => {
                drawing.rebase_logical(mapping);
            }
            Self::Update { before, after } => {
                before.rebase_logical(mapping);
                after.rebase_logical(mapping);
            }
            Self::Clear { drawings } => {
                for drawing in drawings {
                    drawing.rebase_logical(mapping);
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

    fn rebase_logical(&mut self, mapping: &MergedTimeMapping) {
        for command in self.undo.iter_mut().chain(&mut self.redo) {
            command.rebase_logical(mapping);
        }
    }
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
/// distance on the way in (media px) and committed as-is — the stored path is exactly the curve
/// the live stroke painted.
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
#[derive(Clone, serde::Deserialize, Default)]
pub(crate) struct DrawingPatch {
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
        if let Some(scale) = patch
            .price_scale_id
            .as_deref()
            .and_then(DrawingPriceScale::from_name)
        {
            self.price_scale = scale;
        }
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
        if let Some(size) = patch.text_size {
            if size.is_finite() && size > 0.0 {
                self.text_size = Some(size);
            }
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
        if let Some(width) = patch.box_border_width {
            if width.is_finite() && width > 0.0 {
                self.box_border_width = width;
            }
        }
    }

    fn options_json(&self) -> serde_json::Value {
        serde_json::json!({
            "price_scale_id": self.price_scale.name(),
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
    pub(crate) fn rebase_drawing_logicals(&mut self, mapping: &MergedTimeMapping) {
        let mut committed_changed = false;
        for drawing in &mut self.drawings {
            committed_changed |= drawing.rebase_logical(mapping);
        }
        let mut transient_changed = false;
        if let Some(pending) = self.pending_drawing.as_mut() {
            transient_changed |= pending.drawing.rebase_logical(mapping);
            if let Some(preview) = pending.preview.as_mut() {
                transient_changed |= rebase_points(std::slice::from_mut(preview), mapping);
            }
        }
        if let Some(capture) = self.brush_capture.as_mut() {
            transient_changed |= rebase_points(&mut capture.points, mapping);
            transient_changed |= capture.options.rebase_logical(mapping);
        }
        if let Some(drag) = self.drawing_drag.as_mut() {
            transient_changed |= rebase_points(&mut drag.start_points, mapping);
            transient_changed |= rebase_points(&mut drag.history_points, mapping);
        }
        self.drawing_history.rebase_logical(mapping);

        if committed_changed {
            self.drawing_runtime
                .borrow_mut()
                .rebuild_all(&self.drawings, self.panes.len());
        }
        if committed_changed || transient_changed {
            self.invalidate_frame_drawings();
        }
    }

    pub(crate) fn refresh_drawing_pixel_baselines(&mut self) {
        let drag_baseline = self.drawing_drag.as_ref().and_then(|drag| {
            let drawing = self.drawing(drag.id)?;
            let points = drawing.points.clone();
            let px = points
                .iter()
                .map(|&point| {
                    self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)
                })
                .collect::<Option<Vec<_>>>()?;
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

        let brush_px = self
            .brush_capture
            .as_ref()
            .and_then(|capture| self.drawing_to_px(capture.pane_index, *capture.points.last()?));
        if let (Some(capture), Some(last_px)) = (self.brush_capture.as_mut(), brush_px) {
            capture.last_px = last_px;
        }
    }

    fn record_drawing_command(&mut self, command: DrawingCommand) {
        if self.drawing_history.undo.len() == DRAWING_HISTORY_LIMIT {
            self.drawing_history.undo.remove(0);
        }
        self.drawing_history.undo.push(command);
        self.drawing_history.redo.clear();
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
        self.drawing_runtime.borrow_mut().remove(id, &self.drawings);
        if self.selected_drawing == Some(id) {
            self.selected_drawing = None;
        }
        if self.drawing_drag.as_ref().is_some_and(|drag| drag.id == id) {
            self.drawing_drag = None;
        }
        if self.editing_drawing == Some(id) {
            self.editing_drawing = None;
        }
        if self.hovered_text == Some(id) {
            self.hovered_text = None;
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
            }
        }
    }

    /// Undo one committed drawing-semantic operation for this chart only.
    pub fn undo_drawing(&mut self) -> bool {
        let Some(command) = self.drawing_history.undo.pop() else {
            return false;
        };
        self.invalidate_frame_drawings();
        self.pending_drawing = None;
        self.brush_capture = None;
        self.apply_drawing_command(&command, true);
        self.drawing_history.redo.push(command);
        true
    }

    /// Redo one previously undone drawing-semantic operation for this chart only.
    pub fn redo_drawing(&mut self) -> bool {
        let Some(command) = self.drawing_history.redo.pop() else {
            return false;
        };
        self.invalidate_frame_drawings();
        self.pending_drawing = None;
        self.brush_capture = None;
        self.apply_drawing_command(&command, false);
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

    fn drawing_scale_for(
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

    fn drawing_scale_base_for(&self, pane_index: usize, target: DrawingPriceScale) -> f64 {
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

    fn snap_drawing_time_to_data(&self, mut point: DrawingPoint) -> Option<DrawingPoint> {
        let logical = point.logical.round() as i64;
        if logical < 0 || logical as usize >= self.data.merged_times().len() {
            return None;
        }
        point.logical = logical as f64;
        Some(point)
    }

    /// TradingView magnet (Ctrl held): resolve the live pointer through the same pixel-space
    /// OHLC candidate path as the crosshair, then encode the winning coordinate on the drawing's
    /// own price scale. A bar with no visible real candidate keeps the unsnapped point.
    fn magnet_snap_point_at(
        &self,
        pane_index: usize,
        price_scale: DrawingPriceScale,
        x: f64,
        y: f64,
        point: DrawingPoint,
    ) -> DrawingPoint {
        let Some((logical, snapped_y)) = self.ohlc_magnet_snap_coordinate(
            pane_index,
            x,
            y,
            &[
                PlotValueIndex::Open,
                PlotValueIndex::High,
                PlotValueIndex::Low,
                PlotValueIndex::Close,
            ],
        ) else {
            return point;
        };
        let Some(mut snapped) = self.drawing_from_px_for(pane_index, price_scale, x, snapped_y)
        else {
            return point;
        };
        snapped.logical = logical as f64;
        snapped
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
        price_scale: DrawingPriceScale,
        kind: DrawingKind,
        fixed: DrawingPoint,
        dragged: DrawingPoint,
    ) -> Option<DrawingPoint> {
        let (fx, fy) = self.drawing_to_px_for(pane_index, price_scale, fixed)?;
        let (dx, dy) = self.drawing_to_px_for(pane_index, price_scale, dragged)?;
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
        self.drawing_from_px_for(pane_index, price_scale, fx + vx, fy + vy)
    }

    /// The anchors of `drawing` in media px (`None` when any anchor fails to resolve), with the
    /// pending preview appended when `preview` holds (the interactive-creation geometry).
    pub(crate) fn drawing_px(&self, drawing: &Drawing) -> Option<Vec<(f64, f64)>> {
        drawing
            .points
            .iter()
            .map(|&point| self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point))
            .collect()
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
            self.dpr.to_bits(),
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
        let (left, right) = match entry.bounds.logical {
            LogicalBounds::Full => (0.0, self.pane_w),
            LogicalBounds::From(logical) => {
                let start_x = self.time_scale.logical_to_coordinate(logical);
                if start_x <= self.pane_w || !drawing.text.is_empty() {
                    (start_x, self.pane_w)
                } else {
                    (start_x, start_x)
                }
            }
            LogicalBounds::Finite { min, max } => (
                self.time_scale.logical_to_coordinate(min),
                self.time_scale.logical_to_coordinate(max),
            ),
        };
        let (top, bottom) = match (entry.bounds.min_price, entry.bounds.max_price) {
            (Some(min), Some(max)) => (
                scale.price_to_coordinate(max, base),
                scale.price_to_coordinate(min, base),
            ),
            _ => (pane.top, pane.top + pane.height),
        };
        let mut extra_x = drawing.width / 2.0 + HitProfile::TOUCH.drawing_stroke_tolerance;
        let mut extra_y = extra_x;
        extra_x = extra_x.max(HitProfile::TOUCH.drawing_anchor_radius);
        extra_y = extra_y.max(HitProfile::TOUCH.drawing_anchor_radius);
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
            for &point in &drawing.points {
                entry.media_px.push(self.drawing_to_px_for(
                    drawing.pane_index,
                    drawing.price_scale,
                    point,
                )?);
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
        if drawing.text.is_empty() && drawing.kind != DrawingKind::Text {
            return None;
        }
        if entry.text_key != key {
            let size = drawing.resolved_text_size(font_size);
            entry.text_width = self.measure_drawing_text_with_family(drawing, size, font_family);
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

    /// The rectangle's 8 TradingView anchors derived from its two corners (media px), in
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

    /// The directional resize cursor for a rectangle anchor (TradingView parity): diagonal
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

    /// Measure (or estimate) a label's width in the same px units as `size`. Empty text tools
    /// use one em so the focus/hover chrome and hit target stay a caret-sized box (the host
    /// never leaves an empty text drawing on the chart after editing).
    pub(crate) fn measure_drawing_text(&self, drawing: &Drawing, size: f64) -> f64 {
        let layout = &self.options.get().layout;
        self.measure_drawing_text_with_family(drawing, size, &layout.font_family)
    }

    fn measure_drawing_text_with_family(&self, drawing: &Drawing, size: f64, family: &str) -> f64 {
        if drawing.kind == DrawingKind::Text && drawing.text.is_empty() {
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
        self.invalidate_frame_drawings();
        if pane_index >= self.panes.len() || !kind.valid_point_count(points.len()) {
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
        if let Some(json) = options_json {
            if let Ok(patch) = serde_json::from_str::<DrawingPatch>(json) {
                drawing.apply_patch(patch);
            }
        }
        self.drawings.push(drawing);
        self.insert_drawing_runtime(id);
        let index = self.drawings.len() - 1;
        self.record_drawing_command(DrawingCommand::Create {
            drawing: self.drawings[index].clone(),
            index,
        });
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
        let drawing = &mut self.drawings[index];
        drawing.apply_patch(patch);
        let after = drawing.clone();
        self.update_drawing_runtime(id);
        if before != after {
            self.record_drawing_command(DrawingCommand::Update {
                before,
                after: Box::new(after),
            });
        }
        true
    }

    /// Replace a drawing's anchors from a JSON `[{logical, price}, ...]` array. Returns false
    /// for malformed JSON, a wrong count for the kind, non-finite values, or an unknown id.
    pub fn drawing_set_points(&mut self, id: DrawingId, json: &str) -> bool {
        self.invalidate_frame_drawings();
        let Ok(points) = serde_json::from_str::<Vec<DrawingPoint>>(json) else {
            return false;
        };
        let Some(index) = self.drawings.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        let drawing = &self.drawings[index];
        if !drawing.kind.valid_point_count(points.len())
            || points
                .iter()
                .any(|p| !p.logical.is_finite() || !p.price.is_finite())
        {
            return false;
        }
        let before = drawing.clone();
        self.drawings[index].points = points;
        let after = self.drawings[index].clone();
        self.update_drawing_runtime(id);
        if before != after {
            self.record_drawing_command(DrawingCommand::Update {
                before,
                after: Box::new(after),
            });
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
        true
    }

    /// Remove every drawing (the demo's "clear all") and release the selection/drag state.
    pub fn clear_drawings(&mut self) {
        self.invalidate_frame_drawings();
        if self.drawings.is_empty() {
            return;
        }
        let drawings = std::mem::take(&mut self.drawings);
        self.drawing_runtime.borrow_mut().clear();
        self.selected_drawing = None;
        self.drawing_drag = None;
        self.record_drawing_command(DrawingCommand::Clear { drawings });
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

    /// One anchor's media-px position (x pane-relative, y chart-top — the host's overlay
    /// coordinate space; the text editor positions itself with it). `None` for an unknown
    /// id/index or when the anchor cannot convert (stale pane, empty scale/data).
    pub fn drawing_point_to_coordinate(&self, id: DrawingId, index: usize) -> Option<(f64, f64)> {
        let drawing = self.drawing(id)?;
        let point = drawing.points.get(index)?;
        self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, *point)
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
        self.invalidate_frame_overlay();
        self.selected_drawing = id.filter(|&sid| self.drawings.iter().any(|d| d.id == sid));
    }

    pub fn selected_drawing(&self) -> Option<DrawingId> {
        self.selected_drawing
    }

    /// Mark the text drawing the host's typing-mode editor currently owns. The frame keeps
    /// painting the label and the focus border underneath the host's borderless caret overlay
    /// (TradingView's overlay-caret model); this flag is the host/query seam for that session.
    /// Cleared when the editor closes. An unknown id never sticks.
    pub fn set_editing_drawing(&mut self, id: Option<DrawingId>) {
        self.invalidate_frame_overlay();
        self.editing_drawing = id.filter(|&eid| {
            self.drawings
                .iter()
                .any(|d| d.id == eid && d.kind == DrawingKind::Text)
        });
    }

    pub fn editing_drawing(&self) -> Option<DrawingId> {
        self.editing_drawing
    }

    /// Mark the TEXT drawing under the host's pointer (`None` when hovering anything else):
    /// the overlay frame paints its focus border at hover opacity (TradingView's hover ring).
    /// Only text drawings stick — the other kinds have no hover affordance, so hovering them
    /// must not invalidate the overlay. Repaint-worthy changes only.
    pub fn set_hovered_text(&mut self, id: Option<DrawingId>) {
        let valid = id.filter(|&hid| {
            self.drawings
                .iter()
                .any(|d| d.id == hid && d.kind == DrawingKind::Text)
        });
        if valid != self.hovered_text {
            self.invalidate_frame_overlay();
            self.hovered_text = valid;
        }
    }

    pub fn hovered_text(&self) -> Option<DrawingId> {
        self.hovered_text
    }

    /// Select the drawing under pane-relative media px `(x, y)` (the host click pipeline):
    /// the topmost hit is selected; a miss clears the selection. Returns whether a drawing was
    /// hit (the host then skips its series-selection path).
    pub fn select_drawing_at(&mut self, x: f64, y: f64) -> bool {
        self.invalidate_frame_overlay();
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
        // TradingView anchors (four corners + four edge midpoints); the rest show one per
        // defining anchor.
        if let Some(selected) = self.selected_drawing {
            if let Some(drawing) = self.drawing(selected) {
                if drawing.pane_index == pane {
                    if drawing.kind == DrawingKind::Brush && drawing.points.len() > 2 {
                        for &index in &[0, drawing.points.len() - 1] {
                            if let Some((ax, ay)) = self.drawing_to_px_for(
                                drawing.pane_index,
                                drawing.price_scale,
                                drawing.points[index],
                            ) {
                                if (x - ax).hypot(y - ay) <= profile.drawing_anchor_radius {
                                    return Some(DrawingHit {
                                        id: selected,
                                        part: DrawingDragPart::Anchor(index),
                                        cursor: "pointer",
                                    });
                                }
                            }
                        }
                    } else if let Some(px) = self.drawing_px(drawing) {
                        if drawing.kind == DrawingKind::Rectangle && px.len() == 2 {
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
                        } else {
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
                    }
                }
            }
        }
        if !indexed {
            for drawing in self.drawings.iter().rev() {
                if drawing.pane_index != pane {
                    continue;
                }
                let Some(px) = self.drawing_px(drawing) else {
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
        match drawing.kind {
            DrawingKind::TrendLine => {
                let (a, b) = (px[0], px[1]);
                distance_to_segment(x, y, a.0, a.1, b.0, b.1) <= tolerance
            }
            DrawingKind::HorizontalLine => (y - px[0].1).abs() <= tolerance,
            DrawingKind::HorizontalRay => {
                (y - px[0].1).abs() <= tolerance && x >= px[0].0 - hit_tolerance
            }
            DrawingKind::VerticalLine => (x - px[0].0).abs() <= tolerance,
            DrawingKind::Rectangle => {
                // TradingView: the fill is a drag surface only while the drawing is SELECTED
                // (a border click selects first); unselected, the body hits just the band
                // around the border frame and the middle pans the chart.
                let (a, b) = (px[0], px[1]);
                let (left, right) = (a.0.min(b.0), a.0.max(b.0));
                let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
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
                    hit_tolerance,
                )
                .is_some()
            }
            DrawingKind::Text => {
                // The click/hover target is the interaction-chrome box (the label run while
                // non-empty, else a one-em caret box — empty text paints nothing on the chart)
                // plus the editing chrome's border+padding, so the painted hover/focus border
                // is itself hittable.
                let layout = &self.options.get().layout;
                let size = drawing.resolved_text_size(layout.font_size);
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
                x >= left - TEXT_CHROME_PAD
                    && x <= left + width + TEXT_CHROME_PAD
                    && y >= ty - height / 2.0 - TEXT_CHROME_PAD
                    && y <= ty + height / 2.0 + TEXT_CHROME_PAD
            }
        }
    }

    // --- drag session (anchor re-anchoring / whole-body move) ---

    /// Hit-test `(x, y)` and open a drag session on the hit part (body move or anchor
    /// re-anchor). A successful grab also selects the drawing (TradingView parity). Returns
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
            current_x: x,
            current_y: y,
            history_points: start_points.clone(),
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
        self.invalidate_frame_drawings();
        let Some(drag) = self.drawing_drag.as_mut() else {
            return;
        };
        drag.current_x = x;
        drag.current_y = y;
        let (dx, dy) = (x - drag.start_x, y - drag.start_y);
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
        let mut points = start_points;
        let convert = |index: usize, dx: f64, dy: f64| -> Option<DrawingPoint> {
            let (px, py) = start_px.get(index)?;
            self.drawing_from_px_for(pane, price_scale, px + dx, py + dy)
        };
        match part {
            DrawingDragPart::Anchor(index) => {
                if kind == DrawingKind::Rectangle && points.len() == 2 && index < 8 {
                    // Rectangle anchors (drawings.rs `rectangle_anchors` clock order): a corner
                    // drag moves that corner (Shift squares against the fixed opposite corner),
                    // an edge-midpoint drag moves only that edge. Every slot KEEPS its corner
                    // identity — crossing the opposite side flips visually at render (the box
                    // normalizes), it never reorders the anchors or slides the fixed side
                    // (TradingView parity).
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
                    if kind == DrawingKind::Brush {
                        self.invalidate_brush_drag_runtime(id);
                    } else {
                        self.update_drawing_runtime(id);
                    }
                    return;
                }
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
                if snap_time_to_data {
                    let Some(snapped) = self.snap_drawing_time_to_data(point) else {
                        return;
                    };
                    point = snapped;
                }
                if modifiers.magnet {
                    let snapped = self.magnet_snap_point_at(pane, price_scale, x, y, point);
                    point = match kind {
                        DrawingKind::HorizontalLine => DrawingPoint {
                            price: snapped.price,
                            ..point
                        },
                        DrawingKind::VerticalLine => DrawingPoint {
                            logical: snapped.logical,
                            ..point
                        },
                        _ => snapped,
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
                let single_anchor = points.len() == 1;
                for (index, slot) in points.iter_mut().enumerate() {
                    let (dx, dy) = match kind {
                        DrawingKind::HorizontalLine => (0.0, dy),
                        DrawingKind::VerticalLine => (dx, 0.0),
                        _ => (dx, dy),
                    };
                    let Some(mut point) = convert(index, dx, dy) else {
                        return;
                    };
                    if snap_time_to_data {
                        let Some(snapped) = self.snap_drawing_time_to_data(point) else {
                            return;
                        };
                        point = snapped;
                    }
                    // Single-anchor kinds drag by their line, not a handle — the body drag IS
                    // the anchor drag, so the magnet applies here too (a Ctrl-dragged vertical
                    // line snaps to bar centers, a horizontal one to the nearest OHLC price).
                    if modifiers.magnet && single_anchor {
                        let snapped = self.magnet_snap_point_at(pane, price_scale, x, y, point);
                        point = match kind {
                            DrawingKind::HorizontalLine => DrawingPoint {
                                price: snapped.price,
                                ..point
                            },
                            DrawingKind::VerticalLine => DrawingPoint {
                                logical: snapped.logical,
                                ..point
                            },
                            _ => snapped,
                        };
                    }
                    *slot = point;
                }
            }
        }
        if let Some(drawing) = self.drawings.iter_mut().find(|d| d.id == id) {
            drawing.points = points;
        }
        if kind == DrawingKind::Brush {
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
            self.update_drawing_runtime(id);
            if let Some(after) = self.drawing(id).cloned() {
                let mut before = after.clone();
                before.points = drag.history_points;
                if before != after {
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
            start_points,
            start_px,
        });
        self.drawing_drag_to(dx_css, dy_css, DrawingModifiers::default());
        self.drawing_drag_end();
        true
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
        self.invalidate_frame_drawings();
        let Some(pending) = &self.pending_drawing else {
            return 0;
        };
        let price_scale = pending.drawing.price_scale;
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
        let Some(mut point) = self.drawing_from_px_for(pane, price_scale, x, y) else {
            return -1;
        };
        let (anchor_count, kind, fixed, snap_time_to_data) = {
            let Some(pending) = &self.pending_drawing else {
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
        if modifiers.straighten {
            if let Some(fixed) = fixed {
                if let Some(snapped) = self.straighten_point(pane, price_scale, kind, fixed, point)
                {
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
        let Some(id) = self.take_drawing_id() else {
            return 0;
        };
        let mut drawing = pending.drawing;
        drawing.id = id;
        self.drawings.push(drawing);
        self.insert_drawing_runtime(id);
        self.selected_drawing = Some(id);
        let index = self.drawings.len() - 1;
        self.record_drawing_command(DrawingCommand::Create {
            drawing: self.drawings[index].clone(),
            index,
        });
        i64::from(id)
    }

    /// Update the creation preview point from a mouse move (no-op while unarmed or off the data).
    /// `modifiers` snaps the preview exactly as a click would snap the placed anchor.
    pub fn drawing_create_move(&mut self, x: f64, y: f64, modifiers: DrawingModifiers) {
        self.invalidate_frame_drawings();
        let Some(pending) = &self.pending_drawing else {
            return;
        };
        let unplaced = pending.drawing.points.is_empty();
        let bound_pane = pending.drawing.pane_index;
        let price_scale = pending.drawing.price_scale;
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
        if modifiers.straighten {
            if let Some(pending) = &self.pending_drawing {
                if let Some(&fixed) = pending.drawing.points.last() {
                    if let Some(snapped) =
                        self.straighten_point(pane, price_scale, pending.drawing.kind, fixed, point)
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
        match (&mut self.pending_drawing, &mut self.brush_capture) {
            (Some(pending), Some(capture)) => {
                pending.drawing.apply_patch(patch.clone());
                capture.options.apply_patch(patch);
            }
            (Some(pending), None) => pending.drawing.apply_patch(patch),
            (None, Some(capture)) => capture.options.apply_patch(patch),
            (None, None) => return false,
        }
        true
    }

    /// Abandon the in-progress creation (Escape / tool disarm).
    pub fn drawing_create_cancel(&mut self) {
        self.invalidate_frame_drawings();
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
        self.invalidate_frame_drawings();
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
    /// ([`BRUSH_MIN_POINT_DISTANCE`] in media px — closer samples are pointer noise). Returns
    /// whether a point was captured; rejected samples leave the frame untouched, so hosts can
    /// skip the repaint instead of rebuilding the drawings layer per raw pointer event.
    pub fn brush_create_add(&mut self, x: f64, y: f64) -> bool {
        let Some(capture) = &self.brush_capture else {
            return false;
        };
        if (x - capture.last_px.0).hypot(y - capture.last_px.1) < BRUSH_MIN_POINT_DISTANCE {
            return false;
        }
        let pane = capture.pane_index;
        let Some(point) = self.drawing_from_px(pane, x, y) else {
            return false;
        };
        if let Some(capture) = self.brush_capture.as_mut() {
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
        let Some(capture) = self.brush_capture.take() else {
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
