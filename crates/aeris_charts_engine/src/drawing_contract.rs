//! Typed drawing customization contracts shared by native and browser hosts.
//!
//! The engine keeps the live [`Drawing`](crate::Drawing) representation compact for the hot
//! render and hit-test paths.  This module is the stable, typed view used by property panels,
//! templates, persistence adapters, and cross-cell synchronization.  Values are deliberately
//! bounded and deterministic so a host cannot turn a drawing patch into unbounded work.

use crate::{DrawingKind, DrawingPoint, DrawingPriceScale};

pub const DRAWING_CONTRACT_REVISION: u32 = 2;
pub const MAX_DRAWING_NAME_BYTES: usize = 256;
pub const MAX_DRAWING_GROUP_BYTES: usize = 128;
pub const MAX_DRAWING_LABELS: usize = 32;
pub const MAX_DRAWING_LEVELS: usize = 64;
pub const MAX_DRAWING_TEMPLATE_BYTES: usize = 64 * 1024;
pub const MAX_DRAWING_TEMPLATES: usize = 128;
pub const MAX_DRAWING_OBJECTS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingPropertyType {
    Boolean,
    Number,
    Integer,
    String,
    Color,
    Enum,
    /// A list of [`DrawingLevel`] rows.
    Levels,
    /// A list of [`DrawingLabelOptions`]; `enum_values` names the metrics the kind renders.
    Labels,
    IntervalSet,
}

/// The settings surface a property belongs to. `General` properties are identity and state the
/// object tree or toolbar owns (name, lock, visibility); a settings dialog does not show them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingPropertyGroup {
    #[default]
    General,
    Inputs,
    Style,
    Text,
    Coordinates,
    Visibility,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingPropertyDescriptor {
    pub name: String,
    /// English display label.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub group: DrawingPropertyGroup,
    /// Offered as a quick control on the floating drawing toolbar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub toolbar: bool,
    pub property_type: DrawingPropertyType,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_values: Vec<String>,
}

/// One editable anchor for a Coordinates editor: which of its price and time the kind actually
/// uses. Variable-length freehand and multi-click kinds expose none.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DrawingAnchorDescriptor {
    pub index: usize,
    pub label: String,
    pub price: bool,
    pub time: bool,
}

/// Complete editor description of one kind: every property that changes what the kind paints,
/// grouped for a settings dialog and flagged for the toolbar, plus its editable anchors.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingPropertySchema {
    pub revision: u32,
    pub kind: DrawingKind,
    pub properties: Vec<DrawingPropertyDescriptor>,
    #[serde(default)]
    pub anchors: Vec<DrawingAnchorDescriptor>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingLineCap {
    #[default]
    None,
    Arrow,
    Circle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingMagnetMode {
    #[default]
    Off,
    Weak,
    Strong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingIntervalUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
    Ticks,
    Ranges,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingInterval {
    pub unit: DrawingIntervalUnit,
    pub value: f64,
}

impl DrawingInterval {
    pub fn validate(self) -> bool {
        self.value.is_finite() && self.value > 0.0
    }
}

/// An inclusive span of one interval unit, e.g. 1–59 minutes.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingIntervalRange {
    pub unit: DrawingIntervalUnit,
    pub min: f64,
    pub max: f64,
}

impl DrawingIntervalRange {
    pub fn validate(self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.min > 0.0 && self.min <= self.max
    }

    fn contains(self, interval: DrawingInterval) -> bool {
        let tolerance = f64::EPSILON.max(interval.value * 1e-9);
        self.unit == interval.unit
            && interval.value >= self.min - tolerance
            && interval.value <= self.max + tolerance
    }
}

/// Per-drawing timeframe visibility. Disabled shows the drawing on every interval; enabled shows
/// it only on an exact listed interval or inside a listed unit range.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingIntervalVisibility {
    pub enabled: bool,
    #[serde(default)]
    pub intervals: Vec<DrawingInterval>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<DrawingIntervalRange>,
}

impl DrawingIntervalVisibility {
    pub fn allows(&self, current: Option<DrawingInterval>) -> bool {
        if !self.enabled {
            return true;
        }
        let Some(current) = current else {
            return false;
        };
        self.intervals.iter().any(|allowed| {
            allowed.unit == current.unit
                && (allowed.value - current.value).abs() <= f64::EPSILON.max(current.value * 1e-9)
        }) || self.ranges.iter().any(|range| range.contains(current))
    }

    pub fn validate(&self) -> bool {
        self.intervals.len() <= 16
            && self.intervals.iter().all(|interval| interval.validate())
            && self.ranges.len() <= 16
            && self.ranges.iter().all(|range| range.validate())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingLabelMetric {
    Price,
    PriceChange,
    PercentChange,
    Ticks,
    BarCount,
    DateTimeRange,
    Duration,
    Angle,
    Distance,
    VolumeInRange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingLabelPosition {
    Above,
    #[default]
    On,
    Below,
    Inside,
    Outside,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingLabelOptions {
    pub metric: DrawingLabelMetric,
    pub visible: bool,
    pub position: DrawingLabelPosition,
    #[serde(default)]
    pub text: Option<String>,
}

impl DrawingLabelOptions {
    pub fn validate(&self) -> bool {
        self.text.as_ref().is_none_or(|text| text.len() <= 256)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingLevel {
    pub value: f64,
    pub color: String,
    pub visible: bool,
    pub style: String,
    pub fill_between: bool,
    #[serde(default)]
    pub fill_color: Option<String>,
    pub label_visible: bool,
}

impl DrawingLevel {
    pub fn validate(&self) -> bool {
        self.value.is_finite()
            && self.color.len() <= 256
            && self
                .fill_color
                .as_ref()
                .is_none_or(|color| color.len() <= 256)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingClipboardItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    pub kind: DrawingKind,
    pub pane_index: usize,
    pub points: Vec<DrawingPoint>,
    pub options: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bars_pattern: Option<Vec<crate::drawings::BarsPatternBar>>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingClipboardPayload {
    pub schema: String,
    pub revision: u64,
    pub drawings: Vec<DrawingClipboardItem>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingSyncPayload {
    pub schema: String,
    pub source: String,
    pub revision: u64,
    pub drawings: Vec<DrawingClipboardItem>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingTemplate {
    pub name: String,
    pub kind: DrawingKind,
    pub options: serde_json::Value,
}

impl DrawingTemplate {
    pub fn validate(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= MAX_DRAWING_NAME_BYTES
            && self.options.is_object()
            && serde_json::to_vec(&self.options)
                .map(|bytes| bytes.len() <= MAX_DRAWING_TEMPLATE_BYTES)
                .unwrap_or(false)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingCommonSnapshot {
    pub id: u32,
    pub kind: DrawingKind,
    pub name: String,
    pub group_id: Option<String>,
    pub revision: u64,
    pub visible: bool,
    pub locked: bool,
    pub z_order: i32,
    pub pane_index: usize,
    pub price_scale: DrawingPriceScale,
    pub magnet: DrawingMagnetMode,
    pub interval_visibility: DrawingIntervalVisibility,
    pub stroke_start: DrawingLineCap,
    pub stroke_end: DrawingLineCap,
    pub extend_left: bool,
    pub extend_right: bool,
    pub fill_enabled: bool,
    pub labels: Vec<DrawingLabelOptions>,
    pub levels: Vec<DrawingLevel>,
}

/// Kind-specific option block projected from the authoritative live drawing.  Hosts can use this
/// typed view instead of switching over the legacy flat options object; the engine intentionally
/// does not store a second copy of these values.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawingKindOptions {
    Rectangle {
        fill_color: Option<String>,
        preview_fill_color: Option<String>,
        border_visible: bool,
        show_labels: bool,
        axis_bands_visible: bool,
        label_color: Option<String>,
        label_text_color: Option<String>,
        snap_time_to_data: bool,
    },
    Text {
        box_color: Option<String>,
        box_border_color: Option<String>,
        box_border_width: f64,
    },
    AnchoredText {
        screen_x: f64,
        screen_y: f64,
        box_color: Option<String>,
        box_border_color: Option<String>,
        box_border_width: f64,
    },
    IconStamp {
        icon_name: Option<String>,
        icon_size: f64,
    },
    ArrowMarker {
        icon_size: f64,
    },
    BarsPattern {
        mirror_x: bool,
        mirror_y: bool,
        mode: String,
        bar_count: usize,
    },
    Position {
        levels: Vec<DrawingLevel>,
        account_size: f64,
        risk_percent: f64,
        profit_color: Option<String>,
        loss_color: Option<String>,
    },
    Levels {
        levels: Vec<DrawingLevel>,
        reverse: bool,
        log_scale: bool,
        show_prices: bool,
        show_values: bool,
        show_percents: bool,
        label_align: String,
        label_v_align: String,
        trend_line: bool,
    },
    GannSquare {
        levels: Vec<DrawingLevel>,
        fans: Vec<DrawingLevel>,
        arcs: Vec<DrawingLevel>,
        reverse: bool,
        show_prices: bool,
        show_values: bool,
        show_percents: bool,
        label_align: String,
    },
    RegressionTrend {
        source_id: Option<u32>,
        deviations: f64,
    },
    Elliott {
        wave_degree: String,
    },
    Generic,
}

const LINE_STYLES: &[&str] = &["solid", "dotted", "dashed"];
const LINE_CAPS: &[&str] = &["none", "arrow", "circle"];
const H_ALIGNS: &[&str] = &["left", "center", "right"];
const V_ALIGNS: &[&str] = &["top", "middle", "bottom"];
/// Statistics-card metrics, in card order (`info_line_rows`).
const STAT_METRICS: &[&str] = &[
    "price_change",
    "percent_change",
    "ticks",
    "bar_count",
    "duration",
    "distance",
    "angle",
];
const WAVE_DEGREES: &[&str] = &[
    "subminuette",
    "minuette",
    "minute",
    "minor",
    "intermediate",
    "primary",
    "cycle",
    "supercycle",
    "grand_supercycle",
];

/// Which text properties a kind paints.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextSupport {
    None,
    /// Only the statistic label ink (positions).
    Color,
    /// Generated vertex labels: ink, size and font style but no content.
    Style,
    /// Level labels inked by their level: size and font style only.
    Font,
    /// Boxed annotation text that is always left-aligned.
    Content,
    /// Free text placed on the 3×3 grid of the kind's box.
    Aligned,
    /// The text tools: aligned text inside an optional styled box.
    Boxed,
}

fn text_support(kind: DrawingKind) -> TextSupport {
    use DrawingKind as K;
    match kind {
        K::LongPosition | K::ShortPosition => TextSupport::Color,
        K::PatternXabcd
        | K::PatternCypher
        | K::PatternAbcd
        | K::PatternHeadShoulders
        | K::PatternTriangle
        | K::PatternThreeDrives => TextSupport::Style,
        kind if kind.is_elliott() => TextSupport::Style,
        kind if kind.has_levels() => TextSupport::Font,
        K::Note | K::Comment | K::Callout | K::PriceNote | K::Signpost | K::PriceLabel => {
            TextSupport::Content
        }
        K::Text | K::AnchoredText => TextSupport::Boxed,
        K::TrendLine
        | K::Ray
        | K::ExtendedLine
        | K::InfoLine
        | K::TrendAngle
        | K::ArrowLine
        | K::HorizontalLine
        | K::HorizontalRay
        | K::VerticalLine
        | K::CrossLine
        | K::Rectangle
        | K::RotatedRectangle
        | K::Ellipse
        | K::Circle
        | K::Triangle
        | K::Arc
        | K::Curve
        | K::DoubleCurve
        | K::ParallelChannel
        | K::FlatTopChannel
        | K::FlatBottomChannel
        | K::DisjointChannel
        | K::RegressionTrend
        | K::Path
        | K::Polyline => TextSupport::Aligned,
        _ => TextSupport::None,
    }
}

fn has_stroke_color(kind: DrawingKind) -> bool {
    !matches!(
        kind,
        DrawingKind::LongPosition
            | DrawingKind::ShortPosition
            | DrawingKind::Text
            | DrawingKind::AnchoredText
            | DrawingKind::FixedRangeVolumeProfile
            | DrawingKind::AnchoredVolumeProfile
    )
}

fn has_line_width(kind: DrawingKind) -> bool {
    has_stroke_color(kind)
        && !kind.has_icon_size()
        && !matches!(
            kind,
            DrawingKind::PriceLabel | DrawingKind::Comment | DrawingKind::Callout
        )
}

fn has_line_style(kind: DrawingKind) -> bool {
    has_line_width(kind) && kind != DrawingKind::DatePriceRange
}

/// Kinds that paint one area fill from `fill_color`.
fn has_area_fill(kind: DrawingKind) -> bool {
    kind.is_measure()
        || matches!(
            kind,
            DrawingKind::ParallelChannel
                | DrawingKind::FlatTopChannel
                | DrawingKind::FlatBottomChannel
                | DrawingKind::DisjointChannel
                | DrawingKind::RegressionTrend
                | DrawingKind::RotatedRectangle
                | DrawingKind::Ellipse
                | DrawingKind::Circle
                | DrawingKind::Triangle
                | DrawingKind::Projection
                | DrawingKind::Rectangle
                | DrawingKind::CyclicLines
                | DrawingKind::TimeCycles
                | DrawingKind::GannBox
                | DrawingKind::GannSquare
                | DrawingKind::GannSquareFixed
        )
}

fn has_line_caps(kind: DrawingKind) -> bool {
    kind.is_straight_line()
        || matches!(
            kind,
            DrawingKind::Forecast
                | DrawingKind::Brush
                | DrawingKind::Highlighter
                | DrawingKind::Path
                | DrawingKind::Polyline
        )
}

fn has_extend(kind: DrawingKind) -> bool {
    matches!(
        kind,
        DrawingKind::TrendLine
            | DrawingKind::Forecast
            | DrawingKind::InfoLine
            | DrawingKind::TrendAngle
            | DrawingKind::ArrowLine
            | DrawingKind::FibonacciRetracement
            | DrawingKind::FibonacciExtension
    )
}

impl DrawingPropertyDescriptor {
    fn on_toolbar(&mut self) -> &mut Self {
        self.toolbar = true;
        self
    }

    fn bounded(&mut self, min: f64, max: f64) -> &mut Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    fn choices(&mut self, values: &[&str]) -> &mut Self {
        self.enum_values = values.iter().map(|value| (*value).to_string()).collect();
        self
    }
}

/// Appends descriptors whose defaults come from the kind's factory options, so the schema can
/// never disagree with what a new drawing actually stores.
struct SchemaBuilder {
    defaults: serde_json::Value,
    properties: Vec<DrawingPropertyDescriptor>,
}

impl SchemaBuilder {
    fn add(
        &mut self,
        group: DrawingPropertyGroup,
        name: &str,
        label: &str,
        property_type: DrawingPropertyType,
    ) -> &mut DrawingPropertyDescriptor {
        let index = self.properties.len();
        self.properties.push(DrawingPropertyDescriptor {
            name: name.to_string(),
            label: label.to_string(),
            group,
            toolbar: false,
            property_type,
            default: self
                .defaults
                .get(name)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            min: None,
            max: None,
            enum_values: Vec::new(),
        });
        &mut self.properties[index]
    }
}

/// The complete editor description of one built-in kind: only properties that change what the
/// kind paints, in display order, with their settings group and toolbar placement. It is data,
/// so one host panel and one toolbar serve every tool without a per-tool switch.
pub fn drawing_property_schema(kind: DrawingKind) -> DrawingPropertySchema {
    use DrawingPropertyGroup::{Coordinates, General, Inputs, Style, Text, Visibility};
    use DrawingPropertyType::{Boolean, Color, Enum, Integer, IntervalSet, Labels, Levels, Number};
    let defaults = crate::drawings::Drawing::new(0, kind, 0, Vec::new());
    let mut schema = SchemaBuilder {
        defaults: defaults.options_json(),
        properties: Vec::new(),
    };
    let s = &mut schema;
    let position = matches!(kind, DrawingKind::LongPosition | DrawingKind::ShortPosition);

    if position {
        s.add(Inputs, "position_account_size", "Account size", Number)
            .bounded(0.01, 1e15);
        s.add(Inputs, "position_risk_percent", "Risk %", Number)
            .bounded(0.0, 100.0);
    }
    if kind == DrawingKind::RegressionTrend {
        s.add(Inputs, "regression_deviations", "Deviation", Number)
            .bounded(0.0, 10.0);
    }

    if has_stroke_color(kind) {
        s.add(Style, "color", "Line", Color).on_toolbar();
    }
    if has_line_width(kind) {
        s.add(Style, "width", "Width", Number)
            .bounded(1.0, crate::drawings::MAX_DRAWING_WIDTH)
            .on_toolbar();
    }
    if has_line_style(kind) {
        s.add(Style, "style", "Line style", Enum)
            .choices(LINE_STYLES)
            .on_toolbar();
    }
    if has_line_caps(kind) {
        s.add(Style, "stroke_start", "Line start", Enum)
            .choices(LINE_CAPS);
        s.add(Style, "stroke_end", "Line end", Enum)
            .choices(LINE_CAPS);
    }
    if has_extend(kind) {
        s.add(Style, "extend_left", "Extend left", Boolean);
        s.add(Style, "extend_right", "Extend right", Boolean);
    }
    if kind.is_straight_line() {
        s.add(Style, "middle_point_visible", "Middle point", Boolean);
        s.add(Style, "price_labels_visible", "Price labels", Boolean);
    }
    if kind.has_stats_card() {
        s.add(Style, "labels", "Stats", Labels)
            .choices(STAT_METRICS);
        s.add(Style, "stats_position", "Stats position", Enum)
            .choices(H_ALIGNS);
        s.add(Style, "stats_always_visible", "Always show stats", Boolean);
    }
    if position {
        s.add(Style, "position_profit_color", "Profit", Color)
            .on_toolbar();
        s.add(Style, "position_loss_color", "Stop", Color)
            .on_toolbar();
    }
    if kind == DrawingKind::Rectangle {
        s.add(Style, "border_visible", "Border", Boolean);
    }
    if has_area_fill(kind) {
        s.add(Style, "fill_enabled", "Background", Boolean);
        s.add(Style, "fill_color", "Background color", Color)
            .on_toolbar();
    } else if kind.has_levels() {
        s.add(Style, "fill_enabled", "Background", Boolean);
    }
    if kind == DrawingKind::Rectangle {
        s.add(Style, "show_labels", "Price and time labels", Boolean);
        s.add(Style, "axis_bands_visible", "Axis highlight", Boolean);
        s.add(Style, "label_color", "Label background", Color);
        s.add(Style, "label_text_color", "Label text", Color);
    }
    if matches!(
        kind,
        DrawingKind::HorizontalLine | DrawingKind::HorizontalRay
    ) {
        s.add(Style, "label_text_color", "Price label text", Color);
    }
    if kind.has_levels() {
        if kind == DrawingKind::FibonacciRetracement {
            s.add(Style, "level_trend_line", "Trend line", Boolean);
        }
        s.add(Style, "levels", "Levels", Levels);
        if matches!(kind, DrawingKind::GannSquare | DrawingKind::GannSquareFixed) {
            s.add(Style, "gann_fans", "Fans", Levels);
            s.add(Style, "gann_arcs", "Arcs", Levels);
        }
        s.add(Style, "level_reverse", "Reverse", Boolean);
        s.add(Style, "level_show_prices", "Prices", Boolean);
        s.add(Style, "level_show_values", "Values", Boolean);
        s.add(Style, "level_show_percents", "Percents", Boolean);
        if kind.supports_log_levels() {
            s.add(
                Style,
                "level_log_scale",
                "Fib levels based on log scale",
                Boolean,
            );
        }
        s.add(Style, "level_label_align", "Labels", Enum)
            .choices(H_ALIGNS);
        if matches!(
            kind,
            DrawingKind::FibonacciRetracement | DrawingKind::FibonacciExtension
        ) {
            s.add(Style, "level_label_v_align", "Labels vertical", Enum)
                .choices(V_ALIGNS);
        }
    }
    if kind.is_elliott() {
        s.add(Style, "wave_degree", "Degree", Enum)
            .choices(WAVE_DEGREES);
    }
    if kind.has_icon_size() {
        s.add(Style, "icon_size", "Size", Number).bounded(
            crate::drawings::MIN_DRAWING_ICON_DISPLAY_SIZE,
            crate::drawings::MAX_DRAWING_ICON_DISPLAY_SIZE,
        );
    }
    if kind == DrawingKind::BarsPattern {
        s.add(Style, "bars_pattern_mode", "Mode", Enum).choices(&[
            "bars",
            "line_open",
            "line_high",
            "line_low",
            "line_close",
        ]);
        s.add(Style, "bars_pattern_mirror_x", "Flip horizontally", Boolean);
        s.add(Style, "bars_pattern_mirror_y", "Flip vertically", Boolean);
    }

    let text = text_support(kind);
    if !matches!(text, TextSupport::None | TextSupport::Font) {
        s.add(Text, "text_color", "Text color", Color).on_toolbar();
    }
    if !matches!(text, TextSupport::None | TextSupport::Color) {
        s.add(Text, "text_size", "Font size", Number)
            .bounded(6.0, 96.0);
        s.add(Text, "text_bold", "Bold", Boolean);
        s.add(Text, "text_italic", "Italic", Boolean);
    }
    if matches!(
        text,
        TextSupport::Content | TextSupport::Aligned | TextSupport::Boxed
    ) {
        s.add(Text, "text", "Text", DrawingPropertyType::String);
    }
    if matches!(text, TextSupport::Aligned | TextSupport::Boxed) {
        s.add(Text, "text_v_align", "Vertical alignment", Enum)
            .choices(V_ALIGNS);
        s.add(Text, "text_h_align", "Horizontal alignment", Enum)
            .choices(H_ALIGNS);
    }
    if text == TextSupport::Boxed || kind.is_annotation() {
        s.add(Text, "box_color", "Background", Color);
        s.add(Text, "box_border_color", "Border", Color);
        if kind != DrawingKind::Callout {
            s.add(Text, "box_border_width", "Border width", Number)
                .bounded(1.0, 8.0);
        }
    }

    if kind == DrawingKind::AnchoredText {
        s.add(Coordinates, "screen_x", "Horizontal position", Number)
            .bounded(0.0, 1.0);
        s.add(Coordinates, "screen_y", "Vertical position", Number)
            .bounded(0.0, 1.0);
    }
    s.add(Visibility, "interval_visibility", "Intervals", IntervalSet);

    s.add(General, "name", "Name", DrawingPropertyType::String);
    s.add(General, "visible", "Visible", Boolean);
    s.add(General, "locked", "Locked", Boolean);
    s.add(General, "group_id", "Group", DrawingPropertyType::String);
    s.add(General, "z_order", "Order", Integer);
    s.add(General, "magnet", "Magnet", Enum)
        .choices(&["off", "weak", "strong"]);
    s.add(General, "price_scale_id", "Price scale", Enum)
        .choices(&["left", "right", "overlay"]);
    if kind == DrawingKind::RegressionTrend {
        s.add(General, "regression_source_id", "Source", Integer);
    }
    if kind == DrawingKind::IconStamp {
        s.add(General, "icon_name", "Icon", DrawingPropertyType::String);
    }

    let anchors = kind
        .anchor_axes()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, (price, time))| DrawingAnchorDescriptor {
            index,
            label: match (kind, index) {
                (DrawingKind::LongPosition | DrawingKind::ShortPosition, 0) => "Entry".to_string(),
                (DrawingKind::LongPosition | DrawingKind::ShortPosition, 1) => "Target".to_string(),
                (DrawingKind::LongPosition | DrawingKind::ShortPosition, _) => "Stop".to_string(),
                _ if kind.anchor_count() == 1 => "Point".to_string(),
                _ => format!("Point {}", index + 1),
            },
            price,
            time,
        })
        .collect();
    DrawingPropertySchema {
        revision: DRAWING_CONTRACT_REVISION,
        kind,
        properties: schema.properties,
        anchors,
    }
}
