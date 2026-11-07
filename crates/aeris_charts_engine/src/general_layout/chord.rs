//! Chord diagram of a directed graph with the pinned ECharts 6.1.0 `chord` series layout and
//! defaults: each node is a group on a ring between 70% and 80% of half the shorter plot side,
//! spanning an angle proportional to the larger of its explicit `value` and its incoming plus
//! outgoing edge value, clockwise from 12 o'clock with 3° after every group. Edges claim
//! subgroups at both ends in input order. Each edge is a ribbon from its source subgroup to its
//! target subgroup through quadratic curves with the center as control point, painted in input
//! order in its source group's color at 20% opacity. Frame meshes and hit tests use the same
//! triangles.

use std::f64::consts::{PI, TAU};

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use super::gauge::push_sector_mesh;
use super::{
    GeneralLayoutElement, GraphEdge, GraphNode, LayoutSeriesEntry, SunburstShape, graph, mosaic,
    point_in_triangle,
};
use crate::ChartEngine;
use crate::general_axes::GeneralPlotRect;

const INNER_RADIUS: f64 = 0.7;
const OUTER_RADIUS: f64 = 0.8;
/// 3° after each nonempty group, shrunk so the pads never take more than half the circle.
/// ECharts also pads after empty groups, which pushes the ring past a full turn, and lets the
/// pads take the whole circle when there are 120 or more groups, which leaves nothing to paint.
const PAD_ANGLE: f64 = 3.0 * PI / 180.0;
/// 12 o'clock as a screen angle (radians clockwise from three o'clock, Y down).
const START_ANGLE: f64 = -0.5 * PI;
/// Flattened segments of each ribbon's curved sides.
const RIBBON_SEGMENTS: usize = 16;
/// Arc segments per full turn of a ribbon end cap.
const CAP_SEGMENTS_PER_TURN: f64 = 72.0;
/// ECharts' `lineStyle.opacity` 0.2.
const RIBBON_ALPHA: u8 = 51;
const LABEL_SIZE: f64 = 12.0;
const LABEL_GAP: f64 = 5.0;

type Point = (f64, f64);

/// One edge's ribbon: its source and target subgroup screen angles. A zero-value edge has an
/// empty ribbon that paints nothing and is not hit.
#[derive(Clone, Copy, Debug)]
struct Ribbon {
    source_node: usize,
    source: (f64, f64),
    target: (f64, f64),
    /// `(min_x, min_y, max_x, max_y)` of the ribbon's triangles.
    bounds: (f64, f64, f64, f64),
}

impl Ribbon {
    fn is_empty(&self) -> bool {
        self.source.1 <= self.source.0
    }
}

pub(super) struct ChordCache {
    plot: GeneralPlotRect,
    cx: f64,
    cy: f64,
    inner: f64,
    outer: f64,
    /// One group per node, in input order.
    groups: Vec<SunburstShape>,
    /// Color slot of each group (see [`graph::color_slots`]).
    slots: Vec<usize>,
    /// One ribbon per edge, in input order.
    ribbons: Vec<Ribbon>,
}

pub(super) fn cache_reservation(nodes: usize, edges: usize) -> usize {
    2 * (nodes * (std::mem::size_of::<SunburstShape>() + std::mem::size_of::<usize>())
        + edges * std::mem::size_of::<Ribbon>())
}

impl ChordCache {
    fn new(nodes: &[GraphNode], edges: &[GraphEdge], plot: GeneralPlotRect) -> Self {
        let half = plot.width.min(plot.height).max(0.0) * 0.5;
        let mut cache = Self {
            plot,
            cx: plot.x + plot.width * 0.5,
            cy: plot.y + plot.height * 0.5,
            inner: INNER_RADIUS * half,
            outer: OUTER_RADIUS * half,
            groups: Vec::with_capacity(nodes.len()),
            slots: graph::color_slots(nodes),
            ribbons: Vec::with_capacity(edges.len()),
        };
        let index = graph::node_index(nodes);
        let ends: Vec<(usize, usize)> = edges
            .iter()
            .map(|edge| {
                (
                    index.get(edge.source.as_str()).copied().unwrap_or(0),
                    index.get(edge.target.as_str()).copied().unwrap_or(0),
                )
            })
            .collect();
        // ECharts lays out an all-zero graph as if every edge weighed 1.
        let all_zero = nodes.iter().all(|node| node.value.unwrap_or(0.0) == 0.0)
            && edges.iter().all(|edge| edge.value == 0.0);
        let edge_value = |edge: &GraphEdge| if all_zero { 1.0 } else { edge.value };
        let mut weights = vec![0.0; nodes.len()];
        for (edge, &(source, target)) in edges.iter().zip(&ends) {
            weights[source] += edge_value(edge);
            weights[target] += edge_value(edge);
        }
        for (weight, node) in weights.iter_mut().zip(nodes) {
            *weight = node.value.unwrap_or(0.0).max(*weight);
        }
        let total: f64 = weights.iter().sum();
        let rendered = if all_zero {
            2 * edges.len()
        } else {
            weights.iter().filter(|&&weight| weight > 0.0).count()
        };
        let pad = if rendered == 0 {
            0.0
        } else {
            PAD_ANGLE.min(PI / rendered as f64)
        };
        let scale = if edges.is_empty() || rendered == 0 || !(total > 0.0 && total.is_finite()) {
            0.0
        } else {
            (TAU - pad * rendered as f64) / total
        };
        let mut angle = START_ANGLE;
        let mut next = Vec::with_capacity(nodes.len());
        for (node, weight) in weights.iter().enumerate() {
            let span = weight * scale;
            cache.groups.push(SunburstShape {
                row: node,
                cx: cache.cx,
                cy: cache.cy,
                inner: cache.inner,
                outer: cache.outer,
                start: angle,
                end: angle + span,
            });
            next.push(angle);
            if *weight > 0.0 || all_zero {
                angle += span + pad;
            }
        }
        let mut source_angles = Vec::with_capacity(edges.len());
        let mut target_angles = Vec::with_capacity(edges.len());
        for (edge, &(source, target)) in edges.iter().zip(&ends) {
            let span = edge_value(edge) * scale;
            // Both ends read their group cursor before either advances, so a self-link's
            // source and target subgroups coincide.
            let (source_start, target_start) = (next[source], next[target]);
            source_angles.push((source_start, source_start + span));
            target_angles.push((target_start, target_start + span));
            next[source] = source_start + span;
            next[target] = target_start + span;
        }
        for (row, &(source_node, _)) in ends.iter().enumerate() {
            let mut ribbon = Ribbon {
                source_node,
                source: source_angles[row],
                target: target_angles[row],
                bounds: (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ),
            };
            let mut bounds = ribbon.bounds;
            cache.ribbon_triangles(&ribbon, |triangle| {
                for (x, y) in triangle {
                    bounds = (
                        bounds.0.min(x),
                        bounds.1.min(y),
                        bounds.2.max(x),
                        bounds.3.max(y),
                    );
                }
            });
            ribbon.bounds = bounds;
            cache.ribbons.push(ribbon);
        }
        cache
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.groups.capacity() * std::mem::size_of::<SunburstShape>()
            + self.slots.capacity() * std::mem::size_of::<usize>()
            + self.ribbons.capacity() * std::mem::size_of::<Ribbon>()
    }

    fn drawable(&self) -> bool {
        self.outer - self.inner >= 1.0 && self.inner > 0.0
    }

    fn point(&self, angle: f64) -> Point {
        (
            self.cx + self.inner * angle.cos(),
            self.cy + self.inner * angle.sin(),
        )
    }

    /// Quadratic Bézier from `from` to `to` with the center as its control point.
    fn curve(&self, from: Point, to: Point, t: f64) -> Point {
        let u = 1.0 - t;
        (
            u * u * from.0 + 2.0 * u * t * self.cx + t * t * to.0,
            u * u * from.1 + 2.0 * u * t * self.cy + t * t * to.1,
        )
    }

    /// The ribbon's triangles: both end caps (the circular segments between each subgroup's
    /// arc and its chord) and the strip between the side curves `source.0 → target.1` and
    /// `source.1 → target.0`.
    fn ribbon_triangles(&self, ribbon: &Ribbon, mut f: impl FnMut([Point; 3])) {
        if ribbon.is_empty() {
            return;
        }
        for (start, end) in [ribbon.source, ribbon.target] {
            let segments = (((end - start) * CAP_SEGMENTS_PER_TURN / TAU).ceil() as usize).max(1);
            let first = self.point(start);
            let at =
                |index: usize| self.point(start + (end - start) * index as f64 / segments as f64);
            for index in 1..segments {
                f([first, at(index), at(index + 1)]);
            }
        }
        let (a0, a1) = (self.point(ribbon.source.0), self.point(ribbon.source.1));
        let (b0, b1) = (self.point(ribbon.target.0), self.point(ribbon.target.1));
        let mut p = a0;
        let mut q = a1;
        for segment in 1..=RIBBON_SEGMENTS {
            let t = segment as f64 / RIBBON_SEGMENTS as f64;
            let (next_p, next_q) = (self.curve(a0, b1, t), self.curve(a1, b0, t));
            f([p, q, next_q]);
            f([p, next_q, next_p]);
            p = next_p;
            q = next_q;
        }
    }

    fn ribbon_contains(&self, ribbon: &Ribbon, x: f64, y: f64) -> bool {
        let (min_x, min_y, max_x, max_y) = ribbon.bounds;
        if ribbon.is_empty() || x < min_x || x > max_x || y < min_y || y > max_y {
            return false;
        }
        let mut inside = false;
        self.ribbon_triangles(ribbon, |[a, b, c]| {
            inside = inside || point_in_triangle((x, y), a, b, c);
        });
        inside
    }

    /// Groups on the ring first, then ribbons from the last painted to the first.
    pub(super) fn hit(&self, x: f64, y: f64) -> Option<(GeneralLayoutElement, usize)> {
        if !self.drawable() {
            return None;
        }
        if let Some(group) = self
            .groups
            .iter()
            .find(|group| group.end > group.start && group.contains(x, y))
        {
            return Some((GeneralLayoutElement::Node, group.row));
        }
        let (dx, dy) = (x - self.cx, y - self.cy);
        if dx * dx + dy * dy > self.inner * self.inner {
            return None;
        }
        self.ribbons
            .iter()
            .rposition(|ribbon| self.ribbon_contains(ribbon, x, y))
            .map(|row| (GeneralLayoutElement::Link, row))
    }

    /// A group's ring middle, or the middle of a ribbon's strip a quarter of the way from its
    /// source.
    pub(super) fn anchor(&self, element: GeneralLayoutElement, row: usize) -> Option<Point> {
        if !self.drawable() {
            return None;
        }
        match element {
            GeneralLayoutElement::Node => {
                let group = self.groups.get(row)?;
                let angle = (group.start + group.end) * 0.5;
                let radius = (group.inner + group.outer) * 0.5;
                Some((
                    self.cx + radius * angle.cos(),
                    self.cy + radius * angle.sin(),
                ))
            }
            GeneralLayoutElement::Link => {
                let ribbon = self.ribbons.get(row).filter(|ribbon| !ribbon.is_empty())?;
                let p = self.curve(
                    self.point(ribbon.source.0),
                    self.point(ribbon.target.1),
                    0.25,
                );
                let q = self.curve(
                    self.point(ribbon.source.1),
                    self.point(ribbon.target.0),
                    0.25,
                );
                Some(((p.0 + q.0) * 0.5, (p.1 + q.1) * 0.5))
            }
            _ => None,
        }
    }

    fn group_color(&self, series_color: Color, node: usize) -> Color {
        mosaic::segment_color(series_color, self.slots.get(node).copied().unwrap_or(node))
    }
}

fn ribbon_color(group: Color) -> Color {
    Color((group.0 & !0xFF) | u32::from(RIBBON_ALPHA))
}

impl LayoutSeriesEntry {
    pub(super) fn with_chord<R>(
        &self,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
        plot: GeneralPlotRect,
        f: impl FnOnce(&ChordCache) -> R,
    ) -> R {
        let mut cache = self.chord.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(ChordCache::new(nodes, edges, plot));
        }
        f(cache.as_ref().expect("chord cache initialized"))
    }
}

impl ChartEngine {
    /// Ribbons in edge order, then the group ring, then node labels outside the ring. A label
    /// is skipped when its group's arc is shorter than the label's text height.
    pub(super) fn build_chord_frame(
        &self,
        series: &LayoutSeriesEntry,
        (nodes, edges): (&[GraphNode], &[GraphEdge]),
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        series.with_chord(nodes, edges, plot, |cache| {
            if !cache.drawable() {
                return;
            }
            for ribbon in &cache.ribbons {
                let first_point = points.len() as u32;
                cache.ribbon_triangles(ribbon, |triangle| {
                    points.extend(triangle.map(|(x, y)| [(x * hpr) as f32, (y * vpr) as f32]));
                });
                let point_count = points.len() as u32 - first_point;
                if point_count > 0 {
                    out.push(Prim::MeshFill {
                        first_point,
                        point_count,
                        color: ribbon_color(cache.group_color(series.color, ribbon.source_node)),
                    });
                }
            }
            for group in &cache.groups {
                push_sector_mesh(
                    *group,
                    cache.group_color(series.color, group.row),
                    (hpr, vpr),
                    out,
                    points,
                );
            }
            let options = self.options.get();
            let text_color = self.primary_text_color();
            let radius = cache.outer + LABEL_GAP;
            for (group, node) in cache.groups.iter().zip(nodes) {
                if node.label.is_empty() || (group.end - group.start) * cache.outer < LABEL_SIZE {
                    continue;
                }
                let angle = (group.start + group.end) * 0.5;
                let (ux, uy) = (angle.cos(), angle.sin());
                let align = if ux > 0.1 {
                    TextAlign::Left
                } else if ux < -0.1 {
                    TextAlign::Right
                } else {
                    TextAlign::Center
                };
                // Prim::Text anchors the vertical center: labels above the ring stand on the
                // anchor and labels below it hang from the anchor.
                let shift = uy * LABEL_SIZE * 0.5;
                out.push(Prim::Text {
                    x: ((cache.cx + ux * radius) * hpr) as f32,
                    y: ((cache.cy + uy * radius + shift) * vpr) as f32,
                    text: node.label.clone(),
                    color: text_color,
                    size: (LABEL_SIZE * vpr) as f32,
                    family: options.layout.font_family.clone(),
                    align,
                    weight: 400,
                    italic: false,
                });
            }
        });
    }

    /// Label, painted color, and detail of a chord node or ribbon.
    pub(super) fn chord_item_tooltip(
        &self,
        series: &LayoutSeriesEntry,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<(String, Color, Option<String>)> {
        let slots = graph::color_slots(nodes);
        match element {
            GeneralLayoutElement::Node => {
                let node = nodes.get(row)?;
                Some((
                    node.label.clone(),
                    mosaic::segment_color(series.color, slots[row]),
                    None,
                ))
            }
            GeneralLayoutElement::Link => {
                let edge = edges.get(row)?;
                let source = nodes.iter().position(|node| node.id == edge.source)?;
                Some((
                    self.graph_link_label(nodes, edge),
                    mosaic::segment_color(series.color, slots[source]),
                    None,
                ))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLOT: GeneralPlotRect = GeneralPlotRect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };

    fn node(id: &str) -> GraphNode {
        GraphNode {
            id: id.into(),
            label: id.into(),
            category: None,
            value: None,
            x: None,
            y: None,
        }
    }

    fn edge(source: &str, target: &str, value: f64) -> GraphEdge {
        GraphEdge {
            source: source.into(),
            target: target.into(),
            value,
        }
    }

    #[test]
    fn groups_and_subgroups_follow_the_echarts_chord_layout() {
        let nodes = [node("a"), node("b"), node("c")];
        let edges = [
            edge("a", "b", 2.0),
            edge("b", "c", 1.0),
            edge("c", "a", 1.0),
        ];
        let cache = ChordCache::new(&nodes, &edges, PLOT);
        let scale = (TAU - 3.0 * PAD_ANGLE) / 8.0;
        assert!((cache.groups[0].start - START_ANGLE).abs() < 1e-12);
        let spans: Vec<f64> = cache
            .groups
            .iter()
            .map(|group| (group.end - group.start) / scale)
            .collect();
        for (span, expected) in spans.iter().zip([3.0, 3.0, 2.0]) {
            assert!((span - expected).abs() < 1e-9, "{spans:?}");
        }
        assert!((cache.groups[1].start - cache.groups[0].end - PAD_ANGLE).abs() < 1e-12);
        // Edges claim subgroups in input order: a→b first in group a, then c→a.
        assert!((cache.ribbons[0].source.0 - START_ANGLE).abs() < 1e-12);
        assert!((cache.ribbons[2].target.0 - cache.ribbons[0].source.1).abs() < 1e-12);
        assert!((cache.groups[0].end - cache.ribbons[2].target.1).abs() < 1e-12);
        assert!((cache.ribbons[0].target.0 - cache.groups[1].start).abs() < 1e-12);
        assert!((cache.ribbons[1].source.0 - cache.ribbons[0].target.1).abs() < 1e-12);
    }

    #[test]
    fn hits_and_anchors_agree_for_groups_ribbons_and_self_links() {
        let nodes = [node("a"), node("b"), node("c")];
        let edges = [
            edge("a", "b", 2.0),
            edge("b", "b", 1.0),
            edge("c", "a", 0.0),
        ];
        let cache = ChordCache::new(&nodes, &edges, PLOT);
        for (row, group) in cache.groups.iter().enumerate() {
            if group.end > group.start {
                let (x, y) = cache.anchor(GeneralLayoutElement::Node, row).unwrap();
                assert_eq!(cache.hit(x, y), Some((GeneralLayoutElement::Node, row)));
            }
        }
        for row in 0..2 {
            let (x, y) = cache.anchor(GeneralLayoutElement::Link, row).unwrap();
            assert_eq!(cache.hit(x, y), Some((GeneralLayoutElement::Link, row)));
        }
        assert_eq!(cache.anchor(GeneralLayoutElement::Link, 2), None);
        assert_eq!(cache.hit(PLOT.width - 1.0, 1.0), None);
    }

    #[test]
    fn pads_follow_only_nonempty_groups_and_never_exceed_half_the_circle() {
        let nodes: Vec<GraphNode> = (0..400).map(|index| node(&index.to_string())).collect();
        let edges = [edge("0", "1", 1.0)];
        let cache = ChordCache::new(&nodes, &edges, PLOT);
        let last = cache.groups.last().unwrap();
        assert!((last.end - (START_ANGLE + TAU)).abs() < 1e-9);
        let ring: Vec<GraphEdge> = (0..400)
            .map(|index| edge(&index.to_string(), &((index + 1) % 400).to_string(), 1.0))
            .collect();
        let cache = ChordCache::new(&nodes, &ring, PLOT);
        let painted: f64 = cache
            .groups
            .iter()
            .map(|group| group.end - group.start)
            .sum();
        assert!((painted - PI).abs() < 1e-9);
        // An all-zero graph is laid out as if every edge weighed 1.
        let zero = [edge("0", "1", 0.0)];
        let cache = ChordCache::new(&nodes[..2], &zero, PLOT);
        assert!(!cache.ribbons[0].is_empty());
    }
}
