//! Choropleth map layout (complete build): projects a map dataset into the plot, triangulates
//! its polygons, indexes the projected shapes for hits, and paints value-colored fills,
//! borders, and a color legend.
//!
//! One cache per series and plot rectangle holds the projected rings, the triangle mesh, a
//! uniform grid over feature bounds, and one anchor per feature. Colors are not cached: the
//! frame maps values through the scale every time, so theme and data-value changes repaint
//! without reprojecting. Hits test the same projected rings the frame fills, even-odd, so a
//! hole answers no hit and an island answers its feature.
//!
//! Polygon orientation follows d3-geo: an exterior ring runs clockwise on the sphere, so its
//! projection has a positive screen-space (y-down) shoelace area and its holes negative. A
//! polygon whose exterior ring encloses more than a hemisphere is read as wound the other way
//! (RFC 7946 counterclockwise exteriors) and streamed reversed; planar identity data is
//! normalized per ring. Rings that clipping or projection collapse to zero area are dropped,
//! and a hole that lies in no exterior of its polygon is dropped with it.

use std::f64::consts::{FRAC_PI_4, PI, TAU};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::super::{GeneralLayoutElement, LayoutSeriesEntry, layout_irect, layout_value_text};
use super::earcut::Earcut;
use super::projection::{Centroid, Projection, Stream};
use super::view::{MapTransform, push_clipped_polyline, push_clipped_triangle};
use super::{ChoroplethScale, GeoFeature, GeoInput, GeoProjectionKind, GeoShape, QuantizeMethod};
use crate::ChartEngine;
use crate::frame::interpolate_color;
use crate::general_axes::GeneralPlotRect;

const LEGEND_HEIGHT: f64 = 36.0;
const LEGEND_WIDTH: f64 = 160.0;
const LEGEND_STRIP: f64 = 10.0;
const LEGEND_STEPS: usize = 32;
const TEXT_SIZE: f64 = 12.0;
const LABEL_GAP: f64 = 6.0;
const BORDER_WIDTH: f64 = 0.75;
const LINE_WIDTH: f64 = 1.5;
const POINT_RADIUS: f64 = 3.0;
/// Hit slop around line features, in media px.
const LINE_HIT: f64 = 4.0;
/// Hit slop around point features beyond their radius.
const POINT_HIT: f64 = 2.0;
/// Largest side of the hit grid.
const MAX_GRID: usize = 64;
/// Cells one feature may occupy; larger features are tested on every hit.
const MAX_FEATURE_CELLS: usize = 16;
/// Side of the sample grid searched for an anchor when a polygon's centroid is outside it.
const ANCHOR_GRID: usize = 16;
/// Edges one anchor search measures distances to; longer outlines are strided.
const ANCHOR_EDGES: usize = 2_048;
/// Hole-to-exterior containment work per feature before holes attach by bounds only.
const HOLE_WORK: usize = 1 << 22;
/// Subdivision points resampling may add across one fit or projection pass.
const RESAMPLE_POINTS: usize = 8_192;

/// Projected positions a cache may hold for a dataset: every input position twice (resampling
/// and clipping split and densify rings), clip-edge interpolation per ring, and slack.
fn point_capacity(positions: usize, rings: usize) -> usize {
    2 * positions + 32 * rings + 2 * RESAMPLE_POINTS
}

/// Projected rings a cache may hold: clipping splits a ring into a few pieces.
fn ring_capacity(rings: usize, features: usize) -> usize {
    4 * rings + features + 1_024
}

fn grid_capacity(features: usize) -> usize {
    MAX_GRID * MAX_GRID + 1 + (MAX_FEATURE_CELLS + 1) * features
}

fn totals(input: &GeoInput) -> (usize, usize) {
    input
        .features
        .iter()
        .fold((0, 0), |(points, rings), feature| {
            (points + feature.points.len(), rings + feature.rings.len())
        })
}

/// Reserved cache bytes for a choropleth over `input`: the projected positions, ring offsets,
/// triangle indices, hit grid, and per-feature shapes at their capacity bounds.
pub(in crate::general_layout) fn cache_reservation(input: &GeoInput) -> usize {
    let (positions, rings) = totals(input);
    let features = input.features.len();
    let points = point_capacity(positions, rings);
    let ring_slots = ring_capacity(rings, features);
    points * std::mem::size_of::<[f64; 2]>()
        + ring_slots * std::mem::size_of::<u32>()
        + 3 * (points + 2 * ring_slots) * std::mem::size_of::<u32>()
        + grid_capacity(features) * std::mem::size_of::<u32>()
        + features * std::mem::size_of::<FeatureShape>()
}

/// The projected geometry of one feature.
#[derive(Clone, Debug)]
struct FeatureShape {
    shape: GeoShape,
    /// Range of this feature's rings (polygon rings, lines, or one ring per point).
    rings: Range<u32>,
    /// Range of this feature's triangle indices.
    triangles: Range<u32>,
    /// Bounds of the projected positions, `[x0, y0, x1, y1]`; empty when nothing is visible.
    bounds: [f64; 4],
    anchor: Option<(f64, f64)>,
}

/// Distinguishes cache builds so bubble and flow caches rebuild when their base map's does.
static NEXT_STAMP: AtomicU64 = AtomicU64::new(1);

/// Projected and triangulated geometry of one choropleth series for one plot rectangle.
pub(in crate::general_layout) struct GeoCache {
    plot: GeneralPlotRect,
    stamp: u64,
    /// The fitted projection, shared by the pane's bubble and flow maps; `None` when the
    /// plot is too small to fit.
    projection: Option<Projection>,
    /// The map rectangle `[x0, y0, x1, y1]`: the plot above the legend, which clips the
    /// geometry and bounds the view.
    clip: [f64; 4],
    /// Space reserved under the map for the legend.
    legend: bool,
    points: Vec<[f64; 2]>,
    /// Exclusive ring end offsets into `points`; polygon rings are stored open.
    rings: Vec<u32>,
    triangles: Vec<u32>,
    features: Vec<FeatureShape>,
    grid: usize,
    /// CSR hit grid: `cell_starts[c]..cell_starts[c + 1]` indexes `cell_items`.
    cell_starts: Vec<u32>,
    cell_items: Vec<u32>,
    /// Features covering more than `MAX_FEATURE_CELLS` cells.
    wide: Vec<u32>,
    /// Whether the position or ring capacity cut geometry short.
    truncated: bool,
}

fn empty_bounds() -> [f64; 4] {
    [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ]
}

fn extend_bounds(bounds: &mut [f64; 4], [x, y]: [f64; 2]) {
    bounds[0] = bounds[0].min(x);
    bounds[1] = bounds[1].min(y);
    bounds[2] = bounds[2].max(x);
    bounds[3] = bounds[3].max(y);
}

fn bounds_contain(bounds: &[f64; 4], x: f64, y: f64) -> bool {
    x >= bounds[0] && x <= bounds[2] && y >= bounds[1] && y <= bounds[3]
}

/// Shoelace area of an open ring; positive for clockwise rings in y-down screen space.
fn ring_area(ring: &[[f64; 2]]) -> f64 {
    let Some(&last) = ring.last() else {
        return 0.0;
    };
    let mut previous = last;
    let mut sum = 0.0;
    for &point in ring {
        sum += previous[0] * point[1] - point[0] * previous[1];
        previous = point;
    }
    sum / 2.0
}

/// Even-odd crossing test of one open ring.
fn ring_crossings(ring: &[[f64; 2]], x: f64, y: f64) -> bool {
    let Some(&last) = ring.last() else {
        return false;
    };
    let mut inside = false;
    let mut previous = last;
    for &point in ring {
        if (point[1] > y) != (previous[1] > y)
            && x < (previous[0] - point[0]) * (y - point[1]) / (previous[1] - point[1]) + point[0]
        {
            inside = !inside;
        }
        previous = point;
    }
    inside
}

pub(super) fn segment_distance(a: [f64; 2], b: [f64; 2], x: f64, y: f64) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((x - a[0]) * dx + (y - a[1]) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a[0] + t * dx - x).hypot(a[1] + t * dy - y)
}

/// d3 `geoArea` of one polygon in degrees: the spherical excess, in steradians, of the region
/// to the right of travel along its rings (inside a clockwise exterior, outside its holes).
fn spherical_polygon_area<'a>(rings: impl Iterator<Item = &'a [[f64; 2]]>) -> f64 {
    let sum: f64 = rings.map(spherical_ring_sum).sum();
    2.0 * if sum < 0.0 { TAU + sum } else { sum }
}

/// d3 `areaRingSum` of one ring: half its signed spherical excess, before wrapping.
fn spherical_ring_sum(ring: &[[f64; 2]]) -> f64 {
    let Some(&first) = ring.first() else {
        return 0.0;
    };
    let radians = PI / 180.0;
    let mut lambda0 = first[0] * radians;
    let phi0 = first[1] * radians / 2.0 + FRAC_PI_4;
    let (mut sin_phi0, mut cos_phi0) = phi0.sin_cos();
    let mut sum = 0.0;
    for &[lon, lat] in ring[1..].iter().chain(std::iter::once(&first)) {
        let lambda = lon * radians;
        let phi = lat * radians / 2.0 + FRAC_PI_4;
        let delta = lambda - lambda0;
        let sign = if delta >= 0.0 { 1.0 } else { -1.0 };
        let abs_delta = sign * delta;
        let (sin_phi, cos_phi) = phi.sin_cos();
        let k = sin_phi0 * sin_phi;
        sum += (k * sign * abs_delta.sin()).atan2(cos_phi0 * cos_phi + k * abs_delta.cos());
        lambda0 = lambda;
        sin_phi0 = sin_phi;
        cos_phi0 = cos_phi;
    }
    sum
}

/// Per input ring: whether to stream it reversed so exteriors and holes reach the projection
/// in d3's winding.
fn reverse_flags(input: &GeoInput) -> Vec<bool> {
    let planar = input.projection.kind == GeoProjectionKind::Identity;
    let mut flags = Vec::with_capacity(input.features.iter().map(|f| f.rings.len()).sum());
    for feature in &input.features {
        feature_flags(feature, planar, &mut flags);
    }
    flags
}

/// Appends the reverse flags of one feature's rings to `flags`.
fn feature_flags(feature: &GeoFeature, planar: bool, flags: &mut Vec<bool>) {
    if feature.shape != GeoShape::Polygon {
        flags.extend(std::iter::repeat_n(false, feature.rings.len()));
        return;
    }
    let ranges: Vec<Range<usize>> = feature.ring_ranges().collect();
    let mut start = 0usize;
    for &end in &feature.polygons {
        let polygon = &ranges[start..end as usize];
        if planar {
            // Planar y-up input: exteriors must run clockwise (negative shoelace), holes
            // counterclockwise, to keep d3's screen orientation after the y reflection.
            for (index, range) in polygon.iter().enumerate() {
                let area = ring_area(&feature.points[range.clone()]);
                flags.push(if index == 0 { area > 0.0 } else { area < 0.0 });
            }
        } else {
            // Summing all rings, as d3 does, keeps a polygon whose exterior is a sliver
            // along a pole cut (the 1:50m Antarctica) from reading as nearly the sphere.
            let area =
                spherical_polygon_area(polygon.iter().map(|range| &feature.points[range.clone()]));
            let reverse = area > TAU;
            flags.extend(std::iter::repeat_n(reverse, polygon.len()));
        }
        start = end as usize;
    }
}

/// Where a mark anchored to feature `row` sits, in input coordinates: the d3 `geoCentroid`
/// of the feature, or under the identity projection its planar area centroid (the vertex mean
/// of line and point features).
pub(super) fn feature_centroid(input: &GeoInput, row: usize) -> Option<[f64; 2]> {
    let feature = input.features.get(row)?;
    if input.projection.kind != GeoProjectionKind::Identity {
        let mut flags = Vec::with_capacity(feature.rings.len());
        feature_flags(feature, false, &mut flags);
        let mut centroid = Centroid::default();
        emit_feature(feature, &flags, &mut centroid);
        return centroid.result();
    }
    if feature.shape == GeoShape::Polygon {
        let ranges: Vec<Range<usize>> = feature.ring_ranges().collect();
        let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
        let mut start = 0usize;
        for &end in &feature.polygons {
            for (index, range) in ranges[start..end as usize].iter().enumerate() {
                let ring = &feature.points[range.clone()];
                let signed = ring_area(ring);
                let Some(&last) = ring.last() else { continue };
                let (mut sx, mut sy) = (0.0, 0.0);
                let mut previous = last;
                for &point in ring {
                    let cross = previous[0] * point[1] - point[0] * previous[1];
                    sx += (previous[0] + point[0]) * cross;
                    sy += (previous[1] + point[1]) * cross;
                    previous = point;
                }
                // Exteriors add and holes subtract whatever their input winding.
                let sign = if (index == 0) == (signed >= 0.0) {
                    1.0
                } else {
                    -1.0
                };
                area += sign * signed;
                cx += sign * sx / 6.0;
                cy += sign * sy / 6.0;
            }
            start = end as usize;
        }
        if area != 0.0 && (cx / area).is_finite() && (cy / area).is_finite() {
            return Some([cx / area, cy / area]);
        }
    }
    let count = feature.points.len();
    (count > 0).then(|| {
        let [x, y] = feature.points.iter().fold([0.0, 0.0], |sum, point| {
            [sum[0] + point[0], sum[1] + point[1]]
        });
        [x / count as f64, y / count as f64]
    })
}

/// Streams one feature in degrees, in d3 `geoStream` order.
fn emit_feature(feature: &GeoFeature, flags: &[bool], stream: &mut dyn Stream) {
    let ring = |stream: &mut dyn Stream, range: Range<usize>, reverse: bool| {
        stream.line_start();
        let points = &feature.points[range];
        if reverse {
            for &[x, y] in points.iter().rev() {
                stream.point(x, y, 0);
            }
        } else {
            for &[x, y] in points {
                stream.point(x, y, 0);
            }
        }
        stream.line_end();
    };
    match feature.shape {
        GeoShape::Empty => {}
        GeoShape::Point => {
            for &[x, y] in &feature.points {
                stream.point(x, y, 0);
            }
        }
        GeoShape::Line => {
            for range in feature.ring_ranges() {
                ring(stream, range, false);
            }
        }
        GeoShape::Polygon => {
            let mut rings = feature.ring_ranges().zip(flags.iter().copied());
            let mut start = 0usize;
            for &end in &feature.polygons {
                stream.polygon_start();
                for (range, reverse) in rings.by_ref().take(end as usize - start) {
                    ring(stream, range, reverse);
                }
                stream.polygon_end();
                start = end as usize;
            }
        }
    }
}

/// Stream sink recording projected rings and polygon groups.
#[derive(Default)]
pub(super) struct Collector {
    pub(super) points: Vec<[f64; 2]>,
    pub(super) rings: Vec<u32>,
    groups: Vec<u32>,
    ring_start: usize,
    in_line: bool,
    limit: usize,
    pub(super) overflow: bool,
}

impl Collector {
    pub(super) fn reset(&mut self, limit: usize) {
        self.points.clear();
        self.rings.clear();
        self.groups.clear();
        self.in_line = false;
        self.limit = limit;
    }

    pub(super) fn ring(&self, index: usize) -> &[[f64; 2]] {
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.rings[previous] as usize);
        &self.points[start..self.rings[index] as usize]
    }
}

impl Stream for Collector {
    fn point(&mut self, x: f64, y: f64, _m: u8) {
        if self.points.len() >= self.limit {
            self.overflow = true;
            return;
        }
        self.points.push([x, y]);
        if !self.in_line {
            self.rings.push(self.points.len() as u32);
        }
    }

    fn line_start(&mut self) {
        self.in_line = true;
        self.ring_start = self.points.len();
    }

    fn line_end(&mut self) {
        self.in_line = false;
        if self.points.len() > self.ring_start {
            self.rings.push(self.points.len() as u32);
        }
    }

    fn polygon_start(&mut self) {}

    fn polygon_end(&mut self) {
        if self.groups.last().copied() != Some(self.rings.len() as u32) {
            self.groups.push(self.rings.len() as u32);
        }
    }
}

/// Fitted projection of `input` into `plot`: d3 `fitExtent` over the padded plot, then the
/// optional center shift. Returns the projection and its clip rectangle.
fn fitted_projection(
    input: &GeoInput,
    flags: &[bool],
    ring_bases: &[usize],
    plot: GeneralPlotRect,
    legend: bool,
) -> Option<(Projection, [f64; 4])> {
    let (positions, _) = totals(input);
    fit_projection(
        &input.projection,
        plot,
        legend,
        positions + RESAMPLE_POINTS,
        |stream| {
            for (feature, &base) in input.features.iter().zip(ring_bases) {
                emit_feature(feature, &flags[base..base + feature.rings.len()], stream);
            }
        },
    )
}

/// d3 `fitExtent` of the geometry `feed` streams over the plot inset by the projection
/// padding (above the legend when one is reserved), then the optional center shift. Returns
/// the projection and its clip rectangle.
pub(super) fn fit_projection(
    options: &super::GeoProjection,
    plot: GeneralPlotRect,
    legend: bool,
    budget: usize,
    feed: impl FnOnce(&mut dyn Stream),
) -> Option<(Projection, [f64; 4])> {
    let bottom = plot.y + plot.height - if legend { LEGEND_HEIGHT } else { 0.0 };
    let pad = options.padding;
    let extent = [
        plot.x + pad,
        plot.y + pad,
        plot.x + plot.width - pad,
        bottom - pad,
    ];
    if extent[2] - extent[0] < 1.0 || extent[3] - extent[1] < 1.0 {
        return None;
    }
    let mut projection = Projection::new(options.kind, options.parallels);
    if options.kind != GeoProjectionKind::Identity {
        projection.set_rotate(options.rotate);
    }
    projection.fit_extent(extent, budget, feed);
    if let Some([lon, lat]) = options.center {
        let (x, y) = projection.forward(lon, lat);
        let (tx, ty) = projection.translate();
        let (mx, my) = ((extent[0] + extent[2]) / 2.0, (extent[1] + extent[3]) / 2.0);
        if x.is_finite() && y.is_finite() {
            projection.set_translate(tx + mx - x, ty + my - y);
        }
    }
    Some((projection, [plot.x, plot.y, plot.x + plot.width, bottom]))
}

impl GeoCache {
    fn empty(plot: GeneralPlotRect, legend: bool, features: usize) -> Self {
        Self {
            plot,
            stamp: NEXT_STAMP.fetch_add(1, Ordering::Relaxed),
            projection: None,
            clip: [plot.x, plot.y, plot.x + plot.width, plot.y + plot.height],
            legend,
            points: Vec::new(),
            rings: Vec::new(),
            triangles: Vec::new(),
            features: vec![
                FeatureShape {
                    shape: GeoShape::Empty,
                    rings: 0..0,
                    triangles: 0..0,
                    bounds: empty_bounds(),
                    anchor: None,
                };
                features
            ],
            grid: 0,
            cell_starts: Vec::new(),
            cell_items: Vec::new(),
            wide: Vec::new(),
            truncated: false,
        }
    }

    pub(in crate::general_layout) fn new(input: &GeoInput, plot: GeneralPlotRect) -> Self {
        let legend = !input.values.is_empty();
        let mut cache = Self::empty(plot, legend, input.features.len());
        let flags = reverse_flags(input);
        let ring_bases: Vec<usize> = input
            .features
            .iter()
            .scan(0usize, |base, feature| {
                let start = *base;
                *base += feature.rings.len();
                Some(start)
            })
            .collect();
        let Some((projection, clip)) = fitted_projection(input, &flags, &ring_bases, plot, legend)
        else {
            return cache;
        };
        cache.clip = clip;
        let (positions, rings) = totals(input);
        let point_limit = point_capacity(positions, rings);
        let ring_limit = ring_capacity(rings, input.features.len());
        let mut collector = Collector::default();
        let mut earcut = Earcut::default();
        let mut triangles = Vec::new();
        let mut budget = positions + RESAMPLE_POINTS;
        for (row, feature) in input.features.iter().enumerate() {
            if feature.shape == GeoShape::Empty {
                continue;
            }
            collector.reset(point_limit - cache.points.len());
            let base = ring_bases[row];
            projection.stream(&mut collector, &mut budget, Some(clip), |stream| {
                emit_feature(feature, &flags[base..base + feature.rings.len()], stream);
            });
            if collector.overflow {
                cache.truncated = true;
            }
            let ring_start = cache.rings.len() as u32;
            let triangle_start = cache.triangles.len() as u32;
            if feature.shape == GeoShape::Polygon {
                cache.add_polygons(&collector, ring_limit, &mut earcut, &mut triangles);
            } else {
                for index in 0..collector.rings.len() {
                    if cache.rings.len() >= ring_limit {
                        cache.truncated = true;
                        break;
                    }
                    cache.points.extend_from_slice(collector.ring(index));
                    cache.rings.push(cache.points.len() as u32);
                }
            }
            let shape = &mut cache.features[row];
            shape.shape = feature.shape;
            shape.rings = ring_start..cache.rings.len() as u32;
            shape.triangles = triangle_start..cache.triangles.len() as u32;
            let first = ring_start
                .checked_sub(1)
                .map_or(0, |previous| cache.rings[previous as usize] as usize);
            let mut bounds = empty_bounds();
            for &point in &cache.points[first..] {
                extend_bounds(&mut bounds, point);
            }
            cache.features[row].bounds = bounds;
            cache.features[row].anchor = cache.anchor_of(row);
        }
        cache.build_grid();
        cache.projection = Some(projection);
        cache.points.shrink_to_fit();
        cache.rings.shrink_to_fit();
        cache.triangles.shrink_to_fit();
        cache
    }

    /// Classifies, attaches holes, stores, and triangulates the polygon groups of one feature.
    fn add_polygons(
        &mut self,
        collector: &Collector,
        ring_limit: usize,
        earcut: &mut Earcut,
        triangles: &mut Vec<u32>,
    ) {
        let mut work = HOLE_WORK;
        let mut start = 0usize;
        for &end in &collector.groups {
            let group = start..end as usize;
            start = end as usize;
            let mut exteriors: Vec<(usize, f64, [f64; 4])> = Vec::new();
            let mut holes: Vec<usize> = Vec::new();
            for index in group {
                let ring = collector.ring(index);
                let area = ring_area(ring);
                if area > 0.0 {
                    let mut bounds = empty_bounds();
                    for &point in ring {
                        extend_bounds(&mut bounds, point);
                    }
                    exteriors.push((index, area, bounds));
                } else if area < 0.0 {
                    holes.push(index);
                }
            }
            let mut owned: Vec<Vec<usize>> = vec![Vec::new(); exteriors.len()];
            for hole in holes {
                let [x, y] = collector.ring(hole)[0];
                let mut best: Option<usize> = None;
                for (slot, (exterior, area, bounds)) in exteriors.iter().enumerate() {
                    if !bounds_contain(bounds, x, y) {
                        continue;
                    }
                    if exteriors.len() == 1 || work == 0 {
                        best = Some(slot);
                        break;
                    }
                    let ring = collector.ring(*exterior);
                    work = work.saturating_sub(ring.len());
                    if ring_crossings(ring, x, y)
                        && best.is_none_or(|known| exteriors[known].1 > *area)
                    {
                        best = Some(slot);
                    }
                }
                if let Some(slot) = best {
                    owned[slot].push(hole);
                }
            }
            for ((exterior, ..), holes) in exteriors.iter().zip(&owned) {
                if self.rings.len() + 1 + holes.len() > ring_limit {
                    self.truncated = true;
                    return;
                }
                let base = self.points.len();
                let mut hole_starts = Vec::with_capacity(holes.len());
                for &index in std::iter::once(exterior).chain(holes) {
                    if index != *exterior {
                        hole_starts.push(self.points.len() - base);
                    }
                    self.points.extend_from_slice(collector.ring(index));
                    self.rings.push(self.points.len() as u32);
                }
                triangles.clear();
                earcut.triangulate(&self.points[base..], &hole_starts, triangles);
                self.triangles
                    .extend(triangles.iter().map(|&index| index + base as u32));
            }
        }
    }

    pub(in crate::general_layout) fn heap_bytes(&self) -> usize {
        self.points.capacity() * std::mem::size_of::<[f64; 2]>()
            + (self.rings.capacity()
                + self.triangles.capacity()
                + self.cell_starts.capacity()
                + self.cell_items.capacity()
                + self.wide.capacity())
                * std::mem::size_of::<u32>()
            + self.features.capacity() * std::mem::size_of::<FeatureShape>()
    }

    fn ring(&self, index: usize) -> &[[f64; 2]] {
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.rings[previous] as usize);
        &self.points[start..self.rings[index] as usize]
    }

    fn feature_rings(&self, row: usize) -> impl Iterator<Item = &[[f64; 2]]> + '_ {
        let range = self.features[row].rings.clone();
        (range.start as usize..range.end as usize).map(|index| self.ring(index))
    }

    fn polygon_contains(&self, row: usize, x: f64, y: f64) -> bool {
        self.feature_rings(row)
            .fold(false, |inside, ring| inside ^ ring_crossings(ring, x, y))
    }

    /// Hit slop of a feature shape around its geometry.
    fn slop(shape: GeoShape) -> f64 {
        match shape {
            GeoShape::Line => LINE_HIT,
            GeoShape::Point => POINT_RADIUS + POINT_HIT,
            GeoShape::Polygon | GeoShape::Empty => 0.0,
        }
    }

    /// Whether feature `row` contains the pre-view point; `scale` converts screen slop into
    /// pre-view units under a zoomed view.
    fn feature_contains(&self, row: usize, x: f64, y: f64, scale: f64) -> bool {
        let feature = &self.features[row];
        let slop = Self::slop(feature.shape) * scale;
        let b = feature.bounds;
        if !(x >= b[0] - slop && x <= b[2] + slop && y >= b[1] - slop && y <= b[3] + slop) {
            return false;
        }
        match feature.shape {
            GeoShape::Polygon => self.polygon_contains(row, x, y),
            GeoShape::Line => self.feature_rings(row).any(|line| {
                line.windows(2)
                    .any(|pair| segment_distance(pair[0], pair[1], x, y) <= slop)
            }),
            GeoShape::Point => self
                .feature_rings(row)
                .flatten()
                .any(|point| (point[0] - x).hypot(point[1] - y) <= slop),
            GeoShape::Empty => false,
        }
    }

    /// The focus and tooltip anchor of a feature: the centroid of its largest exterior ring
    /// when inside the feature, else the sampled interior point farthest from every edge
    /// (a bounded pole of inaccessibility); a line's middle vertex; a point's position.
    fn anchor_of(&self, row: usize) -> Option<(f64, f64)> {
        let feature = &self.features[row];
        match feature.shape {
            GeoShape::Empty => None,
            GeoShape::Point => self
                .feature_rings(row)
                .next()
                .map(|ring| (ring[0][0], ring[0][1])),
            GeoShape::Line => self
                .feature_rings(row)
                .max_by_key(|line| line.len())
                .map(|line| {
                    let [x, y] = line[line.len() / 2];
                    (x, y)
                }),
            GeoShape::Polygon => {
                let exterior = self
                    .feature_rings(row)
                    .map(|ring| (ring, ring_area(ring)))
                    .filter(|(_, area)| *area > 0.0)
                    .max_by(|a, b| a.1.total_cmp(&b.1))?;
                let (ring, area) = exterior;
                let (mut cx, mut cy) = (0.0, 0.0);
                let mut previous = *ring.last()?;
                for &point in ring {
                    let cross = previous[0] * point[1] - point[0] * previous[1];
                    cx += (previous[0] + point[0]) * cross;
                    cy += (previous[1] + point[1]) * cross;
                    previous = point;
                }
                let centroid = (cx / (6.0 * area), cy / (6.0 * area));
                if centroid.0.is_finite()
                    && centroid.1.is_finite()
                    && self.polygon_contains(row, centroid.0, centroid.1)
                {
                    return Some(centroid);
                }
                let mut bounds = empty_bounds();
                for &point in ring {
                    extend_bounds(&mut bounds, point);
                }
                let edges: usize = self.feature_rings(row).map(<[_]>::len).sum();
                let stride = edges.div_ceil(ANCHOR_EDGES).max(1);
                let clearance = |x: f64, y: f64| {
                    let mut best = f64::INFINITY;
                    for ring in self.feature_rings(row) {
                        let mut index = 0;
                        while index < ring.len() {
                            let next = ring[(index + stride).min(ring.len()) % ring.len()];
                            best = best.min(segment_distance(ring[index], next, x, y));
                            index += stride;
                        }
                    }
                    best
                };
                let mut anchor = None;
                let mut deepest = f64::NEG_INFINITY;
                for gx in 0..ANCHOR_GRID {
                    for gy in 0..ANCHOR_GRID {
                        let x = bounds[0]
                            + (bounds[2] - bounds[0]) * (gx as f64 + 0.5) / ANCHOR_GRID as f64;
                        let y = bounds[1]
                            + (bounds[3] - bounds[1]) * (gy as f64 + 0.5) / ANCHOR_GRID as f64;
                        if !self.polygon_contains(row, x, y) {
                            continue;
                        }
                        let depth = clearance(x, y);
                        if depth > deepest {
                            deepest = depth;
                            anchor = Some((x, y));
                        }
                    }
                }
                anchor.or_else(|| Some((ring[0][0], ring[0][1])))
            }
        }
    }

    fn cell_span(&self, bounds: [f64; 4], slop: f64) -> Option<(Range<usize>, Range<usize>)> {
        if self.grid == 0
            || bounds[0]
                .partial_cmp(&bounds[2])
                .is_none_or(|order| order.is_gt())
        {
            return None;
        }
        let plot = self.plot;
        let cell_w = plot.width / self.grid as f64;
        let cell_h = plot.height / self.grid as f64;
        let index = |value: f64, start: f64, size: f64| {
            (((value - start) / size).floor().max(0.0) as usize).min(self.grid - 1)
        };
        let x0 = index(bounds[0] - slop, plot.x, cell_w);
        let x1 = index(bounds[2] + slop, plot.x, cell_w);
        let y0 = index(bounds[1] - slop, plot.y, cell_h);
        let y1 = index(bounds[3] + slop, plot.y, cell_h);
        Some((x0..x1 + 1, y0..y1 + 1))
    }

    fn build_grid(&mut self) {
        let drawn = self
            .features
            .iter()
            .filter(|feature| feature.bounds[0] <= feature.bounds[2])
            .count();
        if drawn == 0 || self.plot.width <= 0.0 || self.plot.height <= 0.0 {
            return;
        }
        self.grid = ((drawn as f64).sqrt().ceil() as usize).clamp(1, MAX_GRID);
        let cells = self.grid * self.grid;
        let mut counts = vec![0u32; cells + 1];
        let spans: Vec<Option<(Range<usize>, Range<usize>)>> = self
            .features
            .iter()
            .map(|feature| self.cell_span(feature.bounds, Self::slop(feature.shape)))
            .collect();
        for (row, span) in spans.iter().enumerate() {
            let Some((xs, ys)) = span else {
                continue;
            };
            if xs.len() * ys.len() > MAX_FEATURE_CELLS {
                self.wide.push(row as u32);
                continue;
            }
            for y in ys.clone() {
                for x in xs.clone() {
                    counts[y * self.grid + x + 1] += 1;
                }
            }
        }
        for cell in 0..cells {
            counts[cell + 1] += counts[cell];
        }
        let mut fill = counts.clone();
        self.cell_items = vec![0; counts[cells] as usize];
        for (row, span) in spans.iter().enumerate() {
            let Some((xs, ys)) = span else {
                continue;
            };
            if xs.len() * ys.len() > MAX_FEATURE_CELLS {
                continue;
            }
            for y in ys.clone() {
                for x in xs.clone() {
                    let cell = y * self.grid + x;
                    self.cell_items[fill[cell] as usize] = row as u32;
                    fill[cell] += 1;
                }
            }
        }
        self.cell_starts = counts;
        self.wide.shrink_to_fit();
    }

    pub(in crate::general_layout) fn stamp(&self) -> u64 {
        self.stamp
    }

    pub(super) fn projection(&self) -> Option<&Projection> {
        self.projection.as_ref()
    }

    pub(in crate::general_layout) fn clip(&self) -> [f64; 4] {
        self.clip
    }

    #[cfg(test)]
    fn hit(&self, x: f64, y: f64) -> Option<(GeneralLayoutElement, usize)> {
        self.hit_view(&MapTransform::identity(self.clip), x, y)
    }

    #[cfg(test)]
    fn anchor(&self, row: usize) -> Option<(f64, f64)> {
        self.anchor_view(&MapTransform::identity(self.clip), row)
    }

    /// The topmost (last painted) feature under the screen point seen through `view`.
    pub(in crate::general_layout) fn hit_view(
        &self,
        view: &MapTransform,
        x: f64,
        y: f64,
    ) -> Option<(GeneralLayoutElement, usize)> {
        if !view.contains([x, y]) {
            return None;
        }
        let [x, y] = view.inverse([x, y]);
        let scale = 1.0 / view.zoom();
        let (xs, ys) = self.cell_span([x, y, x, y], 0.0)?;
        let cell = ys.start * self.grid + xs.start;
        let items =
            &self.cell_items[self.cell_starts[cell] as usize..self.cell_starts[cell + 1] as usize];
        let in_cell = items
            .iter()
            .rev()
            .find(|&&row| self.feature_contains(row as usize, x, y, scale));
        let in_wide = self
            .wide
            .iter()
            .rev()
            .find(|&&row| self.feature_contains(row as usize, x, y, scale));
        let row = in_cell.max(in_wide)?;
        Some((GeneralLayoutElement::Feature, *row as usize))
    }

    /// The screen anchor of feature `row` through `view`, or `None` outside the map.
    pub(in crate::general_layout) fn anchor_view(
        &self,
        view: &MapTransform,
        row: usize,
    ) -> Option<(f64, f64)> {
        let (x, y) = self.features.get(row)?.anchor?;
        let [x, y] = view.forward([x, y]);
        view.contains([x, y]).then_some((x, y))
    }
}

impl LayoutSeriesEntry {
    pub(in crate::general_layout) fn with_geo<R>(
        &self,
        input: &GeoInput,
        plot: GeneralPlotRect,
        f: impl FnOnce(&GeoCache) -> R,
    ) -> R {
        let mut cache = self.geo.borrow_mut();
        if cache.as_ref().is_none_or(|cached| cached.plot != plot) {
            *cache = Some(GeoCache::new(input, plot));
        }
        f(cache.as_ref().expect("geo cache initialized"))
    }
}

/// The resolved color scale of one choropleth frame.
struct Palette {
    scale: Scale,
    low: Color,
    mid: Color,
    high: Color,
    missing: Color,
    extent: Option<(f64, f64)>,
}

enum Scale {
    Sequential,
    Diverging(f64),
    /// Class thresholds (`classes - 1` of them) and the class count.
    Quantize(Vec<f64>, usize),
}

fn parse_or(color: Option<&str>, fallback: Color) -> Color {
    color.and_then(Color::parse_css).unwrap_or(fallback)
}

/// d3-array `quantileSorted` (R-7 interpolation).
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let position = (sorted.len() - 1) as f64 * p;
    let index = position.floor() as usize;
    let low = sorted[index];
    match sorted.get(index + 1) {
        Some(&high) => low + (high - low) * (position - index as f64),
        None => low,
    }
}

impl Palette {
    fn new(base: Color, input: &GeoInput, values: &[f64], surface: Color, text: Color) -> Self {
        let faint = Color::rgba(
            base.r(),
            base.g(),
            base.b(),
            (f64::from(base.a()) * 0.2).round() as u8,
        );
        let extent = values
            .iter()
            .filter(|value| !value.is_nan())
            .fold(None, |extent, &value| match extent {
                None => Some((value, value)),
                Some((low, high)) => Some((f64::min(low, value), f64::max(high, value))),
            });
        let missing = parse_or(
            input.missing_color.as_deref(),
            interpolate_color(surface, text, 0.25),
        );
        let (scale, low, mid, high) = match &input.scale {
            ChoroplethScale::Sequential {
                low_color,
                high_color,
            } => (
                Scale::Sequential,
                parse_or(low_color.as_deref(), faint),
                base,
                parse_or(high_color.as_deref(), base),
            ),
            ChoroplethScale::Diverging {
                low_color,
                mid_color,
                high_color,
                midpoint,
            } => (
                Scale::Diverging(*midpoint),
                parse_or(low_color.as_deref(), Color::rgb(0x21, 0x66, 0xac)),
                parse_or(mid_color.as_deref(), Color::rgb(0xf7, 0xf7, 0xf7)),
                parse_or(high_color.as_deref(), Color::rgb(0xb2, 0x18, 0x2b)),
            ),
            ChoroplethScale::Quantize {
                classes,
                method,
                low_color,
                high_color,
            } => {
                let classes = usize::from(*classes).max(2);
                let thresholds = match (method, extent) {
                    (_, None) => Vec::new(),
                    (QuantizeMethod::EqualInterval, Some((low, high))) => (1..classes)
                        .map(|k| low + (high - low) * k as f64 / classes as f64)
                        .collect(),
                    (QuantizeMethod::Quantile, Some(_)) => {
                        let mut sorted: Vec<f64> =
                            values.iter().copied().filter(|v| !v.is_nan()).collect();
                        sorted.sort_by(f64::total_cmp);
                        (1..classes)
                            .map(|k| quantile(&sorted, k as f64 / classes as f64))
                            .collect()
                    }
                };
                (
                    Scale::Quantize(thresholds, classes),
                    parse_or(low_color.as_deref(), faint),
                    base,
                    parse_or(high_color.as_deref(), base),
                )
            }
        };
        Self {
            scale,
            low,
            mid,
            high,
            missing,
            extent,
        }
    }

    fn class_color(&self, class: usize, classes: usize) -> Color {
        interpolate_color(self.low, self.high, class as f64 / (classes - 1) as f64)
    }

    fn color(&self, value: f64) -> Color {
        let Some((low, high)) = self.extent.filter(|_| !value.is_nan()) else {
            return self.missing;
        };
        match &self.scale {
            Scale::Sequential => {
                let t = if high > low {
                    (value - low) / (high - low)
                } else {
                    1.0
                };
                interpolate_color(self.low, self.high, t)
            }
            Scale::Diverging(midpoint) => {
                if value < *midpoint {
                    let t = if *midpoint > low {
                        (value - low) / (midpoint - low)
                    } else {
                        1.0
                    };
                    interpolate_color(self.low, self.mid, t)
                } else {
                    let t = if high > *midpoint {
                        (value - midpoint) / (high - midpoint)
                    } else {
                        0.0
                    };
                    interpolate_color(self.mid, self.high, t)
                }
            }
            Scale::Quantize(thresholds, classes) => {
                // d3 `bisectRight`: a value on a threshold belongs to the upper class.
                let class = thresholds.partition_point(|&threshold| threshold <= value);
                self.class_color(class, *classes)
            }
        }
    }
}

impl ChartEngine {
    pub(super) fn geo_text(
        &self,
        text: String,
        (x, y): (f64, f64),
        align: TextAlign,
        ratio: (f64, f64),
    ) -> Prim {
        Prim::Text {
            x: (x * ratio.0) as f32,
            y: (y * ratio.1) as f32,
            text,
            color: self.primary_text_color(),
            size: (TEXT_SIZE * ratio.1) as f32,
            family: self.options.get().layout.font_family.clone(),
            align,
            weight: 400,
            italic: false,
        }
    }

    /// Fills, then borders, then line and point features, then labels and the legend. The
    /// cached geometry is in pre-view coordinates; `view` maps it to the screen and clips it
    /// to the map rectangle, so a pan or zoom never reprojects or retriangulates.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::general_layout) fn build_choropleth_frame(
        &self,
        series: &LayoutSeriesEntry,
        (input, values): (&GeoInput, &[f64]),
        plot: GeneralPlotRect,
        view: &MapTransform,
        ratio: (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (hpr, vpr) = ratio;
        let surface = self.chart_surface_color();
        let text = self.primary_text_color();
        let palette = Palette::new(series.color, input, values, surface, text);
        let scale = hpr.min(vpr);
        let bitmap = |[x, y]: [f64; 2]| [(x * hpr) as f32, (y * vpr) as f32];
        let rect = view.rect();
        series.with_geo(input, plot, |cache| {
            let fill = |row: usize| palette.color(values.get(row).copied().unwrap_or(f64::NAN));
            for (row, feature) in cache.features.iter().enumerate() {
                if feature.shape != GeoShape::Polygon || feature.triangles.is_empty() {
                    continue;
                }
                let first_point = points.len() as u32;
                let indices = &cache.triangles
                    [feature.triangles.start as usize..feature.triangles.end as usize];
                for triangle in indices.as_chunks::<3>().0 {
                    let corners = triangle.map(|index| view.forward(cache.points[index as usize]));
                    push_clipped_triangle(corners, rect, &bitmap, points);
                }
                let point_count = points.len() as u32 - first_point;
                if point_count > 0 {
                    out.push(Prim::MeshFill {
                        first_point,
                        point_count,
                        color: fill(row),
                    });
                }
            }
            for (row, feature) in cache.features.iter().enumerate() {
                let (width, color, closed) = match feature.shape {
                    GeoShape::Polygon => (BORDER_WIDTH, surface, true),
                    GeoShape::Line => (LINE_WIDTH, fill(row), false),
                    GeoShape::Point | GeoShape::Empty => continue,
                };
                for ring in cache.feature_rings(row) {
                    let closing = closed.then(|| ring[0]);
                    push_clipped_polyline(
                        ring.iter().copied().chain(closing).map(|p| view.forward(p)),
                        rect,
                        &bitmap,
                        points,
                        |first_point, point_count| {
                            out.push(Prim::Polyline {
                                first_point,
                                point_count,
                                width: (width * scale) as f32,
                                style: LineStyle::Solid,
                                line_type: LineType::Simple,
                                color,
                            });
                        },
                    );
                }
            }
            for (row, feature) in cache.features.iter().enumerate() {
                if feature.shape != GeoShape::Point {
                    continue;
                }
                for &point in cache.feature_rings(row).flatten() {
                    let [x, y] = view.forward(point);
                    if !view.contains([x, y]) {
                        continue;
                    }
                    out.push(Prim::Circle {
                        cx: (x * hpr) as f32,
                        cy: (y * vpr) as f32,
                        radius: (POINT_RADIUS * scale) as f32,
                        fill: fill(row),
                        stroke_width: scale as f32,
                        stroke: surface,
                    });
                }
            }
            if series.options.data_labels {
                let family = &self.options.get().layout.font_family;
                let zoom = view.zoom();
                for (row, feature) in cache.features.iter().enumerate() {
                    let (Some(anchor), Some(source)) =
                        (cache.anchor_view(view, row), input.features.get(row))
                    else {
                        continue;
                    };
                    let width = self.measure_text_run(&source.label, TEXT_SIZE, family, 400, false);
                    if feature.shape == GeoShape::Polygon
                        && width.is_finite()
                        && width <= (feature.bounds[2] - feature.bounds[0]) * zoom
                        && TEXT_SIZE <= (feature.bounds[3] - feature.bounds[1]) * zoom
                    {
                        out.push(self.geo_text(
                            source.label.clone(),
                            anchor,
                            TextAlign::Center,
                            ratio,
                        ));
                    }
                }
            }
            if cache.legend {
                self.push_choropleth_legend(&palette, input, plot, ratio, out);
            }
        });
    }

    fn push_choropleth_legend(
        &self,
        palette: &Palette,
        input: &GeoInput,
        plot: GeneralPlotRect,
        ratio: (f64, f64),
        out: &mut Vec<Prim>,
    ) {
        let Some((minimum, maximum)) = palette.extent else {
            return;
        };
        let pad = input.projection.padding;
        let strip = GeneralPlotRect {
            x: plot.x + pad,
            y: plot.y + plot.height - LEGEND_HEIGHT + LABEL_GAP,
            width: LEGEND_WIDTH.min(plot.width - 2.0 * pad),
            height: LEGEND_STRIP,
        };
        if strip.width <= 0.0 || strip.y < plot.y {
            return;
        }
        let (steps, color): (usize, Box<dyn Fn(usize) -> Color + '_>) = match &palette.scale {
            Scale::Quantize(_, classes) => (
                *classes,
                Box::new(|index| palette.class_color(index, *classes)),
            ),
            _ => (
                LEGEND_STEPS,
                Box::new(|index| {
                    let t = (index as f64 + 0.5) / LEGEND_STEPS as f64;
                    palette.color(minimum + (maximum - minimum) * t)
                }),
            ),
        };
        let step = strip.width / steps as f64;
        for index in 0..steps {
            if let Some(rect) = layout_irect(
                GeneralPlotRect {
                    x: strip.x + index as f64 * step,
                    width: step,
                    ..strip
                },
                ratio.0,
                ratio.1,
            ) {
                out.push(Prim::Rect {
                    rect,
                    color: color(index),
                });
            }
        }
        let label_y = strip.y + LEGEND_STRIP + LABEL_GAP + TEXT_SIZE * 0.5;
        out.push(self.geo_text(
            layout_value_text(Some(minimum)),
            (strip.x, label_y),
            TextAlign::Left,
            ratio,
        ));
        out.push(self.geo_text(
            layout_value_text(Some(maximum)),
            (strip.x + strip.width, label_y),
            TextAlign::Right,
            ratio,
        ));
    }

    /// Label and painted color of one feature.
    pub(in crate::general_layout) fn choropleth_item_tooltip(
        &self,
        series: &LayoutSeriesEntry,
        input: &GeoInput,
        values: &[f64],
        row: usize,
    ) -> Option<(String, Color, Option<String>)> {
        let feature = input.features.get(row)?;
        let palette = Palette::new(
            series.color,
            input,
            values,
            self.chart_surface_color(),
            self.primary_text_color(),
        );
        let color = palette.color(values.get(row).copied().unwrap_or(f64::NAN));
        Some((feature.label.clone(), color, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::general_layout::GeoProjection;

    const PLOT: GeneralPlotRect = GeneralPlotRect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    fn polygon(id: &str, polygons: &[&[&[[f64; 2]]]]) -> GeoFeature {
        let mut feature = GeoFeature {
            id: id.into(),
            label: id.to_uppercase(),
            shape: GeoShape::Polygon,
            ..GeoFeature::default()
        };
        for rings in polygons {
            for ring in *rings {
                feature.points.extend_from_slice(ring);
                feature.rings.push(feature.points.len() as u32);
            }
            feature.polygons.push(feature.rings.len() as u32);
        }
        feature
    }

    fn input(features: Vec<GeoFeature>, kind: GeoProjectionKind) -> GeoInput {
        GeoInput {
            features,
            projection: GeoProjection {
                kind,
                ..GeoProjection::default()
            },
            ..GeoInput::default()
        }
    }

    /// A square with a square hole and a two-island multipolygon in planar y-up units.
    fn planar() -> GeoInput {
        let a = polygon(
            "a",
            &[&[
                &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                &[[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]],
            ]],
        );
        let b = polygon(
            "b",
            &[
                &[&[[20.0, 0.0], [30.0, 0.0], [30.0, 10.0], [20.0, 10.0]]],
                &[&[[40.0, 0.0], [45.0, 0.0], [45.0, 5.0], [40.0, 5.0]]],
            ],
        );
        input(vec![a, b], GeoProjectionKind::Identity)
    }

    fn mesh_area(cache: &GeoCache, row: usize) -> f64 {
        let range = &cache.features[row].triangles;
        cache.triangles[range.start as usize..range.end as usize]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| cache.points[index as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum()
    }

    fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
        (actual - expected).abs() <= tolerance * expected.abs().max(1.0)
    }

    /// Plot position of planar `(x, y)`, read off the fitted bounds of feature `a`, which
    /// spans 0..10 on both axes.
    fn planar_to_plot(cache: &GeoCache) -> impl Fn(f64, f64) -> (f64, f64) {
        let bounds = cache.features[0].bounds;
        let k = (bounds[2] - bounds[0]) / 10.0;
        move |x, y| (bounds[0] + k * x, bounds[3] - k * y)
    }

    #[test]
    fn triangulated_area_matches_the_shoelace_area_with_holes() {
        let cache = GeoCache::new(&planar(), PLOT);
        let bounds = cache.features[0].bounds;
        let k = (bounds[2] - bounds[0]) / 10.0;
        assert!(close(bounds[3] - bounds[1], 10.0 * k, 1e-12));
        assert!(close(mesh_area(&cache, 0), k * k * (100.0 - 16.0), 1e-9));
        assert!(close(mesh_area(&cache, 1), k * k * (100.0 + 25.0), 1e-9));
        assert!(!cache.truncated);
    }

    #[test]
    fn hits_fill_rings_skip_holes_and_reach_every_island() {
        let cache = GeoCache::new(&planar(), PLOT);
        let at = planar_to_plot(&cache);
        let hit = |x: f64, y: f64| {
            let (px, py) = at(x, y);
            cache.hit(px, py)
        };
        assert_eq!(hit(1.5, 5.0), Some((GeneralLayoutElement::Feature, 0)));
        assert_eq!(hit(5.0, 5.0), None, "the hole is not part of the feature");
        assert_eq!(hit(25.0, 5.0), Some((GeneralLayoutElement::Feature, 1)));
        assert_eq!(hit(42.5, 2.5), Some((GeneralLayoutElement::Feature, 1)));
        assert_eq!(hit(35.0, 5.0), None);
        assert_eq!(hit(42.5, 7.5), None);
    }

    #[test]
    fn anchors_avoid_holes_and_hit_back() {
        let cache = GeoCache::new(&planar(), PLOT);
        let at = planar_to_plot(&cache);
        let hole = at(5.0, 5.0);
        for row in 0..2 {
            let (x, y) = cache.anchor(row).expect("anchor");
            assert_eq!(cache.hit(x, y), Some((GeneralLayoutElement::Feature, row)));
        }
        let anchor = cache.anchor(0).unwrap();
        assert!((anchor.0 - hole.0).hypot(anchor.1 - hole.1) > 1.0);
    }

    #[test]
    fn cache_bytes_stay_within_the_reservation() {
        for input in [
            planar(),
            input(planar().features, GeoProjectionKind::Orthographic),
        ] {
            let cache = GeoCache::new(&input, PLOT);
            assert!(cache.heap_bytes() <= cache_reservation(&input));
        }
    }

    #[test]
    fn rfc_7946_and_d3_windings_project_the_same_spherical_polygon() {
        let ccw = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let mut cw = ccw;
        cw.reverse();
        let decoy = polygon("z", &[&[&[[60.0, 40.0], [70.0, 40.0], [70.0, 50.0]]]]);
        let area = |ring: &[[f64; 2]]| {
            let input = input(
                vec![polygon("a", &[&[ring]]), decoy.clone()],
                GeoProjectionKind::EqualEarth,
            );
            mesh_area(&GeoCache::new(&input, PLOT), 0)
        };
        let (left, right) = (area(&ccw), area(&cw));
        assert!(left > 100.0);
        assert!(close(left, right, 1e-9), "{left} != {right}");
    }

    #[test]
    fn antimeridian_features_split_into_hittable_pieces() {
        let ring = [
            [170.0, 10.0],
            [-170.0, 10.0],
            [-170.0, -10.0],
            [170.0, -10.0],
        ];
        let features = vec![polygon("b", &[&[&ring]])];
        let cache = GeoCache::new(&input(features, GeoProjectionKind::EqualEarth), PLOT);
        assert_eq!(cache.feature_rings(0).count(), 2);
        let bounds = cache.features[0].bounds;
        let middle = (bounds[1] + bounds[3]) / 2.0;
        for x in [bounds[0] + 2.0, bounds[2] - 2.0] {
            assert_eq!(
                cache.hit(x, middle),
                Some((GeneralLayoutElement::Feature, 0))
            );
        }
        assert_eq!(cache.hit((bounds[0] + bounds[2]) / 2.0, middle), None);
    }

    #[test]
    fn orthographic_back_faces_draw_and_hit_nothing() {
        let front = polygon(
            "front",
            &[&[&[[-10.0, 10.0], [10.0, 10.0], [10.0, -10.0], [-10.0, -10.0]]]],
        );
        let back = polygon(
            "back",
            &[&[&[
                [170.0, 10.0],
                [-170.0, 10.0],
                [-170.0, -10.0],
                [170.0, -10.0],
            ]]],
        );
        let cache = GeoCache::new(
            &input(vec![front, back], GeoProjectionKind::Orthographic),
            PLOT,
        );
        assert!(!cache.features[0].triangles.is_empty());
        assert!(cache.features[1].triangles.is_empty());
        assert_eq!(cache.anchor(1), None);
        let (x, y) = cache.anchor(0).unwrap();
        assert_eq!(cache.hit(x, y), Some((GeneralLayoutElement::Feature, 0)));
    }

    #[test]
    fn quantize_classes_follow_d3_thresholds_and_missing_values_stay_neutral() {
        let surface = Color::rgb(255, 255, 255);
        let text = Color::rgb(0, 0, 0);
        let base = Color::rgb(0, 0, 255);
        let mut geo = planar();
        geo.scale = ChoroplethScale::Quantize {
            classes: 4,
            method: QuantizeMethod::EqualInterval,
            low_color: None,
            high_color: None,
        };
        let values = [0.0, 1.0, 2.0, 3.0, 4.0, f64::NAN];
        let palette = Palette::new(base, &geo, &values, surface, text);
        let Scale::Quantize(thresholds, 4) = &palette.scale else {
            panic!("quantize scale");
        };
        assert_eq!(thresholds, &[1.0, 2.0, 3.0]);
        assert_eq!(palette.color(1.0), palette.class_color(1, 4));
        assert_eq!(palette.color(0.999), palette.class_color(0, 4));
        assert_eq!(palette.color(4.0), palette.class_color(3, 4));
        assert_eq!(
            palette.color(f64::NAN),
            interpolate_color(surface, text, 0.25)
        );
        geo.scale = ChoroplethScale::Quantize {
            classes: 2,
            method: QuantizeMethod::Quantile,
            low_color: None,
            high_color: None,
        };
        let skewed = [0.0, 0.0, 0.0, 10.0];
        let Scale::Quantize(thresholds, _) = Palette::new(base, &geo, &skewed, surface, text).scale
        else {
            panic!("quantize scale");
        };
        assert_eq!(thresholds, [0.0]);
    }

    fn world(scale: &str) -> Option<String> {
        let path = format!(
            "{}/../../examples/web_demo/node_modules/world-atlas/countries-{scale}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).ok()
    }

    /// Release timings of the Natural Earth countries through decode, projection,
    /// triangulation, and hit testing, checked against d3-geo 3.1.1 `fitExtent` and
    /// `geoPath.area` under Node (`natural_earth1`, extent `[[8, 8], [792, 592]]`). Needs
    /// `npm ci` in `examples/web_demo`; run with
    /// `cargo test --release -p aeris_charts_engine --features complete world_atlas -- --ignored --nocapture`.
    #[test]
    #[ignore = "release timing; reads the world-atlas fixture"]
    fn world_atlas_decode_project_triangulate_and_hit_timings() {
        use std::time::Instant;
        let cases = [
            (
                "110m",
                145.050_196_094_943_7,
                11_910.727_732_156_418,
                88_858.751_661_632_02,
            ),
            (
                "50m",
                145.069_293_122_311_8,
                11_881.234_172_261_396,
                88_502.574_253_379_54,
            ),
        ];
        for (scale, d3_scale, d3_russia, d3_total) in cases {
            let Some(text) = world(scale) else {
                panic!("run npm ci in examples/web_demo for the world-atlas fixture");
            };
            // The 1:50m countries give Australia and Ashmore and Cartier Is. the same id "036",
            // so the unique `name` property is the identity.
            let wire = format!(
                r#"{{"topojson":{text},"object":"countries","id_property":"name","projection":{{"type":"natural_earth1"}}}}"#
            );
            let started = Instant::now();
            let geo: GeoInput = serde_json::from_str(&wire).unwrap();
            let topojson_ms = started.elapsed().as_secs_f64() * 1e3;
            let started = Instant::now();
            super::super::validate(&geo).unwrap();
            let validate_ms = started.elapsed().as_secs_f64() * 1e3;
            let geojson = serde_json::to_string(&geo).unwrap();
            let started = Instant::now();
            let reparsed: GeoInput = serde_json::from_str(&geojson).unwrap();
            let geojson_ms = started.elapsed().as_secs_f64() * 1e3;
            assert_eq!(reparsed, geo);

            let flags = reverse_flags(&geo);
            let ring_bases: Vec<usize> = geo
                .features
                .iter()
                .scan(0usize, |base, feature| {
                    let start = *base;
                    *base += feature.rings.len();
                    Some(start)
                })
                .collect();
            let started = Instant::now();
            let (projection, clip) =
                fitted_projection(&geo, &flags, &ring_bases, PLOT, false).unwrap();
            let fit_ms = started.elapsed().as_secs_f64() * 1e3;
            assert!(
                close(projection.scale(), d3_scale, 1e-9),
                "{}",
                projection.scale()
            );
            let started = Instant::now();
            let mut sink = super::super::projection::Bounds::default();
            let mut budget = usize::MAX;
            for (feature, &base) in geo.features.iter().zip(&ring_bases) {
                projection.stream(&mut sink, &mut budget, Some(clip), |stream| {
                    emit_feature(feature, &flags[base..base + feature.rings.len()], stream);
                });
            }
            let project_ms = started.elapsed().as_secs_f64() * 1e3;
            let started = Instant::now();
            let cache = GeoCache::new(&geo, PLOT);
            let cache_ms = started.elapsed().as_secs_f64() * 1e3;
            assert!(!cache.truncated);
            assert!(cache.heap_bytes() <= cache_reservation(&geo));
            let russia = geo
                .features
                .iter()
                .position(|feature| feature.label == "Russia")
                .unwrap();
            let russia_area = mesh_area(&cache, russia);
            let total: f64 = (0..geo.features.len())
                .map(|row| mesh_area(&cache, row))
                .sum();
            assert!(close(russia_area, d3_russia, 1e-6), "{russia_area}");
            // The 1:110m Sudan ring crosses itself twice; earcut 3.2.4 covers 894.454463 px²
            // of it (this port matches to the last digit) where the shoelace area d3 reports
            // is 871.410497 px². Every other feature matches d3 within 1e-6.
            assert!(close(total, d3_total, 1e-3), "{total}");

            let samples = 100_000;
            let mut state = 0x9e37_79b9_7f4a_7c15_u64;
            let mut next = || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 11) as f64 / (1u64 << 53) as f64
            };
            let started = Instant::now();
            let mut hits = 0usize;
            for _ in 0..samples {
                if cache
                    .hit(next() * PLOT.width, next() * PLOT.height)
                    .is_some()
                {
                    hits += 1;
                }
            }
            let hit_us = started.elapsed().as_secs_f64() * 1e6 / f64::from(samples);
            let positions: usize = geo
                .features
                .iter()
                .map(|feature| feature.points.len())
                .sum();
            println!(
                "{scale}: {} features, {positions} positions; topojson {} B parse {topojson_ms:.2} ms, \
                 geojson {} B parse {geojson_ms:.2} ms, validate {validate_ms:.2} ms, fit {fit_ms:.2} ms, \
                 project {project_ms:.2} ms, cache (project + triangulate + grid) {cache_ms:.2} ms, \
                 {} triangles, cache {} B of {} B reserved, hit {hit_us:.3} us ({hits} hits), \
                 Russia {russia_area} (d3 {d3_russia}), total {total} (d3 {d3_total})",
                geo.features.len(),
                text.len(),
                geojson.len(),
                cache.triangles.len() / 3,
                cache.heap_bytes(),
                cache_reservation(&geo),
            );
        }
    }
}
