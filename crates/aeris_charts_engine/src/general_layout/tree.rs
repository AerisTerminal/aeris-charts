//! Tidy tree and dendrogram layouts of a hierarchy dataset (complete build), following the
//! d3-hierarchy 3.1.2 algorithm definitions. `tree()` is Buchheim, Jünger and Leipert's
//! linear-time Reingold–Tilford layout; `cluster()` places every leaf at the deepest level and
//! each parent over the mean of its children. Both use d3's default separation: 1 between
//! siblings and 2 between cousins. Only the visible tree is laid out, so a collapsed node is a
//! leaf until it expands. Horizontal trees put the root on the left, vertical trees on top, and
//! radial trees at the center with angle 0 at 12 o'clock increasing clockwise. Tree links are
//! cubic "bump" curves (d3 `linkHorizontal`/`linkVertical`/`linkRadial`); dendrogram links are
//! orthogonal elbows (radial elbows follow the parent's circle). Frame, hits and anchors read
//! the same cached node positions.

use std::collections::HashMap;
use std::f64::consts::TAU;

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::{
    GeneralLayoutAccessibilityItem, GeneralLayoutElement, GeneralLayoutHit, GeneralLayoutInput,
    GeneralLayoutSeriesKind, HierarchyNode, HierarchyOrientation, LayoutSeriesEntry,
    MAX_LAYOUT_STORE_BYTES,
};
use crate::ChartEngine;
use crate::general_axes::GeneralPlotRect;

/// ECharts `tree` series default `left`/`right`/`top`/`bottom` of 12% of the plot.
const MARGIN: f64 = 0.12;
/// ECharts `tree` default `symbolSize` 7 and `itemStyle.borderWidth` 1.5.
const NODE_RADIUS: f64 = 3.5;
const NODE_STROKE: f64 = 1.5;
/// ECharts `tree` default `lineStyle`: `#ccc` at width 1.5.
const LINK_COLOR: Color = Color(0xCCCC_CCFF);
const LINK_WIDTH: f64 = 1.5;
/// Flattened segments of each curved link.
const LINK_SEGMENTS: usize = 16;
/// Arc segments per full turn of a radial elbow.
const ARC_SEGMENTS_PER_TURN: f64 = 72.0;
/// A pointer within this distance of a node center hits the node.
const HIT_RADIUS: f64 = 8.0;
const LABEL_SIZE: f64 = 12.0;
const LABEL_GAP: f64 = 8.0;

type Point = (f64, f64);

/// A hierarchy's topology resolved against a series' collapsed node IDs. Descendants of a
/// collapsed node are hidden: they paint nothing, hit nothing, and are absent from
/// accessibility snapshots.
pub(super) struct TreeOutline {
    root: usize,
    parents: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    depth: Vec<usize>,
    /// Rows with children whose subtree is folded away.
    collapsed: Vec<bool>,
    hidden: Vec<bool>,
    /// Visible rows in depth-first pre-order, children in input order. A subtree is contiguous,
    /// so collapsing or expanding a node never moves the node itself in this order.
    order: Vec<usize>,
}

impl TreeOutline {
    pub(super) fn new(nodes: &[HierarchyNode], collapsed_ids: &[String]) -> Option<Self> {
        let ids: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(row, node)| (node.id.as_str(), row))
            .collect();
        let count = nodes.len();
        let mut parents = vec![None; count];
        let mut children = vec![Vec::new(); count];
        let mut root = None;
        for (row, node) in nodes.iter().enumerate() {
            match node.parent.as_deref().and_then(|parent| ids.get(parent)) {
                Some(&parent) => {
                    parents[row] = Some(parent);
                    children[parent].push(row);
                }
                None => root = root.or(Some(row)),
            }
        }
        let root = root?;
        let mut collapsed = vec![false; count];
        for id in collapsed_ids {
            if let Some(&row) = ids.get(id.as_str())
                && !children[row].is_empty()
            {
                collapsed[row] = true;
            }
        }
        let mut hidden = vec![true; count];
        let mut depth = vec![0; count];
        let mut order = Vec::with_capacity(count);
        let mut stack = vec![root];
        while let Some(row) = stack.pop() {
            hidden[row] = false;
            order.push(row);
            if !collapsed[row] {
                for &child in children[row].iter().rev() {
                    depth[child] = depth[row] + 1;
                    stack.push(child);
                }
            }
        }
        Some(Self {
            root,
            parents,
            children,
            depth,
            collapsed,
            hidden,
            order,
        })
    }

    fn visible_children(&self, row: usize) -> &[usize] {
        if self.collapsed[row] {
            &[]
        } else {
            &self.children[row]
        }
    }

    fn heap_bytes(&self) -> usize {
        self.parents.capacity() * std::mem::size_of::<Option<usize>>()
            + self.children.capacity() * std::mem::size_of::<Vec<usize>>()
            + self
                .children
                .iter()
                .map(|list| list.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + (self.depth.capacity() + self.order.capacity()) * std::mem::size_of::<usize>()
            + self.collapsed.capacity()
            + self.hidden.capacity()
    }
}

/// Buchheim's walk state over the visible tree plus d3's virtual root at index `count`.
struct Tidy<'a> {
    children: &'a [&'a [usize]],
    parent: Vec<usize>,
    /// Position among siblings.
    index: Vec<usize>,
    /// Preliminary x, modifier, change, shift, thread, ancestor and default ancestor.
    z: Vec<f64>,
    m: Vec<f64>,
    c: Vec<f64>,
    s: Vec<f64>,
    thread: Vec<Option<usize>>,
    ancestor: Vec<usize>,
    default_ancestor: Vec<Option<usize>>,
}

impl Tidy<'_> {
    fn separation(&self, a: usize, b: usize) -> f64 {
        if self.parent[a] == self.parent[b] {
            1.0
        } else {
            2.0
        }
    }

    fn next_left(&self, v: usize) -> Option<usize> {
        self.children[v].first().copied().or(self.thread[v])
    }

    fn next_right(&self, v: usize) -> Option<usize> {
        self.children[v].last().copied().or(self.thread[v])
    }

    fn move_subtree(&mut self, wm: usize, wp: usize, shift: f64) {
        let change = shift / (self.index[wp] as f64 - self.index[wm] as f64);
        self.c[wp] -= change;
        self.s[wp] += shift;
        self.c[wm] += change;
        self.z[wp] += shift;
        self.m[wp] += shift;
    }

    fn execute_shifts(&mut self, v: usize) {
        let (mut shift, mut change) = (0.0, 0.0);
        for &w in self.children[v].iter().rev() {
            self.z[w] += shift;
            self.m[w] += shift;
            change += self.c[w];
            shift += self.s[w] + change;
        }
    }

    fn next_ancestor(&self, vim: usize, v: usize, ancestor: usize) -> usize {
        if self.parent[self.ancestor[vim]] == self.parent[v] {
            self.ancestor[vim]
        } else {
            ancestor
        }
    }

    fn first_walk(&mut self, v: usize) {
        let children = self.children;
        for &child in children[v] {
            self.first_walk(child);
        }
        let parent = self.parent[v];
        let siblings = children[parent];
        let w = (self.index[v] > 0).then(|| siblings[self.index[v] - 1]);
        if let (Some(&first), Some(&last)) = (children[v].first(), children[v].last()) {
            self.execute_shifts(v);
            let midpoint = (self.z[first] + self.z[last]) / 2.0;
            if let Some(w) = w {
                self.z[v] = self.z[w] + self.separation(v, w);
                self.m[v] = self.z[v] - midpoint;
            } else {
                self.z[v] = midpoint;
            }
        } else if let Some(w) = w {
            self.z[v] = self.z[w] + self.separation(v, w);
        }
        let default = self.default_ancestor[parent].unwrap_or(siblings[0]);
        self.default_ancestor[parent] = Some(self.apportion(v, w, default));
    }

    fn apportion(&mut self, v: usize, w: Option<usize>, mut ancestor: usize) -> usize {
        let Some(w) = w else {
            return ancestor;
        };
        let (mut vip, mut vop, mut vim) = (v, v, w);
        let mut vom = self.children[self.parent[v]][0];
        let (mut sip, mut sop, mut sim, mut som) =
            (self.m[vip], self.m[vop], self.m[vim], self.m[vom]);
        let mut next_im = self.next_right(vim);
        let mut next_ip = self.next_left(vip);
        while let (Some(im), Some(ip)) = (next_im, next_ip) {
            vim = im;
            vip = ip;
            let (Some(om), Some(op)) = (self.next_left(vom), self.next_right(vop)) else {
                return ancestor;
            };
            vom = om;
            vop = op;
            self.ancestor[vop] = v;
            let shift = self.z[vim] + sim - self.z[vip] - sip + self.separation(vim, vip);
            if shift > 0.0 {
                let moved = self.next_ancestor(vim, v, ancestor);
                self.move_subtree(moved, v, shift);
                sip += shift;
                sop += shift;
            }
            sim += self.m[vim];
            sip += self.m[vip];
            som += self.m[vom];
            sop += self.m[vop];
            next_im = self.next_right(vim);
            next_ip = self.next_left(vip);
        }
        if let Some(im) = next_im
            && self.next_right(vop).is_none()
        {
            self.thread[vop] = Some(im);
            self.m[vop] += sim - sop;
        }
        if let Some(ip) = next_ip
            && self.next_left(vom).is_none()
        {
            self.thread[vom] = Some(ip);
            self.m[vom] += sip - som;
            ancestor = v;
        }
        ancestor
    }

    fn second_walk(&mut self, v: usize, x: &mut [f64]) {
        let parent = self.parent[v];
        x[v] = self.z[v] + self.m[parent];
        self.m[v] += self.m[parent];
        let children = self.children;
        for &child in children[v] {
            self.second_walk(child, x);
        }
    }
}

/// `children` holds the visible children of every row followed by the virtual root's only
/// child, the real root. Returns d3 `(x, y)` per row sized to `(dx, dy)`; hidden rows are NaN.
fn tidy_layout(outline: &TreeOutline, children: &[&[usize]], (dx, dy): (f64, f64)) -> Vec<Point> {
    let count = children.len() - 1;
    let mut parent = vec![count; count + 1];
    let mut index = vec![0; count + 1];
    for (row, list) in children.iter().enumerate() {
        for (position, &child) in list.iter().enumerate() {
            parent[child] = row;
            index[child] = position;
        }
    }
    let mut tidy = Tidy {
        children,
        parent,
        index,
        z: vec![0.0; count + 1],
        m: vec![0.0; count + 1],
        c: vec![0.0; count + 1],
        s: vec![0.0; count + 1],
        thread: vec![None; count + 1],
        ancestor: (0..=count).collect(),
        default_ancestor: vec![None; count + 1],
    };
    let root = outline.root;
    tidy.first_walk(root);
    tidy.m[count] = -tidy.z[root];
    let mut x = vec![f64::NAN; count];
    tidy.second_walk(root, &mut x);
    let (mut left, mut right, mut bottom) = (root, root, root);
    for &row in &outline.order {
        if x[row] < x[left] {
            left = row;
        }
        if x[row] > x[right] {
            right = row;
        }
        if outline.depth[row] > outline.depth[bottom] {
            bottom = row;
        }
    }
    let s = if left == right {
        1.0
    } else {
        tidy.separation(left, right) / 2.0
    };
    let tx = s - x[left];
    let kx = dx / (x[right] + s + tx);
    let ky = dy / outline.depth[bottom].max(1) as f64;
    let mut out = vec![(f64::NAN, f64::NAN); count];
    for &row in &outline.order {
        out[row] = ((x[row] + tx) * kx, outline.depth[row] as f64 * ky);
    }
    out
}

/// d3 `cluster()` over the visible tree, sized to `(dx, dy)`; hidden rows are NaN.
fn cluster_layout(outline: &TreeOutline, (dx, dy): (f64, f64)) -> Vec<Point> {
    struct Cluster<'a> {
        outline: &'a TreeOutline,
        x: Vec<f64>,
        y: Vec<f64>,
        previous: Option<usize>,
        cursor: f64,
    }
    impl Cluster<'_> {
        fn separation(&self, a: usize, b: usize) -> f64 {
            if self.outline.parents[a] == self.outline.parents[b] {
                1.0
            } else {
                2.0
            }
        }

        fn visit(&mut self, node: usize) {
            let outline = self.outline;
            let children = outline.visible_children(node);
            for &child in children {
                self.visit(child);
            }
            if children.is_empty() {
                self.x[node] = match self.previous {
                    Some(previous) => {
                        self.cursor += self.separation(node, previous);
                        self.cursor
                    }
                    None => 0.0,
                };
                self.y[node] = 0.0;
                self.previous = Some(node);
            } else {
                self.x[node] = children.iter().map(|&child| self.x[child]).sum::<f64>()
                    / children.len() as f64;
                self.y[node] = 1.0
                    + children
                        .iter()
                        .fold(0.0_f64, |deepest, &child| deepest.max(self.y[child]));
            }
        }
    }
    let count = outline.parents.len();
    let mut cluster = Cluster {
        outline,
        x: vec![0.0; count],
        y: vec![0.0; count],
        previous: None,
        cursor: 0.0,
    };
    let root = outline.root;
    cluster.visit(root);
    let extreme = |first: bool| {
        let mut node = root;
        while let Some(&child) = if first {
            outline.visible_children(node).first()
        } else {
            outline.visible_children(node).last()
        } {
            node = child;
        }
        node
    };
    let (left, right) = (extreme(true), extreme(false));
    let x0 = cluster.x[left] - cluster.separation(left, right) / 2.0;
    let x1 = cluster.x[right] + cluster.separation(right, left) / 2.0;
    let root_y = cluster.y[root];
    let mut out = vec![(f64::NAN, f64::NAN); count];
    for &row in &outline.order {
        let depth = if root_y != 0.0 {
            cluster.y[row] / root_y
        } else {
            1.0
        };
        out[row] = ((cluster.x[row] - x0) / (x1 - x0) * dx, (1.0 - depth) * dy);
    }
    out
}

pub(super) fn cache_reservation(nodes: usize) -> usize {
    2 * nodes
        * (std::mem::size_of::<Point>()
            + std::mem::size_of::<Option<Point>>()
            + 2 * std::mem::size_of::<Option<usize>>()
            + 3 * std::mem::size_of::<usize>()
            + std::mem::size_of::<Vec<usize>>()
            + 2)
}

pub(super) struct TreeCache {
    plot: GeneralPlotRect,
    orientation: HierarchyOrientation,
    elbow: bool,
    center: Point,
    /// d3 breadth (`x`) and depth (`y`) of each visible row: radians and radius for radial
    /// trees, layout pixels inside the margins otherwise.
    layout: Vec<Point>,
    /// Plot position of each visible row.
    points: Vec<Option<Point>>,
    outline: Option<TreeOutline>,
}

impl TreeCache {
    fn new(
        kind: GeneralLayoutSeriesKind,
        nodes: &[HierarchyNode],
        orientation: HierarchyOrientation,
        collapsed: &[String],
        plot: GeneralPlotRect,
    ) -> Self {
        let inner = GeneralPlotRect {
            x: plot.x + plot.width * MARGIN,
            y: plot.y + plot.height * MARGIN,
            width: plot.width * (1.0 - 2.0 * MARGIN),
            height: plot.height * (1.0 - 2.0 * MARGIN),
        };
        let center = (inner.x + inner.width * 0.5, inner.y + inner.height * 0.5);
        let mut cache = Self {
            plot,
            orientation,
            elbow: kind == GeneralLayoutSeriesKind::Dendrogram,
            center,
            layout: Vec::new(),
            points: Vec::new(),
            outline: None,
        };
        if !(inner.width > 0.0 && inner.height > 0.0) {
            return cache;
        }
        let Some(outline) = TreeOutline::new(nodes, collapsed) else {
            return cache;
        };
        let size = match orientation {
            HierarchyOrientation::Horizontal => (inner.height, inner.width),
            HierarchyOrientation::Vertical => (inner.width, inner.height),
            HierarchyOrientation::Radial => (TAU, inner.width.min(inner.height) * 0.5),
        };
        cache.layout = if cache.elbow {
            cluster_layout(&outline, size)
        } else {
            let mut children: Vec<&[usize]> = (0..nodes.len())
                .map(|row| outline.visible_children(row))
                .collect();
            children.push(std::slice::from_ref(&outline.root));
            tidy_layout(&outline, &children, size)
        };
        cache.points = vec![None; nodes.len()];
        for &row in &outline.order {
            let (x, y) = cache.layout[row];
            cache.points[row] = Some(match orientation {
                HierarchyOrientation::Horizontal => (inner.x + y, inner.y + x),
                HierarchyOrientation::Vertical => (inner.x + x, inner.y + y),
                HierarchyOrientation::Radial => cache.polar(x, y),
            });
        }
        cache.outline = Some(outline);
        cache
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.layout.capacity() * std::mem::size_of::<Point>()
            + self.points.capacity() * std::mem::size_of::<Option<Point>>()
            + self.outline.as_ref().map_or(0, TreeOutline::heap_bytes)
    }

    /// d3 `pointRadial`: angle 0 at 12 o'clock, clockwise on screen.
    fn polar(&self, angle: f64, radius: f64) -> Point {
        (
            self.center.0 + radius * angle.sin(),
            self.center.1 - radius * angle.cos(),
        )
    }

    /// The link from `parent` to `child` as a polyline in plot coordinates.
    fn link_points(&self, parent: usize, child: usize, mut push: impl FnMut(Point)) {
        let (Some(source), Some(target)) = (self.points[parent], self.points[child]) else {
            return;
        };
        let (a0, r0) = self.layout[parent];
        let (a1, r1) = self.layout[child];
        if self.elbow {
            push(source);
            match self.orientation {
                HierarchyOrientation::Horizontal => push((source.0, target.1)),
                HierarchyOrientation::Vertical => push((target.0, source.1)),
                HierarchyOrientation::Radial => {
                    let segments =
                        (((a1 - a0).abs() * ARC_SEGMENTS_PER_TURN / TAU).ceil() as usize).max(1);
                    for step in 1..=segments {
                        push(self.polar(a0 + (a1 - a0) * step as f64 / segments as f64, r0));
                    }
                }
            }
            push(target);
            return;
        }
        let (c1, c2) = match self.orientation {
            HierarchyOrientation::Horizontal => {
                let mid = (source.0 + target.0) * 0.5;
                ((mid, source.1), (mid, target.1))
            }
            HierarchyOrientation::Vertical => {
                let mid = (source.1 + target.1) * 0.5;
                ((source.0, mid), (target.0, mid))
            }
            HierarchyOrientation::Radial => {
                let mid = (r0 + r1) * 0.5;
                (self.polar(a0, mid), self.polar(a1, mid))
            }
        };
        for step in 0..=LINK_SEGMENTS {
            let t = step as f64 / LINK_SEGMENTS as f64;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            push((
                b0 * source.0 + b1 * c1.0 + b2 * c2.0 + b3 * target.0,
                b0 * source.1 + b1 * c1.1 + b2 * c2.1 + b3 * target.1,
            ));
        }
    }

    /// The nearest visible node within [`HIT_RADIUS`] of the pointer.
    pub(super) fn hit(&self, x: f64, y: f64) -> Option<usize> {
        let outline = self.outline.as_ref()?;
        let mut best = None;
        let mut best_distance = HIT_RADIUS * HIT_RADIUS;
        for &row in &outline.order {
            let Some((px, py)) = self.points[row] else {
                continue;
            };
            let distance = (px - x) * (px - x) + (py - y) * (py - y);
            if distance <= best_distance {
                best = Some(row);
                best_distance = distance;
            }
        }
        best
    }

    pub(super) fn anchor(&self, row: usize) -> Option<Point> {
        self.points.get(row).copied().flatten()
    }

    /// Label anchor and alignment beside a node. Nodes with visible children label toward the
    /// root side and the rest away from it, like ECharts' default tree labels.
    fn label_position(
        &self,
        outline: &TreeOutline,
        row: usize,
        (x, y): Point,
    ) -> (Point, TextAlign) {
        let open = !outline.visible_children(row).is_empty();
        match self.orientation {
            HierarchyOrientation::Horizontal if open => ((x - LABEL_GAP, y), TextAlign::Right),
            HierarchyOrientation::Horizontal => ((x + LABEL_GAP, y), TextAlign::Left),
            HierarchyOrientation::Vertical if open => ((x - LABEL_GAP, y), TextAlign::Right),
            HierarchyOrientation::Vertical => {
                ((x, y + LABEL_GAP + LABEL_SIZE * 0.5), TextAlign::Center)
            }
            HierarchyOrientation::Radial if row == outline.root => {
                ((x, y - LABEL_GAP - LABEL_SIZE * 0.5), TextAlign::Center)
            }
            HierarchyOrientation::Radial => {
                let angle = self.layout[row].0;
                let (ux, uy) = (angle.sin(), -angle.cos());
                let align = if ux > 0.1 {
                    TextAlign::Left
                } else if ux < -0.1 {
                    TextAlign::Right
                } else {
                    TextAlign::Center
                };
                (
                    // Prim::Text anchors the vertical center: labels above the node stand on
                    // the anchor and labels below it hang from it.
                    (x + ux * LABEL_GAP, y + uy * (LABEL_GAP + LABEL_SIZE * 0.5)),
                    align,
                )
            }
        }
    }
}

impl LayoutSeriesEntry {
    pub(super) fn with_tree<R>(
        &self,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        f: impl FnOnce(&TreeCache) -> R,
    ) -> R {
        let mut cache = self.tree.borrow_mut();
        if cache.as_ref().is_none_or(|cached| {
            cached.plot != plot || cached.orientation != self.options.hierarchy_orientation
        }) {
            *cache = Some(TreeCache::new(
                self.options.kind,
                nodes,
                self.options.hierarchy_orientation,
                &self.options.collapsed,
                plot,
            ));
        }
        f(cache.as_ref().expect("tree cache initialized"))
    }
}

impl ChartEngine {
    /// Links in visible pre-order, then each node with its label. A collapsed node is a filled
    /// circle; expanded nodes and leaves are hollow.
    pub(super) fn build_tree_frame(
        &self,
        series: &LayoutSeriesEntry,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        series.with_tree(nodes, plot, |cache| {
            let Some(outline) = cache.outline.as_ref() else {
                return;
            };
            let ratio = hpr.min(vpr);
            let to_bitmap = |(x, y): Point| [(x * hpr) as f32, (y * vpr) as f32];
            for &row in &outline.order {
                let Some(parent) = outline.parents[row] else {
                    continue;
                };
                let first_point = points.len() as u32;
                cache.link_points(parent, row, |point| points.push(to_bitmap(point)));
                out.push(Prim::Polyline {
                    first_point,
                    point_count: points.len() as u32 - first_point,
                    width: (LINK_WIDTH * ratio) as f32,
                    style: LineStyle::Solid,
                    line_type: LineType::Simple,
                    color: LINK_COLOR,
                });
            }
            let surface = self.chart_surface_color();
            let text_color = self.primary_text_color();
            let family = self.options.get().layout.font_family.clone();
            for &row in &outline.order {
                let Some(center) = cache.points[row] else {
                    continue;
                };
                let [cx, cy] = to_bitmap(center);
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius: (NODE_RADIUS * ratio) as f32,
                    fill: if outline.collapsed[row] {
                        series.color
                    } else {
                        surface
                    },
                    stroke_width: (NODE_STROKE * ratio) as f32,
                    stroke: series.color,
                });
                let label = &nodes[row].label;
                if label.is_empty() {
                    continue;
                }
                let (position, align) = cache.label_position(outline, row, center);
                let [x, y] = to_bitmap(position);
                out.push(Prim::Text {
                    x,
                    y,
                    text: label.clone(),
                    color: text_color,
                    size: (LABEL_SIZE * vpr) as f32,
                    family: family.clone(),
                    align,
                    weight: 400,
                    italic: false,
                });
            }
        });
    }

    /// Accessibility items of a tree or dendrogram: visible nodes in depth-first order, each
    /// node with children reporting whether it is expanded.
    pub(super) fn collapsible_accessibility_items(
        &self,
        series: &LayoutSeriesEntry,
        nodes: &[HierarchyNode],
        values: &[f64],
        selected: Option<GeneralLayoutHit>,
        offset: usize,
        limit: usize,
    ) -> Option<(usize, Vec<GeneralLayoutAccessibilityItem>)> {
        if !series.options.kind.is_collapsible() {
            return None;
        }
        let Some(outline) = TreeOutline::new(nodes, &series.options.collapsed) else {
            return Some((0, Vec::new()));
        };
        let items = outline
            .order
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&row| {
                let hit = GeneralLayoutHit {
                    series: series.id,
                    element: GeneralLayoutElement::Node,
                    row,
                };
                GeneralLayoutAccessibilityItem {
                    element: GeneralLayoutElement::Node,
                    row,
                    id: nodes[row].id.clone(),
                    label: nodes[row].label.clone(),
                    value: values.get(row).copied(),
                    selected: selected == Some(hit),
                    expanded: (!outline.children[row].is_empty())
                        .then_some(!outline.collapsed[row]),
                    placed: None,
                }
            })
            .collect();
        Some((outline.order.len(), items))
    }

    /// Folds or unfolds the subtree of a visible tree or dendrogram node with children. Hover,
    /// selection and keyboard focus on rows the fold hides are cleared. Returns whether the
    /// collapse state changed.
    pub(crate) fn toggle_general_layout_node_collapsed(&mut self, hit: GeneralLayoutHit) -> bool {
        if hit.element != GeneralLayoutElement::Node {
            return false;
        }
        let Some(store) = self.general_layout_data.as_mut() else {
            return false;
        };
        let Some(index) = store
            .series
            .iter()
            .position(|series| series.id == hit.series && series.options.visible)
        else {
            return false;
        };
        let options = &store.series[index].options;
        let Some(GeneralLayoutInput::Hierarchy(nodes)) = store
            .entries
            .iter()
            .find(|entry| entry.id == options.dataset)
            .map(|entry| &entry.input)
        else {
            return false;
        };
        if !options.kind.is_collapsible() {
            return false;
        }
        let Some(outline) = TreeOutline::new(nodes, &options.collapsed) else {
            return false;
        };
        if hit.row >= nodes.len() || outline.hidden[hit.row] || outline.children[hit.row].is_empty()
        {
            return false;
        }
        let id = nodes[hit.row].id.clone();
        let fits = store
            .reserved_bytes()
            .saturating_add(id.capacity() + std::mem::size_of::<String>())
            <= MAX_LAYOUT_STORE_BYTES;
        let series = &mut store.series[index];
        if let Some(position) = series.options.collapsed.iter().position(|item| *item == id) {
            series.options.collapsed.remove(position);
        } else if fits {
            series.options.collapsed.push(id);
        } else {
            return false;
        }
        series.clear_caches();
        let hidden = TreeOutline::new(nodes, &series.options.collapsed)
            .map(|outline| outline.hidden)
            .unwrap_or_default();
        let hides = |slot: &Option<GeneralLayoutHit>| {
            slot.is_some_and(|other| {
                other.series == hit.series && hidden.get(other.row).copied().unwrap_or(true)
            })
        };
        if hides(&store.hovered) {
            store.hovered = None;
        }
        if hides(&store.selected) {
            store.selected = None;
        }
        if hides(&store.focused) {
            store.focused = None;
        }
        self.invalidate_frame_all();
        true
    }
}

/// Whether a tree or dendrogram row sits under a collapsed ancestor.
pub(super) fn row_hidden(series: &LayoutSeriesEntry, nodes: &[HierarchyNode], row: usize) -> bool {
    series.options.kind.is_collapsible()
        && !series.options.collapsed.is_empty()
        && TreeOutline::new(nodes, &series.options.collapsed)
            .is_none_or(|outline| outline.hidden.get(row).copied().unwrap_or(true))
}
