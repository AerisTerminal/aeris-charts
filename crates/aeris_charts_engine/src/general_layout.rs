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

mod calendar;
mod chart3d;
#[cfg(feature = "complete")]
mod chord;
mod gantt;
#[cfg(feature = "complete")]
mod gauge;
mod geo;
#[cfg(feature = "complete")]
mod graph;
#[cfg(feature = "complete")]
mod icicle;
mod mosaic;
#[cfg(feature = "complete")]
mod network;
#[cfg(feature = "complete")]
pub(crate) use network::NetworkDragPhase;
#[cfg(feature = "complete")]
mod packing;
#[cfg(feature = "complete")]
mod tree;
#[cfg(feature = "complete")]
mod venn;
#[cfg(feature = "complete")]
mod word_cloud;

pub use calendar::{CalendarDay, CalendarInput};
pub use chart3d::{Axis3d, Point3d, Points3dInput, SurfaceInput};
#[cfg(feature = "complete")]
pub use chart3d::{Camera3d, Projection3d};
pub use gantt::GanttTask;
#[cfg(feature = "complete")]
pub use geo::MapView;
pub use geo::{
    ChoroplethScale, GeoAnchor, GeoFeature, GeoInput, GeoLink, GeoLinksInput, GeoPoint,
    GeoPointsInput, GeoProjection, GeoProjectionKind, GeoShape, GeoValue, QuantizeMethod,
};
pub use mosaic::MosaicCell;

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

/// Tooltip parts of one layout item.
pub(crate) struct LayoutItemTooltip {
    pub(crate) label: String,
    pub(crate) title: String,
    pub(crate) value: Option<f64>,
    /// The item's painted color.
    pub(crate) color: Color,
    /// Text that follows the formatted value, or replaces it when there is no value.
    pub(crate) detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralLayoutElement {
    Stage,
    Node,
    Link,
    /// A Gantt task bar or milestone; its row is the task's input index.
    Task,
    /// A calendar day; its row is the day offset from January 1 of the first covered year.
    Day,
    /// A Marimekko cell; its row is the cell's input index.
    Cell,
    /// A gauge value band; its row is the band's input index.
    Band,
    /// A gauge progress arc; its row is always 0.
    Progress,
    /// A gauge pointer; its row is always 0.
    Pointer,
    /// A venn exclusive region; its row is the region's input index.
    Region,
    /// A word cloud word; its row is the word's input index.
    Word,
    /// A map feature; its row is the feature's input index.
    Feature,
    /// A bubble map bubble; its row is the point's input index.
    Bubble,
    /// A 3D bar, scatter symbol or line vertex; its row is the point's input index.
    Point,
    /// A 3D surface grid sample; its row is `y index * x count + x index`.
    Sample,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct GeneralLayoutAccessibilityItem {
    pub element: GeneralLayoutElement,
    pub row: usize,
    pub id: String,
    pub label: String,
    pub value: Option<f64>,
    pub selected: bool,
    /// Whether a tree or dendrogram node with children shows them; `None` for every other item.
    #[cfg(feature = "complete")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Whether a word cloud word was placed; `None` for every other item.
    #[cfg(feature = "complete")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placed: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct GeneralLayoutAccessibilitySnapshot {
    pub series: GeneralLayoutSeriesId,
    pub title: String,
    pub total: usize,
    pub offset: usize,
    pub items: Vec<GeneralLayoutAccessibilityItem>,
    /// Items per grid column when the items form a column-major grid (a calendar's seven
    /// weekdays): the vertical arrows then move one item and the horizontal arrows one column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_stride: Option<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GeneralLayoutSeriesKind {
    Funnel,
    Treemap,
    Sankey,
    Sunburst,
    Gantt,
    CalendarHeatmap,
    Marimekko,
    /// A dial with a progress arc, value bands, ticks and a pointer (complete build).
    Gauge,
    /// A chord diagram over a graph dataset (complete build).
    Chord,
    /// A tidy node-link tree over a hierarchy dataset (complete build).
    Tree,
    /// A node-link tree with every leaf at the deepest level (complete build).
    Dendrogram,
    /// Nested circles sized by leaf value over a hierarchy dataset (complete build).
    CirclePacking,
    /// A rectangular partition with one band per depth over a hierarchy dataset (complete build).
    Icicle,
    /// A node-link network over a graph dataset with force, circular or fixed layout
    /// (complete build).
    Network,
    /// An area-proportional set diagram over a sets dataset (complete build).
    Venn,
    /// A weight-sized word placement over a words dataset (complete build).
    WordCloud,
    /// Projected map features filled by value over a geo dataset (complete build).
    Choropleth,
    /// Value-sized symbols at map positions over a geo points dataset (complete build).
    BubbleMap,
    /// Great-circle arcs between map positions over a geo links dataset (complete build).
    FlowMap,
    /// Boxes rising from zero to each point's height in a 3D axis box (complete build).
    Bar3d,
    /// Screen-sized symbols at 3D points (complete build).
    Scatter3d,
    /// A polyline through 3D points in input order (complete build).
    Line3d,
    /// A shaded, height-colored surface over a 3D grid (complete build).
    Surface3d,
}

impl GeneralLayoutSeriesKind {
    /// Layout families that exist only in the complete build; the standard build rejects them
    /// with `unsupported_operation` at series creation.
    pub fn requires_complete_build(self) -> bool {
        matches!(
            self,
            Self::Gauge
                | Self::Chord
                | Self::Tree
                | Self::Dendrogram
                | Self::CirclePacking
                | Self::Icicle
                | Self::Network
                | Self::Venn
                | Self::WordCloud
                | Self::Choropleth
                | Self::BubbleMap
                | Self::FlowMap
                | Self::Bar3d
                | Self::Scatter3d
                | Self::Line3d
                | Self::Surface3d
        )
    }

    /// Families whose nodes fold and unfold their subtrees.
    #[cfg(feature = "complete")]
    pub fn is_collapsible(self) -> bool {
        matches!(self, Self::Tree | Self::Dendrogram)
    }

    /// Whether the family can lay out in `orientation`. Every family accepts the default
    /// horizontal orientation; trees and dendrograms also lay out vertically and radially, and
    /// icicles vertically (the radial partition is the sunburst family).
    #[cfg(feature = "complete")]
    pub fn supports_orientation(self, orientation: HierarchyOrientation) -> bool {
        match orientation {
            HierarchyOrientation::Horizontal => true,
            HierarchyOrientation::Vertical => {
                matches!(self, Self::Tree | Self::Dendrogram | Self::Icicle)
            }
            HierarchyOrientation::Radial => matches!(self, Self::Tree | Self::Dendrogram),
        }
    }
}

/// Direction of a hierarchy layout: horizontal puts the root on the left, vertical on top, and
/// radial at the center.
#[cfg(feature = "complete")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HierarchyOrientation {
    #[default]
    Horizontal,
    Vertical,
    Radial,
}

#[cfg(feature = "complete")]
impl HierarchyOrientation {
    pub(crate) fn is_horizontal(&self) -> bool {
        *self == Self::Horizontal
    }
}

/// Placement of network nodes: a d3-force simulation, evenly on a circle in input order, or at
/// each node's input `x`/`y`.
#[cfg(feature = "complete")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkLayout {
    #[default]
    Force,
    Circular,
    Fixed,
}

#[cfg(feature = "complete")]
impl NetworkLayout {
    pub(crate) fn is_force(&self) -> bool {
        *self == Self::Force
    }
}

/// A network node held at a layout position, as a node drag leaves it.
#[cfg(feature = "complete")]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NetworkPin {
    pub id: String,
    pub x: f64,
    pub y: f64,
}

/// Pan offset in layout units and zoom factor of a network view.
#[cfg(feature = "complete")]
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NetworkView {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[cfg(feature = "complete")]
impl Default for NetworkView {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

#[cfg(feature = "complete")]
impl NetworkView {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// d3-force `forceLink` default distance.
#[cfg(feature = "complete")]
pub(crate) const DEFAULT_NETWORK_LINK_DISTANCE: f64 = 30.0;

#[cfg(feature = "complete")]
pub(crate) fn network_link_distance_default() -> f64 {
    DEFAULT_NETWORK_LINK_DISTANCE
}

#[cfg(feature = "complete")]
pub(crate) fn is_default_network_link_distance(distance: &f64) -> bool {
    *distance == DEFAULT_NETWORK_LINK_DISTANCE
}

/// Weight-to-font mapping of a word cloud.
#[cfg(feature = "complete")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordCloudScaling {
    /// Font size is linear in the weight.
    Linear,
    /// Font size follows the square root of the weight, so the painted area is linear.
    #[default]
    Sqrt,
}

#[cfg(feature = "complete")]
pub(crate) const DEFAULT_WORD_CLOUD_MIN_SIZE: f64 = 10.0;
#[cfg(feature = "complete")]
pub(crate) const DEFAULT_WORD_CLOUD_MAX_SIZE: f64 = 48.0;

#[cfg(feature = "complete")]
pub(crate) fn word_cloud_min_size_default() -> f64 {
    DEFAULT_WORD_CLOUD_MIN_SIZE
}

#[cfg(feature = "complete")]
pub(crate) fn word_cloud_max_size_default() -> f64 {
    DEFAULT_WORD_CLOUD_MAX_SIZE
}

#[cfg(feature = "complete")]
pub(crate) fn is_default_word_cloud_min_size(size: &f64) -> bool {
    *size == DEFAULT_WORD_CLOUD_MIN_SIZE
}

#[cfg(feature = "complete")]
pub(crate) fn is_default_word_cloud_max_size(size: &f64) -> bool {
    *size == DEFAULT_WORD_CLOUD_MAX_SIZE
}

#[cfg(feature = "complete")]
pub(crate) fn is_default_word_cloud_scaling(scaling: &WordCloudScaling) -> bool {
    *scaling == WordCloudScaling::Sqrt
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
    // The hierarchy, network and word cloud options below configure complete-build families
    // only. The standard build omits them, so its deserializer ignores them like any other
    // unknown key and its series creation rejects the families they configure.
    /// Tree, dendrogram and icicle direction; other families accept only the default.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "HierarchyOrientation::is_horizontal")]
    pub hierarchy_orientation: HierarchyOrientation,
    /// IDs of collapsed tree or dendrogram nodes. Each names a node with children; the engine
    /// toggles them through its input controller and drops IDs a data replacement removes.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed: Vec<String>,
    /// Network node placement; other families accept only the default.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "NetworkLayout::is_force")]
    pub network_layout: NetworkLayout,
    /// Rest length of force-layout links in layout units, `1..=1000`.
    #[cfg(feature = "complete")]
    #[serde(
        default = "network_link_distance_default",
        skip_serializing_if = "is_default_network_link_distance"
    )]
    pub network_link_distance: f64,
    /// Network nodes held in place. Dragging a node pins it where it is released; a data
    /// replacement drops pins of removed nodes.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<NetworkPin>,
    /// Positions of every settled network node, exported so a restore reuses them directly
    /// instead of re-simulating; a data replacement drops positions of removed nodes.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<NetworkPin>,
    /// Pan and zoom of a network series.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "NetworkView::is_default")]
    pub network_view: NetworkView,
    /// Smallest word cloud font size in media px, `4..=256`.
    #[cfg(feature = "complete")]
    #[serde(
        default = "word_cloud_min_size_default",
        skip_serializing_if = "is_default_word_cloud_min_size"
    )]
    pub word_cloud_min_size: f64,
    /// Largest word cloud font size in media px, `4..=256`.
    #[cfg(feature = "complete")]
    #[serde(
        default = "word_cloud_max_size_default",
        skip_serializing_if = "is_default_word_cloud_max_size"
    )]
    pub word_cloud_max_size: f64,
    /// Weight-to-font mapping of a word cloud.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "is_default_word_cloud_scaling")]
    pub word_cloud_scaling: WordCloudScaling,
    /// Rotate every second word of a word cloud by 90 degrees.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub word_cloud_rotate: bool,
    /// Pan and zoom of a map pane, held by every choropleth, bubble and flow map series of
    /// the pane; other families accept only the default.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "MapView::is_default")]
    pub map_view: MapView,
    /// Orbit camera of a 3D pane, held by every 3D series of the pane; other families accept
    /// only the default.
    #[cfg(feature = "complete")]
    #[serde(default, skip_serializing_if = "Camera3d::is_default")]
    pub camera_3d: Camera3d,
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
            #[cfg(feature = "complete")]
            hierarchy_orientation: HierarchyOrientation::Horizontal,
            #[cfg(feature = "complete")]
            collapsed: Vec::new(),
            #[cfg(feature = "complete")]
            network_layout: NetworkLayout::Force,
            #[cfg(feature = "complete")]
            network_link_distance: DEFAULT_NETWORK_LINK_DISTANCE,
            #[cfg(feature = "complete")]
            pinned: Vec::new(),
            #[cfg(feature = "complete")]
            positions: Vec::new(),
            #[cfg(feature = "complete")]
            network_view: NetworkView::default(),
            #[cfg(feature = "complete")]
            word_cloud_min_size: DEFAULT_WORD_CLOUD_MIN_SIZE,
            #[cfg(feature = "complete")]
            word_cloud_max_size: DEFAULT_WORD_CLOUD_MAX_SIZE,
            #[cfg(feature = "complete")]
            word_cloud_scaling: WordCloudScaling::Sqrt,
            #[cfg(feature = "complete")]
            word_cloud_rotate: false,
            #[cfg(feature = "complete")]
            map_view: MapView::default(),
            #[cfg(feature = "complete")]
            camera_3d: Camera3d::default(),
        }
    }

    /// Retained heap bytes of the option text, collapsed IDs and pins.
    fn heap_bytes(&self) -> usize {
        let text = self.title.capacity() + self.color.capacity();
        #[cfg(not(feature = "complete"))]
        {
            text
        }
        #[cfg(feature = "complete")]
        {
            text + self.collapsed.capacity() * std::mem::size_of::<String>()
                + self.collapsed.iter().map(String::capacity).sum::<usize>()
                + (self.pinned.capacity() + self.positions.capacity())
                    * std::mem::size_of::<NetworkPin>()
                + self
                    .pinned
                    .iter()
                    .chain(self.positions.iter())
                    .map(|pin| pin.id.capacity())
                    .sum::<usize>()
        }
    }

    #[cfg(feature = "complete")]
    fn has_default_network_options(&self) -> bool {
        self.network_layout == NetworkLayout::Force
            && self.network_link_distance == DEFAULT_NETWORK_LINK_DISTANCE
            && self.pinned.is_empty()
            && self.positions.is_empty()
            && self.network_view.is_default()
    }

    #[cfg(feature = "complete")]
    fn has_default_word_cloud_options(&self) -> bool {
        self.word_cloud_min_size == DEFAULT_WORD_CLOUD_MIN_SIZE
            && self.word_cloud_max_size == DEFAULT_WORD_CLOUD_MAX_SIZE
            && self.word_cloud_scaling == WordCloudScaling::Sqrt
            && !self.word_cloud_rotate
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

/// One graph vertex. `category` groups nodes for color; `value` raises the node's value above
/// its incoming plus outgoing edge value; `x`/`y` (both or neither) pin the node for fixed layouts.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

/// One directed graph edge, identified by its ordered `(source, target)` pair. Cycles and
/// self-links are allowed; a repeated pair is rejected.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    #[serde(default = "graph_edge_value_default")]
    pub value: f64,
}

fn graph_edge_value_default() -> f64 {
    1.0
}

/// A gauge dial: one value on a `min..max` scale with optional colored bands.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GaugeInput {
    pub value: f64,
    #[serde(default)]
    pub min: f64,
    #[serde(default = "gauge_max_default")]
    pub max: f64,
    /// Name shown under the dial center and announced for the pointer.
    #[serde(default)]
    pub label: String,
    /// Suffix appended to the value text, such as `%` or ` km/h`.
    #[serde(default)]
    pub unit: String,
    /// Fixed fraction digits of the value text; `None` prints the shortest exact form.
    #[serde(default)]
    pub decimals: Option<u8>,
    /// Bands in ascending `to` order; each starts where the previous one ends.
    #[serde(default)]
    pub bands: Vec<GaugeBand>,
    /// Paint a progress arc from `min` to the value over the track.
    #[serde(default)]
    pub progress: bool,
}

fn gauge_max_default() -> f64 {
    100.0
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GaugeBand {
    /// Upper bound of the band in value units.
    pub to: f64,
    pub color: String,
    #[serde(default)]
    pub label: String,
}

/// One exclusive venn region: the sets covering it and its area, venn.js-shaped. A region
/// listing one set is that set's exclusive core; `label` overrides the joined set names in
/// tooltips and accessibility text (complete build).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VennSet {
    pub sets: Vec<String>,
    pub size: f64,
    #[serde(default)]
    pub label: Option<String>,
}

/// One word cloud word; `color` overrides the series color (complete build).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WordCloudWord {
    pub text: String,
    pub weight: f64,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GeneralLayoutInput {
    Funnel(Vec<FunnelStage>),
    Hierarchy(Vec<HierarchyNode>),
    Flow {
        nodes: Vec<FlowNode>,
        links: Vec<FlowLink>,
    },
    /// Task rows on a time track. `now` is the host's current time in UTC epoch milliseconds:
    /// the engine never reads a wall clock, so frames stay deterministic and replayable.
    Gantt {
        tasks: Vec<GanttTask>,
        #[serde(default)]
        now: Option<f64>,
    },
    Calendar(CalendarInput),
    Mosaic(Vec<MosaicCell>),
    // The standard build keeps the seven complete-build variants below so it can reject their
    // data with the typed complete-build error, but skips their serde field code: persisted
    // data deserializes as an empty placeholder that validation then rejects.
    /// A gauge dial (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_gauge_fields")
    )]
    Gauge(GaugeInput),
    /// Shared graph store for relationship layouts (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    Graph {
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    },
    /// Exclusive venn regions, venn.js-shaped (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    Sets(Vec<VennSet>),
    /// Weighted words (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    Words(Vec<WordCloudWord>),
    /// Map features with values by feature id (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    Geo(GeoInput),
    /// Bubble map points (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    GeoPoints(GeoPointsInput),
    /// Flow map links (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    GeoLinks(GeoLinksInput),
    /// Rows of a 3D bar, scatter or line series (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    Points3d(Points3dInput),
    /// A 3D surface height grid (complete build).
    #[cfg_attr(
        not(feature = "complete"),
        serde(skip_serializing, deserialize_with = "skip_layout_fields")
    )]
    Surface(SurfaceInput),
}

#[cfg(not(feature = "complete"))]
fn skip_layout_fields<'de, D: serde::Deserializer<'de>, T: Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    <serde::de::IgnoredAny as serde::Deserialize>::deserialize(deserializer)?;
    Ok(T::default())
}

#[cfg(not(feature = "complete"))]
fn skip_gauge_fields<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<GaugeInput, D::Error> {
    skip_layout_fields::<D, ()>(deserializer)?;
    Ok(GaugeInput {
        value: 0.0,
        min: 0.0,
        max: 0.0,
        label: String::new(),
        unit: String::new(),
        decimals: None,
        bands: Vec::new(),
        progress: false,
    })
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
            Self::Gantt { tasks, .. } => gantt::semantic_values(tasks),
            Self::Calendar(calendar) => calendar::semantic_values(&calendar.days),
            Self::Mosaic(cells) => cells.iter().map(|cell| cell.value).collect(),
            #[cfg(feature = "complete")]
            Self::Gauge(gauge) => std::iter::once(gauge.value)
                .chain(gauge.bands.iter().map(|band| band.to))
                .collect(),
            #[cfg(feature = "complete")]
            Self::Graph { nodes, edges } => graph::semantic_values(nodes, edges),
            #[cfg(feature = "complete")]
            Self::Sets(sets) => sets.iter().map(|region| region.size).collect(),
            #[cfg(feature = "complete")]
            Self::Words(words) => words.iter().map(|word| word.weight).collect(),
            #[cfg(feature = "complete")]
            Self::Geo(input) => geo::semantic_values(input),
            #[cfg(feature = "complete")]
            Self::GeoPoints(input) => input
                .points
                .iter()
                .map(|point| point.value.unwrap_or(f64::NAN))
                .collect(),
            #[cfg(feature = "complete")]
            Self::GeoLinks(input) => input
                .links
                .iter()
                .map(|link| link.value.unwrap_or(f64::NAN))
                .collect(),
            #[cfg(feature = "complete")]
            Self::Points3d(input) => input.points.iter().map(|point| point.z).collect(),
            #[cfg(feature = "complete")]
            Self::Surface(input) => input.z.iter().map(|z| z.unwrap_or(f64::NAN)).collect(),
            #[cfg(not(feature = "complete"))]
            Self::Gauge(_)
            | Self::Graph { .. }
            | Self::Sets(_)
            | Self::Words(_)
            | Self::Geo(_)
            | Self::GeoPoints(_)
            | Self::GeoLinks(_)
            | Self::Points3d(_)
            | Self::Surface(_) => Vec::new(),
        }
    }

    fn semantic_value_count(&self) -> usize {
        match self {
            Self::Funnel(stages) => stages.len(),
            Self::Hierarchy(nodes) => nodes.len(),
            Self::Flow { nodes, links } => nodes.len() + links.len(),
            Self::Gantt { tasks, .. } => tasks.len(),
            Self::Calendar(calendar) => calendar::semantic_value_count(&calendar.days),
            Self::Mosaic(cells) => cells.len(),
            #[cfg(feature = "complete")]
            Self::Gauge(gauge) => 1 + gauge.bands.len(),
            #[cfg(feature = "complete")]
            Self::Graph { nodes, edges } => nodes.len() + edges.len(),
            #[cfg(feature = "complete")]
            Self::Sets(sets) => sets.len(),
            #[cfg(feature = "complete")]
            Self::Words(words) => words.len(),
            #[cfg(feature = "complete")]
            Self::Geo(input) => input.features.len(),
            #[cfg(feature = "complete")]
            Self::GeoPoints(input) => input.points.len(),
            #[cfg(feature = "complete")]
            Self::GeoLinks(input) => input.links.len(),
            #[cfg(feature = "complete")]
            Self::Points3d(input) => input.points.len(),
            #[cfg(feature = "complete")]
            Self::Surface(input) => input.z.len(),
            #[cfg(not(feature = "complete"))]
            Self::Gauge(_)
            | Self::Graph { .. }
            | Self::Sets(_)
            | Self::Words(_)
            | Self::Geo(_)
            | Self::GeoPoints(_)
            | Self::GeoLinks(_)
            | Self::Points3d(_)
            | Self::Surface(_) => 0,
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
            Self::Gantt { tasks, now } => gantt::validate(tasks, *now)?,
            Self::Calendar(calendar) => calendar::validate(calendar)?,
            Self::Mosaic(cells) => mosaic::validate(cells)?,
            #[cfg(feature = "complete")]
            Self::Gauge(gauge) => gauge::validate(gauge)?,
            #[cfg(feature = "complete")]
            Self::Graph { nodes, edges } => graph::validate(nodes, edges)?,
            #[cfg(feature = "complete")]
            Self::Sets(sets) => venn::validate(sets)?,
            #[cfg(feature = "complete")]
            Self::Words(words) => word_cloud::validate(words)?,
            #[cfg(feature = "complete")]
            Self::Geo(input) => geo::validate(input)?,
            #[cfg(feature = "complete")]
            Self::GeoPoints(input) => geo::validate_points(input)?,
            #[cfg(feature = "complete")]
            Self::GeoLinks(input) => geo::validate_links(input)?,
            #[cfg(feature = "complete")]
            Self::Points3d(input) => chart3d::validate_points(input)?,
            #[cfg(feature = "complete")]
            Self::Surface(input) => chart3d::validate_surface(input)?,
            #[cfg(not(feature = "complete"))]
            Self::Gauge(_)
            | Self::Graph { .. }
            | Self::Sets(_)
            | Self::Words(_)
            | Self::Geo(_)
            | Self::GeoPoints(_)
            | Self::GeoLinks(_)
            | Self::Points3d(_)
            | Self::Surface(_) => {
                return Err(crate::complete_build_required(
                    "gauge, graph, venn, word cloud, map and 3D layout datasets",
                ));
            }
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
            Self::Gantt { tasks, .. } => {
                tasks.capacity() * std::mem::size_of::<GanttTask>() + gantt::heap_bytes(tasks)
            }
            Self::Calendar(calendar) => calendar::heap_bytes(calendar),
            Self::Mosaic(cells) => {
                cells.capacity() * std::mem::size_of::<MosaicCell>() + mosaic::heap_bytes(cells)
            }
            #[cfg(feature = "complete")]
            Self::Gauge(gauge) => {
                gauge.label.capacity()
                    + gauge.unit.capacity()
                    + gauge.bands.capacity() * std::mem::size_of::<GaugeBand>()
                    + gauge
                        .bands
                        .iter()
                        .map(|band| band.color.capacity() + band.label.capacity())
                        .sum::<usize>()
            }
            #[cfg(feature = "complete")]
            Self::Graph { nodes, edges } => {
                nodes.capacity() * std::mem::size_of::<GraphNode>()
                    + edges.capacity() * std::mem::size_of::<GraphEdge>()
                    + nodes
                        .iter()
                        .map(|node| {
                            node.id.capacity()
                                + node.label.capacity()
                                + node.category.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
                    + edges
                        .iter()
                        .map(|edge| edge.source.capacity() + edge.target.capacity())
                        .sum::<usize>()
            }
            #[cfg(feature = "complete")]
            Self::Sets(sets) => {
                sets.capacity() * std::mem::size_of::<VennSet>()
                    + sets
                        .iter()
                        .map(|region| {
                            region.sets.capacity() * std::mem::size_of::<String>()
                                + region.sets.iter().map(String::capacity).sum::<usize>()
                                + region.label.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
            }
            #[cfg(feature = "complete")]
            Self::Words(words) => {
                words.capacity() * std::mem::size_of::<WordCloudWord>()
                    + words
                        .iter()
                        .map(|word| {
                            word.text.capacity() + word.color.as_ref().map_or(0, String::capacity)
                        })
                        .sum::<usize>()
            }
            #[cfg(feature = "complete")]
            Self::Geo(input) => geo::heap_bytes(input),
            #[cfg(feature = "complete")]
            Self::GeoPoints(input) => geo::points_heap_bytes(input),
            #[cfg(feature = "complete")]
            Self::GeoLinks(input) => geo::links_heap_bytes(input),
            #[cfg(feature = "complete")]
            Self::Points3d(input) => chart3d::points_heap_bytes(input),
            #[cfg(feature = "complete")]
            Self::Surface(input) => chart3d::surface_heap_bytes(input),
            #[cfg(not(feature = "complete"))]
            Self::Gauge(_)
            | Self::Graph { .. }
            | Self::Sets(_)
            | Self::Words(_)
            | Self::Geo(_)
            | Self::GeoPoints(_)
            | Self::GeoLinks(_)
            | Self::Points3d(_)
            | Self::Surface(_) => 0,
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
        #[cfg(feature = "complete")]
        (GeneralLayoutInput::Gauge(a), GeneralLayoutInput::Gauge(b)) => {
            a.bands.len() == b.bands.len()
        }
        #[cfg(feature = "complete")]
        (
            GeneralLayoutInput::Graph {
                nodes: an,
                edges: ae,
            },
            GeneralLayoutInput::Graph {
                nodes: bn,
                edges: be,
            },
        ) => {
            an.len() == bn.len()
                && ae.len() == be.len()
                && an.iter().zip(bn).all(|(left, right)| left.id == right.id)
                && ae
                    .iter()
                    .zip(be)
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
        #[cfg(feature = "complete")]
        (
            GeneralLayoutInput::Gauge(a),
            GeneralLayoutInput::Gauge(b),
            GeneralLayoutInput::Gauge(out),
        ) => {
            // Blending both ends keeps min < max and the band bounds increasing and inside them.
            out.value = blend(a.value, b.value);
            out.min = blend(a.min, b.min);
            out.max = blend(a.max, b.max);
            for ((left, right), item) in a.bands.iter().zip(&b.bands).zip(&mut out.bands) {
                item.to = blend(left.to, right.to);
            }
        }
        #[cfg(feature = "complete")]
        (
            GeneralLayoutInput::Graph {
                nodes: an,
                edges: ae,
            },
            GeneralLayoutInput::Graph {
                nodes: bn,
                edges: be,
            },
            GeneralLayoutInput::Graph {
                nodes: on,
                edges: oe,
            },
        ) => {
            let pair = |a: Option<f64>, b: Option<f64>| a.zip(b).map(|(a, b)| blend(a, b));
            for ((left, right), item) in an.iter().zip(bn).zip(on) {
                item.value = pair(left.value, right.value).or(right.value);
                item.x = pair(left.x, right.x).or(right.x);
                item.y = pair(left.y, right.y).or(right.y);
            }
            for ((left, right), item) in ae.iter().zip(be).zip(oe) {
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
    #[cfg(feature = "complete")]
    chord: RefCell<Option<chord::ChordCache>>,
    #[cfg(feature = "complete")]
    tree: RefCell<Option<tree::TreeCache>>,
    #[cfg(feature = "complete")]
    packing: RefCell<Option<packing::PackCache>>,
    #[cfg(feature = "complete")]
    icicle: RefCell<Option<icicle::IcicleCache>>,
    #[cfg(feature = "complete")]
    venn: RefCell<Option<venn::VennCache>>,
    #[cfg(feature = "complete")]
    word_cloud: RefCell<Option<word_cloud::WordCloudCache>>,
    #[cfg(feature = "complete")]
    geo: RefCell<Option<geo::GeoCache>>,
    #[cfg(feature = "complete")]
    marks: RefCell<Option<geo::MarkCache>>,
    /// Scene of the pane when this series is the pane's first 3D series.
    #[cfg(feature = "complete")]
    scene: RefCell<Option<chart3d::SceneCache>>,
    /// Simulated positions of a network series. Not a cache: positions evolve over time and
    /// survive data replacement, so only a series rebuild replaces them.
    #[cfg(feature = "complete")]
    network: Option<Box<network::NetworkSim>>,
    /// First Gantt row in the viewport. Clamped wherever it is read, so a data or size change
    /// never leaves the viewport past the last row.
    scroll_row: usize,
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
    /// Drops every cached layout of this series; each cache rebuilds from the next read.
    fn clear_caches(&self) {
        *self.treemap.borrow_mut() = None;
        *self.sankey.borrow_mut() = None;
        *self.sunburst.borrow_mut() = None;
        #[cfg(feature = "complete")]
        {
            *self.chord.borrow_mut() = None;
            *self.tree.borrow_mut() = None;
            *self.packing.borrow_mut() = None;
            *self.icicle.borrow_mut() = None;
            *self.venn.borrow_mut() = None;
            *self.word_cloud.borrow_mut() = None;
            *self.geo.borrow_mut() = None;
            *self.marks.borrow_mut() = None;
            *self.scene.borrow_mut() = None;
        }
    }

    fn relationship_cache_bytes(&self) -> usize {
        #[cfg(feature = "complete")]
        {
            self.chord
                .borrow()
                .as_ref()
                .map_or(0, chord::ChordCache::heap_bytes)
                + self
                    .tree
                    .borrow()
                    .as_ref()
                    .map_or(0, tree::TreeCache::heap_bytes)
                + self
                    .packing
                    .borrow()
                    .as_ref()
                    .map_or(0, packing::PackCache::heap_bytes)
                + self
                    .icicle
                    .borrow()
                    .as_ref()
                    .map_or(0, icicle::IcicleCache::heap_bytes)
                + self
                    .network
                    .as_deref()
                    .map_or(0, network::NetworkSim::heap_bytes)
                + self
                    .venn
                    .borrow()
                    .as_ref()
                    .map_or(0, venn::VennCache::heap_bytes)
                + self
                    .word_cloud
                    .borrow()
                    .as_ref()
                    .map_or(0, word_cloud::WordCloudCache::heap_bytes)
                + self
                    .geo
                    .borrow()
                    .as_ref()
                    .map_or(0, geo::GeoCache::heap_bytes)
                + self
                    .marks
                    .borrow()
                    .as_ref()
                    .map_or(0, geo::MarkCache::heap_bytes)
                + self
                    .scene
                    .borrow()
                    .as_ref()
                    .map_or(0, chart3d::SceneCache::heap_bytes)
        }
        #[cfg(not(feature = "complete"))]
        {
            0
        }
    }

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
        let Some(entry) = self.entry(series.options.dataset) else {
            return false;
        };
        match (&entry.input, hit.element) {
            (GeneralLayoutInput::Funnel(stages), GeneralLayoutElement::Stage) => {
                hit.row < stages.len()
            }
            (GeneralLayoutInput::Hierarchy(nodes), GeneralLayoutElement::Node) => {
                hit.row < nodes.len() && !hierarchy_row_hidden(series, nodes, hit.row)
            }
            (GeneralLayoutInput::Flow { nodes, .. }, GeneralLayoutElement::Node) => {
                hit.row < nodes.len()
            }
            (GeneralLayoutInput::Flow { links, .. }, GeneralLayoutElement::Link) => {
                hit.row < links.len()
            }
            (GeneralLayoutInput::Gantt { tasks, .. }, GeneralLayoutElement::Task) => {
                hit.row < tasks.len()
            }
            (GeneralLayoutInput::Calendar(_), GeneralLayoutElement::Day) => {
                hit.row < entry.semantic_values.len()
            }
            (GeneralLayoutInput::Mosaic(cells), GeneralLayoutElement::Cell) => {
                hit.row < cells.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(_), GeneralLayoutElement::Pointer) => hit.row == 0,
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(gauge), GeneralLayoutElement::Progress) => {
                hit.row == 0 && gauge.progress
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(gauge), GeneralLayoutElement::Band) => {
                hit.row < gauge.bands.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Graph { nodes, .. }, GeneralLayoutElement::Node) => {
                hit.row < nodes.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Graph { edges, .. }, GeneralLayoutElement::Link) => {
                hit.row < edges.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Sets(sets), GeneralLayoutElement::Region) => hit.row < sets.len(),
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Words(words), GeneralLayoutElement::Word) => hit.row < words.len(),
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Geo(input), GeneralLayoutElement::Feature) => {
                hit.row < input.features.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::GeoPoints(input), GeneralLayoutElement::Bubble) => {
                hit.row < input.points.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::GeoLinks(input), GeneralLayoutElement::Link) => {
                hit.row < input.links.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Points3d(input), GeneralLayoutElement::Point) => {
                hit.row < input.points.len()
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Surface(input), GeneralLayoutElement::Sample) => {
                hit.row < input.z.len()
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
            #[cfg(feature = "complete")]
            if !network::accepts_replacement(&series.options, &input) {
                return Err(invalid_data(
                    "a fixed network layout needs x and y on every node",
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
        #[cfg(feature = "complete")]
        let previous_input = std::mem::replace(&mut self.entries[index].input, input);
        #[cfg(not(feature = "complete"))]
        {
            self.entries[index].input = input;
        }
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
        #[cfg(feature = "complete")]
        if let GeneralLayoutInput::Hierarchy(nodes) = &self.entries[index].input {
            let parents: HashSet<&str> = nodes
                .iter()
                .filter_map(|node| node.parent.as_deref())
                .collect();
            for series in self
                .series
                .iter_mut()
                .filter(|series| series.options.dataset == id)
            {
                series
                    .options
                    .collapsed
                    .retain(|collapsed| parents.contains(collapsed.as_str()));
            }
        }
        #[cfg(feature = "complete")]
        if let (
            GeneralLayoutInput::Graph { nodes, edges },
            GeneralLayoutInput::Graph {
                nodes: previous, ..
            },
        ) = (&self.entries[index].input, &previous_input)
        {
            let values = &self.entries[index].semantic_values;
            for series in self
                .series
                .iter_mut()
                .filter(|series| series.options.dataset == id)
            {
                network::retain_pins(&mut series.options.pinned, nodes);
                network::retain_pins(&mut series.options.positions, nodes);
                series.rebuild_network(nodes, edges, values, Some(previous));
            }
        }
        for series in &self.series {
            if series.options.dataset == id {
                series.clear_caches();
            }
        }
        #[cfg(feature = "complete")]
        self.clear_scenes_showing(id);
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
            series.clear_caches();
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
                    series.options.heap_bytes()
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
                        + series.relationship_cache_bytes()
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
                    series.options.heap_bytes()
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
        #[cfg_attr(not(feature = "complete"), allow(unused_mut))]
        mut options: GeneralLayoutSeriesOptions,
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
        #[cfg(feature = "complete")]
        validate_relationship_options(&options, dataset)?;
        #[cfg(feature = "complete")]
        if options.kind == GeneralLayoutSeriesKind::WordCloud {
            word_cloud::validate_options(&options)?;
        }
        #[cfg(feature = "complete")]
        let network = if options.kind == GeneralLayoutSeriesKind::Network {
            network::validate_options(&options, dataset)?;
            let GeneralLayoutInput::Graph { nodes, edges } = dataset else {
                unreachable!("network series accept only graph datasets");
            };
            let values = self
                .entries
                .iter()
                .find(|entry| entry.id == options.dataset)
                .map_or(&[][..], |entry| &entry.semantic_values);
            Some(Box::new(network::NetworkSim::new(
                &options, nodes, edges, values, None,
            )))
        } else {
            None
        };
        // A pane has one camera: a later 3D series joins the camera of the pane's 3D series.
        #[cfg(feature = "complete")]
        if options.kind.is_3d()
            && let Some(shared) = self
                .series
                .iter()
                .find(|series| series.pane_id == pane_id && series.options.kind.is_3d())
        {
            options.camera_3d = shared.options.camera_3d;
        }
        if self
            .reserved_bytes()
            .saturating_add(options.heap_bytes())
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
            #[cfg(feature = "complete")]
            chord: RefCell::new(None),
            #[cfg(feature = "complete")]
            tree: RefCell::new(None),
            #[cfg(feature = "complete")]
            packing: RefCell::new(None),
            #[cfg(feature = "complete")]
            icicle: RefCell::new(None),
            #[cfg(feature = "complete")]
            venn: RefCell::new(None),
            #[cfg(feature = "complete")]
            word_cloud: RefCell::new(None),
            #[cfg(feature = "complete")]
            geo: RefCell::new(None),
            #[cfg(feature = "complete")]
            marks: RefCell::new(None),
            #[cfg(feature = "complete")]
            scene: RefCell::new(None),
            #[cfg(feature = "complete")]
            network,
            scroll_row: 0,
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

    pub(crate) fn persistence_series(
        &self,
    ) -> impl Iterator<Item = (GeneralLayoutSeriesId, &GeneralLayoutSeriesOptions)> {
        self.series.iter().map(|entry| (entry.id, &entry.options))
    }

    /// Settled force positions of a network series as persistable pins, or `None` while its
    /// simulation still moves nodes: an export captures rest state, never a half-settled
    /// layout.
    #[cfg(feature = "complete")]
    pub(crate) fn settled_positions(
        &self,
        series: GeneralLayoutSeriesId,
    ) -> Option<Vec<NetworkPin>> {
        let entry = self.series.iter().find(|entry| entry.id == series)?;
        let sim = entry.network.as_deref()?;
        let GeneralLayoutInput::Graph { nodes, .. } = &self.entry(entry.options.dataset)?.input
        else {
            return None;
        };
        sim.snapshot_positions(nodes)
    }

    /// Drop every word placement cache: text measurements changed and each word box must be
    /// re-measured and re-placed on the next frame.
    #[cfg(feature = "complete")]
    pub(crate) fn invalidate_word_cloud_caches(&self) {
        for series in &self.series {
            *series.word_cloud.borrow_mut() = None;
        }
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
                    Self::Treemap
                        | Self::Sunburst
                        | Self::Tree
                        | Self::Dendrogram
                        | Self::CirclePacking
                        | Self::Icicle,
                    GeneralLayoutInput::Hierarchy(_)
                )
                | (Self::Sankey, GeneralLayoutInput::Flow { .. })
                | (Self::Gantt, GeneralLayoutInput::Gantt { .. })
                | (Self::CalendarHeatmap, GeneralLayoutInput::Calendar(_))
                | (Self::Marimekko, GeneralLayoutInput::Mosaic(_))
        ) || self.accepts_complete(input)
    }

    #[cfg(feature = "complete")]
    fn accepts_complete(self, input: &GeneralLayoutInput) -> bool {
        matches!(
            (self, input),
            (Self::Gauge, GeneralLayoutInput::Gauge(_))
                | (
                    Self::Chord | Self::Network,
                    GeneralLayoutInput::Graph { .. }
                )
                | (Self::Venn, GeneralLayoutInput::Sets(_))
                | (Self::WordCloud, GeneralLayoutInput::Words(_))
                | (Self::Choropleth, GeneralLayoutInput::Geo(_))
                | (Self::BubbleMap, GeneralLayoutInput::GeoPoints(_))
                | (Self::FlowMap, GeneralLayoutInput::GeoLinks(_))
                | (
                    Self::Bar3d | Self::Scatter3d | Self::Line3d,
                    GeneralLayoutInput::Points3d(_)
                )
                | (Self::Surface3d, GeneralLayoutInput::Surface(_))
        )
    }

    #[cfg(not(feature = "complete"))]
    fn accepts_complete(self, _input: &GeneralLayoutInput) -> bool {
        false
    }
}

/// Device-pixel rectangle of layout-space bounds, or `None` when it rounds to nothing.
fn layout_irect(bounds: GeneralPlotRect, hpr: f64, vpr: f64) -> Option<IRect> {
    let x0 = (bounds.x * hpr).round() as i32;
    let y0 = (bounds.y * vpr).round() as i32;
    let x1 = ((bounds.x + bounds.width) * hpr).round() as i32;
    let y1 = ((bounds.y + bounds.height) * vpr).round() as i32;
    (x1 > x0 && y1 > y0).then_some(IRect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
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
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::Chord, GeneralLayoutInput::Graph { nodes, edges }) => {
            chord::cache_reservation(nodes.len(), edges.len())
        }
        #[cfg(feature = "complete")]
        (
            GeneralLayoutSeriesKind::Tree | GeneralLayoutSeriesKind::Dendrogram,
            GeneralLayoutInput::Hierarchy(nodes),
        ) => tree::cache_reservation(nodes.len()),
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::CirclePacking, GeneralLayoutInput::Hierarchy(nodes)) => {
            packing::cache_reservation(nodes.len())
        }
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::Icicle, GeneralLayoutInput::Hierarchy(nodes)) => {
            icicle::cache_reservation(nodes.len())
        }
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::Network, GeneralLayoutInput::Graph { nodes, edges }) => {
            network::cache_reservation(nodes.len(), edges.len())
        }
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::Venn, GeneralLayoutInput::Sets(_)) => venn::cache_reservation(),
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::WordCloud, GeneralLayoutInput::Words(_)) => {
            word_cloud::cache_reservation()
        }
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::Choropleth, GeneralLayoutInput::Geo(input)) => {
            geo::cache_reservation(input)
        }
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::BubbleMap, GeneralLayoutInput::GeoPoints(input)) => {
            geo::mark_cache_reservation(geo::MarkInput::Points(input))
        }
        #[cfg(feature = "complete")]
        (GeneralLayoutSeriesKind::FlowMap, GeneralLayoutInput::GeoLinks(input)) => {
            geo::mark_cache_reservation(geo::MarkInput::Links(input))
        }
        #[cfg(feature = "complete")]
        (kind, input) if kind.is_3d() => chart3d::cache_reservation(kind, input),
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
            if let Some(entry) = store.entry(series.options.dataset) {
                match (series.options.kind, entry.display_input()) {
                    (GeneralLayoutSeriesKind::Gantt, GeneralLayoutInput::Gantt { tasks, now }) => {
                        self.build_gantt_frame(
                            series,
                            (tasks, *now),
                            plot,
                            (hpr, vpr),
                            out,
                            points,
                        );
                    }
                    (
                        GeneralLayoutSeriesKind::CalendarHeatmap,
                        GeneralLayoutInput::Calendar(calendar),
                    ) => self.build_calendar_frame(
                        series,
                        (calendar, &entry.semantic_values),
                        plot,
                        (hpr, vpr),
                        out,
                        points,
                    ),
                    (GeneralLayoutSeriesKind::Marimekko, GeneralLayoutInput::Mosaic(cells)) => {
                        self.build_mosaic_frame(series, cells, plot, (hpr, vpr), out);
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::Gauge, GeneralLayoutInput::Gauge(gauge)) => {
                        let GeneralLayoutInput::Gauge(target) = &entry.input else {
                            unreachable!("a transition keeps the input variant");
                        };
                        self.build_gauge_frame(
                            series,
                            (gauge, target),
                            plot,
                            (hpr, vpr),
                            out,
                            points,
                        );
                    }
                    #[cfg(feature = "complete")]
                    (
                        GeneralLayoutSeriesKind::Chord,
                        GeneralLayoutInput::Graph { nodes, edges },
                    ) => {
                        self.build_chord_frame(
                            series,
                            (nodes, edges),
                            plot,
                            (hpr, vpr),
                            out,
                            points,
                        );
                    }
                    #[cfg(feature = "complete")]
                    (
                        GeneralLayoutSeriesKind::Tree | GeneralLayoutSeriesKind::Dendrogram,
                        GeneralLayoutInput::Hierarchy(nodes),
                    ) => self.build_tree_frame(series, nodes, plot, (hpr, vpr), out, points),
                    #[cfg(feature = "complete")]
                    (
                        GeneralLayoutSeriesKind::CirclePacking,
                        GeneralLayoutInput::Hierarchy(nodes),
                    ) => self.build_packing_frame(series, nodes, plot, (hpr, vpr), out),
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::Icicle, GeneralLayoutInput::Hierarchy(nodes)) => {
                        self.build_icicle_frame(series, nodes, plot, (hpr, vpr), out);
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::Network, GeneralLayoutInput::Graph { nodes, .. }) => {
                        self.build_network_frame(series, nodes, plot, (hpr, vpr), out, points);
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::Venn, GeneralLayoutInput::Sets(sets)) => {
                        self.build_venn_frame(series, sets, plot, (hpr, vpr), out);
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::WordCloud, GeneralLayoutInput::Words(words)) => {
                        self.build_word_cloud_frame(series, words, plot, (hpr, vpr), out);
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::Choropleth, GeneralLayoutInput::Geo(input)) => {
                        self.build_choropleth_frame(
                            series,
                            (input, &entry.semantic_values),
                            plot,
                            &store.map_transform(pane_id, plot),
                            (hpr, vpr),
                            out,
                            points,
                        );
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::BubbleMap, GeneralLayoutInput::GeoPoints(input)) => {
                        self.build_mark_frame(
                            series,
                            store,
                            geo::MarkInput::Points(input),
                            plot,
                            &store.map_transform(pane_id, plot),
                            (hpr, vpr),
                            out,
                            points,
                        );
                    }
                    #[cfg(feature = "complete")]
                    (GeneralLayoutSeriesKind::FlowMap, GeneralLayoutInput::GeoLinks(input)) => {
                        self.build_mark_frame(
                            series,
                            store,
                            geo::MarkInput::Links(input),
                            plot,
                            &store.map_transform(pane_id, plot),
                            (hpr, vpr),
                            out,
                            points,
                        );
                    }
                    // One pane paints its whole 3D scene once, at its first visible 3D series,
                    // so pieces of different series depth-sort together.
                    #[cfg(feature = "complete")]
                    (kind, _) if kind.is_3d() && store.scene_drawer(pane_id) == Some(series.id) => {
                        self.build_scene_frame(store, pane_id, plot, (hpr, vpr), out, points);
                    }
                    _ => {}
                }
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
                _ => None,
            }),
            (
                GeneralLayoutInput::Gantt { tasks, .. },
                GeneralLayoutSeriesKind::Gantt,
                GeneralLayoutElement::Task,
            ) => gantt::anchor(tasks, plot, series.scroll_row, hit.row),
            (
                GeneralLayoutInput::Calendar(calendar),
                GeneralLayoutSeriesKind::CalendarHeatmap,
                GeneralLayoutElement::Day,
            ) => self.calendar_anchor(calendar, plot, hit.row),
            (
                GeneralLayoutInput::Mosaic(cells),
                GeneralLayoutSeriesKind::Marimekko,
                GeneralLayoutElement::Cell,
            ) => mosaic::MosaicLayout::new(cells, plot).anchor(hit.row),
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(gauge), GeneralLayoutSeriesKind::Gauge, element) => {
                gauge::GaugeLayout::new(gauge, plot).anchor(element, hit.row)
            }
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Graph { nodes, edges },
                GeneralLayoutSeriesKind::Chord,
                element,
            ) => series.with_chord(nodes, edges, plot, |cache| cache.anchor(element, hit.row)),
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Hierarchy(nodes),
                GeneralLayoutSeriesKind::Tree | GeneralLayoutSeriesKind::Dendrogram,
                GeneralLayoutElement::Node,
            ) => series.with_tree(nodes, plot, |cache| cache.anchor(hit.row)),
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Hierarchy(nodes),
                GeneralLayoutSeriesKind::CirclePacking,
                GeneralLayoutElement::Node,
            ) => series.with_packing(nodes, plot, |cache| cache.anchor(hit.row)),
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Hierarchy(nodes),
                GeneralLayoutSeriesKind::Icicle,
                GeneralLayoutElement::Node,
            ) => series.with_icicle(nodes, plot, |cache| cache.anchor(hit.row)),
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Graph { .. }, GeneralLayoutSeriesKind::Network, element) => {
                series.network_anchor(plot, element, hit.row)
            }
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Sets(sets),
                GeneralLayoutSeriesKind::Venn,
                GeneralLayoutElement::Region,
            ) => series.with_venn(sets, plot, |cache| cache.anchor(hit.row)),
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Words(words),
                GeneralLayoutSeriesKind::WordCloud,
                GeneralLayoutElement::Word,
            ) => series.with_word_cloud(self, words, plot, |cache| cache.anchor(hit.row)),
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Geo(input),
                GeneralLayoutSeriesKind::Choropleth,
                GeneralLayoutElement::Feature,
            ) => {
                let view = store.map_transform(pane_id, plot);
                series.with_geo(input, plot, |cache| cache.anchor_view(&view, hit.row))
            }
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::GeoPoints(input),
                GeneralLayoutSeriesKind::BubbleMap,
                GeneralLayoutElement::Bubble,
            ) => {
                let view = store.map_transform(pane_id, plot);
                series.with_marks(store, geo::MarkInput::Points(input), plot, |cache| {
                    cache.anchor(&view, hit.element, hit.row)
                })
            }
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::GeoLinks(input),
                GeneralLayoutSeriesKind::FlowMap,
                GeneralLayoutElement::Link,
            ) => {
                let view = store.map_transform(pane_id, plot);
                series.with_marks(store, geo::MarkInput::Links(input), plot, |cache| {
                    cache.anchor(&view, hit.element, hit.row)
                })
            }
            #[cfg(feature = "complete")]
            (_, kind, _) if kind.is_3d() => store.scene_anchor(pane_id, plot, hit),
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
            // The pane's scene picks across every 3D series at once, through its drawer.
            #[cfg(feature = "complete")]
            if series.options.kind.is_3d() {
                if store.scene_drawer(pane_id) == Some(series.id)
                    && let Some(hit) = store.scene_hit(pane_id, plot, x, y)
                {
                    return Some(hit);
                }
                continue;
            }
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
            let Some(entry) = store.entry(series.options.dataset) else {
                continue;
            };
            let hit = match (series.options.kind, entry.display_input()) {
                (GeneralLayoutSeriesKind::Gantt, GeneralLayoutInput::Gantt { tasks, .. }) => {
                    gantt::hit(tasks, plot, series.scroll_row, x, y)
                        .map(|row| (GeneralLayoutElement::Task, row))
                }
                (
                    GeneralLayoutSeriesKind::CalendarHeatmap,
                    GeneralLayoutInput::Calendar(calendar),
                ) => self
                    .calendar_hit(calendar, &entry.semantic_values, plot, x, y)
                    .map(|row| (GeneralLayoutElement::Day, row)),
                (GeneralLayoutSeriesKind::Marimekko, GeneralLayoutInput::Mosaic(cells)) => {
                    mosaic::MosaicLayout::new(cells, plot)
                        .hit(x, y)
                        .map(|row| (GeneralLayoutElement::Cell, row))
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::Gauge, GeneralLayoutInput::Gauge(gauge)) => {
                    gauge::GaugeLayout::new(gauge, plot).hit(x, y)
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::Chord, GeneralLayoutInput::Graph { nodes, edges }) => {
                    series.with_chord(nodes, edges, plot, |cache| cache.hit(x, y))
                }
                #[cfg(feature = "complete")]
                (
                    GeneralLayoutSeriesKind::Tree | GeneralLayoutSeriesKind::Dendrogram,
                    GeneralLayoutInput::Hierarchy(nodes),
                ) => series
                    .with_tree(nodes, plot, |cache| cache.hit(x, y))
                    .map(|row| (GeneralLayoutElement::Node, row)),
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::CirclePacking, GeneralLayoutInput::Hierarchy(nodes)) => {
                    series
                        .with_packing(nodes, plot, |cache| cache.hit(x, y))
                        .map(|row| (GeneralLayoutElement::Node, row))
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::Icicle, GeneralLayoutInput::Hierarchy(nodes)) => series
                    .with_icicle(nodes, plot, |cache| cache.hit(x, y))
                    .map(|row| (GeneralLayoutElement::Node, row)),
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::Network, GeneralLayoutInput::Graph { .. }) => {
                    series.network_hit(plot, x, y)
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::Venn, GeneralLayoutInput::Sets(sets)) => {
                    series.with_venn(sets, plot, |cache| cache.hit(x, y))
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::WordCloud, GeneralLayoutInput::Words(words)) => {
                    series.with_word_cloud(self, words, plot, |cache| cache.hit(x, y))
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::Choropleth, GeneralLayoutInput::Geo(input)) => {
                    let view = store.map_transform(pane_id, plot);
                    series.with_geo(input, plot, |cache| cache.hit_view(&view, x, y))
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::BubbleMap, GeneralLayoutInput::GeoPoints(input)) => {
                    let view = store.map_transform(pane_id, plot);
                    series.with_marks(store, geo::MarkInput::Points(input), plot, |cache| {
                        cache.hit(&view, input.symbol, false, x, y)
                    })
                }
                #[cfg(feature = "complete")]
                (GeneralLayoutSeriesKind::FlowMap, GeneralLayoutInput::GeoLinks(input)) => {
                    let view = store.map_transform(pane_id, plot);
                    series.with_marks(store, geo::MarkInput::Links(input), plot, |cache| {
                        cache.hit(&view, crate::GeneralPointSymbol::Circle, input.arrows, x, y)
                    })
                }
                _ => None,
            };
            if let Some((element, row)) = hit {
                return Some(GeneralLayoutHit {
                    series: series.id,
                    element,
                    row,
                });
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
        let Some(store) = self.general_layout_data.as_ref() else {
            return false;
        };
        if !store.contains_hit(hit) {
            return false;
        }
        let revealed = self.reveal_general_layout_hit(hit);
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        if !revealed
            && store.focused == Some(hit)
            && store.focus_color == parsed
            && store.focus_size == size
        {
            return true;
        }
        store.focused = Some(hit);
        store.focus_color = parsed;
        store.focus_size = size;
        self.invalidate_frame_all();
        true
    }

    /// Scrolls a bounded Gantt series so the task in `hit` is inside its row viewport. Returns
    /// whether the viewport moved.
    fn reveal_general_layout_hit(&mut self, hit: GeneralLayoutHit) -> bool {
        if hit.element != GeneralLayoutElement::Task {
            return false;
        }
        let Some(store) = self.general_layout_data.as_ref() else {
            return false;
        };
        let Some(index) = store
            .series
            .iter()
            .position(|series| series.id == hit.series)
        else {
            return false;
        };
        let series = &store.series[index];
        let Some(plot) = self
            .pane_index_for_id(series.pane_id)
            .and_then(|pane| self.general_plot_rect(pane))
        else {
            return false;
        };
        let Some(GeneralLayoutInput::Gantt { tasks, .. }) = store
            .entry(series.options.dataset)
            .map(LayoutDatasetEntry::display_input)
        else {
            return false;
        };
        let scroll_row = gantt::reveal(tasks, plot, series.scroll_row, hit.row);
        self.set_general_layout_scroll_row(index, scroll_row)
    }

    /// Scrolls the topmost visible Gantt series in `pane` under `(x, y)` by a wheel delta
    /// (browser-normalized, wheel up positive). Returns whether its row viewport moved; a
    /// viewport already at its end leaves the wheel to the remaining wheel policy.
    pub(crate) fn scroll_general_layout_rows(
        &mut self,
        pane: usize,
        x: f64,
        y: f64,
        delta_y: f64,
    ) -> bool {
        let (Some(pane_id), Some(plot)) = (self.pane_stable_id(pane), self.general_plot_rect(pane))
        else {
            return false;
        };
        if x < plot.x || x >= plot.x + plot.width || y < plot.y || y >= plot.y + plot.height {
            return false;
        }
        let Some(store) = self.general_layout_data.as_ref() else {
            return false;
        };
        let target = store
            .series
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, series)| {
                series.pane_id == pane_id
                    && series.options.visible
                    && series.options.kind == GeneralLayoutSeriesKind::Gantt
            })
            .find_map(|(index, series)| {
                let GeneralLayoutInput::Gantt { tasks, .. } =
                    store.entry(series.options.dataset)?.display_input()
                else {
                    return None;
                };
                gantt::scrolled(tasks, plot, series.scroll_row, gantt::wheel_rows(delta_y))
                    .map(|scroll_row| (index, scroll_row))
            });
        target.is_some_and(|(index, scroll_row)| {
            self.set_general_layout_scroll_row(index, scroll_row)
        })
    }

    fn set_general_layout_scroll_row(&mut self, index: usize, scroll_row: usize) -> bool {
        let Some(series) = self
            .general_layout_data
            .as_mut()
            .and_then(|store| store.series.get_mut(index))
        else {
            return false;
        };
        if series.scroll_row == scroll_row {
            return false;
        }
        series.scroll_row = scroll_row;
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
            (GeneralLayoutInput::Gantt { .. }, GeneralLayoutElement::Task)
            | (GeneralLayoutInput::Calendar(_), GeneralLayoutElement::Day)
            | (GeneralLayoutInput::Mosaic(_), GeneralLayoutElement::Cell) => hit.row,
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Sets(_), GeneralLayoutElement::Region)
            | (GeneralLayoutInput::Words(_), GeneralLayoutElement::Word)
            | (GeneralLayoutInput::Graph { .. }, GeneralLayoutElement::Node)
            | (GeneralLayoutInput::Geo(_), GeneralLayoutElement::Feature)
            | (GeneralLayoutInput::GeoPoints(_), GeneralLayoutElement::Bubble)
            | (GeneralLayoutInput::GeoLinks(_), GeneralLayoutElement::Link)
            | (GeneralLayoutInput::Points3d(_), GeneralLayoutElement::Point)
            | (GeneralLayoutInput::Surface(_), GeneralLayoutElement::Sample) => hit.row,
            #[cfg(feature = "complete")]
            (
                GeneralLayoutInput::Gauge(_),
                GeneralLayoutElement::Pointer | GeneralLayoutElement::Progress,
            ) => 0,
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(_), GeneralLayoutElement::Band) => hit.row.checked_add(1)?,
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Graph { nodes, .. }, GeneralLayoutElement::Link) => {
                nodes.len().checked_add(hit.row)?
            }
            _ => return None,
        };
        // Calendar days without data hold NaN so day offsets stay addressable.
        entry
            .semantic_values
            .get(index)
            .copied()
            .filter(|value| !value.is_nan())
    }

    /// Host-facing identity and label of a Gantt, calendar, Marimekko, gauge, or graph item.
    pub fn general_layout_item_identity(&self, hit: GeneralLayoutHit) -> Option<(String, String)> {
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| series.id == hit.series)?;
        match (&store.entry(series.options.dataset)?.input, hit.element) {
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(gauge), element) => {
                self.gauge_item_identity(gauge, element, hit.row)
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Graph { nodes, edges }, element) => {
                self.graph_item_identity(nodes, edges, element, hit.row)
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Hierarchy(nodes), GeneralLayoutElement::Node) => {
                let node = nodes.get(hit.row)?;
                Some((node.id.clone(), node.label.clone()))
            }
            (GeneralLayoutInput::Gantt { tasks, .. }, GeneralLayoutElement::Task) => {
                let task = tasks.get(hit.row)?;
                Some((task.id.clone(), task.label.clone()))
            }
            (GeneralLayoutInput::Calendar(calendar), GeneralLayoutElement::Day) => {
                let date = calendar::item_date(&calendar.days, hit.row)?;
                Some((date.clone(), date))
            }
            (GeneralLayoutInput::Mosaic(cells), GeneralLayoutElement::Cell) => {
                let cell = cells.get(hit.row)?;
                Some((
                    format!("{}/{}", cell.column, cell.segment),
                    format!("{} · {}", cell.column, cell.segment),
                ))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Sets(sets), GeneralLayoutElement::Region) => {
                let region = sets.get(hit.row)?;
                Some((region.sets.join("∩"), venn::region_label(sets, hit.row)?))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Words(words), GeneralLayoutElement::Word) => {
                let word = words.get(hit.row)?;
                Some((word.text.clone(), word.text.clone()))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Geo(input), GeneralLayoutElement::Feature) => {
                let feature = input.features.get(hit.row)?;
                Some((feature.id.clone(), feature.label.clone()))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::GeoPoints(input), GeneralLayoutElement::Bubble) => {
                let base = store.map_base(series.pane_id).map(|(_, base)| base);
                Some((
                    input.points.get(hit.row)?.id.clone(),
                    geo::bubble_label(input, base, hit.row)?,
                ))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::GeoLinks(input), GeneralLayoutElement::Link) => {
                let base = store.map_base(series.pane_id).map(|(_, base)| base);
                Some((
                    geo::flow_id(input, hit.row)?,
                    self.flow_label(input, base, hit.row)?,
                ))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Points3d(input), GeneralLayoutElement::Point) => {
                Some((hit.row.to_string(), chart3d::point_label(input, hit.row)?))
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Surface(input), GeneralLayoutElement::Sample) => Some((
                chart3d::sample_id(input, hit.row)?,
                chart3d::sample_label(input, hit.row)?,
            )),
            _ => None,
        }
    }

    /// Tooltip parts of one layout item in `pane_index`.
    pub(crate) fn general_layout_item_tooltip(
        &self,
        pane_index: usize,
        hit: GeneralLayoutHit,
    ) -> Option<LayoutItemTooltip> {
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().find(|series| series.id == hit.series)?;
        if self.pane_index_for_id(series.pane_id) != Some(pane_index) {
            return None;
        }
        let entry = store.entry(series.options.dataset)?;
        let (label, color, detail) = match (&entry.input, hit.element) {
            (GeneralLayoutInput::Funnel(stages), GeneralLayoutElement::Stage) => {
                (stages.get(hit.row)?.label.clone(), series.color, None)
            }
            (GeneralLayoutInput::Hierarchy(nodes), GeneralLayoutElement::Node) => (
                nodes.get(hit.row)?.label.clone(),
                match series.options.kind {
                    GeneralLayoutSeriesKind::Treemap => {
                        treemap_cell_color(series.color, nodes, hit.row, treemap_color_range(nodes))
                    }
                    #[cfg(feature = "complete")]
                    GeneralLayoutSeriesKind::Icicle => {
                        icicle::cell_color(series.color, nodes, hit.row)
                    }
                    _ => series.color,
                },
                None,
            ),
            (GeneralLayoutInput::Flow { nodes, .. }, GeneralLayoutElement::Node) => {
                (nodes.get(hit.row)?.label.clone(), SANKEY_NODE_FILL, None)
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
                    None,
                )
            }
            (GeneralLayoutInput::Gantt { tasks, .. }, GeneralLayoutElement::Task) => {
                let task = tasks.get(hit.row)?;
                // The schedule replaces the progress number, which the detail already states.
                return Some(LayoutItemTooltip {
                    label: task.label.clone(),
                    title: series.options.title.clone(),
                    value: None,
                    color: series.color,
                    detail: Some(self.gantt_task_detail(task)),
                });
            }
            (GeneralLayoutInput::Calendar(calendar), GeneralLayoutElement::Day) => (
                calendar::item_date(&calendar.days, hit.row)?,
                self.calendar_day_color(series, calendar, &entry.semantic_values, hit.row),
                None,
            ),
            (GeneralLayoutInput::Mosaic(cells), GeneralLayoutElement::Cell) => {
                let cell = cells.get(hit.row)?;
                let layout = mosaic::MosaicLayout::new(cells, mosaic::UNIT_PLOT);
                (
                    format!("{} · {}", cell.column, cell.segment),
                    layout.color(series.color, hit.row),
                    Some(format!("({:.1}%)", layout.share(hit.row) * 100.0)),
                )
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Gauge(gauge), element) => {
                return self.gauge_item_tooltip(series, gauge, element, hit.row);
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Graph { nodes, edges }, element) => {
                let (label, color, detail) =
                    self.chord_item_tooltip(series, nodes, edges, element, hit.row)?;
                (label, color, detail)
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Sets(sets), GeneralLayoutElement::Region) => {
                self.venn_item_tooltip(series, sets, hit.row)?
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Words(words), GeneralLayoutElement::Word) => {
                let word = words.get(hit.row)?;
                let color = word
                    .color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(series.color);
                (word.text.clone(), color, None)
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Geo(input), GeneralLayoutElement::Feature) => {
                self.choropleth_item_tooltip(series, input, &entry.semantic_values, hit.row)?
            }
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::GeoPoints(input), GeneralLayoutElement::Bubble) => self
                .mark_item_tooltip(
                    series,
                    geo::MarkInput::Points(input),
                    store.map_base(series.pane_id).map(|(_, base)| base),
                    hit.row,
                )?,
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::GeoLinks(input), GeneralLayoutElement::Link) => self
                .mark_item_tooltip(
                    series,
                    geo::MarkInput::Links(input),
                    store.map_base(series.pane_id).map(|(_, base)| base),
                    hit.row,
                )?,
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Points3d(input), GeneralLayoutElement::Point) => (
                chart3d::point_label(input, hit.row)?,
                chart3d::item_color(series.color, &entry.input, hit.row)?,
                None,
            ),
            #[cfg(feature = "complete")]
            (GeneralLayoutInput::Surface(input), GeneralLayoutElement::Sample) => (
                chart3d::sample_label(input, hit.row)?,
                chart3d::item_color(series.color, &entry.input, hit.row)?,
                None,
            ),
            _ => return None,
        };
        Some(LayoutItemTooltip {
            label,
            title: series.options.title.clone(),
            value: self.general_layout_hit_value(hit),
            color,
            detail,
        })
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
                        #[cfg(feature = "complete")]
                        expanded: None,
                        #[cfg(feature = "complete")]
                        placed: None,
                    })
                    .collect(),
            ),
            GeneralLayoutInput::Hierarchy(nodes) => self
                .collapsible_accessibility_items(
                    series,
                    nodes,
                    &entry.semantic_values,
                    selected,
                    offset,
                    limit,
                )
                .unwrap_or_else(|| {
                    (
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
                                #[cfg(feature = "complete")]
                                expanded: None,
                                #[cfg(feature = "complete")]
                                placed: None,
                            })
                            .collect(),
                    )
                }),
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
                        #[cfg(feature = "complete")]
                        expanded: None,
                        #[cfg(feature = "complete")]
                        placed: None,
                    });
                }
                (nodes.len() + links.len(), items)
            }
            GeneralLayoutInput::Gantt { tasks, .. } => (
                tasks.len(),
                self.gantt_accessibility_items(series_id, tasks, selected, offset, limit),
            ),
            GeneralLayoutInput::Calendar(calendar) => (
                entry.semantic_values.len(),
                self.calendar_accessibility_items(
                    series_id,
                    &calendar.days,
                    &entry.semantic_values,
                    selected,
                    offset,
                    limit,
                ),
            ),
            GeneralLayoutInput::Mosaic(cells) => (
                cells.len(),
                self.mosaic_accessibility_items(series_id, cells, selected, offset, limit),
            ),
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Gauge(gauge) => (
                1 + gauge.bands.len(),
                self.gauge_accessibility_items(series_id, gauge, selected, offset, limit),
            ),
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Graph { nodes, edges } => (
                nodes.len() + edges.len(),
                self.graph_accessibility_items(
                    series_id,
                    (nodes, edges),
                    &entry.semantic_values,
                    selected,
                    offset,
                    limit,
                ),
            ),
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Sets(sets) => (
                sets.len(),
                sets.iter()
                    .enumerate()
                    .skip(offset)
                    .take(limit)
                    .map(|(row, region)| GeneralLayoutAccessibilityItem {
                        element: GeneralLayoutElement::Region,
                        row,
                        id: region.sets.join("∩"),
                        label: venn::region_label(sets, row).unwrap_or_default(),
                        value: Some(region.size),
                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element: GeneralLayoutElement::Region,
                                row,
                            }),
                        #[cfg(feature = "complete")]
                        expanded: None,
                        #[cfg(feature = "complete")]
                        placed: None,
                    })
                    .collect(),
            ),
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Words(words) => {
                let plot = self
                    .pane_index_for_id(series.pane_id)
                    .and_then(|pane| self.general_plot_rect(pane));
                (
                    words.len(),
                    words
                        .iter()
                        .enumerate()
                        .skip(offset)
                        .take(limit)
                        .map(|(row, word)| GeneralLayoutAccessibilityItem {
                            element: GeneralLayoutElement::Word,
                            row,
                            id: word.text.clone(),
                            label: word.text.clone(),
                            value: Some(word.weight),
                            selected: selected
                                == Some(GeneralLayoutHit {
                                    series: series_id,
                                    element: GeneralLayoutElement::Word,
                                    row,
                                }),
                            expanded: None,
                            placed: plot.map(|plot| {
                                series.with_word_cloud(self, words, plot, |cache| {
                                    cache.is_placed(row)
                                })
                            }),
                        })
                        .collect(),
                )
            }
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Geo(input) => (
                input.features.len(),
                input
                    .features
                    .iter()
                    .enumerate()
                    .skip(offset)
                    .take(limit)
                    .map(|(row, feature)| GeneralLayoutAccessibilityItem {
                        element: GeneralLayoutElement::Feature,
                        row,
                        id: feature.id.clone(),
                        label: feature.label.clone(),
                        value: entry
                            .semantic_values
                            .get(row)
                            .copied()
                            .filter(|value| !value.is_nan()),
                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element: GeneralLayoutElement::Feature,
                                row,
                            }),
                        expanded: None,
                        placed: None,
                    })
                    .collect(),
            ),
            #[cfg(feature = "complete")]
            GeneralLayoutInput::GeoPoints(points) => {
                let base = store.map_base(series.pane_id).map(|(_, base)| base);
                (
                    points.points.len(),
                    (offset..points.points.len().min(offset.saturating_add(limit)))
                        .map(|row| GeneralLayoutAccessibilityItem {
                            element: GeneralLayoutElement::Bubble,
                            row,
                            id: points.points[row].id.clone(),
                            label: geo::bubble_label(points, base, row).unwrap_or_default(),
                            value: points.points[row].value,
                            selected: selected
                                == Some(GeneralLayoutHit {
                                    series: series_id,
                                    element: GeneralLayoutElement::Bubble,
                                    row,
                                }),
                            expanded: None,
                            placed: None,
                        })
                        .collect(),
                )
            }
            #[cfg(feature = "complete")]
            GeneralLayoutInput::GeoLinks(links) => {
                let base = store.map_base(series.pane_id).map(|(_, base)| base);
                (
                    links.links.len(),
                    (offset..links.links.len().min(offset.saturating_add(limit)))
                        .map(|row| GeneralLayoutAccessibilityItem {
                            element: GeneralLayoutElement::Link,
                            row,
                            id: geo::flow_id(links, row).unwrap_or_default(),
                            label: self.flow_label(links, base, row).unwrap_or_default(),
                            value: links.links[row].value,
                            selected: selected
                                == Some(GeneralLayoutHit {
                                    series: series_id,
                                    element: GeneralLayoutElement::Link,
                                    row,
                                }),
                            expanded: None,
                            placed: None,
                        })
                        .collect(),
                )
            }
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Points3d(input) => (
                input.points.len(),
                (offset..input.points.len().min(offset.saturating_add(limit)))
                    .map(|row| GeneralLayoutAccessibilityItem {
                        element: GeneralLayoutElement::Point,
                        row,
                        id: row.to_string(),
                        label: chart3d::point_label(input, row).unwrap_or_default(),
                        value: Some(input.points[row].z),
                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element: GeneralLayoutElement::Point,
                                row,
                            }),
                        expanded: None,
                        placed: None,
                    })
                    .collect(),
            ),
            // Holes stay addressable with no value so sample rows keep their grid positions.
            #[cfg(feature = "complete")]
            GeneralLayoutInput::Surface(input) => (
                input.z.len(),
                (offset..input.z.len().min(offset.saturating_add(limit)))
                    .map(|row| GeneralLayoutAccessibilityItem {
                        element: GeneralLayoutElement::Sample,
                        row,
                        id: chart3d::sample_id(input, row).unwrap_or_default(),
                        label: chart3d::sample_label(input, row).unwrap_or_default(),
                        value: input.z[row],
                        selected: selected
                            == Some(GeneralLayoutHit {
                                series: series_id,
                                element: GeneralLayoutElement::Sample,
                                row,
                            }),
                        expanded: None,
                        placed: None,
                    })
                    .collect(),
            ),
            #[cfg(not(feature = "complete"))]
            GeneralLayoutInput::Gauge(_)
            | GeneralLayoutInput::Graph { .. }
            | GeneralLayoutInput::Sets(_)
            | GeneralLayoutInput::Words(_)
            | GeneralLayoutInput::Geo(_)
            | GeneralLayoutInput::GeoPoints(_)
            | GeneralLayoutInput::GeoLinks(_)
            | GeneralLayoutInput::Points3d(_)
            | GeneralLayoutInput::Surface(_) => (0, Vec::new()),
        };
        Some(GeneralLayoutAccessibilitySnapshot {
            series: series_id,
            title: series.options.title.clone(),
            total,
            offset: offset.min(total),
            items,
            grid_stride: matches!(input, GeneralLayoutInput::Calendar(_)).then_some(7),
        })
    }
    pub fn add_general_layout_series(
        &mut self,
        options: GeneralLayoutSeriesOptions,
    ) -> Result<GeneralLayoutSeriesId, ChartError> {
        if options.kind.requires_complete_build() && !Self::build_includes_complete_families() {
            return Err(crate::complete_build_required(
                "gauge, relationship, hierarchy, map, and 3D layout families",
            ));
        }
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
        #[cfg(feature = "complete")]
        self.settle_network_simulations();
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
    /// Changes the direction of a tree, dendrogram, or icicle series. Fails without a change
    /// when the series kind does not support `orientation`.
    #[cfg(feature = "complete")]
    pub fn set_general_layout_hierarchy_orientation(
        &mut self,
        id: GeneralLayoutSeriesId,
        orientation: HierarchyOrientation,
    ) -> Result<(), ChartError> {
        let series = self
            .general_layout_data
            .as_mut()
            .and_then(|store| store.series.iter_mut().find(|series| series.id == id))
            .ok_or_else(|| invalid_handle("layout series handle is stale"))?;
        if !series.options.kind.supports_orientation(orientation) {
            return Err(ChartError::new(
                ErrorCode::InvalidOptions,
                "layout series kind does not support this hierarchy orientation",
            ));
        }
        if series.options.hierarchy_orientation != orientation {
            series.options.hierarchy_orientation = orientation;
            series.clear_caches();
            self.invalidate_frame_all();
        }
        Ok(())
    }

    #[cfg(not(feature = "complete"))]
    fn collapsible_accessibility_items(
        &self,
        _series: &LayoutSeriesEntry,
        _nodes: &[HierarchyNode],
        _values: &[f64],
        _selected: Option<GeneralLayoutHit>,
        _offset: usize,
        _limit: usize,
    ) -> Option<(usize, Vec<GeneralLayoutAccessibilityItem>)> {
        None
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
        #[cfg(feature = "complete")]
        self.settle_network_simulations();
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

/// Whether a hierarchy row of `series` sits under a collapsed tree or dendrogram node.
fn hierarchy_row_hidden(series: &LayoutSeriesEntry, nodes: &[HierarchyNode], row: usize) -> bool {
    #[cfg(feature = "complete")]
    {
        tree::row_hidden(series, nodes, row)
    }
    #[cfg(not(feature = "complete"))]
    {
        let _ = (series, nodes, row);
        false
    }
}

/// Hierarchy, network and word cloud options apply only to the families they configure.
#[cfg(feature = "complete")]
fn validate_relationship_options(
    options: &GeneralLayoutSeriesOptions,
    input: &GeneralLayoutInput,
) -> Result<(), ChartError> {
    if !options
        .kind
        .supports_orientation(options.hierarchy_orientation)
    {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "layout series kind does not support this hierarchy orientation",
        ));
    }
    validate_collapsed(options, input)?;
    if options.kind != GeneralLayoutSeriesKind::Network && !options.has_default_network_options() {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "network options apply only to network series",
        ));
    }
    if options.kind != GeneralLayoutSeriesKind::WordCloud
        && !options.has_default_word_cloud_options()
    {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "word cloud options apply only to word cloud series",
        ));
    }
    if !options.kind.is_map() && !options.map_view.is_default() {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "map views apply only to choropleth, bubble map and flow map series",
        ));
    }
    if !options.kind.is_3d() && !options.camera_3d.is_default() {
        return Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "3D cameras apply only to 3D bar, scatter, line and surface series",
        ));
    }
    chart3d::validate_camera(options.camera_3d)?;
    geo::validate_view(options.map_view)
}

/// Collapsed IDs are bounded by the dataset: each must be unique and name a node with children
/// of a tree or dendrogram series.
#[cfg(feature = "complete")]
fn validate_collapsed(
    options: &GeneralLayoutSeriesOptions,
    input: &GeneralLayoutInput,
) -> Result<(), ChartError> {
    if options.collapsed.is_empty() {
        return Ok(());
    }
    let invalid = || {
        ChartError::new(
            ErrorCode::InvalidOptions,
            "collapsed IDs must be unique internal nodes of a tree or dendrogram series",
        )
    };
    let GeneralLayoutInput::Hierarchy(nodes) = input else {
        return Err(invalid());
    };
    if !options.kind.is_collapsible() || options.collapsed.len() > nodes.len() {
        return Err(invalid());
    }
    let parents: HashSet<&str> = nodes
        .iter()
        .filter_map(|node| node.parent.as_deref())
        .collect();
    let mut seen = HashSet::with_capacity(options.collapsed.len());
    if options
        .collapsed
        .iter()
        .all(|id| parents.contains(id.as_str()) && seen.insert(id.as_str()))
    {
        Ok(())
    } else {
        Err(invalid())
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

    fn gantt_tasks(count: usize) -> Vec<GanttTask> {
        const DAY: f64 = 86_400_000.0;
        (0..count)
            .map(|index| GanttTask {
                id: format!("t{index}"),
                label: format!("Task {index}"),
                start: index as f64 * DAY,
                end: if index == 2 {
                    2.0 * DAY
                } else {
                    (index as f64 + 2.0) * DAY
                },
                progress: if index == 0 { 0.5 } else { 0.0 },
                group: (index < 4).then(|| "Phase 1".to_owned()),
                dependencies: if index == 1 {
                    vec!["t0".into()]
                } else {
                    Vec::new()
                },
            })
            .collect()
    }

    fn laid_out(chart: &mut ChartEngine) -> crate::ChartFrame {
        chart.recompute_layout_with_measure(true, |text, _| text.len() as f64 * 6.0, |_, _| 0.0);
        chart.build_frame()
    }

    fn painted_text(frame: &crate::ChartFrame, text: &str) -> bool {
        frame.panes[0]
            .main
            .iter()
            .any(|prim| matches!(prim, Prim::Text { text: painted, .. } if painted == text))
    }

    #[test]
    fn gantt_paints_progress_milestones_dependencies_and_a_host_supplied_now_line() {
        let mut chart = chart();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Gantt {
                tasks: gantt_tasks(5),
                now: Some(86_400_000.0 * 1.5),
            })
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Gantt,
            ))
            .unwrap();
        let frame = laid_out(&mut chart);
        let main = &frame.panes[0].main;
        assert!(
            main.iter().any(|prim| matches!(
                prim,
                Prim::VLine { style: LineStyle::Dashed, color, .. } if *color == gantt::NOW_COLOR
            )),
            "now line absent"
        );
        // One milestone diamond (two triangles) plus one dependency arrowhead.
        assert_eq!(
            main.iter()
                .filter(|prim| matches!(prim, Prim::Triangle { .. }))
                .count(),
            3
        );
        assert_eq!(
            main.iter()
                .filter(|prim| matches!(prim, Prim::Polyline { .. }))
                .count(),
            1
        );
        assert!(painted_text(&frame, "Phase 1"));
        let task = |row| GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Task,
            row,
        };
        let plot = chart.general_plot_rect(0).unwrap();
        let (x, y) = chart.general_layout_hit_anchor(0, task(0)).unwrap();
        // The 50% progress fill paints in the series color over half of the first bar's track.
        let color = chart.general_layout_data.as_ref().unwrap().series[0].color;
        let rects: Vec<_> = main
            .iter()
            .filter_map(|prim| match prim {
                Prim::Rect { rect, color } if rect.h == 18 => Some((*rect, *color)),
                _ => None,
            })
            .collect();
        let (track, _) = rects[0];
        let (fill, fill_color) = rects[1];
        assert_eq!(fill_color, color);
        assert_eq!((fill.x, fill.w), (track.x, track.w / 2));
        assert_eq!(chart.general_layout_hit_test(0, x, y), Some(task(0)));
        let milestone = chart.general_layout_hit_anchor(0, task(2)).unwrap();
        assert_eq!(
            chart.general_layout_hit_test(0, milestone.0, milestone.1),
            Some(task(2))
        );
        assert_eq!(chart.general_layout_hit_test(0, plot.x + 2.0, y), None);
        let tooltip = chart.general_layout_item_tooltip(0, task(0)).unwrap();
        assert_eq!(tooltip.label, "Task 0");
        assert_eq!(tooltip.value, None);
        assert_eq!(
            tooltip.detail.as_deref(),
            Some("1 Jan 1970 – 3 Jan 1970 · 50% complete")
        );
        assert_eq!(chart.general_layout_hit_value(task(0)), Some(50.0));
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 10)
            .unwrap();
        assert_eq!(snapshot.total, 5);
        assert_eq!(snapshot.grid_stride, None);
        assert_eq!(snapshot.items[2].label, "Task 2, Milestone 3 Jan 1970");
        assert_eq!(
            chart.general_layout_item_identity(task(0)),
            Some(("t0".into(), "Task 0".into()))
        );
    }

    #[test]
    fn gantt_rows_are_bounded_and_scroll_by_wheel_and_keyboard_focus() {
        let mut chart = chart();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Gantt {
                tasks: gantt_tasks(40),
                now: None,
            })
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Gantt,
            ))
            .unwrap();
        let frame = laid_out(&mut chart);
        let task = |row| GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Task,
            row,
        };
        let plot = chart.general_plot_rect(0).unwrap();
        let visible_rows = ((plot.height - 24.0) / 28.0).floor() as usize;
        // Only visible rows are painted: the group header plus one bar per visible task row.
        let bars = frame.panes[0]
            .main
            .iter()
            .filter(|prim| matches!(prim, Prim::Text { text, .. } if text.starts_with("Task")))
            .count();
        assert_eq!(bars, visible_rows - 1);
        assert!(chart.general_layout_hit_anchor(0, task(0)).is_some());
        assert!(chart.general_layout_hit_anchor(0, task(39)).is_none());
        let wheel = |delta_y| crate::WheelSample {
            x: plot.x + plot.width * 0.5,
            y: plot.y + plot.height * 0.5,
            delta_y,
            ..Default::default()
        };
        chart.input_wheel(wheel(1.0));
        assert!(
            chart.general_layout_hit_anchor(0, task(0)).is_some(),
            "already at the first row"
        );
        assert!(chart.input_wheel(wheel(-1.0)));
        assert!(chart.general_layout_hit_anchor(0, task(0)).is_none());
        assert!(chart.set_general_layout_accessibility_focus(task(39), "#2962ff", 14.0));
        assert!(chart.general_layout_hit_anchor(0, task(39)).is_some());
        chart.input_wheel(wheel(-1.0));
        assert!(
            chart.general_layout_hit_anchor(0, task(39)).is_some(),
            "already at the last row"
        );
        assert!(chart.set_general_layout_accessibility_focus(task(0), "#2962ff", 14.0));
        let (_, y) = chart.general_layout_hit_anchor(0, task(0)).unwrap();
        // Revealing the first grouped task keeps its group header above it.
        assert!(y > plot.y + 24.0 + 28.0);
        assert!(chart.input_wheel(wheel(-0.28)));
        let (_, scrolled) = chart.general_layout_hit_anchor(0, task(0)).unwrap();
        assert!((y - scrolled - 28.0).abs() < 1e-9);
    }

    #[test]
    fn calendar_heatmap_is_utc_week_start_and_locale_aware_with_day_and_week_items() {
        const DAY: f64 = 86_400_000.0;
        let utc = |year, month, day| {
            aeris_charts_core::scale::time_tick_marks::days_from_civil(year, month, day).unwrap()
                as f64
                * DAY
        };
        let mut chart =
            ChartEngine::new_with_initial_domain(900.0, 300.0, 1.0, HorizontalDomain::Layout)
                .unwrap();
        // The display time zone does not move calendar days.
        chart.set_time_zone("America/New_York").unwrap();
        let english = aeris_charts_core::format::time_formatter::MonthNames::english();
        let mut long = english.long.clone();
        long[1] = "février".into();
        chart.set_month_names(english.short, long);
        let input = CalendarInput {
            days: vec![
                CalendarDay {
                    date: utc(2024, 2, 29) + DAY - 1.0,
                    value: 10.0,
                },
                CalendarDay {
                    date: utc(2024, 3, 7),
                    value: 0.0,
                },
            ],
            week_start: 1,
            day_labels: ["D", "L", "M", "M", "J", "V", "S"]
                .map(str::to_owned)
                .to_vec(),
            low_color: Some("#ffffff".into()),
            high_color: Some("#003366".into()),
        };
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Calendar(input))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::CalendarHeatmap,
            ))
            .unwrap();
        let frame = laid_out(&mut chart);
        let leap_row = 31 + 28;
        let day = |row| GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Day,
            row,
        };
        let (x, y) = chart.general_layout_hit_anchor(0, day(leap_row)).unwrap();
        assert_eq!(chart.general_layout_hit_test(0, x, y), Some(day(leap_row)));
        // One week later is the next column on the same weekday row.
        let (next_x, next_y) = chart
            .general_layout_hit_anchor(0, day(leap_row + 7))
            .unwrap();
        assert!(next_x > x && (next_y - y).abs() < 1e-9);
        // 2024-03-01 is the next row of the same Monday-first column, and has no value.
        let (below_x, below_y) = chart
            .general_layout_hit_anchor(0, day(leap_row + 1))
            .unwrap();
        assert!((below_x - x).abs() < 1e-9 && below_y > y);
        assert_eq!(chart.general_layout_hit_test(0, below_x, below_y), None);
        let tooltip = chart.general_layout_item_tooltip(0, day(leap_row)).unwrap();
        assert_eq!(tooltip.label, "2024-02-29");
        assert_eq!(tooltip.value, Some(10.0));
        assert_eq!(tooltip.color, Color::parse_css("#003366").unwrap());
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, leap_row, 2)
            .unwrap();
        assert_eq!(snapshot.total, 366);
        assert_eq!(snapshot.grid_stride, Some(7));
        assert_eq!(snapshot.items[0].label, "29 février 2024");
        assert_eq!(snapshot.items[0].id, "2024-02-29");
        assert_eq!(snapshot.items[1].value, None);
        // Keyboard focus may rest on an empty day so week steps cross gaps.
        assert!(chart.set_general_layout_accessibility_focus(day(leap_row + 1), "#2962ff", 14.0));
        assert!(!chart.set_general_layout_accessibility_focus(day(366), "#2962ff", 14.0));
        for text in ["L", "Feb", "2024", "0", "10"] {
            assert!(painted_text(&frame, text), "{text} label absent");
        }
    }

    #[test]
    fn marimekko_cells_have_share_tooltips_hits_and_a_segment_legend() {
        let mut chart = chart();
        let cell = |column: &str, segment: &str, value| MosaicCell {
            column: column.into(),
            segment: segment.into(),
            value,
        };
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Mosaic(vec![
                cell("North", "Retail", 30.0),
                cell("North", "Online", 10.0),
                cell("South", "Retail", 20.0),
                cell("Empty", "Retail", 0.0),
            ]))
            .unwrap();
        let series = chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Marimekko,
            ))
            .unwrap();
        let frame = laid_out(&mut chart);
        let hit = |row| GeneralLayoutHit {
            series,
            element: GeneralLayoutElement::Cell,
            row,
        };
        let (x, y) = chart.general_layout_hit_anchor(0, hit(1)).unwrap();
        assert_eq!(chart.general_layout_hit_test(0, x, y), Some(hit(1)));
        assert_eq!(chart.general_layout_hit_anchor(0, hit(3)), None);
        let tooltip = chart.general_layout_item_tooltip(0, hit(1)).unwrap();
        assert_eq!(tooltip.label, "North · Online");
        assert_eq!(tooltip.value, Some(10.0));
        assert_eq!(tooltip.detail.as_deref(), Some("(25.0%)"));
        let snapshot = chart
            .general_layout_accessibility_snapshot(series, 0, 8)
            .unwrap();
        assert_eq!(snapshot.total, 4);
        for text in ["Retail", "Online", "North", "South"] {
            assert!(painted_text(&frame, text), "{text} label absent");
        }
    }

    #[test]
    fn timeline_calendar_and_mosaic_inputs_are_validated_atomically() {
        let mut chart = chart();
        let dataset = chart
            .create_general_layout_dataset(GeneralLayoutInput::Gantt {
                tasks: gantt_tasks(3),
                now: None,
            })
            .unwrap();
        for kind in [
            GeneralLayoutSeriesKind::CalendarHeatmap,
            GeneralLayoutSeriesKind::Marimekko,
            GeneralLayoutSeriesKind::Funnel,
        ] {
            assert_eq!(
                chart
                    .add_general_layout_series(GeneralLayoutSeriesOptions::new(0, dataset, kind))
                    .unwrap_err()
                    .code(),
                ErrorCode::InvalidData
            );
        }
        chart
            .add_general_layout_series(GeneralLayoutSeriesOptions::new(
                0,
                dataset,
                GeneralLayoutSeriesKind::Gantt,
            ))
            .unwrap();
        let before = chart.general_layout_dataset(dataset).cloned();
        let mut cyclic = gantt_tasks(3);
        cyclic[0].dependencies = vec!["t1".into()];
        for tasks_and_now in [(cyclic, None), (gantt_tasks(3), Some(f64::NAN))] {
            let (tasks, now) = tasks_and_now;
            assert!(
                chart
                    .replace_general_layout_dataset(
                        dataset,
                        GeneralLayoutInput::Gantt { tasks, now }
                    )
                    .is_err()
            );
            assert_eq!(chart.general_layout_dataset(dataset).cloned(), before);
        }
        assert!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Mosaic(vec![MosaicCell {
                    column: "A".into(),
                    segment: "x".into(),
                    value: -1.0,
                }]))
                .is_err()
        );
        assert!(
            chart
                .create_general_layout_dataset(GeneralLayoutInput::Calendar(CalendarInput {
                    days: Vec::new(),
                    week_start: 9,
                    day_labels: Vec::new(),
                    low_color: None,
                    high_color: None,
                }))
                .is_err()
        );
    }
}
