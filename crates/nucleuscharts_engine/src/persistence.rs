//! Versioned persistence for stable semantic chart state.
//!
//! V1 deliberately contains pane topology and built-in drawings only. Market history, series and
//! indicator definitions, custom extensions, runtime caches, retained frames, and renderer state
//! remain host-owned or derived.

use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use nucleuscharts_core::model::data_validation::MAX_SAFE_VALUE;
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::LineStyle;

use crate::drawings::{DrawingPriceScale, DrawingTextHAlign, DrawingTextVAlign};
use crate::{ChartEngine, ChartError, Drawing, DrawingKind, DrawingPoint, ErrorCode, Pane, PaneId};

pub const PERSISTENCE_SCHEMA_VERSION: u32 = 1;
pub const PERSISTENCE_SCHEMA_VERSION_GENERAL: u32 = 2;
pub const PERSISTENCE_MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
pub const PERSISTENCE_MAX_GENERAL_DOCUMENT_BYTES: usize = 32 * 1024 * 1024;
pub const PERSISTENCE_MAX_PANES: usize = 64;
pub const PERSISTENCE_MAX_DRAWINGS: usize = 10_000;
pub const PERSISTENCE_MAX_POINTS_PER_DRAWING: usize = crate::drawings::MAX_DRAWING_POINTS;
pub const PERSISTENCE_MAX_TOTAL_POINTS: usize = 250_000;
const MAX_TEXT_BYTES: usize = 65_536;
const MAX_TOTAL_TEXT_BYTES: usize = 1_048_576;
const MAX_COLOR_BYTES: usize = 256;
const MAX_STYLE_NUMBER: f64 = 1_000.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistenceRestoreResult {
    pub schema_version: u32,
    pub panes: usize,
    pub drawings: usize,
    pub points: usize,
}

/// Release-benchmark evidence for the bounded restore stages. This is not a stable product API.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub struct PersistenceRestoreProfile {
    pub restore: PersistenceRestoreResult,
    pub parse_ns: u64,
    pub validation_ns: u64,
    pub semantic_install_ns: u64,
    pub index_rebuild_ns: u64,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct InstallProfile {
    semantic_install_ns: u64,
    index_rebuild_ns: u64,
}

/// Fully parsed and validated V1 state. Fields are private so installation cannot bypass
/// validation. This split is also the release benchmark seam for parse/validation vs install.
#[derive(Debug)]
#[doc(hidden)]
pub struct ValidatedStateV1 {
    panes: Vec<ValidatedPane>,
    drawings: Vec<Drawing>,
    max_drawing_id: u32,
    max_persistent_pane_id: u32,
    points: usize,
}

#[derive(Debug)]
struct ValidatedPane {
    persistent_id: u32,
    stretch_factor: f64,
    preserve_empty: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StateV1 {
    schema: String,
    schema_version: u32,
    panes: Vec<PaneV1>,
    drawings: Vec<DrawingV1>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StateV2 {
    schema: String,
    schema_version: u32,
    panes: Vec<PaneV2>,
    drawings: Vec<DrawingV1>,
    axes: Vec<crate::GeneralAxisOptions>,
    datasets: Vec<DatasetV2>,
    series: Vec<SeriesV2>,
    chart_options: serde_json::Value,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PaneV2 {
    #[serde(flatten)]
    pane: PaneV1,
    horizontal_domain: crate::HorizontalDomain,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DatasetV2 {
    id: String,
    input: crate::GeneralXyInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    labels: Option<Vec<Option<String>>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SeriesV2 {
    kind: crate::GeneralSeriesKind,
    pane: usize,
    dataset: String,
    x_axis_id: String,
    y_axis_id: String,
    visible: bool,
    title: String,
    color: Option<String>,
    point_radius: f64,
    data_labels: bool,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    stack_id: Option<String>,
    #[serde(default)]
    stack_mode: crate::GeneralStackMode,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PaneV1 {
    id: String,
    #[serde(default = "default_stretch")]
    stretch_factor: f64,
    #[serde(default)]
    preserve_empty: bool,
}

fn default_stretch() -> f64 {
    1.0
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DrawingV1 {
    id: u32,
    kind: String,
    pane_id: String,
    anchors: Vec<DrawingPoint>,
    #[serde(default)]
    style: DrawingStyleV1,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct DrawingStyleV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    price_scale_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview_fill_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    border_visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_labels: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    axis_bands_visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label_text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snap_time_to_data: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_weight: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_h_align: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_v_align: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    box_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    box_border_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    box_border_width: Option<f64>,
}

fn pane_wire_id(id: u32) -> String {
    format!("pane-{id}")
}

fn parse_pane_wire_id(value: &str) -> Option<u32> {
    value
        .strip_prefix("pane-")?
        .parse::<u32>()
        .ok()
        .filter(|&id| id != 0)
}

fn line_style_name(style: LineStyle) -> &'static str {
    match style {
        LineStyle::Solid => "solid",
        LineStyle::Dotted => "dotted",
        LineStyle::Dashed => "dashed",
    }
}

fn parse_line_style(value: &str) -> Option<LineStyle> {
    Some(match value {
        "solid" => LineStyle::Solid,
        "dotted" => LineStyle::Dotted,
        "dashed" => LineStyle::Dashed,
        _ => return None,
    })
}

fn invalid(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

fn validate_color(value: &str, field: &str) -> Result<(), ChartError> {
    if value.len() > MAX_COLOR_BYTES {
        return Err(resource(format!("{field} exceeds {MAX_COLOR_BYTES} bytes")));
    }
    if !value.is_empty() && Color::parse_css(value).is_none() {
        return Err(invalid(format!("{field} is not a supported CSS color")));
    }
    Ok(())
}

fn validate_positive_number(value: f64, field: &str) -> Result<(), ChartError> {
    if !value.is_finite() || value <= 0.0 || value > MAX_STYLE_NUMBER {
        return Err(invalid(format!(
            "{field} must be finite and in (0, {MAX_STYLE_NUMBER}]"
        )));
    }
    Ok(())
}

impl ChartEngine {
    /// Deterministic JSON of stable chart state. Financial-only charts retain the V1 wire format.
    pub fn export_state_json(&self) -> Result<String, ChartError> {
        if self
            .panes
            .iter()
            .any(|pane| pane.general_horizontal_domain.is_some())
            || self.general_dataset_count() != 0
            || self.general_series_count() != 0
            || !self.general_axes(None).is_empty()
        {
            return self.export_state_v2_json();
        }
        self.export_state_v1_json()
    }

    fn export_state_v1_json(&self) -> Result<String, ChartError> {
        let panes = self
            .panes
            .iter()
            .map(|pane| {
                let id = pane.persistent_id().ok_or_else(|| {
                    ChartError::new(
                        ErrorCode::SerializationError,
                        "chart pane has no persistence id",
                    )
                })?;
                Ok(PaneV1 {
                    id: pane_wire_id(id),
                    stretch_factor: pane.stretch_factor,
                    preserve_empty: pane.preserve_empty,
                })
            })
            .collect::<Result<Vec<_>, ChartError>>()?;
        let drawings = self
            .drawings
            .iter()
            .map(|drawing| {
                let pane = self.panes.get(drawing.pane_index).ok_or_else(|| {
                    ChartError::new(
                        ErrorCode::SerializationError,
                        format!("drawing {} is not attached to a live pane", drawing.id),
                    )
                })?;
                let persistent_id = pane.persistent_id().ok_or_else(|| {
                    ChartError::new(
                        ErrorCode::SerializationError,
                        "drawing pane has no persistence id",
                    )
                })?;
                Ok(DrawingV1 {
                    id: drawing.id,
                    kind: drawing.kind.name().to_string(),
                    pane_id: pane_wire_id(persistent_id),
                    anchors: drawing.points.clone(),
                    style: DrawingStyleV1 {
                        price_scale_id: (drawing.price_scale != DrawingPriceScale::Right)
                            .then(|| drawing.price_scale.name().to_string()),
                        color: Some(drawing.color.clone()),
                        width: Some(drawing.width),
                        line_style: Some(line_style_name(drawing.style).to_string()),
                        fill_color: drawing.fill_color.clone(),
                        preview_fill_color: drawing.preview_fill_color.clone(),
                        border_visible: (!drawing.border_visible).then_some(false),
                        show_labels: drawing.show_labels.then_some(true),
                        axis_bands_visible: drawing.axis_bands_visible.then_some(true),
                        label_color: drawing.label_color.clone(),
                        label_text_color: drawing.label_text_color.clone(),
                        snap_time_to_data: drawing.snap_time_to_data.then_some(true),
                        text: (!drawing.text.is_empty()).then(|| drawing.text.clone()),
                        text_color: drawing.text_color.clone(),
                        text_size: drawing.text_size,
                        text_weight: drawing.text_weight,
                        text_italic: drawing.text_italic.then_some(true),
                        text_h_align: Some(drawing.text_h_align.name().to_string()),
                        text_v_align: Some(drawing.text_v_align.name().to_string()),
                        box_color: drawing.box_color.clone(),
                        box_border_color: drawing.box_border_color.clone(),
                        box_border_width: Some(drawing.box_border_width),
                    },
                })
            })
            .collect::<Result<Vec<_>, ChartError>>()?;
        serde_json::to_string(&StateV1 {
            schema: "nucleuscharts-state".to_string(),
            schema_version: PERSISTENCE_SCHEMA_VERSION,
            panes,
            drawings,
        })
        .map_err(|error| ChartError::new(ErrorCode::SerializationError, error.to_string()))
    }

    fn export_state_v2_json(&self) -> Result<String, ChartError> {
        let base: StateV1 = serde_json::from_str(&self.export_state_v1_json()?)
            .map_err(|error| ChartError::new(ErrorCode::SerializationError, error.to_string()))?;
        let panes = base
            .panes
            .into_iter()
            .enumerate()
            .map(|(index, pane)| {
                Ok(PaneV2 {
                    pane,
                    horizontal_domain: self
                        .pane_horizontal_domain(index)
                        .ok_or_else(|| invalid(format!("pane {index} has no horizontal domain")))?,
                })
            })
            .collect::<Result<Vec<_>, ChartError>>()?;
        let axes = self
            .general_axes
            .iter()
            .map(|axis| {
                let pane = self
                    .pane_index_for_id(axis.pane_id())
                    .ok_or_else(|| invalid(format!("axis {:?} has no live pane", axis.id())))?;
                Ok(crate::GeneralAxisOptions {
                    id: axis.id().to_string(),
                    pane,
                    dimension: axis.dimension(),
                    position: axis.position(),
                    scale: axis.scale(),
                    domain: axis.domain().clone(),
                    reverse: axis.reverse(),
                    visible: axis.visible(),
                    title: axis.title().map(str::to_string),
                    tick_count: axis.tick_count(),
                    min_tick_gap: axis.min_tick_gap(),
                    band_padding_inner: axis.band_padding_inner(),
                    band_padding_outer: axis.band_padding_outer(),
                    zero_line: axis.zero_line(),
                    grid_visible: axis.grid_visible(),
                })
            })
            .collect::<Result<Vec<_>, ChartError>>()?;
        let mut dataset_ids = HashMap::new();
        let datasets = self
            .general_data
            .iter()
            .flat_map(|store| store.iter())
            .enumerate()
            .map(|(index, dataset)| {
                let id = format!("dataset-{}", index + 1);
                dataset_ids.insert(dataset.id(), id.clone());
                let ids = (0..dataset.len())
                    .map(|row| match dataset.row_identity(row) {
                        Some(crate::GeneralRowIdentity::Explicit(id)) => id.clone(),
                        _ => crate::GeneralRowId::Generated,
                    })
                    .collect::<Vec<_>>();
                let ids = ids
                    .iter()
                    .any(|id| !matches!(id, crate::GeneralRowId::Generated))
                    .then_some(ids);
                let y = dataset.y().to_vec();
                let y_valid = (0..dataset.len())
                    .any(|row| !dataset.y_is_valid(row))
                    .then(|| {
                        (0..dataset.len())
                            .map(|row| u8::from(dataset.y_is_valid(row)))
                            .collect()
                    });
                let size = dataset.size().map(ToOwned::to_owned);
                let size_valid = size.as_ref().and_then(|_| {
                    (0..dataset.len())
                        .any(|row| !dataset.size_is_valid(row))
                        .then(|| {
                            (0..dataset.len())
                                .map(|row| u8::from(dataset.size_is_valid(row)))
                                .collect()
                        })
                });
                let low = dataset.low().map(ToOwned::to_owned);
                let low_valid = low.as_ref().and_then(|_| {
                    (0..dataset.len())
                        .any(|row| !dataset.low_is_valid(row))
                        .then(|| {
                            (0..dataset.len())
                                .map(|row| u8::from(dataset.low_is_valid(row)))
                                .collect()
                        })
                });
                debug_assert!(
                    low.is_none() || size.is_none(),
                    "range and bubble channels are mutually exclusive"
                );
                let input = match dataset.x_kind() {
                    crate::GeneralXKind::Numeric => match (low, size) {
                        (Some(low), None) => crate::GeneralXyInput::RangeNumeric {
                            ids,
                            x: dataset.numeric_x().unwrap_or_default().to_vec(),
                            low,
                            low_valid,
                            high: y,
                            high_valid: y_valid,
                        },
                        (None, Some(size)) => crate::GeneralXyInput::Bubble {
                            ids,
                            x: dataset.numeric_x().unwrap_or_default().to_vec(),
                            y,
                            y_valid,
                            size,
                            size_valid,
                        },
                        (None, None) => crate::GeneralXyInput::Numeric {
                            ids,
                            x: dataset.numeric_x().unwrap_or_default().to_vec(),
                            y,
                            y_valid,
                        },
                        (Some(low), Some(_)) => crate::GeneralXyInput::RangeNumeric {
                            ids,
                            x: dataset.numeric_x().unwrap_or_default().to_vec(),
                            low,
                            low_valid,
                            high: y,
                            high_valid: y_valid,
                        },
                    },
                    crate::GeneralXKind::Temporal => match low {
                        Some(low) => crate::GeneralXyInput::RangeTemporal {
                            ids,
                            x_epoch_ms: dataset.temporal_x_epoch_ms().unwrap_or_default().to_vec(),
                            low,
                            low_valid,
                            high: y,
                            high_valid: y_valid,
                        },
                        None => crate::GeneralXyInput::Temporal {
                            ids,
                            x_epoch_ms: dataset.temporal_x_epoch_ms().unwrap_or_default().to_vec(),
                            y,
                            y_valid,
                        },
                    },
                    crate::GeneralXKind::Category => match low {
                        Some(low) => crate::GeneralXyInput::RangeCategory {
                            ids,
                            categories: dataset.categories().unwrap_or_default().to_vec(),
                            category_indices: dataset
                                .category_indices()
                                .unwrap_or_default()
                                .to_vec(),
                            low,
                            low_valid,
                            high: y,
                            high_valid: y_valid,
                        },
                        None => crate::GeneralXyInput::Category {
                            ids,
                            categories: dataset.categories().unwrap_or_default().to_vec(),
                            category_indices: dataset
                                .category_indices()
                                .unwrap_or_default()
                                .to_vec(),
                            y,
                            y_valid,
                        },
                    },
                };
                let labels = (0..dataset.len())
                    .map(|row| dataset.row_label(row).map(str::to_string))
                    .collect::<Vec<_>>();
                let labels = labels.iter().any(Option::is_some).then_some(labels);
                DatasetV2 { id, input, labels }
            })
            .collect::<Vec<_>>();
        let series = self
            .general_series_iter()
            .map(|series| {
                Ok(SeriesV2 {
                    kind: series.kind(),
                    pane: self.pane_index_for_id(series.pane_id()).ok_or_else(|| {
                        invalid(format!("series {} has no live pane", series.id().get()))
                    })?,
                    dataset: dataset_ids.get(&series.dataset()).cloned().ok_or_else(|| {
                        invalid(format!("series {} has no live dataset", series.id().get()))
                    })?,
                    x_axis_id: series.x_axis_id().to_string(),
                    y_axis_id: series.y_axis_id().to_string(),
                    visible: series.visible(),
                    title: series.title().to_string(),
                    color: series.color().map(str::to_string),
                    point_radius: series.point_radius(),
                    data_labels: series.data_labels(),
                    group_id: series.group_id().map(str::to_string),
                    stack_id: series.stack_id().map(str::to_string),
                    stack_mode: series.stack_mode(),
                })
            })
            .collect::<Result<Vec<_>, ChartError>>()?;
        let document = serde_json::to_string(&StateV2 {
            schema: base.schema,
            schema_version: PERSISTENCE_SCHEMA_VERSION_GENERAL,
            panes,
            drawings: base.drawings,
            axes,
            datasets,
            series,
            chart_options: self.options.value().clone(),
        })
        .map_err(|error| ChartError::new(ErrorCode::SerializationError, error.to_string()))?;
        if document.len() > PERSISTENCE_MAX_GENERAL_DOCUMENT_BYTES {
            return Err(resource("V2 persistence document exceeds the size limit"));
        }
        Ok(document)
    }

    /// Parse and validate untrusted state without mutating the chart.
    #[doc(hidden)]
    pub fn validate_state_json(json: &str) -> Result<ValidatedStateV1, ChartError> {
        Self::validate_state_v1(Self::parse_state_json(json)?)
    }

    fn parse_state_json(json: &str) -> Result<StateV1, ChartError> {
        if json.len() > PERSISTENCE_MAX_DOCUMENT_BYTES {
            return Err(resource(format!(
                "persistence document exceeds {PERSISTENCE_MAX_DOCUMENT_BYTES} bytes"
            )));
        }
        let envelope: serde_json::Value = serde_json::from_str(json).map_err(|error| {
            ChartError::new(
                ErrorCode::SerializationError,
                format!("malformed JSON: {error}"),
            )
        })?;
        let object = envelope.as_object().ok_or_else(|| {
            ChartError::new(
                ErrorCode::SerializationError,
                "persistence document must be an object",
            )
        })?;
        let schema = object.get("schema").and_then(serde_json::Value::as_str);
        if schema != Some("nucleuscharts-state") {
            return Err(ChartError::new(
                ErrorCode::SerializationError,
                "unsupported or missing persistence schema",
            ));
        }
        let version = object
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                ChartError::new(
                    ErrorCode::SerializationError,
                    "schema_version must be an integer",
                )
            })?;
        if version != u64::from(PERSISTENCE_SCHEMA_VERSION) {
            return Err(ChartError::new(
                ErrorCode::PersistenceVersionError,
                format!("unsupported persistence schema version {version}"),
            ));
        }
        serde_json::from_value(envelope).map_err(|error| {
            ChartError::new(
                ErrorCode::SerializationError,
                format!("invalid V1 document: {error}"),
            )
        })
    }

    fn validate_state_v1(state: StateV1) -> Result<ValidatedStateV1, ChartError> {
        if state.panes.is_empty() {
            return Err(invalid("V1 requires at least one pane"));
        }
        if state.panes.len() > PERSISTENCE_MAX_PANES {
            return Err(resource(format!(
                "V1 supports at most {PERSISTENCE_MAX_PANES} panes"
            )));
        }
        if state.drawings.len() > PERSISTENCE_MAX_DRAWINGS {
            return Err(resource(format!(
                "V1 supports at most {PERSISTENCE_MAX_DRAWINGS} drawings"
            )));
        }

        let mut pane_ids = HashSet::with_capacity(state.panes.len());
        let mut panes = Vec::with_capacity(state.panes.len());
        let mut max_persistent_pane_id = 0;
        for pane in state.panes {
            let id = parse_pane_wire_id(&pane.id)
                .ok_or_else(|| invalid(format!("invalid pane id {:?}", pane.id)))?;
            if !pane_ids.insert(id) {
                return Err(invalid(format!("duplicate pane id {:?}", pane.id)));
            }
            if !pane.stretch_factor.is_finite()
                || pane.stretch_factor <= 0.0
                || pane.stretch_factor > 1_000_000.0
            {
                return Err(invalid(format!(
                    "pane {:?} has invalid stretch_factor",
                    pane.id
                )));
            }
            max_persistent_pane_id = max_persistent_pane_id.max(id);
            panes.push(ValidatedPane {
                persistent_id: id,
                stretch_factor: pane.stretch_factor,
                preserve_empty: pane.preserve_empty,
            });
        }
        if max_persistent_pane_id == u32::MAX {
            return Err(resource("pane persistence identity space is exhausted"));
        }

        let pane_positions = panes
            .iter()
            .enumerate()
            .map(|(index, pane)| (pane.persistent_id, index))
            .collect::<std::collections::HashMap<_, _>>();
        let mut drawing_ids = HashSet::with_capacity(state.drawings.len());
        let mut drawings = Vec::with_capacity(state.drawings.len());
        let mut max_drawing_id = 0;
        let mut total_points = 0usize;
        let mut total_text = 0usize;
        for item in state.drawings {
            if item.id == 0 || item.id == u32::MAX || !drawing_ids.insert(item.id) {
                return Err(invalid(format!(
                    "drawing id {} is invalid or duplicated",
                    item.id
                )));
            }
            let kind = DrawingKind::from_name(&item.kind)
                .ok_or_else(|| invalid(format!("unknown drawing kind {:?}", item.kind)))?;
            if item.anchors.len() > PERSISTENCE_MAX_POINTS_PER_DRAWING {
                return Err(resource(format!(
                    "drawing {} exceeds {PERSISTENCE_MAX_POINTS_PER_DRAWING} anchors",
                    item.id
                )));
            }
            total_points = total_points
                .checked_add(item.anchors.len())
                .ok_or_else(|| resource("drawing anchor count overflow"))?;
            if total_points > PERSISTENCE_MAX_TOTAL_POINTS {
                return Err(resource(format!(
                    "V1 supports at most {PERSISTENCE_MAX_TOTAL_POINTS} total anchors"
                )));
            }
            if !kind.valid_point_count(item.anchors.len()) {
                return Err(invalid(format!(
                    "drawing {} has an invalid anchor count",
                    item.id
                )));
            }
            if item.anchors.iter().any(|point| {
                !point.logical.is_finite()
                    || !point.price.is_finite()
                    || point.logical.abs() > MAX_SAFE_VALUE
                    || point.price.abs() > MAX_SAFE_VALUE
            }) {
                return Err(invalid(format!(
                    "drawing {} has an invalid anchor",
                    item.id
                )));
            }
            let pane_id = parse_pane_wire_id(&item.pane_id).ok_or_else(|| {
                invalid(format!("drawing {} has an invalid pane reference", item.id))
            })?;
            let pane_index = pane_positions.get(&pane_id).copied().ok_or_else(|| {
                invalid(format!(
                    "drawing {} references unknown pane {:?}",
                    item.id, item.pane_id
                ))
            })?;
            let mut drawing = Drawing::new(item.id, kind, pane_index, item.anchors);
            let style = item.style;
            if let Some(scale) = style.price_scale_id {
                drawing.price_scale = DrawingPriceScale::from_name(&scale)
                    .ok_or_else(|| invalid(format!("unknown drawing price scale {scale:?}")))?;
            }
            if let Some(color) = style.color {
                validate_color(&color, "drawing color")?;
                if !color.is_empty() {
                    drawing.color = color;
                }
            }
            if let Some(width) = style.width {
                validate_positive_number(width, "drawing width")?;
                drawing.width = width;
            }
            if let Some(value) = style.line_style {
                drawing.style = parse_line_style(&value)
                    .ok_or_else(|| invalid(format!("unknown line style {value:?}")))?;
            }
            if let Some(color) = style.fill_color {
                validate_color(&color, "fill_color")?;
                drawing.fill_color = (!color.is_empty()).then_some(color);
            }
            if let Some(color) = style.preview_fill_color {
                validate_color(&color, "preview_fill_color")?;
                drawing.preview_fill_color = (!color.is_empty()).then_some(color);
            }
            if let Some(visible) = style.border_visible {
                drawing.border_visible = visible;
            }
            if let Some(visible) = style.show_labels {
                drawing.show_labels = visible;
            }
            if let Some(visible) = style.axis_bands_visible {
                drawing.axis_bands_visible = visible;
            }
            if let Some(color) = style.label_color {
                validate_color(&color, "label_color")?;
                drawing.label_color = (!color.is_empty()).then_some(color);
            }
            if let Some(color) = style.label_text_color {
                validate_color(&color, "label_text_color")?;
                drawing.label_text_color = (!color.is_empty()).then_some(color);
            }
            if let Some(snap) = style.snap_time_to_data {
                drawing.snap_time_to_data = snap;
            }
            if let Some(text) = style.text {
                if text.len() > MAX_TEXT_BYTES {
                    return Err(resource(format!("drawing {} text is too large", item.id)));
                }
                total_text = total_text
                    .checked_add(text.len())
                    .ok_or_else(|| resource("drawing text size overflow"))?;
                if total_text > MAX_TOTAL_TEXT_BYTES {
                    return Err(resource("drawing text exceeds the document limit"));
                }
                drawing.text = text;
            }
            if let Some(color) = style.text_color {
                validate_color(&color, "text_color")?;
                drawing.text_color = (!color.is_empty()).then_some(color);
            }
            if let Some(size) = style.text_size {
                validate_positive_number(size, "text_size")?;
                drawing.text_size = Some(size);
            }
            if let Some(weight) = style.text_weight {
                if !(100..=900).contains(&weight) {
                    return Err(invalid("text_weight must be in 100..=900"));
                }
                drawing.text_weight = Some(weight);
            }
            if let Some(italic) = style.text_italic {
                drawing.text_italic = italic;
            }
            if let Some(align) = style.text_h_align {
                drawing.text_h_align = DrawingTextHAlign::from_name(&align).ok_or_else(|| {
                    invalid(format!("unknown horizontal text alignment {align:?}"))
                })?;
            }
            if let Some(align) = style.text_v_align {
                drawing.text_v_align = DrawingTextVAlign::from_name(&align)
                    .ok_or_else(|| invalid(format!("unknown vertical text alignment {align:?}")))?;
            }
            if let Some(color) = style.box_color {
                validate_color(&color, "box_color")?;
                drawing.box_color = (!color.is_empty()).then_some(color);
            }
            if let Some(color) = style.box_border_color {
                validate_color(&color, "box_border_color")?;
                drawing.box_border_color = (!color.is_empty()).then_some(color);
            }
            if let Some(width) = style.box_border_width {
                validate_positive_number(width, "box_border_width")?;
                drawing.box_border_width = width;
            }
            max_drawing_id = max_drawing_id.max(item.id);
            drawings.push(drawing);
        }
        Ok(ValidatedStateV1 {
            panes,
            drawings,
            max_drawing_id,
            max_persistent_pane_id,
            points: total_points,
        })
    }

    /// Install already-validated state as one logical transaction.
    #[doc(hidden)]
    pub fn install_validated_state(
        &mut self,
        state: ValidatedStateV1,
    ) -> Result<PersistenceRestoreResult, ChartError> {
        #[cfg(target_arch = "wasm32")]
        return self.install_validated_state_inner(state);
        #[cfg(not(target_arch = "wasm32"))]
        self.install_validated_state_inner(state, None)
    }

    fn install_validated_state_inner(
        &mut self,
        state: ValidatedStateV1,
        #[cfg(not(target_arch = "wasm32"))] mut profile: Option<&mut InstallProfile>,
    ) -> Result<PersistenceRestoreResult, ChartError> {
        if self.panes.len() != 1
            || !self.drawings.is_empty()
            || self.next_drawing_id != 1
            || self.general_dataset_count() != 0
            || self.general_series_count() != 0
        {
            return Err(ChartError::new(
                ErrorCode::UnsupportedOperation,
                "state import requires a fresh chart before drawing handles have been issued",
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let semantic_started = profile.as_ref().map(|_| Instant::now());
        let pane_count = u32::try_from(state.panes.len())
            .map_err(|_| resource("pane count cannot be represented"))?;
        let next_runtime = self
            .next_pane_id
            .checked_add(pane_count)
            .ok_or_else(|| resource("pane handle identity space is exhausted"))?;
        let mut panes = Vec::with_capacity(state.panes.len());
        for (offset, persisted) in state.panes.iter().enumerate() {
            let offset = u32::try_from(offset).map_err(|_| resource("pane index overflow"))?;
            let runtime_id = PaneId::try_from(self.next_pane_id + offset)?;
            let mut pane = Pane::with_chart_ids(runtime_id, persisted.persistent_id);
            pane.stretch_factor = persisted.stretch_factor;
            pane.preserve_empty = persisted.preserve_empty;
            self.apply_chart_scale_options(&mut pane);
            panes.push(pane);
        }

        let drawing_count = state.drawings.len();
        let points = state.points;
        self.panes = panes;
        self.general_horizontal_domains = crate::domains::HorizontalDomainRegistry::new();
        self.general_axes = crate::general_axes::GeneralAxisRegistry::new();
        self.general_data = None;
        self.general_series = None;
        self.drawings = state.drawings;
        self.next_pane_id = next_runtime;
        self.next_persistent_pane_id = state.max_persistent_pane_id + 1;
        self.next_drawing_id = state.max_drawing_id + 1;
        for series in &mut self.series {
            if !series.removed {
                series.pane_index = 0;
            }
        }
        self.selected_drawing = None;
        self.drawing_drag = None;
        self.drawing_history = crate::DrawingHistory::default();
        // Persistence replaces committed semantic state, but an armed host tool is transient UI
        // state and historically survived import. Abort only the in-flight placement/capture.
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
        self.editing_drawing = None;
        self.hovered_drawing = None;
        self.hovered_text = None;
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), semantic_started) {
            profile.semantic_install_ns =
                started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let index_started = profile.as_ref().map(|_| Instant::now());
        self.drawing_runtime
            .borrow_mut()
            .rebuild_all(&self.drawings, self.panes.len());
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(profile), Some(started)) = (profile, index_started) {
            profile.index_rebuild_ns =
                started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        }
        self.layout_panes(self.pane_h);
        self.invalidate_frame_all();
        Ok(PersistenceRestoreResult {
            schema_version: PERSISTENCE_SCHEMA_VERSION,
            panes: self.panes.len(),
            drawings: drawing_count,
            points,
        })
    }

    /// Validate first, then install atomically. Any error leaves the chart unchanged.
    pub fn import_state_json(
        &mut self,
        json: &str,
    ) -> Result<PersistenceRestoreResult, ChartError> {
        if json.len() > PERSISTENCE_MAX_GENERAL_DOCUMENT_BYTES {
            return Err(resource("persistence document exceeds the size limit"));
        }
        let envelope: serde_json::Value = match serde_json::from_str(json) {
            Ok(envelope) => envelope,
            Err(_) if json.len() > PERSISTENCE_MAX_DOCUMENT_BYTES => {
                return Err(resource("persistence document exceeds the V1 size limit"));
            }
            Err(error) => {
                return Err(ChartError::new(
                    ErrorCode::SerializationError,
                    format!("malformed JSON: {error}"),
                ));
            }
        };
        let is_v2 = envelope
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(PERSISTENCE_SCHEMA_VERSION_GENERAL));
        if !is_v2 && json.len() > PERSISTENCE_MAX_DOCUMENT_BYTES {
            return Err(resource("persistence document exceeds the V1 size limit"));
        }
        if is_v2 {
            if envelope.get("schema").and_then(serde_json::Value::as_str)
                != Some("nucleuscharts-state")
            {
                return Err(ChartError::new(
                    ErrorCode::SerializationError,
                    "unsupported or missing persistence schema",
                ));
            }
            let state: StateV2 = serde_json::from_value(envelope).map_err(|error| {
                ChartError::new(
                    ErrorCode::SerializationError,
                    format!("invalid V2 document: {error}"),
                )
            })?;
            return self.import_state_v2(state);
        }
        let state = Self::validate_state_json(json)?;
        self.install_validated_state(state)
    }

    fn import_state_v2(&mut self, state: StateV2) -> Result<PersistenceRestoreResult, ChartError> {
        if self.panes.len() != 1
            || self.panes[0].general_horizontal_domain.is_some()
            || !self.drawings.is_empty()
            || self.next_drawing_id != 1
            || self.general_dataset_count() != 0
            || self.general_series_count() != 0
            || self.general_axes.has_issued_handles()
        {
            return Err(ChartError::new(
                ErrorCode::UnsupportedOperation,
                "state import requires a fresh chart before general or drawing handles have been issued",
            ));
        }
        if !state.chart_options.is_object() {
            return Err(invalid("V2 chart_options must be an object"));
        }
        serde_json::from_value::<nucleuscharts_core::options::ChartOptions>(
            state.chart_options.clone(),
        )
        .map_err(|error| invalid(format!("invalid V2 chart_options: {error}")))?;
        let domains = state
            .panes
            .iter()
            .map(|pane| pane.horizontal_domain)
            .collect::<Vec<_>>();
        let validated = Self::validate_state_v1(StateV1 {
            schema: state.schema,
            schema_version: PERSISTENCE_SCHEMA_VERSION,
            panes: state.panes.into_iter().map(|pane| pane.pane).collect(),
            drawings: state.drawings,
        })?;
        let mut staged = ChartEngine::new(self.css_width, self.css_height, self.dpr);
        staged.next_pane_id = self.next_pane_id;
        let mut result = staged.install_validated_state(validated)?;
        for (pane, domain) in staged.panes.iter_mut().zip(domains) {
            pane.general_horizontal_domain = staged.general_horizontal_domains.register(domain)?;
        }
        for axis in state.axes {
            staged.add_general_axis(axis)?;
        }
        let mut datasets = HashMap::new();
        for dataset in state.datasets {
            if datasets.contains_key(&dataset.id) {
                return Err(invalid(format!("duplicate dataset id {:?}", dataset.id)));
            }
            let id = staged.create_general_xy_dataset(dataset.input.clone())?;
            if let Some(labels) = dataset.labels {
                staged.replace_general_xy_dataset_labeled(id, dataset.input, Some(labels))?;
            }
            datasets.insert(dataset.id, id);
        }
        for series in state.series {
            let dataset = *datasets.get(&series.dataset).ok_or_else(|| {
                invalid(format!(
                    "series references unknown dataset {:?}",
                    series.dataset
                ))
            })?;
            staged.add_general_series(crate::GeneralSeriesOptions {
                kind: series.kind,
                pane: series.pane,
                dataset,
                x_axis_id: series.x_axis_id,
                y_axis_id: series.y_axis_id,
                visible: series.visible,
                title: series.title,
                color: series.color,
                point_radius: series.point_radius,
                data_labels: series.data_labels,
                group_id: series.group_id,
                stack_id: series.stack_id,
                stack_mode: series.stack_mode,
            })?;
        }
        let options_json = serde_json::to_string(&state.chart_options)
            .map_err(|error| invalid(format!("invalid V2 chart_options: {error}")))?;
        staged
            .apply_options(&options_json)
            .map_err(|error| invalid(format!("invalid V2 chart_options: {error}")))?;
        self.panes = staged.panes;
        self.general_horizontal_domains = staged.general_horizontal_domains;
        self.general_axes = staged.general_axes;
        self.general_data = staged.general_data;
        self.general_series = staged.general_series;
        self.drawings = staged.drawings;
        self.next_pane_id = staged.next_pane_id;
        self.next_persistent_pane_id = staged.next_persistent_pane_id;
        self.next_drawing_id = staged.next_drawing_id;
        self.options = staged.options;
        self.apply_options(&options_json)
            .expect("validated V2 chart options must serialize");
        self.selected_drawing = None;
        self.drawing_drag = None;
        self.drawing_history = crate::DrawingHistory::default();
        self.drawing_controller.pending = None;
        self.drawing_controller.brush = None;
        self.editing_drawing = None;
        self.hovered_drawing = None;
        self.hovered_text = None;
        for series in &mut self.series {
            if !series.removed {
                series.pane_index = 0;
            }
        }
        self.drawing_runtime
            .borrow_mut()
            .rebuild_all(&self.drawings, self.panes.len());
        self.layout_panes(self.pane_h);
        self.invalidate_frame_all();
        result.schema_version = PERSISTENCE_SCHEMA_VERSION_GENERAL;
        Ok(result)
    }

    /// Import with per-stage timings for the repository's release evidence harness.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn import_state_json_profiled(
        &mut self,
        json: &str,
    ) -> Result<PersistenceRestoreProfile, ChartError> {
        let parse_started = Instant::now();
        let state = Self::parse_state_json(json)?;
        let parse_ns = parse_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let validation_started = Instant::now();
        let state = Self::validate_state_v1(state)?;
        let validation_ns = validation_started
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        let mut install = InstallProfile::default();
        let restore = self.install_validated_state_inner(state, Some(&mut install))?;
        Ok(PersistenceRestoreProfile {
            restore,
            parse_ns,
            validation_ns,
            semantic_install_ns: install.semantic_install_ns,
            index_rebuild_ns: install.index_rebuild_ns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = include_str!("../fixtures/persistence/minimal-v1.json");
    const VALID: &str = include_str!("../fixtures/persistence/valid-v1.json");
    const ALL_DRAWINGS: &str =
        include_str!("../fixtures/persistence/all-drawings-multipane-v1.json");
    const MALFORMED: &str = include_str!("../fixtures/persistence/malformed.json");
    const UNKNOWN_VERSION: &str = include_str!("../fixtures/persistence/unknown-version.json");
    const UNKNOWN_KIND: &str = include_str!("../fixtures/persistence/unknown-kind-v1.json");

    fn settled_chart() -> ChartEngine {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
        let values = [11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0, 10.0];
        chart
            .set_series_data(0, &times, &values, &values, &values, &values)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();
        chart
    }

    #[test]
    fn supported_fixtures_restore_and_canonical_output_round_trips() {
        for fixture in [MINIMAL, VALID, ALL_DRAWINGS] {
            let mut first = ChartEngine::new(800.0, 500.0, 1.0);
            first.import_state_json(fixture).unwrap();
            let canonical = first.export_state_json().unwrap();
            let mut second = ChartEngine::new(800.0, 500.0, 1.0);
            second.import_state_json(&canonical).unwrap();
            assert_eq!(second.export_state_json().unwrap(), canonical);
        }
    }

    #[test]
    fn v2_round_trip_preserves_general_panes_axes_data_and_series() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                crate::HorizontalDomain::Category {
                    scale: crate::CategoryScaleType::Band,
                },
            )
            .unwrap();
        chart
            .add_general_axis(crate::GeneralAxisOptions::new(
                "category-x",
                pane,
                crate::AxisDimension::X,
                crate::GeneralScaleType::Band,
            ))
            .unwrap();
        chart
            .add_general_axis(crate::GeneralAxisOptions::new(
                "category-y",
                pane,
                crate::AxisDimension::Y,
                crate::GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = chart
            .create_general_xy_dataset(crate::GeneralXyInput::Category {
                ids: Some(vec![crate::GeneralRowId::Text("jan".into())]),
                categories: vec!["Jan".into()],
                category_indices: vec![0],
                y: vec![42.0],
                y_valid: None,
            })
            .unwrap();
        chart
            .replace_general_xy_dataset_labeled(
                dataset,
                crate::GeneralXyInput::Category {
                    ids: Some(vec![crate::GeneralRowId::Text("jan".into())]),
                    categories: vec!["Jan".into()],
                    category_indices: vec![0],
                    y: vec![42.0],
                    y_valid: None,
                },
                Some(vec![Some("January".into())]),
            )
            .unwrap();
        let mut series =
            crate::GeneralSeriesOptions::column(pane, dataset, "category-x", "category-y");
        series.data_labels = true;
        series.group_id = Some("sales".into());
        series.stack_id = Some("share".into());
        series.stack_mode = crate::GeneralStackMode::Percent;
        chart.add_general_series(series).unwrap();
        let mut line =
            crate::GeneralSeriesOptions::xy_line(pane, dataset, "category-x", "category-y");
        line.title = "Category trend".into();
        chart.add_general_series(line).unwrap();
        let mut area =
            crate::GeneralSeriesOptions::xy_area(pane, dataset, "category-x", "category-y");
        area.title = "Category area".into();
        chart.add_general_series(area).unwrap();

        let document = chart.export_state_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&document).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["series"][0]["group_id"], "sales");
        assert_eq!(value["series"][0]["stack_id"], "share");
        assert_eq!(value["series"][0]["stack_mode"], "Percent");
        let mut restored = ChartEngine::new(800.0, 500.0, 1.0);
        let result = restored.import_state_json(&document).unwrap();
        assert_eq!(result.schema_version, 2);
        assert_eq!(restored.export_state_json().unwrap(), document);
    }

    #[test]
    fn v2_round_trip_preserves_bubble_size_channel_and_missingness() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                crate::HorizontalDomain::Continuous {
                    scale: crate::ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        chart
            .add_general_axis(crate::GeneralAxisOptions::new(
                "bubble-x",
                pane,
                crate::AxisDimension::X,
                crate::GeneralScaleType::Linear,
            ))
            .unwrap();
        chart
            .add_general_axis(crate::GeneralAxisOptions::new(
                "bubble-y",
                pane,
                crate::AxisDimension::Y,
                crate::GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = chart
            .create_general_xy_dataset(crate::GeneralXyInput::Bubble {
                ids: Some(vec![
                    crate::GeneralRowId::Text("small".into()),
                    crate::GeneralRowId::Text("missing".into()),
                ]),
                x: vec![1.0, 2.0],
                y: vec![3.0, 4.0],
                y_valid: None,
                size: vec![25.0, 0.0],
                size_valid: Some(vec![1, 0]),
            })
            .unwrap();
        let series = crate::GeneralSeriesOptions::bubble(pane, dataset, "bubble-x", "bubble-y");
        chart.add_general_series(series).unwrap();

        let document = chart.export_state_json().unwrap();
        let mut restored = ChartEngine::new(800.0, 500.0, 1.0);
        restored.import_state_json(&document).unwrap();
        assert_eq!(restored.export_state_json().unwrap(), document);
        let restored_series = restored.general_series_ids_in_pane(pane)[0];
        assert_eq!(
            restored
                .general_tooltip_snapshot(restored_series, 0)
                .unwrap()
                .size,
            Some(25.0)
        );
        assert_eq!(
            restored
                .general_tooltip_snapshot(restored_series, 1)
                .unwrap()
                .size,
            None
        );
    }

    #[test]
    fn v2_round_trip_preserves_range_area_bounds_and_missingness() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let pane = chart
            .add_pane_with_domain(
                true,
                crate::HorizontalDomain::Continuous {
                    scale: crate::ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        chart
            .add_general_axis(crate::GeneralAxisOptions::new(
                "range-x",
                pane,
                crate::AxisDimension::X,
                crate::GeneralScaleType::Linear,
            ))
            .unwrap();
        chart
            .add_general_axis(crate::GeneralAxisOptions::new(
                "range-y",
                pane,
                crate::AxisDimension::Y,
                crate::GeneralScaleType::Linear,
            ))
            .unwrap();
        let dataset = chart
            .create_general_xy_dataset(crate::GeneralXyInput::RangeNumeric {
                ids: Some(vec![
                    crate::GeneralRowId::Text("first".into()),
                    crate::GeneralRowId::Text("gap".into()),
                ]),
                x: vec![1.0, 2.0],
                low: vec![10.0, 0.0],
                low_valid: Some(vec![1, 0]),
                high: vec![20.0, 0.0],
                high_valid: Some(vec![1, 0]),
            })
            .unwrap();
        chart
            .add_general_series(crate::GeneralSeriesOptions::range_area(
                pane, dataset, "range-x", "range-y",
            ))
            .unwrap();

        let document = chart.export_state_json().unwrap();
        let mut restored = ChartEngine::new(800.0, 500.0, 1.0);
        restored.import_state_json(&document).unwrap();
        assert_eq!(restored.export_state_json().unwrap(), document);
        let restored_series = restored.general_series_ids_in_pane(pane)[0];
        let first = restored
            .general_tooltip_snapshot(restored_series, 0)
            .unwrap();
        assert_eq!(first.low, Some(10.0));
        assert_eq!(first.high, Some(20.0));
        let gap = restored
            .general_tooltip_snapshot(restored_series, 1)
            .unwrap();
        assert_eq!(gap.low, None);
        assert_eq!(gap.high, None);
    }

    #[test]
    fn v2_invalid_series_reference_leaves_target_unchanged() {
        let mut source = ChartEngine::new(800.0, 500.0, 1.0);
        source
            .add_pane_with_domain(
                true,
                crate::HorizontalDomain::Continuous {
                    scale: crate::ContinuousScaleType::Linear,
                },
            )
            .unwrap();
        let mut document: serde_json::Value =
            serde_json::from_str(&source.export_state_json().unwrap()).unwrap();
        document["series"] = serde_json::json!([{
            "kind": "Scatter",
            "pane": 1,
            "dataset": "dataset-999",
            "x_axis_id": "x",
            "y_axis_id": "y",
            "visible": true,
            "title": "",
            "color": null,
            "point_radius": 3.0,
            "data_labels": false
        }]);
        let mut target = ChartEngine::new(800.0, 500.0, 1.0);
        let before = target.export_state_json().unwrap();
        assert!(target.import_state_json(&document.to_string()).is_err());
        assert_eq!(target.export_state_json().unwrap(), before);
    }

    #[test]
    fn v2_import_rejects_a_chart_after_general_axis_handles_were_issued() {
        let mut source = ChartEngine::new(800.0, 500.0, 1.0);
        source
            .add_pane_with_domain(true, crate::HorizontalDomain::Temporal)
            .unwrap();
        let document = source.export_state_json().unwrap();

        let mut target = ChartEngine::new(800.0, 500.0, 1.0);
        let pane = target
            .add_pane_with_domain(true, crate::HorizontalDomain::Temporal)
            .unwrap();
        target
            .add_general_axis(crate::GeneralAxisOptions::new(
                "stale-x",
                pane,
                crate::AxisDimension::X,
                crate::GeneralScaleType::Temporal,
            ))
            .unwrap();
        assert!(target.remove_general_axis("stale-x"));
        assert!(target.remove_pane(pane));
        let before = target.export_state_json().unwrap();

        let error = target.import_state_json(&document).unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);
        assert_eq!(target.export_state_json().unwrap(), before);
    }

    #[test]
    fn unknown_optional_v1_fields_are_ignored_deterministically() {
        let mut document: serde_json::Value = serde_json::from_str(VALID).unwrap();
        document["future_metadata"] = serde_json::json!({ "safe_to_ignore": true });
        document["panes"][0]["future_pane_option"] = serde_json::json!(42);
        document["drawings"][0]["style"]["future_style_option"] = serde_json::json!("x");

        let mut baseline = ChartEngine::new(800.0, 500.0, 1.0);
        baseline.import_state_json(VALID).unwrap();
        let mut restored = ChartEngine::new(800.0, 500.0, 1.0);
        restored.import_state_json(&document.to_string()).unwrap();
        assert_eq!(restored.export_state_json(), baseline.export_state_json());
    }

    #[test]
    fn all_ten_kinds_and_multi_pane_associations_restore() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let restored = chart.import_state_json(ALL_DRAWINGS).unwrap();
        assert_eq!(restored.panes, 2);
        assert_eq!(restored.drawings, 10);
        assert_eq!(
            chart
                .drawings
                .iter()
                .map(|drawing| drawing.kind)
                .collect::<Vec<_>>(),
            [
                DrawingKind::TrendLine,
                DrawingKind::HorizontalLine,
                DrawingKind::HorizontalRay,
                DrawingKind::VerticalLine,
                DrawingKind::Rectangle,
                DrawingKind::Text,
                DrawingKind::Brush,
                DrawingKind::Path,
                DrawingKind::LongPosition,
                DrawingKind::ShortPosition,
            ]
        );
        assert!(chart.drawings[..3]
            .iter()
            .all(|drawing| drawing.pane_index == 0));
        assert!(chart.drawings[3..]
            .iter()
            .all(|drawing| drawing.pane_index == 1));
        assert_eq!(chart.panes[0].persistent_id(), Some(3));
        assert_eq!(chart.panes[1].persistent_id(), Some(9));
    }

    #[test]
    fn restore_issues_fresh_live_pane_ids_and_stales_old_handles() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let old = chart.pane_stable_id(0).unwrap();
        chart.import_state_json(ALL_DRAWINGS).unwrap();
        assert_eq!(chart.pane_index_for_id(old), None);
        assert_ne!(chart.pane_stable_id(0), Some(old));
    }

    #[test]
    fn malformed_unknown_and_semantically_invalid_documents_are_atomic() {
        for (document, code) in [
            (MALFORMED, ErrorCode::SerializationError),
            (r#"{}"#, ErrorCode::SerializationError),
            (
                r#"{"schema":"nucleuscharts-state","schema_version":"1","panes":[],"drawings":[]}"#,
                ErrorCode::SerializationError,
            ),
            (UNKNOWN_VERSION, ErrorCode::PersistenceVersionError),
            (UNKNOWN_KIND, ErrorCode::InvalidData),
            (
                r#"{"schema":"nucleuscharts-state","schema_version":1,"panes":[{"id":"pane-1"}],"drawings":[{"id":1,"kind":"text","pane_id":"pane-2","anchors":[{"logical":1,"price":1}]}]}"#,
                ErrorCode::InvalidData,
            ),
            (
                r#"{"schema":"nucleuscharts-state","schema_version":1,"panes":[{"id":"pane-1"},{"id":"pane-1"}],"drawings":[]}"#,
                ErrorCode::InvalidData,
            ),
            (
                r#"{"schema":"nucleuscharts-state","schema_version":1,"panes":[{"id":"pane-1"}],"drawings":[{"id":1,"kind":"text","pane_id":"pane-1","anchors":[{"logical":1,"price":1}]},{"id":1,"kind":"text","pane_id":"pane-1","anchors":[{"logical":2,"price":2}]}]}"#,
                ErrorCode::InvalidData,
            ),
            (
                r#"{"schema":"nucleuscharts-state","schema_version":1,"panes":[{"id":"pane-1"}],"drawings":[{"id":1,"kind":"text","pane_id":"pane-1","anchors":[{"logical":1e999,"price":1}]}]}"#,
                ErrorCode::SerializationError,
            ),
            (
                r#"{"schema":"nucleuscharts-state","schema_version":1,"panes":[{"id":"pane-1"}],"drawings":[{"id":1,"kind":"rectangle","pane_id":"pane-1","anchors":[{"logical":1,"price":1}]}]}"#,
                ErrorCode::InvalidData,
            ),
        ] {
            let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
            let before = chart.export_state_json().unwrap();
            let error = chart.import_state_json(document).unwrap_err();
            assert_eq!(error.code(), code);
            assert_eq!(chart.export_state_json().unwrap(), before);
        }
    }

    #[test]
    fn document_and_anchor_resource_limits_fail_before_install() {
        let too_large = " ".repeat(PERSISTENCE_MAX_DOCUMENT_BYTES + 1);
        assert_eq!(
            ChartEngine::validate_state_json(&too_large)
                .unwrap_err()
                .code(),
            ErrorCode::ResourceLimit
        );
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        assert_eq!(
            chart.import_state_json(&too_large).unwrap_err().code(),
            ErrorCode::ResourceLimit,
            "V2 capacity must not weaken the legacy oversized-document contract"
        );
        let anchors = (0..=PERSISTENCE_MAX_POINTS_PER_DRAWING)
            .map(|index| serde_json::json!({ "logical": index, "price": 1 }))
            .collect::<Vec<_>>();
        let document = serde_json::json!({
            "schema": "nucleuscharts-state",
            "schema_version": 1,
            "panes": [{ "id": "pane-1" }],
            "drawings": [{ "id": 1, "kind": "brush", "pane_id": "pane-1", "anchors": anchors }]
        })
        .to_string();
        assert_eq!(
            ChartEngine::validate_state_json(&document)
                .unwrap_err()
                .code(),
            ErrorCode::ResourceLimit
        );
    }

    #[test]
    fn import_is_fresh_chart_only_and_drawing_ids_are_not_reused() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let issued = chart
            .add_drawing(
                DrawingKind::Text,
                0,
                vec![DrawingPoint {
                    logical: 1.0,
                    price: 1.0,
                }],
                None,
            )
            .unwrap();
        assert!(chart.remove_drawing(issued));
        let error = chart.import_state_json(VALID).unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnsupportedOperation);

        let mut restored = ChartEngine::new(800.0, 500.0, 1.0);
        restored.import_state_json(VALID).unwrap();
        assert!(restored.remove_drawing(12));
        let next = restored
            .add_drawing(
                DrawingKind::Text,
                0,
                vec![DrawingPoint {
                    logical: 1.0,
                    price: 1.0,
                }],
                None,
            )
            .unwrap();
        assert_eq!(next, 13);
    }

    #[test]
    fn one_thousand_drawing_restore_rebuilds_m6_runtime_once_and_remains_candidate_bounded() {
        let mut source = settled_chart();
        for index in 0..1_000 {
            let logical = if index < 10 {
                2.0 + index as f64 * 0.4
            } else {
                1_000.0 + index as f64 * 10.0
            };
            source
                .add_drawing(
                    DrawingKind::TrendLine,
                    0,
                    vec![
                        DrawingPoint {
                            logical,
                            price: 10.5,
                        },
                        DrawingPoint {
                            logical: logical + 0.25,
                            price: 11.0,
                        },
                    ],
                    None,
                )
                .unwrap();
        }
        let document = source.export_state_json().unwrap();
        drop(source);

        let mut restored = settled_chart();
        let result = restored.import_state_json(&document).unwrap();
        assert_eq!(result.drawings, 1_000);
        restored.reset_drawing_work_stats();
        restored.build_frame();
        let work = restored.drawing_work_stats();
        assert_eq!(work.drawings_total, 1_000);
        assert_eq!(work.candidates, 10);
        assert_eq!(work.visible, 10);
        assert_eq!(work.geometry_rebuilds, 10);
    }
}
