//! Series hit testing (reference v5.2 `hitTestPane`/`hitTestSeriesRange`/`hitTestLineSeries` ports).
//!
//! Geometry is computed in media (CSS) px — x from the pane's left edge, y from the chart's
//! top — the same space the frame builders' coordinate converters produce, so a hit exactly
//! matches what was painted. Item selection reuses the frame builders' conflation helpers
//! (`visible_ohlc`/`visible_line_rows`/`visible_histogram_rows`), keeping the tested geometry
//! identical to the drawn geometry at any bar spacing.
//!
//! Per-kind rules (ports, with the reference default `hitTestTolerance` of 3 px):
//! - candlestick/bar: `hitTestSeriesRange` over the bar's full high–low span (reference
//!   bars-pane-view-base.ts uses `highY..lowY` — no body/wick distinction), inside the bar's
//!   horizontal slot (midpoints between adjacent bars, `barSpacing/2` at the edges).
//! - histogram: the same range test over `value..base`.
//! - line/area/baseline: `hitTestLineSeries` — distance to the stroke polyline (or the
//!   step/curve expansion) within `lineWidth/2 + tolerance` (width 1 when `lineVisible` is
//!   off), point-marker discs within `radius + tolerance`, and the reference's single-visible-point
//!   horizontal segment. The area/baseline fills carry no hit test, exactly like reference
//!   (no renderer implements `hitTest` in v5.2).
//!
//! Cross-series arbitration ports the reference's `isBetterHit`: point-style hits beat strokes/ranges,
//! otherwise the smaller distance wins, and equal-distance non-point ties keep the paint
//! order (the caller walks topmost-first, so ties resolve to the topmost series).

use nucleuscharts_core::model::data_layer::SeriesId;
use nucleuscharts_core::model::plot_list::PlotValueIndex;
use nucleuscharts_render::draw_list::LineType;

use crate::frame::{pane_scale, series_scale_target};
use crate::{ChartEngine, SeriesKind};

/// reference `SeriesOptionsCommon.hitTestTolerance` default (series-options-defaults.ts:15).
const HIT_TEST_TOLERANCE: f64 = 3.0;

/// reference `HitTestPriority` (model/internal-hit-test.ts): the class a series hit belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeriesHitKind {
    /// Range-style hit (bar/candle/histogram interval).
    Range,
    /// Stroke-style hit (line segment).
    Line,
    /// Point-style hit (point marker or a single visible point).
    Point,
}

/// One series' hit-test outcome (reference `InternalHitTestCandidate` plus the series identity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeriesHit {
    pub series: SeriesId,
    /// Geometric distance from the cursor to the series geometry, in CSS px.
    pub distance: f64,
    pub kind: SeriesHitKind,
}

impl SeriesHit {
    /// Port of reference `isBetterHit`: a point hit beats any non-point hit; otherwise the smaller
    /// distance wins and equal-distance non-point ties lose (preserving the caller's
    /// topmost-first walk order).
    pub fn is_better_than(&self, current: &SeriesHit) -> bool {
        if self.kind == SeriesHitKind::Point && current.kind != SeriesHitKind::Point {
            return true;
        }
        if current.kind == SeriesHitKind::Point && self.kind != SeriesHitKind::Point {
            return false;
        }
        self.distance < current.distance
    }
}

/// reference `distanceToSegment` (renderers/hit-test-common.ts).
fn distance_to_segment(x: f64, y: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    if dx == 0.0 && dy == 0.0 {
        return (x - x1).hypot(y - y1);
    }
    let projection = ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy);
    let clamped = projection.clamp(0.0, 1.0);
    (x - (x1 + dx * clamped)).hypot(y - (y1 + dy * clamped))
}

/// reference `hitTestSeriesRange` (renderers/range-hit-test.ts): horizontal slots bounded by the
/// midpoints between adjacent items (`barSpacing/2` at the data edges), vertical range from
/// the item's `rangeProvider`; inside the range the distance is 0, within tolerance outside
/// it the distance to the nearer edge. `items` is `(x, geometry_time, range_start_y,
/// range_end_y)` per visible item, x ascending.
fn hit_test_series_range(
    items: &[(f64, i64, f64, f64)],
    x: f64,
    y: f64,
    bar_spacing: f64,
    tolerance: f64,
) -> Option<f64> {
    if items.is_empty() {
        return None;
    }
    let horizontal_radius = bar_spacing / 2.0 + tolerance;
    // lowerBoundByX/upperBoundByX over the x-ascending items.
    let candidate_from = items.partition_point(|item| item.0 < x - horizontal_radius);
    let candidate_to = items.partition_point(|item| item.0 <= x + horizontal_radius);
    if candidate_from >= candidate_to {
        return None;
    }
    let mut min_distance = f64::INFINITY;
    for index in candidate_from..candidate_to {
        let item = items[index];
        // NOTE: `then` (lazy), not `then_some` — the argument is eagerly evaluated there and
        // `items[index ± 1]` would panic at the edges of the visible set.
        let previous = (index > 0).then(|| items[index - 1]);
        let next = (index + 1 < items.len()).then(|| items[index + 1]);
        // slotStart/slotEnd: adjacent items (consecutive geometry keys) share their midpoint.
        let slot_start = match previous {
            Some(p) if p.1 == item.1 - 1 => (p.0 + item.0) / 2.0,
            _ => item.0 - bar_spacing / 2.0,
        } - tolerance;
        let slot_end = match next {
            Some(n) if n.1 == item.1 + 1 => (item.0 + n.0) / 2.0,
            _ => item.0 + bar_spacing / 2.0,
        } + tolerance;
        if x < slot_start || x > slot_end {
            continue;
        }
        let actual_top = item.2.min(item.3);
        let actual_bottom = item.2.max(item.3);
        if y >= actual_top && y <= actual_bottom {
            min_distance = min_distance.min(0.0);
            continue;
        }
        if y >= actual_top - tolerance && y <= actual_bottom + tolerance {
            min_distance = min_distance.min((y - actual_top).abs().min((actual_bottom - y).abs()));
        }
    }
    min_distance.is_finite().then_some(min_distance)
}

/// reference `getControlPoints` (renderers/walk-line.ts), tension 6.
fn control_points(points: &[(f64, f64)], from: usize, to: usize) -> [(f64, f64); 2] {
    const CURVE_TENSION: f64 = 6.0;
    let before_from = points[from.saturating_sub(1)];
    let after_to = points[(to + 1).min(points.len() - 1)];
    [
        (
            points[from].0 + (points[to].0 - before_from.0) / CURVE_TENSION,
            points[from].1 + (points[to].1 - before_from.1) / CURVE_TENSION,
        ),
        (
            points[to].0 - (after_to.0 - points[from].0) / CURVE_TENSION,
            points[to].1 - (after_to.1 - points[from].1) / CURVE_TENSION,
        ),
    ]
}

/// reference `distanceToBezierCurve` (renderers/line-hit-test.ts): 12-step polyline approximation.
fn distance_to_bezier_curve(x: f64, y: f64, points: [(f64, f64); 4]) -> f64 {
    const STEPS: usize = 12;
    let cubic = |p0: f64, p1: f64, p2: f64, p3: f64, t: f64| {
        let u = 1.0 - t;
        u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
    };
    let mut min_distance = f64::INFINITY;
    let mut previous = points[0];
    for step in 1..=STEPS {
        let t = step as f64 / STEPS as f64;
        let current = (
            cubic(points[0].0, points[1].0, points[2].0, points[3].0, t),
            cubic(points[0].1, points[1].1, points[2].1, points[3].1, t),
        );
        min_distance = min_distance.min(distance_to_segment(
            x, y, previous.0, previous.1, current.0, current.1,
        ));
        previous = current;
    }
    min_distance
}

/// reference `hitTestLineSeries` (renderers/line-hit-test.ts): point markers and the
/// single-visible-point segment report `Point`, stroke segments report `Line`. Nucleus's
/// `visible_line_rows` selection already produces the drawn item set, so the items here are
/// exactly the visible points (reference instead indexes into its full item list by the extended
/// visible range — same set at the pane edges Nucleus draws). Shared with the brush drawing tool's
/// body test (drawings.rs), whose smooth stroke is the same curved geometry.
#[allow(clippy::too_many_arguments)] // mirrors the reference renderer-data signature
pub(crate) fn hit_test_line_series(
    points: &[(f64, f64)],
    x: f64,
    y: f64,
    line_type: LineType,
    line_width: f64,
    point_markers_radius: Option<f64>,
    bar_spacing: f64,
    tolerance: f64,
) -> Option<(f64, SeriesHitKind)> {
    if points.is_empty() {
        return None;
    }
    let radius = (line_width / 2.0).max(point_markers_radius.unwrap_or(0.0)) + tolerance;
    let mut point_min_distance = f64::INFINITY;

    if let Some(markers_radius) = point_markers_radius {
        let point_radius = markers_radius + tolerance;
        for &(px, py) in points {
            // isWithinHorizontalSweep
            if x < px - point_radius || x > px + point_radius {
                continue;
            }
            let distance = (x - px).hypot(y - py);
            if distance <= point_radius {
                point_min_distance = point_min_distance.min(distance);
            }
        }
    }

    if points.len() < 2 {
        let (px, py) = points[0];
        let single_half_width = (bar_spacing / 2.0).max(radius);
        let distance =
            distance_to_segment(x, y, px - single_half_width, py, px + single_half_width, py);
        if distance <= radius {
            point_min_distance = point_min_distance.min(distance);
        }
        return point_min_distance
            .is_finite()
            .then_some((point_min_distance, SeriesHitKind::Point));
    }

    let mut line_min_distance = f64::INFINITY;
    for index in 1..points.len() {
        let first = points[index - 1];
        let second = points[index];
        // isWithinHorizontalSweep against the segment's horizontal bounds (with the control
        // points included for curves, as in reference).
        let (left, right) = match line_type {
            LineType::Curved => {
                let [cp1, cp2] = control_points(points, index - 1, index);
                (
                    first.0.min(second.0).min(cp1.0).min(cp2.0),
                    first.0.max(second.0).max(cp1.0).max(cp2.0),
                )
            }
            _ => (first.0.min(second.0), first.0.max(second.0)),
        };
        if x < left - radius || x > right + radius {
            continue;
        }
        let distance = match line_type {
            LineType::WithSteps => distance_to_segment(x, y, first.0, first.1, second.0, first.1)
                .min(distance_to_segment(
                    x, y, second.0, first.1, second.0, second.1,
                )),
            LineType::Curved => {
                let [cp1, cp2] = control_points(points, index - 1, index);
                distance_to_bezier_curve(x, y, [first, cp1, cp2, second])
            }
            _ => distance_to_segment(x, y, first.0, first.1, second.0, second.1),
        };
        if distance <= radius {
            line_min_distance = line_min_distance.min(distance);
        }
    }

    if point_min_distance.is_finite() {
        return Some((point_min_distance, SeriesHitKind::Point));
    }
    line_min_distance
        .is_finite()
        .then_some((line_min_distance, SeriesHitKind::Line))
}

impl ChartEngine {
    /// The stacked pane containing chart-top-relative media y `y_css`, or `None` when it
    /// falls between panes, on the time-axis strip, or off the chart. Hit testing is
    /// restricted to the pane under the cursor (reference hit-tests only the hovered pane widget).
    pub fn pane_at_y(&self, y_css: f64) -> Option<usize> {
        if !y_css.is_finite() {
            return None;
        }
        self.panes
            .iter()
            .position(|pane| y_css >= pane.top && y_css <= pane.top + pane.height)
    }

    /// Hit-test one series at pane-relative media px `(x_css, y_css)` (x from the pane's left
    /// edge, y from the chart's top), independent of any other series. `None` for a hidden,
    /// removed, pane-less, or unscaled series, or a miss. Per-kind geometry and tolerances
    /// are the reference ports documented at the module level.
    pub fn hit_test_one_series(&self, id: SeriesId, x_css: f64, y_css: f64) -> Option<SeriesHit> {
        if !x_css.is_finite() || !y_css.is_finite() {
            return None;
        }
        let series = self.series_entry(id)?;
        // reference gates a series' pane-view hit test on visibility (series-pane-view-base.ts).
        if !series.visible || series.pane_index >= self.panes.len() {
            return None;
        }
        let (from, to) = self.visible_range_for_frame()?;
        let base_value = self.series_base_value(id, from)?;
        let scale = pane_scale(&self.panes[series.pane_index], series_scale_target(series));
        if scale.is_empty() {
            return None;
        }
        let plot = self.data.plot(id);
        let bar_spacing = self.time_scale.bar_spacing();
        // The frame build's horizontal ratio (frame/mod.rs): conflation buckets in physical
        // pixels, so the tested items must be selected with the same ratio they were drawn.
        let hpr = (self.pane_w * self.dpr.max(0.01)).round().max(1.0) / self.pane_w.max(1.0);
        let result = match series.kind {
            SeriesKind::Candlestick | SeriesKind::Bar => {
                let items = crate::frame::conflation::visible_ohlc(
                    plot,
                    from,
                    to,
                    bar_spacing,
                    hpr,
                    |index| self.time_scale.index_to_coordinate(index) * hpr,
                )
                .into_iter()
                .map(|bar| {
                    (
                        bar.x_px / hpr,
                        bar.geometry_time,
                        scale.price_to_coordinate(bar.high, base_value),
                        scale.price_to_coordinate(bar.low, base_value),
                    )
                })
                .collect::<Vec<_>>();
                hit_test_series_range(&items, x_css, y_css, bar_spacing, HIT_TEST_TOLERANCE)
                    .map(|distance| (distance, SeriesHitKind::Range))
            }
            SeriesKind::Histogram => {
                let base_y = scale.price_to_coordinate(series.base, base_value);
                let close = plot.column(PlotValueIndex::Close);
                let items = crate::frame::conflation::visible_histogram_rows(
                    plot,
                    from,
                    to,
                    bar_spacing,
                    hpr,
                    |index| self.time_scale.index_to_coordinate(index) * hpr,
                )
                .into_iter()
                .map(|item| {
                    (
                        item.x_px / hpr,
                        item.geometry_time,
                        scale.price_to_coordinate(close[item.source_row], base_value),
                        base_y,
                    )
                })
                .collect::<Vec<_>>();
                hit_test_series_range(&items, x_css, y_css, bar_spacing, HIT_TEST_TOLERANCE)
                    .map(|distance| (distance, SeriesHitKind::Range))
            }
            SeriesKind::Line | SeriesKind::Area | SeriesKind::Baseline => {
                let close = plot.column(PlotValueIndex::Close);
                let points = crate::frame::conflation::visible_line_rows(
                    plot,
                    from,
                    to,
                    bar_spacing,
                    hpr,
                    |index| self.time_scale.index_to_coordinate(index) * hpr,
                )
                .into_iter()
                .map(|row| {
                    (
                        self.time_scale
                            .index_to_coordinate(plot.index_at(row).expect("hit-test row index")),
                        scale.price_to_coordinate(close[row], base_value),
                    )
                })
                .collect::<Vec<_>>();
                // reference line-hit-test-pane-view-base.ts: width 1 when the stroke is hidden;
                // point markers join with their resolved radius (default lineWidth/2 + 2).
                let line_width = if series.line_visible {
                    series.line_width.unwrap_or(crate::frame::LINE_WIDTH)
                } else {
                    1.0
                };
                let markers_radius = series.point_markers.then(|| {
                    series
                        .point_markers_radius
                        .unwrap_or(line_width / 2.0 + 2.0)
                });
                hit_test_line_series(
                    &points,
                    x_css,
                    y_css,
                    series.line_type,
                    line_width,
                    markers_radius,
                    bar_spacing,
                    HIT_TEST_TOLERANCE,
                )
            }
            // A custom series' geometry is plugin-defined; the engine has no built-in hit for
            // it (the reference's renderer-level `hitTest` is out of scope of the host contract).
            SeriesKind::Custom => None,
        };
        result.map(|(distance, kind)| SeriesHit {
            series: id,
            distance,
            kind,
        })
    }

    /// The series under pane-relative media px `(x_css, y_css)` (reference
    /// `MouseEventParams.hoveredSeries`), or `None` off the panes/data. Walks the stable
    /// paint order topmost-first (reference deliberately does NOT use the temporary
    /// hovered-on-top render order for arbitration, pane-hit-test.ts) and arbitrates
    /// competing hits with [`SeriesHit::is_better_than`].
    pub fn hit_test_series(&self, x_css: f64, y_css: f64) -> Option<SeriesId> {
        let pane = self.pane_at_y(y_css)?;
        let mut best: Option<SeriesHit> = None;
        for &id in self.series_order.iter().rev() {
            if self
                .series_entry(id)
                .is_none_or(|series| series.pane_index != pane)
            {
                continue;
            }
            let Some(hit) = self.hit_test_one_series(id, x_css, y_css) else {
                continue;
            };
            if best.is_none_or(|current| hit.is_better_than(&current)) {
                best = Some(hit);
            }
        }
        best.map(|hit| hit.series)
    }

    /// The hovered series driving the `hoveredSeriesOnTop` render z-bump (reference
    /// `ChartModel.hoveredSource`). Hosts refresh it from their hover pipeline; a removed
    /// id never sticks.
    pub fn set_hovered_series(&mut self, id: Option<SeriesId>) {
        let next = id.filter(|&sid| self.series.iter().any(|s| s.id == sid && !s.removed));
        if self.hovered_series != next {
            self.hovered_series = next;
            // Hover-on-top changes only retained-layer assembly order; per-series geometry stays
            // valid and is reassembled in the new order by every canonical frame build.
        }
    }

    pub fn hovered_series(&self) -> Option<SeriesId> {
        self.hovered_series
    }

    /// The selected series the frame build paints anchor points on (TradingView-style
    /// click-to-select). Hosts refresh it from their click pipeline (empty-space clicks pass
    /// `None`); a removed id never sticks.
    pub fn set_selected_series(&mut self, id: Option<SeriesId>) {
        let next = id.filter(|&sid| self.series.iter().any(|s| s.id == sid && !s.removed));
        if self.selected_series != next {
            self.selected_series = next;
            self.invalidate_frame_overlay();
        }
    }

    pub fn selected_series(&self) -> Option<SeriesId> {
        self.selected_series
    }
}

#[cfg(test)]
mod tests;
