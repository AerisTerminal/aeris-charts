//! Bubble and flow maps (complete build): marks projected through the pane's base map, cached
//! in pre-view coordinates, and drawn and hit through the pane's shared view.
//!
//! A mark cache keys on the plot and on the build stamp of the base map's cache, so a base
//! map rebuild (new data, a resize, another base map) rebuilds the marks against the new
//! projection. Without a base map the marks fit their own projection to their positions.
//!
//! Flows stream each link as a two-point GeoJSON `LineString` through the d3 pipeline, whose
//! adaptive resampling follows the great circle and whose antimeridian or hemisphere clip
//! cuts it. They resample at the precision the deepest zoom needs, so a zoom never
//! reprojects; the per-link subdivision budget bounds the result.

use std::collections::HashMap;
use std::f64::consts::FRAC_1_SQRT_2;
use std::ops::Range;

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{LineStyle, LineType, Prim, TextAlign};

use super::super::{
    GeneralLayoutElement, GeneralLayoutStore, LayoutSeriesEntry, layout_value_text,
};
use super::choropleth::{Collector, GeoCache, feature_centroid, fit_projection, segment_distance};
use super::projection::{Projection, Stream};
use super::view::{MAX_MAP_ZOOM, MapTransform, push_clipped_polyline};
use super::{GeoAnchor, GeoInput, GeoLinksInput, GeoPointsInput};
use crate::ChartEngine;
use crate::GeneralPointSymbol;
use crate::frame::push_general_point_symbol;
use crate::general_axes::GeneralPlotRect;

/// Resampling precision of flows in pre-view px: d3's default `sqrt(0.5)` at the deepest zoom.
const FLOW_PRECISION: f64 = FRAC_1_SQRT_2 / MAX_MAP_ZOOM;
/// Subdivision points one flow may add while resampling.
const FLOW_SUBDIVISIONS: usize = 256;
/// Projected points one flow may keep: its endpoints, its subdivisions, and clip cut points.
const FLOW_POINTS: usize = FLOW_SUBDIVISIONS + 8;
/// Pieces one flow may keep after the antimeridian, hemisphere, and extent clips.
const FLOW_RUNS: usize = 4;
/// Hit slop around flows beyond half their width, in media px.
const LINK_HIT: f64 = 3.0;
/// Opacity of the series color on bubbles and flows, so the base map shows through.
const MARK_ALPHA: f64 = 0.75;
const LABEL_GAP: f64 = 4.0;

/// The marks of a bubble or flow map series.
#[derive(Clone, Copy)]
pub(in crate::general_layout) enum MarkInput<'a> {
    Points(&'a GeoPointsInput),
    Links(&'a GeoLinksInput),
}

/// Reserved cache bytes for the marks: one slot per bubble, or every flow at its point and
/// piece bounds.
pub(in crate::general_layout) fn mark_cache_reservation(input: MarkInput<'_>) -> usize {
    match input {
        MarkInput::Points(input) => {
            input.points.len()
                * (std::mem::size_of::<Option<Bubble>>() + std::mem::size_of::<u32>())
        }
        MarkInput::Links(input) => {
            input.links.len()
                * (FLOW_POINTS * std::mem::size_of::<[f64; 2]>()
                    + FLOW_RUNS * std::mem::size_of::<u32>()
                    + std::mem::size_of::<FlowShape>())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bubble {
    /// Pre-view center.
    center: [f64; 2],
    /// Screen radius in media px.
    radius: f64,
}

#[derive(Clone, Debug)]
struct FlowShape {
    /// Range of this flow's pieces in `runs`.
    runs: Range<u32>,
    /// Screen width in media px.
    width: f64,
    /// Pre-view bounds of the pieces.
    bounds: [f64; 4],
    /// Whether the last piece ends at the visible target.
    target: bool,
}

/// Projected marks of one bubble or flow series for one plot rectangle and base map.
pub(in crate::general_layout) struct MarkCache {
    plot: GeneralPlotRect,
    /// Build stamp of the base map cache; 0 when the marks fitted their own projection.
    base: u64,
    /// Per input point, `None` when hidden (unresolved, clipped, or on the far side).
    bubbles: Vec<Option<Bubble>>,
    /// Paint order of the visible bubbles: larger first, so smaller ones stay on top.
    order: Vec<u32>,
    points: Vec<[f64; 2]>,
    /// Exclusive end offsets into `points` of each flow piece.
    runs: Vec<u32>,
    links: Vec<FlowShape>,
}

/// d3 `scaleSqrt().domain([0, max]).range(range)` (`sqrt` false: linear).
fn mark_size(value: Option<f64>, max: f64, [low, high]: [f64; 2], sqrt: bool) -> f64 {
    match value {
        Some(value) if max > 0.0 => {
            let t = value / max;
            low + (high - low) * if sqrt { t.sqrt() } else { t }
        }
        _ => low,
    }
}

fn max_value(values: impl Iterator<Item = Option<f64>>) -> f64 {
    values.flatten().fold(0.0, f64::max)
}

/// Resolves mark anchors to input coordinates against an optional base map.
struct Anchors<'a> {
    base: Option<&'a GeoInput>,
    rows: HashMap<&'a str, usize>,
    centroids: HashMap<usize, Option<[f64; 2]>>,
}

impl<'a> Anchors<'a> {
    fn new(base: Option<&'a GeoInput>) -> Self {
        let rows = base.map_or_else(HashMap::new, |input| {
            input
                .features
                .iter()
                .enumerate()
                .map(|(row, feature)| (feature.id.as_str(), row))
                .collect()
        });
        Self {
            base,
            rows,
            centroids: HashMap::new(),
        }
    }

    fn resolve(&mut self, anchor: &GeoAnchor) -> Option<[f64; 2]> {
        match anchor {
            GeoAnchor::Position(position) => Some(*position),
            GeoAnchor::Feature(id) => {
                let base = self.base?;
                let row = *self.rows.get(id.as_str())?;
                *self
                    .centroids
                    .entry(row)
                    .or_insert_with(|| feature_centroid(base, row))
            }
        }
    }
}

/// The projection and clip rectangle marks draw through: the base map's, else one fitted to
/// the mark positions `feed` streams.
fn mark_projection(
    base: Option<&GeoCache>,
    own: &super::GeoProjection,
    plot: GeneralPlotRect,
    budget: usize,
    feed: impl FnOnce(&mut dyn Stream),
) -> Option<(Projection, [f64; 4])> {
    match base {
        Some(cache) => cache
            .projection()
            .map(|projection| (projection.clone(), cache.clip())),
        None => fit_projection(own, plot, false, budget, feed),
    }
}

fn stream_line(stream: &mut dyn Stream, [a, b]: [[f64; 2]; 2]) {
    stream.line_start();
    stream.point(a[0], a[1], 0);
    stream.point(b[0], b[1], 0);
    stream.line_end();
}

fn extend_bounds(bounds: &mut [f64; 4], [x, y]: [f64; 2]) {
    bounds[0] = bounds[0].min(x);
    bounds[1] = bounds[1].min(y);
    bounds[2] = bounds[2].max(x);
    bounds[3] = bounds[3].max(y);
}

impl MarkCache {
    fn new(
        input: MarkInput<'_>,
        base: Option<(&GeoInput, &GeoCache)>,
        plot: GeneralPlotRect,
    ) -> Self {
        let mut cache = Self {
            plot,
            base: base.map_or(0, |(_, geo)| geo.stamp()),
            bubbles: Vec::new(),
            order: Vec::new(),
            points: Vec::new(),
            runs: Vec::new(),
            links: Vec::new(),
        };
        let mut anchors = Anchors::new(base.map(|(input, _)| input));
        match input {
            MarkInput::Points(input) => cache.build_bubbles(input, base, &mut anchors),
            MarkInput::Links(input) => cache.build_flows(input, base, &mut anchors),
        }
        cache
    }

    fn build_bubbles(
        &mut self,
        input: &GeoPointsInput,
        base: Option<(&GeoInput, &GeoCache)>,
        anchors: &mut Anchors<'_>,
    ) {
        let positions: Vec<Option<[f64; 2]>> = input
            .points
            .iter()
            .map(|point| anchors.resolve(&point.at))
            .collect();
        self.bubbles = vec![None; input.points.len()];
        let Some((projection, clip)) = mark_projection(
            base.map(|(_, geo)| geo),
            &input.projection,
            self.plot,
            0,
            |stream| {
                for &[x, y] in positions.iter().flatten() {
                    stream.point(x, y, 0);
                }
            },
        ) else {
            return;
        };
        let max = max_value(input.points.iter().map(|point| point.value));
        let mut collector = Collector::default();
        for (row, (point, position)) in input.points.iter().zip(&positions).enumerate() {
            let Some([x, y]) = *position else {
                continue;
            };
            collector.reset(1);
            let mut budget = 0;
            // The clip drops far-side points under the orthographic projection and points
            // outside the map rectangle.
            projection.stream(&mut collector, &mut budget, Some(clip), |stream| {
                stream.point(x, y, 0);
            });
            if let Some(&center) = collector.points.first() {
                self.bubbles[row] = Some(Bubble {
                    center,
                    radius: mark_size(point.value, max, input.radius, true),
                });
            }
        }
        self.order = (0..input.points.len() as u32)
            .filter(|&row| self.bubbles[row as usize].is_some())
            .collect();
        let radius = |row: &u32| self.bubbles[*row as usize].map_or(0.0, |bubble| bubble.radius);
        self.order.sort_by(|a, b| radius(b).total_cmp(&radius(a)));
        self.order.shrink_to_fit();
    }

    fn build_flows(
        &mut self,
        input: &GeoLinksInput,
        base: Option<(&GeoInput, &GeoCache)>,
        anchors: &mut Anchors<'_>,
    ) {
        let ends: Vec<Option<[[f64; 2]; 2]>> = input
            .links
            .iter()
            .map(|link| {
                Some([
                    anchors.resolve(&link.source)?,
                    anchors.resolve(&link.target)?,
                ])
            })
            .collect();
        let empty = FlowShape {
            runs: 0..0,
            width: 0.0,
            bounds: [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            target: false,
        };
        self.links = vec![empty; input.links.len()];
        let Some((mut projection, clip)) = mark_projection(
            base.map(|(_, geo)| geo),
            &input.projection,
            self.plot,
            ends.len().saturating_mul(FLOW_SUBDIVISIONS),
            |stream| {
                for &line in ends.iter().flatten() {
                    stream_line(stream, line);
                }
            },
        ) else {
            return;
        };
        projection.set_precision(FLOW_PRECISION);
        let max = max_value(input.links.iter().map(|link| link.value));
        let mut collector = Collector::default();
        for (row, (link, line)) in input.links.iter().zip(&ends).enumerate() {
            let Some(line) = *line else {
                continue;
            };
            collector.reset(FLOW_POINTS);
            collector.overflow = false;
            let mut budget = FLOW_SUBDIVISIONS;
            projection.stream(&mut collector, &mut budget, Some(clip), |stream| {
                stream_line(stream, line);
            });
            let first = self.runs.len() as u32;
            let shape = &mut self.links[row];
            for index in 0..collector.rings.len() {
                let run = collector.ring(index);
                if run.len() < 2 || self.runs.len() - first as usize >= FLOW_RUNS {
                    continue;
                }
                for &point in run {
                    extend_bounds(&mut shape.bounds, point);
                }
                self.points.extend_from_slice(run);
                self.runs.push(self.points.len() as u32);
            }
            shape.runs = first..self.runs.len() as u32;
            shape.width = mark_size(link.value, max, input.width, false);
            let (tx, ty) = projection.forward(line[1][0], line[1][1]);
            shape.target = shape.runs.end > first
                && self
                    .points
                    .last()
                    .is_some_and(|&[x, y]| (x - tx).abs() <= 1e-6 && (y - ty).abs() <= 1e-6);
        }
        self.points.shrink_to_fit();
        self.runs.shrink_to_fit();
    }

    pub(in crate::general_layout) fn heap_bytes(&self) -> usize {
        self.bubbles.capacity() * std::mem::size_of::<Option<Bubble>>()
            + (self.order.capacity() + self.runs.capacity()) * std::mem::size_of::<u32>()
            + self.points.capacity() * std::mem::size_of::<[f64; 2]>()
            + self.links.capacity() * std::mem::size_of::<FlowShape>()
    }

    fn run(&self, index: usize) -> &[[f64; 2]] {
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.runs[previous] as usize);
        &self.points[start..self.runs[index] as usize]
    }

    fn link_runs(&self, row: usize) -> impl Iterator<Item = &[[f64; 2]]> + '_ {
        let range = self.links[row].runs.clone();
        (range.start as usize..range.end as usize).map(|index| self.run(index))
    }

    /// The screen arrowhead of flow `row` through `view` when it has one: tip, then the two
    /// base corners.
    fn arrow(&self, row: usize, view: &MapTransform) -> Option<[[f64; 2]; 3]> {
        let shape = &self.links[row];
        if !shape.target {
            return None;
        }
        let run = self.link_runs(row).last()?;
        let tip = view.forward(run[run.len() - 1]);
        if !view.contains(tip) {
            return None;
        }
        let from = view.forward(run[run.len() - 2]);
        let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
        let length = dx.hypot(dy);
        if !length.is_finite() || length <= 0.0 {
            return None;
        }
        let (ux, uy) = (dx / length, dy / length);
        let (long, half) = (3.0 * shape.width + 4.0, 1.5 * shape.width + 2.0);
        let base = [tip[0] - ux * long, tip[1] - uy * long];
        Some([
            tip,
            [base[0] - uy * half, base[1] + ux * half],
            [base[0] + uy * half, base[1] - ux * half],
        ])
    }

    /// The topmost (last painted) mark under the screen point seen through `view`.
    pub(in crate::general_layout) fn hit(
        &self,
        view: &MapTransform,
        symbol: GeneralPointSymbol,
        arrows: bool,
        x: f64,
        y: f64,
    ) -> Option<(GeneralLayoutElement, usize)> {
        for &row in self.order.iter().rev() {
            let Some(bubble) = self.bubbles[row as usize] else {
                continue;
            };
            let center = view.forward(bubble.center);
            if view.contains(center) && symbol_contains(symbol, center, bubble.radius, x, y) {
                return Some((GeneralLayoutElement::Bubble, row as usize));
            }
        }
        if !view.contains([x, y]) {
            return None;
        }
        let [px, py] = view.inverse([x, y]);
        let scale = 1.0 / view.zoom();
        for row in (0..self.links.len()).rev() {
            if arrows
                && self
                    .arrow(row, view)
                    .is_some_and(|tri| triangle_contains(tri, x, y))
            {
                return Some((GeneralLayoutElement::Link, row));
            }
            let shape = &self.links[row];
            let slop = (shape.width / 2.0 + LINK_HIT) * scale;
            let b = shape.bounds;
            if !(px >= b[0] - slop && px <= b[2] + slop && py >= b[1] - slop && py <= b[3] + slop) {
                continue;
            }
            if self.link_runs(row).any(|run| {
                run.windows(2)
                    .any(|pair| segment_distance(pair[0], pair[1], px, py) <= slop)
            }) {
                return Some((GeneralLayoutElement::Link, row));
            }
        }
        None
    }

    /// The screen anchor of a bubble's center or the halfway point along a flow's longest
    /// piece, or `None` when hidden.
    pub(in crate::general_layout) fn anchor(
        &self,
        view: &MapTransform,
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<(f64, f64)> {
        let point = match element {
            GeneralLayoutElement::Bubble => self.bubbles.get(row).copied().flatten()?.center,
            GeneralLayoutElement::Link => {
                self.links.get(row)?;
                let length = |run: &[[f64; 2]]| {
                    run.windows(2)
                        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
                        .sum::<f64>()
                };
                let run = self
                    .link_runs(row)
                    .max_by(|a, b| length(a).total_cmp(&length(b)))?;
                let mut remaining = length(run) / 2.0;
                let mut middle = run[0];
                for pair in run.windows(2) {
                    let step = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                    if step >= remaining && step > 0.0 {
                        let t = remaining / step;
                        middle = [
                            pair[0][0] + t * (pair[1][0] - pair[0][0]),
                            pair[0][1] + t * (pair[1][1] - pair[0][1]),
                        ];
                        break;
                    }
                    remaining -= step;
                    middle = pair[1];
                }
                middle
            }
            _ => return None,
        };
        let [x, y] = view.forward(point);
        view.contains([x, y]).then_some((x, y))
    }

    /// The pre-view centers of the visible bubbles, by input row.
    #[cfg(test)]
    pub(in crate::general_layout) fn bubble_center(&self, row: usize) -> Option<[f64; 2]> {
        self.bubbles
            .get(row)
            .copied()
            .flatten()
            .map(|bubble| bubble.center)
    }

    /// The pre-view pieces of flow `row`.
    #[cfg(test)]
    pub(in crate::general_layout) fn flow_runs(&self, row: usize) -> Vec<Vec<[f64; 2]>> {
        self.link_runs(row).map(<[_]>::to_vec).collect()
    }
}

fn symbol_contains(symbol: GeneralPointSymbol, [cx, cy]: [f64; 2], r: f64, x: f64, y: f64) -> bool {
    let (dx, dy) = (x - cx, y - cy);
    match symbol {
        GeneralPointSymbol::Circle => dx.hypot(dy) <= r,
        GeneralPointSymbol::Square => dx.abs() <= r && dy.abs() <= r,
        GeneralPointSymbol::Diamond => dx.abs() + dy.abs() <= r,
        GeneralPointSymbol::Triangle => dy.abs() <= r && dx.abs() <= (dy + r) / 2.0,
    }
}

fn triangle_contains([a, b, c]: [[f64; 2]; 3], x: f64, y: f64) -> bool {
    let side = |p: [f64; 2], q: [f64; 2]| (q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0]);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

impl LayoutSeriesEntry {
    /// Runs `f` on this series' marks, rebuilt when the plot or the pane's base map changed.
    pub(in crate::general_layout) fn with_marks<R>(
        &self,
        store: &GeneralLayoutStore,
        input: MarkInput<'_>,
        plot: GeneralPlotRect,
        f: impl FnOnce(&MarkCache) -> R,
    ) -> R {
        let read = |base: Option<(&GeoInput, &GeoCache)>| {
            let stamp = base.map_or(0, |(_, geo)| geo.stamp());
            let mut cache = self.marks.borrow_mut();
            if cache
                .as_ref()
                .is_none_or(|cached| cached.plot != plot || cached.base != stamp)
            {
                *cache = Some(MarkCache::new(input, base, plot));
            }
            f(cache.as_ref().expect("mark cache initialized"))
        };
        match store.map_base(self.pane_id) {
            Some((series, geo)) => series.with_geo(geo, plot, |cache| read(Some((geo, cache)))),
            None => read(None),
        }
    }
}

/// The color a mark paints: its own, else the series color at `MARK_ALPHA`.
fn mark_color(own: Option<&str>, series: Color) -> Color {
    own.and_then(Color::parse_css).unwrap_or_else(|| {
        Color::rgba(
            series.r(),
            series.g(),
            series.b(),
            (f64::from(series.a()) * MARK_ALPHA).round() as u8,
        )
    })
}

/// Name of a mark anchor: the base feature's label, a bare feature ID, or the coordinates.
fn anchor_label(anchor: &GeoAnchor, base: Option<&GeoInput>) -> String {
    match anchor {
        GeoAnchor::Feature(id) => base
            .and_then(|input| input.features.iter().find(|feature| feature.id == *id))
            .map_or_else(|| id.clone(), |feature| feature.label.clone()),
        GeoAnchor::Position([x, y]) => format!(
            "{}, {}",
            layout_value_text(Some(*x)),
            layout_value_text(Some(*y))
        ),
    }
}

pub(in crate::general_layout) fn bubble_label(
    input: &GeoPointsInput,
    base: Option<&GeoInput>,
    row: usize,
) -> Option<String> {
    let point = input.points.get(row)?;
    Some(match (&point.label, &point.at) {
        (Some(label), _) => label.clone(),
        (None, GeoAnchor::Feature(_)) => anchor_label(&point.at, base),
        (None, GeoAnchor::Position(_)) => point.id.clone(),
    })
}

pub(in crate::general_layout) fn flow_id(input: &GeoLinksInput, row: usize) -> Option<String> {
    let link = input.links.get(row)?;
    Some(link.id.clone().unwrap_or_else(|| format!("#{row}")))
}

impl ChartEngine {
    /// Readable "source to target" name of a flow in the chart language.
    pub(in crate::general_layout) fn flow_label(
        &self,
        input: &GeoLinksInput,
        base: Option<&GeoInput>,
        row: usize,
    ) -> Option<String> {
        let link = input.links.get(row)?;
        let end = |label: &Option<String>, anchor: &GeoAnchor| {
            label.clone().unwrap_or_else(|| anchor_label(anchor, base))
        };
        Some(crate::text::complete::GRAPH_LINK.format(
            self.language,
            &[
                ("source", &end(&link.source_label, &link.source)),
                ("target", &end(&link.target_label, &link.target)),
            ],
        ))
    }

    /// Label and painted color of one bubble or flow.
    pub(in crate::general_layout) fn mark_item_tooltip(
        &self,
        series: &LayoutSeriesEntry,
        input: MarkInput<'_>,
        base: Option<&GeoInput>,
        row: usize,
    ) -> Option<(String, Color, Option<String>)> {
        match input {
            MarkInput::Points(input) => Some((
                bubble_label(input, base, row)?,
                mark_color(input.points.get(row)?.color.as_deref(), series.color),
                None,
            )),
            MarkInput::Links(input) => Some((
                self.flow_label(input, base, row)?,
                mark_color(input.links.get(row)?.color.as_deref(), series.color),
                None,
            )),
        }
    }

    /// Bubbles from largest to smallest with optional labels, or flows in input order with
    /// optional target arrowheads, through the pane's view.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::general_layout) fn build_mark_frame(
        &self,
        series: &LayoutSeriesEntry,
        store: &GeneralLayoutStore,
        input: MarkInput<'_>,
        plot: GeneralPlotRect,
        view: &MapTransform,
        ratio: (f64, f64),
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (hpr, vpr) = ratio;
        let scale = hpr.min(vpr);
        let bitmap = |[x, y]: [f64; 2]| [(x * hpr) as f32, (y * vpr) as f32];
        let base = store.map_base(series.pane_id).map(|(_, input)| input);
        series.with_marks(store, input, plot, |cache| match input {
            MarkInput::Points(input) => {
                for &row in &cache.order {
                    let (Some(bubble), Some(point)) =
                        (cache.bubbles[row as usize], input.points.get(row as usize))
                    else {
                        continue;
                    };
                    let [x, y] = view.forward(bubble.center);
                    if !view.contains([x, y]) {
                        continue;
                    }
                    push_general_point_symbol(
                        out,
                        input.symbol,
                        x * hpr,
                        y * vpr,
                        bubble.radius * scale,
                        mark_color(point.color.as_deref(), series.color),
                    );
                }
                if series.options.data_labels {
                    for (row, bubble) in cache.bubbles.iter().enumerate() {
                        let (Some(bubble), Some(label)) = (bubble, bubble_label(input, base, row))
                        else {
                            continue;
                        };
                        let [x, y] = view.forward(bubble.center);
                        if view.contains([x, y]) {
                            out.push(self.geo_text(
                                label,
                                (x + bubble.radius + LABEL_GAP, y),
                                TextAlign::Left,
                                ratio,
                            ));
                        }
                    }
                }
            }
            MarkInput::Links(input) => {
                let rect = view.rect();
                for (row, (shape, link)) in cache.links.iter().zip(&input.links).enumerate() {
                    let color = mark_color(link.color.as_deref(), series.color);
                    for run in cache.link_runs(row) {
                        push_clipped_polyline(
                            run.iter().map(|&p| view.forward(p)),
                            rect,
                            &bitmap,
                            points,
                            |first_point, point_count| {
                                out.push(Prim::Polyline {
                                    first_point,
                                    point_count,
                                    width: (shape.width * scale) as f32,
                                    style: LineStyle::Solid,
                                    line_type: LineType::Simple,
                                    color,
                                });
                            },
                        );
                    }
                    if input.arrows
                        && let Some([a, b, c]) = cache.arrow(row, view)
                    {
                        out.push(Prim::Triangle {
                            a: bitmap(a),
                            b: bitmap(b),
                            c: bitmap(c),
                            color,
                        });
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(kind: super::super::GeoProjectionKind) -> (GeoInput, GeoCache) {
        let input = GeoInput {
            features: vec![super::super::GeoFeature {
                id: "corners".into(),
                label: "Corners".into(),
                shape: super::super::GeoShape::Point,
                points: vec![[-179.0, 0.0], [179.0, 0.0], [0.0, 80.0], [0.0, -80.0]],
                ..Default::default()
            }],
            projection: super::super::GeoProjection {
                kind,
                ..Default::default()
            },
            ..GeoInput::default()
        };
        let cache = GeoCache::new(&input, PLOT);
        (input, cache)
    }

    const PLOT: GeneralPlotRect = GeneralPlotRect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };

    fn links(pairs: &[([f64; 2], [f64; 2])]) -> GeoLinksInput {
        GeoLinksInput {
            links: pairs
                .iter()
                .map(|&(source, target)| super::super::GeoLink {
                    id: None,
                    source: GeoAnchor::Position(source),
                    target: GeoAnchor::Position(target),
                    source_label: None,
                    target_label: None,
                    value: None,
                    color: None,
                })
                .collect(),
            ..GeoLinksInput::default()
        }
    }

    /// d3-geo 3.1.1 `geoInterpolate(a, b)(t)`, in degrees.
    fn geo_interpolate(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
        let radians = std::f64::consts::PI / 180.0;
        let haversin = |x: f64| (x / 2.0).sin().powi(2);
        let (x0, y0) = (a[0] * radians, a[1] * radians);
        let (x1, y1) = (b[0] * radians, b[1] * radians);
        let (cy0, cy1) = (y0.cos(), y1.cos());
        let (kx0, ky0) = (cy0 * x0.cos(), cy0 * x0.sin());
        let (kx1, ky1) = (cy1 * x1.cos(), cy1 * x1.sin());
        let d = 2.0
            * (haversin(y1 - y0) + cy0 * cy1 * haversin(x1 - x0))
                .sqrt()
                .asin();
        let k = d.sin();
        let t = t * d;
        let (wb, wa) = (t.sin() / k, (d - t).sin() / k);
        let x = wa * kx0 + wb * kx1;
        let y = wa * ky0 + wb * ky1;
        let z = wa * y0.sin() + wb * y1.sin();
        [
            y.atan2(x) / radians,
            z.atan2((x * x + y * y).sqrt()) / radians,
        ]
    }

    #[test]
    fn flows_follow_the_great_circle_through_the_d3_midpoint() {
        let (input, geo) = base(super::super::GeoProjectionKind::EqualEarth);
        let (london, tokyo) = ([-0.1276, 51.5072], [139.6917, 35.6895]);
        let flows = links(&[(london, tokyo)]);
        let cache = MarkCache::new(MarkInput::Links(&flows), Some((&input, &geo)), PLOT);
        let runs = cache.flow_runs(0);
        assert_eq!(runs.len(), 1);
        let projection = geo.projection().unwrap();
        let ends = [london, tokyo].map(|[lon, lat]| projection.forward(lon, lat));
        assert_eq!(runs[0][0], [ends[0].0, ends[0].1]);
        assert_eq!(*runs[0].last().unwrap(), [ends[1].0, ends[1].1]);
        assert!(runs[0].len() > 16, "resampled: {}", runs[0].len());
        assert!(runs[0].len() <= FLOW_POINTS);
        let [lon, lat] = geo_interpolate(london, tokyo, 0.5);
        let (mx, my) = projection.forward(lon, lat);
        let nearest = runs[0]
            .iter()
            .map(|p| (p[0] - mx).hypot(p[1] - my))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1e-9, "midpoint off by {nearest}");
        // Every other point of the great circle lies within the resampling precision.
        let tolerance = FLOW_PRECISION * 1.5;
        for t in [0.1, 0.25, 0.75, 0.9] {
            let [lon, lat] = geo_interpolate(london, tokyo, t);
            let (x, y) = projection.forward(lon, lat);
            let distance = runs[0]
                .windows(2)
                .map(|pair| segment_distance(pair[0], pair[1], x, y))
                .fold(f64::INFINITY, f64::min);
            assert!(distance < tolerance, "t = {t}: {distance}");
        }
        assert!(cache.links[0].target);
        assert!(cache.heap_bytes() <= mark_cache_reservation(MarkInput::Links(&flows)));
    }

    #[test]
    fn flows_crossing_the_antimeridian_split_at_the_map_edges() {
        let (input, geo) = base(super::super::GeoProjectionKind::EqualEarth);
        let flows = links(&[([170.0, 0.0], [-170.0, 10.0])]);
        let cache = MarkCache::new(MarkInput::Links(&flows), Some((&input, &geo)), PLOT);
        let runs = cache.flow_runs(0);
        assert_eq!(runs.len(), 2, "{runs:?}");
        let projection = geo.projection().unwrap();
        let (east, _) = projection.forward(180.0 - 1e-9, 5.0);
        let (west, _) = projection.forward(-180.0 + 1e-9, 5.0);
        assert!((runs[0].last().unwrap()[0] - east).abs() < 1.0);
        assert!((runs[1][0][0] - west).abs() < 1.0);
        assert!(
            cache.links[0].target,
            "the target is visible on the second piece"
        );
    }

    #[test]
    fn orthographic_marks_on_the_far_side_are_hidden() {
        let (input, geo) = base(super::super::GeoProjectionKind::Orthographic);
        let points = GeoPointsInput {
            points: [[10.0, 10.0], [180.0, 0.0], [-120.0, 20.0]]
                .into_iter()
                .enumerate()
                .map(|(row, at)| super::super::GeoPoint {
                    id: format!("p{row}"),
                    label: None,
                    at: GeoAnchor::Position(at),
                    value: Some(row as f64),
                    color: None,
                })
                .collect(),
            ..GeoPointsInput::default()
        };
        let cache = MarkCache::new(MarkInput::Points(&points), Some((&input, &geo)), PLOT);
        let projection = geo.projection().unwrap();
        let (x, y) = projection.forward(10.0, 10.0);
        assert_eq!(cache.bubble_center(0), Some([x, y]));
        assert_eq!(cache.bubble_center(1), None);
        assert_eq!(cache.bubble_center(2), None);
        assert_eq!(cache.order, [0]);

        let flows = links(&[([0.0, 0.0], [170.0, 0.0]), ([0.0, 0.0], [40.0, 20.0])]);
        let cache = MarkCache::new(MarkInput::Links(&flows), Some((&input, &geo)), PLOT);
        assert_eq!(cache.flow_runs(0).len(), 1);
        assert!(
            !cache.links[0].target,
            "the far-side target draws no arrowhead"
        );
        assert!(cache.links[1].target);
        let view = MapTransform::identity(geo.clip());
        assert!(cache.arrow(0, &view).is_none());
        let [tip, ..] = cache.arrow(1, &view).unwrap();
        let (tx, ty) = projection.forward(40.0, 20.0);
        assert_eq!(tip, [tx, ty]);
    }

    #[test]
    fn marks_without_a_base_map_fit_their_own_projection() {
        let flows = links(&[([0.0, 0.0], [40.0, 20.0]), ([-30.0, -10.0], [10.0, 50.0])]);
        let cache = MarkCache::new(MarkInput::Links(&flows), None, PLOT);
        assert_eq!(cache.base, 0);
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for shape in &cache.links {
            bounds[0] = bounds[0].min(shape.bounds[0]);
            bounds[1] = bounds[1].min(shape.bounds[1]);
            bounds[2] = bounds[2].max(shape.bounds[2]);
            bounds[3] = bounds[3].max(shape.bounds[3]);
        }
        let pad = 8.0;
        let fits_x =
            (bounds[0] - pad).abs() < 1e-6 && (bounds[2] - (PLOT.width - pad)).abs() < 1e-6;
        let fits_y =
            (bounds[1] - pad).abs() < 1e-6 && (bounds[3] - (PLOT.height - pad)).abs() < 1e-6;
        assert!(fits_x || fits_y, "{bounds:?}");
    }

    #[test]
    fn sizes_scale_by_square_root_or_linearly_within_the_range() {
        assert_eq!(mark_size(Some(25.0), 100.0, [2.0, 12.0], true), 7.0);
        assert_eq!(mark_size(Some(25.0), 100.0, [2.0, 12.0], false), 4.5);
        assert_eq!(mark_size(None, 100.0, [2.0, 12.0], true), 2.0);
        assert_eq!(mark_size(Some(0.0), 0.0, [2.0, 12.0], true), 2.0);
        assert_eq!(mark_size(Some(100.0), 100.0, [2.0, 12.0], true), 12.0);
    }

    #[test]
    fn symbol_hits_follow_the_painted_shape() {
        let c = [100.0, 100.0];
        assert!(symbol_contains(
            GeneralPointSymbol::Circle,
            c,
            10.0,
            107.0,
            107.0
        ));
        assert!(!symbol_contains(
            GeneralPointSymbol::Circle,
            c,
            10.0,
            108.0,
            108.0
        ));
        assert!(symbol_contains(
            GeneralPointSymbol::Square,
            c,
            10.0,
            109.0,
            109.0
        ));
        assert!(!symbol_contains(
            GeneralPointSymbol::Diamond,
            c,
            10.0,
            106.0,
            106.0
        ));
        assert!(symbol_contains(
            GeneralPointSymbol::Diamond,
            c,
            10.0,
            104.0,
            105.0
        ));
        assert!(symbol_contains(
            GeneralPointSymbol::Triangle,
            c,
            10.0,
            109.0,
            109.0
        ));
        assert!(!symbol_contains(
            GeneralPointSymbol::Triangle,
            c,
            10.0,
            104.0,
            91.0
        ));
        assert!(triangle_contains(
            [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
            2.0,
            2.0
        ));
        assert!(!triangle_contains(
            [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
            8.0,
            8.0
        ));
    }
}
