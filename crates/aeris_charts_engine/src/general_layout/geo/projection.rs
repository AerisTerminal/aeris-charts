//! Spherical projection pipeline ported from d3-geo 3.1.1: raw projections, rotation, antimeridian
//! and hemisphere clipping, adaptive resampling, the Mercator clip extent, and `fitExtent`.
//! Stream semantics follow d3-geo `projection.stream` so projected rings, bounds, and fitted
//! scales match d3 within floating-point rounding.
//!
//! Ported from d3-geo (https://github.com/d3/d3-geo), used under the ISC license:
//!
//! Copyright 2010-2024 Mike Bostock
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

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use super::GeoProjectionKind;

const EPSILON: f64 = 1e-6;
const EPSILON2: f64 = 1e-12;
const RADIANS: f64 = PI / 180.0;
/// d3 `projection.precision` default `sqrt(0.5)`: resampling stops within about 0.7 px.
const DELTA2: f64 = 0.5;
const MAX_DEPTH: u32 = 16;
/// `cos(30°)`: segments longer than 30° of arc always subdivide.
const COS_MIN_DISTANCE: f64 = 0.866_025_403_784_438_7;
/// d3 `geoOrthographic().clipAngle(90 + epsilon)`, in radians.
const ORTHOGRAPHIC_CLIP: f64 = (90.0 + EPSILON) * RADIANS;
/// d3 circle interpolation step of 2°.
const CIRCLE_STEP: f64 = 2.0 * RADIANS;

pub(super) type P2 = (f64, f64);
/// A buffered clip point with d3's intersection marker (`point[2]`).
type P3 = (f64, f64, u8);

fn asin(x: f64) -> f64 {
    if x > 1.0 {
        FRAC_PI_2
    } else if x < -1.0 {
        -FRAC_PI_2
    } else {
        x.asin()
    }
}

fn acos(x: f64) -> f64 {
    if x > 1.0 {
        0.0
    } else if x < -1.0 {
        PI
    } else {
        x.acos()
    }
}

fn cartesian((lambda, phi): P2) -> [f64; 3] {
    let cos_phi = phi.cos();
    [cos_phi * lambda.cos(), cos_phi * lambda.sin(), phi.sin()]
}

fn spherical(c: [f64; 3]) -> P2 {
    (c[1].atan2(c[0]), asin(c[2]))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(d: &mut [f64; 3]) {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    d[0] /= l;
    d[1] /= l;
    d[2] /= l;
}

fn point_equal(a: P2, b: P2) -> bool {
    (a.0 - b.0).abs() < EPSILON && (a.1 - b.1).abs() < EPSILON
}

/// d3-array `Adder`: Shewchuk's exact floating-point summation.
#[derive(Default)]
struct Adder {
    partials: Vec<f64>,
}

impl Adder {
    fn add(&mut self, mut x: f64) {
        let mut i = 0;
        for j in 0..self.partials.len().min(32) {
            let y = self.partials[j];
            let hi = x + y;
            let lo = if x.abs() < y.abs() {
                x - (hi - y)
            } else {
                y - (hi - x)
            };
            if lo != 0.0 {
                self.partials[i] = lo;
                i += 1;
            }
            x = hi;
        }
        self.partials.truncate(i);
        self.partials.push(x);
    }

    fn value(&self) -> f64 {
        let p = &self.partials;
        let mut n = p.len();
        let mut hi = 0.0;
        if n > 0 {
            n -= 1;
            hi = p[n];
            let mut lo = 0.0;
            while n > 0 {
                let x = hi;
                n -= 1;
                let y = p[n];
                hi = x + y;
                lo = y - (hi - x);
                if lo != 0.0 {
                    break;
                }
            }
            if n > 0 && ((lo < 0.0 && p[n - 1] < 0.0) || (lo > 0.0 && p[n - 1] > 0.0)) {
                let y = lo * 2.0;
                let x = hi + y;
                if y == x - hi {
                    hi = x;
                }
            }
        }
        hi
    }
}

/// d3 `rotateRadians`: a longitude shift, then the φ/γ rotation.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rotation {
    lambda: f64,
    phi_gamma: Option<[f64; 4]>,
}

fn wrap_lambda(mut lambda: f64) -> f64 {
    if lambda.abs() > PI {
        lambda -= (lambda / TAU).round() * TAU;
    }
    lambda
}

impl Rotation {
    fn new(delta_lambda: f64, delta_phi: f64, delta_gamma: f64) -> Self {
        Self {
            lambda: delta_lambda % TAU,
            phi_gamma: (delta_phi != 0.0 || delta_gamma != 0.0).then(|| {
                [
                    delta_phi.cos(),
                    delta_phi.sin(),
                    delta_gamma.cos(),
                    delta_gamma.sin(),
                ]
            }),
        }
    }

    pub(super) fn apply(&self, lambda: f64, phi: f64) -> P2 {
        let lambda = wrap_lambda(lambda + self.lambda);
        let Some([cos_dp, sin_dp, cos_dg, sin_dg]) = self.phi_gamma else {
            return (lambda, phi);
        };
        let cos_phi = phi.cos();
        let x = lambda.cos() * cos_phi;
        let y = lambda.sin() * cos_phi;
        let z = phi.sin();
        let k = z * cos_dp + x * sin_dp;
        (
            (y * cos_dg - k * sin_dg).atan2(x * cos_dp - z * sin_dp),
            asin(k * cos_dg + y * sin_dg),
        )
    }

    fn invert(&self, lambda: f64, phi: f64) -> P2 {
        let (lambda, phi) = match self.phi_gamma {
            Some([cos_dp, sin_dp, cos_dg, sin_dg]) => {
                let cos_phi = phi.cos();
                let x = lambda.cos() * cos_phi;
                let y = lambda.sin() * cos_phi;
                let z = phi.sin();
                let k = z * cos_dg - y * sin_dg;
                (
                    (y * cos_dg + z * sin_dg).atan2(x * cos_dp + k * sin_dp),
                    asin(k * cos_dp - x * sin_dp),
                )
            }
            None => (lambda, phi),
        };
        (wrap_lambda(lambda - self.lambda), phi)
    }
}

#[derive(Clone, Copy, Debug)]
enum Raw {
    Mercator,
    EqualEarth,
    NaturalEarth1,
    Orthographic,
    Conic { n: f64, c: f64, r0: f64 },
    Cylindrical { cos_phi0: f64 },
    Identity,
}

impl Raw {
    fn conic(phi0: f64, phi1: f64) -> Self {
        let sy0 = phi0.sin();
        let n = (sy0 + phi1.sin()) / 2.0;
        if n.abs() < EPSILON {
            return Self::Cylindrical {
                cos_phi0: phi0.cos(),
            };
        }
        let c = 1.0 + sy0 * (2.0 * n - sy0);
        Self::Conic {
            n,
            c,
            r0: c.sqrt() / n,
        }
    }

    fn project(self, lambda: f64, phi: f64) -> P2 {
        match self {
            Self::Mercator => (lambda, ((FRAC_PI_2 + phi) / 2.0).tan().ln()),
            Self::EqualEarth => {
                const A1: f64 = 1.340264;
                const A2: f64 = -0.081106;
                const A3: f64 = 0.000893;
                const A4: f64 = 0.003796;
                let m = 3.0_f64.sqrt() / 2.0;
                let l = asin(m * phi.sin());
                let l2 = l * l;
                let l6 = l2 * l2 * l2;
                (
                    lambda * l.cos() / (m * (A1 + 3.0 * A2 * l2 + l6 * (7.0 * A3 + 9.0 * A4 * l2))),
                    l * (A1 + A2 * l2 + l6 * (A3 + A4 * l2)),
                )
            }
            Self::NaturalEarth1 => {
                let phi2 = phi * phi;
                let phi4 = phi2 * phi2;
                (
                    lambda
                        * (0.8707 - 0.131979 * phi2
                            + phi4 * (-0.013791 + phi4 * (0.003971 * phi2 - 0.001529 * phi4))),
                    phi * (1.007226
                        + phi2
                            * (0.015085 + phi4 * (-0.044475 + 0.028874 * phi2 - 0.005916 * phi4))),
                )
            }
            Self::Orthographic => (phi.cos() * lambda.sin(), phi.sin()),
            Self::Conic { n, c, r0 } => {
                let r = (c - 2.0 * n * phi.sin()).sqrt() / n;
                let x = lambda * n;
                (r * x.sin(), r0 - r * x.cos())
            }
            Self::Cylindrical { cos_phi0 } => (lambda * cos_phi0, phi.sin() / cos_phi0),
            Self::Identity => (lambda, phi),
        }
    }
}

/// One configured projection: d3 `projectionMutator` state (scale, translate, center, rotate)
/// or, for [`GeoProjectionKind::Identity`], d3 `geoIdentity().reflectY(true)`.
#[derive(Clone, Debug)]
pub(super) struct Projection {
    raw: Raw,
    k: f64,
    x: f64,
    y: f64,
    center: P2,
    rotate: [f64; 3],
    dx: f64,
    dy: f64,
    rotation: Rotation,
    /// The automatic Mercator clip extent `[x0, y0, x1, y1]`.
    extent: Option<[f64; 4]>,
    /// Squared resampling precision in px (d3 `precision` squared).
    delta2: f64,
}

impl Projection {
    /// The d3 defaults of `kind`: its default scale, translate `[480, 250]`, and for the conic
    /// projection d3's `center([0, 33.6442])`.
    pub(super) fn new(kind: GeoProjectionKind, parallels: [f64; 2]) -> Self {
        let (raw, k, center) = match kind {
            GeoProjectionKind::Mercator => (Raw::Mercator, 961.0 / TAU, (0.0, 0.0)),
            GeoProjectionKind::EqualEarth => (Raw::EqualEarth, 177.158, (0.0, 0.0)),
            GeoProjectionKind::NaturalEarth1 => (Raw::NaturalEarth1, 175.295, (0.0, 0.0)),
            GeoProjectionKind::Orthographic => (Raw::Orthographic, 249.5, (0.0, 0.0)),
            GeoProjectionKind::ConicEqualArea => (
                Raw::conic(parallels[0] * RADIANS, parallels[1] * RADIANS),
                155.424,
                (0.0, 33.6442 % 360.0 * RADIANS),
            ),
            GeoProjectionKind::Identity => (Raw::Identity, 1.0, (0.0, 0.0)),
        };
        let mut projection = Self {
            raw,
            k,
            x: 480.0,
            y: 250.0,
            center,
            rotate: [0.0; 3],
            dx: 0.0,
            dy: 0.0,
            rotation: Rotation::new(0.0, 0.0, 0.0),
            extent: None,
            delta2: DELTA2,
        };
        if matches!(raw, Raw::Identity) {
            projection.x = 0.0;
            projection.y = 0.0;
        }
        projection.recenter();
        projection
    }

    fn is_identity(&self) -> bool {
        matches!(self.raw, Raw::Identity)
    }

    #[cfg(test)]
    pub(super) fn scale(&self) -> f64 {
        self.k
    }

    pub(super) fn translate(&self) -> P2 {
        (self.x, self.y)
    }

    #[cfg(test)]
    pub(super) fn set_scale(&mut self, k: f64) {
        self.k = k;
        self.recenter();
    }

    pub(super) fn set_translate(&mut self, x: f64, y: f64) {
        self.x = x;
        self.y = y;
        self.recenter();
    }

    /// d3 `projection.center`, in degrees.
    #[cfg(test)]
    pub(super) fn set_center(&mut self, lon: f64, lat: f64) {
        self.center = (lon % 360.0 * RADIANS, lat % 360.0 * RADIANS);
        self.recenter();
    }

    /// d3 `projection.precision`: resampling stops within `delta` px of the true curve.
    pub(super) fn set_precision(&mut self, delta: f64) {
        self.delta2 = delta * delta;
    }

    /// d3 `projection.rotate`, in degrees.
    pub(super) fn set_rotate(&mut self, rotate: [f64; 3]) {
        self.rotate = rotate.map(|angle| angle % 360.0 * RADIANS);
        self.recenter();
    }

    fn recenter(&mut self) {
        if self.is_identity() {
            return;
        }
        let (cx, cy) = self.raw.project(self.center.0, self.center.1);
        self.dx = self.x - self.k * cx;
        self.dy = self.y + self.k * cy;
        self.rotation = Rotation::new(self.rotate[0], self.rotate[1], self.rotate[2]);
        if matches!(self.raw, Raw::Mercator) {
            let inverse = self.rotation.invert(0.0, 0.0);
            let (lambda, phi) = self.rotation.apply(inverse.0, inverse.1);
            let (tx, ty) = self.project_raw(lambda, phi);
            let k = PI * self.k;
            self.extent = Some([tx - k, ty - k, tx + k, ty + k]);
        }
    }

    /// d3 `projectTransform`: the raw projection of rotated radians, scaled and translated.
    fn project_raw(&self, lambda: f64, phi: f64) -> P2 {
        let (x, y) = self.raw.project(lambda, phi);
        (self.dx + self.k * x, self.dy - self.k * y)
    }

    /// d3 `projection([lon, lat])`; the identity projection maps planar `x`/`y` with a
    /// reflected y axis.
    pub(super) fn forward(&self, lon: f64, lat: f64) -> P2 {
        if self.is_identity() {
            return (lon * self.k + self.x, -lat * self.k + self.y);
        }
        let (lambda, phi) = self.rotation.apply(lon * RADIANS, lat * RADIANS);
        self.project_raw(lambda, phi)
    }

    /// Streams geometry through d3's pipeline: radians, rotation, the antimeridian or hemisphere
    /// clip, adaptive resampling within `budget` extra points, and the Mercator clip extent
    /// intersected with `clip` (d3 `clipExtent`). `feed` emits geometry in degrees.
    pub(super) fn stream(
        &self,
        sink: &mut dyn Stream,
        budget: &mut usize,
        clip: Option<[f64; 4]>,
        feed: impl FnOnce(&mut dyn Stream),
    ) {
        let extent = match (self.extent, clip) {
            (Some(a), Some(b)) => Some([
                a[0].max(b[0]),
                a[1].max(b[1]),
                a[2].min(b[2]).max(a[0].max(b[0])),
                a[3].min(b[3]).max(a[1].max(b[1])),
            ]),
            (extent, None) | (None, extent) => extent,
        };
        let mut rect;
        let post: &mut dyn Stream = match extent {
            Some(extent) => {
                rect = RectClip::new(extent, sink);
                &mut rect
            }
            None => sink,
        };
        if self.is_identity() {
            let mut stage = IdentityStage {
                projection: self,
                next: post,
            };
            feed(&mut stage);
            return;
        }
        let mut resample = Resample::new(self, post, budget);
        let circle = matches!(self.raw, Raw::Orthographic);
        let mut clip = Clip::new(circle, &mut resample);
        let mut rotate = RotateStage {
            rotation: self.rotation,
            next: &mut clip,
        };
        feed(&mut rotate);
    }

    /// d3 `fitExtent`: projects the geometry at scale 150 and translate zero, then scales and
    /// centers its bounds in `extent`. Empty or degenerate bounds leave the projection as is.
    pub(super) fn fit_extent(
        &mut self,
        extent: [f64; 4],
        budget: usize,
        feed: impl FnOnce(&mut dyn Stream),
    ) {
        let (k0, x0, y0) = (self.k, self.x, self.y);
        self.k = 150.0;
        self.x = 0.0;
        self.y = 0.0;
        self.recenter();
        let mut bounds = Bounds::default();
        let mut budget = budget;
        self.stream(&mut bounds, &mut budget, None, feed);
        let [bx0, by0, bx1, by1] = bounds.result();
        let w = extent[2] - extent[0];
        let h = extent[3] - extent[1];
        let k = (w / (bx1 - bx0)).min(h / (by1 - by0));
        if !k.is_finite() || k <= 0.0 {
            self.k = k0;
            self.x = x0;
            self.y = y0;
            self.recenter();
            return;
        }
        let x = extent[0] + (w - k * (bx1 + bx0)) / 2.0;
        let y = extent[1] + (h - k * (by1 + by0)) / 2.0;
        self.k = 150.0 * k;
        self.x = x;
        self.y = y;
        self.recenter();
    }
}

/// d3 projection stream events.
pub(super) trait Stream {
    fn point(&mut self, x: f64, y: f64, m: u8);
    fn line_start(&mut self);
    fn line_end(&mut self);
    fn polygon_start(&mut self);
    fn polygon_end(&mut self);
}

/// d3 `path/bounds`.
pub(super) struct Bounds([f64; 4]);

impl Default for Bounds {
    fn default() -> Self {
        Self([
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ])
    }
}

impl Bounds {
    pub(super) fn result(&self) -> [f64; 4] {
        self.0
    }
}

impl Stream for Bounds {
    fn point(&mut self, x: f64, y: f64, _m: u8) {
        let b = &mut self.0;
        if x < b[0] {
            b[0] = x;
        }
        if x > b[2] {
            b[2] = x;
        }
        if y < b[1] {
            b[1] = y;
        }
        if y > b[3] {
            b[3] = y;
        }
    }
    fn line_start(&mut self) {}
    fn line_end(&mut self) {}
    fn polygon_start(&mut self) {}
    fn polygon_end(&mut self) {}
}

#[derive(Clone, Copy, Default, PartialEq)]
enum CentroidMode {
    #[default]
    Point,
    LineFirst,
    Line,
    RingFirst,
    Ring,
}

/// d3 `geoCentroid`: the spherical centroid of streamed geometry in degrees, weighted by
/// area for polygons, by length for lines, and evenly for points. Polygons must arrive in
/// d3 winding (clockwise exteriors).
#[derive(Default)]
pub(super) struct Centroid {
    w0: f64,
    w1: f64,
    s0: [f64; 3],
    s1: [f64; 3],
    s2: [Adder; 3],
    mode: CentroidMode,
    in_polygon: bool,
    previous: [f64; 3],
    ring_first: P2,
}

fn cartesian_degrees(lon: f64, lat: f64) -> [f64; 3] {
    cartesian((lon * RADIANS, lat * RADIANS))
}

impl Centroid {
    fn point_cartesian(&mut self, c: [f64; 3]) {
        self.w0 += 1.0;
        for (sum, value) in self.s0.iter_mut().zip(c) {
            *sum += (value - *sum) / self.w0;
        }
    }

    fn line_point(&mut self, c: [f64; 3]) {
        let p = self.previous;
        let w = dot(cross(p, c), cross(p, c)).sqrt().atan2(dot(p, c));
        self.add_weighted(w, c);
    }

    fn ring_point(&mut self, c: [f64; 3]) {
        let p = self.previous;
        let k = cross(p, c);
        let m = dot(k, k).sqrt();
        let w = asin(m);
        let v = if m == 0.0 { 0.0 } else { -w / m };
        for (sum, value) in self.s2.iter_mut().zip(k) {
            sum.add(v * value);
        }
        self.add_weighted(w, c);
    }

    fn add_weighted(&mut self, w: f64, c: [f64; 3]) {
        self.w1 += w;
        for ((sum, previous), value) in self.s1.iter_mut().zip(self.previous).zip(c) {
            *sum += w * (previous + value);
        }
        self.previous = c;
        self.point_cartesian(c);
    }

    /// The centroid in degrees, or `None` for empty or antipodally balanced geometry.
    pub(super) fn result(&self) -> Option<[f64; 2]> {
        let mut c = [self.s2[0].value(), self.s2[1].value(), self.s2[2].value()];
        let mut m = dot(c, c).sqrt();
        if m < EPSILON2 {
            c = if self.w1 < EPSILON { self.s0 } else { self.s1 };
            m = dot(c, c).sqrt();
            if m < EPSILON2 {
                return None;
            }
        }
        Some([c[1].atan2(c[0]) / RADIANS, asin(c[2] / m) / RADIANS])
    }
}

impl Stream for Centroid {
    fn point(&mut self, lon: f64, lat: f64, _m: u8) {
        let c = cartesian_degrees(lon, lat);
        match self.mode {
            CentroidMode::Point => self.point_cartesian(c),
            CentroidMode::LineFirst | CentroidMode::RingFirst => {
                if self.mode == CentroidMode::RingFirst {
                    self.ring_first = (lon, lat);
                    self.mode = CentroidMode::Ring;
                } else {
                    self.mode = CentroidMode::Line;
                }
                self.previous = c;
                self.point_cartesian(c);
            }
            CentroidMode::Line => self.line_point(c),
            CentroidMode::Ring => self.ring_point(c),
        }
    }
    fn line_start(&mut self) {
        self.mode = if self.in_polygon {
            CentroidMode::RingFirst
        } else {
            CentroidMode::LineFirst
        };
    }
    fn line_end(&mut self) {
        if self.mode == CentroidMode::Ring {
            let (lon, lat) = self.ring_first;
            self.ring_point(cartesian_degrees(lon, lat));
        }
        self.mode = CentroidMode::Point;
    }
    fn polygon_start(&mut self) {
        self.in_polygon = true;
    }
    fn polygon_end(&mut self) {
        self.in_polygon = false;
    }
}

struct IdentityStage<'a> {
    projection: &'a Projection,
    next: &'a mut dyn Stream,
}

impl Stream for IdentityStage<'_> {
    fn point(&mut self, x: f64, y: f64, m: u8) {
        let (x, y) = self.projection.forward(x, y);
        self.next.point(x, y, m);
    }
    fn line_start(&mut self) {
        self.next.line_start();
    }
    fn line_end(&mut self) {
        self.next.line_end();
    }
    fn polygon_start(&mut self) {
        self.next.polygon_start();
    }
    fn polygon_end(&mut self) {
        self.next.polygon_end();
    }
}

/// d3 `transformRadians` then `transformRotate`.
struct RotateStage<'a> {
    rotation: Rotation,
    next: &'a mut dyn Stream,
}

impl Stream for RotateStage<'_> {
    fn point(&mut self, x: f64, y: f64, _m: u8) {
        let (lambda, phi) = self.rotation.apply(x * RADIANS, y * RADIANS);
        self.next.point(lambda, phi, 0);
    }
    fn line_start(&mut self) {
        self.next.line_start();
    }
    fn line_end(&mut self) {
        self.next.line_end();
    }
    fn polygon_start(&mut self) {
        self.next.polygon_start();
    }
    fn polygon_end(&mut self) {
        self.next.polygon_end();
    }
}

/// d3 `clip/buffer`.
#[derive(Default)]
struct ClipBuffer {
    lines: Vec<Vec<P3>>,
}

impl ClipBuffer {
    fn rejoin(&mut self) {
        if self.lines.len() > 1 {
            let mut last = self.lines.pop().expect("two buffered lines");
            let first = self.lines.remove(0);
            last.extend(first);
            self.lines.push(last);
        }
    }
}

impl Stream for ClipBuffer {
    fn point(&mut self, x: f64, y: f64, m: u8) {
        if let Some(line) = self.lines.last_mut() {
            line.push((x, y, m));
        }
    }
    fn line_start(&mut self) {
        self.lines.push(Vec::new());
    }
    fn line_end(&mut self) {}
    fn polygon_start(&mut self) {}
    fn polygon_end(&mut self) {}
}

/// d3 `polygonContains`: whether the spherical polygon contains `point`.
fn polygon_contains(polygon: &[Vec<P2>], point: P2) -> bool {
    fn longitude(lambda: f64) -> f64 {
        if lambda.abs() <= PI {
            lambda
        } else {
            lambda.signum() * ((lambda.abs() + PI) % TAU - PI)
        }
    }
    let lambda = longitude(point.0);
    let mut phi = point.1;
    let sin_phi = phi.sin();
    let normal = [lambda.sin(), -lambda.cos(), 0.0];
    let mut angle = 0.0;
    let mut winding: i32 = 0;
    let mut sum = Adder::default();
    if sin_phi == 1.0 {
        phi = FRAC_PI_2 + EPSILON;
    } else if sin_phi == -1.0 {
        phi = -FRAC_PI_2 - EPSILON;
    }
    for ring in polygon {
        let Some(&last) = ring.last() else {
            continue;
        };
        let mut point0 = last;
        let mut lambda0 = longitude(point0.0);
        let phi0 = point0.1 / 2.0 + FRAC_PI_4;
        let mut sin_phi0 = phi0.sin();
        let mut cos_phi0 = phi0.cos();
        for &point1 in ring {
            let lambda1 = longitude(point1.0);
            let phi1 = point1.1 / 2.0 + FRAC_PI_4;
            let sin_phi1 = phi1.sin();
            let cos_phi1 = phi1.cos();
            let delta = lambda1 - lambda0;
            let sign = if delta >= 0.0 { 1.0 } else { -1.0 };
            let abs_delta = sign * delta;
            let antimeridian = abs_delta > PI;
            let k = sin_phi0 * sin_phi1;
            sum.add((k * sign * abs_delta.sin()).atan2(cos_phi0 * cos_phi1 + k * abs_delta.cos()));
            angle += if antimeridian {
                delta + sign * TAU
            } else {
                delta
            };
            if antimeridian ^ (lambda0 >= lambda) ^ (lambda1 >= lambda) {
                let mut arc = cross(cartesian(point0), cartesian(point1));
                normalize(&mut arc);
                let mut intersection = cross(normal, arc);
                normalize(&mut intersection);
                let phi_arc = if antimeridian ^ (delta >= 0.0) {
                    -1.0
                } else {
                    1.0
                } * asin(intersection[2]);
                if phi > phi_arc || (phi == phi_arc && (arc[0] != 0.0 || arc[1] != 0.0)) {
                    winding += if antimeridian ^ (delta >= 0.0) { 1 } else { -1 };
                }
            }
            lambda0 = lambda1;
            sin_phi0 = sin_phi1;
            cos_phi0 = cos_phi1;
            point0 = point1;
        }
    }
    (angle < -EPSILON || (angle < EPSILON && sum.value() < -EPSILON2)) ^ (winding & 1 != 0)
}

struct Intersection {
    x: P3,
    /// Index of the subject segment, for subject entries.
    z: Option<usize>,
    o: usize,
    e: bool,
    v: bool,
    n: usize,
    p: usize,
}

type Interpolate<'a> = dyn FnMut(Option<P3>, Option<P3>, f64, &mut dyn Stream) + 'a;

/// d3 `clip/rejoin`: joins the visible segments of a clipped polygon along the clip edge.
fn rejoin(
    segments: &mut [Vec<P3>],
    compare: &dyn Fn(P3, P3) -> f64,
    mut start_inside: bool,
    interpolate: &mut Interpolate<'_>,
    stream: &mut dyn Stream,
) {
    let mut nodes: Vec<Intersection> = Vec::new();
    let mut subject: Vec<usize> = Vec::new();
    let mut clip: Vec<usize> = Vec::new();
    let push = |nodes: &mut Vec<Intersection>, x: P3, z: Option<usize>, e: bool| {
        nodes.push(Intersection {
            x,
            z,
            o: 0,
            e,
            v: false,
            n: 0,
            p: 0,
        });
        nodes.len() - 1
    };
    for (index, segment) in segments.iter_mut().enumerate() {
        if segment.len() < 2 {
            continue;
        }
        let n = segment.len() - 1;
        let p0 = segment[0];
        let p1 = segment[n];
        if point_equal((p0.0, p0.1), (p1.0, p1.1)) {
            if p0.2 == 0 && p1.2 == 0 {
                stream.line_start();
                for point in &segment[..n] {
                    stream.point(point.0, point.1, 0);
                }
                stream.line_end();
                continue;
            }
            segment[n].0 += 2.0 * EPSILON;
        }
        let p1 = segment[n];
        let a = push(&mut nodes, p0, Some(index), true);
        let b = push(&mut nodes, p0, None, false);
        nodes[a].o = b;
        nodes[b].o = a;
        subject.push(a);
        clip.push(b);
        let a = push(&mut nodes, p1, Some(index), false);
        let b = push(&mut nodes, p1, None, true);
        nodes[a].o = b;
        nodes[b].o = a;
        subject.push(a);
        clip.push(b);
    }
    if subject.is_empty() {
        return;
    }
    clip.sort_by(|&a, &b| {
        compare(nodes[a].x, nodes[b].x)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for list in [&subject, &clip] {
        let count = list.len();
        for (position, &node) in list.iter().enumerate() {
            nodes[node].n = list[(position + 1) % count];
            nodes[node].p = list[(position + count - 1) % count];
        }
    }
    for &node in &clip {
        start_inside = !start_inside;
        nodes[node].e = start_inside;
    }
    let start = subject[0];
    loop {
        let mut current = start;
        let mut is_subject = true;
        while nodes[current].v {
            current = nodes[current].n;
            if current == start {
                return;
            }
        }
        let mut points = nodes[current].z;
        stream.line_start();
        loop {
            nodes[current].v = true;
            let other = nodes[current].o;
            nodes[other].v = true;
            if nodes[current].e {
                if is_subject {
                    if let Some(segment) = points {
                        for point in &segments[segment] {
                            stream.point(point.0, point.1, 0);
                        }
                    }
                } else {
                    let next = nodes[current].n;
                    interpolate(Some(nodes[current].x), Some(nodes[next].x), 1.0, stream);
                }
                current = nodes[current].n;
            } else {
                if is_subject {
                    points = nodes[nodes[current].p].z;
                    if let Some(segment) = points {
                        for point in segments[segment].iter().rev() {
                            stream.point(point.0, point.1, 0);
                        }
                    }
                } else {
                    let previous = nodes[current].p;
                    interpolate(
                        Some(nodes[current].x),
                        Some(nodes[previous].x),
                        -1.0,
                        stream,
                    );
                }
                current = nodes[current].p;
            }
            current = nodes[current].o;
            points = nodes[current].z;
            is_subject = !is_subject;
            if nodes[current].v {
                break;
            }
        }
        stream.line_end();
    }
}

/// Line clipper state of d3 `clipAntimeridianLine` or the hemisphere `clipLine` of
/// `clip/circle` (a 90° + ε radius, so d3's small-circle double-intersection branch, which
/// only runs when the radius is not a hemisphere, never applies).
enum LineClip {
    Antimeridian {
        lambda0: f64,
        phi0: f64,
        sign0: f64,
        clean: u8,
    },
    Circle {
        point0: Option<P2>,
        v0: bool,
        v00: bool,
        clean: u8,
    },
}

/// Hemisphere clip constants: `cos(radius)` of the 90° + ε clip circle.
fn circle_cr() -> f64 {
    ORTHOGRAPHIC_CLIP.cos()
}

fn circle_visible(lambda: f64, phi: f64) -> bool {
    lambda.cos() * phi.cos() > circle_cr()
}

/// d3 `clip/circle` `intersect(a, b)` for one crossing of the clip circle.
fn circle_intersect(a: P2, b: P2) -> Option<P2> {
    let cr = circle_cr();
    let pa = cartesian(a);
    let pb = cartesian(b);
    let n1 = [1.0, 0.0, 0.0];
    let n2 = cross(pa, pb);
    let n2n2 = dot(n2, n2);
    let n1n2 = n2[0];
    let determinant = n2n2 - n1n2 * n1n2;
    if determinant == 0.0 {
        return Some(a);
    }
    let c1 = cr * n2n2 / determinant;
    let c2 = -cr * n1n2 / determinant;
    let u = cross(n1, n2);
    let a3 = [n1[0] * c1 + n2[0] * c2, n2[1] * c2, n2[2] * c2];
    let w = dot(a3, u);
    let uu = dot(u, u);
    let t2 = w * w - uu * (dot(a3, a3) - 1.0);
    if t2 < 0.0 {
        return None;
    }
    let t = t2.sqrt();
    let s = (-w - t) / uu;
    Some(spherical([
        u[0] * s + a3[0],
        u[1] * s + a3[1],
        u[2] * s + a3[2],
    ]))
}

fn antimeridian_intersect(lambda0: f64, phi0: f64, lambda1: f64, phi1: f64) -> f64 {
    let sin_lambda0_lambda1 = (lambda0 - lambda1).sin();
    if sin_lambda0_lambda1.abs() > EPSILON {
        let cos_phi1 = phi1.cos();
        let cos_phi0 = phi0.cos();
        ((phi0.sin() * cos_phi1 * lambda1.sin() - phi1.sin() * cos_phi0 * lambda0.sin())
            / (cos_phi0 * cos_phi1 * sin_lambda0_lambda1))
            .atan()
    } else {
        (phi0 + phi1) / 2.0
    }
}

impl LineClip {
    fn new(circle: bool) -> Self {
        if circle {
            Self::Circle {
                point0: None,
                v0: false,
                v00: false,
                clean: 0,
            }
        } else {
            Self::Antimeridian {
                lambda0: f64::NAN,
                phi0: f64::NAN,
                sign0: f64::NAN,
                clean: 0,
            }
        }
    }

    fn line_start(&mut self, out: &mut dyn Stream) {
        match self {
            Self::Antimeridian { clean, .. } => {
                out.line_start();
                *clean = 1;
            }
            Self::Circle { v0, v00, clean, .. } => {
                *v00 = false;
                *v0 = false;
                *clean = 1;
            }
        }
    }

    fn point(&mut self, lambda: f64, phi: f64, out: &mut dyn Stream) {
        match self {
            Self::Antimeridian {
                lambda0,
                phi0,
                sign0,
                clean,
            } => {
                let mut lambda1 = lambda;
                let sign1 = if lambda1 > 0.0 { PI } else { -PI };
                let delta = (lambda1 - *lambda0).abs();
                if (delta - PI).abs() < EPSILON {
                    *phi0 = if (*phi0 + phi) / 2.0 > 0.0 {
                        FRAC_PI_2
                    } else {
                        -FRAC_PI_2
                    };
                    out.point(*lambda0, *phi0, 0);
                    out.point(*sign0, *phi0, 0);
                    out.line_end();
                    out.line_start();
                    out.point(sign1, *phi0, 0);
                    out.point(lambda1, *phi0, 0);
                    *clean = 0;
                } else if *sign0 != sign1 && delta >= PI {
                    if (*lambda0 - *sign0).abs() < EPSILON {
                        *lambda0 -= *sign0 * EPSILON;
                    }
                    if (lambda1 - sign1).abs() < EPSILON {
                        lambda1 -= sign1 * EPSILON;
                    }
                    *phi0 = antimeridian_intersect(*lambda0, *phi0, lambda1, phi);
                    out.point(*sign0, *phi0, 0);
                    out.line_end();
                    out.line_start();
                    out.point(sign1, *phi0, 0);
                    *clean = 0;
                }
                *lambda0 = lambda1;
                *phi0 = phi;
                out.point(lambda1, phi, 0);
                *sign0 = sign1;
            }
            Self::Circle {
                point0,
                v0,
                v00,
                clean,
            } => {
                let point1 = (lambda, phi);
                let v = circle_visible(lambda, phi);
                if point0.is_none() {
                    *v00 = v;
                    *v0 = v;
                    if v {
                        out.line_start();
                    }
                }
                if v != *v0 {
                    *clean = 0;
                    let previous = point0.unwrap_or(point1);
                    if v {
                        out.line_start();
                        let point2 = circle_intersect(point1, previous).unwrap_or(point1);
                        out.point(point2.0, point2.1, 0);
                        *point0 = Some(point2);
                    } else {
                        let point2 = circle_intersect(previous, point1).unwrap_or(previous);
                        out.point(point2.0, point2.1, 2);
                        out.line_end();
                        *point0 = Some(point2);
                    }
                }
                if v && point0.is_none_or(|previous| !point_equal(previous, point1)) {
                    out.point(point1.0, point1.1, 0);
                }
                *point0 = Some(point1);
                *v0 = v;
            }
        }
    }

    fn line_end(&mut self, out: &mut dyn Stream) {
        match self {
            Self::Antimeridian { lambda0, phi0, .. } => {
                out.line_end();
                *lambda0 = f64::NAN;
                *phi0 = f64::NAN;
            }
            Self::Circle { point0, v0, .. } => {
                if *v0 {
                    out.line_end();
                }
                *point0 = None;
            }
        }
    }

    /// d3 `clean()`: bit 1 means no intersections, bit 2 that the first and last segments join.
    fn clean(&self) -> u8 {
        match self {
            Self::Antimeridian { clean, .. } => 2 - *clean,
            Self::Circle { v0, v00, clean, .. } => *clean | (u8::from(*v00 && *v0) << 1),
        }
    }
}

/// d3 `circleStream` for the hemisphere clip edge.
fn circle_interpolate(from: Option<P3>, to: Option<P3>, direction: f64, out: &mut dyn Stream) {
    let radius = ORTHOGRAPHIC_CLIP;
    let cos_radius = radius.cos();
    let sin_radius = radius.sin();
    let step = direction * CIRCLE_STEP;
    let circle_radius = |point: P3| {
        let mut p = cartesian((point.0, point.1));
        p[0] -= cos_radius;
        normalize(&mut p);
        let r = acos(-p[1]);
        ((if -p[2] < 0.0 { -r } else { r }) + TAU - EPSILON) % TAU
    };
    let (mut t0, t1) = match (from, to) {
        (Some(from), Some(to)) => {
            let mut t0 = circle_radius(from);
            let t1 = circle_radius(to);
            if if direction > 0.0 { t0 < t1 } else { t0 > t1 } {
                t0 += direction * TAU;
            }
            (t0, t1)
        }
        _ => (radius + direction * TAU, radius - step / 2.0),
    };
    while if direction > 0.0 { t0 > t1 } else { t0 < t1 } {
        let (lambda, phi) = spherical([cos_radius, -sin_radius * t0.cos(), -sin_radius * t0.sin()]);
        out.point(lambda, phi, 0);
        t0 -= step;
    }
}

/// d3 `clipAntimeridianInterpolate`.
fn antimeridian_interpolate(
    from: Option<P3>,
    to: Option<P3>,
    direction: f64,
    out: &mut dyn Stream,
) {
    match (from, to) {
        (Some(from), Some(to)) if (from.0 - to.0).abs() > EPSILON => {
            let lambda = if from.0 < to.0 { PI } else { -PI };
            let phi = direction * lambda / 2.0;
            out.point(-lambda, phi, 0);
            out.point(0.0, phi, 0);
            out.point(lambda, phi, 0);
        }
        (Some(_), Some(to)) => out.point(to.0, to.1, 0),
        _ => {
            let phi = direction * FRAC_PI_2;
            for (lambda, phi) in [
                (-PI, phi),
                (0.0, phi),
                (PI, phi),
                (PI, 0.0),
                (PI, -phi),
                (0.0, -phi),
                (-PI, -phi),
                (-PI, 0.0),
                (-PI, phi),
            ] {
                out.point(lambda, phi, 0);
            }
        }
    }
}

/// d3 `compareIntersection` of `clip/index`, shared by both spherical clips.
fn compare_intersection(a: P3, b: P3) -> f64 {
    let key = |p: P3| {
        if p.0 < 0.0 {
            p.1 - FRAC_PI_2 - EPSILON
        } else {
            FRAC_PI_2 - p.1
        }
    };
    key(a) - key(b)
}

/// d3 `clip/index` over the antimeridian or hemisphere line clipper.
struct Clip<'a> {
    circle: bool,
    next: &'a mut dyn Stream,
    line: LineClip,
    ring_line: LineClip,
    ring_buffer: ClipBuffer,
    polygon_started: bool,
    in_polygon: bool,
    in_line: bool,
    polygon: Vec<Vec<P2>>,
    segments: Vec<Vec<P3>>,
    ring: Vec<P2>,
}

impl<'a> Clip<'a> {
    fn new(circle: bool, next: &'a mut dyn Stream) -> Self {
        Self {
            circle,
            next,
            line: LineClip::new(circle),
            ring_line: LineClip::new(circle),
            ring_buffer: ClipBuffer::default(),
            polygon_started: false,
            in_polygon: false,
            in_line: false,
            polygon: Vec::new(),
            segments: Vec::new(),
            ring: Vec::new(),
        }
    }

    fn start(&self) -> P2 {
        if self.circle {
            (-PI, ORTHOGRAPHIC_CLIP - PI)
        } else {
            (-PI, -FRAC_PI_2)
        }
    }

    fn interpolate(&self) -> fn(Option<P3>, Option<P3>, f64, &mut dyn Stream) {
        if self.circle {
            circle_interpolate
        } else {
            antimeridian_interpolate
        }
    }

    fn ensure_polygon(&mut self) {
        if !self.polygon_started {
            self.next.polygon_start();
            self.polygon_started = true;
        }
    }

    fn ring_end(&mut self) {
        let Some(&first) = self.ring.first() else {
            self.ring_line.line_end(&mut self.ring_buffer);
            self.ring_buffer.lines.clear();
            return;
        };
        self.ring.push(first);
        self.ring_line
            .point(first.0, first.1, &mut self.ring_buffer);
        self.ring_line.line_end(&mut self.ring_buffer);
        let clean = self.ring_line.clean();
        let mut ring_segments = std::mem::take(&mut self.ring_buffer.lines);
        self.ring.pop();
        self.polygon.push(std::mem::take(&mut self.ring));
        if ring_segments.is_empty() {
            return;
        }
        if clean & 1 != 0 {
            let segment = &ring_segments[0];
            if segment.len() > 1 {
                self.ensure_polygon();
                self.next.line_start();
                for point in &segment[..segment.len() - 1] {
                    self.next.point(point.0, point.1, 0);
                }
                self.next.line_end();
            }
            return;
        }
        if ring_segments.len() > 1 && clean & 2 != 0 {
            let mut last = ring_segments.pop().expect("two ring segments");
            let first = ring_segments.remove(0);
            last.extend(first);
            ring_segments.push(last);
        }
        self.segments.extend(
            ring_segments
                .into_iter()
                .filter(|segment| segment.len() > 1),
        );
    }
}

impl Stream for Clip<'_> {
    fn point(&mut self, lambda: f64, phi: f64, _m: u8) {
        if self.in_polygon {
            self.ring.push((lambda, phi));
            self.ring_line.point(lambda, phi, &mut self.ring_buffer);
        } else if self.in_line {
            self.line.point(lambda, phi, self.next);
        } else if !self.circle || circle_visible(lambda, phi) {
            self.next.point(lambda, phi, 0);
        }
    }

    fn line_start(&mut self) {
        if self.in_polygon {
            self.ring_line.line_start(&mut self.ring_buffer);
            self.ring.clear();
        } else {
            self.in_line = true;
            self.line.line_start(self.next);
        }
    }

    fn line_end(&mut self) {
        if self.in_polygon {
            self.ring_end();
        } else {
            self.in_line = false;
            self.line.line_end(self.next);
        }
    }

    fn polygon_start(&mut self) {
        self.in_polygon = true;
        self.segments.clear();
        self.polygon.clear();
    }

    fn polygon_end(&mut self) {
        self.in_polygon = false;
        let start_inside = polygon_contains(&self.polygon, self.start());
        let interpolate = self.interpolate();
        if !self.segments.is_empty() {
            self.ensure_polygon();
            let mut segments = std::mem::take(&mut self.segments);
            rejoin(
                &mut segments,
                &compare_intersection,
                start_inside,
                &mut |from, to, direction, out| interpolate(from, to, direction, out),
                self.next,
            );
        } else if start_inside {
            self.ensure_polygon();
            self.next.line_start();
            interpolate(None, None, 1.0, self.next);
            self.next.line_end();
        }
        if self.polygon_started {
            self.next.polygon_end();
            self.polygon_started = false;
        }
        self.segments.clear();
        self.polygon.clear();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ResampleMode {
    Point,
    Line,
    RingFirst,
}

/// d3 `projection/resample`; `budget` bounds the subdivision points one stream may add.
struct Resample<'a> {
    projection: &'a Projection,
    next: &'a mut dyn Stream,
    budget: &'a mut usize,
    mode: ResampleMode,
    in_polygon: bool,
    ring_open: bool,
    first: (f64, f64, f64, [f64; 3]),
    last: (f64, f64, f64, [f64; 3]),
}

impl<'a> Resample<'a> {
    fn new(projection: &'a Projection, next: &'a mut dyn Stream, budget: &'a mut usize) -> Self {
        let empty = (f64::NAN, f64::NAN, f64::NAN, [f64::NAN; 3]);
        Self {
            projection,
            next,
            budget,
            mode: ResampleMode::Point,
            in_polygon: false,
            ring_open: false,
            first: empty,
            last: empty,
        }
    }

    /// d3 `resampleLineTo` between projected `(x, y)` points with spherical `lambda` and
    /// cartesian `c`.
    fn line_to(
        &mut self,
        (x0, y0, lambda0, c0): (f64, f64, f64, [f64; 3]),
        (x1, y1, lambda1, c1): (f64, f64, f64, [f64; 3]),
        depth: u32,
    ) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let d2 = dx * dx + dy * dy;
        let delta2 = self.projection.delta2;
        if !(d2 > 4.0 * delta2 && depth > 0 && *self.budget > 0) {
            return;
        }
        let depth = depth - 1;
        let a = c0[0] + c1[0];
        let b = c0[1] + c1[1];
        let c = c0[2] + c1[2];
        let m = (a * a + b * b + c * c).sqrt();
        let c = c / m;
        let phi2 = asin(c);
        let lambda2 = if (c.abs() - 1.0).abs() < EPSILON || (lambda0 - lambda1).abs() < EPSILON {
            (lambda0 + lambda1) / 2.0
        } else {
            b.atan2(a)
        };
        let (x2, y2) = self.projection.project_raw(lambda2, phi2);
        let dx2 = x2 - x0;
        let dy2 = y2 - y0;
        let dz = dy * dx2 - dx * dy2;
        if dz * dz / d2 > delta2
            || ((dx * dx2 + dy * dy2) / d2 - 0.5).abs() > 0.3
            || c0[0] * c1[0] + c0[1] * c1[1] + c0[2] * c1[2] < COS_MIN_DISTANCE
        {
            let middle = (x2, y2, lambda2, [a / m, b / m, c]);
            *self.budget -= 1;
            self.line_to((x0, y0, lambda0, c0), middle, depth);
            self.next.point(x2, y2, 0);
            self.line_to(middle, (x1, y1, lambda1, c1), depth);
        }
    }

    fn line_point(&mut self, lambda: f64, phi: f64) {
        let c = cartesian((lambda, phi));
        let (x, y) = self.projection.project_raw(lambda, phi);
        let point = (x, y, lambda, c);
        self.line_to(self.last, point, MAX_DEPTH);
        self.last = point;
        self.next.point(x, y, 0);
    }
}

impl Stream for Resample<'_> {
    fn point(&mut self, lambda: f64, phi: f64, _m: u8) {
        match self.mode {
            ResampleMode::Point => {
                let (x, y) = self.projection.project_raw(lambda, phi);
                self.next.point(x, y, 0);
            }
            ResampleMode::Line => self.line_point(lambda, phi),
            ResampleMode::RingFirst => {
                self.line_point(lambda, phi);
                self.first = self.last;
                self.mode = ResampleMode::Line;
            }
        }
    }

    fn line_start(&mut self) {
        self.last.0 = f64::NAN;
        self.mode = ResampleMode::Line;
        self.next.line_start();
        if self.in_polygon {
            self.mode = ResampleMode::RingFirst;
            self.ring_open = true;
        }
    }

    fn line_end(&mut self) {
        if self.ring_open {
            self.line_to(self.last, self.first, MAX_DEPTH);
            self.ring_open = false;
        }
        self.mode = ResampleMode::Point;
        self.next.line_end();
    }

    fn polygon_start(&mut self) {
        self.next.polygon_start();
        self.in_polygon = true;
    }

    fn polygon_end(&mut self) {
        self.next.polygon_end();
        self.in_polygon = false;
    }
}

/// d3 `clip/rectangle` (the Mercator clip extent).
struct RectClip<'a> {
    extent: [f64; 4],
    next: &'a mut dyn Stream,
    buffer: ClipBuffer,
    buffering: bool,
    in_line: bool,
    segments: Vec<Vec<P3>>,
    polygon: Option<Vec<Vec<P2>>>,
    first_point: (f64, f64, bool),
    previous: (f64, f64, bool),
    first: bool,
    clean: bool,
}

const CLIP_MAX: f64 = 1e9;

/// d3 `clip/line`: Liang-Barsky clipping of segment `a`-`b` to the extent.
pub(super) fn clip_segment(a: &mut P2, b: &mut P2, [x0, y0, x1, y1]: [f64; 4]) -> bool {
    let (ax, ay) = *a;
    let (bx, by) = *b;
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let dx = bx - ax;
    let dy = by - ay;
    for (edge, delta, lower) in [
        (x0 - ax, dx, true),
        (x1 - ax, dx, false),
        (y0 - ay, dy, true),
        (y1 - ay, dy, false),
    ] {
        if delta == 0.0 && (if lower { edge > 0.0 } else { edge < 0.0 }) {
            return false;
        }
        let r = edge / delta;
        let entering = if lower { delta > 0.0 } else { delta < 0.0 };
        let leaving = if lower { delta < 0.0 } else { delta > 0.0 };
        if leaving {
            if r < t0 {
                return false;
            }
            if r < t1 {
                t1 = r;
            }
        } else if entering {
            if r > t1 {
                return false;
            }
            if r > t0 {
                t0 = r;
            }
        }
    }
    if t0 > 0.0 {
        *a = (ax + t0 * dx, ay + t0 * dy);
    }
    if t1 < 1.0 {
        *b = (ax + t1 * dx, ay + t1 * dy);
    }
    true
}

impl<'a> RectClip<'a> {
    fn new(extent: [f64; 4], next: &'a mut dyn Stream) -> Self {
        Self {
            extent,
            next,
            buffer: ClipBuffer::default(),
            buffering: false,
            in_line: false,
            segments: Vec::new(),
            polygon: None,
            first_point: (f64::NAN, f64::NAN, false),
            previous: (f64::NAN, f64::NAN, false),
            first: true,
            clean: true,
        }
    }

    fn visible(&self, x: f64, y: f64) -> bool {
        let [x0, y0, x1, y1] = self.extent;
        x0 <= x && x <= x1 && y0 <= y && y <= y1
    }

    fn active(&mut self) -> &mut dyn Stream {
        if self.buffering {
            &mut self.buffer
        } else {
            &mut *self.next
        }
    }

    fn polygon_inside(&self) -> i32 {
        let [x0, _, _, y1] = self.extent;
        let mut winding = 0;
        for ring in self.polygon.iter().flatten() {
            for pair in ring.windows(2) {
                let ((a0, a1), (b0, b1)) = (pair[0], pair[1]);
                if a1 <= y1 {
                    if b1 > y1 && (b0 - a0) * (y1 - a1) > (b1 - a1) * (x0 - a0) {
                        winding += 1;
                    }
                } else if b1 <= y1 && (b0 - a0) * (y1 - a1) < (b1 - a1) * (x0 - a0) {
                    winding -= 1;
                }
            }
        }
        winding
    }

    fn line_point(&mut self, x: f64, y: f64) {
        let v = self.visible(x, y);
        if let Some(ring) = self.polygon.as_mut().and_then(|polygon| polygon.last_mut()) {
            ring.push((x, y));
        }
        // d3 keeps the clamped coordinates as the previous point.
        let mut stored = (x, y);
        if self.first {
            self.first_point = (x, y, v);
            self.first = false;
            if v {
                let active = self.active();
                active.line_start();
                active.point(x, y, 0);
            }
        } else {
            let (px, py, pv) = self.previous;
            if v && pv {
                self.active().point(x, y, 0);
            } else {
                let mut a = (px.clamp(-CLIP_MAX, CLIP_MAX), py.clamp(-CLIP_MAX, CLIP_MAX));
                let mut b = (x.clamp(-CLIP_MAX, CLIP_MAX), y.clamp(-CLIP_MAX, CLIP_MAX));
                let extent = self.extent;
                stored = b;
                if clip_segment(&mut a, &mut b, extent) {
                    let active = self.active();
                    if !pv {
                        active.line_start();
                        active.point(a.0, a.1, 0);
                    }
                    active.point(b.0, b.1, 0);
                    if !v {
                        active.line_end();
                    }
                    self.clean = false;
                } else if v {
                    let active = self.active();
                    active.line_start();
                    active.point(x, y, 0);
                    self.clean = false;
                }
            }
        }
        self.previous = (stored.0, stored.1, v);
    }
}

fn rect_corner([x0, y0, x1, _]: [f64; 4], p: P3, direction: f64) -> i32 {
    if (p.0 - x0).abs() < EPSILON {
        if direction > 0.0 { 0 } else { 3 }
    } else if (p.0 - x1).abs() < EPSILON {
        if direction > 0.0 { 2 } else { 1 }
    } else if (p.1 - y0).abs() < EPSILON {
        if direction > 0.0 { 1 } else { 0 }
    } else if direction > 0.0 {
        3
    } else {
        2
    }
}

fn rect_compare(extent: [f64; 4], a: P3, b: P3) -> f64 {
    let ca = rect_corner(extent, a, 1.0);
    let cb = rect_corner(extent, b, 1.0);
    if ca != cb {
        f64::from(ca - cb)
    } else {
        match ca {
            0 => b.1 - a.1,
            1 => a.0 - b.0,
            2 => a.1 - b.1,
            _ => b.0 - a.0,
        }
    }
}

fn rect_interpolate(
    extent: [f64; 4],
    from: Option<P3>,
    to: Option<P3>,
    direction: f64,
    out: &mut dyn Stream,
) {
    let [x0, y0, x1, y1] = extent;
    let mut a = 0;
    let mut a1 = 0;
    let walk = match (from, to) {
        (Some(from), Some(to)) => {
            a = rect_corner(extent, from, direction);
            a1 = rect_corner(extent, to, direction);
            a != a1 || ((rect_compare(extent, from, to) < 0.0) ^ (direction > 0.0))
        }
        _ => true,
    };
    if walk {
        let step = if direction > 0.0 { 1 } else { 3 };
        loop {
            out.point(
                if a == 0 || a == 3 { x0 } else { x1 },
                if a > 1 { y1 } else { y0 },
                0,
            );
            a = (a + step) % 4;
            if a == a1 {
                break;
            }
        }
    } else if let Some(to) = to {
        out.point(to.0, to.1, 0);
    }
}

impl Stream for RectClip<'_> {
    fn point(&mut self, x: f64, y: f64, _m: u8) {
        if self.in_line {
            self.line_point(x, y);
        } else if self.visible(x, y) {
            self.active().point(x, y, 0);
        }
    }

    fn line_start(&mut self) {
        self.in_line = true;
        if let Some(polygon) = self.polygon.as_mut() {
            polygon.push(Vec::new());
        }
        self.first = true;
        self.previous = (f64::NAN, f64::NAN, false);
    }

    fn line_end(&mut self) {
        if self.polygon.is_some() {
            let (x, y, _) = self.first_point;
            self.line_point(x, y);
            if self.first_point.2 && self.previous.2 {
                self.buffer.rejoin();
            }
            let lines = std::mem::take(&mut self.buffer.lines);
            self.segments.extend(lines);
        }
        self.in_line = false;
        if self.previous.2 {
            self.active().line_end();
        }
    }

    fn polygon_start(&mut self) {
        self.buffering = true;
        self.segments.clear();
        self.polygon = Some(Vec::new());
        self.clean = true;
    }

    fn polygon_end(&mut self) {
        let start_inside = self.polygon_inside() != 0;
        let clean_inside = self.clean && start_inside;
        let mut segments = std::mem::take(&mut self.segments);
        let visible = !segments.is_empty();
        self.buffering = false;
        if clean_inside || visible {
            let extent = self.extent;
            self.next.polygon_start();
            if clean_inside {
                self.next.line_start();
                rect_interpolate(extent, None, None, 1.0, self.next);
                self.next.line_end();
            }
            if visible {
                rejoin(
                    &mut segments,
                    &|a, b| rect_compare(extent, a, b),
                    start_inside,
                    &mut |from, to, direction, out| {
                        rect_interpolate(extent, from, to, direction, out);
                    },
                    self.next,
                );
            }
            self.next.polygon_end();
        }
        self.polygon = None;
    }
}

#[cfg(test)]
mod tests {
    //! Expected values were printed by d3-geo 3.1.1 under Node from the same inputs; the
    //! projection formulas are d3-geo's (Equal Earth: Šavrič, Patterson and Jenny 2018;
    //! Natural Earth I: Šavrič, Jenny, Patterson et al. 2011; Albers equal-area conic: Snyder,
    //! "Map Projections: A Working Manual", 1987, pp. 98-103).
    use super::*;

    const POINTS: [[f64; 2]; 6] = [
        [0.0, 0.0],
        [10.0, 20.0],
        [-120.5, 45.25],
        [179.0, -60.0],
        [45.0, -30.0],
        [-75.3, 10.7],
    ];

    fn close(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= 1e-9 * actual.abs().max(expected.abs()).max(1.0)
    }

    fn assert_points(name: &str, projection: &Projection, expected: [[f64; 2]; 6]) {
        for (point, want) in POINTS.iter().zip(expected) {
            let (x, y) = projection.forward(point[0], point[1]);
            assert!(
                close(x, want[0]) && close(y, want[1]),
                "{name} {point:?}: ({x}, {y}) != {want:?}"
            );
        }
    }

    fn kind(name: &str) -> GeoProjectionKind {
        match name {
            "mercator" => GeoProjectionKind::Mercator,
            "equal_earth" => GeoProjectionKind::EqualEarth,
            "natural_earth" => GeoProjectionKind::NaturalEarth1,
            "orthographic" => GeoProjectionKind::Orthographic,
            "conic" => GeoProjectionKind::ConicEqualArea,
            _ => GeoProjectionKind::Identity,
        }
    }

    fn default_projection(name: &str) -> Projection {
        Projection::new(kind(name), [0.0, 60.0])
    }

    #[test]
    fn default_projections_match_d3_forward_values() {
        let cases: [(&str, [[f64; 2]; 6]); 5] = [
            (
                "mercator",
                [
                    [480.0, 250.0],
                    [506.694_444_444_444_46, 195.492_655_986_310_58],
                    [158.331_944_444_444_4, 114.249_901_288_011_87],
                    [957.830_555_555_555_5, 451.425_945_133_051_2],
                    [600.125, 334.015_221_403_995_84],
                    [278.990_833_333_333_34, 221.269_456_204_799_3],
                ],
            ),
            (
                "equal_earth",
                [
                    [480.0, 250.0],
                    [505.868_099_042_655_7, 178.990_606_107_064_74],
                    [205.383_260_119_259_1, 96.863_882_778_935_79],
                    [839.228_941_399_591_2, 442.801_199_416_451_37],
                    [592.127_801_371_551_6, 355.043_199_962_035],
                    [281.076_894_538_842_8, 211.715_850_938_139_68],
                ],
            ),
            (
                "natural_earth",
                [
                    [480.0, 250.0],
                    [506.140_585_344_220_9, 188.260_419_740_486_95],
                    [191.222_858_401_498_68, 110.222_165_937_588_78],
                    [870.483_976_806_154_1, 433.107_944_635_572_95],
                    [594.751_349_660_141_8, 342.757_081_818_082_4],
                    [280.473_736_689_159_86, 217.009_903_135_185_77],
                ],
            ),
            (
                "orthographic",
                [
                    [480.0, 250.0],
                    [520.712_389_836_050_4, 164.665_974_240_245_65],
                    [328.653_395_815_118_76, 72.808_748_781_990_29],
                    [482.177_187_703_051_1, 466.073_338_244_217_44],
                    [632.786_922_706_100_8, 374.75],
                    [242.862_806_945_390_16, 203.676_179_461_298_5],
                ],
            ),
            (
                "conic",
                [
                    [480.0, 350.055_752_849_250_3],
                    [502.735_638_762_888_5, 291.381_983_798_043_56],
                    [304.082_465_618_852_67, 127.683_098_090_734_26],
                    [943.589_467_891_994_2, 93.815_991_053_839_81],
                    [623.327_247_291_486_1, 396.187_208_264_779_1],
                    [302.815_513_382_346_2, 268.112_677_828_812_7],
                ],
            ),
        ];
        for (name, expected) in cases {
            assert_points(name, &default_projection(name), expected);
        }
        assert!(close(
            default_projection("mercator").scale(),
            152.947_900_311_311_43
        ));
    }

    #[test]
    fn rotated_centered_and_planar_projections_match_d3_forward_values() {
        let mut equal_earth = default_projection("equal_earth");
        equal_earth.set_rotate([20.0, -10.0, 5.0]);
        assert_points(
            "ee_rot",
            &equal_earth,
            [
                [535.663_096_658_597_4, 277.334_763_803_783_97],
                [552.667_708_017_330_9, 201.312_094_913_761_85],
                [268.575_264_706_473_6, 109.087_988_111_157_5],
                [108.210_314_728_103_13, 420.809_820_072_483_55],
                [659.579_400_882_481_9, 351.848_391_679_642_65],
                [335.292_078_463_932_17, 246.920_563_250_155_6],
            ],
        );
        let mut orthographic = default_projection("orthographic");
        orthographic.set_rotate([-30.0, -20.0, 10.0]);
        assert_points(
            "ortho_rot",
            &orthographic,
            [
                [369.978_082_172_794_36, 344.441_315_439_263_24],
                [400.190_730_327_268, 259.162_012_488_831_24],
                [356.825_998_146_048_96, 49.550_505_303_324_77],
                [572.182_052_072_509_5, 402.783_634_593_855_2],
                [567.826_060_990_718_4, 426.033_466_716_428_96],
                [235.718_786_931_660_6, 226.404_531_065_400_76],
            ],
        );
        let mut mercator = default_projection("mercator");
        mercator.set_center(10.0, 40.0);
        mercator.set_scale(300.0);
        mercator.set_translate(100.0, 50.0);
        assert_points(
            "merc_center",
            &mercator,
            [
                [47.640_122_440_170_12, 278.872_895_619_983_24],
                [100.0, 171.959_344_202_648_2],
                [-583.296_402_155_780_1, 12.605_561_902_394_584],
                [984.881_930_761_125_1, 673.960_264_697_428_1],
                [283.259_571_459_404_6, 443.664_738_920_199_6],
                [-346.629_755_585_348_9, 222.519_306_449_967_2],
            ],
        );
        let mut albers = Projection::new(GeoProjectionKind::ConicEqualArea, [29.5, 45.5]);
        albers.set_rotate([96.0, 0.0, 0.0]);
        albers.set_center(-0.6, 38.7);
        albers.set_scale(1070.0);
        albers.set_translate(480.0, 250.0);
        assert_points(
            "albers",
            &albers,
            [
                [2_235.936_349_430_882_3, -24.594_281_642_375_563],
                [2_033.117_440_853_713, -365.236_140_212_294_23],
                [170.316_896_091_190_93, 85.649_627_942_202_76],
                [-1_653.471_436_361_309_3, 597.953_177_581_081_4],
                [2_960.366_709_917_335_4, -905.588_191_341_813_6],
                [895.876_933_971_111_6, 718.234_576_491_985_8],
            ],
        );
        let mut identity = default_projection("identity");
        identity.set_scale(2.0);
        identity.set_translate(5.0, 7.0);
        assert_points(
            "identity",
            &identity,
            [
                [5.0, 7.0],
                [25.0, -33.0],
                [-236.0, -83.5],
                [363.0, 127.0],
                [95.0, 67.0],
                [-145.6, -14.399_999_999_999_999],
            ],
        );
    }

    /// Records every projected line as one ring.
    #[derive(Default)]
    struct Capture {
        rings: Vec<Vec<P2>>,
    }

    impl Stream for Capture {
        fn point(&mut self, x: f64, y: f64, _m: u8) {
            if let Some(ring) = self.rings.last_mut() {
                ring.push((x, y));
            }
        }
        fn line_start(&mut self) {
            self.rings.push(Vec::new());
        }
        fn line_end(&mut self) {}
        fn polygon_start(&mut self) {}
        fn polygon_end(&mut self) {}
    }

    /// d3 `geoStream` of a polygon: the closing position is not streamed.
    fn feed_polygon(stream: &mut dyn Stream, ring: &[[f64; 2]]) {
        stream.polygon_start();
        stream.line_start();
        for &[x, y] in &ring[..ring.len() - 1] {
            stream.point(x, y, 0);
        }
        stream.line_end();
        stream.polygon_end();
    }

    const BOX_A: [[f64; 2]; 5] = [
        [-10.0, 40.0],
        [20.0, 40.0],
        [20.0, 10.0],
        [-10.0, 10.0],
        [-10.0, 40.0],
    ];
    const BOX_B: [[f64; 2]; 5] = [
        [170.0, 10.0],
        [-170.0, 10.0],
        [-170.0, -10.0],
        [170.0, -10.0],
        [170.0, 10.0],
    ];
    const BOX_C: [[f64; 2]; 5] = [
        [60.0, 20.0],
        [120.0, 20.0],
        [120.0, -20.0],
        [60.0, -20.0],
        [60.0, 20.0],
    ];

    fn shoelace(ring: &[P2]) -> f64 {
        let mut sum = 0.0;
        for (index, &(x0, y0)) in ring.iter().enumerate() {
            let (x1, y1) = ring[(index + 1) % ring.len()];
            sum += x0 * y1 - x1 * y0;
        }
        sum / 2.0
    }

    fn capture(projection: &Projection, ring: &[[f64; 2]]) -> (Vec<Vec<P2>>, [f64; 4]) {
        let mut sink = Capture::default();
        let mut budget = 1 << 20;
        projection.stream(&mut sink, &mut budget, None, |stream| {
            feed_polygon(stream, ring)
        });
        let mut bounds = Bounds::default();
        for &(x, y) in sink.rings.iter().flatten() {
            bounds.point(x, y, 0);
        }
        (sink.rings, bounds.result())
    }

    fn assert_bounds(actual: [f64; 4], expected: [f64; 4]) {
        assert!(
            actual.iter().zip(expected).all(|(&a, e)| close(a, e)),
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn fit_extent_matches_d3_scale_and_translate() {
        let cases = [
            (
                "mercator",
                124.140_855_611_678_38,
                [400.0, 223.377_258_844_862_35],
            ),
            (
                "equal_earth",
                144.090_622_785_250_56,
                [400.0, 224.233_285_226_011_73],
            ),
            (
                "natural_earth",
                142.575_922_374_731_08,
                [400.0, 222.906_320_555_767_73],
            ),
            (
                "orthographic",
                345.073_537_267_351_87,
                [256.985_399_437_606_8, 246.301_515_959_091_66],
            ),
            (
                "conic",
                160.845_422_948_173_34,
                [400.0, 242.112_352_137_504_44],
            ),
            (
                "identity",
                2.294_117_647_058_824,
                [400.0, 222.941_176_470_588_23],
            ),
        ];
        for (name, scale, translate) in cases {
            let mut projection = default_projection(name);
            projection.fit_extent([10.0, 20.0, 790.0, 380.0], 1 << 20, |stream| {
                for ring in [&BOX_A, &BOX_B, &BOX_C] {
                    feed_polygon(stream, ring);
                }
            });
            let (x, y) = projection.translate();
            assert!(
                close(projection.scale(), scale)
                    && close(x, translate[0])
                    && close(y, translate[1]),
                "{name}: {} ({x}, {y})",
                projection.scale()
            );
        }
    }

    #[test]
    fn antimeridian_clip_splits_a_crossing_polygon_like_d3() {
        let (rings, bounds) = capture(&default_projection("equal_earth"), &BOX_B);
        let mut sizes: Vec<usize> = rings.iter().map(Vec::len).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, [11, 12]);
        for ring in &rings {
            let area = shoelace(ring).abs();
            assert!(close(area, 1_915.551_855_043_3), "{area}");
        }
        let total: f64 = rings.iter().map(|ring| shoelace(ring)).sum::<f64>().abs();
        assert!(close(total, 3_831.103_710_086_735_6), "{total}");
        assert_bounds(
            bounds,
            [
                0.498_845_348_370_821_3,
                213.668_746_620_069_27,
                959.501_154_651_629_1,
                286.331_253_379_930_73,
            ],
        );
    }

    #[test]
    fn orthographic_clips_the_back_hemisphere_like_d3() {
        let orthographic = default_projection("orthographic");
        let (rings, bounds) = capture(&orthographic, &BOX_C);
        assert_eq!(rings.iter().map(Vec::len).collect::<Vec<_>>(), [35]);
        let area = shoelace(&rings[0]).abs();
        assert!(close(area, 5_967.745_125_386_74), "{area}");
        assert_bounds(
            bounds,
            [
                683.042_521_496_668_8,
                153.331_411_523_561_4,
                729.475_933_221_234_5,
                346.668_588_476_436_4,
            ],
        );
        let hidden = [
            [150.0, 20.0],
            [170.0, 20.0],
            [170.0, -20.0],
            [150.0, -20.0],
            [150.0, 20.0],
        ];
        assert!(capture(&orthographic, &hidden).0.is_empty());
    }

    #[test]
    fn polar_ring_clips_to_the_mercator_extent_like_d3() {
        let pole = [
            [-180.0, -60.0],
            [-90.0, -65.0],
            [0.0, -62.0],
            [90.0, -70.0],
            [180.0, -60.0],
            [180.0, -90.0],
            [-180.0, -90.0],
            [-180.0, -60.0],
        ];
        let (rings, bounds) = capture(&default_projection("mercator"), &pole);
        let area: f64 = rings.iter().map(|ring| shoelace(ring)).sum::<f64>().abs();
        assert!(close(area, 210_299.827_183_103_48), "{area}");
        assert_bounds(bounds, [-0.5, 451.425_945_133_051_2, 960.5, 730.5]);
        let (rings, _) = capture(&default_projection("equal_earth"), &pole);
        let area: f64 = rings.iter().map(|ring| shoelace(ring)).sum::<f64>().abs();
        assert!(close(area, 13_003.553_354_651_47), "{area}");
    }
}
