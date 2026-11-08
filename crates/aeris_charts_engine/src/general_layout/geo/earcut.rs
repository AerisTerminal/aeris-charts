//! Polygon triangulation by ear clipping, ported from earcut 3.2.4
//! (https://github.com/mapbox/earcut) without its hole-bridge block index and its `refine`
//! pass: holes bridge to the outer ring by David Eberly's method, ears are tested through a
//! z-order curve index on larger rings, and stalled rings fall back to filtering, local
//! self-intersection curing, and splitting. A step budget bounds the work on degenerate input;
//! a polygon that exhausts it keeps the triangles found so far.
//!
//! ISC License
//!
//! Copyright (c) 2026, Mapbox
//!
//! Permission to use, copy, modify, and/or distribute this software for any purpose
//! with or without fee is hereby granted, provided that the above copyright notice
//! and this permission notice appear in all copies.
//!
//! THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
//! REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
//! FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
//! INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
//! OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
//! TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
//! THIS SOFTWARE.

const NONE: usize = usize::MAX;
/// Split recursion depth; deeper splits keep the triangles found so far.
const MAX_SPLIT_DEPTH: u32 = 64;

struct Node {
    /// Vertex index in the input positions.
    i: u32,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
    z: u32,
    prev_z: usize,
    next_z: usize,
    /// A single-vertex hole that filtering must keep.
    steiner: bool,
}

/// Reusable triangulation scratch: the node arena and the z-order sort buffer.
#[derive(Default)]
pub(super) struct Earcut {
    nodes: Vec<Node>,
    sort: Vec<usize>,
    filtered_out: bool,
    budget: usize,
}

impl Earcut {
    /// Triangulates the polygon whose outer ring is `data[..holes[0]]` and whose holes start
    /// at each `holes` offset, appending vertex-index triples to `triangles`.
    pub(super) fn triangulate(
        &mut self,
        data: &[[f64; 2]],
        holes: &[usize],
        triangles: &mut Vec<u32>,
    ) {
        self.nodes.clear();
        let outer_len = holes.first().copied().unwrap_or(data.len());
        let n = data.len();
        self.budget = n.saturating_mul(n).clamp(1 << 16, 1 << 26);
        let mut outer = self.linked_list(data, 0, outer_len, true);
        if outer == NONE || self.nodes[outer].next == self.nodes[outer].prev {
            return;
        }
        if !holes.is_empty() {
            outer = self.eliminate_holes(data, holes, outer);
        }
        let (mut min_x, mut min_y, mut inv_size) = (0.0, 0.0, 0.0);
        if data.len() > 80 {
            min_x = data[0][0];
            min_y = data[0][1];
            let (mut max_x, mut max_y) = (min_x, min_y);
            for &[x, y] in &data[1..outer_len] {
                min_x = f64::min(min_x, x);
                min_y = f64::min(min_y, y);
                max_x = f64::max(max_x, x);
                max_y = f64::max(max_y, y);
            }
            inv_size = f64::max(max_x - min_x, max_y - min_y);
            inv_size = if inv_size != 0.0 {
                32767.0 / inv_size
            } else {
                0.0
            };
        }
        self.earcut_linked(outer, triangles, (min_x, min_y, inv_size), 0);
    }

    fn node(&self, index: usize) -> &Node {
        &self.nodes[index]
    }

    fn next(&self, index: usize) -> usize {
        self.nodes[index].next
    }

    fn prev(&self, index: usize) -> usize {
        self.nodes[index].prev
    }

    fn area(&self, p: usize, q: usize, r: usize) -> f64 {
        let (p, q, r) = (self.node(p), self.node(q), self.node(r));
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    fn equals(&self, a: usize, b: usize) -> bool {
        let (a, b) = (self.node(a), self.node(b));
        a.x == b.x && a.y == b.y
    }

    fn create(&mut self, i: u32, x: f64, y: f64) -> usize {
        self.nodes.push(Node {
            i,
            x,
            y,
            prev: NONE,
            next: NONE,
            z: 0,
            prev_z: NONE,
            next_z: NONE,
            steiner: false,
        });
        self.nodes.len() - 1
    }

    fn insert(&mut self, i: u32, [x, y]: [f64; 2], last: usize) -> usize {
        let p = self.create(i, x, y);
        if last == NONE {
            self.nodes[p].prev = p;
            self.nodes[p].next = p;
        } else {
            let next = self.next(last);
            self.nodes[p].next = next;
            self.nodes[p].prev = last;
            self.nodes[next].prev = p;
            self.nodes[last].next = p;
        }
        p
    }

    fn remove(&mut self, p: usize) {
        let Node {
            prev,
            next,
            prev_z,
            next_z,
            ..
        } = self.nodes[p];
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
        if prev_z != NONE {
            self.nodes[prev_z].next_z = next_z;
        }
        if next_z != NONE {
            self.nodes[next_z].prev_z = prev_z;
        }
    }

    fn linked_list(
        &mut self,
        data: &[[f64; 2]],
        start: usize,
        end: usize,
        clockwise: bool,
    ) -> usize {
        let mut last = NONE;
        if clockwise == (signed_area(&data[start..end]) > 0.0) {
            for (index, &point) in data.iter().enumerate().take(end).skip(start) {
                last = self.insert(index as u32, point, last);
            }
        } else {
            for index in (start..end).rev() {
                last = self.insert(index as u32, data[index], last);
            }
        }
        if last != NONE && self.equals(last, self.next(last)) {
            self.remove(last);
            last = self.next(last);
        }
        last
    }

    /// earcut `filterPoints`: removes collinear or coincident points from `start` through
    /// `end` (the whole ring when `end` is `None`) and returns a live node.
    fn filter_points(&mut self, start: usize, end: Option<usize>) -> usize {
        let full = end.is_none();
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let next = self.next(p);
            if p != next
                && !self.nodes[p].steiner
                && (self.equals(p, next) || self.area(self.prev(p), p, next) == 0.0)
            {
                if full || p == end {
                    end = self.prev(p);
                }
                self.filtered_out = true;
                self.remove(p);
                p = self.prev(p);
                again = true;
            } else if full || p != end {
                p = next;
                again = !full;
            }
            if !(again || p != end) {
                return end;
            }
        }
    }

    fn earcut_linked(
        &mut self,
        mut ear: usize,
        triangles: &mut Vec<u32>,
        hash: (f64, f64, f64),
        depth: u32,
    ) {
        if hash.2 != 0.0 {
            self.index_curve(ear, hash);
        }
        let mut stop = ear;
        let mut cured = false;
        while self.prev(ear) != self.next(ear) {
            if self.budget == 0 {
                return;
            }
            self.budget -= 1;
            let prev = self.prev(ear);
            let next = self.next(ear);
            if self.area(prev, ear, next) < 0.0
                && if hash.2 != 0.0 {
                    self.is_ear_hashed(ear, hash)
                } else {
                    self.is_ear(ear)
                }
            {
                triangles.extend([self.nodes[prev].i, self.nodes[ear].i, self.nodes[next].i]);
                self.remove(ear);
                ear = next;
                stop = next;
                continue;
            }
            ear = next;
            if ear == stop {
                self.filtered_out = false;
                ear = self.filter_points(ear, None);
                if self.filtered_out {
                    stop = ear;
                    continue;
                }
                if !cured {
                    ear = self.cure_local_intersections(ear, triangles);
                    stop = ear;
                    cured = true;
                    continue;
                }
                self.split_earcut(ear, triangles, hash, depth);
                break;
            }
        }
    }

    fn triangle_bounds(&self, ear: usize) -> ([usize; 3], [f64; 4]) {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        let (pa, pb, pc) = (self.node(a), self.node(b), self.node(c));
        (
            [a, b, c],
            [
                pa.x.min(pb.x).min(pc.x),
                pa.y.min(pb.y).min(pc.y),
                pa.x.max(pb.x).max(pc.x),
                pa.y.max(pb.y).max(pc.y),
            ],
        )
    }

    /// Whether `p` blocks the ear `a`-`b`-`c` with bounding box `bounds`.
    fn blocks(&self, [a, b, c]: [usize; 3], bounds: [f64; 4], p: usize) -> bool {
        let (pa, pb, pc, pp) = (self.node(a), self.node(b), self.node(c), self.node(p));
        pp.x >= bounds[0]
            && pp.x <= bounds[2]
            && pp.y >= bounds[1]
            && pp.y <= bounds[3]
            && !(pa.x == pp.x && pa.y == pp.y)
            && point_in_triangle((pa.x, pa.y), (pb.x, pb.y), (pc.x, pc.y), (pp.x, pp.y))
            && self.area(self.prev(p), p, self.next(p)) >= 0.0
    }

    fn is_ear(&mut self, ear: usize) -> bool {
        let (corners, bounds) = self.triangle_bounds(ear);
        let mut p = self.next(corners[2]);
        while p != corners[0] {
            if self.budget == 0 {
                return false;
            }
            self.budget -= 1;
            if self.blocks(corners, bounds, p) {
                return false;
            }
            p = self.next(p);
        }
        true
    }

    fn is_ear_hashed(&mut self, ear: usize, (min_x, min_y, inv_size): (f64, f64, f64)) -> bool {
        let (corners, bounds) = self.triangle_bounds(ear);
        let min_z = z_order(bounds[0], bounds[1], min_x, min_y, inv_size);
        let max_z = z_order(bounds[2], bounds[3], min_x, min_y, inv_size);
        let mut p = self.nodes[ear].prev_z;
        while p != NONE && self.nodes[p].z >= min_z {
            if self.budget == 0 {
                return false;
            }
            self.budget -= 1;
            if p != corners[2] && self.blocks(corners, bounds, p) {
                return false;
            }
            p = self.nodes[p].prev_z;
        }
        let mut n = self.nodes[ear].next_z;
        while n != NONE && self.nodes[n].z <= max_z {
            if self.budget == 0 {
                return false;
            }
            self.budget -= 1;
            if n != corners[2] && self.blocks(corners, bounds, n) {
                return false;
            }
            n = self.nodes[n].next_z;
        }
        true
    }

    fn cure_local_intersections(&mut self, start: usize, triangles: &mut Vec<u32>) -> usize {
        let mut start = start;
        let mut p = start;
        let mut cured = false;
        loop {
            let a = self.prev(p);
            let b = self.next(self.next(p));
            if self.intersects(a, p, self.next(p), b, false)
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                triangles.extend([self.nodes[a].i, self.nodes[p].i, self.nodes[b].i]);
                let next = self.next(p);
                self.remove(p);
                self.remove(next);
                p = b;
                start = b;
                cured = true;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        if cured {
            self.filter_points(p, None)
        } else {
            p
        }
    }

    fn split_earcut(
        &mut self,
        start: usize,
        triangles: &mut Vec<u32>,
        hash: (f64, f64, f64),
        depth: u32,
    ) {
        if depth >= MAX_SPLIT_DEPTH {
            return;
        }
        let mut a = start;
        loop {
            let mut b = self.next(self.next(a));
            while b != self.prev(a) {
                if self.budget == 0 {
                    return;
                }
                self.budget -= 1;
                if self.nodes[a].i != self.nodes[b].i && self.is_valid_diagonal(a, b) {
                    let c = self.split_polygon(a, b);
                    let a = self.filter_points(a, Some(self.next(a)));
                    let c = self.filter_points(c, Some(self.next(c)));
                    self.earcut_linked(a, triangles, hash, depth + 1);
                    self.earcut_linked(c, triangles, hash, depth + 1);
                    return;
                }
                b = self.next(b);
            }
            a = self.next(a);
            if a == start {
                return;
            }
        }
    }

    fn eliminate_holes(&mut self, data: &[[f64; 2]], holes: &[usize], mut outer: usize) -> usize {
        let mut queue = Vec::with_capacity(holes.len());
        for (index, &start) in holes.iter().enumerate() {
            let end = holes.get(index + 1).copied().unwrap_or(data.len());
            let list = self.linked_list(data, start, end, false);
            if list == NONE {
                continue;
            }
            if list == self.next(list) {
                self.nodes[list].steiner = true;
            }
            queue.push(self.leftmost(list));
        }
        // earcut's compareXYSlope: holes meeting at their leftmost vertex sort
        // counterclockwise so the shared vertex becomes the bridge.
        let slope = |earcut: &Self, node: usize| {
            let (p, n) = (earcut.node(node), earcut.node(earcut.next(node)));
            (n.y - p.y) / (n.x - p.x)
        };
        queue.sort_by(|&a, &b| {
            let (pa, pb) = (self.node(a), self.node(b));
            (pa.x + 0.0)
                .total_cmp(&(pb.x + 0.0))
                .then((pa.y + 0.0).total_cmp(&(pb.y + 0.0)))
                .then(slope(self, a).total_cmp(&slope(self, b)))
        });
        for hole in queue {
            outer = self.eliminate_hole(hole, outer);
        }
        self.filter_points(outer, None)
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let Some(bridge) = self.find_hole_bridge(hole, outer) else {
            return outer;
        };
        let bridge_reverse = self.split_polygon(bridge, hole);
        self.filter_points(bridge_reverse, Some(self.next(bridge_reverse)));
        self.filter_points(bridge, Some(self.next(bridge)))
    }

    /// David Eberly's bridge from a hole's leftmost vertex to the outer ring.
    fn find_hole_bridge(&mut self, hole: usize, outer: usize) -> Option<usize> {
        let (hx, hy) = (self.nodes[hole].x, self.nodes[hole].y);
        let mut qx = f64::NEG_INFINITY;
        let mut m = NONE;
        if self.equals(hole, outer) {
            return Some(outer);
        }
        let mut p = outer;
        loop {
            let next = self.next(p);
            if self.equals(hole, next) {
                return Some(next);
            }
            let (pp, pn) = (self.node(p), self.node(next));
            if hy <= pp.y && hy >= pn.y && pn.y != pp.y {
                let x = pp.x + (hy - pp.y) * (pn.x - pp.x) / (pn.y - pp.y);
                if x <= hx && x > qx {
                    qx = x;
                    m = if pp.x < pn.x { p } else { next };
                    if x == hx {
                        return Some(m);
                    }
                }
            }
            p = next;
            if p == outer {
                break;
            }
        }
        if m == NONE {
            return None;
        }
        let stop = m;
        let (mx, my) = (self.nodes[m].x, self.nodes[m].y);
        let mut tan_min = f64::INFINITY;
        p = m;
        loop {
            if self.budget == 0 {
                break;
            }
            self.budget -= 1;
            let (px, py) = (self.nodes[p].x, self.nodes[p].y);
            if hx >= px
                && px >= mx
                && hx != px
                && point_in_triangle(
                    (if hy < my { hx } else { qx }, hy),
                    (mx, my),
                    (if hy < my { qx } else { hx }, hy),
                    (px, py),
                )
            {
                let tan = (hy - py).abs() / (hx - px);
                let next = self.node(self.next(p));
                let touches = py == hy && next.y == hy && next.x > hx;
                if (self.locally_inside(p, hole) || touches)
                    && (tan < tan_min
                        || (tan == tan_min
                            && (px > self.nodes[m].x
                                || (px == self.nodes[m].x && self.sector_contains_sector(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = self.next(p);
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    fn sector_contains_sector(&self, m: usize, p: usize) -> bool {
        self.area(self.prev(m), m, self.prev(p)) < 0.0
            && self.area(self.next(p), m, self.next(m)) < 0.0
    }

    fn index_curve(&mut self, start: usize, (min_x, min_y, inv_size): (f64, f64, f64)) {
        self.sort.clear();
        let mut p = start;
        loop {
            let node = &mut self.nodes[p];
            node.z = z_order(node.x, node.y, min_x, min_y, inv_size);
            self.sort.push(p);
            p = self.nodes[p].next;
            if p == start {
                break;
            }
        }
        let nodes = &self.nodes;
        self.sort.sort_by_key(|&index| nodes[index].z);
        let mut prev = NONE;
        for position in 0..self.sort.len() {
            let node = self.sort[position];
            self.nodes[node].prev_z = prev;
            if prev != NONE {
                self.nodes[prev].next_z = node;
            }
            prev = node;
        }
        if prev != NONE {
            self.nodes[prev].next_z = NONE;
        }
    }

    fn leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut leftmost = start;
        loop {
            let (node, best) = (self.node(p), self.node(leftmost));
            if node.x < best.x || (node.x == best.x && node.y < best.y) {
                leftmost = p;
            }
            p = self.next(p);
            if p == start {
                return leftmost;
            }
        }
    }

    fn is_valid_diagonal(&mut self, a: usize, b: usize) -> bool {
        let zero_length = self.equals(a, b)
            && self.area(self.prev(a), a, self.next(a)) > 0.0
            && self.area(self.prev(b), b, self.next(b)) > 0.0;
        self.nodes[self.next(a)].i != self.nodes[b].i
            && (zero_length
                || (self.locally_inside(a, b)
                    && self.locally_inside(b, a)
                    && (self.area(a, self.prev(b), b) != 0.0
                        || self.area(self.prev(a), a, self.prev(b)) != 0.0)))
            && !self.intersects_polygon(a, b)
            && (zero_length || self.middle_inside(a, b))
    }

    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize, boundary: bool) -> bool {
        let o1 = self.area(p1, q1, p2);
        let o2 = self.area(p1, q1, q2);
        let o3 = self.area(p2, q2, p1);
        let o4 = self.area(p2, q2, q1);
        if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
            && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
        {
            return true;
        }
        if !boundary {
            return false;
        }
        (o1 == 0.0 && self.on_segment(p1, p2, q1))
            || (o2 == 0.0 && self.on_segment(p1, q2, q1))
            || (o3 == 0.0 && self.on_segment(p2, p1, q2))
            || (o4 == 0.0 && self.on_segment(p2, q1, q2))
    }

    fn on_segment(&self, p: usize, q: usize, r: usize) -> bool {
        let (p, q, r) = (self.node(p), self.node(q), self.node(r));
        q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
    }

    fn intersects_polygon(&mut self, a: usize, b: usize) -> bool {
        let (pa, pb) = (self.node(a), self.node(b));
        let (min_x, max_x) = (pa.x.min(pb.x), pa.x.max(pb.x));
        let (min_y, max_y) = (pa.y.min(pb.y), pa.y.max(pb.y));
        let (ia, ib) = (pa.i, pb.i);
        let mut p = a;
        loop {
            self.budget = self.budget.saturating_sub(1);
            let n = self.next(p);
            let (pp, pn) = (self.node(p), self.node(n));
            let outside = (pp.x > max_x && pn.x > max_x)
                || (pp.x < min_x && pn.x < min_x)
                || (pp.y > max_y && pn.y > max_y)
                || (pp.y < min_y && pn.y < min_y);
            if !outside
                && pp.i != ia
                && pn.i != ia
                && pp.i != ib
                && pn.i != ib
                && self.intersects(p, n, a, b, true)
            {
                return true;
            }
            p = n;
            if p == a {
                return false;
            }
        }
    }

    fn locally_inside(&self, a: usize, b: usize) -> bool {
        if self.area(self.prev(a), a, self.next(a)) < 0.0 {
            self.area(a, b, self.next(a)) >= 0.0 && self.area(a, self.prev(a), b) >= 0.0
        } else {
            self.area(a, b, self.prev(a)) < 0.0 || self.area(a, self.next(a), b) < 0.0
        }
    }

    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let (pa, pb) = (self.node(a), self.node(b));
        let px = (pa.x + pb.x) / 2.0;
        let py = (pa.y + pb.y) / 2.0;
        let mut inside = false;
        let mut p = a;
        loop {
            let n = self.next(p);
            let (pp, pn) = (self.node(p), self.node(n));
            if (pp.y > py) != (pn.y > py) && px < (pn.x - pp.x) * (py - pp.y) / (pn.y - pp.y) + pp.x
            {
                inside = !inside;
            }
            p = n;
            if p == a {
                return inside;
            }
        }
    }

    /// Links `a` and `b` with a bridge: splits one ring in two, or merges a hole into the
    /// outer ring. Returns the copy of `b`.
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let (ai, ax, ay) = (self.nodes[a].i, self.nodes[a].x, self.nodes[a].y);
        let (bi, bx, by) = (self.nodes[b].i, self.nodes[b].x, self.nodes[b].y);
        let a2 = self.create(ai, ax, ay);
        let b2 = self.create(bi, bx, by);
        let an = self.next(a);
        let bp = self.prev(b);
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[a2].next = an;
        self.nodes[an].prev = a2;
        self.nodes[b2].next = a2;
        self.nodes[a2].prev = b2;
        self.nodes[bp].next = b2;
        self.nodes[b2].prev = bp;
        b2
    }
}

/// earcut `signedArea` (the shoelace sum with earcut's sign).
fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let Some(&last) = ring.last() else {
        return 0.0;
    };
    let mut previous = last;
    let mut sum = 0.0;
    for &point in ring {
        sum += (previous[0] - point[0]) * (point[1] + previous[1]);
        previous = point;
    }
    sum
}

fn point_in_triangle(a: (f64, f64), b: (f64, f64), c: (f64, f64), p: (f64, f64)) -> bool {
    (c.0 - p.0) * (a.1 - p.1) >= (a.0 - p.0) * (c.1 - p.1)
        && (a.0 - p.0) * (b.1 - p.1) >= (b.0 - p.0) * (a.1 - p.1)
        && (b.0 - p.0) * (c.1 - p.1) >= (c.0 - p.0) * (b.1 - p.1)
}

/// The z-order (Morton) key of a point on the 15-bit grid over the outer ring's bounds.
fn z_order(x: f64, y: f64, min_x: f64, min_y: f64, inv_size: f64) -> u32 {
    let spread = |value: f64| {
        let mut v = (value as i32) as u32;
        v = (v | (v << 8)) & 0x00FF_00FF;
        v = (v | (v << 4)) & 0x0F0F_0F0F;
        v = (v | (v << 2)) & 0x3333_3333;
        (v | (v << 1)) & 0x5555_5555
    };
    spread((x - min_x) * inv_size) | (spread((y - min_y) * inv_size) << 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shoelace(ring: &[[f64; 2]]) -> f64 {
        signed_area(ring).abs() / 2.0
    }

    fn mesh_area(data: &[[f64; 2]], triangles: &[u32]) -> f64 {
        triangles
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = [
                    data[t[0] as usize],
                    data[t[1] as usize],
                    data[t[2] as usize],
                ];
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn earcut_square_matches_reference_indices() {
        // earcut 3.2.4 under Node returns [1,0,3, 1,3,2] for the README input
        // [10,0, 0,50, 60,60, 70,10]; the README comment still shows the 2.x order.
        let data = [[10.0, 0.0], [0.0, 50.0], [60.0, 60.0], [70.0, 10.0]];
        let mut triangles = Vec::new();
        Earcut::default().triangulate(&data, &[], &mut triangles);
        assert_eq!(triangles, [1, 0, 3, 1, 3, 2]);
    }

    #[test]
    fn triangulated_area_equals_shoelace_area_with_holes() {
        let outer = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let hole_a = [[10.0, 10.0], [10.0, 40.0], [40.0, 40.0], [40.0, 10.0]];
        let hole_b = [[60.0, 60.0], [60.0, 90.0], [90.0, 75.0]];
        let mut data = outer.to_vec();
        data.extend(hole_a);
        data.extend(hole_b);
        let mut triangles = Vec::new();
        Earcut::default().triangulate(&data, &[4, 8], &mut triangles);
        let want = shoelace(&outer) - shoelace(&hole_a) - shoelace(&hole_b);
        let got = mesh_area(&data, &triangles);
        assert!((got - want).abs() <= 1e-9 * want, "{got} vs {want}");
    }

    #[test]
    fn large_ring_uses_the_z_order_index_and_keeps_area() {
        // 400-gon star with alternating radii: concave, above the 80-coordinate hash threshold.
        let data: Vec<[f64; 2]> = (0..400)
            .map(|index| {
                let angle = index as f64 * std::f64::consts::TAU / 400.0;
                let radius = if index % 2 == 0 { 100.0 } else { 70.0 };
                [radius * angle.cos(), radius * angle.sin()]
            })
            .collect();
        let mut triangles = Vec::new();
        Earcut::default().triangulate(&data, &[], &mut triangles);
        assert_eq!(triangles.len(), 3 * 398);
        let want = shoelace(&data);
        assert!((mesh_area(&data, &triangles) - want).abs() <= 1e-9 * want);
    }

    #[test]
    fn degenerate_rings_terminate_without_triangles() {
        let mut earcut = Earcut::default();
        for data in [
            vec![],
            vec![[1.0, 1.0]],
            vec![[0.0, 0.0], [1.0, 1.0]],
            vec![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]],
            vec![[5.0, 5.0]; 6],
        ] {
            let mut triangles = Vec::new();
            earcut.triangulate(&data, &[], &mut triangles);
            assert!(mesh_area(&data, &triangles) <= 1e-12, "{data:?}");
        }
    }

    #[test]
    fn self_intersecting_bowtie_terminates() {
        let data = [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        let mut triangles = Vec::new();
        Earcut::default().triangulate(&data, &[], &mut triangles);
        assert!(triangles.len() % 3 == 0);
    }
}
