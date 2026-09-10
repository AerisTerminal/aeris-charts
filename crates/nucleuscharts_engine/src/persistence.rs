//! Versioned persistence for stable semantic chart state.
//!
//! V1 deliberately contains pane topology and built-in drawings only. Market history, series and
//! indicator definitions, custom extensions, runtime caches, retained frames, and renderer state
//! remain host-owned or derived.

use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use nucleuscharts_core::model::data_validation::MAX_SAFE_VALUE;
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::LineStyle;

use crate::drawings::{DrawingPriceScale, DrawingTextHAlign, DrawingTextVAlign};
use crate::{ChartEngine, ChartError, Drawing, DrawingKind, DrawingPoint, ErrorCode, Pane, PaneId};

pub const PERSISTENCE_SCHEMA_VERSION: u32 = 1;
pub const PERSISTENCE_MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
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
    /// Deterministic V1 JSON containing pane topology and semantic drawings only.
    pub fn export_state_json(&self) -> Result<String, ChartError> {
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
        if self.panes.len() != 1 || !self.drawings.is_empty() || self.next_drawing_id != 1 {
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
        let state = Self::validate_state_json(json)?;
        self.install_validated_state(state)
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
    fn all_eight_kinds_and_multi_pane_associations_restore() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let restored = chart.import_state_json(ALL_DRAWINGS).unwrap();
        assert_eq!(restored.panes, 2);
        assert_eq!(restored.drawings, 8);
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
