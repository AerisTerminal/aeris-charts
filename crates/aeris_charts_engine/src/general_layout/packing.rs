//! Circle packing of a hierarchy dataset (complete build), following the d3-hierarchy 3.1.2
//! `pack()` definition: each leaf's radius is the square root of its value, siblings are placed
//! by Wang et al.'s front-chain packing, and each parent is the smallest circle enclosing its
//! children (Welzl's algorithm over d3's shuffled order, using d3's seeded linear congruential
//! generator so equal input gives equal output). A second pass pads every parent by
//! [`PADDING`] layout pixels and the result is scaled to fit the shorter plot side, centered.
//! Circles paint parents before children, so hits take the deepest circle under the pointer.

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{Prim, TextAlign};

use super::{HierarchyNode, LayoutSeriesEntry, hierarchy_layout_tree};
use crate::ChartEngine;
use crate::general_axes::GeneralPlotRect;

/// Gap between a parent's edge and its children and between siblings, in layout pixels.
const PADDING: f64 = 3.0;
const PARENT_ALPHA: u8 = 0x40;
const LABEL_SIZE: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Circle {
    x: f64,
    y: f64,
    r: f64,
}

/// d3-hierarchy's `lcg()`: `s = (1664525 s + 1013904223) mod 2^32`, starting from 1.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = (1_664_525 * self.0 + 1_013_904_223) % (1 << 32);
        self.0 as f64 / (1_u64 << 32) as f64
    }
}

fn place(b: Circle, a: Circle, c: &mut Circle) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d2 = dx * dx + dy * dy;
    if d2 != 0.0 {
        let a2 = (a.r + c.r) * (a.r + c.r);
        let b2 = (b.r + c.r) * (b.r + c.r);
        if a2 > b2 {
            let x = (d2 + b2 - a2) / (2.0 * d2);
            let y = (b2 / d2 - x * x).max(0.0).sqrt();
            c.x = b.x - x * dx - y * dy;
            c.y = b.y - x * dy + y * dx;
        } else {
            let x = (d2 + a2 - b2) / (2.0 * d2);
            let y = (a2 / d2 - x * x).max(0.0).sqrt();
            c.x = a.x + x * dx - y * dy;
            c.y = a.y + x * dy + y * dx;
        }
    } else {
        c.x = a.x + c.r;
        c.y = a.y;
    }
}

fn intersects(a: Circle, b: Circle) -> bool {
    let dr = a.r + b.r - 1e-6;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    dr > 0.0 && dr * dr > dx * dx + dy * dy
}

/// Packs `circles` around (0, 0) and returns the enclosing radius (d3 `packSiblingsRandom`).
fn pack_siblings(circles: &mut [Circle], random: &mut Lcg) -> f64 {
    let n = circles.len();
    if n == 0 {
        return 0.0;
    }
    circles[0].x = 0.0;
    circles[0].y = 0.0;
    if n == 1 {
        return circles[0].r;
    }
    circles[0].x = -circles[1].r;
    circles[1].x = circles[0].r;
    circles[1].y = 0.0;
    if n == 2 {
        return circles[0].r + circles[1].r;
    }
    let (first, second) = (circles[0], circles[1]);
    place(second, first, &mut circles[2]);
    // The front chain as a circular doubly linked list over circle positions.
    let mut next = vec![0; n];
    let mut previous = vec![0; n];
    let (mut a, mut b, c) = (0, 1, 2);
    next[a] = b;
    previous[c] = b;
    next[b] = c;
    previous[a] = c;
    next[c] = a;
    previous[b] = a;
    let score = |circles: &[Circle], next: &[usize], node: usize| {
        let (a, b) = (circles[node], circles[next[node]]);
        let ab = a.r + b.r;
        let dx = (a.x * b.r + b.x * a.r) / ab;
        let dy = (a.y * b.r + b.y * a.r) / ab;
        dx * dx + dy * dy
    };
    let mut i = 3;
    'pack: while i < n {
        let (left, right) = (circles[a], circles[b]);
        place(left, right, &mut circles[i]);
        let c = i;
        let (mut j, mut k) = (next[b], previous[a]);
        let (mut sj, mut sk) = (circles[b].r, circles[a].r);
        loop {
            if sj <= sk {
                if intersects(circles[j], circles[c]) {
                    b = j;
                    next[a] = b;
                    previous[b] = a;
                    continue 'pack;
                }
                sj += circles[j].r;
                j = next[j];
            } else {
                if intersects(circles[k], circles[c]) {
                    a = k;
                    next[a] = b;
                    previous[b] = a;
                    continue 'pack;
                }
                sk += circles[k].r;
                k = previous[k];
            }
            if j == next[k] {
                break;
            }
        }
        previous[c] = a;
        next[c] = b;
        next[a] = c;
        previous[b] = c;
        b = c;
        let mut best = score(circles, &next, a);
        let mut node = next[c];
        while node != b {
            let candidate = score(circles, &next, node);
            if candidate < best {
                a = node;
                best = candidate;
            }
            node = next[node];
        }
        b = next[a];
        i += 1;
    }
    let mut chain = vec![circles[b]];
    let mut node = next[b];
    while node != b {
        chain.push(circles[node]);
        node = next[node];
    }
    let enclosing = enclose(chain, random);
    for circle in circles.iter_mut() {
        circle.x -= enclosing.x;
        circle.y -= enclosing.y;
    }
    enclosing.r
}

fn encloses_not(a: Circle, b: Circle) -> bool {
    let dr = a.r - b.r;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    dr < 0.0 || dr * dr < dx * dx + dy * dy
}

fn encloses_weak(a: Circle, b: Circle) -> bool {
    let dr = a.r - b.r + a.r.max(b.r).max(1.0) * 1e-9;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    dr > 0.0 && dr * dr > dx * dx + dy * dy
}

fn encloses_weak_all(a: Circle, basis: &[Circle]) -> bool {
    basis.iter().all(|&b| encloses_weak(a, b))
}

fn enclose_basis(basis: &[Circle]) -> Circle {
    match *basis {
        [a] => a,
        [a, b] => enclose_basis2(a, b),
        [a, b, c] => enclose_basis3(a, b, c),
        _ => unreachable!("an enclosing basis has one to three circles"),
    }
}

fn enclose_basis2(a: Circle, b: Circle) -> Circle {
    let (x21, y21, r21) = (b.x - a.x, b.y - a.y, b.r - a.r);
    let l = (x21 * x21 + y21 * y21).sqrt();
    Circle {
        x: (a.x + b.x + x21 / l * r21) / 2.0,
        y: (a.y + b.y + y21 / l * r21) / 2.0,
        r: (l + a.r + b.r) / 2.0,
    }
}

fn enclose_basis3(a: Circle, b: Circle, c: Circle) -> Circle {
    let (x1, y1, r1) = (a.x, a.y, a.r);
    let (a2, a3) = (x1 - b.x, x1 - c.x);
    let (b2, b3) = (y1 - b.y, y1 - c.y);
    let (c2, c3) = (b.r - r1, c.r - r1);
    let d1 = x1 * x1 + y1 * y1 - r1 * r1;
    let d2 = d1 - b.x * b.x - b.y * b.y + b.r * b.r;
    let d3 = d1 - c.x * c.x - c.y * c.y + c.r * c.r;
    let ab = a3 * b2 - a2 * b3;
    let xa = (b2 * d3 - b3 * d2) / (ab * 2.0) - x1;
    let xb = (b3 * c2 - b2 * c3) / ab;
    let ya = (a3 * d2 - a2 * d3) / (ab * 2.0) - y1;
    let yb = (a2 * c3 - a3 * c2) / ab;
    let qa = xb * xb + yb * yb - 1.0;
    let qb = 2.0 * (r1 + xa * xb + ya * yb);
    let qc = xa * xa + ya * ya - r1 * r1;
    let r = -(if qa.abs() > 1e-6 {
        (qb + (qb * qb - 4.0 * qa * qc).sqrt()) / (2.0 * qa)
    } else {
        qc / qb
    });
    Circle {
        x: x1 + xa + xb * r,
        y: y1 + ya + yb * r,
        r,
    }
}

fn extend_basis(basis: &[Circle], p: Circle) -> Option<Vec<Circle>> {
    if encloses_weak_all(p, basis) {
        return Some(vec![p]);
    }
    for &b in basis {
        if encloses_not(p, b) && encloses_weak_all(enclose_basis2(b, p), basis) {
            return Some(vec![b, p]);
        }
    }
    for i in 0..basis.len().saturating_sub(1) {
        for j in i + 1..basis.len() {
            let (bi, bj) = (basis[i], basis[j]);
            if encloses_not(enclose_basis2(bi, bj), p)
                && encloses_not(enclose_basis2(bi, p), bj)
                && encloses_not(enclose_basis2(bj, p), bi)
                && encloses_weak_all(enclose_basis3(bi, bj, p), basis)
            {
                return Some(vec![bi, bj, p]);
            }
        }
    }
    None
}

/// The smallest circle enclosing `circles` (d3 `packEncloseRandom`). Rounding can, in
/// principle, keep the basis search from converging; after a bounded number of restarts the
/// result falls back to a circle around the bounding box that still encloses every circle.
fn enclose(mut circles: Vec<Circle>, random: &mut Lcg) -> Circle {
    let mut m = circles.len();
    while m > 0 {
        let i = (random.next() * m as f64) as usize;
        m -= 1;
        circles.swap(m, i);
    }
    let n = circles.len();
    let mut basis: Vec<Circle> = Vec::new();
    let mut enclosing: Option<Circle> = None;
    let mut restarts = 0usize;
    let mut i = 0;
    while i < n {
        let p = circles[i];
        if enclosing.is_some_and(|e| encloses_weak(e, p)) {
            i += 1;
            continue;
        }
        restarts += 1;
        let Some(next) = extend_basis(&basis, p).filter(|_| restarts <= 4 * n + 64) else {
            return bounding_circle(&circles);
        };
        basis = next;
        enclosing = Some(enclose_basis(&basis));
        i = 0;
    }
    enclosing.unwrap_or(Circle {
        x: 0.0,
        y: 0.0,
        r: 0.0,
    })
}

fn bounding_circle(circles: &[Circle]) -> Circle {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for c in circles {
        min_x = min_x.min(c.x - c.r);
        min_y = min_y.min(c.y - c.r);
        max_x = max_x.max(c.x + c.r);
        max_y = max_y.max(c.y + c.r);
    }
    let (x, y) = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    let r = circles
        .iter()
        .map(|c| ((c.x - x).hypot(c.y - y)) + c.r)
        .fold(0.0, f64::max);
    Circle { x, y, r }
}

#[derive(Clone, Copy, Debug)]
struct PackShape {
    row: usize,
    x: f64,
    y: f64,
    r: f64,
    leaf: bool,
}

impl PackShape {
    fn contains(self, x: f64, y: f64) -> bool {
        self.r > 0.0 && (x - self.x) * (x - self.x) + (y - self.y) * (y - self.y) <= self.r * self.r
    }
}

pub(super) struct PackCache {
    plot: GeneralPlotRect,
    /// Circles in depth-first pre-order: parents before children.
    shapes: Vec<PackShape>,
    /// Index into `shapes` of each row.
    index: Vec<Option<usize>>,
}

pub(super) fn cache_reservation(nodes: usize) -> usize {
    2 * nodes
        * (std::mem::size_of::<PackShape>()
            + std::mem::size_of::<Option<usize>>()
            + std::mem::size_of::<Circle>()
            + std::mem::size_of::<Vec<usize>>()
            + 3 * std::mem::size_of::<usize>())
}

impl PackCache {
    fn new(nodes: &[HierarchyNode], plot: GeneralPlotRect) -> Self {
        let mut cache = Self {
            plot,
            shapes: Vec::new(),
            index: Vec::new(),
        };
        if !(plot.width > 0.0 && plot.height > 0.0) {
            return cache;
        }
        let Some((root, children, weights)) = hierarchy_layout_tree(nodes) else {
            return cache;
        };
        let mut circles: Vec<Circle> = weights
            .iter()
            .zip(&children)
            .map(|(&weight, kids)| Circle {
                x: 0.0,
                y: 0.0,
                r: if kids.is_empty() {
                    weight.max(0.0).sqrt()
                } else {
                    0.0
                },
            })
            .collect();
        let mut random = Lcg(1);
        let size = plot.width.min(plot.height);
        // Without padding, to learn the root radius that converts padding to layout units.
        pack_tree(root, &children, &mut circles, 0.0, &mut random);
        let padding = PADDING * circles[root].r / size;
        pack_tree(root, &children, &mut circles, padding, &mut random);
        let root_r = circles[root].r;
        if !(root_r > 0.0 && root_r.is_finite()) {
            return cache;
        }
        let k = size / (2.0 * root_r);
        circles[root].x = plot.x + plot.width * 0.5;
        circles[root].y = plot.y + plot.height * 0.5;
        cache.index = vec![None; nodes.len()];
        cache.shapes.reserve(nodes.len());
        let mut stack = vec![(root, None::<usize>)];
        while let Some((row, parent)) = stack.pop() {
            circles[row].r *= k;
            if let Some(parent) = parent {
                circles[row].x = circles[parent].x + k * circles[row].x;
                circles[row].y = circles[parent].y + k * circles[row].y;
            }
            cache.index[row] = Some(cache.shapes.len());
            cache.shapes.push(PackShape {
                row,
                x: circles[row].x,
                y: circles[row].y,
                r: circles[row].r,
                leaf: children[row].is_empty(),
            });
            for &child in children[row].iter().rev() {
                stack.push((child, Some(row)));
            }
        }
        cache
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.shapes.capacity() * std::mem::size_of::<PackShape>()
            + self.index.capacity() * std::mem::size_of::<Option<usize>>()
    }

    /// The deepest painted circle under the pointer.
    pub(super) fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.shapes
            .iter()
            .rev()
            .find(|shape| shape.contains(x, y))
            .map(|shape| shape.row)
    }

    /// A leaf's center; for a parent, the first point just inside its edge (from 12 o'clock,
    /// clockwise) that no child covers, so the anchor hits the parent itself.
    pub(super) fn anchor(&self, row: usize) -> Option<(f64, f64)> {
        let shape = self.shapes[(*self.index.get(row)?)?];
        if shape.r <= 0.0 {
            return None;
        }
        if shape.leaf {
            return Some((shape.x, shape.y));
        }
        let inset = (PADDING * 0.5).min(shape.r * 0.5);
        (0..32)
            .map(|step| {
                let angle = std::f64::consts::TAU * f64::from(step) / 32.0;
                (
                    shape.x + (shape.r - inset) * angle.sin(),
                    shape.y - (shape.r - inset) * angle.cos(),
                )
            })
            .find(|&(x, y)| self.hit(x, y) == Some(row))
            .or(Some((shape.x, shape.y)))
    }
}

/// d3 `packChildrenRandom` over the whole tree in post-order, children in input order.
fn pack_tree(
    node: usize,
    children: &[Vec<usize>],
    circles: &mut [Circle],
    padding: f64,
    random: &mut Lcg,
) {
    let kids = &children[node];
    if kids.is_empty() {
        return;
    }
    for &child in kids {
        pack_tree(child, children, circles, padding, random);
    }
    let mut siblings: Vec<Circle> = kids
        .iter()
        .map(|&child| {
            let circle = circles[child];
            Circle {
                r: circle.r + padding,
                ..circle
            }
        })
        .collect();
    let enclosing = pack_siblings(&mut siblings, random);
    for (&child, packed) in kids.iter().zip(&siblings) {
        circles[child].x = packed.x;
        circles[child].y = packed.y;
    }
    circles[node].r = enclosing + padding;
}

impl LayoutSeriesEntry {
    pub(super) fn with_packing<R>(
        &self,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        f: impl FnOnce(&PackCache) -> R,
    ) -> R {
        let mut cache = self.packing.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(PackCache::new(nodes, plot));
        }
        f(cache.as_ref().expect("circle packing cache initialized"))
    }
}

fn parent_fill(color: Color) -> Color {
    Color((color.0 & !0xFF) | u32::from(PARENT_ALPHA))
}

impl ChartEngine {
    /// Parents as translucent series-colored discs with a series-colored edge, so nesting
    /// deepens the tint; leaves as opaque discs with their label when it fits inside.
    pub(super) fn build_packing_frame(
        &self,
        series: &LayoutSeriesEntry,
        nodes: &[HierarchyNode],
        plot: GeneralPlotRect,
        (hpr, vpr): (f64, f64),
        out: &mut Vec<Prim>,
    ) {
        series.with_packing(nodes, plot, |cache| {
            let ratio = hpr.min(vpr);
            let layout = &self.options.get().layout;
            for shape in &cache.shapes {
                if shape.r <= 0.0 {
                    continue;
                }
                let (cx, cy) = ((shape.x * hpr) as f32, (shape.y * vpr) as f32);
                let radius = (shape.r * ratio) as f32;
                if !shape.leaf {
                    out.push(Prim::Circle {
                        cx,
                        cy,
                        radius,
                        fill: parent_fill(series.color),
                        stroke_width: ratio as f32,
                        stroke: series.color,
                    });
                    continue;
                }
                out.push(Prim::Circle {
                    cx,
                    cy,
                    radius,
                    fill: series.color,
                    stroke_width: 0.0,
                    stroke: Color::rgba(0, 0, 0, 0),
                });
                let label = &nodes[shape.row].label;
                if label.is_empty() || 2.0 * shape.r < LABEL_SIZE + 4.0 {
                    continue;
                }
                let width =
                    self.measure_text_run(label, LABEL_SIZE, &layout.font_family, 400, false);
                if !width.is_finite() || width + 4.0 > 2.0 * shape.r {
                    continue;
                }
                out.push(Prim::Text {
                    x: cx,
                    y: cy,
                    text: label.clone(),
                    color: series.color.contrast_srgb_over(self.chart_surface_color()),
                    size: (LABEL_SIZE * vpr) as f32,
                    family: layout.font_family.clone(),
                    align: TextAlign::Center,
                    weight: 400,
                    italic: false,
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(x: f64, y: f64, r: f64) -> Circle {
        Circle { x, y, r }
    }

    #[test]
    fn lcg_matches_d3_hierarchy() {
        let mut random = Lcg(1);
        // (1664525 + 1013904223) / 2^32 and the next step, as d3's lcg() yields them.
        assert_eq!(random.next(), 1_015_568_748.0 / 4_294_967_296.0);
        assert_eq!(random.next(), 1_586_005_467.0 / 4_294_967_296.0);
    }

    #[test]
    fn siblings_follow_the_front_chain_placement() {
        // Two unit circles sit side by side; a third is tangent to both above the axis line
        // (d3 places it on the left of the directed pair b → a).
        let mut circles = [
            circle(0.0, 0.0, 1.0),
            circle(0.0, 0.0, 1.0),
            circle(0.0, 0.0, 1.0),
        ];
        let r = pack_siblings(&mut circles, &mut Lcg(1));
        let distance = |a: Circle, b: Circle| (a.x - b.x).hypot(a.y - b.y);
        for (i, j) in [(0, 1), (1, 2), (0, 2)] {
            assert!((distance(circles[i], circles[j]) - 2.0).abs() < 1e-9);
        }
        // The enclosing circle of three mutually tangent unit circles: 1 + 2/sqrt(3).
        assert!((r - (1.0 + 2.0 / 3.0_f64.sqrt())).abs() < 1e-9, "{r}");
        let centroid = circles
            .iter()
            .fold((0.0, 0.0), |(x, y), c| (x + c.x, y + c.y));
        assert!(centroid.0.abs() < 1e-9 && centroid.1.abs() < 1e-9);
        // Many siblings never overlap and stay inside the enclosing circle.
        let mut many: Vec<Circle> = (1..=24)
            .map(|i| circle(0.0, 0.0, f64::from(i).sqrt()))
            .collect();
        let r = pack_siblings(&mut many, &mut Lcg(1));
        for (i, a) in many.iter().enumerate() {
            assert!(a.x.hypot(a.y) + a.r <= r + 1e-6);
            for b in &many[i + 1..] {
                assert!(distance(*a, *b) >= a.r + b.r - 1e-6);
            }
        }
    }

    #[test]
    fn enclosing_circle_is_minimal_for_known_bases() {
        let e = enclose(
            vec![circle(-1.0, 0.0, 1.0), circle(1.0, 0.0, 1.0)],
            &mut Lcg(1),
        );
        assert!((e.x).abs() < 1e-12 && (e.y).abs() < 1e-12 && (e.r - 2.0).abs() < 1e-12);
        // A circle inside another adds nothing.
        let e = enclose(
            vec![circle(0.0, 0.0, 5.0), circle(1.0, 1.0, 1.0)],
            &mut Lcg(1),
        );
        assert_eq!(e, circle(0.0, 0.0, 5.0));
    }
}
