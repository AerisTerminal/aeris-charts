//! Typed drawing customization contracts shared by native and browser hosts.
//!
//! The engine keeps the live [`Drawing`](crate::Drawing) representation compact for the hot
//! render and hit-test paths.  This module is the stable, typed view used by property panels,
//! templates, persistence adapters, and cross-cell synchronization.  Values are deliberately
//! bounded and deterministic so a host cannot turn a drawing patch into unbounded work.

use crate::{DrawingKind, DrawingPoint, DrawingPriceScale};

pub const DRAWING_CONTRACT_REVISION: u32 = 1;
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
    Points,
    Levels,
    IntervalSet,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingPropertyDescriptor {
    pub name: String,
    pub property_type: DrawingPropertyType,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingPropertySchema {
    pub revision: u32,
    pub kind: DrawingKind,
    pub properties: Vec<DrawingPropertyDescriptor>,
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

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingIntervalVisibility {
    pub enabled: bool,
    #[serde(default)]
    pub intervals: Vec<DrawingInterval>,
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
        })
    }

    pub fn validate(&self) -> bool {
        self.intervals.len() <= 16 && self.intervals.iter().all(|interval| interval.validate())
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
    Position {
        levels: Vec<DrawingLevel>,
    },
    Generic,
}

fn descriptor(
    name: &str,
    property_type: DrawingPropertyType,
    default: serde_json::Value,
) -> DrawingPropertyDescriptor {
    DrawingPropertyDescriptor {
        name: name.to_string(),
        property_type,
        default,
        min: None,
        max: None,
        enum_values: Vec::new(),
    }
}

/// Return the complete generic property schema for one built-in drawing kind.  The schema is
/// data, so a host can build a property panel without a tool-specific switch statement.
pub fn drawing_property_schema(kind: DrawingKind) -> DrawingPropertySchema {
    let mut properties = vec![
        descriptor("name", DrawingPropertyType::String, serde_json::json!("")),
        descriptor(
            "visible",
            DrawingPropertyType::Boolean,
            serde_json::json!(true),
        ),
        descriptor(
            "locked",
            DrawingPropertyType::Boolean,
            serde_json::json!(false),
        ),
        descriptor(
            "group_id",
            DrawingPropertyType::String,
            serde_json::json!(""),
        ),
        descriptor(
            "z_order",
            DrawingPropertyType::Integer,
            serde_json::json!(0),
        ),
        descriptor(
            "interval_visibility",
            DrawingPropertyType::IntervalSet,
            serde_json::json!({}),
        ),
        descriptor(
            "color",
            DrawingPropertyType::Color,
            serde_json::json!("#2962ff"),
        ),
        descriptor("width", DrawingPropertyType::Number, serde_json::json!(2.0)),
        descriptor(
            "style",
            DrawingPropertyType::Enum,
            serde_json::json!("solid"),
        ),
        descriptor(
            "stroke_start",
            DrawingPropertyType::Enum,
            serde_json::json!("none"),
        ),
        descriptor(
            "stroke_end",
            DrawingPropertyType::Enum,
            serde_json::json!("none"),
        ),
        descriptor(
            "extend_left",
            DrawingPropertyType::Boolean,
            serde_json::json!(false),
        ),
        descriptor(
            "extend_right",
            DrawingPropertyType::Boolean,
            serde_json::json!(false),
        ),
        descriptor(
            "fill_enabled",
            DrawingPropertyType::Boolean,
            serde_json::json!(false),
        ),
        descriptor(
            "fill_color",
            DrawingPropertyType::Color,
            serde_json::json!(""),
        ),
        descriptor("text", DrawingPropertyType::String, serde_json::json!("")),
        descriptor(
            "text_color",
            DrawingPropertyType::Color,
            serde_json::json!(""),
        ),
        descriptor(
            "text_size",
            DrawingPropertyType::Number,
            serde_json::Value::Null,
        ),
        descriptor(
            "text_weight",
            DrawingPropertyType::Integer,
            serde_json::json!(400),
        ),
        descriptor(
            "text_italic",
            DrawingPropertyType::Boolean,
            serde_json::json!(false),
        ),
        descriptor(
            "text_h_align",
            DrawingPropertyType::Enum,
            serde_json::json!("center"),
        ),
        descriptor(
            "text_v_align",
            DrawingPropertyType::Enum,
            serde_json::json!("middle"),
        ),
        descriptor("labels", DrawingPropertyType::Levels, serde_json::json!([])),
        descriptor("levels", DrawingPropertyType::Levels, serde_json::json!([])),
        descriptor(
            "magnet",
            DrawingPropertyType::Enum,
            serde_json::json!("off"),
        ),
        descriptor("points", DrawingPropertyType::Points, serde_json::json!([])),
        descriptor(
            "price_scale_id",
            DrawingPropertyType::Enum,
            serde_json::json!("right"),
        ),
    ];
    for property in &mut properties {
        if property.name == "style" {
            property.enum_values = ["solid", "dotted", "dashed", "large_dashed", "sparse_dotted"]
                .into_iter()
                .map(str::to_string)
                .collect();
        } else if matches!(property.name.as_str(), "stroke_start" | "stroke_end") {
            property.enum_values = ["none", "arrow", "circle"]
                .into_iter()
                .map(str::to_string)
                .collect();
        } else if property.name == "magnet" {
            property.enum_values = ["off", "weak", "strong"]
                .into_iter()
                .map(str::to_string)
                .collect();
        } else if property.name == "price_scale_id" {
            property.enum_values = ["left", "right", "overlay"]
                .into_iter()
                .map(str::to_string)
                .collect();
        }
    }
    DrawingPropertySchema {
        revision: DRAWING_CONTRACT_REVISION,
        kind,
        properties,
    }
}
