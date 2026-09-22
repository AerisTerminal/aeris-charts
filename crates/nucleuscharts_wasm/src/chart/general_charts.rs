//! Thin browser boundary for engine-owned general Cartesian charts.

use js_sys::{Float64Array, Uint32Array, Uint8Array};
use nucleuscharts_engine::{
    AxisDimension, AxisPosition, CategoryScaleType, ChartError, ContinuousScaleType,
    GeneralAxisDomain, GeneralAxisOptions, GeneralDatasetId, GeneralHitMode, GeneralRowId,
    GeneralRowIdentity, GeneralScaleType, GeneralSeriesId, GeneralSeriesKind, GeneralSeriesOptions,
    GeneralStackMode, GeneralXyInput, HorizontalDomain, MAX_GENERAL_TEMPORAL_MILLISECONDS,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::ChartInner;

#[derive(Deserialize)]
struct PaneInput {
    #[serde(default)]
    preserve_empty: bool,
    horizontal_domain: DomainInput,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DomainInput {
    FinancialTime,
    Continuous {
        #[serde(default)]
        scale: ContinuousInput,
    },
    Temporal,
    Category {
        #[serde(default)]
        scale: CategoryInput,
    },
    Polar,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContinuousInput {
    #[default]
    Linear,
    Log,
    Symlog,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CategoryInput {
    #[default]
    Band,
    Point,
}

#[derive(Deserialize)]
struct AxisInput {
    id: String,
    pane: usize,
    dimension: String,
    scale: String,
    #[serde(default)]
    position: Option<String>,
    #[serde(default)]
    domain: Option<Value>,
    #[serde(default)]
    reverse: bool,
    #[serde(default = "default_true")]
    visible: bool,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    tick_count: Option<u16>,
    #[serde(default = "default_tick_gap")]
    min_tick_gap: f64,
    #[serde(default = "default_band_padding")]
    band_padding_inner: f64,
    #[serde(default = "default_band_padding")]
    band_padding_outer: f64,
    #[serde(default = "default_true")]
    zero_line: bool,
    #[serde(default = "default_true")]
    grid_visible: bool,
}

#[derive(Deserialize)]
struct SeriesInput {
    pane: usize,
    x_axis_id: String,
    y_axis_id: String,
    #[serde(default = "default_true")]
    visible: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default = "default_point_radius")]
    point_radius: f64,
    #[serde(default)]
    data_labels: bool,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    stack_id: Option<String>,
    #[serde(default)]
    stack_mode: Option<String>,
}

#[derive(Deserialize)]
struct CategoryUpdateInput {
    categories: Vec<String>,
    max_rows: u32,
    labels: Option<Vec<Option<String>>>,
}

#[derive(Deserialize)]
struct CategoryDataInput {
    categories: Vec<String>,
    labels: Option<Vec<Option<String>>>,
}

#[derive(Deserialize)]
struct NumericDataInput {
    ids: Option<Vec<Value>>,
    labels: Option<Vec<Option<String>>>,
}

fn default_true() -> bool {
    true
}

fn default_tick_gap() -> f64 {
    4.0
}

fn default_band_padding() -> f64 {
    0.1
}

fn default_point_radius() -> f64 {
    3.0
}

fn result_ok(value: Value) -> String {
    json!({ "ok": true, "result": value }).to_string()
}

fn result_error(error: &ChartError) -> String {
    json!({
        "ok": false,
        "error": { "code": error.code().name(), "message": error.message() }
    })
    .to_string()
}

fn input_error(message: impl Into<String>) -> String {
    let error = ChartError::new(nucleuscharts_engine::ErrorCode::InvalidOptions, message);
    result_error(&error)
}

fn data_error(message: impl Into<String>) -> String {
    let error = ChartError::new(nucleuscharts_engine::ErrorCode::InvalidData, message);
    result_error(&error)
}

fn temporal_x_values(values: &Float64Array) -> Result<Vec<i64>, String> {
    values
        .to_vec()
        .into_iter()
        .map(|value| {
            if !value.is_finite()
                || value.fract() != 0.0
                || value.abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as f64
            {
                return Err(data_error(format!(
                    "general temporal X values must be whole epoch milliseconds within +/-{MAX_GENERAL_TEMPORAL_MILLISECONDS}"
                )));
            }
            Ok(value as i64)
        })
        .collect()
}

fn parse_json<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, String> {
    serde_json::from_str(text)
        .map_err(|error| input_error(format!("invalid general-chart options: {error}")))
}

fn horizontal_domain(input: DomainInput) -> HorizontalDomain {
    match input {
        DomainInput::FinancialTime => HorizontalDomain::FinancialTime,
        DomainInput::Continuous { scale } => HorizontalDomain::Continuous {
            scale: match scale {
                ContinuousInput::Linear => ContinuousScaleType::Linear,
                ContinuousInput::Log => ContinuousScaleType::Logarithmic,
                ContinuousInput::Symlog => ContinuousScaleType::SymmetricLog,
            },
        },
        DomainInput::Temporal => HorizontalDomain::Temporal,
        DomainInput::Category { scale } => HorizontalDomain::Category {
            scale: match scale {
                CategoryInput::Band => CategoryScaleType::Band,
                CategoryInput::Point => CategoryScaleType::Point,
            },
        },
        DomainInput::Polar => HorizontalDomain::Polar,
    }
}

fn dimension(value: &str) -> Option<AxisDimension> {
    match value {
        "x" => Some(AxisDimension::X),
        "y" => Some(AxisDimension::Y),
        "angle" => Some(AxisDimension::Angle),
        "radius" => Some(AxisDimension::Radius),
        _ => None,
    }
}

fn position(value: Option<&str>) -> Option<Option<AxisPosition>> {
    match value {
        None => Some(None),
        Some("top") => Some(Some(AxisPosition::Top)),
        Some("bottom") => Some(Some(AxisPosition::Bottom)),
        Some("left") => Some(Some(AxisPosition::Left)),
        Some("right") => Some(Some(AxisPosition::Right)),
        Some(_) => None,
    }
}

fn scale(value: &str) -> Option<GeneralScaleType> {
    match value {
        "linear" => Some(GeneralScaleType::Linear),
        "log" => Some(GeneralScaleType::Logarithmic),
        "symlog" => Some(GeneralScaleType::SymmetricLog),
        "temporal" => Some(GeneralScaleType::Temporal),
        "band" => Some(GeneralScaleType::Band),
        "point" => Some(GeneralScaleType::Point),
        "radial_linear" => Some(GeneralScaleType::RadialLinear),
        "angular_category" => Some(GeneralScaleType::AngularCategory),
        _ => None,
    }
}

fn axis_domain(scale: GeneralScaleType, value: Option<Value>) -> Option<GeneralAxisDomain> {
    let Some(value) = value else {
        return Some(GeneralAxisDomain::Auto);
    };
    if value == "auto" {
        return Some(GeneralAxisDomain::Auto);
    }
    let values = value.as_array()?;
    match scale {
        GeneralScaleType::Linear
        | GeneralScaleType::Logarithmic
        | GeneralScaleType::SymmetricLog
        | GeneralScaleType::RadialLinear => {
            if values.len() != 2 {
                return None;
            }
            Some(GeneralAxisDomain::Numeric([
                values.first()?.as_f64()?,
                values.get(1)?.as_f64()?,
            ]))
        }
        GeneralScaleType::Temporal => {
            if values.len() != 2 {
                return None;
            }
            Some(GeneralAxisDomain::Temporal([
                values.first()?.as_i64()?,
                values.get(1)?.as_i64()?,
            ]))
        }
        GeneralScaleType::Band | GeneralScaleType::Point | GeneralScaleType::AngularCategory => {
            values
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .map(GeneralAxisDomain::Category)
        }
    }
}

fn parse_ids(ids_json: &str) -> Result<Option<Vec<GeneralRowId>>, String> {
    if ids_json.is_empty() || ids_json == "null" {
        return Ok(None);
    }
    let values: Vec<Value> = serde_json::from_str(ids_json)
        .map_err(|error| input_error(format!("invalid general row IDs: {error}")))?;
    parse_id_values(Some(values))
}

fn parse_id_values(values: Option<Vec<Value>>) -> Result<Option<Vec<GeneralRowId>>, String> {
    let Some(values) = values else {
        return Ok(None);
    };
    values
        .into_iter()
        .map(|value| match value {
            Value::String(value) => Ok(GeneralRowId::Text(value)),
            Value::Number(value) => value
                .as_f64()
                .map(GeneralRowId::Number)
                .ok_or_else(|| input_error("general numeric row IDs must be finite numbers")),
            Value::Null => Ok(GeneralRowId::Generated),
            _ => Err(input_error(
                "general row IDs must be strings, numbers, or omitted-row markers",
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(super) fn row_identity(identity: &GeneralRowIdentity) -> Value {
    match identity {
        GeneralRowIdentity::Generated(value) => json!({ "generated": value.to_string() }),
        GeneralRowIdentity::Explicit(GeneralRowId::Number(value)) => json!(value),
        GeneralRowIdentity::Explicit(GeneralRowId::Text(value)) => json!(value),
        GeneralRowIdentity::Explicit(GeneralRowId::Generated) => Value::Null,
    }
}

pub(super) fn general_hit_value(hit: &nucleuscharts_engine::GeneralSeriesHit) -> Value {
    json!({
        "series": hit.series.get(),
        "row": hit.row,
        "row_id": row_identity(&hit.row_id),
        "distance": hit.distance,
    })
}

impl ChartInner {
    pub fn add_general_pane_result_json(&mut self, options_json: &str) -> String {
        let input = match parse_json::<PaneInput>(options_json) {
            Ok(input) => input,
            Err(error) => return error,
        };
        match self.engine.add_pane_with_domain(
            input.preserve_empty,
            horizontal_domain(input.horizontal_domain),
        ) {
            Ok(index) => result_ok(json!({ "pane": index })),
            Err(error) => result_error(&error),
        }
    }

    pub fn add_general_axis_result_json(&mut self, options_json: &str) -> String {
        let input = match parse_json::<AxisInput>(options_json) {
            Ok(input) => input,
            Err(error) => return error,
        };
        let Some(dimension) = dimension(&input.dimension) else {
            return input_error("unknown general axis dimension");
        };
        let Some(position) = position(input.position.as_deref()) else {
            return input_error("unknown general axis position");
        };
        let Some(scale) = scale(&input.scale) else {
            return input_error("unknown general axis scale");
        };
        let Some(domain) = axis_domain(scale, input.domain) else {
            return input_error("general axis domain does not match its scale");
        };
        let mut options = GeneralAxisOptions::new(input.id.clone(), input.pane, dimension, scale);
        options.position = position;
        options.domain = domain;
        options.reverse = input.reverse;
        options.visible = input.visible;
        options.title = input.title;
        options.tick_count = input.tick_count;
        options.min_tick_gap = input.min_tick_gap;
        options.band_padding_inner = input.band_padding_inner;
        options.band_padding_outer = input.band_padding_outer;
        options.zero_line = input.zero_line;
        options.grid_visible = input.grid_visible;
        match self.engine.add_general_axis(options) {
            Ok(()) => {
                let handle_token = self
                    .engine
                    .general_axis(&input.id)
                    .map_or(0, nucleuscharts_engine::GeneralAxis::handle_token);
                result_ok(json!({ "id": input.id, "handle_token": handle_token }))
            }
            Err(error) => result_error(&error),
        }
    }

    pub fn general_axis_handle_token(&self, id: &str) -> u32 {
        self.engine
            .general_axis(id)
            .map_or(0, nucleuscharts_engine::GeneralAxis::handle_token)
    }

    pub fn general_axis_json(&self, id: &str) -> String {
        let Some(axis) = self.engine.general_axis(id) else {
            return "null".to_owned();
        };
        let domain = match axis.domain() {
            GeneralAxisDomain::Auto => json!("auto"),
            GeneralAxisDomain::Numeric(value) => json!(value),
            GeneralAxisDomain::Temporal(value) => json!(value),
            GeneralAxisDomain::Category(value) => json!(value),
        };
        json!({
            "id": axis.id(),
            "pane": self.engine.general_axis_pane_index(id),
            "dimension": match axis.dimension() { AxisDimension::X => "x", AxisDimension::Y => "y", AxisDimension::Angle => "angle", AxisDimension::Radius => "radius" },
            "position": axis.position().map(|value| match value { AxisPosition::Top => "top", AxisPosition::Bottom => "bottom", AxisPosition::Left => "left", AxisPosition::Right => "right" }),
            "scale": match axis.scale() { GeneralScaleType::Linear => "linear", GeneralScaleType::Logarithmic => "log", GeneralScaleType::SymmetricLog => "symlog", GeneralScaleType::Temporal => "temporal", GeneralScaleType::Band => "band", GeneralScaleType::Point => "point", GeneralScaleType::RadialLinear => "radial_linear", GeneralScaleType::AngularCategory => "angular_category" },
            "domain": domain,
            "reverse": axis.reverse(),
            "visible": axis.visible(),
            "title": axis.title(),
            "tick_count": axis.tick_count(),
            "min_tick_gap": axis.min_tick_gap(),
            "band_padding_inner": axis.band_padding_inner(),
            "band_padding_outer": axis.band_padding_outer(),
            "zero_line": axis.zero_line(),
            "grid_visible": axis.grid_visible(),
        }).to_string()
    }

    pub fn general_axis_ids_json(&self, pane: i32) -> String {
        let pane = (pane >= 0).then_some(pane as usize);
        json!(self
            .engine
            .general_axes(pane)
            .into_iter()
            .map(|axis| axis.id())
            .collect::<Vec<_>>())
        .to_string()
    }

    pub fn general_series_ids(&self, pane: usize) -> Vec<u32> {
        self.engine
            .general_series_ids_in_pane(pane)
            .into_iter()
            .map(GeneralSeriesId::get)
            .collect()
    }

    /// Rehydrate browser handles after an atomic persistence restore.
    pub fn general_series_catalog_json(&self) -> String {
        let series = (0..self.engine.panes.len())
            .flat_map(|pane| self.engine.general_series_ids_in_pane(pane))
            .filter_map(|id| self.engine.general_series(id))
            .map(|series| {
                json!({
                    "id": series.id().get(),
                    "dataset": series.dataset().get(),
                    "x_axis_id": series.x_axis_id(),
                    "y_axis_id": series.y_axis_id(),
                    "kind": match series.kind() {
                        GeneralSeriesKind::XyLine => "xy_line",
                        GeneralSeriesKind::XyArea => "xy_area",
                        GeneralSeriesKind::RangeArea => "range_area",
                        GeneralSeriesKind::ErrorBar => "error_bar",
                        GeneralSeriesKind::Column => "column",
                        GeneralSeriesKind::Scatter => "scatter",
                        GeneralSeriesKind::Bubble => "bubble",
                    },
                })
            })
            .collect::<Vec<_>>();
        json!(series).to_string()
    }

    pub fn add_general_series_result_json(&mut self, kind: &str, options_json: &str) -> String {
        let input = match parse_json::<SeriesInput>(options_json) {
            Ok(input) => input,
            Err(error) => return error,
        };
        let (kind, empty) = match kind {
            "xy_line" | "xy_area" | "range_area" => {
                let Some(domain) = self.engine.pane_horizontal_domain(input.pane) else {
                    return input_error(format!("{kind} references a stale pane"));
                };
                let is_range = kind == "range_area";
                let empty = match (domain, is_range) {
                    (HorizontalDomain::Continuous { .. }, false) => GeneralXyInput::Numeric {
                        ids: None,
                        x: Vec::new(),
                        y: Vec::new(),
                        y_valid: None,
                    },
                    (HorizontalDomain::Continuous { .. }, true) => GeneralXyInput::RangeNumeric {
                        ids: None,
                        x: Vec::new(),
                        low: Vec::new(),
                        low_valid: None,
                        high: Vec::new(),
                        high_valid: None,
                    },
                    (HorizontalDomain::Temporal, false) => GeneralXyInput::Temporal {
                        ids: None,
                        x_epoch_ms: Vec::new(),
                        y: Vec::new(),
                        y_valid: None,
                    },
                    (HorizontalDomain::Temporal, true) => GeneralXyInput::RangeTemporal {
                        ids: None,
                        x_epoch_ms: Vec::new(),
                        low: Vec::new(),
                        low_valid: None,
                        high: Vec::new(),
                        high_valid: None,
                    },
                    (HorizontalDomain::Category { .. }, false) => GeneralXyInput::Category {
                        ids: None,
                        categories: Vec::new(),
                        category_indices: Vec::new(),
                        y: Vec::new(),
                        y_valid: None,
                    },
                    (HorizontalDomain::Category { .. }, true) => GeneralXyInput::RangeCategory {
                        ids: None,
                        categories: Vec::new(),
                        category_indices: Vec::new(),
                        low: Vec::new(),
                        low_valid: None,
                        high: Vec::new(),
                        high_valid: None,
                    },
                    (HorizontalDomain::FinancialTime | HorizontalDomain::Polar, _) => {
                        return input_error(format!(
                            "{kind} requires a continuous, temporal, or category pane"
                        ));
                    }
                };
                (
                    if kind == "xy_area" {
                        GeneralSeriesKind::XyArea
                    } else if kind == "range_area" {
                        GeneralSeriesKind::RangeArea
                    } else {
                        GeneralSeriesKind::XyLine
                    },
                    empty,
                )
            }
            "column" => (
                GeneralSeriesKind::Column,
                GeneralXyInput::Category {
                    ids: None,
                    categories: Vec::new(),
                    category_indices: Vec::new(),
                    y: Vec::new(),
                    y_valid: None,
                },
            ),
            "scatter" => (
                GeneralSeriesKind::Scatter,
                GeneralXyInput::Numeric {
                    ids: None,
                    x: Vec::new(),
                    y: Vec::new(),
                    y_valid: None,
                },
            ),
            "bubble" => (
                GeneralSeriesKind::Bubble,
                GeneralXyInput::Bubble {
                    ids: None,
                    x: Vec::new(),
                    y: Vec::new(),
                    y_valid: None,
                    size: Vec::new(),
                    size_valid: None,
                },
            ),
            "error_bar" => (
                GeneralSeriesKind::ErrorBar,
                GeneralXyInput::ErrorNumeric {
                    ids: None,
                    x: Vec::new(),
                    y: Vec::new(),
                    y_valid: None,
                    x_low: Vec::new(),
                    x_low_valid: None,
                    x_high: Vec::new(),
                    x_high_valid: None,
                    y_low: Vec::new(),
                    y_low_valid: None,
                    y_high: Vec::new(),
                    y_high_valid: None,
                },
            ),
            _ => return input_error("unsupported general series kind"),
        };
        let dataset = match self.engine.create_general_xy_dataset(empty) {
            Ok(dataset) => dataset,
            Err(error) => return result_error(&error),
        };
        let mut options = match kind {
            GeneralSeriesKind::XyLine => {
                GeneralSeriesOptions::xy_line(input.pane, dataset, input.x_axis_id, input.y_axis_id)
            }
            GeneralSeriesKind::XyArea => {
                GeneralSeriesOptions::xy_area(input.pane, dataset, input.x_axis_id, input.y_axis_id)
            }
            GeneralSeriesKind::RangeArea => GeneralSeriesOptions::range_area(
                input.pane,
                dataset,
                input.x_axis_id,
                input.y_axis_id,
            ),
            GeneralSeriesKind::ErrorBar => GeneralSeriesOptions::error_bar(
                input.pane,
                dataset,
                input.x_axis_id,
                input.y_axis_id,
            ),
            GeneralSeriesKind::Column => {
                GeneralSeriesOptions::column(input.pane, dataset, input.x_axis_id, input.y_axis_id)
            }
            GeneralSeriesKind::Scatter => {
                GeneralSeriesOptions::scatter(input.pane, dataset, input.x_axis_id, input.y_axis_id)
            }
            GeneralSeriesKind::Bubble => {
                GeneralSeriesOptions::bubble(input.pane, dataset, input.x_axis_id, input.y_axis_id)
            }
        };
        options.visible = input.visible;
        options.title = input.title;
        options.color = input.color;
        options.point_radius = input.point_radius;
        options.data_labels = input.data_labels;
        options.group_id = input.group_id;
        options.stack_id = input.stack_id;
        options.stack_mode = match input.stack_mode.as_deref().unwrap_or("normal") {
            "normal" => GeneralStackMode::Normal,
            "percent" => GeneralStackMode::Percent,
            _ => {
                self.engine.remove_general_dataset(dataset);
                return input_error("general series stack_mode must be normal or percent");
            }
        };
        match self.engine.add_general_series(options) {
            Ok(series) => result_ok(json!({ "series": series.get(), "dataset": dataset.get() })),
            Err(error) => {
                self.engine.remove_general_dataset(dataset);
                result_error(&error)
            }
        }
    }

    pub fn set_general_numeric_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Numeric {
            ids,
            x: x.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_general_bubble_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
        size: &Float64Array,
        size_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Bubble {
            ids,
            x: x.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
            size: size.to_vec(),
            size_valid: size_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    pub fn set_general_temporal_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x_epoch_ms: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let x_epoch_ms = match temporal_x_values(x_epoch_ms) {
            Ok(values) => values,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Temporal {
            ids,
            x_epoch_ms,
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    pub fn set_general_category_data_typed(
        &mut self,
        dataset: u32,
        ids_json: &str,
        metadata_json: &str,
        category_indices: &Uint32Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let ids = match parse_ids(ids_json) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let metadata = match parse_json::<CategoryDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Category {
            ids,
            categories: metadata.categories,
            category_indices: category_indices.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_general_range_numeric_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        low: &Float64Array,
        low_valid: Option<Uint8Array>,
        high: &Float64Array,
        high_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::RangeNumeric {
            ids,
            x: x.to_vec(),
            low: low.to_vec(),
            low_valid: low_valid.map(|values| values.to_vec()),
            high: high.to_vec(),
            high_valid: high_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_general_error_numeric_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
        x_low: &Float64Array,
        x_low_valid: Option<Uint8Array>,
        x_high: &Float64Array,
        x_high_valid: Option<Uint8Array>,
        y_low: &Float64Array,
        y_low_valid: Option<Uint8Array>,
        y_high: &Float64Array,
        y_high_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::ErrorNumeric {
            ids,
            x: x.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
            x_low: x_low.to_vec(),
            x_low_valid: x_low_valid.map(|values| values.to_vec()),
            x_high: x_high.to_vec(),
            x_high_valid: x_high_valid.map(|values| values.to_vec()),
            y_low: y_low.to_vec(),
            y_low_valid: y_low_valid.map(|values| values.to_vec()),
            y_high: y_high.to_vec(),
            y_high_valid: y_high_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_general_range_temporal_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x_epoch_ms: &Float64Array,
        low: &Float64Array,
        low_valid: Option<Uint8Array>,
        high: &Float64Array,
        high_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let x_epoch_ms = match temporal_x_values(x_epoch_ms) {
            Ok(values) => values,
            Err(error) => return error,
        };
        let input = GeneralXyInput::RangeTemporal {
            ids,
            x_epoch_ms,
            low: low.to_vec(),
            low_valid: low_valid.map(|values| values.to_vec()),
            high: high.to_vec(),
            high_valid: high_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_general_range_category_data_typed(
        &mut self,
        dataset: u32,
        ids_json: &str,
        metadata_json: &str,
        category_indices: &Uint32Array,
        low: &Float64Array,
        low_valid: Option<Uint8Array>,
        high: &Float64Array,
        high_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let ids = match parse_ids(ids_json) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let metadata = match parse_json::<CategoryDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let input = GeneralXyInput::RangeCategory {
            ids,
            categories: metadata.categories,
            category_indices: category_indices.to_vec(),
            low: low.to_vec(),
            low_valid: low_valid.map(|values| values.to_vec()),
            high: high.to_vec(),
            high_valid: high_valid.map(|values| values.to_vec()),
        };
        match self
            .engine
            .replace_general_xy_dataset_labeled(dataset, input, metadata.labels)
        {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    pub fn upsert_general_numeric_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
        max_rows: u32,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Numeric {
            ids,
            x: x.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            metadata.labels,
            (max_rows > 0).then_some(max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_general_bubble_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
        size: &Float64Array,
        size_valid: Option<Uint8Array>,
        max_rows: u32,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Bubble {
            ids,
            x: x.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
            size: size.to_vec(),
            size_valid: size_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            metadata.labels,
            (max_rows > 0).then_some(max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    pub fn upsert_general_temporal_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x_epoch_ms: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
        max_rows: u32,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let x_epoch_ms = match temporal_x_values(x_epoch_ms) {
            Ok(values) => values,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Temporal {
            ids,
            x_epoch_ms,
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            metadata.labels,
            (max_rows > 0).then_some(max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    pub fn upsert_general_category_data_typed(
        &mut self,
        dataset: u32,
        ids_json: &str,
        update_json: &str,
        category_indices: &Uint32Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let ids = match parse_ids(ids_json) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let update = match parse_json::<CategoryUpdateInput>(update_json) {
            Ok(update) => update,
            Err(error) => return error,
        };
        let input = GeneralXyInput::Category {
            ids,
            categories: update.categories,
            category_indices: category_indices.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            update.labels,
            (update.max_rows > 0).then_some(update.max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_general_range_numeric_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        low: &Float64Array,
        low_valid: Option<Uint8Array>,
        high: &Float64Array,
        high_valid: Option<Uint8Array>,
        max_rows: u32,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::RangeNumeric {
            ids,
            x: x.to_vec(),
            low: low.to_vec(),
            low_valid: low_valid.map(|values| values.to_vec()),
            high: high.to_vec(),
            high_valid: high_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            metadata.labels,
            (max_rows > 0).then_some(max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_general_error_numeric_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x: &Float64Array,
        y: &Float64Array,
        y_valid: Option<Uint8Array>,
        x_low: &Float64Array,
        x_low_valid: Option<Uint8Array>,
        x_high: &Float64Array,
        x_high_valid: Option<Uint8Array>,
        y_low: &Float64Array,
        y_low_valid: Option<Uint8Array>,
        y_high: &Float64Array,
        y_high_valid: Option<Uint8Array>,
        max_rows: u32,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let input = GeneralXyInput::ErrorNumeric {
            ids,
            x: x.to_vec(),
            y: y.to_vec(),
            y_valid: y_valid.map(|values| values.to_vec()),
            x_low: x_low.to_vec(),
            x_low_valid: x_low_valid.map(|values| values.to_vec()),
            x_high: x_high.to_vec(),
            x_high_valid: x_high_valid.map(|values| values.to_vec()),
            y_low: y_low.to_vec(),
            y_low_valid: y_low_valid.map(|values| values.to_vec()),
            y_high: y_high.to_vec(),
            y_high_valid: y_high_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            metadata.labels,
            (max_rows > 0).then_some(max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_general_range_temporal_data_typed(
        &mut self,
        dataset: u32,
        metadata_json: &str,
        x_epoch_ms: &Float64Array,
        low: &Float64Array,
        low_valid: Option<Uint8Array>,
        high: &Float64Array,
        high_valid: Option<Uint8Array>,
        max_rows: u32,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let metadata = match parse_json::<NumericDataInput>(metadata_json) {
            Ok(metadata) => metadata,
            Err(error) => return error,
        };
        let ids = match parse_id_values(metadata.ids) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let x_epoch_ms = match temporal_x_values(x_epoch_ms) {
            Ok(values) => values,
            Err(error) => return error,
        };
        let input = GeneralXyInput::RangeTemporal {
            ids,
            x_epoch_ms,
            low: low.to_vec(),
            low_valid: low_valid.map(|values| values.to_vec()),
            high: high.to_vec(),
            high_valid: high_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            metadata.labels,
            (max_rows > 0).then_some(max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_general_range_category_data_typed(
        &mut self,
        dataset: u32,
        ids_json: &str,
        update_json: &str,
        category_indices: &Uint32Array,
        low: &Float64Array,
        low_valid: Option<Uint8Array>,
        high: &Float64Array,
        high_valid: Option<Uint8Array>,
    ) -> String {
        let Some(dataset) = GeneralDatasetId::from_raw(dataset) else {
            return input_error("general dataset handle is stale");
        };
        let ids = match parse_ids(ids_json) {
            Ok(ids) => ids,
            Err(error) => return error,
        };
        let update = match parse_json::<CategoryUpdateInput>(update_json) {
            Ok(update) => update,
            Err(error) => return error,
        };
        let input = GeneralXyInput::RangeCategory {
            ids,
            categories: update.categories,
            category_indices: category_indices.to_vec(),
            low: low.to_vec(),
            low_valid: low_valid.map(|values| values.to_vec()),
            high: high.to_vec(),
            high_valid: high_valid.map(|values| values.to_vec()),
        };
        match self.engine.upsert_general_xy_dataset_labeled(
            dataset,
            input,
            update.labels,
            (update.max_rows > 0).then_some(update.max_rows as usize),
        ) {
            Ok(()) => result_ok(Value::Null),
            Err(error) => result_error(&error),
        }
    }

    pub fn remove_general_series(&mut self, series: u32, dataset: u32) -> bool {
        let (Some(series), Some(dataset)) = (
            GeneralSeriesId::from_raw(series),
            GeneralDatasetId::from_raw(dataset),
        ) else {
            return false;
        };
        self.engine.remove_general_series(series) && self.engine.remove_general_dataset(dataset)
    }

    pub fn general_tooltip_json(&self, series: u32, row: usize) -> String {
        let Some(series) = GeneralSeriesId::from_raw(series) else {
            return "null".to_owned();
        };
        let Some(snapshot) = self.engine.general_tooltip_snapshot(series, row) else {
            return "null".to_owned();
        };
        json!({
            "series": snapshot.series.get(), "row": snapshot.row,
            "row_id": row_identity(&snapshot.row_id), "x_label": snapshot.x_label,
            "label": snapshot.label, "value": snapshot.value, "low": snapshot.low, "high": snapshot.high,
            "x_low": snapshot.x_low, "x_high": snapshot.x_high,
            "size": snapshot.size, "title": snapshot.title,
        })
        .to_string()
    }

    pub fn general_accessibility_json(&self, series: u32, offset: usize, limit: usize) -> String {
        let Some(series) = GeneralSeriesId::from_raw(series) else {
            return "null".to_owned();
        };
        let Some(snapshot) = self
            .engine
            .general_accessibility_snapshot(series, offset, limit)
        else {
            return "null".to_owned();
        };
        json!({
            "series": snapshot.series.get(),
            "title": snapshot.title,
            "total_rows": snapshot.total_rows,
            "offset": snapshot.offset,
            "items": snapshot.items.into_iter().map(|item| json!({
                "row": item.row,
                "row_id": row_identity(&item.row_id),
                "x_label": item.x_label,
                "label": item.label,
                "value": item.value,
                "low": item.low,
                "high": item.high,
                "x_low": item.x_low,
                "x_high": item.x_high,
                "size": item.size,
            })).collect::<Vec<_>>(),
        })
        .to_string()
    }

    pub fn general_hit_test_json(&self, pane: usize, x: f64, y: f64, max_distance: f64) -> String {
        let mode = if max_distance < 0.0 {
            GeneralHitMode::Exact
        } else {
            GeneralHitMode::Nearest { max_distance }
        };
        let Some(hit) = self.engine.general_hit_test(pane, x, y, mode) else {
            return "null".to_owned();
        };
        general_hit_value(&hit).to_string()
    }

    pub fn general_selected_hit_json(&self) -> String {
        self.engine.general_selected_hit().as_ref().map_or_else(
            || "null".to_owned(),
            |hit| general_hit_value(hit).to_string(),
        )
    }

    pub fn general_accessibility_focused_hit_json(&self) -> String {
        self.engine
            .general_accessibility_focused_hit()
            .as_ref()
            .map_or_else(
                || "null".to_owned(),
                |hit| general_hit_value(hit).to_string(),
            )
    }

    pub fn select_general_hovered(&mut self) -> bool {
        self.engine.select_general_hovered()
    }

    pub fn clear_general_selection(&mut self) {
        self.engine.clear_general_selection();
    }

    pub fn set_general_accessibility_focus(&mut self, series: u32, row: usize) -> bool {
        let Some(series) = GeneralSeriesId::from_raw(series) else {
            return false;
        };
        self.engine.set_general_accessibility_focus(series, row)
    }

    pub fn clear_general_accessibility_focus(&mut self) {
        self.engine.clear_general_accessibility_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_axis_and_id_inputs_parse_without_host_state() {
        let pane: PaneInput =
            serde_json::from_str(r#"{"horizontal_domain":{"type":"continuous","scale":"symlog"}}"#)
                .unwrap();
        assert_eq!(
            horizontal_domain(pane.horizontal_domain),
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::SymmetricLog
            }
        );
        assert_eq!(parse_ids(r#"[1,"row"]"#).unwrap().unwrap().len(), 2);
        assert!(axis_domain(GeneralScaleType::Linear, Some(json!([0.0, 1.0]))).is_some());
        assert!(axis_domain(GeneralScaleType::Linear, Some(json!(["bad", 1.0]))).is_none());
    }
}
