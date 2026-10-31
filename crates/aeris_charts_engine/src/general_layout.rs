//! Typed, bounded source data for hierarchy and flow layouts.
//!
//! These inputs are intentionally separate from XY columns: a tree edge or a Sankey link is not
//! an X/Y row, and its identity and validation must survive every host boundary unchanged.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use crate::domains::HorizontalDomain;
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, ErrorCode, PaneId};
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, LineStyle, LineType, Prim, TextAlign};

const MAX_LAYOUT_DATASETS: usize = 64;
const MAX_LAYOUT_SERIES: usize = 64;
const MAX_LAYOUT_ROWS: usize = 4_096;
const MAX_FLOW_LINKS: usize = 16_384;
const MAX_LAYOUT_DEPTH: usize = 32;
const MAX_LAYOUT_TEXT_BYTES: usize = 1_048_576;
const MAX_LAYOUT_DATASET_BYTES: usize = 8 * 1_048_576;
const MAX_LAYOUT_STORE_BYTES: usize = 32 * 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GeneralLayoutDatasetId(NonZeroU32);

impl GeneralLayoutDatasetId {
    pub fn from_raw(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GeneralLayoutSeriesId(NonZeroU32);

impl GeneralLayoutSeriesId {
    pub fn from_raw(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneralLayoutHit {
    pub series: GeneralLayoutSeriesId,
    pub element: GeneralLayoutElement,
    pub row: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralLayoutElement {
    Stage,
    Node,
    Link,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct GeneralLayoutAccessibilityItem {
    pub element: GeneralLayoutElement,
    pub row: usize,
    pub id: String,
    pub label: String,
    pub value: Option<f64>,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct GeneralLayoutAccessibilitySnapshot {
    pub series: GeneralLayoutSeriesId,
    pub title: String,
    pub total: usize,
    pub offset: usize,
    pub items: Vec<GeneralLayoutAccessibilityItem>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralLayoutSeriesKind {
    Funnel,
    Treemap,
    Sankey,
    Sunburst,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeneralLayoutSeriesOptions {
    pub pane: usize,
    pub dataset: GeneralLayoutDatasetId,
    pub kind: GeneralLayoutSeriesKind,
    pub title: String,
    pub color: String,
    #[serde(default)]
    pub data_labels: bool,
    #[serde(default = "layout_visible_default")]
    pub visible: bool,
}

pub(crate) fn layout_visible_default() -> bool {
    true
}

impl GeneralLayoutSeriesOptions {
    pub fn new(
        pane: usize,
        dataset: GeneralLayoutDatasetId,
        kind: GeneralLayoutSeriesKind,
    ) -> Self {
        Self {
            pane,
            dataset,
            kind,
            title: String::new(),
            color: "#4682b4".into(),
            data_labels: false,
            visible: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FunnelStage {
    pub id: String,
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HierarchyNode {
    pub id: String,
    pub parent: Option<String>,
    pub label: String,
    /// Leaf weight. Internal nodes derive their weight from descendants.
    pub value: Option<f64>,
    /// Signed leaf metric, such as a percent change, that colors a treemap cell: positive values
    /// shade toward the up color and negative toward the down color by magnitude.
    #[serde(default)]
    pub color_value: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FlowLink {
    pub source: String,
    pub target: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GeneralLayoutInput {
    Funnel(Vec<FunnelStage>),
    Hierarchy(Vec<HierarchyNode>),
    Flow {
        nodes: Vec<FlowNode>,
        links: Vec<FlowLink>,
    },
}

impl GeneralLayoutInput {
    fn funnel_max(&self) -> f64 {
        match self {
            Self::Funnel(stages) => stages
                .iter()
                .fold(0.0_f64, |max, stage| max.max(stage.value)),
            _ => 0.0,
        }
    }
    fn semantic_values(&self) -> Vec<f64> {
        match self {
            Self::Funnel(stages) => stages.iter().map(|stage| stage.value).collect(),
            Self::Hierarchy(nodes) => {
                let maximum = nodes
                    .iter()
                    .filter_map(|node| node.value)
                    .fold(0.0_f64, f64::max);
                let weights = hierarchy_layout_tree(nodes).map(|(_, _, weights)| weights);
                nodes
                    .iter()
                    .enumerate()
                    .map(|(row, node)| {
                        node.value.unwrap_or_else(|| {
                            weights
                                .as_ref()
                                .map_or(0.0, |weights| weights[row] * maximum)
                                .min(f64::MAX)
                        })
                    })
                    .collect()
            }
            Self::Flow { nodes, links } => {
                let ids: HashMap<&str, usize> = nodes
                    .iter()
                    .enumerate()
                    .map(|(row, node)| (node.id.as_str(), row))
                    .collect();
                let mut totals = vec![(0.0_f64, 0.0_f64); nodes.len()];
                for link in links {
                    let source = ids[link.source.as_str()];
                    let target = ids[link.target.as_str()];
                    totals[source].1 = (totals[source].1 + link.value).min(f64::MAX);
                    totals[target].0 = (totals[target].0 + link.value).min(f64::MAX);
                }
                totals
                    .into_iter()
                    .map(|(incoming, outgoing)| incoming.max(outgoing))
                    .chain(links.iter().map(|link| link.value))
                    .collect()
            }
        }
    }

    fn semantic_value_count(&self) -> usize {
        match self {
            Self::Funnel(stages) => stages.len(),
            Self::Hierarchy(nodes) => nodes.len(),
            Self::Flow { nodes, links } => nodes.len() + links.len(),
        }
    }
    fn validate(&self) -> Result<(), ChartError> {
        match self {
            Self::Funnel(stages) => {
                validate_rows(stages.len())?;
                let mut ids = HashSet::with_capacity(stages.len());
                let mut bytes = 0usize;
                for stage in stages {
                    validate_id(&stage.id, &mut ids)?;
                    add_text_bytes(&mut bytes, &stage.id)?;
                    add_text_bytes(&mut bytes, &stage.label)?;
                    validate_value(stage.value)?;
                }
            }
            Self::Hierarchy(nodes) => validate_hierarchy(nodes)?,
            Self::Flow { nodes, links } => validate_flow(nodes, links)?,
        }
        if self.estimated_bytes() + self.semantic_value_count() * std::mem::size_of::<f64>()
            > MAX_LAYOUT_DATASET_BYTES
        {
            return Err(resource_limit(
                "layout dataset retained capacity exceeds limit",
            ));
        }
        Ok(())
    }

    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Funnel(stages) => {
                stages.capacity() * std::mem::size_of::<FunnelStage>()
                    + stages
                        .iter()
                        .map(|row| row.id.capacity() + row.label.capacity())
                        .sum::<usize>()
            }
            Self::Hierarchy(nodes) => {
                nodes.capacity() * std::mem::size_of::<HierarchyNode>()
                    + nodes
                        .iter()
                        .map(|row| {
                            row.id.capacity()
                                + row.label.capacity()
                                + row.parent.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
            }
            Self::Flow { nodes, links } => {
                nodes.capacity() * std::mem::size_of::<FlowNode>()
                    + links.capacity() * std::mem::size_of::<FlowLink>()
                    + nodes
                        .iter()
                        .map(|row| row.id.capacity() + row.label.capacity())
                        .sum::<usize>()
                    + links
                        .iter()
                        .map(|row| row.source.capacity() + row.target.capacity())
                        .sum::<usize>()
            }
        }
    }
}

struct LayoutDatasetEntry {
    id: GeneralLayoutDatasetId,
    input: GeneralLayoutInput,
    funnel_max: f64,
    semantic_values: Vec<f64>,
    transition: Option<LayoutTransition>,
}

struct LayoutTransition {
    from: GeneralLayoutInput,
    presentation: GeneralLayoutInput,
    presentation_funnel_max: f64,
    started_ms: f64,
}

impl LayoutDatasetEntry {
    fn display_input(&self) -> &GeneralLayoutInput {
        self.transition
            .as_ref()
            .map_or(&self.input, |motion| &motion.presentation)
    }

    fn display_funnel_max(&self) -> f64 {
        self.transition
            .as_ref()
            .map_or(self.funnel_max, |motion| motion.presentation_funnel_max)
    }
}

impl LayoutTransition {
    fn estimated_bytes(&self) -> usize {
        self.from
            .estimated_bytes()
            .saturating_add(self.presentation.estimated_bytes())
    }
}

fn layout_transition_compatible(from: &GeneralLayoutInput, to: &GeneralLayoutInput) -> bool {
    match (from, to) {
        (GeneralLayoutInput::Funnel(a), GeneralLayoutInput::Funnel(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(left, right)| left.id == right.id)
        }
        (GeneralLayoutInput::Hierarchy(a), GeneralLayoutInput::Hierarchy(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(left, right)| {
                    left.id == right.id
                        && left.parent == right.parent
                        && left.value.is_some() == right.value.is_some()
                })
        }
        (
            GeneralLayoutInput::Flow {
                nodes: an,
                links: al,
            },
            GeneralLayoutInput::Flow {
                nodes: bn,
                links: bl,
            },
        ) => {
            an.len() == bn.len()
                && al.len() == bl.len()
                && an.iter().zip(bn).all(|(left, right)| left.id == right.id)
                && al
                    .iter()
                    .zip(bl)
                    .all(|(left, right)| left.source == right.source && left.target == right.target)
        }
        _ => false,
    }
}

fn blend_layout_input(
    from: &GeneralLayoutInput,
    to: &GeneralLayoutInput,
    out: &mut GeneralLayoutInput,
    progress: f64,
) {
    let blend = |a: f64, b: f64| (a * (1.0 - progress) + b * progress).min(f64::MAX);
    match (from, to, out) {
        (
            GeneralLayoutInput::Funnel(a),
            GeneralLayoutInput::Funnel(b),
            GeneralLayoutInput::Funnel(out),
        ) => {
            for ((left, right), item) in a.iter().zip(b).zip(out) {
                item.value = blend(left.value, right.value);
            }
        }
        (
            GeneralLayoutInput::Hierarchy(a),
            GeneralLayoutInput::Hierarchy(b),
            GeneralLayoutInput::Hierarchy(out),
        ) => {
            for ((left, right), item) in a.iter().zip(b).zip(out) {
                if let (Some(x), Some(y)) = (left.value, right.value) {
                    item.value = Some(blend(x, y));
                }
            }
        }
        (
            GeneralLayoutInput::Flow { links: a, .. },
            GeneralLayoutInput::Flow { links: b, .. },
            GeneralLayoutInput::Flow { links: out, .. },
        ) => {
            for ((left, right), item) in a.iter().zip(b).zip(out) {
                item.value = blend(left.value, right.value);
            }
        }
        _ => unreachable!("layout transition topology validated before interpolation"),
    }
}

pub(crate) struct GeneralLayoutStore {
    entries: Vec<LayoutDatasetEntry>,
    next_id: u32,
    series: Vec<LayoutSeriesEntry>,
    next_series_id: u32,
    hovered: Option<GeneralLayoutHit>,
    selected: Option<GeneralLayoutHit>,
    focused: Option<GeneralLayoutHit>,
    focus_color: Color,
    focus_size: f64,
}

struct LayoutSeriesEntry {
    id: GeneralLayoutSeriesId,
    pane_id: PaneId,
    options: GeneralLayoutSeriesOptions,
    color: Color,
    treemap: RefCell<Option<TreemapCache>>,
    sankey: RefCell<Option<SankeyCache>>,
    sunburst: RefCell<Option<SunburstCache>>,
}

struct SunburstCache {
    plot: GeneralPlotRect,
    shapes: Vec<SunburstShape>,
}

#[derive(Clone, Copy, Debug)]
struct SunburstShape {
    row: usize,
    cx: f64,
    cy: f64,
    inner: f64,
    outer: f64,
    start: f64,
    end: f64,
}

impl SunburstShape {
    fn segments(self) -> usize {
        (((self.end - self.start) * 72.0 / std::f64::consts::TAU).ceil() as usize).max(1)
    }

    fn arc_point(self, radius: f64, index: usize) -> (f64, f64) {
        let angle = self.start + (self.end - self.start) * index as f64 / self.segments() as f64;
        (
            self.cx + radius * angle.cos(),
            self.cy + radius * angle.sin(),
        )
    }

    fn segment_vertices(self, index: usize) -> [(f64, f64); 4] {
        let count = self.segments() as f64;
        let a0 = self.start + (self.end - self.start) * index as f64 / count;
        let a1 = self.start + (self.end - self.start) * (index + 1) as f64 / count;
        [
            (
                self.cx + self.inner * a0.cos(),
                self.cy + self.inner * a0.sin(),
            ),
            (
                self.cx + self.outer * a0.cos(),
                self.cy + self.outer * a0.sin(),
            ),
            (
                self.cx + self.outer * a1.cos(),
                self.cy + self.outer * a1.sin(),
            ),
            (
                self.cx + self.inner * a1.cos(),
                self.cy + self.inner * a1.sin(),
            ),
        ]
    }

    fn contains(self, x: f64, y: f64) -> bool {
        let dx = x - self.cx;
        let dy = y - self.cy;
        let distance2 = dx * dx + dy * dy;
        if distance2 < self.inner * self.inner || distance2 > self.outer * self.outer {
            return false;
        }
        for segment in 0..self.segments() {
            let [a, b, c, d] = self.segment_vertices(segment);
            if point_in_triangle((x, y), a, b, c)
                || (self.inner > 0.0 && point_in_triangle((x, y), a, c, d))
            {
                return true;
            }
        }
        false
    }
}

fn point_in_triangle(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let cross = |u: (f64, f64), v: (f64, f64), w: (f64, f64)| {
        (v.0 - u.0) * (w.1 - u.1) - (v.1 - u.1) * (w.0 - u.0)
    };
    let first = cross(a, b, p);
    let second = cross(b, c, p);
    let third = cross(c, a, p);
    (first >= 0.0 && second >= 0.0 && third >= 0.0)
        || (first <= 0.0 && second <= 0.0 && third <= 0.0)
}

struct SankeyCache {
    plot: GeneralPlotRect,
    column_count: usize,
    /// Horizontal distance between adjacent node columns.
    column_stride: f64,
    nodes: Vec<TreemapShape>,
    /// Column of each entry in `nodes`.
    node_columns: Vec<usize>,
    links: Vec<SankeyLinkShape>,
}

/// One Sankey link: the Recharts cubic `M x0,y0 C c0,y0 c1,y1 x1,y1` path stroked `thickness`
/// wide. Frame meshes and hit tests both use the flattened ribbon from [`Self::edge`].
#[derive(Clone, Copy, Debug)]
struct SankeyLinkShape {
    row: usize,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    thickness: f64,
}

impl SankeyLinkShape {
    fn controls(self) -> (f64, f64) {
        let span = self.x1 - self.x0;
        (
            self.x0 + span * SANKEY_LINK_CURVATURE,
            self.x0 + span * (1.0 - SANKEY_LINK_CURVATURE),
        )
    }

    fn point(self, t: f64) -> (f64, f64) {
        let (c0, c1) = self.controls();
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        (
            a * self.x0 + b * c0 + c * c1 + d * self.x1,
            (a + b) * self.y0 + (c + d) * self.y1,
        )
    }

    /// Bounded flattening: about one segment per 6 px of span, at most
    /// `SANKEY_LINK_SEGMENTS`.
    fn segments(self) -> usize {
        ((self.x1 - self.x0) / 6.0)
            .ceil()
            .clamp(4.0, SANKEY_LINK_SEGMENTS as f64) as usize
    }

    /// Upper and lower ribbon edge at sample `index` of `segments`: the curve offset by half the
    /// link width along its normal, which is how SVG strokes the Recharts path.
    fn edge(self, index: usize, segments: usize) -> ((f64, f64), (f64, f64)) {
        let t = index as f64 / segments as f64;
        let (x, y) = self.point(t);
        let (c0, c1) = self.controls();
        let u = 1.0 - t;
        let dx =
            3.0 * u * u * (c0 - self.x0) + 6.0 * u * t * (c1 - c0) + 3.0 * t * t * (self.x1 - c1);
        let dy = 6.0 * u * t * (self.y1 - self.y0);
        let length = dx.hypot(dy);
        let (nx, ny) = if length > 0.0 {
            (-dy / length, dx / length)
        } else {
            (0.0, 1.0)
        };
        let half = self.thickness / 2.0;
        (
            (x - nx * half, y - ny * half),
            (x + nx * half, y + ny * half),
        )
    }

    fn contains(self, x: f64, y: f64) -> bool {
        if self.thickness <= 0.0 || self.x1 <= self.x0 {
            return false;
        }
        let half = self.thickness / 2.0;
        if x < self.x0 - half
            || x > self.x1 + half
            || y < self.y0.min(self.y1) - half
            || y > self.y0.max(self.y1) + half
        {
            return false;
        }
        let segments = self.segments();
        let mut previous = self.edge(0, segments);
        for index in 1..=segments {
            let next = self.edge(index, segments);
            let ((top0, bottom0), (top1, bottom1)) = (previous, next);
            if point_in_triangle((x, y), top0, top1, bottom1)
                || point_in_triangle((x, y), top0, bottom1, bottom0)
            {
                return true;
            }
            previous = next;
        }
        false
    }
}

struct TreemapCache {
    plot: GeneralPlotRect,
    shapes: Vec<TreemapShape>,
    bounds: Vec<Option<GeneralPlotRect>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TreemapShape {
    row: usize,
    rect: GeneralPlotRect,
}

impl TreemapShape {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.rect.x
            && x < self.rect.x + self.rect.width
            && y >= self.rect.y
            && y < self.rect.y + self.rect.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FunnelShape {
    row: usize,
    center_x: f64,
    top_y: f64,
    bottom_y: f64,
    top_half_width: f64,
    bottom_half_width: f64,
}

impl FunnelShape {
    fn vertices(self) -> [(f64, f64); 4] {
        [
            (self.center_x - self.top_half_width, self.top_y),
            (self.center_x + self.top_half_width, self.top_y),
            (self.center_x + self.bottom_half_width, self.bottom_y),
            (self.center_x - self.bottom_half_width, self.bottom_y),
        ]
    }

    fn contains(self, x: f64, y: f64) -> bool {
        if y < self.top_y || y >= self.bottom_y {
            return false;
        }
        let t = (y - self.top_y) / (self.bottom_y - self.top_y);
        let half_width = self.top_half_width + (self.bottom_half_width - self.top_half_width) * t;
        half_width > 0.0 && (x - self.center_x).abs() <= half_width
    }
}

fn visit_funnel_shapes(
    stages: &[FunnelStage],
    maximum: f64,
    plot: crate::general_axes::GeneralPlotRect,
    mut visit: impl FnMut(FunnelShape),
) {
    if stages.is_empty() || plot.width <= 0.0 || plot.height <= 0.0 {
        return;
    }
    if maximum == 0.0 {
        return;
    }
    for row in 0..stages.len() {
        if let Some(shape) = funnel_shape_at(stages, maximum, plot, row) {
            visit(shape);
        }
    }
}

fn funnel_shape_at(
    stages: &[FunnelStage],
    maximum: f64,
    plot: crate::general_axes::GeneralPlotRect,
    row: usize,
) -> Option<FunnelShape> {
    if maximum <= 0.0 || plot.width <= 0.0 || plot.height <= 0.0 {
        return None;
    }
    let stage = stages.get(row)?;
    let next = stages.get(row + 1).map_or(0.0, |stage| stage.value);
    if stage.value == 0.0 && next == 0.0 {
        return None;
    }
    let row_height = plot.height / stages.len() as f64;
    Some(FunnelShape {
        row,
        center_x: plot.x + plot.width / 2.0,
        top_y: plot.y + row as f64 * row_height,
        bottom_y: plot.y + (row + 1) as f64 * row_height,
        top_half_width: (stage.value / maximum) * plot.width / 2.0,
        bottom_half_width: (next / maximum) * plot.width / 2.0,
    })
}

fn treemap_cache(nodes: &[HierarchyNode], plot: GeneralPlotRect) -> TreemapCache {
    let mut cache = TreemapCache {
        plot,
        shapes: Vec::new(),
        bounds: Vec::new(),
    };
    if nodes.is_empty() || plot.width <= 0.0 || plot.height <= 0.0 {
        return cache;
    }
    let Some((root, children, weights)) = hierarchy_layout_tree(nodes) else {
        return cache;
    };
    cache.shapes = Vec::with_capacity(nodes.len());
    cache.bounds = vec![None; nodes.len()];
    layout_treemap_node(
        root,
        plot,
        &children,
        &weights,
        &mut cache.shapes,
        &mut cache.bounds,
    );
    cache
}

/// Recharts `Treemap` default `aspectRatio`, the golden ratio `0.5 * (1 + sqrt(5))`.
const TREEMAP_ASPECT_RATIO: f64 = std::f64::consts::GOLDEN_RATIO;
/// Recharts' default node `stroke="#fff"` at the SVG default 1 px width.
const TREEMAP_STROKE: Color = Color(0xFFFF_FFFF);
const TREEMAP_UP: Color = Color(0x0899_81FF);
const TREEMAP_DOWN: Color = Color(0xF236_45FF);
const TREEMAP_FLAT: Color = Color(0x4246_4FFF);

/// Largest leaf `color_value` magnitude, or `None` when no leaf carries one.
fn treemap_color_range(nodes: &[HierarchyNode]) -> Option<f64> {
    nodes
        .iter()
        .filter_map(|node| node.color_value)
        .map(f64::abs)
        .reduce(f64::max)
}

/// A leaf's fill. By default every cell uses the one series color, like Recharts' flat
/// `fill`; a leaf's signed `color_value` opts into shading from the flat color toward up or down
/// by magnitude.
fn treemap_cell_color(
    base: Color,
    nodes: &[HierarchyNode],
    row: usize,
    color_range: Option<f64>,
) -> Color {
    let (Some(range), Some(value)) = (
        color_range,
        nodes.get(row).and_then(|node| node.color_value),
    ) else {
        return base;
    };
    if value == 0.0 || range <= 0.0 {
        return TREEMAP_FLAT;
    }
    let target = if value > 0.0 {
        TREEMAP_UP
    } else {
        TREEMAP_DOWN
    };
    let amount = 0.3 + 0.7 * (value.abs() / range).clamp(0.0, 1.0);
    let mix = |from: u8, to: u8| {
        (f64::from(from) + (f64::from(to) - f64::from(from)) * amount).round() as u8
    };
    Color::rgba(
        mix(TREEMAP_FLAT.r(), target.r()),
        mix(TREEMAP_FLAT.g(), target.g()),
        mix(TREEMAP_FLAT.b(), target.b()),
        255,
    )
}

#[cfg(test)]
fn treemap_shapes(nodes: &[HierarchyNode], plot: GeneralPlotRect) -> Vec<TreemapShape> {
    treemap_cache(nodes, plot).shapes
}

fn hierarchy_layout_tree(nodes: &[HierarchyNode]) -> Option<(usize, Vec<Vec<usize>>, Vec<f64>)> {
    if nodes.is_empty() {
        return None;
    }
    let ids: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();
    let mut children = vec![Vec::new(); nodes.len()];
    let mut root = 0;
    let mut maximum = 0.0_f64;
    for (index, node) in nodes.iter().enumerate() {
        if let Some(parent) = &node.parent {
            children[ids[parent.as_str()]].push(index);
        } else {
            root = index;
        }
        maximum = maximum.max(node.value.unwrap_or(0.0));
    }
    if maximum == 0.0 {
        return None;
    }
    // Normalization keeps descendant totals finite even when leaf values approach f64::MAX.
    let mut weights = vec![0.0; nodes.len()];
    fn weight(
        index: usize,
        nodes: &[HierarchyNode],
        children: &[Vec<usize>],
        weights: &mut [f64],
        maximum: f64,
    ) -> f64 {
        let value = if children[index].is_empty() {
            nodes[index].value.unwrap_or(0.0) / maximum
        } else {
            children[index]
                .iter()
                .map(|&child| weight(child, nodes, children, weights, maximum))
                .sum()
        };
        weights[index] = value;
        value
    }
    weight(root, nodes, &children, &mut weights, maximum);
    Some((root, children, weights))
}

/// Recharts `squarify` (pinned 3.10.1 `Treemap`, `nodeInset` and `nodeGap` 0): children keep
/// data order and join the current row while its worst aspect ratio does not grow. A row is laid
/// along the short side of the remaining rectangle with Recharts' whole-pixel rounding, its last
/// cell absorbs the rounding remainder, and the final row is flush with the remaining space.
fn layout_treemap_node(
    node: usize,
    rect: GeneralPlotRect,
    children: &[Vec<usize>],
    weights: &[f64],
    shapes: &mut Vec<TreemapShape>,
    bounds: &mut [Option<GeneralPlotRect>],
) {
    if rect.width <= 0.0 || rect.height <= 0.0 || weights[node] <= 0.0 {
        return;
    }
    bounds[node] = Some(rect);
    if children[node].is_empty() {
        shapes.push(TreemapShape { row: node, rect });
        return;
    }
    let scale = rect.width * rect.height / weights[node];
    let area = |child: usize| weights[child] * scale;
    let active: Vec<usize> = children[node]
        .iter()
        .copied()
        .filter(|&child| weights[child] > 0.0)
        .collect();
    let mut placed: Vec<(usize, GeneralPlotRect)> = Vec::with_capacity(active.len());
    let mut row: Vec<usize> = Vec::with_capacity(active.len());
    let mut row_area = 0.0;
    let mut best = f64::INFINITY;
    let mut remaining = rect;
    let mut size = remaining.width.min(remaining.height);
    let mut index = 0;
    while index < active.len() {
        let child = active[index];
        row.push(child);
        row_area += area(child);
        let score = treemap_worst_score(&row, row_area, size, &area);
        if score <= best || row.len() == 1 {
            index += 1;
            best = score;
        } else {
            row.pop();
            row_area -= area(child);
            remaining =
                treemap_position_row(&row, row_area, size, remaining, false, &area, &mut placed);
            size = remaining.width.min(remaining.height);
            row.clear();
            row_area = 0.0;
            best = f64::INFINITY;
        }
    }
    if !row.is_empty() {
        treemap_position_row(&row, row_area, size, remaining, true, &area, &mut placed);
    }
    for (child, child_rect) in placed {
        layout_treemap_node(child, child_rect, children, weights, shapes, bounds);
    }
}

/// Recharts `getWorstScore`.
fn treemap_worst_score(
    row: &[usize],
    row_area: f64,
    size: f64,
    area: &impl Fn(usize) -> f64,
) -> f64 {
    let parent_area = size * size;
    let row_area = row_area * row_area;
    if row_area == 0.0 {
        return f64::INFINITY;
    }
    let (min, max) = row
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(min, max), &child| {
            (min.min(area(child)), max.max(area(child)))
        });
    (parent_area * max * TREEMAP_ASPECT_RATIO / row_area)
        .max(row_area / (parent_area * min * TREEMAP_ASPECT_RATIO))
}

/// Recharts `position`: a row spans the short side `size` of `rect` and the rest of `rect` is
/// returned.
fn treemap_position_row(
    row: &[usize],
    row_area: f64,
    size: f64,
    rect: GeneralPlotRect,
    flush: bool,
    area: &impl Fn(usize) -> f64,
    placed: &mut Vec<(usize, GeneralPlotRect)>,
) -> GeneralPlotRect {
    let horizontal = rect.width <= rect.height;
    let (along, across) = if horizontal {
        (rect.width, rect.height)
    } else {
        (rect.height, rect.width)
    };
    let mut thickness = if size > 0.0 {
        (row_area / size).round()
    } else {
        0.0
    };
    if flush || thickness > across {
        thickness = across;
    }
    let start = if horizontal { rect.x } else { rect.y };
    let end = start + along;
    let mut cursor = start;
    for (position, &child) in row.iter().enumerate() {
        let mut length = if thickness > 0.0 {
            (area(child) / thickness).round()
        } else {
            0.0
        }
        .min(end - cursor);
        if position + 1 == row.len() {
            length = end - cursor;
        }
        placed.push((
            child,
            if horizontal {
                GeneralPlotRect {
                    x: cursor,
                    y: rect.y,
                    width: length,
                    height: thickness,
                }
            } else {
                GeneralPlotRect {
                    x: rect.x,
                    y: cursor,
                    width: thickness,
                    height: length,
                }
            },
        ));
        cursor += length;
    }
    if horizontal {
        GeneralPlotRect {
            y: rect.y + thickness,
            height: rect.height - thickness,
            ..rect
        }
    } else {
        GeneralPlotRect {
            x: rect.x + thickness,
            width: rect.width - thickness,
            ..rect
        }
    }
}

impl LayoutSeriesEntry {
    fn with_treemap_cache<R>(
        &self,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        f: impl FnOnce(&TreemapCache) -> R,
    ) -> R {
        let mut cache = self.treemap.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(treemap_cache(nodes, plot));
        }
        f(cache.as_ref().expect("treemap cache initialized"))
    }

    fn with_treemap<R>(
        &self,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        f: impl FnOnce(&[TreemapShape]) -> R,
    ) -> R {
        self.with_treemap_cache(nodes, plot, |cache| f(&cache.shapes))
    }
}

/// Recharts `SunburstChart` defaults (pinned 3.10.1 `defaultSunburstChartProps`): a fixed
/// center hole, a gap between rings, and a white sector stroke as wide as `padding`.
const SUNBURST_INNER_RADIUS: f64 = 50.0;
const SUNBURST_RING_PADDING: f64 = 2.0;
const SUNBURST_STROKE_WIDTH: f64 = 2.0;
const SUNBURST_STROKE: Color = Color(0xFFFF_FFFF);
/// Recharts' default sector `textOptions.fontSize` of `.75rem`.
const SUNBURST_LABEL_SIZE: f64 = 12.0;

/// Recharts sector labels print the node's own `value`; internal nodes without one stay blank.
fn layout_value_text(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value}"))
}

fn sunburst_shapes(nodes: &[HierarchyNode], plot: GeneralPlotRect) -> SunburstCache {
    let mut cache = SunburstCache {
        plot,
        shapes: Vec::new(),
    };
    if plot.width <= 0.0 || plot.height <= 0.0 {
        return cache;
    }
    let Some((root, children, weights)) = hierarchy_layout_tree(nodes) else {
        return cache;
    };
    let mut depths = vec![0usize; nodes.len()];
    fn assign_depth(node: usize, children: &[Vec<usize>], depths: &mut [usize]) {
        for &child in &children[node] {
            depths[child] = depths[node] + 1;
            assign_depth(child, children, depths);
        }
    }
    assign_depth(root, &children, &mut depths);
    // Recharts counts the root as a level although it paints only descendants, so the outer
    // ring stops one ring short of the outer radius.
    let levels = depths.iter().copied().max().unwrap_or(0).max(1) + 1;
    let outer = plot.width.min(plot.height) / 2.0;
    // The fixed hole would swallow a very small chart; cap it at half the radius.
    let inner = SUNBURST_INNER_RADIUS.min(outer * 0.5);
    let ring = (outer - inner) / levels as f64;
    if ring <= 0.0 {
        return cache;
    }
    let center = (plot.x + plot.width / 2.0, plot.y + plot.height / 2.0);
    struct SectorTree<'a> {
        root: usize,
        children: &'a [Vec<usize>],
        weights: &'a [f64],
        depths: &'a [usize],
        center: (f64, f64),
        inner: f64,
        ring: f64,
    }
    impl SectorTree<'_> {
        /// `start..end` are Recharts angles: radians counterclockwise from three o'clock.
        fn visit(&self, node: usize, start: f64, end: f64, shapes: &mut Vec<SunburstShape>) {
            if self.weights[node] <= 0.0 || end <= start {
                return;
            }
            if node != self.root || self.children[node].is_empty() {
                let depth = self.depths[node].max(1);
                let inner = self.inner + (depth - 1) as f64 * (self.ring + SUNBURST_RING_PADDING);
                // Screen Y points down, so a counterclockwise span maps to negated angles.
                shapes.push(SunburstShape {
                    row: node,
                    cx: self.center.0,
                    cy: self.center.1,
                    inner,
                    outer: inner + self.ring,
                    start: -end,
                    end: -start,
                });
            }
            let mut cursor = start;
            let last_positive = self.children[node]
                .iter()
                .rposition(|&child| self.weights[child] > 0.0);
            for (position, &child) in self.children[node].iter().enumerate() {
                let child_end = if self.weights[child] <= 0.0 {
                    cursor
                } else if Some(position) == last_positive {
                    end
                } else {
                    cursor + (end - start) * self.weights[child] / self.weights[node]
                };
                self.visit(child, cursor, child_end, shapes);
                cursor = child_end;
            }
        }
    }
    SectorTree {
        root,
        children: &children,
        weights: &weights,
        depths: &depths,
        center,
        inner,
        ring,
    }
    .visit(root, 0.0, std::f64::consts::TAU, &mut cache.shapes);
    cache
}

impl LayoutSeriesEntry {
    fn with_sunburst<R>(
        &self,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        f: impl FnOnce(&SunburstCache) -> R,
    ) -> R {
        let mut cache = self.sunburst.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(sunburst_shapes(nodes, plot));
        }
        f(cache.as_ref().expect("sunburst cache initialized"))
    }
}

/// Recharts `Sankey` defaults (pinned 3.10.1 `sankeyDefaultProps`).
const SANKEY_MARGIN: f64 = 5.0;
const SANKEY_NODE_WIDTH: f64 = 10.0;
const SANKEY_NODE_PADDING: f64 = 10.0;
const SANKEY_ITERATIONS: usize = 32;
const SANKEY_LINK_CURVATURE: f64 = 0.5;
/// Upper bound of the flattened segments per link ribbon.
const SANKEY_LINK_SEGMENTS: usize = 24;
/// Recharts' default node `fill="#0088fe"` with `fillOpacity="0.8"`, kept apart from the link
/// color.
const SANKEY_NODE_FILL: Color = Color(0x0088_FECC);
/// Recharts' default link `strokeOpacity="0.2"`.
const SANKEY_LINK_ALPHA: u8 = 51;

/// Recharts `computeData` with `align` and `verticalAlign` `justify`, `sort` on: longest-path
/// columns with sinks pushed to the last column, a shared value-to-pixel ratio, 32 relaxation
/// passes with collision resolution, link offsets ordered by the far node, and the
/// skipped-column obstacle pass.
fn sankey_shapes(nodes: &[FlowNode], links: &[FlowLink], plot: GeneralPlotRect) -> SankeyCache {
    let mut cache = SankeyCache {
        plot,
        column_count: 0,
        column_stride: 0.0,
        nodes: Vec::new(),
        node_columns: Vec::new(),
        links: Vec::new(),
    };
    let width = plot.width - 2.0 * SANKEY_MARGIN;
    let height = plot.height - 2.0 * SANKEY_MARGIN;
    if nodes.is_empty() || links.is_empty() || width <= 0.0 || height <= 0.0 {
        return cache;
    }
    let ids: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();
    let maximum = links.iter().fold(0.0_f64, |max, link| max.max(link.value));
    if maximum == 0.0 {
        return cache;
    }
    // Normalizing by the largest link keeps every sum finite near f64::MAX; the shared
    // value-to-pixel ratio absorbs the scale, so pixel geometry is unchanged.
    let values: Vec<f64> = links.iter().map(|link| link.value / maximum).collect();
    let ends: Vec<(usize, usize)> = links
        .iter()
        .map(|link| (ids[link.source.as_str()], ids[link.target.as_str()]))
        .collect();
    let count = nodes.len();
    let mut incoming = vec![Vec::new(); count];
    let mut outgoing = vec![Vec::new(); count];
    for (index, &(source, target)) in ends.iter().enumerate() {
        outgoing[source].push(index);
        incoming[target].push(index);
    }
    let sum = |links: &[usize]| links.iter().map(|&link| values[link]).sum::<f64>();
    let node_values: Vec<f64> = (0..count)
        .map(|node| sum(&incoming[node]).max(sum(&outgoing[node])))
        .collect();

    let mut depth = vec![0usize; count];
    let mut pending: Vec<usize> = incoming.iter().map(Vec::len).collect();
    let mut ready: std::collections::VecDeque<usize> =
        (0..count).filter(|&node| pending[node] == 0).collect();
    while let Some(node) = ready.pop_front() {
        for &link in &outgoing[node] {
            let target = ends[link].1;
            depth[target] = depth[target].max(depth[node] + 1);
            pending[target] -= 1;
            if pending[target] == 0 {
                ready.push_back(target);
            }
        }
    }
    let max_depth = depth.iter().copied().max().unwrap_or(0);
    if max_depth == 0 {
        return cache;
    }
    for node in 0..count {
        if outgoing[node].is_empty() {
            depth[node] = max_depth;
        }
    }
    let stride = (width - SANKEY_NODE_WIDTH) / max_depth as f64;
    let x: Vec<f64> = depth.iter().map(|&depth| depth as f64 * stride).collect();
    let mut columns = vec![Vec::new(); max_depth + 1];
    for node in 0..count {
        columns[depth[node]].push(node);
    }
    let ratio = columns
        .iter()
        .filter(|column| !column.is_empty())
        .map(|column| {
            let total = column.iter().map(|&node| node_values[node]).sum::<f64>();
            if total == 0.0 {
                f64::INFINITY
            } else {
                (height - (column.len() - 1) as f64 * SANKEY_NODE_PADDING) / total
            }
        })
        .fold(f64::INFINITY, f64::min);
    let ratio = if ratio.is_finite() {
        ratio.max(0.0)
    } else {
        0.0
    };
    let dy: Vec<f64> = node_values.iter().map(|value| value * ratio).collect();
    let link_dy: Vec<f64> = values.iter().map(|value| value * ratio).collect();
    let mut y = vec![0.0; count];
    for column in &columns {
        for (position, &node) in column.iter().enumerate() {
            y[node] = position as f64;
        }
    }

    sankey_resolve_collisions(&mut columns, &mut y, &dy, height);
    let mut alpha = 1.0;
    for _ in 0..SANKEY_ITERATIONS {
        alpha *= 0.99;
        for column in columns.iter().rev() {
            for &node in column {
                if outgoing[node].is_empty() {
                    continue;
                }
                let total = sum(&outgoing[node]);
                let center = y[node] + dy[node] / 2.0;
                let target = if total == 0.0 {
                    center
                } else {
                    outgoing[node]
                        .iter()
                        .map(|&link| {
                            let target = ends[link].1;
                            (y[target] + dy[target] / 2.0) * values[link]
                        })
                        .sum::<f64>()
                        / total
                };
                y[node] += (target - center) * alpha;
            }
        }
        sankey_resolve_collisions(&mut columns, &mut y, &dy, height);
        for column in &columns {
            for &node in column {
                if incoming[node].is_empty() {
                    continue;
                }
                let total = sum(&incoming[node]);
                let center = y[node] + dy[node] / 2.0;
                let target = if total == 0.0 {
                    center
                } else {
                    incoming[node]
                        .iter()
                        .map(|&link| {
                            let source = ends[link].0;
                            (y[source] + dy[source] / 2.0) * values[link]
                        })
                        .sum::<f64>()
                        / total
                };
                y[node] += (target - center) * alpha;
            }
        }
        sankey_resolve_collisions(&mut columns, &mut y, &dy, height);
    }

    let mut source_offset = vec![0.0; links.len()];
    let mut target_offset = vec![0.0; links.len()];
    let offsets = |outgoing: &mut [Vec<usize>],
                   incoming: &mut [Vec<usize>],
                   y: &[f64],
                   source_offset: &mut [f64],
                   target_offset: &mut [f64]| {
        let by_y = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        for node in 0..count {
            outgoing[node].sort_by(|&a, &b| by_y(y[ends[a].1], y[ends[b].1]));
            let mut offset = 0.0;
            for &link in &outgoing[node] {
                source_offset[link] = offset;
                offset += link_dy[link];
            }
            incoming[node].sort_by(|&a, &b| by_y(y[ends[a].0], y[ends[b].0]));
            let mut offset = 0.0;
            for &link in &incoming[node] {
                target_offset[link] = offset;
                offset += link_dy[link];
            }
        }
    };
    offsets(
        &mut outgoing,
        &mut incoming,
        &y,
        &mut source_offset,
        &mut target_offset,
    );

    // Recharts `resolveNodeLinkCollisions`: a node lying wholly inside a link that skips its
    // column is moved off that link.
    for (column_index, column) in columns.iter().enumerate() {
        let Some(&first) = column.first() else {
            continue;
        };
        let column_x = x[first] + SANKEY_NODE_WIDTH / 2.0;
        // The column is sorted by y; suffix minima of node bottoms answer "is any node at or
        // below `top` also above `bottom`" with one binary search.
        let mut lowest_bottom = vec![f64::INFINITY; column.len() + 1];
        for position in (0..column.len()).rev() {
            let node = column[position];
            lowest_bottom[position] = lowest_bottom[position + 1].min(y[node] + dy[node]);
        }
        let mut obstacles: Vec<(f64, f64)> = Vec::new();
        for (link, &(source, target)) in ends.iter().enumerate() {
            let (low, high) = (
                depth[source].min(depth[target]),
                depth[source].max(depth[target]),
            );
            if column_index <= low || column_index >= high {
                continue;
            }
            let source_x = x[source] + SANKEY_NODE_WIDTH;
            let target_x = x[target];
            let progress = if target_x == source_x {
                0.0
            } else {
                ((column_x - source_x) / (target_x - source_x)).clamp(0.0, 1.0)
            };
            let source_y = y[source] + source_offset[link] + link_dy[link] / 2.0;
            let target_y = y[target] + target_offset[link] + link_dy[link] / 2.0;
            let u = 1.0 - progress;
            let center = (u * u * u + 3.0 * u * u * progress) * source_y
                + (3.0 * u * progress * progress + progress * progress * progress) * target_y;
            let top = center - link_dy[link] / 2.0;
            let start = column.partition_point(|&node| y[node] < top);
            if lowest_bottom[start] <= top + link_dy[link] {
                obstacles.push((top, link_dy[link]));
            }
        }
        if obstacles.is_empty() {
            continue;
        }
        let mut items: Vec<(Option<usize>, f64, f64)> = column
            .iter()
            .map(|&node| (Some(node), 0.0, 0.0))
            .chain(obstacles.into_iter().map(|(top, size)| (None, top, size)))
            .collect();
        let key =
            |y: &[f64], item: &(Option<usize>, f64, f64)| item.0.map_or(item.1, |node| y[node]);
        items.sort_by(|a, b| {
            key(&y, a)
                .partial_cmp(&key(&y, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut next = 0.0_f64;
        for &(node, top, size) in &items {
            match node {
                None => next = next.max(top + size + SANKEY_NODE_PADDING),
                Some(node) => {
                    if y[node] < next {
                        y[node] = next;
                    }
                    next = y[node] + dy[node] + SANKEY_NODE_PADDING;
                }
            }
        }
        items.sort_by(|a, b| {
            key(&y, a)
                .partial_cmp(&key(&y, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut previous = height + SANKEY_NODE_PADDING;
        for &(node, top, _) in items.iter().rev() {
            match node {
                None => previous = previous.min(top - SANKEY_NODE_PADDING),
                Some(node) => {
                    let overflow = y[node] + dy[node] + SANKEY_NODE_PADDING - previous;
                    if overflow > 0.0 {
                        y[node] -= overflow;
                    }
                    previous = y[node];
                }
            }
        }
    }
    offsets(
        &mut outgoing,
        &mut incoming,
        &y,
        &mut source_offset,
        &mut target_offset,
    );

    let left = plot.x + SANKEY_MARGIN;
    let top = plot.y + SANKEY_MARGIN;
    cache.column_count = max_depth + 1;
    cache.column_stride = stride;
    cache.nodes = (0..count)
        .map(|node| TreemapShape {
            row: node,
            rect: GeneralPlotRect {
                x: left + x[node],
                y: top + y[node],
                width: SANKEY_NODE_WIDTH,
                height: dy[node],
            },
        })
        .collect();
    cache.node_columns = depth;
    for (row, &(source, target)) in ends.iter().enumerate() {
        if link_dy[row] <= 0.0 {
            continue;
        }
        cache.links.push(SankeyLinkShape {
            row,
            x0: left + x[source] + SANKEY_NODE_WIDTH,
            x1: left + x[target],
            y0: top + y[source] + source_offset[row] + link_dy[row] / 2.0,
            y1: top + y[target] + target_offset[row] + link_dy[row] / 2.0,
            thickness: link_dy[row],
        });
    }
    cache
}

/// Recharts `resolveCollisions`: sort each column by y, push overlapping nodes down by the node
/// padding, then pull the overflowing tail back above the bottom edge.
fn sankey_resolve_collisions(columns: &mut [Vec<usize>], y: &mut [f64], dy: &[f64], height: f64) {
    for column in columns {
        column.sort_by(|&a, &b| y[a].partial_cmp(&y[b]).unwrap_or(std::cmp::Ordering::Equal));
        let mut next = 0.0;
        for &node in column.iter() {
            let shift = next - y[node];
            if shift > 0.0 {
                y[node] += shift;
            }
            next = y[node] + dy[node] + SANKEY_NODE_PADDING;
        }
        let mut limit = height + SANKEY_NODE_PADDING;
        for &node in column.iter().rev() {
            let overflow = y[node] + dy[node] + SANKEY_NODE_PADDING - limit;
            if overflow > 0.0 {
                y[node] -= overflow;
                limit = y[node];
            } else {
                break;
            }
        }
    }
}
impl LayoutSeriesEntry {
    fn with_sankey<R>(
        &self,
        nodes: &[FlowNode],
        links: &[FlowLink],
        plot: GeneralPlotRect,
        f: impl FnOnce(&SankeyCache) -> R,
    ) -> R {
        let mut cache = self.sankey.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(sankey_shapes(nodes, links, plot));
        }
        f(cache.as_ref().expect("sankey cache initialized"))
    }
}

impl GeneralLayoutStore {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            series: Vec::new(),
            next_series_id: 1,
            hovered: None,
            selected: None,
            focused: None,
            focus_color: Color::rgb(41, 98, 255),
            focus_size: 14.0,
        }
    }

    fn contains_hit(&self, hit: GeneralLayoutHit) -> bool {
        let Some(series) = self
            .series
            .iter()
            .find(|series| series.id == hit.series && series.options.visible)
        else {
            return false;
        };
        let Some(input) = self.get(series.options.dataset) else {
            return false;
        };
        match (input, hit.element) {
            (GeneralLayoutInput::Funnel(stages), GeneralLayoutElement::Stage) => {
                hit.row < stages.len()
            }
            (GeneralLayoutInput::Hierarchy(nodes), GeneralLayoutElement::Node) => {
                hit.row < nodes.len()
            }
            (GeneralLayoutInput::Flow { nodes, .. }, GeneralLayoutElement::Node) => {
                hit.row < nodes.len()
            }
            (GeneralLayoutInput::Flow { links, .. }, GeneralLayoutElement::Link) => {
                hit.row < links.len()
            }
            _ => false,
        }
    }

    fn insert(&mut self, input: GeneralLayoutInput) -> Result<GeneralLayoutDatasetId, ChartError> {
        if self.entries.len() >= MAX_LAYOUT_DATASETS {
            return Err(resource_limit("too many layout datasets"));
        }
        let semantic_values = input.semantic_values();
        if self
            .reserved_bytes()
            .saturating_add(input.estimated_bytes())
            .saturating_add(semantic_values.capacity() * std::mem::size_of::<f64>())
            > MAX_LAYOUT_STORE_BYTES
        {
            return Err(resource_limit(
                "layout store retained capacity exceeds limit",
            ));
        }
        let id = GeneralLayoutDatasetId(
            NonZeroU32::new(self.next_id)
                .ok_or_else(|| resource_limit("layout dataset IDs exhausted"))?,
        );
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| resource_limit("layout dataset IDs exhausted"))?;
        let funnel_max = input.funnel_max();
        self.entries.push(LayoutDatasetEntry {
            id,
            input,
            funnel_max,
            semantic_values,
            transition: None,
        });
        self.next_id = next;
        Ok(id)
    }

    fn get(&self, id: GeneralLayoutDatasetId) -> Option<&GeneralLayoutInput> {
        self.entry(id).map(|entry| &entry.input)
    }

    fn entry(&self, id: GeneralLayoutDatasetId) -> Option<&LayoutDatasetEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    fn replace(
        &mut self,
        id: GeneralLayoutDatasetId,
        input: GeneralLayoutInput,
        animate: bool,
        time_ms: f64,
    ) -> Result<(), ChartError> {
        input.validate()?;
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| invalid_handle("layout dataset handle is stale"))?;
        for series in self
            .series
            .iter()
            .filter(|series| series.options.dataset == id)
        {
            if !series.options.kind.accepts(&input) {
                return Err(invalid_data(
                    "layout dataset kind cannot change while bound",
                ));
            }
        }
        let cache_before = self
            .series
            .iter()
            .filter(|series| series.options.dataset == id)
            .map(|series| cache_reservation(series.options.kind, &self.entries[index].input))
            .sum::<usize>();
        let cache_after = self
            .series
            .iter()
            .filter(|series| series.options.dataset == id)
            .map(|series| cache_reservation(series.options.kind, &input))
            .sum::<usize>();
        let semantic_values = input.semantic_values();
        let retained = self
            .reserved_bytes()
            .saturating_sub(self.entries[index].input.estimated_bytes())
            .saturating_sub(
                self.entries[index].semantic_values.capacity() * std::mem::size_of::<f64>(),
            )
            .saturating_sub(
                self.entries[index]
                    .transition
                    .as_ref()
                    .map_or(0, LayoutTransition::estimated_bytes),
            )
            .saturating_sub(cache_before)
            .saturating_add(input.estimated_bytes())
            .saturating_add(semantic_values.capacity() * std::mem::size_of::<f64>())
            .saturating_add(cache_after);
        if retained > MAX_LAYOUT_STORE_BYTES {
            return Err(resource_limit(
                "layout store retained capacity exceeds limit",
            ));
        }
        let source = self.entries[index].display_input();
        let animate = animate
            && self
                .series
                .iter()
                .any(|series| series.options.dataset == id && series.options.visible);
        let transition_bytes = source
            .estimated_bytes()
            .saturating_add(input.estimated_bytes());
        let transition_from = if animate
            && source != &input
            && layout_transition_compatible(source, &input)
            && retained.saturating_add(transition_bytes) <= MAX_LAYOUT_STORE_BYTES
        {
            Some(source.clone())
        } else {
            None
        };
        self.entries[index].funnel_max = input.funnel_max();
        self.entries[index].input = input;
        self.entries[index].semantic_values = semantic_values;
        self.entries[index].transition = transition_from.map(|from| {
            let mut presentation = self.entries[index].input.clone();
            blend_layout_input(&from, &self.entries[index].input, &mut presentation, 0.0);
            let presentation_funnel_max = presentation.funnel_max();
            LayoutTransition {
                from,
                presentation,
                presentation_funnel_max,
                started_ms: time_ms,
            }
        });
        if self.hovered.is_some_and(|hit| {
            self.series
                .iter()
                .any(|series| series.id == hit.series && series.options.dataset == id)
        }) {
            self.hovered = None;
        }
        if self.selected.is_some_and(|hit| {
            self.series
                .iter()
                .any(|series| series.id == hit.series && series.options.dataset == id)
        }) {
            self.selected = None;
        }
        if self.focused.is_some_and(|hit| {
            self.series
                .iter()
                .any(|series| series.id == hit.series && series.options.dataset == id)
        }) {
            self.focused = None;
        }
        for series in &self.series {
            if series.options.dataset == id {
                *series.treemap.borrow_mut() = None;
                *series.sankey.borrow_mut() = None;
                *series.sunburst.borrow_mut() = None;
            }
        }
        Ok(())
    }

    pub(crate) fn transition_active(&self) -> bool {
        self.entries.iter().any(|entry| entry.transition.is_some())
    }

    pub(crate) fn finish_transitions(&mut self) {
        self.invalidate_transition_caches();
        for entry in &mut self.entries {
            entry.transition = None;
        }
    }

    pub(crate) fn advance_transitions(&mut self, time_ms: f64, duration_ms: f64) {
        self.invalidate_transition_caches();
        for entry in &mut self.entries {
            let Some(motion) = entry.transition.as_mut() else {
                continue;
            };
            let elapsed = time_ms - motion.started_ms;
            if elapsed >= duration_ms || duration_ms == 0.0 {
                entry.transition = None;
            } else {
                blend_layout_input(
                    &motion.from,
                    &entry.input,
                    &mut motion.presentation,
                    (elapsed / duration_ms).clamp(0.0, 1.0),
                );
                motion.presentation_funnel_max = motion.presentation.funnel_max();
            }
        }
    }

    fn invalidate_transition_caches(&self) {
        for series in &self.series {
            if !self
                .entries
                .iter()
                .any(|entry| entry.id == series.options.dataset && entry.transition.is_some())
            {
                continue;
            }
            *series.treemap.borrow_mut() = None;
            *series.sankey.borrow_mut() = None;
            *series.sunburst.borrow_mut() = None;
        }
    }

    fn remove(&mut self, id: GeneralLayoutDatasetId) -> Result<(), ChartError> {
        if self
            .series
            .iter()
            .any(|series| series.options.dataset == id)
        {
            return Err(invalid_data("layout dataset is bound to a series"));
        }
        let index = self
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| invalid_handle("layout dataset handle is stale"))?;
        self.entries.remove(index);
        self.entries.shrink_to_fit();
        Ok(())
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<LayoutDatasetEntry>()
            + self.series.capacity() * std::mem::size_of::<LayoutSeriesEntry>()
            + self
                .series
                .iter()
                .map(|series| {
                    series.options.title.capacity()
                        + series.options.color.capacity()
                        + series.treemap.borrow().as_ref().map_or(0, |cache| {
                            cache.shapes.capacity() * std::mem::size_of::<TreemapShape>()
                                + cache.bounds.capacity()
                                    * std::mem::size_of::<Option<GeneralPlotRect>>()
                        })
                        + series.sankey.borrow().as_ref().map_or(0, |cache| {
                            cache.nodes.capacity() * std::mem::size_of::<TreemapShape>()
                                + cache.node_columns.capacity() * std::mem::size_of::<usize>()
                                + cache.links.capacity() * std::mem::size_of::<SankeyLinkShape>()
                        })
                        + series.sunburst.borrow().as_ref().map_or(0, |cache| {
                            cache.shapes.capacity() * std::mem::size_of::<SunburstShape>()
                        })
                })
                .sum::<usize>()
            + self
                .entries
                .iter()
                .map(|entry| {
                    entry.input.estimated_bytes()
                        + entry.semantic_values.capacity() * std::mem::size_of::<f64>()
                        + entry
                            .transition
                            .as_ref()
                            .map_or(0, LayoutTransition::estimated_bytes)
                })
                .sum::<usize>()
    }

    fn reserved_bytes(&self) -> usize {
        MAX_LAYOUT_DATASETS * std::mem::size_of::<LayoutDatasetEntry>()
            + MAX_LAYOUT_SERIES * std::mem::size_of::<LayoutSeriesEntry>()
            + self
                .entries
                .iter()
                .map(|entry| {
                    entry.input.estimated_bytes()
                        + entry.semantic_values.capacity() * std::mem::size_of::<f64>()
                        + entry
                            .transition
                            .as_ref()
                            .map_or(0, LayoutTransition::estimated_bytes)
                })
                .sum::<usize>()
            + self
                .series
                .iter()
                .map(|series| {
                    series.options.title.capacity()
                        + series.options.color.capacity()
                        + self.entry(series.options.dataset).map_or(0, |entry| {
                            cache_reservation(series.options.kind, &entry.input)
                        })
                })
                .sum::<usize>()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.series.is_empty()
    }

    fn insert_series(
        &mut self,
        pane_id: PaneId,
        options: GeneralLayoutSeriesOptions,
    ) -> Result<GeneralLayoutSeriesId, ChartError> {
        if self.series.len() >= MAX_LAYOUT_SERIES {
            return Err(resource_limit("too many layout series"));
        }
        let dataset = self
            .get(options.dataset)
            .ok_or_else(|| invalid_handle("layout dataset handle is stale"))?;
        if !options.kind.accepts(dataset) {
            return Err(invalid_data(
                "layout series and dataset kinds are incompatible",
            ));
        }
        if options.title.len() > 1_024 || options.color.len() > 64 {
            return Err(resource_limit("layout series option text exceeds limit"));
        }
        let color = Color::parse_css(&options.color).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidOptions, "invalid layout series color")
        })?;
        if self
            .reserved_bytes()
            .saturating_add(options.title.capacity())
            .saturating_add(options.color.capacity())
            .saturating_add(cache_reservation(options.kind, dataset))
            > MAX_LAYOUT_STORE_BYTES
        {
            return Err(resource_limit(
                "layout store retained capacity exceeds limit",
            ));
        }
        let id = GeneralLayoutSeriesId(
            NonZeroU32::new(self.next_series_id)
                .ok_or_else(|| resource_limit("layout series IDs exhausted"))?,
        );
        let next = self
            .next_series_id
            .checked_add(1)
            .ok_or_else(|| resource_limit("layout series IDs exhausted"))?;
        self.series.push(LayoutSeriesEntry {
            id,
            pane_id,
            options,
            color,
            treemap: RefCell::new(None),
            sankey: RefCell::new(None),
            sunburst: RefCell::new(None),
        });
        self.next_series_id = next;
        Ok(id)
    }

    fn remove_series(&mut self, id: GeneralLayoutSeriesId) -> Result<(), ChartError> {
        let index = self
            .series
            .iter()
            .position(|series| series.id == id)
            .ok_or_else(|| invalid_handle("layout series handle is stale"))?;
        let dataset = self.series[index].options.dataset;
        self.series.remove(index);
        self.series.shrink_to_fit();
        if !self
            .series
            .iter()
            .any(|series| series.options.dataset == dataset)
            && let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == dataset)
        {
            entry.transition = None;
        }
        if self.hovered.is_some_and(|hit| hit.series == id) {
            self.hovered = None;
        }
        if self.selected.is_some_and(|hit| hit.series == id) {
            self.selected = None;
        }
        if self.focused.is_some_and(|hit| hit.series == id) {
            self.focused = None;
        }
        Ok(())
    }

    pub(crate) fn persistence_datasets(
        &self,
    ) -> impl Iterator<Item = (GeneralLayoutDatasetId, &GeneralLayoutInput)> {
        self.entries.iter().map(|entry| (entry.id, &entry.input))
    }

    pub(crate) fn persistence_series(&self) -> impl Iterator<Item = &GeneralLayoutSeriesOptions> {
        self.series.iter().map(|entry| &entry.options)
    }

    pub(crate) fn uses_pane(&self, pane_id: PaneId) -> bool {
        self.series.iter().any(|series| series.pane_id == pane_id)
    }
}

impl GeneralLayoutSeriesKind {
    fn accepts(self, input: &GeneralLayoutInput) -> bool {
        matches!(
            (self, input),
            (Self::Funnel, GeneralLayoutInput::Funnel(_))
                | (
                    Self::Treemap | Self::Sunburst,
                    GeneralLayoutInput::Hierarchy(_)
                )
                | (Self::Sankey, GeneralLayoutInput::Flow { .. })
        )
    }
}

fn cache_reservation(kind: GeneralLayoutSeriesKind, input: &GeneralLayoutInput) -> usize {
    match (kind, input) {
        (GeneralLayoutSeriesKind::Treemap, GeneralLayoutInput::Hierarchy(nodes)) => {
            2 * nodes.len()
                * (std::mem::size_of::<TreemapShape>()
                    + std::mem::size_of::<Option<GeneralPlotRect>>())
        }
        (GeneralLayoutSeriesKind::Sunburst, GeneralLayoutInput::Hierarchy(nodes)) => {
            2 * nodes.len() * std::mem::size_of::<SunburstShape>()
        }
        (GeneralLayoutSeriesKind::Sankey, GeneralLayoutInput::Flow { nodes, links }) => {
            2 * nodes.len() * (std::mem::size_of::<TreemapShape>() + std::mem::size_of::<usize>())
                + 2 * links.len() * std::mem::size_of::<SankeyLinkShape>()
        }
        _ => 0,
    }
}

impl ChartEngine {
    fn push_general_layout_label(
        &self,
        out: &mut Vec<Prim>,
        label: &str,
        bounds: GeneralPlotRect,
        fill: Color,
        hpr: f64,
        vpr: f64,
    ) -> bool {
        if label.is_empty() || !bounds.width.is_finite() || !bounds.height.is_finite() {
            return false;
        }
        let layout = &self.options.get().layout;
        let size = layout.font_size.clamp(9.0, 18.0);
        if bounds.height < size + 6.0 {
            return false;
        }
        let width = self.measure_text_run(label, size, &layout.font_family, 400, false);
        if !width.is_finite() || width + 10.0 > bounds.width {
            return false;
        }
        out.push(Prim::Text {
            x: ((bounds.x + bounds.width * 0.5) * hpr) as f32,
            y: ((bounds.y + bounds.height * 0.5) * vpr) as f32,
            text: label.to_owned(),
            color: fill.contrast_srgb_over(self.chart_surface_color()),
            size: (size * vpr) as f32,
            family: layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 400,
            italic: false,
        });
        true
    }
    /// Recharts' default treemap cell name: 14 px, 8 px from the left edge, baseline 7 px below
    /// the middle, painted only in cells larger than 20 px that fit the measured name.
    fn push_treemap_label(
        &self,
        out: &mut Vec<Prim>,
        label: &str,
        rect: GeneralPlotRect,
        fill: Color,
        hpr: f64,
        vpr: f64,
    ) {
        const SIZE: f64 = 14.0;
        if label.is_empty() || rect.width <= 20.0 || rect.height <= 20.0 || SIZE >= rect.height {
            return;
        }
        let layout = &self.options.get().layout;
        let width = self.measure_text_run(label, SIZE, &layout.font_family, 400, false);
        if !width.is_finite() || width >= rect.width {
            return;
        }
        out.push(Prim::Text {
            x: ((rect.x + 8.0) * hpr) as f32,
            // A 7 px baseline offset puts the 14 px glyph middle about 2 px below the cell middle.
            y: ((rect.y + rect.height * 0.5 + 2.0) * vpr) as f32,
            text: label.to_owned(),
            color: fill.contrast_srgb_over(self.chart_surface_color()),
            size: (SIZE * vpr) as f32,
            family: layout.font_family.clone(),
            align: TextAlign::Left,
            weight: 400,
            italic: false,
        });
    }

    /// Recharts' sector label: `.75rem` text centered on the sector's middle radius and angle,
    /// skipped when it does not fit the ring.
    fn push_sunburst_label(
        &self,
        out: &mut Vec<Prim>,
        label: &str,
        shape: SunburstShape,
        fill: Color,
        hpr: f64,
        vpr: f64,
    ) {
        let radius = (shape.inner + shape.outer) * 0.5;
        if label.is_empty() || shape.outer - shape.inner < SUNBURST_LABEL_SIZE {
            return;
        }
        let layout = &self.options.get().layout;
        let width =
            self.measure_text_run(label, SUNBURST_LABEL_SIZE, &layout.font_family, 400, false);
        if !width.is_finite() || width + 4.0 > (shape.end - shape.start) * radius {
            return;
        }
        let angle = (shape.start + shape.end) * 0.5;
        out.push(Prim::Text {
            x: ((shape.cx + radius * angle.cos()) * hpr) as f32,
            y: ((shape.cy + radius * angle.sin()) * vpr) as f32,
            text: label.to_owned(),
            color: fill.contrast_srgb_over(self.chart_surface_color()),
            size: (SUNBURST_LABEL_SIZE * vpr) as f32,
            family: layout.font_family.clone(),
            align: TextAlign::Center,
            weight: 400,
            italic: false,
        });
    }

    pub(crate) fn build_general_layout_series_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return;
        };
        let Some(plot) = self.general_plot_rect(pane_index) else {
            return;
        };
        let Some(store) = self.general_layout_data.as_ref() else {
            return;
        };
        for series in store
            .series
            .iter()
            .filter(|series| series.pane_id == pane_id && series.options.visible)
        {
            if let (GeneralLayoutSeriesKind::Sunburst, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Hierarchy(nodes) = entry.display_input()
            {
                let stroke_width = (SUNBURST_STROKE_WIDTH * hpr.min(vpr)) as f32;
                series.with_sunburst(nodes, plot, |cache| {
                    for shape in &cache.shapes {
                        let segments = shape.segments();
                        let first_point = points.len() as u32;
                        for segment in 0..segments {
                            let [a, b, c, d] = shape.segment_vertices(segment);
                            points.extend(
                                [a, b, c, a, c, d]
                                    .map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]),
                            );
                        }
                        let fill = series.color;
                        out.push(Prim::MeshFill {
                            first_point,
                            point_count: (segments * 6) as u32,
                            color: fill,
                        });
                        let outline = points.len() as u32;
                        let to_bitmap = |(x, y): (f64, f64)| [(x * hpr) as f32, (y * vpr) as f32];
                        points.extend(
                            (0..=segments)
                                .map(|index| shape.arc_point(shape.outer, index))
                                .chain(
                                    (0..=segments)
                                        .rev()
                                        .map(|index| shape.arc_point(shape.inner, index)),
                                )
                                .chain(std::iter::once(shape.arc_point(shape.outer, 0)))
                                .map(to_bitmap),
                        );
                        out.push(Prim::Polyline {
                            first_point: outline,
                            point_count: points.len() as u32 - outline,
                            width: stroke_width,
                            style: LineStyle::Solid,
                            line_type: LineType::Simple,
                            color: SUNBURST_STROKE,
                        });
                        let label = if series.options.data_labels {
                            nodes[shape.row].label.clone()
                        } else {
                            layout_value_text(nodes[shape.row].value)
                        };
                        self.push_sunburst_label(out, &label, *shape, fill, hpr, vpr);
                    }
                });
            }
            if let (GeneralLayoutSeriesKind::Sankey, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Flow { nodes, links } = entry.display_input()
            {
                series.with_sankey(nodes, links, plot, |cache| {
                    let link_color = Color::rgba(
                        series.color.r(),
                        series.color.g(),
                        series.color.b(),
                        SANKEY_LINK_ALPHA,
                    );
                    for link in &cache.links {
                        if link.x1 <= link.x0 {
                            continue;
                        }
                        let segments = link.segments();
                        let first_point = points.len() as u32;
                        let mut previous = link.edge(0, segments);
                        for index in 1..=segments {
                            let next = link.edge(index, segments);
                            let ((top0, bottom0), (top1, bottom1)) = (previous, next);
                            points.extend(
                                [top0, top1, bottom1, top0, bottom1, bottom0]
                                    .map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]),
                            );
                            previous = next;
                        }
                        out.push(Prim::MeshFill {
                            first_point,
                            point_count: (segments * 6) as u32,
                            color: link_color,
                        });
                    }
                    for (node, &column) in cache.nodes.iter().zip(&cache.node_columns) {
                        let x0 = (node.rect.x * hpr).round() as i32;
                        let y0 = (node.rect.y * vpr).round() as i32;
                        let x1 = ((node.rect.x + node.rect.width) * hpr).round() as i32;
                        let y1 = ((node.rect.y + node.rect.height) * vpr).round() as i32;
                        if x1 > x0 && y1 > y0 {
                            let fill = SANKEY_NODE_FILL;
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: x0,
                                    y: y0,
                                    w: x1 - x0,
                                    h: y1 - y0,
                                },
                                color: fill,
                            });
                            if series.options.data_labels
                                && !self.push_general_layout_label(
                                    out,
                                    &nodes[node.row].label,
                                    node.rect,
                                    fill,
                                    hpr,
                                    vpr,
                                )
                            {
                                let gap = (cache.column_stride - node.rect.width).max(0.0);
                                let right =
                                    cache.column_count == 1 || column + 1 < cache.column_count;
                                let label_x = if right {
                                    node.rect.x + node.rect.width + gap * 0.5
                                } else {
                                    node.rect.x - gap * 0.5
                                };
                                let surface =
                                    Color::parse_css(&self.options.get().layout.background.color)
                                        .unwrap_or(Color::rgb(255, 255, 255));
                                self.push_general_layout_label(
                                    out,
                                    &nodes[node.row].label,
                                    GeneralPlotRect {
                                        x: label_x - gap * 0.5,
                                        y: node.rect.y,
                                        width: gap,
                                        height: node.rect.height,
                                    },
                                    surface,
                                    hpr,
                                    vpr,
                                );
                            }
                        }
                    }
                });
            }
            if let (GeneralLayoutSeriesKind::Treemap, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Hierarchy(nodes) = entry.display_input()
            {
                let color_range = treemap_color_range(nodes);
                // A centered 1 px SVG stroke on every cell edge: each cell gives up the low half
                // on its leading edges and the high half on its trailing edges.
                let stroke_x = hpr.round().max(1.0) as i32;
                let stroke_y = vpr.round().max(1.0) as i32;
                let (lead_x, trail_x) = (stroke_x / 2, stroke_x - stroke_x / 2);
                let (lead_y, trail_y) = (stroke_y / 2, stroke_y - stroke_y / 2);
                series.with_treemap_cache(nodes, plot, |cache| {
                    if !cache.shapes.is_empty() {
                        let x0 = (plot.x * hpr).round() as i32;
                        let y0 = (plot.y * vpr).round() as i32;
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: x0,
                                y: y0,
                                w: ((plot.x + plot.width) * hpr).round() as i32 - x0,
                                h: ((plot.y + plot.height) * vpr).round() as i32 - y0,
                            },
                            color: TREEMAP_STROKE,
                        });
                    }
                    for shape in &cache.shapes {
                        let x0 = (shape.rect.x * hpr).round() as i32 + lead_x;
                        let y0 = (shape.rect.y * vpr).round() as i32 + lead_y;
                        let x1 = ((shape.rect.x + shape.rect.width) * hpr).round() as i32 - trail_x;
                        let y1 =
                            ((shape.rect.y + shape.rect.height) * vpr).round() as i32 - trail_y;
                        if x1 > x0 && y1 > y0 {
                            let rect = IRect {
                                x: x0,
                                y: y0,
                                w: x1 - x0,
                                h: y1 - y0,
                            };
                            let fill =
                                treemap_cell_color(series.color, nodes, shape.row, color_range);
                            out.push(Prim::Rect { rect, color: fill });
                            self.push_treemap_label(
                                out,
                                &nodes[shape.row].label,
                                shape.rect,
                                fill,
                                hpr,
                                vpr,
                            );
                        }
                    }
                });
            }
            if let (GeneralLayoutSeriesKind::Funnel, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Funnel(stages) = entry.display_input()
            {
                visit_funnel_shapes(stages, entry.display_funnel_max(), plot, |shape| {
                    let [a, b, c, d] = shape.vertices();
                    let first_point = points.len() as u32;
                    points.extend(
                        [a, b, c, a, c, d].map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]),
                    );
                    out.push(Prim::MeshFill {
                        first_point,
                        point_count: 6,
                        color: series.color,
                    });
                    if series.options.data_labels {
                        self.push_general_layout_label(
                            out,
                            &stages[shape.row].label,
                            GeneralPlotRect {
                                x: shape.center_x
                                    - (shape.top_half_width + shape.bottom_half_width) * 0.5,
                                y: shape.top_y,
                                width: shape.top_half_width + shape.bottom_half_width,
                                height: shape.bottom_y - shape.top_y,
                            },
                            series.color,
                            hpr,
                            vpr,
                        );
                    }
                });
            }
        }
        if let Some(hit) = store.focused
            && let Some((x, y)) = self.general_layout_hit_anchor(pane_index, hit)
        {
            let ratio = hpr.min(vpr);
            let (cx, cy) = ((x * hpr) as f32, (y * vpr) as f32);
            let radius = (store.focus_size * ratio * 0.5) as f32;
            let transparent = Color::rgba(0, 0, 0, 0);
            // The ring sits on arbitrary layout fills, so a black band outside a white band
            // keeps the indicator at least 3:1 against any adjacent color (WCAG 1.4.11).
            for (offset, stroke) in [(3.0, Color::rgb(0, 0, 0)), (1.0, Color::rgb(255, 255, 255))] {
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius: radius + (offset * ratio) as f32,
                    fill: transparent,
                    stroke_width: (4.0 * ratio) as f32,
                    stroke,
                });
            }
            out.push(Prim::Circle {
                cx,
                cy,
                radius,
                fill: transparent,
                stroke_width: (2.0 * ratio) as f32,
                stroke: store.focus_color,
            });
        }
    }

    /// The center of one visible layout item in `pane_index`, in layout frame coordinates:
    /// where its keyboard focus ring and keyboard tooltip anchor.
    pub(crate) fn general_layout_hit_anchor(
        &self,
        pane_index: usize,
        hit: GeneralLayoutHit,
    ) -> Option<(f64, f64)> {
        let pane_id = self.pane_stable_id(pane_index)?;
        let plot = self.general_plot_rect(pane_index)?;
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| {
            series.id == hit.series && series.pane_id == pane_id && series.options.visible
        })?;
        let entry = store.entry(series.options.dataset)?;
        match (entry.display_input(), series.options.kind, hit.element) {
            (
                GeneralLayoutInput::Funnel(stages),
                GeneralLayoutSeriesKind::Funnel,
                GeneralLayoutElement::Stage,
            ) => funnel_shape_at(stages, entry.display_funnel_max(), plot, hit.row)
                .map(|shape| (shape.center_x, (shape.top_y + shape.bottom_y) * 0.5)),
            (
                GeneralLayoutInput::Hierarchy(nodes),
                GeneralLayoutSeriesKind::Treemap,
                GeneralLayoutElement::Node,
            ) => series.with_treemap_cache(nodes, plot, |cache| {
                cache
                    .bounds
                    .get(hit.row)
                    .copied()
                    .flatten()
                    .map(|rect| (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5))
            }),
            (
                GeneralLayoutInput::Hierarchy(nodes),
                GeneralLayoutSeriesKind::Sunburst,
                GeneralLayoutElement::Node,
            ) => series.with_sunburst(nodes, plot, |cache| {
                cache
                    .shapes
                    .iter()
                    .find(|shape| shape.row == hit.row)
                    .map(|shape| {
                        let radius = (shape.inner + shape.outer) * 0.5;
                        let angle = (shape.start + shape.end) * 0.5;
                        (
                            shape.cx + radius * angle.cos(),
                            shape.cy + radius * angle.sin(),
                        )
                    })
                    .or_else(|| {
                        nodes
                            .get(hit.row)
                            .filter(|node| node.parent.is_none())
                            .map(|_| (plot.x + plot.width * 0.5, plot.y + plot.height * 0.5))
                    })
            }),
            (
                GeneralLayoutInput::Flow { nodes, links },
                GeneralLayoutSeriesKind::Sankey,
                element,
            ) => series.with_sankey(nodes, links, plot, |cache| match element {
                GeneralLayoutElement::Node => cache
                    .nodes
                    .iter()
                    .find(|shape| shape.row == hit.row)
                    .map(|shape| {
                        (
                            shape.rect.x + shape.rect.width * 0.5,
                            shape.rect.y + shape.rect.height * 0.5,
                        )
                    }),
                GeneralLayoutElement::Link => cache
                    .links
                    .iter()
                    .find(|shape| shape.row == hit.row)
                    .map(|shape| shape.point(0.5)),
                GeneralLayoutElement::Stage => None,
            }),
            _ => None,
        }
    }

    pub fn general_layout_hit_test(
        &self,
        pane_index: usize,
        x: f64,
        y: f64,
    ) -> Option<GeneralLayoutHit> {
        let pane_id = self.pane_stable_id(pane_index)?;
        let plot = self.general_plot_rect(pane_index)?;
        if x < plot.x || x >= plot.x + plot.width || y < plot.y || y >= plot.y + plot.height {
            return None;
        }
        let store = self.general_layout_data.as_ref()?;
        for series in store
            .series
            .iter()
            .rev()
            .filter(|series| series.pane_id == pane_id && series.options.visible)
        {
            if let (GeneralLayoutSeriesKind::Sunburst, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Hierarchy(nodes) = entry.display_input()
            {
                let row = series.with_sunburst(nodes, plot, |cache| {
                    cache
                        .shapes
                        .iter()
                        .rev()
                        .find(|shape| shape.contains(x, y))
                        .map(|shape| shape.row)
                });
                if let Some(row) = row {
                    return Some(GeneralLayoutHit {
                        series: series.id,
                        element: GeneralLayoutElement::Node,
                        row,
                    });
                }
            }
            if let (GeneralLayoutSeriesKind::Sankey, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Flow { nodes, links } = entry.display_input()
            {
                let hit = series.with_sankey(nodes, links, plot, |cache| {
                    cache
                        .nodes
                        .iter()
                        .rev()
                        .find(|shape| shape.contains(x, y))
                        .map(|shape| (GeneralLayoutElement::Node, shape.row))
                        .or_else(|| {
                            cache
                                .links
                                .iter()
                                .rev()
                                .find(|shape| shape.contains(x, y))
                                .map(|shape| (GeneralLayoutElement::Link, shape.row))
                        })
                });
                if let Some((element, row)) = hit {
                    return Some(GeneralLayoutHit {
                        series: series.id,
                        element,
                        row,
                    });
                }
            }
            if let (GeneralLayoutSeriesKind::Treemap, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Hierarchy(nodes) = entry.display_input()
            {
                let hit = series.with_treemap(nodes, plot, |shapes| {
                    shapes
                        .iter()
                        .rev()
                        .find(|shape| shape.contains(x, y))
                        .map(|shape| shape.row)
                });
                if let Some(row) = hit {
                    return Some(GeneralLayoutHit {
                        series: series.id,
                        element: GeneralLayoutElement::Node,
                        row,
                    });
                }
            }
            if let (GeneralLayoutSeriesKind::Funnel, Some(entry)) =
                (series.options.kind, store.entry(series.options.dataset))
                && let GeneralLayoutInput::Funnel(stages) = entry.display_input()
                && !stages.is_empty()
            {
                let row = (((y - plot.y) / plot.height) * stages.len() as f64).floor() as usize;
                if let Some(shape) = funnel_shape_at(stages, entry.display_funnel_max(), plot, row)
                    && shape.contains(x, y)
                {
                    return Some(GeneralLayoutHit {
                        series: series.id,
                        element: GeneralLayoutElement::Stage,
                        row,
                    });
                }
            }
        }
        None
    }

    pub fn update_general_layout_hover(
        &mut self,
        pane: usize,
        x: f64,
        y: f64,
    ) -> Option<GeneralLayoutHit> {
        let hit = self.general_layout_hit_test(pane, x, y);
        if let Some(store) = self.general_layout_data.as_mut() {
            store.hovered = hit;
        }
        hit
    }

    pub fn clear_general_layout_hover(&mut self) {
        if let Some(store) = self.general_layout_data.as_mut() {
            store.hovered = None;
        }
    }

    pub fn general_layout_hovered_hit(&self) -> Option<GeneralLayoutHit> {
        self.general_layout_data.as_ref()?.hovered
    }

    pub fn select_general_layout_hovered(&mut self) -> bool {
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        store.selected = store.hovered;
        store.selected.is_some()
    }

    pub fn select_general_layout_item(
        &mut self,
        series_id: GeneralLayoutSeriesId,
        element: GeneralLayoutElement,
        row: usize,
    ) -> bool {
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let hit = GeneralLayoutHit {
            series: series_id,
            element,
            row,
        };
        if !store.contains_hit(hit) {
            return false;
        }
        store.selected = Some(hit);
        true
    }
    pub fn set_general_layout_accessibility_focus(
        &mut self,
        hit: GeneralLayoutHit,
        color: &str,
        size: f64,
    ) -> bool {
        let Some(parsed) = Color::parse_css(color) else {
            return false;
        };
        if !size.is_finite() || !(4.0..=128.0).contains(&size) {
            return false;
        }
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        if !store.contains_hit(hit) {
            return false;
        }
        if store.focused == Some(hit) && store.focus_color == parsed && store.focus_size == size {
            return true;
        }
        store.focused = Some(hit);
        store.focus_color = parsed;
        store.focus_size = size;
        self.invalidate_frame_all();
        true
    }

    pub fn clear_general_layout_accessibility_focus(&mut self) {
        self.reset_focus_tooltip_dismissal();
        if let Some(store) = self.general_layout_data.as_mut()
            && store.focused.take().is_some()
        {
            self.invalidate_frame_all();
        }
    }

    pub fn general_layout_accessibility_focused_hit(&self) -> Option<GeneralLayoutHit> {
        self.general_layout_data.as_ref()?.focused
    }
    pub fn clear_general_layout_selection(&mut self) {
        if let Some(store) = self.general_layout_data.as_mut() {
            store.selected = None;
        }
    }

    pub fn general_layout_selected_hit(&self) -> Option<GeneralLayoutHit> {
        self.general_layout_data.as_ref()?.selected
    }

    pub fn general_layout_hit_value(&self, hit: GeneralLayoutHit) -> Option<f64> {
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| series.id == hit.series)?;
        let entry = store.entry(series.options.dataset)?;
        let index = match (&entry.input, hit.element) {
            (GeneralLayoutInput::Funnel(_), GeneralLayoutElement::Stage)
            | (GeneralLayoutInput::Hierarchy(_), GeneralLayoutElement::Node)
            | (GeneralLayoutInput::Flow { .. }, GeneralLayoutElement::Node) => hit.row,
            (GeneralLayoutInput::Flow { nodes, .. }, GeneralLayoutElement::Link) => {
                nodes.len().checked_add(hit.row)?
            }
            _ => return None,
        };
        entry.semantic_values.get(index).copied()
    }

    /// Tooltip parts of one layout item in `pane_index`: item label, series title, semantic
    /// value, and the item's painted color.
    pub(crate) fn general_layout_item_tooltip(
        &self,
        pane_index: usize,
        hit: GeneralLayoutHit,
    ) -> Option<(String, String, Option<f64>, Color)> {
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| series.id == hit.series)?;
        if self.pane_index_for_id(series.pane_id) != Some(pane_index) {
            return None;
        }
        let entry = store.entry(series.options.dataset)?;
        let (label, color) = match (&entry.input, hit.element) {
            (GeneralLayoutInput::Funnel(stages), GeneralLayoutElement::Stage) => {
                (stages.get(hit.row)?.label.clone(), series.color)
            }
            (GeneralLayoutInput::Hierarchy(nodes), GeneralLayoutElement::Node) => (
                nodes.get(hit.row)?.label.clone(),
                if series.options.kind == GeneralLayoutSeriesKind::Treemap {
                    treemap_cell_color(series.color, nodes, hit.row, treemap_color_range(nodes))
                } else {
                    series.color
                },
            ),
            (GeneralLayoutInput::Flow { nodes, .. }, GeneralLayoutElement::Node) => {
                (nodes.get(hit.row)?.label.clone(), SANKEY_NODE_FILL)
            }
            (GeneralLayoutInput::Flow { nodes, links }, GeneralLayoutElement::Link) => {
                let link = links.get(hit.row)?;
                let name = |id: &str| {
                    nodes
                        .iter()
                        .find(|node| node.id == id)
                        .map_or(id, |node| node.label.as_str())
                        .to_owned()
                };
                (
                    format!("{} → {}", name(&link.source), name(&link.target)),
                    series.color,
                )
            }
            _ => return None,
        };
        Some((
            label,
            series.options.title.clone(),
            self.general_layout_hit_value(hit),
            color,
        ))
    }

    pub fn general_layout_accessibility_snapshot(
        &self,
        series_id: GeneralLayoutSeriesId,
        offset: usize,
        limit: usize,
    ) -> Option<GeneralLayoutAccessibilitySnapshot> {
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| series.id == series_id)?;
        let entry = store.entry(series.options.dataset)?;
        let input = &entry.input;
        let selected = store.selected;
        let limit = limit.min(512);
        let (total, items) = match input {
            GeneralLayoutInput::Funnel(stages) => (
                stages.len(),
                stages
                    .iter()
                    .enumerate()
                    .skip(offset)
                    .take(limit)
                    .map(|(row, stage)| GeneralLayoutAccessibilityItem {
                        element: GeneralLayoutElement::Stage,
                        row,
                        id: stage.id.clone(),
                        label: stage.label.clone(),
                        value: Some(stage.value),
                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element: GeneralLayoutElement::Stage,
                                row,
                            }),
                    })
                    .collect(),
            ),
            GeneralLayoutInput::Hierarchy(nodes) => (
                nodes.len(),
                nodes
                    .iter()
                    .enumerate()
                    .skip(offset)
                    .take(limit)
                    .map(|(row, node)| GeneralLayoutAccessibilityItem {
                        element: GeneralLayoutElement::Node,
                        row,
                        id: node.id.clone(),
                        label: node.label.clone(),
                        value: entry.semantic_values.get(row).copied(),

                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element: GeneralLayoutElement::Node,
                                row,
                            }),
                    })
                    .collect(),
            ),
            GeneralLayoutInput::Flow { nodes, links } => {
                let mut items = Vec::with_capacity(limit.min(nodes.len() + links.len()));
                for row in offset..(offset.saturating_add(limit)).min(nodes.len() + links.len()) {
                    let (element, id, label, value) = if row < nodes.len() {
                        let node = &nodes[row];
                        (
                            GeneralLayoutElement::Node,
                            node.id.clone(),
                            node.label.clone(),
                            entry.semantic_values.get(row).copied(),
                        )
                    } else {
                        let link_row = row - nodes.len();
                        let link = &links[link_row];
                        (
                            GeneralLayoutElement::Link,
                            format!("{}→{}", link.source, link.target),
                            format!("{} to {}", link.source, link.target),
                            entry.semantic_values.get(row).copied(),
                        )
                    };
                    let element_row = if row < nodes.len() {
                        row
                    } else {
                        row - nodes.len()
                    };
                    items.push(GeneralLayoutAccessibilityItem {
                        element,
                        row: element_row,
                        id,
                        label,
                        value,
                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element,
                                row: element_row,
                            }),
                    });
                }
                (nodes.len() + links.len(), items)
            }
        };
        Some(GeneralLayoutAccessibilitySnapshot {
            series: series_id,
            title: series.options.title.clone(),
            total,
            offset: offset.min(total),
            items,
        })
    }
    pub fn add_general_layout_series(
        &mut self,
        options: GeneralLayoutSeriesOptions,
    ) -> Result<GeneralLayoutSeriesId, ChartError> {
        let pane_id = self
            .pane_stable_id(options.pane)
            .ok_or_else(|| invalid_handle("layout pane is stale"))?;
        if self.pane_horizontal_domain(options.pane) != Some(HorizontalDomain::Layout) {
            return Err(ChartError::new(
                ErrorCode::InvalidOptions,
                "layout series needs a layout pane",
            ));
        }
        let store = self
            .general_layout_data
            .as_mut()
            .ok_or_else(|| invalid_handle("layout dataset handle is stale"))?;
        let id = store.insert_series(pane_id, options)?;
        self.invalidate_frame_all();
        Ok(id)
    }

    pub fn update_general_layout_series_presentation(
        &mut self,
        id: GeneralLayoutSeriesId,
        title: String,
        color: String,
        data_labels: bool,
        visible: bool,
    ) -> Result<(), ChartError> {
        if title.len() > 1_024 || color.len() > 64 {
            return Err(resource_limit("layout series option text exceeds limit"));
        }
        let parsed = Color::parse_css(&color).ok_or_else(|| {
            ChartError::new(ErrorCode::InvalidOptions, "invalid layout series color")
        })?;
        let store = self
            .general_layout_data
            .as_mut()
            .ok_or_else(|| invalid_handle("layout series handle is stale"))?;
        let index = store
            .series
            .iter()
            .position(|series| series.id == id)
            .ok_or_else(|| invalid_handle("layout series handle is stale"))?;
        let retained = store
            .reserved_bytes()
            .saturating_sub(store.series[index].options.title.capacity())
            .saturating_sub(store.series[index].options.color.capacity())
            .saturating_add(title.capacity())
            .saturating_add(color.capacity());
        if retained > MAX_LAYOUT_STORE_BYTES {
            return Err(resource_limit(
                "layout store retained capacity exceeds limit",
            ));
        }
        let series = &mut store.series[index];
        series.options.title = title;
        series.options.color = color;
        series.options.data_labels = data_labels;
        series.options.visible = visible;
        series.color = parsed;
        if !visible {
            let dataset = series.options.dataset;
            if store.hovered.is_some_and(|hit| hit.series == id) {
                store.hovered = None;
            }
            if store.selected.is_some_and(|hit| hit.series == id) {
                store.selected = None;
            }
            if store.focused.is_some_and(|hit| hit.series == id) {
                store.focused = None;
            }
            if !store
                .series
                .iter()
                .any(|bound| bound.options.dataset == dataset && bound.options.visible)
                && let Some(entry) = store.entries.iter_mut().find(|entry| entry.id == dataset)
            {
                entry.transition = None;
            }
        }
        self.invalidate_frame_all();
        Ok(())
    }
    pub fn remove_general_layout_series(
        &mut self,
        id: GeneralLayoutSeriesId,
    ) -> Result<(), ChartError> {
        let store = self
            .general_layout_data
            .as_mut()
            .ok_or_else(|| invalid_handle("layout series handle is stale"))?;
        store.remove_series(id)?;
        self.invalidate_frame_all();
        Ok(())
    }
    pub fn create_general_layout_dataset(
        &mut self,
        input: GeneralLayoutInput,
    ) -> Result<GeneralLayoutDatasetId, ChartError> {
        input.validate()?;
        let store = self
            .general_layout_data
            .get_or_insert_with(GeneralLayoutStore::new);
        let id = store.insert(input)?;
        self.invalidate_frame_all();
        Ok(id)
    }

    pub fn general_layout_dataset(
        &self,
        id: GeneralLayoutDatasetId,
    ) -> Option<&GeneralLayoutInput> {
        self.general_layout_data.as_ref()?.get(id)
    }

    pub fn general_layout_dataset_is_bound(&self, id: GeneralLayoutDatasetId) -> bool {
        self.general_layout_data.as_ref().is_some_and(|store| {
            store
                .series
                .iter()
                .any(|series| series.options.dataset == id)
        })
    }
    pub fn general_layout_series_options(
        &self,
        id: GeneralLayoutSeriesId,
    ) -> Option<&GeneralLayoutSeriesOptions> {
        self.general_layout_data
            .as_ref()?
            .series
            .iter()
            .find(|series| series.id == id)
            .map(|series| &series.options)
    }

    /// Legend rows for a pane's layout series in insertion order: identity, title (the kind name
    /// when untitled), resolved color, and visibility.
    pub(crate) fn general_layout_legend_entries(
        &self,
        pane_id: PaneId,
    ) -> Vec<(GeneralLayoutSeriesId, String, Color, bool)> {
        let Some(store) = self.general_layout_data.as_ref() else {
            return Vec::new();
        };
        store
            .series
            .iter()
            .filter(|series| series.pane_id == pane_id)
            .map(|series| {
                let title = if series.options.title.is_empty() {
                    format!("{:?}", series.options.kind)
                } else {
                    series.options.title.clone()
                };
                (series.id, title, series.color, series.options.visible)
            })
            .collect()
    }

    pub(crate) fn toggle_general_layout_series_visible(
        &mut self,
        id: GeneralLayoutSeriesId,
    ) -> bool {
        let Some(options) = self.general_layout_series_options(id).cloned() else {
            return false;
        };
        self.update_general_layout_series_presentation(
            id,
            options.title,
            options.color,
            options.data_labels,
            !options.visible,
        )
        .is_ok()
    }

    pub fn general_layout_series_ids(&self) -> Vec<GeneralLayoutSeriesId> {
        self.general_layout_data
            .as_ref()
            .map_or_else(Vec::new, |store| {
                store.series.iter().map(|series| series.id).collect()
            })
    }

    pub fn replace_general_layout_dataset(
        &mut self,
        id: GeneralLayoutDatasetId,
        input: GeneralLayoutInput,
    ) -> Result<(), ChartError> {
        self.general_layout_data
            .as_mut()
            .ok_or_else(|| invalid_handle("layout dataset handle is stale"))?
            .replace(
                id,
                input,
                self.general_transition_duration_ms > 0.0
                    && !self.general_transition_reduced_motion,
                self.general_transition_time_ms,
            )?;
        self.invalidate_frame_all();
        Ok(())
    }

    pub fn remove_general_layout_dataset(
        &mut self,
        id: GeneralLayoutDatasetId,
    ) -> Result<(), ChartError> {
        let store = self
            .general_layout_data
            .as_mut()
            .ok_or_else(|| invalid_handle("layout dataset handle is stale"))?;
        store.remove(id)?;
        if store.is_empty() {
            self.general_layout_data = None;
        }
        self.invalidate_frame_all();
        Ok(())
    }
}

fn validate_hierarchy(nodes: &[HierarchyNode]) -> Result<(), ChartError> {
    validate_rows(nodes.len())?;
    if nodes.is_empty() {
        return Ok(());
    }
    let mut ids = HashMap::with_capacity(nodes.len());
    let mut unique = HashSet::with_capacity(nodes.len());
    let mut bytes = 0usize;
    let mut roots = 0usize;
    for (index, node) in nodes.iter().enumerate() {
        validate_id(&node.id, &mut unique)?;
        ids.insert(node.id.as_str(), index);
        add_text_bytes(&mut bytes, &node.id)?;
        add_text_bytes(&mut bytes, &node.label)?;
        if let Some(parent) = &node.parent {
            add_text_bytes(&mut bytes, parent)?;
        } else {
            roots += 1;
        }
        if let Some(value) = node.value {
            validate_value(value)?;
        }
        if node.color_value.is_some_and(|value| !value.is_finite()) {
            return Err(invalid_data("hierarchy color_value must be finite"));
        }
    }
    if roots != 1 {
        return Err(invalid_data("hierarchy requires exactly one root"));
    }
    let mut children = vec![0usize; nodes.len()];
    for node in nodes {
        if let Some(parent) = &node.parent {
            let Some(&index) = ids.get(parent.as_str()) else {
                return Err(invalid_data("hierarchy parent does not exist"));
            };
            children[index] += 1;
        }
    }
    for (index, node) in nodes.iter().enumerate() {
        if (children[index] == 0) != node.value.is_some() {
            return Err(invalid_data(
                "hierarchy leaves need values and internal nodes derive them",
            ));
        }
        let mut ancestor = node.parent.as_deref();
        let mut depth = 0;
        while let Some(parent) = ancestor {
            depth += 1;
            if depth > MAX_LAYOUT_DEPTH {
                return Err(resource_limit("hierarchy depth exceeds limit"));
            }
            let parent_index = ids[parent];
            if parent_index == index {
                return Err(invalid_data("hierarchy contains a cycle"));
            }
            ancestor = nodes[parent_index].parent.as_deref();
        }
    }
    Ok(())
}

fn validate_flow(nodes: &[FlowNode], links: &[FlowLink]) -> Result<(), ChartError> {
    validate_rows(nodes.len())?;
    if links.len() > MAX_FLOW_LINKS {
        return Err(resource_limit("too many flow links"));
    }
    let mut ids = HashMap::with_capacity(nodes.len());
    let mut unique = HashSet::with_capacity(nodes.len());
    let mut bytes = 0usize;
    for (index, node) in nodes.iter().enumerate() {
        validate_id(&node.id, &mut unique)?;
        ids.insert(node.id.as_str(), index);
        add_text_bytes(&mut bytes, &node.id)?;
        add_text_bytes(&mut bytes, &node.label)?;
    }
    let mut outgoing = vec![Vec::new(); nodes.len()];
    let mut indegree = vec![0usize; nodes.len()];
    for link in links {
        add_text_bytes(&mut bytes, &link.source)?;
        add_text_bytes(&mut bytes, &link.target)?;
        validate_value(link.value)?;
        let Some(&source) = ids.get(link.source.as_str()) else {
            return Err(invalid_data("flow source does not exist"));
        };
        let Some(&target) = ids.get(link.target.as_str()) else {
            return Err(invalid_data("flow target does not exist"));
        };
        if source == target {
            return Err(invalid_data("flow self-links are not supported"));
        }
        outgoing[source].push(target);
        indegree[target] += 1;
    }
    let mut ready = std::collections::VecDeque::new();
    let mut depth = vec![0usize; nodes.len()];
    for (index, degree) in indegree.iter().enumerate() {
        if *degree == 0 {
            ready.push_back(index);
        }
    }
    let mut visited = 0usize;
    while let Some(index) = ready.pop_front() {
        visited += 1;
        let next_depth = depth[index] + 1;
        for &target in &outgoing[index] {
            depth[target] = depth[target].max(next_depth);
            if depth[target] > MAX_LAYOUT_DEPTH {
                return Err(resource_limit("flow depth exceeds limit"));
            }
            indegree[target] -= 1;
            if indegree[target] == 0 {
                ready.push_back(target);
            }
        }
    }
    if visited != nodes.len() {
        return Err(invalid_data("flow contains a cycle"));
    }
    Ok(())
}

fn validate_rows(count: usize) -> Result<(), ChartError> {
    if count > MAX_LAYOUT_ROWS {
        Err(resource_limit("too many layout rows"))
    } else {
        Ok(())
    }
}

fn validate_id<'a>(id: &'a str, ids: &mut HashSet<&'a str>) -> Result<(), ChartError> {
    if id.is_empty() {
        return Err(invalid_data("layout ID must not be empty"));
    }
    if !ids.insert(id) {
        return Err(invalid_data("duplicate layout ID"));
    }
    Ok(())
}

fn add_text_bytes(total: &mut usize, text: &str) -> Result<(), ChartError> {
    *total = total
        .checked_add(text.len())
        .ok_or_else(|| resource_limit("layout text exceeds limit"))?;
    if *total > MAX_LAYOUT_TEXT_BYTES {
        return Err(resource_limit("layout text exceeds limit"));
    }
    Ok(())
}

fn validate_value(value: f64) -> Result<(), ChartError> {
    if !value.is_finite() || value < 0.0 {
        return Err(invalid_data("layout values must be finite and nonnegative"));
    }
    Ok(())
}

fn invalid_data(message: &'static str) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}
fn invalid_handle(message: &'static str) -> ChartError {
    ChartError::new(ErrorCode::InvalidHandle, message)
}
fn resource_limit(message: &'static str) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeris_charts_render::draw_list::Prim;

    fn chart() -> ChartEngine {
        ChartEngine::new_with_initial_domain(400.0, 300.0, 1.0, HorizontalDomain::Layout).unwrap()
    }

    fn hierarchy(values: &[f64]) -> Vec<HierarchyNode> {
        let mut nodes = vec![HierarchyNode {
            id: "root".into(),
            parent: None,
            label: "Root".into(),
            value: None,
            color_value: None,
        }];
        for (index, value) in values.iter().enumerate() {
            nodes.push(HierarchyNode {
                id: format!("leaf-{index}"),
                parent: Some("root".into()),
                label: format!("Leaf {index}"),
                value: Some(*value),
                color_value: None,
            });
        }
        nodes
    }

    #[test]
    fn treemap_layout_preserves_area_and_leaf_identity() {
        let plot = GeneralPlotRect {
            x: 11.0,
            y: 7.0,
            width: 360.0,
            height: 180.0,
        };
        let nodes = hierarchy(&[3.0, 2.0, 1.0, 0.0]);
        let shapes = treemap_shapes(&nodes, plot);
        assert_eq!(
            shapes.iter().map(|shape| shape.row).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let area: f64 = shapes
            .iter()
            .map(|shape| shape.rect.width * shape.rect.height)
            .sum();
        assert!((area - plot.width * plot.height).abs() < 1e-6);
        for (index, a) in shapes.iter().enumerate() {
            assert!(a.rect.x >= plot.x && a.rect.y >= plot.y);
            assert!(a.rect.x + a.rect.width <= plot.x + plot.width + 1e-9);
            assert!(a.rect.y + a.rect.height <= plot.y + plot.height + 1e-9);
            for b in shapes.iter().skip(index + 1) {
                let overlap_x =
                    (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width) - a.rect.x.max(b.rect.x);
                let overlap_y = (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height)
                    - a.rect.y.max(b.rect.y);
                assert!(overlap_x <= 1e-9 || overlap_y <= 1e-9);
            }
        }
        assert_eq!(treemap_shapes(&nodes, plot), shapes);
        assert_eq!(
            treemap_shapes(&hierarchy(&[f64::MAX, f64::MAX]), plot).len(),
            2
        );
    }

    #[test]
    fn treemap_frame_hit_and_update_share_cached_rectangles() {
        let mut chart = chart();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Hierarchy(hierarchy(&[
                3.0, 2.0, 1.0,
            ])))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Treemap,
            ))
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::Rect { .. }))
                .count(),
            4,
            "white stroke backdrop plus three cells"
        );
        let plot = chart.general_plot_rect(0).unwrap();
        let shapes = treemap_shapes(&hierarchy(&[3.0, 2.0, 1.0]), plot);
        for shape in &shapes {
            let x = shape.rect.x + shape.rect.width / 2.0;
            let y = shape.rect.y + shape.rect.height / 2.0;
            assert_eq!(
                chart.general_layout_hit_test(0, x, y),
                Some(GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Node,
                    row: shape.row
                })
            );
        }
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Hierarchy(hierarchy(&[0.0, 0.0, 4.0])),
            )
            .unwrap();
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::Rect { .. }))
                .count(),
            2,
            "white stroke backdrop plus the one weighted cell"
        );
        assert_eq!(
            chart.general_layout_hit_test(0, plot.x + plot.width / 2.0, plot.y + plot.height / 2.0),
            Some(GeneralLayoutHit {
                series,
                element: GeneralLayoutElement::Node,
                row: 3
            })
        );
    }

    #[test]
    fn sankey_columns_curved_ribbons_and_hits_share_geometry() {
        let mut chart = chart();
        let nodes = ["a", "b", "c"]
            .map(|id| FlowNode {
                id: id.into(),
                label: id.into(),
            })
            .to_vec();
        let links = vec![
            FlowLink {
                source: "a".into(),
                target: "b".into(),
                value: f64::MAX,
            },
            FlowLink {
                source: "a".into(),
                target: "c".into(),
                value: f64::MAX / 2.0,
            },
        ];
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Flow {
                nodes: nodes.clone(),
                links: links.clone(),
            })
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Sankey,
            ))
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let plot = chart.general_plot_rect(0).unwrap();
        let shapes = sankey_shapes(&nodes, &links, plot);
        assert_eq!(
            shapes
                .nodes
                .iter()
                .map(|shape| shape.row)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            shapes
                .links
                .iter()
                .map(|shape| shape.row)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(
            shapes
                .links
                .iter()
                .all(|shape| shape.x0 < shape.x1 && shape.thickness.is_finite())
        );
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
                .count(),
            2
        );
        for shape in &shapes.nodes {
            let x = shape.rect.x + shape.rect.width / 2.0;
            let y = shape.rect.y + shape.rect.height / 2.0;
            assert_eq!(
                chart.general_layout_hit_test(0, x, y),
                Some(GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Node,
                    row: shape.row
                })
            );
        }
        for shape in &shapes.links {
            // A quarter of the way along, the curve is far from the straight chord.
            let (x, y) = shape.point(0.25);
            assert_eq!(
                chart.general_layout_hit_test(0, x, y),
                Some(GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Link,
                    row: shape.row
                })
            );
        }
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Flow {
                    nodes,
                    links: vec![links[1].clone()],
                },
            )
            .unwrap();
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn flow_depth_has_a_bounded_policy() {
        let mut nodes = Vec::new();
        let mut links = Vec::new();
        for index in 0..=MAX_LAYOUT_DEPTH + 1 {
            nodes.push(FlowNode {
                id: index.to_string(),
                label: String::new(),
            });
            if index > 0 {
                links.push(FlowLink {
                    source: (index - 1).to_string(),
                    target: index.to_string(),
                    value: 1.0,
                });
            }
        }
        assert_eq!(
            GeneralLayoutInput::Flow { nodes, links }
                .validate()
                .unwrap_err()
                .code(),
            ErrorCode::ResourceLimit
        );
    }

    #[test]
    fn layout_store_reserves_geometry_capacity_before_binding() {
        let mut chart = chart();
        let nodes = vec![
            FlowNode {
                id: "a".into(),
                label: String::new(),
            },
            FlowNode {
                id: "b".into(),
                label: String::new(),
            },
        ];
        let links = (0..MAX_FLOW_LINKS)
            .map(|_| FlowLink {
                source: "a".into(),
                target: "b".into(),
                value: 1.0,
            })
            .collect();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Flow { nodes, links })
            .unwrap();
        let mut accepted = 0;
        for _ in 0..MAX_LAYOUT_SERIES {
            match chart.add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Sankey,
            )) {
                Ok(_) => accepted += 1,
                Err(error) => {
                    assert_eq!(error.code(), ErrorCode::ResourceLimit);
                    break;
                }
            }
        }
        assert!(accepted > 0 && accepted < MAX_LAYOUT_SERIES);
        assert!(chart.memory_usage().general_data_capacity_bytes <= MAX_LAYOUT_STORE_BYTES);
    }

    #[test]
    fn treemap_cells_default_to_one_flat_fill_and_color_value_opts_into_shading() {
        let rects = |nodes: Vec<HierarchyNode>| {
            let mut chart = chart();
            let dataset = chart
                .create_general_layout_dataset(GeneralLayoutInput::Hierarchy(nodes))
                .unwrap();
            let mut options =
                GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::Treemap);
            options.color = "#4c8bf5".into();
            chart.add_general_layout_series(options).unwrap();
            chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
            chart.build_frame().panes[0]
                .main
                .iter()
                .filter_map(|prim| match prim {
                    Prim::Rect { rect, color } => Some((*rect, *color)),
                    _ => None,
                })
                .collect::<Vec<(IRect, Color)>>()
        };
        let flat = rects(hierarchy(&[3.0, 2.0, 1.0]));
        assert_eq!(flat.len(), 4);
        assert_eq!(
            flat[0].1, TREEMAP_STROKE,
            "white stroke backdrop comes first"
        );
        let series = Color::parse_css("#4c8bf5").unwrap();
        assert!(
            flat[1..].iter().all(|(_, color)| *color == series),
            "Recharts paints every cell with the one flat fill: {flat:?}"
        );
        for (index, (a, _)) in flat[1..].iter().enumerate() {
            for (b, _) in flat[1..].iter().skip(index + 1) {
                let gap_x = (b.x - (a.x + a.w)).max(a.x - (b.x + b.w));
                let gap_y = (b.y - (a.y + a.h)).max(a.y - (b.y + b.h));
                assert!(
                    gap_x == 1 || gap_y == 1,
                    "1 px stroke between {a:?} and {b:?}"
                );
            }
        }

        let mut nodes = hierarchy(&[3.0, 2.0, 1.0]);
        for (node, change) in nodes.iter_mut().skip(1).zip([2.0, -1.0, 0.0]) {
            node.color_value = Some(change);
        }
        let colors: Vec<Color> = rects(nodes)[1..].iter().map(|(_, color)| *color).collect();
        assert_eq!(colors.len(), 3);
        assert!(
            colors.contains(&TREEMAP_UP),
            "largest gain is full up color"
        );
        assert!(colors.contains(&TREEMAP_FLAT), "unchanged cell is flat");
        let partial_loss = colors
            .iter()
            .find(|color| **color != TREEMAP_UP && **color != TREEMAP_FLAT)
            .unwrap();
        assert!(
            partial_loss.r() > partial_loss.g(),
            "losses shade toward down"
        );
    }

    #[test]
    fn treemap_squarify_matches_recharts_rectangles() {
        // Expected cells follow pinned Recharts 3.10.1 `squarify` by hand: golden-ratio worst
        // score, rows along the short side, whole-pixel rounding, remainder to the last cell.
        let plot = GeneralPlotRect {
            x: 11.0,
            y: 7.0,
            width: 600.0,
            height: 400.0,
        };
        let shapes = treemap_shapes(&hierarchy(&[6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0]), plot);
        let cells: Vec<(usize, [f64; 4])> = shapes
            .iter()
            .map(|shape| {
                (
                    shape.row,
                    [
                        shape.rect.x - plot.x,
                        shape.rect.y - plot.y,
                        shape.rect.width,
                        shape.rect.height,
                    ],
                )
            })
            .collect();
        assert_eq!(
            cells,
            vec![
                (1, [0.0, 0.0, 300.0, 200.0]),
                (2, [0.0, 200.0, 300.0, 200.0]),
                (3, [300.0, 0.0, 172.0, 233.0]),
                (4, [472.0, 0.0, 128.0, 233.0]),
                (5, [300.0, 233.0, 240.0, 83.0]),
                (6, [300.0, 316.0, 240.0, 84.0]),
                (7, [540.0, 233.0, 60.0, 167.0]),
            ]
        );
    }

    #[test]
    fn sankey_link_hit_follows_the_curve_not_the_chord() {
        let link = SankeyLinkShape {
            row: 0,
            x0: 0.0,
            x1: 100.0,
            y0: 0.0,
            y1: 100.0,
            thickness: 10.0,
        };
        // Recharts' control points sit at the horizontal midpoint, so the curve leaves and
        // enters each node horizontally.
        let (x, y) = link.point(0.25);
        assert!((x - 29.6875).abs() < 1e-9 && (y - 15.625).abs() < 1e-9);
        assert!(link.contains(x, y));
        assert!(link.contains(x, y + 4.0));
        assert!(
            !link.contains(x, x),
            "the straight chord is outside the ribbon"
        );
        assert!(link.contains(1.0, 4.0) && !link.contains(1.0, 6.0));
        assert!(link.contains(99.0, 96.0) && !link.contains(99.0, 94.0));
        assert!(link.segments() <= SANKEY_LINK_SEGMENTS);
    }

    #[test]
    fn sankey_nodes_use_their_own_fill_and_links_the_translucent_series_stroke() {
        let mut chart = chart();
        let nodes = ["a", "b", "c"]
            .map(|id| FlowNode {
                id: id.into(),
                label: id.into(),
            })
            .to_vec();
        let links = vec![
            FlowLink {
                source: "a".into(),
                target: "b".into(),
                value: 3.0,
            },
            FlowLink {
                source: "a".into(),
                target: "c".into(),
                value: 1.0,
            },
        ];
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Flow {
                nodes: nodes.clone(),
                links: links.clone(),
            })
            .unwrap();
        let mut options =
            GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::Sankey);
        options.color = "#8b6df6".into();
        chart.add_general_layout_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        let node_fills: Vec<Color> = main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Rect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(node_fills, vec![SANKEY_NODE_FILL; 3]);
        let link_fills: Vec<Color> = main
            .iter()
            .filter_map(|prim| match prim {
                Prim::MeshFill { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(link_fills, vec![Color::rgba(0x8b, 0x6d, 0xf6, 51); 2]);

        let plot = chart.general_plot_rect(0).unwrap();
        let cache = sankey_shapes(&nodes, &links, plot);
        // Recharts defaults: 5 px margin, 10 px nodes, 10 px padding, and the sink column
        // justified to the right edge.
        let rect = |row: usize| cache.nodes[row].rect;
        assert_eq!(rect(0).x, plot.x + SANKEY_MARGIN);
        assert_eq!(rect(0).width, SANKEY_NODE_WIDTH);
        assert!((rect(1).x + rect(1).width - (plot.x + plot.width - SANKEY_MARGIN)).abs() < 1e-9);
        assert!((rect(2).x - rect(1).x).abs() < 1e-9);
        let (upper, lower) = if rect(1).y < rect(2).y {
            (rect(1), rect(2))
        } else {
            (rect(2), rect(1))
        };
        assert!(lower.y - (upper.y + upper.height) >= SANKEY_NODE_PADDING - 1e-9);
        let total = plot.height - 2.0 * SANKEY_MARGIN;
        assert!((rect(0).height - (total - SANKEY_NODE_PADDING)).abs() < 1e-9);
    }

    #[test]
    fn sunburst_keeps_a_recharts_hole_and_paints_value_labels() {
        let mut chart = chart();
        let nodes = vec![
            HierarchyNode {
                id: "root".into(),
                parent: None,
                label: "Root".into(),
                value: None,
                color_value: None,
            },
            HierarchyNode {
                id: "a".into(),
                parent: Some("root".into()),
                label: "A".into(),
                value: None,
                color_value: None,
            },
            HierarchyNode {
                id: "b".into(),
                parent: Some("root".into()),
                label: "B".into(),
                value: Some(2.0),
                color_value: None,
            },
            HierarchyNode {
                id: "a1".into(),
                parent: Some("a".into()),
                label: "A1".into(),
                value: Some(3.0),
                color_value: None,
            },
            HierarchyNode {
                id: "a2".into(),
                parent: Some("a".into()),
                label: "A2".into(),
                value: Some(1.0),
                color_value: None,
            },
        ];
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Hierarchy(nodes.clone()))
            .unwrap();
        let mut options =
            GeneralLayoutSeriesOptions::new(0, dataset, GeneralLayoutSeriesKind::Sunburst);
        options.color = "#28b7a4".into();
        let series = chart.add_general_layout_series(options).unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let plot = chart.general_plot_rect(0).unwrap();
        let shapes = sunburst_shapes(&nodes, plot).shapes;
        let (cx, cy) = (plot.x + plot.width / 2.0, plot.y + plot.height / 2.0);
        let outer = plot.width.min(plot.height) / 2.0;
        // Recharts counts the root as one of three levels, so each ring is a third of the
        // space outside the 50 px hole, with 2 px between rings.
        let ring = (outer - SUNBURST_INNER_RADIUS) / 3.0;
        let a = shapes.iter().find(|shape| shape.row == 1).unwrap();
        let a1 = shapes.iter().find(|shape| shape.row == 3).unwrap();
        assert!((a.inner - SUNBURST_INNER_RADIUS).abs() < 1e-9);
        assert!((a.outer - a.inner - ring).abs() < 1e-9);
        assert!((a1.inner - (a.outer + SUNBURST_RING_PADDING)).abs() < 1e-9);
        assert_eq!(
            chart.general_layout_hit_test(0, cx, cy),
            None,
            "center hole"
        );
        assert_eq!(
            chart.general_layout_hit_test(0, cx, cy - SUNBURST_INNER_RADIUS + 5.0),
            None
        );
        assert_eq!(
            chart.general_layout_hit_test(0, cx + a.outer + 1.0, cy - 1.0),
            None,
            "ring padding"
        );
        let hit = |x: f64, y: f64| chart.general_layout_hit_test(0, x, y).map(|hit| hit.row);
        let middle = SUNBURST_INNER_RADIUS + ring / 2.0;
        // The first child starts at three o'clock and runs counterclockwise: A owns the top
        // two thirds of the turn and B the remaining lower right third.
        assert_eq!(hit(cx, cy - middle), Some(1));
        assert_eq!(hit(cx - middle, cy), Some(1));
        assert_eq!(hit(cx + 1.0, cy + middle), Some(2));
        assert_eq!(
            chart.general_layout_hit_test(0, cx, cy - middle),
            Some(GeneralLayoutHit {
                series,
                element: GeneralLayoutElement::Node,
                row: 1
            })
        );
        let frame = chart.build_frame();
        let main = &frame.panes[0].main;
        let mut labels: Vec<&str> = main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        labels.sort_unstable();
        assert_eq!(
            labels,
            vec!["1", "2", "3"],
            "leaf values; A has no own value"
        );
        let fill = Color::parse_css("#28b7a4").unwrap();
        assert!(
            main.iter()
                .filter_map(|prim| match prim {
                    Prim::MeshFill { color, .. } => Some(*color),
                    _ => None,
                })
                .all(|color| color == fill)
        );
        assert_eq!(
            main.iter()
                .filter(|prim| matches!(
                    prim,
                    Prim::Polyline { color, .. } if *color == SUNBURST_STROKE
                ))
                .count(),
            4
        );
    }
    #[test]
    fn sunburst_rings_preserve_parent_order_and_exact_hits() {
        let mut chart = chart();
        let nodes = vec![
            HierarchyNode {
                id: "root".into(),
                parent: None,
                label: "Root".into(),
                value: None,
                color_value: None,
            },
            HierarchyNode {
                id: "a".into(),
                parent: Some("root".into()),
                label: "A".into(),
                value: None,
                color_value: None,
            },
            HierarchyNode {
                id: "b".into(),
                parent: Some("root".into()),
                label: "B".into(),
                value: Some(2.0),
                color_value: None,
            },
            HierarchyNode {
                id: "a1".into(),
                parent: Some("a".into()),
                label: "A1".into(),
                value: Some(3.0),
                color_value: None,
            },
            HierarchyNode {
                id: "a2".into(),
                parent: Some("a".into()),
                label: "A2".into(),
                value: Some(1.0),
                color_value: None,
            },
        ];
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Hierarchy(nodes.clone()))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Sunburst,
            ))
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let plot = chart.general_plot_rect(0).unwrap();
        let shapes = sunburst_shapes(&nodes, plot).shapes;
        assert_eq!(
            shapes.iter().map(|shape| shape.row).collect::<Vec<_>>(),
            vec![1, 3, 4, 2]
        );
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
                .count(),
            4
        );
        for shape in &shapes {
            let angle = (shape.start + shape.end) / 2.0;
            let radius = (shape.inner + shape.outer) / 2.0;
            let x = shape.cx + radius * angle.cos();
            let y = shape.cy + radius * angle.sin();
            assert!(shape.contains(x, y));
            assert_eq!(
                chart.general_layout_hit_test(0, x, y),
                Some(GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Node,
                    row: shape.row
                })
            );
        }
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Hierarchy(hierarchy(&[0.0, 2.0])),
            )
            .unwrap();
        let frame = chart.build_frame();
        assert_eq!(
            frame.panes[0]
                .main
                .iter()
                .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn funnel_input_is_atomic_and_releases_its_lazy_store() {
        let mut chart = chart();
        assert_eq!(chart.memory_usage().general_data_capacity_bytes, 0);
        let invalid = GeneralLayoutInput::Funnel(vec![FunnelStage {
            id: "a".into(),
            label: "A".into(),
            value: f64::NAN,
        }]);
        assert_eq!(
            chart
                .create_general_layout_dataset(invalid)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        assert!(chart.general_layout_data.is_none());
        let mut oversized_id = String::with_capacity(MAX_LAYOUT_DATASET_BYTES + 1);
        oversized_id.push('x');
        assert_eq!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: oversized_id,
                    label: String::new(),
                    value: 1.0,
                }]))
                .unwrap_err()
                .code(),
            ErrorCode::ResourceLimit
        );
        assert!(chart.general_layout_data.is_none());
        let input = GeneralLayoutInput::Funnel(vec![
            FunnelStage {
                id: "a".into(),
                label: "A".into(),
                value: 10.0,
            },
            FunnelStage {
                id: "b".into(),
                label: "B".into(),
                value: 0.0,
            },
        ]);
        let id = chart.create_general_layout_dataset(input.clone()).unwrap();
        assert_eq!(chart.general_layout_dataset(id), Some(&input));
        assert!(chart.memory_usage().general_data_capacity_bytes > 0);
        let duplicate = GeneralLayoutInput::Funnel(vec![
            FunnelStage {
                id: "a".into(),
                label: "A".into(),
                value: 10.0,
            },
            FunnelStage {
                id: "a".into(),
                label: "Again".into(),
                value: 5.0,
            },
        ]);
        assert_eq!(
            chart
                .replace_general_layout_dataset(id, duplicate)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        assert_eq!(chart.general_layout_dataset(id), Some(&input));
        chart.remove_general_layout_dataset(id).unwrap();
        assert!(chart.general_layout_dataset(id).is_none());
        assert_eq!(chart.memory_usage().general_data_capacity_bytes, 0);
        assert_eq!(
            chart.remove_general_layout_dataset(id).unwrap_err().code(),
            ErrorCode::InvalidHandle
        );
    }

    #[test]
    fn hierarchy_rejects_orphans_cycles_and_internal_leaf_weight_mismatches() {
        let mut chart = chart();
        let root = HierarchyNode {
            id: "root".into(),
            parent: None,
            label: "Root".into(),
            value: None,
            color_value: None,
        };
        let leaf = HierarchyNode {
            id: "leaf".into(),
            parent: Some("root".into()),
            label: "Leaf".into(),
            value: Some(5.0),
            color_value: None,
        };
        let good = GeneralLayoutInput::Hierarchy(vec![leaf.clone(), root.clone()]);
        let id = chart.create_general_layout_dataset(good.clone()).unwrap();
        assert_eq!(chart.general_layout_dataset(id), Some(&good));
        let orphan = GeneralLayoutInput::Hierarchy(vec![
            root.clone(),
            HierarchyNode {
                parent: Some("absent".into()),
                ..leaf.clone()
            },
        ]);
        assert_eq!(
            chart
                .replace_general_layout_dataset(id, orphan)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        let mismatch = GeneralLayoutInput::Hierarchy(vec![
            HierarchyNode {
                value: Some(10.0),
                ..root.clone()
            },
            leaf.clone(),
        ]);
        assert_eq!(
            chart
                .replace_general_layout_dataset(id, mismatch)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        let cycle = GeneralLayoutInput::Hierarchy(vec![
            root,
            leaf,
            HierarchyNode {
                id: "a".into(),
                parent: Some("b".into()),
                label: "A".into(),
                value: None,
                color_value: None,
            },
            HierarchyNode {
                id: "b".into(),
                parent: Some("a".into()),
                label: "B".into(),
                value: None,
                color_value: None,
            },
        ]);
        assert_eq!(
            chart
                .replace_general_layout_dataset(id, cycle)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        assert_eq!(chart.general_layout_dataset(id), Some(&good));
    }

    #[test]
    fn flow_rejects_missing_endpoints_and_cycles_before_install() {
        let mut chart = chart();
        let nodes = vec![
            FlowNode {
                id: "a".into(),
                label: "A".into(),
            },
            FlowNode {
                id: "b".into(),
                label: "B".into(),
            },
            FlowNode {
                id: "c".into(),
                label: "C".into(),
            },
        ];
        let links = vec![
            FlowLink {
                source: "a".into(),
                target: "b".into(),
                value: 2.0,
            },
            FlowLink {
                source: "b".into(),
                target: "c".into(),
                value: 2.0,
            },
        ];
        let id = chart
            .create_general_layout_dataset(GeneralLayoutInput::Flow {
                nodes: nodes.clone(),
                links: links.clone(),
            })
            .unwrap();
        for bad in [
            FlowLink {
                source: "c".into(),
                target: "a".into(),
                value: 1.0,
            },
            FlowLink {
                source: "c".into(),
                target: "missing".into(),
                value: 1.0,
            },
            FlowLink {
                source: "c".into(),
                target: "c".into(),
                value: 1.0,
            },
        ] {
            let mut changed = links.clone();
            changed.push(bad);
            assert_eq!(
                chart
                    .replace_general_layout_dataset(
                        id,
                        GeneralLayoutInput::Flow {
                            nodes: nodes.clone(),
                            links: changed
                        }
                    )
                    .unwrap_err()
                    .code(),
                ErrorCode::InvalidData
            );
        }
        assert_eq!(
            chart.general_layout_dataset(id),
            Some(&GeneralLayoutInput::Flow { nodes, links })
        );
    }

    #[test]
    fn funnel_series_uses_the_ordered_frame_and_exact_hit_geometry() {
        let mut chart = chart();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Funnel(vec![
                FunnelStage {
                    id: "top".into(),
                    label: "Top".into(),
                    value: 10.0,
                },
                FunnelStage {
                    id: "middle".into(),
                    label: "Middle".into(),
                    value: 5.0,
                },
                FunnelStage {
                    id: "zero".into(),
                    label: "Zero".into(),
                    value: 0.0,
                },
            ]))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Funnel,
            ))
            .unwrap();
        assert_eq!(
            chart
                .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                    0,
                    dataset,
                    GeneralLayoutSeriesKind::Sankey,
                ))
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        assert_eq!(
            chart
                .replace_general_layout_dataset(dataset, GeneralLayoutInput::Hierarchy(Vec::new()))
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        assert!(!chart.remove_pane(0));
        assert_eq!(
            chart
                .remove_general_layout_dataset(dataset)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidData
        );
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame = chart.build_frame();
        let fills: Vec<_> = frame.panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::MeshFill { .. }))
            .collect();
        assert_eq!(fills.len(), 2);
        assert!(
            fills
                .iter()
                .all(|prim| matches!(prim, Prim::MeshFill { point_count: 6, .. }))
        );
        let plot = chart.general_plot_rect(0).unwrap();
        let center_x = plot.x + plot.width / 2.0;
        for (row, fraction) in [(0, 1.0 / 6.0), (1, 0.5)] {
            let y = plot.y + plot.height * fraction;
            assert_eq!(
                chart.general_layout_hit_test(0, center_x, y),
                Some(GeneralLayoutHit {
                    series,
                    element: GeneralLayoutElement::Stage,
                    row
                })
            );
        }
        assert_eq!(
            chart.general_layout_hit_test(0, center_x, plot.y + plot.height * 5.0 / 6.0),
            None
        );
        let pointer = crate::PointerInput {
            x: center_x,
            y: plot.y + plot.height / 6.0,
            ..Default::default()
        };
        chart.input_pointer_move(pointer, false);
        assert_eq!(chart.input_hover(), crate::ChartHover::Layout);
        assert_eq!(chart.general_layout_hovered_hit().unwrap().row, 0);
        chart.input_pointer_down(pointer, 1);
        chart.input_pointer_up(pointer);
        assert_eq!(chart.general_layout_selected_hit().unwrap().row, 0);
        let accessible = chart
            .general_layout_accessibility_snapshot(series, 0, 1)
            .unwrap();
        assert_eq!(accessible.total, 3);
        assert_eq!(accessible.items.len(), 1);
        assert!(accessible.items[0].selected);
        assert_eq!(accessible.items[0].label, "Top");
        assert!(chart.input_target_key_down(
            crate::ChartFocusTarget::LayoutDatum {
                series,
                element: GeneralLayoutElement::Stage,
                row: 1,
            },
            crate::ChartKey::Enter,
            crate::InputModifiers::default(),
        ));
        assert_eq!(chart.general_layout_selected_hit().unwrap().row, 1);
        assert!(chart.input_key_down(
            crate::ChartKey::Escape,
            crate::InputModifiers::default(),
            false,
            0.0,
        ));
        assert!(chart.general_layout_selected_hit().is_none());
        chart.input_pointer_down(pointer, 1);
        chart.input_pointer_up(pointer);
        assert!(chart.general_layout_selected_hit().is_some());
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: "replacement".into(),
                    label: "Replacement".into(),
                    value: 1.0,
                }]),
            )
            .unwrap();
        assert!(chart.general_layout_selected_hit().is_none());
        chart.remove_general_layout_series(series).unwrap();
        assert_eq!(
            chart
                .remove_general_layout_series(series)
                .unwrap_err()
                .code(),
            ErrorCode::InvalidHandle
        );
        chart.remove_general_layout_dataset(dataset).unwrap();
        assert_eq!(chart.memory_usage().general_data_capacity_bytes, 0);
    }
    #[test]
    fn layout_accessibility_pages_have_derived_tree_and_flow_values() {
        let mut chart = chart();
        let tree = chart
            .create_general_layout_dataset(GeneralLayoutInput::Hierarchy(hierarchy(&[3.0, 2.0])))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                tree,
                GeneralLayoutSeriesKind::Treemap,
            ))
            .unwrap();
        let root = chart
            .general_layout_accessibility_snapshot(series, 0, 1)
            .unwrap();
        assert_eq!(root.total, 3);
        assert_eq!(root.items[0].label, "Root");
        assert_eq!(root.items[0].value, Some(5.0));
        assert_eq!(
            chart.general_layout_hit_value(GeneralLayoutHit {
                series,
                element: GeneralLayoutElement::Node,
                row: 0,
            }),
            Some(5.0)
        );
        let leaf = chart
            .general_layout_accessibility_snapshot(series, 1, 1)
            .unwrap();
        assert_eq!(leaf.items[0].value, Some(3.0));
        assert!(
            chart
                .general_layout_accessibility_snapshot(series, 3, 1)
                .unwrap()
                .items
                .is_empty()
        );

        let flow = chart
            .create_general_layout_dataset(GeneralLayoutInput::Flow {
                nodes: vec![
                    FlowNode {
                        id: "source".into(),
                        label: "Source".into(),
                    },
                    FlowNode {
                        id: "target".into(),
                        label: "Target".into(),
                    },
                ],
                links: vec![FlowLink {
                    source: "source".into(),
                    target: "target".into(),
                    value: 7.0,
                }],
            })
            .unwrap();
        let flow_series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                flow,
                GeneralLayoutSeriesKind::Sankey,
            ))
            .unwrap();
        let snapshot = chart
            .general_layout_accessibility_snapshot(flow_series, 0, usize::MAX)
            .unwrap();
        assert_eq!(snapshot.total, 3);
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.items[0].value, Some(7.0));
        assert_eq!(snapshot.items[1].value, Some(7.0));
        assert_eq!(snapshot.items[2].element, GeneralLayoutElement::Link);
        assert_eq!(snapshot.items[2].value, Some(7.0));
        assert_eq!(
            chart.general_layout_hit_value(GeneralLayoutHit {
                series: flow_series,
                element: GeneralLayoutElement::Link,
                row: 0,
            }),
            Some(7.0)
        );
    }
    #[test]
    fn opt_in_layout_labels_share_the_ordered_frame_for_all_families() {
        let cases = [
            (
                GeneralLayoutSeriesKind::Funnel,
                GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: "stage".into(),
                    label: "Stage".into(),
                    value: 4.0,
                }]),
                "Stage",
            ),
            (
                GeneralLayoutSeriesKind::Treemap,
                GeneralLayoutInput::Hierarchy(hierarchy(&[4.0])),
                "Leaf 0",
            ),
            (
                GeneralLayoutSeriesKind::Sankey,
                GeneralLayoutInput::Flow {
                    nodes: vec![
                        FlowNode {
                            id: "source".into(),
                            label: "Source".into(),
                        },
                        FlowNode {
                            id: "target".into(),
                            label: "Target".into(),
                        },
                    ],
                    links: vec![FlowLink {
                        source: "source".into(),
                        target: "target".into(),
                        value: 4.0,
                    }],
                },
                "Source",
            ),
            (
                GeneralLayoutSeriesKind::Sunburst,
                GeneralLayoutInput::Hierarchy(hierarchy(&[4.0])),
                "Leaf 0",
            ),
        ];
        for (kind, input, label) in cases {
            let mut chart = chart();
            let dataset = chart.create_general_layout_dataset(input).unwrap();
            let mut options = GeneralLayoutSeriesOptions::new(0, dataset, kind);
            options.data_labels = true;
            chart.add_general_layout_series(options).unwrap();
            chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
            let frame = chart.build_frame();
            assert!(
                frame.panes[0]
                    .main
                    .iter()
                    .any(|prim| { matches!(prim, Prim::Text { text, .. } if text == label) }),
                "{kind:?} label absent from the ordered frame"
            );
        }
    }
    #[test]
    fn keyboard_layout_focus_is_a_shared_frame_primitive_and_clears_with_data() {
        let cases = [
            (
                GeneralLayoutSeriesKind::Funnel,
                GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: "stage".into(),
                    label: "Stage".into(),
                    value: 4.0,
                }]),
                GeneralLayoutElement::Stage,
                0,
            ),
            (
                GeneralLayoutSeriesKind::Treemap,
                GeneralLayoutInput::Hierarchy(hierarchy(&[4.0])),
                GeneralLayoutElement::Node,
                1,
            ),
            (
                GeneralLayoutSeriesKind::Sunburst,
                GeneralLayoutInput::Hierarchy(hierarchy(&[4.0])),
                GeneralLayoutElement::Node,
                1,
            ),
            (
                GeneralLayoutSeriesKind::Sankey,
                GeneralLayoutInput::Flow {
                    nodes: vec![
                        FlowNode {
                            id: "a".into(),
                            label: "A".into(),
                        },
                        FlowNode {
                            id: "b".into(),
                            label: "B".into(),
                        },
                    ],
                    links: vec![FlowLink {
                        source: "a".into(),
                        target: "b".into(),
                        value: 4.0,
                    }],
                },
                GeneralLayoutElement::Link,
                0,
            ),
        ];
        for (kind, input, element, row) in cases {
            let mut chart = chart();
            let dataset = chart.create_general_layout_dataset(input.clone()).unwrap();
            let series = chart
                .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                .unwrap();
            let hit = GeneralLayoutHit {
                series,
                element,
                row,
            };
            assert!(!chart.set_general_layout_accessibility_focus(hit, "bad-color", 14.0));
            assert!(!chart.set_general_layout_accessibility_focus(
                GeneralLayoutHit { row: 99, ..hit },
                "#2962ff",
                14.0
            ));
            assert!(chart.set_general_layout_accessibility_focus(hit, "#2962ff", 14.0));
            assert_eq!(chart.general_layout_accessibility_focused_hit(), Some(hit));
            chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
            let frame = chart.build_frame();
            assert!(
                frame.panes[0]
                    .main
                    .iter()
                    .any(|prim| matches!(prim, Prim::Circle { radius, .. } if *radius > 0.0)),
                "{kind:?} focus marker absent"
            );
            if matches!(
                kind,
                GeneralLayoutSeriesKind::Treemap | GeneralLayoutSeriesKind::Sunburst
            ) {
                let root = GeneralLayoutHit { row: 0, ..hit };
                assert!(chart.set_general_layout_accessibility_focus(root, "#2962ff", 14.0));
                assert!(
                    chart.build_frame().panes[0]
                        .main
                        .iter()
                        .any(|prim| matches!(prim, Prim::Circle { .. })),
                    "{kind:?} root focus marker absent"
                );
            }
            chart
                .replace_general_layout_dataset(dataset, input)
                .unwrap();
            assert_eq!(chart.general_layout_accessibility_focused_hit(), None);
            assert!(
                !chart.build_frame().panes[0]
                    .main
                    .iter()
                    .any(|prim| matches!(prim, Prim::Circle { .. }))
            );
        }
    }
    #[test]
    fn compatible_layout_values_interpolate_on_the_shared_clock() {
        let hierarchy_from = GeneralLayoutInput::Hierarchy(hierarchy(&[3.0, 1.0]));
        let hierarchy_to = GeneralLayoutInput::Hierarchy(hierarchy(&[1.0, 3.0]));
        let cases = [
            (
                GeneralLayoutSeriesKind::Funnel,
                GeneralLayoutInput::Funnel(vec![
                    FunnelStage {
                        id: "a".into(),
                        label: "A".into(),
                        value: 10.0,
                    },
                    FunnelStage {
                        id: "b".into(),
                        label: "B".into(),
                        value: 5.0,
                    },
                ]),
                GeneralLayoutInput::Funnel(vec![
                    FunnelStage {
                        id: "a".into(),
                        label: "A".into(),
                        value: 10.0,
                    },
                    FunnelStage {
                        id: "b".into(),
                        label: "B".into(),
                        value: 10.0,
                    },
                ]),
            ),
            (
                GeneralLayoutSeriesKind::Treemap,
                hierarchy_from.clone(),
                hierarchy_to.clone(),
            ),
            (
                GeneralLayoutSeriesKind::Sankey,
                GeneralLayoutInput::Flow {
                    nodes: vec![
                        FlowNode {
                            id: "a".into(),
                            label: "A".into(),
                        },
                        FlowNode {
                            id: "b".into(),
                            label: "B".into(),
                        },
                        FlowNode {
                            id: "c".into(),
                            label: "C".into(),
                        },
                    ],
                    links: vec![
                        FlowLink {
                            source: "a".into(),
                            target: "b".into(),
                            value: 3.0,
                        },
                        FlowLink {
                            source: "a".into(),
                            target: "c".into(),
                            value: 1.0,
                        },
                    ],
                },
                GeneralLayoutInput::Flow {
                    nodes: vec![
                        FlowNode {
                            id: "a".into(),
                            label: "A".into(),
                        },
                        FlowNode {
                            id: "b".into(),
                            label: "B".into(),
                        },
                        FlowNode {
                            id: "c".into(),
                            label: "C".into(),
                        },
                    ],
                    links: vec![
                        FlowLink {
                            source: "a".into(),
                            target: "b".into(),
                            value: 1.0,
                        },
                        FlowLink {
                            source: "a".into(),
                            target: "c".into(),
                            value: 3.0,
                        },
                    ],
                },
            ),
            (
                GeneralLayoutSeriesKind::Sunburst,
                hierarchy_from,
                hierarchy_to,
            ),
        ];
        for (kind, from, to) in cases {
            let mut chart = chart();
            let dataset = chart.create_general_layout_dataset(from).unwrap();
            chart
                .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                .unwrap();
            chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
            chart.set_general_transition_duration_ms(200.0).unwrap();
            assert!(chart.set_general_animation_time_ms(1_000.0));
            let original = chart.build_frame().panes[0].clone();
            chart
                .replace_general_layout_dataset(dataset, to.clone())
                .unwrap();
            assert_eq!(chart.general_layout_dataset(dataset), Some(&to));
            assert!(chart.general_transition_active());
            let probe = if kind == GeneralLayoutSeriesKind::Funnel {
                let plot = chart.general_plot_rect(0).unwrap();
                Some((plot.x + plot.width * 0.65, plot.y + plot.height * 0.75))
            } else {
                None
            };
            if let Some((x, y)) = probe {
                assert!(chart.general_layout_hit_test(0, x, y).is_none());
            }
            assert_eq!(
                chart.build_frame().panes[0],
                original,
                "{kind:?} initial motion frame"
            );
            assert!(chart.set_general_animation_time_ms(1_100.0));
            let middle = chart.build_frame().panes[0].clone();
            assert_ne!(middle, original, "{kind:?} motion did not advance");
            if let Some((x, y)) = probe {
                assert_eq!(chart.general_layout_hit_test(0, x, y).unwrap().row, 1);
            }
            assert!(!chart.set_general_animation_time_ms(1_050.0));
            assert!(chart.set_general_animation_time_ms(1_200.0));
            assert!(!chart.general_transition_active());
            let end = chart.build_frame().panes[0].clone();
            assert_ne!(end, middle, "{kind:?} motion did not complete");
        }
    }
    #[test]
    fn layout_motion_interrupts_from_presentation_and_reduced_motion_finishes() {
        let input = |second: f64| {
            GeneralLayoutInput::Funnel(vec![
                FunnelStage {
                    id: "a".into(),
                    label: "A".into(),
                    value: 10.0,
                },
                FunnelStage {
                    id: "b".into(),
                    label: "B".into(),
                    value: second,
                },
            ])
        };
        let mut chart = chart();
        let dataset = chart.create_general_layout_dataset(input(5.0)).unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Funnel,
            ))
            .unwrap();
        chart.set_general_transition_duration_ms(200.0).unwrap();
        assert!(chart.set_general_animation_time_ms(1_000.0));
        chart
            .replace_general_layout_dataset(dataset, input(10.0))
            .unwrap();
        assert!(chart.set_general_animation_time_ms(1_100.0));
        let displayed = |chart: &ChartEngine| match chart
            .general_layout_data
            .as_ref()
            .unwrap()
            .entry(dataset)
            .unwrap()
            .display_input()
        {
            GeneralLayoutInput::Funnel(stages) => stages[1].value,
            _ => unreachable!(),
        };
        assert_eq!(displayed(&chart), 7.5);
        chart
            .replace_general_layout_dataset(dataset, input(1.0))
            .unwrap();
        assert_eq!(displayed(&chart), 7.5);
        assert!(chart.set_general_animation_time_ms(1_200.0));
        assert!((displayed(&chart) - 4.25).abs() < 1e-10);
        chart.set_general_transition_reduced_motion(true);
        assert!(!chart.general_transition_active());
        assert_eq!(displayed(&chart), 1.0);
        chart.set_general_transition_reduced_motion(false);
        chart
            .replace_general_layout_dataset(dataset, input(8.0))
            .unwrap();
        assert!(chart.general_transition_active());
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: "only".into(),
                    label: "Only".into(),
                    value: 2.0,
                }]),
            )
            .unwrap();
        assert!(!chart.general_transition_active());
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: "only".into(),
                    label: "Only".into(),
                    value: 4.0,
                }]),
            )
            .unwrap();
        assert!(chart.general_transition_active());
        chart.remove_general_layout_series(series).unwrap();
        assert!(!chart.general_transition_active());
        assert!(chart.memory_usage().general_data_capacity_bytes < MAX_LAYOUT_STORE_BYTES);
    }
    #[test]
    fn layout_presentation_mutation_is_atomic_and_changes_frame_text() {
        let mut chart = chart();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Funnel(vec![FunnelStage {
                id: "one".into(),
                label: "Visible".into(),
                value: 4.0,
            }]))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Funnel,
            ))
            .unwrap();
        let before = chart.general_layout_series_options(series).unwrap().clone();
        assert_eq!(
            chart
                .update_general_layout_series_presentation(
                    series,
                    "Changed".into(),
                    "not-a-color".into(),
                    true,
                    true,
                )
                .unwrap_err()
                .code(),
            ErrorCode::InvalidOptions,
        );
        assert_eq!(chart.general_layout_series_options(series), Some(&before));
        chart
            .update_general_layout_series_presentation(
                series,
                "Changed".into(),
                "#008800".into(),
                true,
                true,
            )
            .unwrap();
        chart.recompute_layout_with_measure(true, |_, _| 14.0, |_, _| 0.0);
        let frame = chart.build_frame();
        assert!(
            frame.panes[0]
                .main
                .iter()
                .any(|prim| { matches!(prim, Prim::Text { text, .. } if text == "Visible") })
        );
        let options = chart.general_layout_series_options(series).unwrap();
        assert_eq!(options.title, "Changed");
        assert_eq!(options.color, "#008800");
        assert!(options.data_labels);
        let hit = GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Stage,
            row: 0,
        };
        chart.set_general_transition_duration_ms(200.0).unwrap();
        assert!(chart.set_general_animation_time_ms(1_000.0));
        chart
            .replace_general_layout_dataset(
                dataset,
                GeneralLayoutInput::Funnel(vec![FunnelStage {
                    id: "one".into(),
                    label: "Visible".into(),
                    value: 6.0,
                }]),
            )
            .unwrap();
        assert!(chart.general_transition_active());
        assert!(chart.select_general_layout_item(series, GeneralLayoutElement::Stage, 0));
        assert!(chart.set_general_layout_accessibility_focus(hit, "#2962ff", 14.0));
        chart
            .update_general_layout_series_presentation(
                series,
                "Changed".into(),
                "#008800".into(),
                true,
                false,
            )
            .unwrap();
        assert!(!chart.general_transition_active());
        assert_eq!(chart.general_layout_selected_hit(), None);
        assert_eq!(chart.general_layout_accessibility_focused_hit(), None);
        assert!(
            !chart.build_frame().panes[0]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::MeshFill { .. }))
        );
        chart
            .update_general_layout_series_presentation(
                series,
                "Changed".into(),
                "#008800".into(),
                true,
                true,
            )
            .unwrap();
        assert!(
            chart.build_frame().panes[0]
                .main
                .iter()
                .any(|prim| matches!(prim, Prim::MeshFill { .. }))
        );
    }
}
