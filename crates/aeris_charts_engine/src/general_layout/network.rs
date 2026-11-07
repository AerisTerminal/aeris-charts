//! Network graphs of a graph dataset (complete build). Three layouts share one data space:
//!
//! - `force` follows d3-force 3.0: `forceLink` (strength `1 / min(degree)`, configurable
//!   distance, bias by degree), `forceManyBody` (strength −30, Barnes–Hut with θ = 0.9,
//!   minimum distance 1) and `forceCenter` at (0, 0), velocity decay 0.4 and the alpha
//!   schedule 1 → 0.001 with decay `1 − 0.001^(1/300)`, so a full run settles in 300 ticks.
//!   Nodes start on d3's phyllotaxis spiral; coincident nodes separate through d3's seeded
//!   linear congruential jiggle, so equal input and an equal tick sequence give identical
//!   positions. Data units are CSS pixels at zoom 1 with (0, 0) at the plot center, as in
//!   d3 and ECharts.
//! - `circular` puts nodes evenly on a unit circle in input order from 12 o'clock clockwise
//!   (ECharts `layout: 'circular'`), fitted to 80% of half the shorter plot side.
//! - `fixed` uses each node's input `x`/`y` (Y down), fitted to 80% of the plot.
//!
//! A dragged node is pinned where it is released (ECharts keeps dragged positions). The view
//! pans in data units and zooms around a screen point. Frame, hits and anchors read the same
//! simulated positions through one transform.

use std::collections::{HashMap, HashSet};
use std::f64::consts::{PI, TAU};

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::{
    GeneralLayoutElement, GeneralLayoutInput, GeneralLayoutSeriesKind, GeneralLayoutSeriesOptions,
    GraphEdge, GraphNode, LayoutSeriesEntry, NetworkLayout, NetworkPin, NetworkView, graph,
    invalid_data, mosaic,
};
use crate::general_axes::GeneralPlotRect;
use crate::{ChartEngine, ChartError, ErrorCode, GeneralLayoutSeriesId};

/// One simulation tick per 60 Hz display frame, as d3's timer runs.
const TICK_MS: f64 = 1_000.0 / 60.0;
/// Ticks one clock sample may run. A host that falls further behind skips the backlog
/// instead of catching up, so per-frame work stays bounded.
pub(super) const MAX_TICKS_PER_ADVANCE: u64 = 2;
/// A full reheat settles in this many ticks; synchronous settling never runs more.
pub(super) const SETTLE_TICKS: u32 = 300;
const ALPHA_MIN: f64 = 0.001;
/// d3 drag examples hold `alphaTarget(0.3)` while a node is dragged.
const ALPHA_DRAG_TARGET: f64 = 0.3;
const VELOCITY_DECAY: f64 = 0.4;
const CHARGE: f64 = -30.0;
/// Barnes–Hut θ² of d3 `forceManyBody`.
const THETA2: f64 = 0.81;
const INITIAL_RADIUS: f64 = 10.0;
/// Quadtree depth below which coincident or near-coincident points share one leaf.
const MAX_QUAD_DEPTH: u32 = 24;
pub(crate) const MIN_ZOOM: f64 = 0.1;
pub(crate) const MAX_ZOOM: f64 = 10.0;
/// Largest absolute pinned coordinate or pan offset, in layout units.
pub(crate) const MAX_COORDINATE: f64 = 1e9;
const FIT: f64 = 0.8;
const NODE_MIN_RADIUS: f64 = 4.0;
const NODE_MAX_RADIUS: f64 = 14.0;
const NODE_DEFAULT_RADIUS: f64 = 5.0;
const NODE_STROKE: f64 = 1.0;
/// ECharts `graph` default `lineStyle`: `#aaa` at opacity 0.5, width 1.
const EDGE_COLOR: Color = Color(0xAAAA_AA80);
const EDGE_WIDTH: f64 = 1.0;
/// A pointer within this many CSS pixels of an edge or a node's disc hits it.
const HIT_SLOP: f64 = 4.0;
const LABEL_SIZE: f64 = 12.0;
const LABEL_GAP: f64 = 4.0;
/// Labels placed per frame; later labels that would overlap a placed one are skipped.
const MAX_LABELS: usize = 256;
const NONE: u32 = u32::MAX;

type Point = [f64; 2];

#[derive(Clone, Copy, Debug)]
struct Quad {
    child: [u32; 4],
    /// First point of a leaf's list.
    head: u32,
    internal: bool,
    /// Side length of the quad's square.
    width: f64,
    x: f64,
    y: f64,
    /// Total charge strength in the quad.
    value: f64,
}

impl Quad {
    fn leaf(width: f64, head: u32) -> Self {
        Self {
            child: [NONE; 4],
            head,
            internal: false,
            width,
            x: 0.0,
            y: 0.0,
            value: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ForceLink {
    source: u32,
    target: u32,
    strength: f64,
    bias: f64,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    row: usize,
    before_pin: Option<Point>,
    before_position: Point,
    /// Node position minus the grabbed data point, so the node does not jump to the pointer.
    offset: Point,
}

/// Phase of a node drag driven by the input controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NetworkDragPhase {
    Begin(f64, f64),
    Move(f64, f64),
    End,
    Cancel,
}

/// Screen mapping of network layout units: `screen = anchor + scale · (p − center)`, with `anchor` the plot center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NetworkTransform {
    anchor: Point,
    center: Point,
    scale: f64,
}

impl NetworkTransform {
    fn screen(&self, p: Point) -> Point {
        [
            self.anchor[0] + self.scale * (p[0] - self.center[0]),
            self.anchor[1] + self.scale * (p[1] - self.center[1]),
        ]
    }

    fn data(&self, q: Point) -> Point {
        [
            self.center[0] + (q[0] - self.anchor[0]) / self.scale,
            self.center[1] + (q[1] - self.anchor[1]) / self.scale,
        ]
    }
}

/// Positions, velocities and alpha of one network series. Persistence keeps the layout
/// options, pins, view and the settled node positions; a restore with positions for every
/// node starts settled instead of re-simulating.
pub(super) struct NetworkSim {
    layout: NetworkLayout,
    positions: Vec<Point>,
    velocities: Vec<Point>,
    pinned: Vec<Option<Point>>,
    /// Endpoint rows of every edge in input order.
    ends: Vec<(u32, u32)>,
    links: Vec<ForceLink>,
    distance: f64,
    alpha: f64,
    alpha_target: f64,
    ticks: u64,
    /// d3's `lcg()` state, seeded with 1 for every simulation.
    random: u64,
    /// Per host clock: the anchor time and the tick count at that time.
    clocks: [Option<(f64, u64)>; 2],
    drag: Option<Drag>,
    /// Input-coordinate bounds of a fixed layout: `(min_x, min_y, max_x, max_y)`.
    bounds: Option<[f64; 4]>,
    radii: Vec<f64>,
    slots: Vec<usize>,
    quads: Vec<Quad>,
    next: Vec<u32>,
    stack: Vec<u32>,
}

/// Retained bytes of a network simulation over `nodes` and `edges`, including a quadtree at
/// its bounded size.
pub(super) fn cache_reservation(nodes: usize, edges: usize) -> usize {
    nodes
        * (3 * std::mem::size_of::<Point>()
            + std::mem::size_of::<Option<Point>>()
            + std::mem::size_of::<f64>()
            + std::mem::size_of::<usize>()
            + 2 * std::mem::size_of::<u32>()
            + quad_capacity(nodes) / nodes.max(1) * std::mem::size_of::<Quad>())
        + edges * (std::mem::size_of::<(u32, u32)>() + std::mem::size_of::<ForceLink>())
}

/// The quadtree stops subdividing at this many quads; further points join an existing leaf.
fn quad_capacity(nodes: usize) -> usize {
    4 * nodes + 64
}

fn phyllotaxis(index: usize) -> Point {
    let radius = INITIAL_RADIUS * (0.5 + index as f64).sqrt();
    let angle = index as f64 * PI * (3.0 - 5f64.sqrt());
    [radius * angle.cos(), radius * angle.sin()]
}

fn circle_position(index: usize, count: usize) -> Point {
    let angle = -0.5 * PI + TAU * index as f64 / count.max(1) as f64;
    [angle.cos(), angle.sin()]
}

fn alpha_decay() -> f64 {
    1.0 - ALPHA_MIN.powf(1.0 / f64::from(SETTLE_TICKS))
}

/// Whether a fixed layout can place every node.
fn has_coordinates(nodes: &[GraphNode]) -> bool {
    nodes
        .iter()
        .all(|node| node.x.is_some() && node.y.is_some())
}

/// Options of a network series against `input`: a fixed layout needs coordinates on every
/// node and pins name unique existing nodes.
pub(super) fn validate_options(
    options: &GeneralLayoutSeriesOptions,
    input: &GeneralLayoutInput,
) -> Result<(), ChartError> {
    let invalid = |message: &'static str| ChartError::new(ErrorCode::InvalidOptions, message);
    let GeneralLayoutInput::Graph { nodes, .. } = input else {
        return Err(invalid("network series need a graph dataset"));
    };
    if !(1.0..=1_000.0).contains(&options.network_link_distance) {
        return Err(invalid("network link distance must be between 1 and 1000"));
    }
    validate_view(options.network_view)?;
    if options.network_layout == NetworkLayout::Fixed && !has_coordinates(nodes) {
        return Err(invalid_data(
            "a fixed network layout needs x and y on every node",
        ));
    }
    if options.pinned.len() > nodes.len() {
        return Err(invalid("pinned network nodes must be unique dataset nodes"));
    }
    let ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut seen = HashSet::with_capacity(options.pinned.len());
    for pin in &options.pinned {
        if !ids.contains(pin.id.as_str())
            || !seen.insert(pin.id.as_str())
            || !coordinate_ok(pin.x)
            || !coordinate_ok(pin.y)
        {
            return Err(invalid("pinned network nodes must be unique dataset nodes"));
        }
    }
    if options.positions.len() > nodes.len() {
        return Err(invalid(
            "persisted network positions must be unique dataset nodes",
        ));
    }
    let mut seen = HashSet::with_capacity(options.positions.len());
    for pin in &options.positions {
        if !ids.contains(pin.id.as_str())
            || !seen.insert(pin.id.as_str())
            || !coordinate_ok(pin.x)
            || !coordinate_ok(pin.y)
        {
            return Err(invalid(
                "persisted network positions must be unique dataset nodes",
            ));
        }
    }
    Ok(())
}

fn coordinate_ok(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_COORDINATE
}

pub(super) fn validate_view(view: NetworkView) -> Result<(), ChartError> {
    if (MIN_ZOOM..=MAX_ZOOM).contains(&view.zoom) && coordinate_ok(view.x) && coordinate_ok(view.y)
    {
        Ok(())
    } else {
        Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "network view needs a zoom between 0.1 and 10 and finite pan offsets",
        ))
    }
}

/// Whether `input` can replace the data of a network series with `options`: a fixed layout
/// still needs coordinates on every node.
pub(super) fn accepts_replacement(
    options: &GeneralLayoutSeriesOptions,
    input: &GeneralLayoutInput,
) -> bool {
    options.kind != GeneralLayoutSeriesKind::Network
        || options.network_layout != NetworkLayout::Fixed
        || matches!(input, GeneralLayoutInput::Graph { nodes, .. } if has_coordinates(nodes))
}

impl NetworkSim {
    /// A simulation at alpha 1. Nodes that `previous` also had keep their positions; new
    /// nodes start on the phyllotaxis spiral (force), the circle (circular) or their input
    /// coordinates (fixed). Pins override the start position.
    pub(super) fn new(
        options: &GeneralLayoutSeriesOptions,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
        values: &[f64],
        previous: Option<(&[GraphNode], &NetworkSim)>,
    ) -> Self {
        let layout = options.network_layout;
        let count = nodes.len();
        let index = graph::node_index(nodes);
        let kept: HashMap<&str, Point> = previous
            .filter(|(_, sim)| sim.layout == layout)
            .map(|(old, sim)| {
                old.iter()
                    .zip(&sim.positions)
                    .map(|(node, &position)| (node.id.as_str(), position))
                    .collect()
            })
            .unwrap_or_default();
        let pins: HashMap<&str, Point> = options
            .pinned
            .iter()
            .map(|pin| (pin.id.as_str(), [pin.x, pin.y]))
            .collect();
        let pinned: Vec<Option<Point>> = nodes
            .iter()
            .map(|node| pins.get(node.id.as_str()).copied())
            .collect();
        // Settled positions persisted by an export; a restore seeds from them directly.
        let persisted: HashMap<&str, Point> = options
            .positions
            .iter()
            .map(|pin| (pin.id.as_str(), [pin.x, pin.y]))
            .collect();
        let kept_any = nodes.iter().any(|node| kept.contains_key(node.id.as_str()));
        // A node restores its own position when a pin or a persisted settled position names
        // it; a surviving live position only reheats, as before.
        let anchored_all = !nodes.is_empty()
            && nodes.iter().all(|node| {
                pins.contains_key(node.id.as_str()) || persisted.contains_key(node.id.as_str())
            });
        let seeded_any = kept_any
            || nodes
                .iter()
                .any(|node| persisted.contains_key(node.id.as_str()));
        let positions = nodes
            .iter()
            .enumerate()
            .map(|(row, node)| {
                if let Some(pin) = pinned[row] {
                    return pin;
                }
                match layout {
                    NetworkLayout::Force => kept
                        .get(node.id.as_str())
                        .copied()
                        .or_else(|| persisted.get(node.id.as_str()).copied())
                        .unwrap_or_else(|| phyllotaxis(row)),
                    NetworkLayout::Circular => circle_position(row, count),
                    NetworkLayout::Fixed => [node.x.unwrap_or(0.0), node.y.unwrap_or(0.0)],
                }
            })
            .collect();
        let ends: Vec<(u32, u32)> = edges
            .iter()
            .map(|edge| {
                (
                    index.get(edge.source.as_str()).copied().unwrap_or(0) as u32,
                    index.get(edge.target.as_str()).copied().unwrap_or(0) as u32,
                )
            })
            .collect();
        // Self-links exert no force; they neither count toward degree nor become links.
        let mut degree = vec![0u32; count];
        for &(source, target) in &ends {
            if source != target {
                degree[source as usize] += 1;
                degree[target as usize] += 1;
            }
        }
        let links = if layout == NetworkLayout::Force {
            ends.iter()
                .filter(|(source, target)| source != target)
                .map(|&(source, target)| {
                    let (a, b) = (
                        f64::from(degree[source as usize]),
                        f64::from(degree[target as usize]),
                    );
                    ForceLink {
                        source,
                        target,
                        strength: 1.0 / a.min(b),
                        bias: a / (a + b),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let bounds = (layout == NetworkLayout::Fixed && count > 0).then(|| {
            nodes.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |[x0, y0, x1, y1], node| {
                    let (x, y) = (node.x.unwrap_or(0.0), node.y.unwrap_or(0.0));
                    [x0.min(x), y0.min(y), x1.max(x), y1.max(y)]
                },
            )
        });
        let largest = values
            .iter()
            .take(count)
            .copied()
            .filter(|value| value.is_finite())
            .fold(0.0, f64::max);
        let radii = (0..count)
            .map(|row| {
                let value = values.get(row).copied().unwrap_or(0.0);
                if largest > 0.0 && value.is_finite() {
                    NODE_MIN_RADIUS
                        + (NODE_MAX_RADIUS - NODE_MIN_RADIUS) * (value.max(0.0) / largest).sqrt()
                } else {
                    NODE_DEFAULT_RADIUS
                }
            })
            .collect();
        Self {
            layout,
            positions,
            velocities: vec![[0.0; 2]; count],
            pinned,
            ends,
            links,
            distance: options.network_link_distance,
            // A replacement that keeps positions reheats like a drag instead of from scratch,
            // so surviving nodes adjust rather than scatter.
            alpha: match (layout, count) {
                (NetworkLayout::Force, 0) | (NetworkLayout::Circular | NetworkLayout::Fixed, _) => {
                    0.0
                }
                // Every node restores its own position: nothing left to simulate.
                (NetworkLayout::Force, _) if anchored_all => 0.0,
                (NetworkLayout::Force, _) if seeded_any => ALPHA_DRAG_TARGET,
                (NetworkLayout::Force, _) => 1.0,
            },
            alpha_target: 0.0,
            ticks: 0,
            random: 1,
            clocks: [None; 2],
            drag: None,
            bounds,
            radii,
            slots: graph::color_slots(nodes),
            quads: Vec::new(),
            next: Vec::new(),
            stack: Vec::new(),
        }
    }

    pub(super) fn heap_bytes(&self) -> usize {
        (self.positions.capacity() + self.velocities.capacity()) * std::mem::size_of::<Point>()
            + self.pinned.capacity() * std::mem::size_of::<Option<Point>>()
            + self.ends.capacity() * std::mem::size_of::<(u32, u32)>()
            + self.links.capacity() * std::mem::size_of::<ForceLink>()
            + self.radii.capacity() * std::mem::size_of::<f64>()
            + self.slots.capacity() * std::mem::size_of::<usize>()
            + self.quads.capacity() * std::mem::size_of::<Quad>()
            + (self.next.capacity() + self.stack.capacity()) * std::mem::size_of::<u32>()
    }

    /// Whether the force simulation still moves nodes.
    pub(super) fn active(&self) -> bool {
        // The tolerance absorbs the rounding of 300 decay steps, so a full reheat settles on
        // tick 300 as d3's nominal schedule promises.
        self.layout == NetworkLayout::Force && self.alpha >= ALPHA_MIN * (1.0 + 1e-9)
    }

    #[cfg(test)]
    pub(super) fn ticks(&self) -> u64 {
        self.ticks
    }

    #[cfg(test)]
    pub(super) fn positions(&self) -> &[Point] {
        &self.positions
    }

    /// Settled force positions in node order as persistable pins, or `None` while the
    /// simulation still moves nodes: an export captures rest state, never a half-settled
    /// layout. Circular and fixed layouts re-derive positions from the data on restore, so
    /// they persist nothing.
    pub(super) fn snapshot_positions(&self, nodes: &[GraphNode]) -> Option<Vec<NetworkPin>> {
        if self.layout != NetworkLayout::Force || self.active() || self.drag.is_some() {
            return None;
        }
        Some(
            nodes
                .iter()
                .zip(&self.positions)
                .map(|(node, &[x, y])| NetworkPin {
                    id: node.id.clone(),
                    x,
                    y,
                })
                .collect(),
        )
    }

    fn jiggle(&mut self) -> f64 {
        // d3-force 3.0 `lcg()`: a = 1664525, c = 1013904223, m = 2^32.
        self.random = (1_664_525 * self.random + 1_013_904_223) % 4_294_967_296;
        (self.random as f64 / 4_294_967_296.0 - 0.5) * 1e-6
    }

    /// One d3 tick: alpha step, link, many-body and center forces, then integration.
    pub(super) fn tick(&mut self) {
        self.alpha += (self.alpha_target - self.alpha) * alpha_decay();
        self.ticks += 1;
        let alpha = self.alpha;
        for index in 0..self.links.len() {
            let link = self.links[index];
            let (s, t) = (link.source as usize, link.target as usize);
            let mut x = self.positions[t][0] + self.velocities[t][0]
                - self.positions[s][0]
                - self.velocities[s][0];
            if x == 0.0 {
                x = self.jiggle();
            }
            let mut y = self.positions[t][1] + self.velocities[t][1]
                - self.positions[s][1]
                - self.velocities[s][1];
            if y == 0.0 {
                y = self.jiggle();
            }
            let length = (x * x + y * y).sqrt();
            let factor = (length - self.distance) / length * alpha * link.strength;
            let (x, y) = (x * factor, y * factor);
            self.velocities[t][0] -= x * link.bias;
            self.velocities[t][1] -= y * link.bias;
            self.velocities[s][0] += x * (1.0 - link.bias);
            self.velocities[s][1] += y * (1.0 - link.bias);
        }
        self.many_body(alpha);
        let count = self.positions.len() as f64;
        if count > 0.0 {
            let (mut sx, mut sy) = (0.0, 0.0);
            for p in &self.positions {
                sx += p[0];
                sy += p[1];
            }
            let (sx, sy) = (sx / count, sy / count);
            for p in &mut self.positions {
                p[0] -= sx;
                p[1] -= sy;
            }
        }
        let decay = 1.0 - VELOCITY_DECAY;
        for ((position, velocity), pin) in self
            .positions
            .iter_mut()
            .zip(&mut self.velocities)
            .zip(&self.pinned)
        {
            if let Some(pin) = pin {
                *position = *pin;
                *velocity = [0.0; 2];
            } else {
                velocity[0] *= decay;
                velocity[1] *= decay;
                position[0] += velocity[0];
                position[1] += velocity[1];
            }
        }
    }

    fn build_quadtree(&mut self) {
        let count = self.positions.len();
        self.quads.clear();
        self.next.clear();
        self.next.resize(count, NONE);
        if count == 0 {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in &self.positions {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        let size = (x1 - x0).max(y1 - y0).max(1.0);
        let capacity = quad_capacity(count);
        self.quads.push(Quad::leaf(size, NONE));
        for point in 0..count {
            let p = self.positions[point];
            let (mut quad, mut qx, mut qy, mut depth) = (0usize, x0, y0, 0u32);
            loop {
                let width = self.quads[quad].width;
                if self.quads[quad].internal {
                    let half = width * 0.5;
                    let right = p[0] >= qx + half;
                    let below = p[1] >= qy + half;
                    let slot = usize::from(right) + 2 * usize::from(below);
                    qx += if right { half } else { 0.0 };
                    qy += if below { half } else { 0.0 };
                    depth += 1;
                    let child = self.quads[quad].child[slot];
                    if child == NONE {
                        self.quads[quad].child[slot] = self.quads.len() as u32;
                        self.quads.push(Quad::leaf(half, point as u32));
                        break;
                    }
                    quad = child as usize;
                    continue;
                }
                let head = self.quads[quad].head;
                if head == NONE {
                    self.quads[quad].head = point as u32;
                    break;
                }
                if self.positions[head as usize] == p
                    || depth >= MAX_QUAD_DEPTH
                    || self.quads.len() >= capacity
                {
                    self.next[point] = head;
                    self.quads[quad].head = point as u32;
                    break;
                }
                // Split: the leaf's points all share one position, so its whole list moves
                // into the child quadrant that position falls in.
                let half = width * 0.5;
                let q = self.positions[head as usize];
                let slot = usize::from(q[0] >= qx + half) + 2 * usize::from(q[1] >= qy + half);
                self.quads[quad].internal = true;
                self.quads[quad].head = NONE;
                self.quads[quad].child[slot] = self.quads.len() as u32;
                self.quads.push(Quad::leaf(half, head));
            }
        }
        // Children always follow their parent, so a reverse sweep accumulates bottom-up.
        for quad in (0..self.quads.len()).rev() {
            if self.quads[quad].internal {
                let (mut value, mut weight, mut x, mut y) = (0.0, 0.0, 0.0, 0.0);
                for child in self.quads[quad].child {
                    if child == NONE {
                        continue;
                    }
                    let c = self.quads[child as usize];
                    let a = c.value.abs();
                    if a > 0.0 {
                        value += c.value;
                        weight += a;
                        x += a * c.x;
                        y += a * c.y;
                    }
                }
                let q = &mut self.quads[quad];
                q.value = value;
                if weight > 0.0 {
                    q.x = x / weight;
                    q.y = y / weight;
                }
            } else {
                let (mut n, mut x, mut y) = (0.0, 0.0, 0.0);
                let mut point = self.quads[quad].head;
                while point != NONE {
                    let p = self.positions[point as usize];
                    n += 1.0;
                    x += p[0];
                    y += p[1];
                    point = self.next[point as usize];
                }
                let q = &mut self.quads[quad];
                q.value = CHARGE * n;
                if n > 0.0 {
                    q.x = x / n;
                    q.y = y / n;
                }
            }
        }
    }

    fn many_body(&mut self, alpha: f64) {
        self.build_quadtree();
        let mut stack = std::mem::take(&mut self.stack);
        for node in 0..self.positions.len() {
            let p = self.positions[node];
            let (mut vx, mut vy) = (0.0, 0.0);
            stack.clear();
            stack.push(0);
            while let Some(quad) = stack.pop() {
                let q = self.quads[quad as usize];
                if q.value == 0.0 {
                    continue;
                }
                let (mut x, mut y) = (q.x - p[0], q.y - p[1]);
                let mut l = x * x + y * y;
                if q.width * q.width / THETA2 < l {
                    if x == 0.0 {
                        x = self.jiggle();
                        l += x * x;
                    }
                    if y == 0.0 {
                        y = self.jiggle();
                        l += y * y;
                    }
                    if l < 1.0 {
                        l = l.sqrt();
                    }
                    vx += x * q.value * alpha / l;
                    vy += y * q.value * alpha / l;
                    continue;
                }
                if q.internal {
                    stack.extend(q.child.into_iter().filter(|&child| child != NONE));
                    continue;
                }
                let mut other = q.head;
                while other != NONE {
                    if other as usize != node {
                        let o = self.positions[other as usize];
                        let (mut x, mut y) = (o[0] - p[0], o[1] - p[1]);
                        let mut l = x * x + y * y;
                        if x == 0.0 {
                            x = self.jiggle();
                            l += x * x;
                        }
                        if y == 0.0 {
                            y = self.jiggle();
                            l += y * y;
                        }
                        if l < 1.0 {
                            l = l.sqrt();
                        }
                        let w = CHARGE * alpha / l;
                        vx += x * w;
                        vy += y * w;
                    }
                    other = self.next[other as usize];
                }
            }
            self.velocities[node][0] += vx;
            self.velocities[node][1] += vy;
        }
        self.stack = stack;
    }

    /// Advance by the whole ticks clock `source` reports since it was anchored, at most
    /// [`MAX_TICKS_PER_ADVANCE`]. Both host clocks may drive one simulation: each keeps its own
    /// anchor and the simulation follows whichever is ahead, so a host that feeds both with
    /// one clock does not run twice as fast. Returns whether a tick ran.
    pub(super) fn advance(&mut self, source: usize, now_ms: f64) -> bool {
        if !self.active() {
            self.clocks = [None; 2];
            return false;
        }
        let Some((anchor, base)) = self.clocks[source].filter(|(anchor, _)| now_ms >= *anchor)
        else {
            self.clocks[source] = Some((now_ms, self.ticks));
            return false;
        };
        // The epsilon keeps a sample exactly one frame later from rounding down a tick.
        let due = base + ((now_ms - anchor) / TICK_MS + 1e-9).floor() as u64;
        if due <= self.ticks {
            return false;
        }
        let run = (due - self.ticks).min(MAX_TICKS_PER_ADVANCE);
        for _ in 0..run {
            self.tick();
            if !self.active() {
                break;
            }
        }
        if due > self.ticks {
            self.clocks[source] = Some((now_ms, self.ticks));
        }
        true
    }

    /// Run to rest synchronously, at most [`SETTLE_TICKS`] ticks. A drag in progress holds
    /// the simulation warm, so it does not settle until released.
    pub(super) fn settle(&mut self) -> bool {
        if self.drag.is_some() {
            return false;
        }
        let mut ran = false;
        for _ in 0..SETTLE_TICKS {
            if !self.active() {
                break;
            }
            self.tick();
            ran = true;
        }
        if self.layout == NetworkLayout::Force {
            self.alpha = 0.0;
        }
        self.clocks = [None; 2];
        ran
    }

    pub(super) fn transform(&self, plot: GeneralPlotRect, view: NetworkView) -> NetworkTransform {
        let anchor = [plot.x + plot.width * 0.5, plot.y + plot.height * 0.5];
        let half = plot.width.min(plot.height).max(0.0) * 0.5;
        let (base_center, base_scale) = match (self.layout, self.bounds) {
            (NetworkLayout::Force, _) | (NetworkLayout::Fixed, None) => ([0.0, 0.0], 1.0),
            (NetworkLayout::Circular, _) => ([0.0, 0.0], FIT * half),
            (NetworkLayout::Fixed, Some([x0, y0, x1, y1])) => {
                let fit = |span: f64, extent: f64| {
                    if span > 0.0 {
                        FIT * extent / span
                    } else {
                        f64::INFINITY
                    }
                };
                let scale = fit(x1 - x0, plot.width).min(fit(y1 - y0, plot.height));
                (
                    [(x0 + x1) * 0.5, (y0 + y1) * 0.5],
                    if scale.is_finite() { scale } else { 1.0 },
                )
            }
        };
        NetworkTransform {
            anchor,
            center: [base_center[0] + view.x, base_center[1] + view.y],
            scale: base_scale * view.zoom,
        }
    }

    /// Screen center of a node.
    fn node_screen(&self, transform: &NetworkTransform, row: usize) -> Option<Point> {
        self.positions.get(row).map(|&p| transform.screen(p))
    }

    /// Screen center and radius of a self-link loop: a circle through the node's center
    /// above and to the right of it.
    fn loop_circle(&self, transform: &NetworkTransform, row: usize) -> Option<(Point, f64)> {
        let center = self.node_screen(transform, row)?;
        let radius = self.radii[row];
        Some((
            [
                center[0] + radius * std::f64::consts::FRAC_1_SQRT_2,
                center[1] - radius * std::f64::consts::FRAC_1_SQRT_2,
            ],
            radius,
        ))
    }

    /// Topmost node under the point, else the nearest edge within [`HIT_SLOP`].
    pub(super) fn hit(
        &self,
        transform: &NetworkTransform,
        x: f64,
        y: f64,
    ) -> Option<(GeneralLayoutElement, usize)> {
        if let Some(row) = self.node_at(transform, x, y) {
            return Some((GeneralLayoutElement::Node, row));
        }
        let mut best: Option<(f64, usize)> = None;
        for (row, &(source, target)) in self.ends.iter().enumerate() {
            let distance = if source == target {
                let Some((center, radius)) = self.loop_circle(transform, source as usize) else {
                    continue;
                };
                ((x - center[0]).hypot(y - center[1]) - radius).abs()
            } else {
                let (Some(a), Some(b)) = (
                    self.node_screen(transform, source as usize),
                    self.node_screen(transform, target as usize),
                ) else {
                    continue;
                };
                segment_distance([x, y], a, b)
            };
            if distance <= HIT_SLOP && best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, row));
            }
        }
        best.map(|(_, row)| (GeneralLayoutElement::Link, row))
    }

    fn node_at(&self, transform: &NetworkTransform, x: f64, y: f64) -> Option<usize> {
        (0..self.positions.len()).rev().find(|&row| {
            let p = transform.screen(self.positions[row]);
            (x - p[0]).hypot(y - p[1]) <= self.radii[row] + HIT_SLOP * 0.5
        })
    }

    /// A node's center, an edge's midpoint, or the top of a self-link loop.
    pub(super) fn anchor(
        &self,
        transform: &NetworkTransform,
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<Point> {
        match element {
            GeneralLayoutElement::Node => self.node_screen(transform, row),
            GeneralLayoutElement::Link => {
                let &(source, target) = self.ends.get(row)?;
                if source == target {
                    let (center, radius) = self.loop_circle(transform, source as usize)?;
                    return Some([center[0], center[1] - radius]);
                }
                let a = self.node_screen(transform, source as usize)?;
                let b = self.node_screen(transform, target as usize)?;
                Some([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5])
            }
            _ => None,
        }
    }

    /// Apply one drag phase in screen coordinates. Returns whether positions changed.
    fn drag(
        &mut self,
        transform: &NetworkTransform,
        row: usize,
        phase: NetworkDragPhase,
        reduced_motion: bool,
    ) -> bool {
        match phase {
            NetworkDragPhase::Begin(x, y) => {
                let Some(&position) = self.positions.get(row) else {
                    return false;
                };
                let grab = transform.data([x, y]);
                self.drag = Some(Drag {
                    row,
                    before_pin: self.pinned[row],
                    before_position: position,
                    offset: [position[0] - grab[0], position[1] - grab[1]],
                });
                if self.layout == NetworkLayout::Force && !reduced_motion {
                    self.alpha_target = ALPHA_DRAG_TARGET;
                    // d3 `restart()` from rest: the target pulls alpha up from here.
                    if !self.active() {
                        self.alpha = self.alpha.max(0.0);
                        self.alpha += (self.alpha_target - self.alpha) * alpha_decay();
                    }
                }
                false
            }
            NetworkDragPhase::Move(x, y) => {
                let Some(drag) = self.drag.filter(|drag| drag.row == row) else {
                    return false;
                };
                let grab = transform.data([x, y]);
                let p = [
                    (grab[0] + drag.offset[0]).clamp(-MAX_COORDINATE, MAX_COORDINATE),
                    (grab[1] + drag.offset[1]).clamp(-MAX_COORDINATE, MAX_COORDINATE),
                ];
                self.pinned[row] = Some(p);
                self.positions[row] = p;
                self.velocities[row] = [0.0; 2];
                true
            }
            NetworkDragPhase::End => {
                if self.drag.take().is_none() {
                    return false;
                }
                self.alpha_target = 0.0;
                if self.layout == NetworkLayout::Force && reduced_motion {
                    // Neighbors still answer the move, all at once.
                    self.alpha = self.alpha.max(ALPHA_DRAG_TARGET);
                }
                true
            }
            NetworkDragPhase::Cancel => {
                let Some(drag) = self.drag.take() else {
                    return false;
                };
                self.alpha_target = 0.0;
                self.pinned[drag.row] = drag.before_pin;
                self.positions[drag.row] = drag.before_position;
                self.velocities[drag.row] = [0.0; 2];
                true
            }
        }
    }
}

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// The part of segment `a → b` inside `plot` (Liang–Barsky), if any.
fn clip_segment(a: Point, b: Point, plot: GeneralPlotRect) -> Option<(Point, Point)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a[0] - plot.x),
        (dx, plot.x + plot.width - a[0]),
        (-dy, a[1] - plot.y),
        (dy, plot.y + plot.height - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| {
        (
            [a[0] + t0 * dx, a[1] + t0 * dy],
            [a[0] + t1 * dx, a[1] + t1 * dy],
        )
    })
}

fn circle_visible(center: Point, radius: f64, plot: GeneralPlotRect) -> bool {
    center[0] + radius >= plot.x
        && center[0] - radius <= plot.x + plot.width
        && center[1] + radius >= plot.y
        && center[1] - radius <= plot.y + plot.height
}

impl LayoutSeriesEntry {
    fn network_transform(&self, plot: GeneralPlotRect) -> Option<NetworkTransform> {
        self.network
            .as_ref()
            .map(|sim| sim.transform(plot, self.options.network_view))
    }

    pub(super) fn network_hit(
        &self,
        plot: GeneralPlotRect,
        x: f64,
        y: f64,
    ) -> Option<(GeneralLayoutElement, usize)> {
        let transform = self.network_transform(plot)?;
        self.network.as_ref()?.hit(&transform, x, y)
    }

    pub(super) fn network_anchor(
        &self,
        plot: GeneralPlotRect,
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<(f64, f64)> {
        let transform = self.network_transform(plot)?;
        let [x, y] = self.network.as_ref()?.anchor(&transform, element, row)?;
        (x >= plot.x && x <= plot.x + plot.width && y >= plot.y && y <= plot.y + plot.height)
            .then_some((x, y))
    }

    /// Rebuild the simulation after `options` or data changed, keeping surviving positions.
    pub(super) fn rebuild_network(
        &mut self,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
        values: &[f64],
        previous_nodes: Option<&[GraphNode]>,
    ) {
        if self.options.kind != GeneralLayoutSeriesKind::Network {
            return;
        }
        let previous = previous_nodes.zip(self.network.as_deref());
        let sim = NetworkSim::new(&self.options, nodes, edges, values, previous);
        self.network = Some(Box::new(sim));
    }
}

/// Keep only pins whose node survives a data replacement.
pub(super) fn retain_pins(pinned: &mut Vec<NetworkPin>, nodes: &[GraphNode]) {
    let ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    pinned.retain(|pin| ids.contains(pin.id.as_str()));
}

impl ChartEngine {
    fn network_reduced_motion(&self) -> bool {
        self.general_transition_reduced_motion || self.interaction_options().reduced_motion
    }

    /// Whether a visible force simulation still needs frames. Reduced motion settles every
    /// simulation synchronously, so nothing animates.
    pub(crate) fn general_layout_simulation_active(&self) -> bool {
        !self.network_reduced_motion()
            && self.general_layout_data.as_ref().is_some_and(|store| {
                store.series.iter().any(|series| {
                    series.options.visible
                        && series.network.as_ref().is_some_and(|sim| sim.active())
                })
            })
    }

    /// Advance visible force simulations on host clock `source` (0: the general animation
    /// clock, 1: the input tick). Returns whether any node moved.
    pub(crate) fn advance_network_simulations(&mut self, source: usize, now_ms: f64) -> bool {
        if !now_ms.is_finite() {
            return false;
        }
        if self.network_reduced_motion() {
            return self.settle_network_simulations();
        }
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let mut moved = false;
        for series in store
            .series
            .iter_mut()
            .filter(|series| series.options.visible)
        {
            if let Some(sim) = series.network.as_mut() {
                moved |= sim.advance(source, now_ms);
            }
        }
        if moved {
            self.invalidate_frame_scene();
        }
        moved
    }

    /// Under reduced motion, run every warm simulation to rest at once. Returns whether any
    /// node moved.
    pub(crate) fn settle_network_simulations(&mut self) -> bool {
        if !self.network_reduced_motion() {
            return false;
        }
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let mut moved = false;
        for series in &mut store.series {
            if let Some(sim) = series.network.as_mut() {
                moved |= sim.settle();
            }
        }
        if moved {
            self.invalidate_frame_scene();
        }
        moved
    }

    /// Topmost visible network series whose plot contains `(x, y)`, with the node under the
    /// point if any.
    pub(crate) fn network_press_target(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(GeneralLayoutSeriesId, Option<usize>)> {
        let pane = self.pane_at_y(y)?;
        let pane_id = self.pane_stable_id(pane)?;
        let plot = self.general_plot_rect(pane)?;
        if x < plot.x || x >= plot.x + plot.width || y < plot.y || y >= plot.y + plot.height {
            return None;
        }
        let store = self.general_layout_data.as_ref()?;
        let series = store.series.iter().rev().find(|series| {
            series.pane_id == pane_id && series.options.visible && series.network.is_some()
        })?;
        let transform = series.network_transform(plot)?;
        let node = series.network.as_ref()?.node_at(&transform, x, y);
        Some((series.id, node))
    }

    fn network_series_index(&self, id: GeneralLayoutSeriesId) -> Option<(usize, GeneralPlotRect)> {
        let store = self.general_layout_data.as_ref()?;
        let index = store
            .series
            .iter()
            .position(|series| series.id == id && series.network.is_some())?;
        let plot = self
            .pane_index_for_id(store.series[index].pane_id)
            .and_then(|pane| self.general_plot_rect(pane))?;
        Some((index, plot))
    }

    pub(crate) fn network_view(&self, id: GeneralLayoutSeriesId) -> Option<NetworkView> {
        self.general_layout_series_options(id)
            .filter(|options| options.kind == GeneralLayoutSeriesKind::Network)
            .map(|options| options.network_view)
    }

    /// Drive a node drag of a network series. `End` keeps the node pinned at its release
    /// position and records the pin in the series options; `Cancel` restores the node's
    /// position and pin from before the drag. Returns whether the frame changed.
    pub(crate) fn network_drag(
        &mut self,
        id: GeneralLayoutSeriesId,
        row: usize,
        phase: NetworkDragPhase,
    ) -> bool {
        let Some((index, plot)) = self.network_series_index(id) else {
            return false;
        };
        let reduced_motion = self.network_reduced_motion();
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let dataset = store.series[index].options.dataset;
        let node_id = match store.entry(dataset).map(|entry| &entry.input) {
            Some(GeneralLayoutInput::Graph { nodes, .. }) => nodes.get(row).map(|n| n.id.clone()),
            _ => None,
        };
        let Some(node_id) = node_id else {
            return false;
        };
        let series = &mut store.series[index];
        let view = series.options.network_view;
        let Some(sim) = series.network.as_mut() else {
            return false;
        };
        let transform = sim.transform(plot, view);
        let changed = sim.drag(&transform, row, phase, reduced_motion);
        match phase {
            NetworkDragPhase::End if changed => {
                let [x, y] = sim.pinned[row].unwrap_or(sim.positions[row]);
                let pins = &mut series.options.pinned;
                match pins.iter_mut().find(|pin| pin.id == node_id) {
                    Some(pin) => (pin.x, pin.y) = (x, y),
                    None => pins.push(NetworkPin { id: node_id, x, y }),
                }
            }
            NetworkDragPhase::Cancel if changed => {
                let restored = sim.pinned[row];
                let pins = &mut series.options.pinned;
                pins.retain(|pin| pin.id != node_id);
                if let Some([x, y]) = restored {
                    pins.push(NetworkPin { id: node_id, x, y });
                }
            }
            _ => {}
        }
        if changed {
            self.invalidate_frame_scene();
        }
        if phase == NetworkDragPhase::End {
            self.settle_network_simulations();
        }
        changed
    }

    /// Set the view of a network series. Returns whether it changed.
    pub(crate) fn set_network_view(
        &mut self,
        id: GeneralLayoutSeriesId,
        view: NetworkView,
    ) -> bool {
        if validate_view(view).is_err() {
            return false;
        }
        let Some(series) = self.general_layout_data.as_mut().and_then(|store| {
            store
                .series
                .iter_mut()
                .find(|series| series.id == id && series.network.is_some())
        }) else {
            return false;
        };
        if series.options.network_view == view {
            return false;
        }
        series.options.network_view = view;
        self.invalidate_frame_scene();
        true
    }

    /// Pan a network view by a screen delta from the view `before` the gesture began.
    pub(crate) fn network_pan(
        &mut self,
        id: GeneralLayoutSeriesId,
        before: NetworkView,
        dx: f64,
        dy: f64,
    ) -> bool {
        let Some((index, plot)) = self.network_series_index(id) else {
            return false;
        };
        let Some(scale) = self.general_layout_data.as_ref().and_then(|store| {
            let series = &store.series[index];
            series
                .network
                .as_ref()
                .map(|sim| sim.transform(plot, before).scale)
        }) else {
            return false;
        };
        let view = NetworkView {
            x: (before.x - dx / scale).clamp(-MAX_COORDINATE, MAX_COORDINATE),
            y: (before.y - dy / scale).clamp(-MAX_COORDINATE, MAX_COORDINATE),
            zoom: before.zoom,
        };
        self.set_network_view(id, view)
    }

    /// Zoom the network under `(x, y)` by `factor` around that point, within the zoom range.
    /// Returns whether a network view was under the point; a zoom already at its limit still
    /// consumes the gesture.
    pub(crate) fn network_zoom_at(&mut self, x: f64, y: f64, factor: f64) -> bool {
        let Some((id, _)) = self.network_press_target(x, y) else {
            return false;
        };
        self.network_zoom_about(id, Some((x, y)), factor);
        true
    }

    /// Zoom a network view by `factor` around a screen point, or the plot center.
    pub(crate) fn network_zoom_about(
        &mut self,
        id: GeneralLayoutSeriesId,
        point: Option<(f64, f64)>,
        factor: f64,
    ) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let Some((index, plot)) = self.network_series_index(id) else {
            return false;
        };
        let Some((view, transform, base)) = self.general_layout_data.as_ref().and_then(|store| {
            let series = &store.series[index];
            let view = series.options.network_view;
            let sim = series.network.as_ref()?;
            let unit = NetworkView {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            };
            Some((view, sim.transform(plot, view), sim.transform(plot, unit)))
        }) else {
            return false;
        };
        let q = point.map_or(transform.anchor, |(x, y)| [x, y]);
        let anchor = transform.data(q);
        let zoom = (view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let scale = base.scale * zoom;
        let center = [
            anchor[0] - (q[0] - transform.anchor[0]) / scale,
            anchor[1] - (q[1] - transform.anchor[1]) / scale,
        ];
        let next = NetworkView {
            x: (center[0] - base.center[0]).clamp(-MAX_COORDINATE, MAX_COORDINATE),
            y: (center[1] - base.center[1]).clamp(-MAX_COORDINATE, MAX_COORDINATE),
            zoom,
        };
        self.set_network_view(id, next)
    }

    /// Edges in input order, then nodes in input order with their labels. A label is placed
    /// to the right of its node only when it lies inside the plot and overlaps no label placed
    /// before it.
    pub(super) fn build_network_frame(
        &self,
        series: &LayoutSeriesEntry,
        nodes: &[GraphNode],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(sim) = series.network.as_ref() else {
            return;
        };
        let transform = sim.transform(plot, series.options.network_view);
        let ratio = hpr.min(vpr);
        let to_bitmap = |[x, y]: Point| [(x * hpr) as f32, (y * vpr) as f32];
        let transparent = Color::rgba(0, 0, 0, 0);
        for &(source, target) in &sim.ends {
            if source == target {
                if let Some((center, radius)) = sim.loop_circle(&transform, source as usize)
                    && circle_visible(center, radius, plot)
                {
                    let [cx, cy] = to_bitmap(center);
                    out.push(Prim::Circle {
                        cx,
                        cy,
                        radius: (radius * ratio) as f32,
                        fill: transparent,
                        stroke_width: (EDGE_WIDTH * ratio) as f32,
                        stroke: EDGE_COLOR,
                    });
                }
                continue;
            }
            let a = transform.screen(sim.positions[source as usize]);
            let b = transform.screen(sim.positions[target as usize]);
            let Some((a, b)) = clip_segment(a, b, plot) else {
                continue;
            };
            let first_point = points.len() as u32;
            points.push(to_bitmap(a));
            points.push(to_bitmap(b));
            out.push(Prim::Polyline {
                first_point,
                point_count: 2,
                width: (EDGE_WIDTH * ratio) as f32,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color: EDGE_COLOR,
            });
        }
        let surface = self.chart_surface_color();
        for (row, &position) in sim.positions.iter().enumerate() {
            let center = transform.screen(position);
            let radius = sim.radii[row];
            if !circle_visible(center, radius, plot) {
                continue;
            }
            let [cx, cy] = to_bitmap(center);
            out.push(Prim::Circle {
                cx,
                cy,
                radius: (radius * ratio) as f32,
                fill: mosaic::segment_color(series.color, sim.slots[row]),
                stroke_width: (NODE_STROKE * ratio) as f32,
                stroke: surface,
            });
        }
        let text_color = self.primary_text_color();
        let family = self.options.get().layout.font_family.clone();
        let mut placed: Vec<[f64; 4]> = Vec::new();
        for (row, node) in nodes.iter().enumerate() {
            if placed.len() >= MAX_LABELS {
                break;
            }
            if node.label.is_empty() {
                continue;
            }
            let Some(&position) = sim.positions.get(row) else {
                break;
            };
            let center = transform.screen(position);
            let x0 = center[0] + sim.radii[row] + LABEL_GAP;
            let (y0, y1) = (center[1] - LABEL_SIZE * 0.5, center[1] + LABEL_SIZE * 0.5);
            if x0 < plot.x || y0 < plot.y || y1 > plot.y + plot.height {
                continue;
            }
            let width = self.measure_text_run(&node.label, LABEL_SIZE, &family, 400, false);
            let x1 = x0 + width;
            if x1 > plot.x + plot.width
                || placed
                    .iter()
                    .any(|b| x0 < b[2] && x1 > b[0] && y0 < b[3] && y1 > b[1])
            {
                continue;
            }
            placed.push([x0, y0, x1, y1]);
            out.push(Prim::Text {
                x: (x0 * hpr) as f32,
                y: (center[1] * vpr) as f32,
                text: node.label.clone(),
                color: text_color,
                size: (LABEL_SIZE * vpr) as f32,
                family: family.clone(),
                align: TextAlign::Left,
                weight: 400,
                italic: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: usize) -> GraphNode {
        GraphNode {
            id: format!("n{id}"),
            label: String::new(),
            category: None,
            value: None,
            x: None,
            y: None,
        }
    }

    fn graph(count: usize, edges_per_node: usize) -> (Vec<GraphNode>, Vec<GraphEdge>) {
        let nodes: Vec<GraphNode> = (0..count).map(node).collect();
        let mut edges = Vec::new();
        for source in 0..count {
            for step in 1..=edges_per_node {
                // A deterministic sparse mix of short and long-range links.
                let target = (source + step * 1_031) % count;
                if target != source {
                    edges.push(GraphEdge {
                        source: format!("n{source}"),
                        target: format!("n{target}"),
                        value: 1.0,
                    });
                }
            }
        }
        let mut seen = HashSet::new();
        edges.retain(|edge| seen.insert((edge.source.clone(), edge.target.clone())));
        (nodes, edges)
    }

    fn sim(nodes: &[GraphNode], edges: &[GraphEdge]) -> NetworkSim {
        let options = GeneralLayoutSeriesOptions::new(
            0,
            crate::GeneralLayoutDatasetId::from_raw(1).unwrap(),
            GeneralLayoutSeriesKind::Network,
        );
        NetworkSim::new(&options, nodes, edges, &[], None)
    }

    #[test]
    fn force_runs_are_deterministic_and_settle_in_300_ticks() {
        let (nodes, edges) = graph(64, 2);
        let mut a = sim(&nodes, &edges);
        let mut b = sim(&nodes, &edges);
        assert_eq!(a.positions()[3], phyllotaxis(3));
        assert!(a.settle());
        assert_eq!(a.ticks(), u64::from(SETTLE_TICKS));
        assert!(!a.active());
        for _ in 0..SETTLE_TICKS {
            b.tick();
        }
        assert!(!b.active());
        assert_eq!(a.positions(), b.positions());
        // The center force re-centers positions before each integration step, so the centroid
        // stays within one step's mean velocity of (0, 0).
        let (sx, sy) = a
            .positions()
            .iter()
            .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
        let n = a.positions().len() as f64;
        assert!((sx / n).abs() < 0.5 && (sy / n).abs() < 0.5, "{sx} {sy}");
    }

    #[test]
    fn linked_nodes_settle_near_the_link_distance_and_coincident_nodes_separate() {
        let nodes = vec![node(0), node(1)];
        let edges = vec![GraphEdge {
            source: "n0".into(),
            target: "n1".into(),
            value: 1.0,
        }];
        let mut pair = sim(&nodes, &edges);
        pair.settle();
        let [a, b] = [pair.positions()[0], pair.positions()[1]];
        let distance = (a[0] - b[0]).hypot(a[1] - b[1]);
        // Repulsion stretches the link a little past its rest length.
        assert!(distance > 30.0 && distance < 45.0, "{distance}");
        let mut stacked = sim(&nodes, &[]);
        stacked.positions = vec![[5.0, 5.0]; 2];
        stacked.settle();
        assert_ne!(stacked.positions()[0], stacked.positions()[1]);
    }

    #[test]
    fn clock_advances_whole_ticks_with_a_per_sample_bound() {
        let (nodes, edges) = graph(16, 1);
        let mut s = sim(&nodes, &edges);
        assert!(!s.advance(0, 1_000.0), "the first sample anchors the clock");
        assert!(!s.advance(0, 1_000.0 + TICK_MS * 0.5));
        assert!(s.advance(0, 1_000.0 + TICK_MS));
        assert_eq!(s.ticks(), 1);
        // A long stall runs at most the bound and drops the backlog.
        assert!(s.advance(0, 1_000.0 + TICK_MS * 50.0));
        assert_eq!(s.ticks(), 1 + MAX_TICKS_PER_ADVANCE);
        assert!(s.advance(0, 1_000.0 + TICK_MS * 51.0));
        assert_eq!(s.ticks(), 2 + MAX_TICKS_PER_ADVANCE);
        // A second clock on the same timebase follows the first instead of doubling speed.
        assert!(!s.advance(1, 1_000.0 + TICK_MS * 51.0));
        assert!(!s.advance(0, 1_000.0 + TICK_MS * 51.5));
        assert!(s.advance(1, 1_000.0 + TICK_MS * 52.0));
        assert!(!s.advance(0, 1_000.0 + TICK_MS * 52.0));
        assert_eq!(s.ticks(), 3 + MAX_TICKS_PER_ADVANCE);
    }

    #[test]
    fn segment_clipping_keeps_only_the_plot_part() {
        let plot = GeneralPlotRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let (a, b) = clip_segment([-50.0, 50.0], [150.0, 50.0], plot).unwrap();
        assert_eq!((a, b), ([0.0, 50.0], [100.0, 50.0]));
        assert!(clip_segment([-50.0, -10.0], [150.0, -10.0], plot).is_none());
        assert!(segment_distance([50.0, 3.0], [0.0, 0.0], [100.0, 0.0]) - 3.0 < 1e-12);
    }

    /// Release-build cost of one force tick at the layout row and edge caps. Run with
    /// `cargo test -p aeris_charts_engine --features complete --release network_tick_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing measurement; run in release"]
    fn network_tick_cost_at_the_caps() {
        let (nodes, edges) = graph(super::super::MAX_LAYOUT_ROWS, 4);
        assert_eq!(edges.len(), super::super::MAX_FLOW_LINKS);
        let mut s = sim(&nodes, &edges);
        let start = std::time::Instant::now();
        let mut samples = Vec::new();
        for _ in 0..SETTLE_TICKS {
            let tick = std::time::Instant::now();
            s.tick();
            samples.push(tick.elapsed().as_secs_f64() * 1_000.0);
        }
        let total = start.elapsed().as_secs_f64() * 1_000.0;
        samples.sort_by(f64::total_cmp);
        println!(
            "network tick at {} nodes / {} edges: median {:.3} ms, p95 {:.3} ms, max {:.3} ms, 300 ticks {:.1} ms, quads {}",
            nodes.len(),
            edges.len(),
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100],
            samples[samples.len() - 1],
            total,
            s.quads.len()
        );
    }
}
