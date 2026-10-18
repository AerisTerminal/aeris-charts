//! Interactive GPUI gallery for the shared general-chart catalog.
//! Run with: cargo run -p aeris_charts_render_gpui --features gpui-backend --example gpui_general_gallery
//!
//! The browser dashboard (`examples/web_demo/general_dashboard.js`) renders the same
//! `general_catalog.json` through the public browser API. This example translates each entry
//! into the equivalent engine calls, so both hosts paint every example from one engine frame.

use std::collections::HashMap;

use aeris_charts_core::options::ChartTheme;
use aeris_charts_engine::{
    CategoryScaleType, ChartEngine, ChartFrame, ContinuousScaleType, FinancialFrameRequest,
    FlowLink, FlowNode, FunnelStage, GeneralAxisOptions, GeneralAxisTick, GeneralHistogramOptions,
    GeneralLayoutInput, GeneralLayoutSeriesKind, GeneralLayoutSeriesOptions, GeneralPolarOptions,
    GeneralReferenceOptions, GeneralScaleType, GeneralSeriesKind, GeneralSeriesOptions,
    GeneralXyInput, HierarchyNode, HorizontalDomain,
};
use aeris_charts_render::draw_list::Prim;
use aeris_charts_render_gpui::{
    AerisViewport, GpuiChartRenderer, PreparedAerisFrame,
    backend::measure_text,
    input::{GpuiChartInput, install_text_metrics},
};
use gpui::{
    App, Bounds, Context, ElementId, Entity, MouseButton, Render, Window, WindowBounds,
    WindowOptions, canvas, div, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

const WIDTH: f32 = 1180.0;
const HEIGHT: f32 = 820.0;
const CATALOG: &str = include_str!("../../../examples/web_demo/general_catalog.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    #[serde(rename = "categories")]
    _categories: Vec<Value>,
    examples: Vec<Example>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    id: String,
    #[serde(rename = "title")]
    _title: String,
    #[serde(rename = "category")]
    _category: String,
    horizontal_domain: DomainSpec,
    #[serde(default = "default_true")]
    grid: bool,
    #[serde(default)]
    axes: Vec<AxisSpec>,
    #[serde(default)]
    series: Vec<SeriesSpec>,
    #[serde(default)]
    layout_series: Vec<LayoutSpec>,
    #[serde(default)]
    references: Vec<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum DomainSpec {
    Continuous { scale: Option<String> },
    Temporal,
    Category { scale: Option<String> },
    Polar,
    Layout,
}

/// Browser `general_axis_options` without `pane`; omitted fields keep the engine defaults.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisSpec {
    id: String,
    dimension: String,
    scale: String,
    position: Option<String>,
    domain: Option<Value>,
    reverse: Option<bool>,
    visible: Option<bool>,
    title: Option<String>,
    tick_count: Option<u16>,
    ticks: Option<Vec<GeneralAxisTick>>,
    min_tick_gap: Option<f64>,
    domain_padding: Option<f64>,
    band_padding_inner: Option<f64>,
    band_padding_outer: Option<f64>,
    zero_line: Option<bool>,
    grid_visible: Option<bool>,
    line_visible: Option<bool>,
}

/// Browser `general_series_options` without `pane`, plus the series kind and object rows.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeriesSpec {
    kind: String,
    data: Vec<Map<String, Value>>,
    x_axis_id: Option<String>,
    y_axis_id: Option<String>,
    visible: Option<bool>,
    title: Option<String>,
    color: Option<String>,
    point_radius: Option<f64>,
    point_markers: Option<bool>,
    point_symbol: Option<String>,
    line_width: Option<f64>,
    line_style: Option<String>,
    interpolation: Option<String>,
    connect_missing: Option<bool>,
    fill_opacity: Option<f64>,
    polar: Option<GeneralPolarOptions>,
    fill_gradient: Option<[String; 2]>,
    baseline_value: Option<f64>,
    baseline_policy: Option<String>,
    data_labels: Option<bool>,
    bar_gap: Option<f64>,
    bar_max_width: Option<f64>,
    bar_corner_radius: Option<f64>,
    heatmap_value_domain: Option<[f64; 2]>,
    heatmap_low_color: Option<String>,
    heatmap_high_color: Option<String>,
    box_fill_color: Option<String>,
    box_median_color: Option<String>,
    group_id: Option<String>,
    stack_id: Option<String>,
    stack_mode: Option<String>,
    histogram: Option<HistogramSpec>,
}

/// Browser `general_histogram_options`; omitted fields keep the engine defaults.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistogramSpec {
    bin_count: Option<usize>,
    bin_width: Option<f64>,
    normalization: Option<String>,
    cumulative: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutSpec {
    kind: String,
    data: LayoutData,
    title: Option<String>,
    color: Option<String>,
    data_labels: Option<bool>,
    visible: Option<bool>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum LayoutData {
    Funnel {
        stages: Vec<FunnelStage>,
    },
    Hierarchy {
        nodes: Vec<HierarchyNode>,
    },
    Flow {
        nodes: Vec<FlowNode>,
        links: Vec<FlowLink>,
    },
}

fn default_true() -> bool {
    true
}

/// Maps a browser snake_case option value onto the engine enum of the same name.
fn engine_enum<T: DeserializeOwned>(name: &str) -> T {
    let pascal = name
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect::<String>();
    serde_json::from_value(Value::String(pascal))
        .unwrap_or_else(|_| panic!("catalog value `{name}` has no engine equivalent"))
}

fn scale_type(name: &str) -> GeneralScaleType {
    engine_enum(match name {
        "log" => "logarithmic",
        "symlog" => "symmetric_log",
        other => other,
    })
}

fn horizontal_domain(spec: &DomainSpec) -> HorizontalDomain {
    match spec {
        DomainSpec::Continuous { scale } => HorizontalDomain::Continuous {
            scale: match scale.as_deref().unwrap_or("linear") {
                "linear" => ContinuousScaleType::Linear,
                "log" => ContinuousScaleType::Logarithmic,
                "symlog" => ContinuousScaleType::SymmetricLog,
                other => panic!("unknown continuous domain scale `{other}`"),
            },
        },
        DomainSpec::Temporal => HorizontalDomain::Temporal,
        DomainSpec::Category { scale } => HorizontalDomain::Category {
            scale: match scale.as_deref().unwrap_or("band") {
                "band" => CategoryScaleType::Band,
                "point" => CategoryScaleType::Point,
                other => panic!("unknown category domain scale `{other}`"),
            },
        },
        DomainSpec::Polar => HorizontalDomain::Polar,
        DomainSpec::Layout => HorizontalDomain::Layout,
    }
}

fn axis_options(spec: &AxisSpec) -> GeneralAxisOptions {
    let scale = scale_type(&spec.scale);
    let mut axis = GeneralAxisOptions::new(&spec.id, 0, engine_enum(&spec.dimension), scale);
    axis.position = spec.position.as_deref().map(engine_enum);
    if let Some(domain) = spec.domain.as_ref().filter(|domain| *domain != "auto") {
        let values = domain
            .as_array()
            .expect("an axis domain is \"auto\" or an array");
        axis.domain = match scale {
            GeneralScaleType::Band
            | GeneralScaleType::Point
            | GeneralScaleType::AngularCategory => {
                serde_json::from_value(serde_json::json!({ "Category": values }))
            }
            GeneralScaleType::Temporal => {
                serde_json::from_value(serde_json::json!({ "Temporal": values }))
            }
            _ => serde_json::from_value(serde_json::json!({ "Numeric": values })),
        }
        .expect("the axis domain matches its scale");
    }
    axis.reverse = spec.reverse.unwrap_or(axis.reverse);
    axis.visible = spec.visible.unwrap_or(axis.visible);
    axis.title = spec.title.clone();
    axis.tick_count = spec.tick_count;
    axis.ticks = spec.ticks.clone();
    axis.min_tick_gap = spec.min_tick_gap.unwrap_or(axis.min_tick_gap);
    axis.domain_padding = spec.domain_padding.unwrap_or(axis.domain_padding);
    axis.band_padding_inner = spec.band_padding_inner.unwrap_or(axis.band_padding_inner);
    axis.band_padding_outer = spec.band_padding_outer.unwrap_or(axis.band_padding_outer);
    axis.zero_line = spec.zero_line.unwrap_or(axis.zero_line);
    axis.grid_visible = spec.grid_visible.unwrap_or(axis.grid_visible);
    axis.line_visible = spec.line_visible.unwrap_or(axis.line_visible);
    axis
}

const ROW_FIELDS: [&str; 18] = [
    "x", "y", "low", "high", "size", "label", "color", "symbol", "x_low", "x_high", "y_low",
    "y_high", "min", "q1", "median", "q3", "max", "value",
];

/// One numeric row channel; `null` or an absent field is a missing value.
fn channel(rows: &[Map<String, Value>], field: &str) -> (Vec<f64>, Option<Vec<u8>>) {
    let values = rows
        .iter()
        .map(|row| row.get(field).and_then(Value::as_f64))
        .collect::<Vec<_>>();
    let valid = values.iter().any(Option::is_none).then(|| {
        values
            .iter()
            .map(|value| u8::from(value.is_some()))
            .collect()
    });
    (
        values
            .into_iter()
            .map(|value| value.unwrap_or(0.0))
            .collect(),
        valid,
    )
}

/// Error bounds always carry a validity mask, as the browser row converter sends them.
fn bound(rows: &[Map<String, Value>], field: &str) -> (Vec<f64>, Option<Vec<u8>>) {
    let (values, valid) = channel(rows, field);
    let valid = valid.unwrap_or_else(|| vec![1; values.len()]);
    (values, Some(valid))
}

fn categories(rows: &[Map<String, Value>], field: &str) -> (Vec<String>, Vec<u32>) {
    let mut categories = Vec::<String>::new();
    let indices = rows
        .iter()
        .map(|row| {
            let value = row
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("category rows need a string `{field}`"));
            let index = categories
                .iter()
                .position(|category| category == value)
                .unwrap_or_else(|| {
                    categories.push(value.to_owned());
                    categories.len() - 1
                });
            u32::try_from(index).expect("catalog categories fit in u32")
        })
        .collect();
    (categories, indices)
}

/// Converts browser object rows into engine columns, choosing the X shape the same way as the
/// browser package: by series kind first, then by the bound X axis scale.
fn xy_input(
    kind: GeneralSeriesKind,
    x_scale: GeneralScaleType,
    rows: &[Map<String, Value>],
) -> GeneralXyInput {
    use GeneralSeriesKind as Kind;
    for row in rows {
        if let Some(field) = row.keys().find(|key| !ROW_FIELDS.contains(&key.as_str())) {
            panic!("the gallery does not translate row field `{field}`");
        }
    }
    if kind == Kind::Histogram {
        let (values, values_valid) = channel(rows, "value");
        return GeneralXyInput::Values {
            ids: None,
            values,
            values_valid,
        };
    }
    let category_kind = matches!(
        kind,
        Kind::Column
            | Kind::HorizontalBar
            | Kind::Pie
            | Kind::Radar
            | Kind::PolarArea
            | Kind::RadialBar
            | Kind::BoxPlot
            | Kind::HeatmapGrid
    );
    let numeric_kind = matches!(kind, Kind::Scatter | Kind::Bubble);
    let range = matches!(kind, Kind::RangeArea | Kind::RangeBar);
    if !numeric_kind
        && (category_kind
            || matches!(
                x_scale,
                GeneralScaleType::Band
                    | GeneralScaleType::Point
                    | GeneralScaleType::AngularCategory
            ))
    {
        let (categories, category_indices) = categories(rows, "x");
        return match kind {
            Kind::HeatmapGrid => {
                let (y_categories, y_category_indices) = self::categories(rows, "y");
                let (value, value_valid) = channel(rows, "value");
                GeneralXyInput::HeatmapCategoryCategory {
                    ids: None,
                    x_categories: categories,
                    x_category_indices: category_indices,
                    y_categories,
                    y_category_indices,
                    value,
                    value_valid,
                }
            }
            Kind::BoxPlot => {
                let (min, min_valid) = channel(rows, "min");
                let (q1, q1_valid) = channel(rows, "q1");
                let (median, median_valid) = channel(rows, "median");
                let (q3, q3_valid) = channel(rows, "q3");
                let (max, max_valid) = channel(rows, "max");
                GeneralXyInput::BoxCategory {
                    ids: None,
                    categories,
                    category_indices,
                    min,
                    min_valid,
                    q1,
                    q1_valid,
                    median,
                    median_valid,
                    q3,
                    q3_valid,
                    max,
                    max_valid,
                }
            }
            Kind::ErrorBar => {
                let (y, y_valid) = channel(rows, "y");
                let (y_low, y_low_valid) = bound(rows, "y_low");
                let (y_high, y_high_valid) = bound(rows, "y_high");
                GeneralXyInput::ErrorCategory {
                    ids: None,
                    categories,
                    category_indices,
                    y,
                    y_valid,
                    y_low,
                    y_low_valid,
                    y_high,
                    y_high_valid,
                }
            }
            _ if range => {
                let (low, low_valid) = channel(rows, "low");
                let (high, high_valid) = channel(rows, "high");
                GeneralXyInput::RangeCategory {
                    ids: None,
                    categories,
                    category_indices,
                    low,
                    low_valid,
                    high,
                    high_valid,
                }
            }
            _ => {
                let (y, y_valid) = channel(rows, "y");
                GeneralXyInput::Category {
                    ids: None,
                    categories,
                    category_indices,
                    y,
                    y_valid,
                }
            }
        };
    }
    let (x, _) = channel(rows, "x");
    if !numeric_kind && x_scale == GeneralScaleType::Temporal {
        // Catalog timestamps are whole epoch milliseconds, exactly representable in f64.
        let x_epoch_ms = x.iter().map(|value| *value as i64).collect();
        return if range {
            let (low, low_valid) = channel(rows, "low");
            let (high, high_valid) = channel(rows, "high");
            GeneralXyInput::RangeTemporal {
                ids: None,
                x_epoch_ms,
                low,
                low_valid,
                high,
                high_valid,
            }
        } else {
            assert!(
                kind != Kind::ErrorBar,
                "temporal error bars are not in the catalog"
            );
            let (y, y_valid) = channel(rows, "y");
            GeneralXyInput::Temporal {
                ids: None,
                x_epoch_ms,
                y,
                y_valid,
            }
        };
    }
    match kind {
        Kind::Bubble => {
            let (y, y_valid) = channel(rows, "y");
            let (size, size_valid) = channel(rows, "size");
            GeneralXyInput::Bubble {
                ids: None,
                x,
                y,
                y_valid,
                size,
                size_valid,
            }
        }
        Kind::ErrorBar => {
            let (y, y_valid) = channel(rows, "y");
            let (x_low, x_low_valid) = bound(rows, "x_low");
            let (x_high, x_high_valid) = bound(rows, "x_high");
            let (y_low, y_low_valid) = bound(rows, "y_low");
            let (y_high, y_high_valid) = bound(rows, "y_high");
            GeneralXyInput::ErrorNumeric {
                ids: None,
                x,
                y,
                y_valid,
                x_low,
                x_low_valid,
                x_high,
                x_high_valid,
                y_low,
                y_low_valid,
                y_high,
                y_high_valid,
            }
        }
        _ if range => {
            let (low, low_valid) = channel(rows, "low");
            let (high, high_valid) = channel(rows, "high");
            GeneralXyInput::RangeNumeric {
                ids: None,
                x,
                low,
                low_valid,
                high,
                high_valid,
            }
        }
        _ => {
            let (y, y_valid) = channel(rows, "y");
            GeneralXyInput::Numeric {
                ids: None,
                x,
                y,
                y_valid,
            }
        }
    }
}

/// Sparse per-row text such as labels, colors, and symbols; `None` when no row sets it.
fn row_text(rows: &[Map<String, Value>], field: &str) -> Option<Vec<Option<String>>> {
    let values = rows
        .iter()
        .map(|row| row.get(field).and_then(Value::as_str).map(str::to_owned))
        .collect::<Vec<_>>();
    values.iter().any(Option::is_some).then_some(values)
}

fn series_options(
    kind: GeneralSeriesKind,
    dataset: aeris_charts_engine::GeneralDatasetId,
    spec: &SeriesSpec,
) -> GeneralSeriesOptions {
    use GeneralSeriesKind as Kind;
    let x = spec.x_axis_id.as_deref().unwrap_or("x");
    let y = spec.y_axis_id.as_deref().unwrap_or("y");
    let mut options = match kind {
        Kind::XyLine => GeneralSeriesOptions::xy_line(0, dataset, x, y),
        Kind::XyArea => GeneralSeriesOptions::xy_area(0, dataset, x, y),
        Kind::RangeArea => GeneralSeriesOptions::range_area(0, dataset, x, y),
        Kind::RangeBar => GeneralSeriesOptions::range_bar(0, dataset, x, y),
        Kind::ErrorBar => GeneralSeriesOptions::error_bar(0, dataset, x, y),
        Kind::Column => GeneralSeriesOptions::column(0, dataset, x, y),
        Kind::HorizontalBar => GeneralSeriesOptions::horizontal_bar(0, dataset, x, y),
        Kind::BoxPlot => GeneralSeriesOptions::box_plot(0, dataset, x, y),
        Kind::HeatmapGrid => GeneralSeriesOptions::heatmap_grid(0, dataset, x, y),
        Kind::Scatter => GeneralSeriesOptions::scatter(0, dataset, x, y),
        Kind::Bubble => GeneralSeriesOptions::bubble(0, dataset, x, y),
        Kind::Pie => GeneralSeriesOptions::pie(0, dataset, x, y),
        Kind::Radar => GeneralSeriesOptions::radar(0, dataset, x, y),
        Kind::RadialBar => GeneralSeriesOptions::radial_bar(0, dataset, x, y),
        Kind::PolarArea => GeneralSeriesOptions::polar_area(0, dataset, x, y),
        Kind::Histogram => GeneralSeriesOptions::histogram(0, dataset, x, y),
    };
    // Browser defaults: options the host omits take these values in `add_series`.
    options.visible = spec.visible.unwrap_or(true);
    options.title = spec.title.clone().unwrap_or_default();
    options.color = spec.color.clone();
    options.point_radius = spec.point_radius.unwrap_or(3.0);
    options.point_markers = spec.point_markers.unwrap_or(false);
    options.point_symbol = engine_enum(spec.point_symbol.as_deref().unwrap_or("circle"));
    options.line_width = spec.line_width.unwrap_or(2.0);
    options.line_style = engine_enum(spec.line_style.as_deref().unwrap_or("solid"));
    options.interpolation = engine_enum(spec.interpolation.as_deref().unwrap_or("linear"));
    options.connect_missing = spec.connect_missing.unwrap_or(false);
    options.fill_opacity = spec.fill_opacity.unwrap_or(
        if matches!(kind, Kind::Pie | Kind::PolarArea | Kind::RadialBar) {
            1.0
        } else {
            aeris_charts_engine::DEFAULT_GENERAL_FILL_OPACITY
        },
    );
    options.polar = spec.polar.unwrap_or_default();
    options.fill_gradient = spec.fill_gradient.clone();
    options.baseline_value = spec.baseline_value;
    options.baseline_policy = engine_enum(spec.baseline_policy.as_deref().unwrap_or("zero"));
    options.data_labels = spec.data_labels.unwrap_or(false);
    options.bar_gap = spec.bar_gap.unwrap_or(0.0);
    options.bar_max_width = spec.bar_max_width;
    options.bar_corner_radius = spec.bar_corner_radius.unwrap_or(0.0);
    options.heatmap_value_domain = spec.heatmap_value_domain;
    options.heatmap_low_color = spec.heatmap_low_color.clone();
    options.heatmap_high_color = spec.heatmap_high_color.clone();
    options.box_fill_color = spec.box_fill_color.clone();
    options.box_median_color = spec.box_median_color.clone();
    options.group_id = spec.group_id.clone();
    options.stack_id = spec.stack_id.clone();
    options.stack_mode = engine_enum(spec.stack_mode.as_deref().unwrap_or("normal"));
    if let Some(histogram) = &spec.histogram {
        options.histogram = GeneralHistogramOptions {
            bin_count: histogram.bin_count,
            bin_width: histogram.bin_width,
            normalization: engine_enum(histogram.normalization.as_deref().unwrap_or("count")),
            cumulative: histogram.cumulative.unwrap_or(false),
        };
    }
    options
}

/// Tags raw reference coordinates with the scale family of the axis they bind to, the same
/// normalization the browser package applies before the engine validates the reference.
fn reference_options(
    reference: &Map<String, Value>,
    scales: &HashMap<&str, GeneralScaleType>,
) -> GeneralReferenceOptions {
    let mut reference = reference.clone();
    let axis_of = |field: &str| -> &str {
        let axis_field = match field {
            "value" => "axis_id",
            "x" | "x_from" | "x_to" => "x_axis_id",
            _ => "y_axis_id",
        };
        reference
            .get(axis_field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("a reference needs `{axis_field}`"))
    };
    let mut tagged = Vec::new();
    for field in ["value", "x", "y", "x_from", "x_to", "y_from", "y_to"] {
        let Some(value) = reference.get(field) else {
            continue;
        };
        let family = match scales[axis_of(field)] {
            GeneralScaleType::Band
            | GeneralScaleType::Point
            | GeneralScaleType::AngularCategory => "category",
            GeneralScaleType::Temporal => "temporal",
            _ => "numeric",
        };
        tagged.push((field, serde_json::json!({ "type": family, "value": value })));
    }
    for (field, value) in tagged {
        reference.insert(field.to_owned(), value);
    }
    reference.insert("pane".to_owned(), Value::from(0));
    serde_json::from_value(Value::Object(reference)).expect("catalog references are valid")
}

fn build_engine(example: &Example) -> ChartEngine {
    let mut engine = ChartEngine::new_with_initial_domain(
        520.0,
        310.0,
        1.0,
        horizontal_domain(&example.horizontal_domain),
    )
    .expect("a catalog pane can be the initial pane");
    engine.set_theme(ChartTheme::Light);
    engine
        .apply_options(
            &serde_json::json!({
                "grid": {
                    "vertLines": { "visible": example.grid },
                    "horzLines": { "visible": example.grid },
                },
                "generalLegend": { "visible": true },
            })
            .to_string(),
        )
        .expect("gallery chart options");
    let mut scales = HashMap::new();
    for spec in &example.axes {
        let axis = axis_options(spec);
        scales.insert(spec.id.as_str(), axis.scale);
        engine.add_general_axis(axis).expect("catalog axis");
    }
    for spec in &example.series {
        let kind: GeneralSeriesKind = engine_enum(&spec.kind);
        let x_axis = spec.x_axis_id.as_deref().unwrap_or("x");
        let input = xy_input(kind, scales[x_axis], &spec.data);
        let dataset = engine
            .create_general_xy_dataset(input.clone())
            .expect("catalog data");
        let labels = row_text(&spec.data, "label");
        let colors = row_text(&spec.data, "color");
        let symbols = row_text(&spec.data, "symbol");
        if labels.is_some() || colors.is_some() || symbols.is_some() {
            engine
                .replace_general_xy_dataset_styled(dataset, input, labels, colors, symbols)
                .expect("catalog row styles");
        }
        engine
            .add_general_series(series_options(kind, dataset, spec))
            .unwrap_or_else(|error| panic!("{}: {error:?}", example.id));
    }
    for spec in &example.layout_series {
        let input = match &spec.data {
            LayoutData::Funnel { stages } => GeneralLayoutInput::Funnel(stages.clone()),
            LayoutData::Hierarchy { nodes } => GeneralLayoutInput::Hierarchy(nodes.clone()),
            LayoutData::Flow { nodes, links } => GeneralLayoutInput::Flow {
                nodes: nodes.clone(),
                links: links.clone(),
            },
        };
        let dataset = engine
            .create_general_layout_dataset(input)
            .expect("catalog layout data");
        let kind: GeneralLayoutSeriesKind = engine_enum(&spec.kind);
        let mut options = GeneralLayoutSeriesOptions::new(0, dataset, kind);
        options.title = spec.title.clone().unwrap_or_default();
        if let Some(color) = &spec.color {
            options.color = color.clone();
        }
        options.data_labels = spec.data_labels.unwrap_or(false);
        options.visible = spec.visible.unwrap_or(true);
        engine
            .add_general_layout_series(options)
            .expect("catalog layout series");
    }
    for reference in &example.references {
        engine
            .add_general_reference(reference_options(reference, &scales))
            .expect("catalog reference");
    }
    engine
}

struct ChartCard {
    id: ElementId,
    engine: ChartEngine,
    renderer: GpuiChartRenderer,
    frame: ChartFrame,
    axis: Vec<Prim>,
    input: GpuiChartInput,
    built_for: (f32, f32, f32),
}

impl ChartCard {
    fn new(example: &Example) -> Self {
        Self {
            id: ElementId::Name(example.id.clone().into()),
            engine: build_engine(example),
            renderer: GpuiChartRenderer::new(),
            frame: ChartFrame::default(),
            axis: Vec::new(),
            input: GpuiChartInput::default(),
            built_for: (0.0, 0.0, 0.0),
        }
    }
    fn prepare(&mut self, bounds: Bounds<gpui::Pixels>, window: &Window) {
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        let dpr = window.scale_factor();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        self.input.set_canvas_bounds(bounds);
        install_text_metrics(&mut self.engine, window);
        self.input.prepare_frame(&mut self.engine);
        let resized = self.built_for != (width, height, dpr);
        if resized && self.built_for.2 != 0.0 && self.built_for.2 != dpr {
            self.renderer.invalidate_caches();
        }
        let font = self.engine.options.get().layout.font_family.clone();
        let font = font.as_str();
        let axis_size = self.engine.axis_font_size() as f32;
        let countdown_size = self.engine.countdown_font_size() as f32;
        let measure = |size: f32| {
            move |text: &str, bold: bool| {
                f64::from(
                    measure_text(
                        window,
                        text,
                        font,
                        size,
                        if bold { 700 } else { 400 },
                        false,
                    )
                    .width,
                )
            }
        };
        self.engine.prepare_financial_frame_with_measure(
            FinancialFrameRequest {
                width: f64::from(width),
                height: f64::from(height),
                dpr: f64::from(dpr),
                force_layout: resized,
                allow_axis_shrink: resized,
                force_frame: false,
                force_axis: false,
                layout_only: false,
                fit_content: false,
                frame: &mut self.frame,
                axis_frame: None,
                axis_primitives: Some(&mut self.axis),
            },
            measure(axis_size),
            measure(countdown_size),
        );
        self.built_for = (width, height, dpr);
    }

    fn paint(&mut self, bounds: Bounds<gpui::Pixels>, window: &mut Window, cx: &mut App) {
        if self.frame.panes.is_empty() {
            return;
        }
        let viewport = AerisViewport::from_bounds(
            bounds.origin.x.into(),
            bounds.origin.y.into(),
            bounds.size.width.into(),
            bounds.size.height.into(),
        );
        let prepared =
            PreparedAerisFrame::from_engine(&self.frame, &self.engine).with_axis(&self.axis, &[]);
        if let Err(error) =
            self.renderer
                .paint_frame(&prepared, viewport, window.scale_factor(), window, cx)
        {
            eprintln!("general gallery paint failed: {error}");
        }
    }
}

impl Render for ChartCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let move_entity = cx.entity();
        let down_entity = cx.entity();
        let up_entity = cx.entity();
        let outside_entity = cx.entity();
        let hover_entity = cx.entity();
        let prepaint = cx.entity();
        let paint = cx.entity();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(rgb(0xffffff))
            .rounded_lg()
            .border_1()
            .border_color(rgb(0xdfe6ef))
            .overflow_hidden()
            .p_2()
            .child(
                div()
                    .id(self.id.clone())
                    .flex_1()
                    .min_h_0()
                    .on_hover(move |hovered, _, app| {
                        hover_entity.update(app, |card, cx| {
                            card.input.hover_changed(&mut card.engine, *hovered);
                            cx.notify();
                        });
                    })
                    .on_mouse_move(move |event, _, app| {
                        move_entity.update(app, |card, cx| {
                            card.input.mouse_move(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .on_mouse_down(MouseButton::Left, move |event, _, app| {
                        down_entity.update(app, |card, cx| {
                            card.input.mouse_down(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .on_mouse_up(MouseButton::Left, move |event, _, app| {
                        up_entity.update(app, |card, cx| {
                            card.input.mouse_up(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .on_mouse_up_out(MouseButton::Left, move |event, _, app| {
                        outside_entity.update(app, |card, cx| {
                            card.input.mouse_up(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                prepaint.update(cx, |card, _| card.prepare(bounds, window));
                                bounds
                            },
                            move |_, bounds, window, cx| {
                                paint.update(cx, |card, cx| card.paint(bounds, window, cx));
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

struct Gallery {
    cards: Vec<Entity<ChartCard>>,
}

impl Gallery {
    fn new(cx: &mut App) -> Self {
        let catalog: Catalog = serde_json::from_str(CATALOG).expect("the shared catalog parses");
        let cards = catalog
            .examples
            .iter()
            .map(|example| cx.new(|_| ChartCard::new(example)))
            .collect();
        Self { cards }
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .cards
            .chunks(2)
            .map(|pair| {
                div()
                    .flex()
                    .h(px(304.0))
                    .flex_shrink_0()
                    .gap_4()
                    .children(pair.iter().cloned())
            })
            .collect::<Vec<_>>();
        div().flex().flex_col().size_full().bg(rgb(0xf3f6fa)).child(
            div()
                .id("gallery-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .child(div().flex().flex_col().gap_4().children(rows)),
        )
    }
}
fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Aeris Charts — General gallery");
                cx.new(|cx| Gallery::new(cx))
            },
        )
        .expect("the gallery window opens");
        cx.activate(true);
    });
}
