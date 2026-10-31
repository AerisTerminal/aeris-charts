//! Drawing-tool frame emission (model in drawings.rs): each pane's committed drawings in
//! z-order, the selected drawing's anchor handles, and the interactive-creation preview.
//!
//! Coordinates follow the frame build's conventions (frame/mod.rs): x is pane-local media px
//! scaled by the exact horizontal ratio (the trailing `translate_prims_x` shifts everything
//! when a left axis reserves space), y is chart-top-relative media px scaled by the vertical
//! ratio — the same space the price-line/series geometry uses, so a drawing's prims land
//! exactly on its converted anchors. Standalone drawing text uses `Prim::Text`; line labels
//! use the backend-neutral `Prim::RotatedText` contract so every executor receives the same
//! aligned anchor and line-normalized angle. A label that sits on its own stroke (a line's
//! middle slot, a centered level label) cuts the stroke under its padded box
//! (`text_box_cutout`), so every kind opens the same gap around its text.

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, ImageSampling, LineStyle, LineType, Prim, TextAlign};
use std::fmt::Write;

use super::{POSITION_ENTRY, PRIMARY};
use crate::ChartEngine;
use crate::drawings::{
    Drawing, DrawingBodyGeometry, DrawingGeometryOptions, DrawingHandleMode, DrawingId,
    DrawingKind, DrawingPoint, DrawingTextHAlign, LINE_TEXT_PLACEHOLDER, MEASURE_LABEL_GAP,
    MeasureAxes, MeasureGeometry, PositionGeometry, PositionZone, TEXT_CHROME_PAD, TEXT_PAD,
    TextBlock, TextRun, arc_segments, arrow_marker_icon, builtin_icon, drawing_text_weight,
    ellipse_segments, gann_arc_segments, path_arrow_points, resolve_drawing_geometry,
};
use aeris_charts_core::model::plot_list::PlotValueIndex;

/// Drawing handles (`push_handle`): a theme-derived fill inside a primary-token border, round or
/// square. The fill radius plus border makes a 12 px handle, larger than the series selection
/// anchors (series_geometry.rs) since these are drag targets. The 2 px ring rounds to whole device
/// pixels so it reads as a solid outline at every density: 2 px at 1x, 3 px at 1.5x, 4 px at 2x.
const ANCHOR_RADIUS: f64 = 4.0;
const ANCHOR_BORDER_WIDTH: f64 = 2.0;
/// Square handles keep slightly rounded corners.
const ANCHOR_SQUARE_RADIUS: f64 = 2.0;
const ANCHOR_BORDER: Color = PRIMARY;
const MARQUEE_FILL: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x1F);
const MARQUEE_EDGE: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0xB3);
/// Opacity of a volume profile's box and spine over the chart text color.
const PROFILE_FRAME_ALPHA: u8 = 80;
const POSITION_ENTRY_WIDTH_CSS: f64 = 0.5;
const POSITION_ZONE_ALPHA: u8 = 70;
/// Progress must read as the emphasized portion of either semantic side on its own. Keep this
/// above the base-zone alpha instead of relying on a second lower-alpha pass to become visible
/// only through accidental compositing.
const POSITION_PROGRESS_ALPHA: u8 = 96;
/// The hover ring's dimmed variant of the focus border (the public reference shows the same border at
/// roughly half strength until the drawing is actually selected).
const HOVER_BORDER: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x73);
/// The painted text selection: a translucent primary wash over the selected glyphs.
pub(super) const TEXT_SELECTION: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x59);
const LINE_TEXT_PLACEHOLDER_ALPHA: u8 = 0x99;

fn point_on_segment(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

const fn text_align(align: DrawingTextHAlign) -> TextAlign {
    match align {
        DrawingTextHAlign::Left => TextAlign::Left,
        DrawingTextHAlign::Center => TextAlign::Center,
        DrawingTextHAlign::Right => TextAlign::Right,
    }
}

/// A one-line text run's padded box around its aligned anchor, in the run's own axes: the
/// advance extent `(start, end)` along the run and the extent across it (the 1.2em line box).
fn padded_text_box(align: TextAlign, width: f64, size: f64, pad: f64) -> ((f64, f64), (f64, f64)) {
    let run = match align {
        TextAlign::Left => (-pad, width + pad),
        TextAlign::Center => (-width / 2.0 - pad, width / 2.0 + pad),
        TextAlign::Right => (-width - pad, pad),
    };
    let half_height = size * crate::drawings::LINE_HEIGHT_EM / 2.0 + pad;
    (run, (-half_height, half_height))
}

/// A label block's padded box in its run's axes, like [`padded_text_box`] for every line.
fn padded_block_box(
    block: &TextBlock<'_>,
    align: DrawingTextHAlign,
    pad: f64,
) -> ((f64, f64), (f64, f64)) {
    let (left, top) = (block.left(align), block.top());
    (
        (left - pad, left + block.width + pad),
        (top - pad, top + block.height() + pad),
    )
}

/// The stretch of segment `a→b`, as parameters in `[0, 1]`, inside a label's padded box. The
/// box is anchored at `anchor`, rotated by `angle`, and spans `run` along the label and
/// `across` its normal. A stroke a label sits on opens over this stretch, so text never
/// crosses its own line whatever the drawing kind or the line's direction.
fn text_box_cutout(
    a: (f64, f64),
    b: (f64, f64),
    anchor: (f64, f64),
    angle: f64,
    (run, across): ((f64, f64), (f64, f64)),
) -> Option<(f64, f64)> {
    let (sin, cos) = angle.sin_cos();
    let local = |point: (f64, f64)| {
        let (dx, dy) = (point.0 - anchor.0, point.1 - anchor.1);
        (dx * cos + dy * sin, -dx * sin + dy * cos)
    };
    let (from, to) = (local(a), local(b));
    let (mut start, mut end) = (0.0_f64, 1.0_f64);
    for (coordinate, delta, low, high) in [
        (from.0, to.0 - from.0, run.0, run.1),
        (from.1, to.1 - from.1, across.0, across.1),
    ] {
        if delta.abs() <= f64::EPSILON {
            if coordinate < low || coordinate > high {
                return None;
            }
        } else {
            let (enter, exit) = ((low - coordinate) / delta, (high - coordinate) / delta);
            start = start.max(enter.min(exit));
            end = end.min(enter.max(exit));
        }
    }
    (start < end).then_some((start, end))
}

/// Segment `a→b` with the parameter stretch `cutout` removed: the whole segment, or the
/// pieces either side of the cut.
fn cut_segment(
    a: (f64, f64),
    b: (f64, f64),
    cutout: Option<(f64, f64)>,
) -> impl Iterator<Item = ((f64, f64), (f64, f64))> {
    let pieces = match cutout {
        Some((start, end)) => [
            Some((a, point_on_segment(a, b, start))),
            Some((point_on_segment(a, b, end), b)),
        ],
        None => [Some((a, b)), None],
    };
    pieces.into_iter().flatten()
}

fn push_segment(
    a: (f64, f64),
    b: (f64, f64),
    drawing: &Drawing,
    color: Color,
    vpr: f64,
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
) {
    if (a.0 - b.0).abs() <= f64::EPSILON && (a.1 - b.1).abs() <= f64::EPSILON {
        return;
    }
    let first_point = points.len() as u32;
    points.extend([[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]]);
    out.push(Prim::Polyline {
        first_point,
        point_count: 2,
        width: (drawing.width * vpr) as f32,
        style: drawing.style,
        line_type: LineType::Simple,
        color,
    });
}

/// A crisp dashed axis-aligned outline through two opposite corners.
fn push_dashed_outline(a: (f64, f64), b: (f64, f64), color: Color, vpr: f64, out: &mut Vec<Prim>) {
    let width = vpr.round().max(1.0) as i32;
    let (left, right) = (a.0.min(b.0).round() as i32, a.0.max(b.0).round() as i32);
    let (top, bottom) = (a.1.min(b.1).round() as i32, a.1.max(b.1).round() as i32);
    for y in [top, bottom] {
        out.push(Prim::HLine {
            y,
            x0: left,
            x1: right,
            width,
            style: LineStyle::Dashed,
            color,
        });
    }
    for x in [left, right] {
        out.push(Prim::VLine {
            x,
            y0: top,
            y1: bottom,
            width,
            style: LineStyle::Dashed,
            color,
        });
    }
}

/// A line end decoration at `endpoint` for a stroke arriving from `toward`. `line_width` is the
/// drawing's CSS width and `width` the same stroke in device px.
#[allow(clippy::too_many_arguments)] // the cap, its segment, the stroke, and the output pools
fn push_drawing_cap(
    cap: crate::DrawingLineCap,
    endpoint: (f64, f64),
    toward: (f64, f64),
    line_width: f64,
    vpr: f64,
    color: Color,
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
) {
    if cap == crate::DrawingLineCap::None
        || (toward.0 - endpoint.0).hypot(toward.1 - endpoint.1) <= f64::EPSILON
    {
        return;
    }
    let width = line_width * vpr;
    let radius = (width * 1.75).max(3.0);
    match cap {
        crate::DrawingLineCap::Circle => out.push(Prim::Circle {
            cx: endpoint.0 as f32,
            cy: endpoint.1 as f32,
            radius: radius as f32,
            fill: color,
            stroke_width: 0.0,
            stroke: color,
        }),
        // The Path tool's open chevron (`path_arrow_points`), stroked like the line.
        crate::DrawingLineCap::Arrow => {
            let Some(wings) = path_arrow_points(&[toward, endpoint], line_width, vpr) else {
                return;
            };
            let first_point = points.len() as u32;
            points.extend(wings.map(|(x, y)| [x as f32, y as f32]));
            out.push(Prim::Polyline {
                first_point,
                point_count: 3,
                width: width as f32,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color,
            });
        }
        crate::DrawingLineCap::None => {}
    }
}

#[cfg(test)]
mod trend_label_tests {
    use super::*;
    use crate::drawings::{DrawingPoint, DrawingTextVAlign};

    #[test]
    fn all_nine_trend_label_positions_follow_the_segment() {
        let mut drawing = Drawing::new(
            1,
            DrawingKind::TrendLine,
            0,
            vec![
                DrawingPoint {
                    logical: 0.0,
                    price: 0.0,
                },
                DrawingPoint {
                    logical: 1.0,
                    price: 1.0,
                },
            ],
        );
        for line in [
            [(10.0, 50.0), (90.0, 50.0)],
            [(10.0, 80.0), (90.0, 20.0)],
            [(50.0, 90.0), (50.0, 10.0)],
            [(90.0, 20.0), (10.0, 80.0)],
        ] as [[(f64, f64); 2]; 4]
        {
            let mut start = line[0];
            let mut end = line[1];
            if end.0 < start.0 || ((end.0 - start.0).abs() <= f64::EPSILON && end.1 > start.1) {
                std::mem::swap(&mut start, &mut end);
            }
            let dx: f64 = end.0 - start.0;
            let dy: f64 = end.1 - start.1;
            let length = dx.hypot(dy);
            let (ux, uy) = (dx / length, dy / length);
            for (h_align, expected_distance) in [
                (DrawingTextHAlign::Left, 4.0),
                (DrawingTextHAlign::Center, length / 2.0),
                (DrawingTextHAlign::Right, length - 4.0),
            ] {
                drawing.text_h_align = h_align;
                for (v_align, expected_normal) in [
                    (DrawingTextVAlign::Top, 11.2),
                    (DrawingTextVAlign::Middle, 0.0),
                    (DrawingTextVAlign::Bottom, -11.2),
                ] {
                    drawing.text_v_align = v_align;
                    let (x, y, align, angle) = ChartEngine::drawing_text_placement(
                        &drawing, &line, 100.0, 0.0, 100.0, 12.0, 4.0,
                    );
                    assert_eq!(align, h_align);
                    let (from_x, from_y) = (x - start.0, y - start.1);
                    assert!((from_x * ux + from_y * uy - expected_distance).abs() < 1e-9);
                    assert!((from_x * uy - from_y * ux - expected_normal).abs() < 1e-9);
                    assert!(
                        (-std::f64::consts::FRAC_PI_2..=std::f64::consts::FRAC_PI_2)
                            .contains(&angle)
                    );
                }
            }
        }

        drawing.text_h_align = DrawingTextHAlign::Right;
        drawing.text_v_align = DrawingTextVAlign::Middle;
        let line = [(10.0, 80.0), (90.0, 20.0)];
        let reversed = [line[1], line[0]];
        let (x, y, _, angle) =
            ChartEngine::drawing_text_placement(&drawing, &reversed, 100.0, 0.0, 100.0, 12.0, 4.0);
        assert!((x - 86.8).abs() < 1e-9);
        assert!((y - 22.4).abs() < 1e-9);
        assert!((angle - (-0.6_f64).atan2(0.8)).abs() < 1e-9);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PositionRunSide {
    Reward,
    Risk,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PositionRunProgress {
    pub(super) start: crate::drawings::DrawingPoint,
    pub(super) point: crate::drawings::DrawingPoint,
    pub(super) side: PositionRunSide,
    pub(super) closed: bool,
}

impl ChartEngine {
    fn drawing_level_style(style: &str) -> LineStyle {
        match style {
            "dotted" | "sparse_dotted" => LineStyle::Dotted,
            "dashed" | "large_dashed" => LineStyle::Dashed,
            _ => LineStyle::Solid,
        }
    }

    fn drawing_level_fill(level: &crate::DrawingLevel, fallback: Color) -> Color {
        let base = Color::parse_css(&level.color).unwrap_or(fallback);
        level
            .fill_color
            .as_deref()
            .and_then(Color::parse_css)
            .unwrap_or(Color::rgba(base.r(), base.g(), base.b(), 35))
    }

    fn drawing_level_price_at(&self, drawing: &Drawing, y: f64, vpr: f64) -> Option<f64> {
        let scale = self.drawing_scale_for(drawing.pane_index, drawing.price_scale)?;
        let price = scale.coordinate_to_price(
            y / vpr,
            self.drawing_scale_base_for(drawing.pane_index, drawing.price_scale),
        );
        price.is_finite().then_some(price)
    }

    fn drawing_level_label(
        &self,
        drawing: &Drawing,
        value: f64,
        price: Option<f64>,
    ) -> Option<String> {
        let mut label = String::new();
        if drawing.level_show_values {
            write!(label, "{value}").expect("formatting a String cannot fail");
        }
        if drawing.level_show_percents {
            if !label.is_empty() {
                label.push_str(" · ");
            }
            write!(label, "{:.1}%", value * 100.0).expect("formatting a String cannot fail");
        }
        if drawing.level_show_prices
            && let Some(price) = price.filter(|price| price.is_finite())
        {
            if !label.is_empty() {
                label.push_str(" · ");
            }
            label.push_str(&self.format_drawing_price(drawing, price));
        }
        (!label.is_empty()).then_some(label)
    }

    fn drawing_level_align(drawing: &Drawing) -> TextAlign {
        match drawing.level_label_align.as_str() {
            "left" => TextAlign::Left,
            "center" => TextAlign::Center,
            _ => TextAlign::Right,
        }
    }
    fn drawing_frame_text<'a>(&self, drawing: &'a Drawing) -> Option<(&'a str, bool)> {
        if drawing.kind == DrawingKind::PriceLabel {
            return None;
        }
        if !drawing.text.is_empty() {
            return Some((drawing.display_text(), false));
        }
        (self.line_text_prompt() == Some(drawing.id)).then_some((LINE_TEXT_PLACEHOLDER, true))
    }

    /// Width source for the middle-line cutout. Selection reserves the full prompt; once editing
    /// begins, an empty value uses the editor's one-em caret opening and measured text expands it.
    fn drawing_frame_gap_text<'a>(&self, drawing: &'a Drawing) -> Option<&'a str> {
        if self.editing_drawing == Some(drawing.id) {
            return Some(drawing.display_text());
        }
        if !drawing.text.is_empty() {
            return Some(drawing.display_text());
        }
        (self.line_text_prompt() == Some(drawing.id)).then_some(LINE_TEXT_PLACEHOLDER)
    }

    /// The stretch of a line's stroke `a→b` (bitmap px) that its middle-aligned label covers,
    /// so the stroke opens around the text. Lines without line text or with the label above or
    /// below the stroke stay whole. A rectangle's middle-aligned label sits on its middle line,
    /// which opens the same way.
    fn line_text_cutout(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        a: (f64, f64),
        b: (f64, f64),
    ) -> Option<(f64, f64)> {
        if !(drawing.kind.has_line_text() || drawing.kind == DrawingKind::Rectangle)
            || drawing.text_v_align != crate::drawings::DrawingTextVAlign::Middle
        {
            return None;
        }
        let text = self.drawing_frame_gap_text(drawing)?;
        let run = self.text_run_geometry(drawing, px, pane_w_px, vpr)?;
        let family = &self.options.get().layout.font_family;
        let mut block = self.drawing_text_block(drawing, text, run.size, run.v_align, family);
        if self.editing_drawing == Some(drawing.id) {
            // Match the host editor's one-em minimum width. This leaves a compact caret slot
            // and then grows from actual shaped advance.
            block.width = block.width.max(run.size);
        }
        text_box_cutout(
            a,
            b,
            (run.x, run.y),
            run.angle,
            padded_block_box(&block, run.align, TEXT_PAD * vpr),
        )
    }

    #[cfg(test)]
    pub(crate) fn build_drawings_frame_reference(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        for drawing in &self.drawings {
            if drawing.pane_index != pane_index
                || !drawing.visible
                || !drawing.interval_visibility.allows(self.drawing_interval)
                || !self.drawing_viewport_candidate_reference(drawing)
            {
                continue;
            }
            let Some(px) = self.drawing_render_px(drawing) else {
                continue;
            };
            let px = px
                .into_iter()
                .map(|(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>();
            self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
            self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
            self.build_drawing_text_session(drawing, &px, pane_w_px, vpr, out, points);
            self.build_drawing_labels(drawing, &px, vpr, out);
        }
    }

    /// Segmented committed build for retained reassembly: stable z-order committed drawings,
    /// recording each emitted drawing's prim/point range in `parts` (stable z-order) and the
    /// trailing preview (brush + pending) start in `preview_start` (prim, point). Previews
    /// always trail committed so assembly can place them topmost among chart content.
    /// Ordering.rs reassembles these parts idle-below / active-above without rebuilding
    /// geometry on hover/selection. Drawings on stale panes draw nowhere.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_drawings_frame_segmented(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        parts: &mut Vec<super::RetainedDrawingPart>,
        preview_start: &mut (usize, usize),
    ) {
        parts.clear();
        if self.drawing_runtime.borrow().pane_count(pane_index) <= 20 {
            for drawing in &self.drawings {
                if drawing.pane_index != pane_index
                    || !drawing.visible
                    || !drawing.interval_visibility.allows(self.drawing_interval)
                {
                    continue;
                }
                let px = if drawing.kind == DrawingKind::RegressionTrend {
                    self.drawing_coordinate_key(drawing).and_then(|key| {
                        let mut runtime = self.drawing_runtime.borrow_mut();
                        self.drawing_px_cached(drawing, &mut runtime, key)
                            .map(<[(f64, f64)]>::to_vec)
                    })
                } else {
                    self.drawing_render_px(drawing)
                };
                let Some(px) = px else {
                    continue;
                };
                let px = px
                    .into_iter()
                    .map(|(x, y)| (x * hpr, y * vpr))
                    .collect::<Vec<_>>();
                let prim_start = out.len();
                let point_start = points.len();
                self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
                self.build_drawing_text_session(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_labels(drawing, &px, vpr, out);
                parts.push(super::RetainedDrawingPart {
                    id: drawing.id,
                    prim_start,
                    prim_end: out.len(),
                    point_start,
                    point_end: points.len(),
                });
            }
        } else {
            let candidates = self.take_drawing_candidates(pane_index, None);
            let mut runtime = self.drawing_runtime.borrow_mut();
            for &id in &candidates {
                let Some(position) = runtime.position(id) else {
                    continue;
                };
                let Some(drawing) = self.drawings.get(position) else {
                    continue;
                };
                if !drawing.visible || !drawing.interval_visibility.allows(self.drawing_interval) {
                    continue;
                }
                let Some(key) = self.drawing_coordinate_key(drawing) else {
                    continue;
                };
                let Some(px) = self.drawing_px_cached(drawing, &mut runtime, key) else {
                    continue;
                };
                let px = px
                    .iter()
                    .map(|&(x, y)| (x * hpr, y * vpr))
                    .collect::<Vec<_>>();
                let prim_start = out.len();
                let point_start = points.len();
                self.build_drawing_prims(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_text(drawing, &px, pane_w_px, vpr, out);
                self.build_drawing_text_session(drawing, &px, pane_w_px, vpr, out, points);
                self.build_drawing_labels(drawing, &px, vpr, out);
                parts.push(super::RetainedDrawingPart {
                    id: drawing.id,
                    prim_start,
                    prim_end: out.len(),
                    point_start,
                    point_end: points.len(),
                });
                runtime.record_visible();
            }
            drop(runtime);
            self.recycle_drawing_candidates(candidates);
        }
        *preview_start = (out.len(), points.len());
        // Live brush stroke: the decimated points so far paint as the same smooth curve the
        // commit will store, so what the user sees while dragging is what they get.
        if let Some(capture) = self.brush_capture()
            && capture.pane_index == pane_index
            && capture.points.len() >= 2
        {
            let px: Option<Vec<(f64, f64)>> = capture
                .points
                .iter()
                .map(|&point| self.drawing_to_px(pane_index, point))
                .collect();
            if let Some(px) = px {
                let px: Vec<(f64, f64)> = px.into_iter().map(|(x, y)| (x * hpr, y * vpr)).collect();
                self.build_drawing_prims(&capture.options, &px, pane_w_px, vpr, out, points);
            }
        }
        // Interactive creation: committed anchors plus the preview point render as a tentative
        // drawing, with handles on the committed anchors (the reference rectangle-drawing-tool's
        // PreviewRectangle — same geometry, shown while placing).
        if let Some(pending) = self.pending_drawing()
            && pending.drawing.pane_index == pane_index
        {
            let mut anchors = pending.drawing.points.clone();
            let is_sequence = pending.drawing.kind.spec().placement.is_sequence();
            if let Some(preview) = pending.preview
                && (is_sequence || anchors.len() < pending.drawing.kind.anchor_count())
            {
                anchors.push(preview);
            }
            let ready = if is_sequence {
                anchors.len() >= pending.drawing.kind.anchor_count()
            } else {
                anchors.len() == pending.drawing.kind.anchor_count()
            };
            if ready {
                let mut preview_drawing = pending.drawing.clone();
                preview_drawing.points = anchors.clone();
                let px = self.drawing_render_px(&preview_drawing);
                if let Some(px) = px {
                    let px: Vec<(f64, f64)> =
                        px.into_iter().map(|(x, y)| (x * hpr, y * vpr)).collect();
                    // Semantic statistics (measure direction, labels) read the full
                    // placed-plus-preview anchor set, exactly as the commit will store it.
                    let handles = pending.drawing.kind.spec().handles;
                    if handles == DrawingHandleMode::RectangleBounds
                        && let Some(fill) = preview_drawing.preview_fill_color.clone()
                    {
                        preview_drawing.fill_color = Some(fill);
                    }
                    self.build_drawing_prims(&preview_drawing, &px, pane_w_px, vpr, out, points);
                    if handles.bounds_slots().is_some() && px.len() == 2 {
                        // the public reference shows all bounds anchors while the shape is being
                        // drawn (committed corner + live preview corner), not only after
                        // the commit.
                        build_rectangle_handles(&px, handles, vpr, self.anchor_fill(), out);
                    } else {
                        let committed = pending.drawing.points.len();
                        let handles = self.drawing_anchor_handles(&preview_drawing, &px, hpr, vpr);
                        build_anchor_handles(
                            &handles[..committed.min(handles.len())],
                            vpr,
                            self.anchor_fill(),
                            out,
                        );
                    }
                }
            } else if anchors.len() >= 2 {
                // A multi-anchor kind between clicks: its legs so far (placed anchors plus the
                // cursor) paint live in the drawing's own stroke, so the user sees the shape
                // build up; the full geometry takes over once every anchor is known. The bars
                // pattern's first two anchors bound its source window, which shows as the same
                // dashed outline the selection uses.
                let px: Option<Vec<(f64, f64)>> = anchors
                    .iter()
                    .map(|&point| {
                        self.drawing_to_px_for(pane_index, pending.drawing.price_scale, point)
                            .map(|(x, y)| (x * hpr, y * vpr))
                    })
                    .collect();
                if let Some(px) = px {
                    let drawing = &pending.drawing;
                    let color = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
                    if drawing.kind == DrawingKind::BarsPattern {
                        push_dashed_outline(px[0], px[1], color, vpr, out);
                    } else {
                        let first_point = points.len() as u32;
                        points.extend(px.iter().map(|&(x, y)| [x as f32, y as f32]));
                        out.push(Prim::Polyline {
                            first_point,
                            point_count: px.len() as u32,
                            width: (drawing.width * vpr) as f32,
                            style: drawing.style,
                            line_type: LineType::Simple,
                            color,
                        });
                    }
                    let committed = drawing.points.len().min(px.len());
                    build_anchor_handles(&px[..committed], vpr, self.anchor_fill(), out);
                }
            } else if anchors.len() == 1 {
                // A one-anchor kind awaiting its click, or a two-anchor kind before the
                // preview resolves: show the placed anchor as a handle alone.
                if let Some((x, y)) =
                    self.drawing_to_px_for(pane_index, pending.drawing.price_scale, anchors[0])
                {
                    build_anchor_handles(&[(x * hpr, y * vpr)], vpr, self.anchor_fill(), out);
                }
            }
        }
        // The transient Shift-click measure paints the date-and-price range geometry without
        // handles; it is never a committed drawing.
        if let Some(session) = self.measure_session()
            && session.drawing.pane_index == pane_index
            && let Some(px) = self.drawing_px(&session.drawing)
        {
            let px: Vec<(f64, f64)> = px.into_iter().map(|(x, y)| (x * hpr, y * vpr)).collect();
            self.build_drawing_prims(&session.drawing, &px, pane_w_px, vpr, out, points);
        }
    }

    /// The selected/hovered drawing's converted bitmap-px render points (semantic anchors, then
    /// derived points), or `None` when the id is stale, on another pane, or off-screen.
    fn overlay_drawing_px(
        &self,
        pane_index: usize,
        id: DrawingId,
        hpr: f64,
        vpr: f64,
    ) -> Option<Vec<(f64, f64)>> {
        let drawing = self.drawing(id)?;
        if drawing.pane_index != pane_index
            || !drawing.visible
            || !drawing.interval_visibility.allows(self.drawing_interval)
        {
            return None;
        }
        let key = self.drawing_coordinate_key(drawing)?;
        let mut runtime = self.drawing_runtime.borrow_mut();
        let px = self.drawing_px_cached(drawing, &mut runtime, key)?;
        Some(
            px.iter()
                .map(|&(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>(),
        )
    }

    /// Selection chrome is retained with the overlay, so selection-only changes do not
    /// invalidate or reconstruct unrelated drawing geometry. The text tool gets no anchor
    /// handles (the public reference: text has no drag points) — its selection affordance is the focus
    /// border alone. That border STAYS painted while the host typing-mode editor is open
    /// (the wrap is borderless; only the caret overlays), so entering/leaving edit cannot
    /// shift the outline.
    pub(super) fn build_selected_drawing_handles_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        // The primary paints last, so its handles sit above overlapping selected drawings.
        let primary = self.selected_drawing;
        for &id in self
            .selected_drawings
            .iter()
            .filter(|&&id| Some(id) != primary)
            .chain(primary.iter())
        {
            self.build_drawing_selection_frame(pane_index, id, pane_w_px, hpr, vpr, out, points);
        }
    }

    /// The Ctrl/Cmd selection box while it is being dragged: a translucent fill with a crisp
    /// border on whole device pixels, above every drawing in its pane.
    pub(super) fn build_drawing_marquee_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some([left, top, right, bottom]) = self
            .drawing_marquee
            .filter(|marquee| marquee.pane == pane_index)
            .and_then(|marquee| marquee.bounds())
        else {
            return;
        };
        let (x0, y0) = ((left * hpr).round() as i32, (top * vpr).round() as i32);
        let (x1, y1) = ((right * hpr).round() as i32, (bottom * vpr).round() as i32);
        let rect = IRect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(1),
            h: (y1 - y0).max(1),
        };
        out.push(Prim::Rect {
            rect,
            color: MARQUEE_FILL,
        });
        out.push(Prim::RectFrame {
            rect,
            border: hpr.min(vpr).round().max(1.0) as i32,
            color: MARQUEE_EDGE,
        });
    }

    /// Drawings on `pane_index` whose painted geometry touches the media-px box
    /// `[left, top, right, bottom]` (x pane-relative, y chart-top), topmost first. The test runs
    /// against the same primitives the frame paints, so every kind is matched by exactly what
    /// the user sees: a stroke or filled area crossing the box selects it, while a box inside
    /// an unfilled outline does not. Hidden drawings never match; locked drawings do.
    pub(crate) fn drawings_touching_box(
        &self,
        pane_index: usize,
        bounds: [f64; 4],
    ) -> Vec<DrawingId> {
        let pane_w_px = self.pane_w.round().max(1.0) as i32;
        let mut prims = Vec::new();
        let mut points = Vec::new();
        let mut touched = Vec::new();
        for drawing in self.drawings.iter().rev() {
            if drawing.pane_index != pane_index
                || !drawing.visible
                || !drawing.interval_visibility.allows(self.drawing_interval)
            {
                continue;
            }
            let Some(px) = self.drawing_render_px(drawing) else {
                continue;
            };
            prims.clear();
            points.clear();
            self.build_drawing_prims(drawing, &px, pane_w_px, 1.0, &mut prims, &mut points);
            self.build_drawing_text(drawing, &px, pane_w_px, 1.0, &mut prims);
            self.build_drawing_labels(drawing, &px, 1.0, &mut prims);
            if prims
                .iter()
                .any(|prim| self.prim_touches_box(prim, &points, bounds))
            {
                touched.push(drawing.id);
            }
        }
        touched
    }

    /// Whether one painted primitive overlaps the box. Fully transparent paint never counts.
    fn prim_touches_box(&self, prim: &Prim, points: &[[f32; 2]], bounds: [f64; 4]) -> bool {
        let [left, top, right, bottom] = bounds;
        let overlaps = |x0: f64, y0: f64, x1: f64, y1: f64| {
            x0.min(x1) <= right && x0.max(x1) >= left && y0.min(y1) <= bottom && y0.max(y1) >= top
        };
        // The box lies wholly inside the hole of a hollow shape.
        let within =
            |x0: f64, y0: f64, x1: f64, y1: f64| left > x0 && right < x1 && top > y0 && bottom < y1;
        let point = |p: [f32; 2]| (f64::from(p[0]), f64::from(p[1]));
        let run = |first: u32, count: u32| {
            let first = first as usize;
            points
                .get(first..first.saturating_add(count as usize))
                .unwrap_or(&[])
        };
        let stroke = |path: &[(f64, f64)], width: f64| {
            let pad = width.max(1.0) / 2.0;
            let grown = [left - pad, top - pad, right + pad, bottom + pad];
            match path {
                [] => false,
                [(x, y)] => *x >= grown[0] && *x <= grown[2] && *y >= grown[1] && *y <= grown[3],
                _ => path
                    .windows(2)
                    .any(|pair| segment_crosses_box(pair[0], pair[1], grown)),
            }
        };
        let filled = |polygon: &[(f64, f64)]| {
            !polygon.is_empty()
                && (polygon
                    .iter()
                    .any(|&(x, y)| x >= left && x <= right && y >= top && y <= bottom)
                    || (0..polygon.len()).any(|index| {
                        segment_crosses_box(
                            polygon[index],
                            polygon[(index + 1) % polygon.len()],
                            bounds,
                        )
                    })
                    || crate::drawings::point_in_polygon((left, top), polygon))
        };
        let irect = |rect: &IRect| {
            (
                f64::from(rect.x),
                f64::from(rect.y),
                f64::from(rect.x + rect.w),
                f64::from(rect.y + rect.h),
            )
        };
        let band = |upper: u32, lower: u32, count: u32| {
            let polygon = run(upper, count)
                .iter()
                .chain(run(lower, count).iter().rev())
                .map(|&p| point(p))
                .collect::<Vec<_>>();
            filled(&polygon)
        };
        let text_box = |x: f32, y: f32, text: &str, size: f32, family: &str, weight, italic| {
            let width = self.measure_text_run(text, f64::from(size), family, weight, italic);
            let half = f64::from(size) * 0.6;
            (width, half, f64::from(x), f64::from(y))
        };
        let aligned_left = |x: f64, width: f64, align: TextAlign| match align {
            TextAlign::Left => x,
            TextAlign::Center => x - width / 2.0,
            TextAlign::Right => x - width,
        };
        match prim {
            Prim::Rect { rect, color } => {
                let (x0, y0, x1, y1) = irect(rect);
                color.a() != 0 && overlaps(x0, y0, x1, y1)
            }
            Prim::GradientRect { rect, .. } => {
                let (x0, y0, x1, y1) = irect(rect);
                overlaps(x0, y0, x1, y1)
            }
            Prim::RectFrame {
                rect,
                border,
                color,
            } => {
                let (x0, y0, x1, y1) = irect(rect);
                let border = f64::from(*border);
                color.a() != 0
                    && overlaps(x0, y0, x1, y1)
                    && !within(x0 + border, y0 + border, x1 - border, y1 - border)
            }
            Prim::HLine {
                y,
                x0,
                x1,
                width,
                color,
                ..
            } => {
                let y = f64::from(*y);
                color.a() != 0
                    && stroke(
                        &[(f64::from(*x0), y), (f64::from(*x1), y)],
                        f64::from(*width),
                    )
            }
            Prim::VLine {
                x,
                y0,
                y1,
                width,
                color,
                ..
            } => {
                let x = f64::from(*x);
                color.a() != 0
                    && stroke(
                        &[(x, f64::from(*y0)), (x, f64::from(*y1))],
                        f64::from(*width),
                    )
            }
            Prim::Polyline {
                first_point,
                point_count,
                width,
                color,
                ..
            } => {
                let path = run(*first_point, *point_count)
                    .iter()
                    .map(|&p| point(p))
                    .collect::<Vec<_>>();
                color.a() != 0 && stroke(&path, f64::from(*width))
            }
            Prim::AreaFill {
                first_point,
                point_count,
                base_y,
                ..
            } => {
                let line = run(*first_point, *point_count);
                let (Some(&first), Some(&last)) = (line.first(), line.last()) else {
                    return false;
                };
                let mut polygon = line.iter().map(|&p| point(p)).collect::<Vec<_>>();
                polygon.push((f64::from(last[0]), f64::from(*base_y)));
                polygon.push((f64::from(first[0]), f64::from(*base_y)));
                filled(&polygon)
            }
            Prim::BandFill {
                upper_first,
                lower_first,
                point_count,
                fill,
                ..
            } => fill.a() != 0 && band(*upper_first, *lower_first, *point_count),
            Prim::BandGradientFill {
                upper_first,
                lower_first,
                point_count,
                ..
            } => band(*upper_first, *lower_first, *point_count),
            Prim::RoundRect {
                x,
                y,
                w,
                h,
                fill,
                border_width,
                border_color,
                ..
            } => {
                let (x0, y0) = (f64::from(*x), f64::from(*y));
                let (x1, y1) = (x0 + f64::from(*w), y0 + f64::from(*h));
                let border = f64::from(*border_width);
                if fill.a() != 0 {
                    overlaps(x0, y0, x1, y1)
                } else {
                    border > 0.0
                        && border_color.a() != 0
                        && overlaps(x0, y0, x1, y1)
                        && !within(x0 + border, y0 + border, x1 - border, y1 - border)
                }
            }
            Prim::GradientRoundRect { x, y, w, h, .. } => {
                let (x0, y0) = (f64::from(*x), f64::from(*y));
                overlaps(x0, y0, x0 + f64::from(*w), y0 + f64::from(*h))
            }
            Prim::Circle {
                cx,
                cy,
                radius,
                fill,
                stroke_width,
                stroke,
            } => {
                let (cx, cy, radius) = (f64::from(*cx), f64::from(*cy), f64::from(*radius));
                let half = if stroke.a() != 0 {
                    f64::from(*stroke_width).max(0.0) / 2.0
                } else {
                    0.0
                };
                let nearest = (cx.clamp(left, right) - cx).hypot(cy.clamp(top, bottom) - cy);
                if nearest > radius + half {
                    false
                } else if fill.a() != 0 {
                    true
                } else {
                    let farthest = (cx - left)
                        .abs()
                        .max((cx - right).abs())
                        .hypot((cy - top).abs().max((cy - bottom).abs()));
                    half > 0.0 && farthest >= radius - half
                }
            }
            Prim::Triangle { a, b, c, color } => {
                color.a() != 0 && filled(&[point(*a), point(*b), point(*c)])
            }
            Prim::MeshFill {
                first_point,
                point_count,
                color,
            } => {
                color.a() != 0
                    && run(*first_point, *point_count)
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .any(|&[a, b, c]| filled(&[point(a), point(b), point(c)]))
            }
            Prim::Background { .. } => false,
            Prim::Text {
                x,
                y,
                text,
                color,
                size,
                family,
                align,
                weight,
                italic,
            } => {
                let (width, half, x, y) = text_box(*x, *y, text, *size, family, *weight, *italic);
                let x0 = aligned_left(x, width, *align);
                color.a() != 0 && overlaps(x0, y - half, x0 + width, y + half)
            }
            Prim::RotatedText {
                x,
                y,
                text,
                color,
                size,
                family,
                align,
                weight,
                italic,
                angle,
            } => {
                let (width, half, x, y) = text_box(*x, *y, text, *size, family, *weight, *italic);
                let x0 = aligned_left(0.0, width, *align);
                let (sin, cos) = f64::from(*angle).sin_cos();
                let corners = [
                    (x0, -half),
                    (x0 + width, -half),
                    (x0 + width, half),
                    (x0, half),
                ]
                .map(|(dx, dy)| (x + dx * cos - dy * sin, y + dx * sin + dy * cos));
                color.a() != 0 && filled(&corners)
            }
            Prim::Image { rect, .. } => {
                let (x0, y0) = (f64::from(rect[0]), f64::from(rect[1]));
                overlaps(x0, y0, x0 + f64::from(rect[2]), y0 + f64::from(rect[3]))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_drawing_selection_frame(
        &self,
        pane_index: usize,
        id: DrawingId,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        if drawing.kind.has_stats_card()
            && !drawing.stats_always_visible
            && let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr)
        {
            self.build_info_line_prims(drawing, &px, pane_w_px, vpr, out, points);
        }
        // Text and anchored text show a focus frame; box annotations show their handles.
        if matches!(drawing.kind, DrawingKind::Text | DrawingKind::AnchoredText) {
            let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
                return;
            };
            self.push_text_chrome(drawing, &px, pane_w_px, vpr, ANCHOR_BORDER, out);
            return;
        }
        let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
            return;
        };
        if drawing.kind == DrawingKind::BarsPattern
            && !drawing.bars_pattern.is_empty()
            && px.len() >= 2
        {
            // The copied bars paint at the target; the source window the two range handles
            // edit only exists as this selection outline, so the handles sit on its corners.
            let color = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
            push_dashed_outline(px[0], px[1], color, vpr, out);
        }
        match drawing.kind.spec().handles {
            DrawingHandleMode::None => {}
            mode @ (DrawingHandleMode::RectangleBounds | DrawingHandleMode::BoundsEdges)
                if px.len() == 2 =>
            {
                build_rectangle_handles(&px, mode, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Position if px.len() == 3 => {
                build_position_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Endpoints if !px.is_empty() => {
                build_anchor_handles(&[px[0], px[px.len() - 1]], vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Anchors | DrawingHandleMode::Endpoints => {
                let handles = self.drawing_anchor_handles(drawing, &px, hpr, vpr);
                build_anchor_handles(&handles, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::IconBox => {
                if let Some(icon) = self.icon_box(drawing, &px, vpr) {
                    // A crisp 1 px frame on the icon's whole-pixel square, handles on its corners.
                    let edge = icon.1.round().max(1.0);
                    let (left, top) = (
                        (icon.0.0 - edge / 2.0).round(),
                        (icon.0.1 - edge / 2.0).round(),
                    );
                    out.push(Prim::RectFrame {
                        rect: IRect {
                            x: left as i32,
                            y: top as i32,
                            w: edge as i32,
                            h: edge as i32,
                        },
                        border: vpr.round().max(1.0) as i32,
                        color: ANCHOR_BORDER,
                    });
                    for corner in
                        ChartEngine::icon_box_corners(((left + edge / 2.0, top + edge / 2.0), edge))
                    {
                        push_handle(corner, false, vpr, self.anchor_fill(), out);
                    }
                }
            }
            DrawingHandleMode::OneAnchor { index, square } => {
                if let Some(&center) = px.get(usize::from(index)) {
                    push_handle(center, square, vpr, self.anchor_fill(), out);
                }
            }
            DrawingHandleMode::RectangleBounds
            | DrawingHandleMode::BoundsEdges
            | DrawingHandleMode::Position => {}
        }
    }

    /// The hovered text drawing's focus border at hover opacity (the public reference's hover ring):
    /// the same chrome box as selection, dimmed. Suppressed while the drawing is selected
    /// (the full-strength border already paints, including during typing mode).
    pub(super) fn build_hovered_text_frame(
        &self,
        pane_index: usize,
        pane_w_px: i32,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(id) = self.hovered_text else {
            return;
        };
        if self.is_drawing_selected(id) {
            return;
        }
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        if !matches!(drawing.kind, DrawingKind::Text | DrawingKind::AnchoredText) {
            return;
        }
        let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
            return;
        };
        self.push_text_chrome(drawing, &px, pane_w_px, vpr, HOVER_BORDER, out);
    }

    /// One drawing's geometry prims at bitmap-px anchors `px`.
    fn build_drawing_prims(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        if drawing.kind == DrawingKind::BarsPattern && !drawing.bars_pattern.is_empty() {
            self.build_bars_pattern_prims(drawing, pane_w_px, vpr, out, points);
            return;
        }
        if drawing.kind.is_profile_bound() {
            let snapshot = self.profile_drawing_data(drawing);
            // The fixed range box paints (and stays controllable) before any data arrives.
            if snapshot.is_some() || drawing.kind == DrawingKind::FixedRangeVolumeProfile {
                self.build_profile_drawing_prims(
                    drawing,
                    snapshot.as_deref(),
                    px,
                    pane_w_px,
                    vpr,
                    out,
                    points,
                );
                return;
            }
        }
        let color = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
        let crisp_width = (drawing.width * vpr).round().max(1.0) as i32;
        let Some(pane) = self.panes.get(drawing.pane_index) else {
            return;
        };
        let Some(geometry) = resolve_drawing_geometry(
            drawing.kind,
            px,
            f64::from(pane_w_px),
            pane.top * vpr,
            pane.height * vpr,
            DrawingGeometryOptions {
                line_width: drawing.width,
                device_scale: vpr,
                extend_left: drawing.extend_left,
                icon_size: drawing.icon_size,
                extend_right: drawing.extend_right,
            },
        ) else {
            return;
        };
        for guide in geometry.guides.into_iter().flatten() {
            let first_point = points.len() as u32;
            points.extend([
                [guide.a.0 as f32, guide.a.1 as f32],
                [guide.b.0 as f32, guide.b.1 as f32],
            ]);
            out.push(Prim::Polyline {
                first_point,
                point_count: 2,
                width: (drawing.width * vpr) as f32,
                style: if guide.dashed {
                    LineStyle::Dashed
                } else {
                    drawing.style
                },
                line_type: LineType::Simple,
                color,
            });
        }
        match geometry.body {
            DrawingBodyGeometry::Annotation => {
                self.build_annotation_prims(drawing, px, color, vpr, out, points);
            }
            DrawingBodyGeometry::Segment { a, b } => {
                let cutout = self.line_text_cutout(drawing, px, pane_w_px, vpr, a, b);
                for (from, to) in cut_segment(a, b, cutout) {
                    push_segment(from, to, drawing, color, vpr, out, points);
                }
                push_drawing_cap(
                    drawing.stroke_start,
                    a,
                    b,
                    drawing.width,
                    vpr,
                    color,
                    out,
                    points,
                );
                push_drawing_cap(
                    drawing.stroke_end,
                    b,
                    a,
                    drawing.width,
                    vpr,
                    color,
                    out,
                    points,
                );
                if drawing.kind == DrawingKind::TrendAngle {
                    self.build_trend_angle_prims(drawing, px, color, vpr, out, points);
                }
                if drawing.kind.is_straight_line() {
                    self.build_line_point_marks(drawing, px, color, vpr, out);
                }
                // A selection-only card is overlay chrome (build_selected_drawing_handles_frame).
                if drawing.kind.has_stats_card() && drawing.stats_always_visible {
                    self.build_info_line_prims(drawing, px, pane_w_px, vpr, out, points);
                }
            }
            DrawingBodyGeometry::Horizontal { y, x0, x1 } => {
                let (a, b) = ((x0, y), (x1, y));
                let cutout = self.line_text_cutout(drawing, px, pane_w_px, vpr, a, b);
                for ((x0, _), (x1, _)) in cut_segment(a, b, cutout) {
                    let x0 = (x0.round() as i32).clamp(0, pane_w_px);
                    let x1 = (x1.round() as i32).clamp(0, pane_w_px);
                    if x0 != x1 {
                        out.push(Prim::HLine {
                            y: y.round() as i32,
                            x0: x0.min(x1),
                            x1: x0.max(x1),
                            width: crisp_width,
                            style: drawing.style,
                            color,
                        });
                    }
                }
            }
            DrawingBodyGeometry::Vertical { x, y0, y1 } => {
                let (a, b) = ((x, y0), (x, y1));
                let cutout = self.line_text_cutout(drawing, px, pane_w_px, vpr, a, b);
                for ((_, y0), (_, y1)) in cut_segment(a, b, cutout) {
                    out.push(Prim::VLine {
                        x: x.round() as i32,
                        y0: y0.round().max(0.0) as i32,
                        y1: y1.round().max(0.0) as i32,
                        width: crisp_width,
                        style: drawing.style,
                        color,
                    });
                }
            }
            DrawingBodyGeometry::Cross {
                x,
                y,
                pane_w,
                pane_top,
                pane_bottom,
            } => {
                out.push(Prim::HLine {
                    y: y.round() as i32,
                    x0: 0,
                    x1: pane_w.round() as i32,
                    width: crisp_width,
                    style: drawing.style,
                    color,
                });
                out.push(Prim::VLine {
                    x: x.round() as i32,
                    y0: pane_top.round().max(0.0) as i32,
                    y1: pane_bottom.round().max(0.0) as i32,
                    width: crisp_width,
                    style: drawing.style,
                    color,
                });
            }
            DrawingBodyGeometry::Channel { first, second } => {
                if drawing.fill_enabled {
                    let fill = area_fill(drawing, color);
                    if drawing.kind == DrawingKind::DisjointChannel {
                        let a = [first[0].0 as f32, first[0].1 as f32];
                        let b = [first[1].0 as f32, first[1].1 as f32];
                        let c = [second[1].0 as f32, second[1].1 as f32];
                        let d = [second[0].0 as f32, second[0].1 as f32];
                        out.push(Prim::Triangle {
                            a,
                            b,
                            c,
                            color: fill,
                        });
                        out.push(Prim::Triangle {
                            a,
                            b: c,
                            c: d,
                            color: fill,
                        });
                    } else {
                        let upper_first = points.len() as u32;
                        points.extend(first.map(|(x, y)| [x as f32, y as f32]));
                        let lower_first = points.len() as u32;
                        points.extend(second.map(|(x, y)| [x as f32, y as f32]));
                        out.push(Prim::BandFill {
                            upper_first,
                            lower_first,
                            point_count: 2,
                            line_type: LineType::Simple,
                            fill,
                        });
                    }
                }
                push_segment(first[0], first[1], drawing, color, vpr, out, points);
                push_segment(second[0], second[1], drawing, color, vpr, out, points);
            }
            DrawingBodyGeometry::Regression {
                center,
                upper,
                lower,
            } => {
                if drawing.fill_enabled {
                    let fill = area_fill(drawing, color);
                    let upper_first = points.len() as u32;
                    points.extend(upper.map(|(x, y)| [x as f32, y as f32]));
                    let lower_first = points.len() as u32;
                    points.extend(lower.map(|(x, y)| [x as f32, y as f32]));
                    out.push(Prim::BandFill {
                        upper_first,
                        lower_first,
                        point_count: 2,
                        line_type: LineType::Simple,
                        fill,
                    });
                }
                for segment in [lower, upper, center] {
                    push_segment(segment[0], segment[1], drawing, color, vpr, out, points);
                }
            }
            DrawingBodyGeometry::Fibonacci(fib) => {
                if drawing.kind == DrawingKind::FibonacciRetracement && drawing.level_trend_line {
                    let first_point = points.len() as u32;
                    points.extend([
                        [fib.start.0 as f32, fib.start.1 as f32],
                        [fib.end.0 as f32, fib.end.1 as f32],
                    ]);
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: 2,
                        width: (drawing.width * vpr) as f32,
                        style: LineStyle::Dashed,
                        line_type: LineType::Simple,
                        color,
                    });
                }
                let mut previous: Option<((f64, f64), (f64, f64))> = None;
                for level in &drawing.levels {
                    if !level.visible {
                        previous = None;
                        continue;
                    }
                    let ((x0, y0), (x1, y1)) =
                        self.drawing_fibonacci_level_segment(drawing, fib, level.value, vpr);
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    if drawing.fill_enabled
                        && level.fill_between
                        && let Some((prior_a, prior_b)) = previous
                    {
                        let fill = level
                            .fill_color
                            .as_deref()
                            .and_then(Color::parse_css)
                            .unwrap_or(Color::rgba(
                                level_color.r(),
                                level_color.g(),
                                level_color.b(),
                                35,
                            ));
                        if (y0 - y1).abs() <= f64::EPSILON {
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: x0.min(x1).round() as i32,
                                    y: y0.min(prior_a.1).round() as i32,
                                    w: (x1 - x0).abs().round().max(1.0) as i32,
                                    h: (y0 - prior_a.1).abs().round().max(1.0) as i32,
                                },
                                color: fill,
                            });
                        } else {
                            let upper_first = points.len() as u32;
                            points.extend([
                                [prior_a.0 as f32, prior_a.1 as f32],
                                [prior_b.0 as f32, prior_b.1 as f32],
                            ]);
                            let lower_first = points.len() as u32;
                            points.extend([[x0 as f32, y0 as f32], [x1 as f32, y1 as f32]]);
                            out.push(Prim::BandFill {
                                upper_first,
                                lower_first,
                                point_count: 2,
                                line_type: LineType::Simple,
                                fill,
                            });
                        }
                    }
                    previous = Some(((x0, y0), (x1, y1)));
                }
                for level in &drawing.levels {
                    if !level.visible {
                        continue;
                    }
                    let ((x0, y0), (x1, y1)) =
                        self.drawing_fibonacci_level_segment(drawing, fib, level.value, vpr);
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    let style = match level.style.as_str() {
                        "dotted" | "sparse_dotted" => LineStyle::Dotted,
                        "dashed" | "large_dashed" => LineStyle::Dashed,
                        _ => LineStyle::Solid,
                    };
                    let layout = &self.options.get().layout;
                    let size = drawing.resolved_text_size(layout.font_size) * vpr;
                    let horizontal = (y1 - y0).abs() <= vpr;
                    let label = level
                        .label_visible
                        .then(|| {
                            let price = self.drawing_level_price_at(drawing, y1, vpr);
                            self.drawing_level_label(drawing, level.value, price)
                        })
                        .flatten()
                        .map(|text| {
                            let label_x = match drawing.level_label_align.as_str() {
                                "left" => x0,
                                "center" => (x0 + x1) / 2.0,
                                _ => x1,
                            };
                            // The label sits on the line's own height at its x and clears the
                            // line everywhere under its run, so a sloped level never cuts it.
                            let width = self.measure_text_run(
                                &text,
                                size,
                                &layout.font_family,
                                drawing.text_weight.unwrap_or(400),
                                drawing.text_italic,
                            );
                            let line_y = |x: f64| {
                                if (x1 - x0).abs() <= f64::EPSILON {
                                    y0.min(y1)
                                } else {
                                    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
                                }
                            };
                            let (run_left, run_right) = match drawing.level_label_align.as_str() {
                                "left" => (label_x, label_x + width),
                                "center" => (label_x - width / 2.0, label_x + width / 2.0),
                                _ => (label_x - width, label_x),
                            };
                            let (label_x, label_y, align) = if horizontal {
                                let clearance = POINT_LABEL_GAP_CSS * vpr + size * 0.6;
                                let label_y = match drawing.level_label_v_align {
                                    crate::drawings::DrawingTextVAlign::Top => {
                                        line_y(run_left).min(line_y(run_right)) - clearance
                                    }
                                    crate::drawings::DrawingTextVAlign::Middle => line_y(label_x),
                                    crate::drawings::DrawingTextVAlign::Bottom => {
                                        line_y(run_left).max(line_y(run_right)) + clearance
                                    }
                                };
                                (label_x, label_y, Self::drawing_level_align(drawing))
                            } else {
                                // A sloped level (a channel's parallels) labels outside its own
                                // end, beside it: every parallel stops at the same x extent, so a
                                // label past that edge can never cross a neighbor.
                                let gap = POINT_LABEL_GAP_CSS * vpr;
                                let (left_end, right_end) = if x0 <= x1 {
                                    ((x0, y0), (x1, y1))
                                } else {
                                    ((x1, y1), (x0, y0))
                                };
                                match drawing.level_label_align.as_str() {
                                    "left" => (left_end.0 - gap, left_end.1, TextAlign::Right),
                                    "center" => {
                                        let (x, y) = clear_label_center(
                                            (label_x, line_y(label_x)),
                                            &[(x0, y0), (x1, y1)],
                                            width,
                                            size * 1.2,
                                            gap,
                                            (0.0, -1.0),
                                        );
                                        (x, y, TextAlign::Center)
                                    }
                                    _ => (right_end.0 + gap, right_end.1, TextAlign::Left),
                                }
                            };
                            (text, label_x, label_y, align, width)
                        });
                    // A label centered on its level opens the level's stroke under it.
                    let cutout = label
                        .as_ref()
                        .filter(|_| {
                            horizontal
                                && drawing.level_label_v_align
                                    == crate::drawings::DrawingTextVAlign::Middle
                        })
                        .and_then(|&(_, label_x, label_y, align, width)| {
                            text_box_cutout(
                                (x0, y0),
                                (x1, y1),
                                (label_x, label_y),
                                0.0,
                                padded_text_box(align, width, size, TEXT_PAD * vpr),
                            )
                        });
                    for ((x0, y0), (x1, y1)) in cut_segment((x0, y0), (x1, y1), cutout) {
                        if (y0 - y1).abs() <= f64::EPSILON {
                            let (left, right) =
                                (x0.min(x1).round() as i32, x0.max(x1).round() as i32);
                            if cutout.is_some() && left == right {
                                continue;
                            }
                            out.push(Prim::HLine {
                                y: y0.round() as i32,
                                x0: left,
                                x1: right,
                                width: crisp_width,
                                style,
                                color: level_color,
                            });
                        } else {
                            let first_point = points.len() as u32;
                            points.extend([[x0 as f32, y0 as f32], [x1 as f32, y1 as f32]]);
                            out.push(Prim::Polyline {
                                first_point,
                                point_count: 2,
                                width: (drawing.width * vpr) as f32,
                                style,
                                line_type: LineType::Simple,
                                color: level_color,
                            });
                        }
                    }
                    if let Some((text, label_x, label_y, align, _)) = label {
                        out.push(Prim::Text {
                            x: label_x as f32,
                            y: label_y as f32,
                            text,
                            color: level_color,
                            size: size as f32,
                            family: layout.font_family.clone(),
                            align,
                            weight: drawing.text_weight.unwrap_or(400),
                            italic: drawing.text_italic,
                        });
                    }
                }
            }
            DrawingBodyGeometry::TimeLevels(time) => {
                let mut previous_x = None;
                for level in &drawing.levels {
                    if !level.visible {
                        previous_x = None;
                        continue;
                    }
                    let x = time.x(drawing.level_value(level.value));
                    if drawing.fill_enabled
                        && level.fill_between
                        && let Some(prior_x) = previous_x
                    {
                        let left = x.min(prior_x).max(0.0);
                        let right = x.max(prior_x).min(f64::from(pane_w_px));
                        if right > left {
                            let level_color = Color::parse_css(&level.color).unwrap_or(color);
                            let fill = level
                                .fill_color
                                .as_deref()
                                .and_then(Color::parse_css)
                                .unwrap_or(Color::rgba(
                                    level_color.r(),
                                    level_color.g(),
                                    level_color.b(),
                                    35,
                                ));
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: left.round() as i32,
                                    y: time.pane_top.round().max(0.0) as i32,
                                    w: (right - left).round().max(1.0) as i32,
                                    h: (time.pane_bottom - time.pane_top).round().max(1.0) as i32,
                                },
                                color: fill,
                            });
                        }
                    }
                    previous_x = Some(x);
                }
                for level in &drawing.levels {
                    if !level.visible {
                        continue;
                    }
                    let x = time.x(drawing.level_value(level.value));
                    if !(0.0..=f64::from(pane_w_px)).contains(&x) {
                        continue;
                    }
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    let style = match level.style.as_str() {
                        "dotted" | "sparse_dotted" => LineStyle::Dotted,
                        "dashed" | "large_dashed" => LineStyle::Dashed,
                        _ => LineStyle::Solid,
                    };
                    let layout = &self.options.get().layout;
                    let size = drawing.resolved_text_size(layout.font_size) * vpr;
                    let align = Self::drawing_level_align(drawing);
                    let label = level
                        .label_visible
                        .then(|| self.drawing_level_label(drawing, level.value, None))
                        .flatten()
                        .map(|text| {
                            let label_x = x + match align {
                                TextAlign::Left => 4.0 * vpr,
                                TextAlign::Right => -4.0 * vpr,
                                TextAlign::Center => 0.0,
                            };
                            (text, label_x, time.pane_top + 14.0 * vpr)
                        });
                    // A centered label straddles its level; the level opens under it.
                    let (top, bottom) = ((x, time.pane_top), (x, time.pane_bottom));
                    let cutout = label
                        .as_ref()
                        .filter(|_| align == TextAlign::Center)
                        .and_then(|(text, label_x, label_y)| {
                            let width = self.measure_text_run(
                                text,
                                size,
                                &layout.font_family,
                                drawing.text_weight.unwrap_or(400),
                                drawing.text_italic,
                            );
                            text_box_cutout(
                                top,
                                bottom,
                                (*label_x, *label_y),
                                0.0,
                                padded_text_box(align, width, size, TEXT_PAD * vpr),
                            )
                        });
                    for ((_, y0), (_, y1)) in cut_segment(top, bottom, cutout) {
                        let (y0, y1) = (y0.round().max(0.0) as i32, y1.round().max(0.0) as i32);
                        if cutout.is_some() && y0 == y1 {
                            continue;
                        }
                        out.push(Prim::VLine {
                            x: x.round() as i32,
                            y0,
                            y1,
                            width: crisp_width,
                            style,
                            color: level_color,
                        });
                    }
                    if let Some((text, label_x, label_y)) = label {
                        out.push(Prim::Text {
                            x: label_x as f32,
                            y: label_y as f32,
                            text,
                            color: level_color,
                            size: size as f32,
                            family: layout.font_family.clone(),
                            align,
                            weight: drawing.text_weight.unwrap_or(400),
                            italic: drawing.text_italic,
                        });
                    }
                }
            }
            DrawingBodyGeometry::FibonacciArcs(arcs) => {
                if arcs.kind == DrawingKind::FibonacciWedge {
                    for &side in &px[1..3] {
                        push_segment(px[0], side, drawing, color, vpr, out, points);
                    }
                }
                let segments = arcs.segments(
                    drawing
                        .levels
                        .iter()
                        .filter(|level| level.visible)
                        .map(|level| drawing.level_value(level.value))
                        .fold(0.0, f64::max),
                );
                let mut previous = None;
                for level in &drawing.levels {
                    if !level.visible || drawing.level_value(level.value) <= 0.0 {
                        previous = None;
                        continue;
                    }
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    if drawing.fill_enabled
                        && level.fill_between
                        && let Some(prior_value) = previous
                    {
                        let fill = level
                            .fill_color
                            .as_deref()
                            .and_then(Color::parse_css)
                            .unwrap_or(Color::rgba(
                                level_color.r(),
                                level_color.g(),
                                level_color.b(),
                                35,
                            ));
                        let upper_first = points.len() as u32;
                        for step in 0..=segments {
                            let (x, y) = arcs.point(prior_value, step as f64 / f64::from(segments));
                            points.push([x as f32, y as f32]);
                        }
                        let lower_first = points.len() as u32;
                        for step in 0..=segments {
                            let (x, y) = arcs.point(
                                drawing.level_value(level.value),
                                step as f64 / f64::from(segments),
                            );
                            points.push([x as f32, y as f32]);
                        }
                        out.push(Prim::BandFill {
                            upper_first,
                            lower_first,
                            point_count: segments + 1,
                            line_type: LineType::Simple,
                            fill,
                        });
                    }
                    previous = Some(drawing.level_value(level.value));
                }
                for level in &drawing.levels {
                    if !level.visible || drawing.level_value(level.value) <= 0.0 {
                        continue;
                    }
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    let style = match level.style.as_str() {
                        "dotted" | "sparse_dotted" => LineStyle::Dotted,
                        "dashed" | "large_dashed" => LineStyle::Dashed,
                        _ => LineStyle::Solid,
                    };
                    let first_point = points.len() as u32;
                    for step in 0..=segments {
                        let (x, y) = arcs.point(
                            drawing.level_value(level.value),
                            step as f64 / f64::from(segments),
                        );
                        points.push([x as f32, y as f32]);
                    }
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: segments + 1,
                        width: (drawing.width * vpr) as f32,
                        style,
                        line_type: LineType::Simple,
                        color: level_color,
                    });
                    if level.label_visible {
                        let value = drawing.level_value(level.value);
                        let label_t = arcs.label_t();
                        let (x, y) = arcs.point(value, label_t);
                        let price = self.drawing_level_price_at(drawing, y, vpr);
                        if let Some(text) = self.drawing_level_label(drawing, level.value, price) {
                            // Outside the arc where its radius runs vertically, never on a stroke.
                            let layout = &self.options.get().layout;
                            let size = drawing.resolved_text_size(layout.font_size) * vpr;
                            let width = self.measure_text_run(
                                &text,
                                size,
                                &layout.font_family,
                                drawing.text_weight.unwrap_or(400),
                                drawing.text_italic,
                            );
                            let radial = (x - arcs.center.0, y - arcs.center.1);
                            let length = radial.0.hypot(radial.1).max(f64::EPSILON);
                            let (x, y) = clear_label_center(
                                (x, y),
                                &[
                                    arcs.point(value, (label_t - 0.01).max(0.0)),
                                    arcs.point(value, (label_t + 0.01).min(1.0)),
                                ],
                                width,
                                size * 1.2,
                                POINT_LABEL_GAP_CSS * vpr,
                                (radial.0 / length, radial.1 / length),
                            );
                            out.push(Prim::Text {
                                x: x as f32,
                                y: y as f32,
                                text,
                                color: level_color,
                                size: size as f32,
                                family: layout.font_family.clone(),
                                align: TextAlign::Center,
                                weight: drawing.text_weight.unwrap_or(400),
                                italic: drawing.text_italic,
                            });
                        }
                    }
                }
            }
            DrawingBodyGeometry::Pitchfork(fork) => {
                let mut previous: Option<((f64, f64), (f64, f64))> = None;
                for level in &drawing.levels {
                    if !level.visible {
                        previous = None;
                        continue;
                    }
                    let segment = fork.segment(drawing.level_value(level.value));
                    if drawing.fill_enabled
                        && level.fill_between
                        && let Some((prior_a, prior_b)) = previous
                    {
                        let level_color = Color::parse_css(&level.color).unwrap_or(color);
                        let fill = level
                            .fill_color
                            .as_deref()
                            .and_then(Color::parse_css)
                            .unwrap_or(Color::rgba(
                                level_color.r(),
                                level_color.g(),
                                level_color.b(),
                                35,
                            ));
                        let upper_first = points.len() as u32;
                        points.extend([
                            [prior_a.0 as f32, prior_a.1 as f32],
                            [prior_b.0 as f32, prior_b.1 as f32],
                        ]);
                        let lower_first = points.len() as u32;
                        points.extend([
                            [segment.0.0 as f32, segment.0.1 as f32],
                            [segment.1.0 as f32, segment.1.1 as f32],
                        ]);
                        out.push(Prim::BandFill {
                            upper_first,
                            lower_first,
                            point_count: 2,
                            line_type: LineType::Simple,
                            fill,
                        });
                    }
                    previous = Some(segment);
                }
                for level in &drawing.levels {
                    if !level.visible {
                        continue;
                    }
                    let (a, b) = fork.segment(drawing.level_value(level.value));
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    let style = match level.style.as_str() {
                        "dotted" | "sparse_dotted" => LineStyle::Dotted,
                        "dashed" | "large_dashed" => LineStyle::Dashed,
                        _ => LineStyle::Solid,
                    };
                    let first_point = points.len() as u32;
                    points.extend([[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]]);
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: 2,
                        width: (drawing.width * vpr) as f32,
                        style,
                        line_type: LineType::Simple,
                        color: level_color,
                    });
                    if level.label_visible {
                        let anchor = fork.anchor(drawing.level_value(level.value));
                        let price = self.drawing_level_price_at(drawing, anchor.1, vpr);
                        if let Some(text) = self.drawing_level_label(drawing, level.value, price) {
                            // Clear of the tine (both ways: the median runs back to the pivot)
                            // and of the base line it starts on.
                            let layout = &self.options.get().layout;
                            let size = drawing.resolved_text_size(layout.font_size) * vpr;
                            let width = self.measure_text_run(
                                &text,
                                size,
                                &layout.font_family,
                                drawing.text_weight.unwrap_or(400),
                                drawing.text_italic,
                            );
                            let (x, y) = clear_label_center(
                                anchor,
                                &[
                                    fork.segment(drawing.level_value(level.value)).0,
                                    fork.segment(drawing.level_value(level.value)).1,
                                    fork.anchor(0.0),
                                    fork.anchor(1.0),
                                ],
                                width,
                                size * 1.2,
                                POINT_LABEL_GAP_CSS * vpr,
                                (0.0, -1.0),
                            );
                            out.push(Prim::Text {
                                x: x as f32,
                                y: y as f32,
                                text,
                                color: level_color,
                                size: size as f32,
                                family: layout.font_family.clone(),
                                align: TextAlign::Center,
                                weight: drawing.text_weight.unwrap_or(400),
                                italic: drawing.text_italic,
                            });
                        }
                    }
                }
            }
            DrawingBodyGeometry::Cycles(cycles) => {
                let fill = area_fill(drawing, color);
                let mut previous_x: Option<f64> = None;
                cycles.for_each_visible_line(|index, x| {
                    if drawing.fill_enabled
                        && index % 2 != 0
                        && let Some(prior_x) = previous_x
                    {
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: prior_x.min(x).round() as i32,
                                y: cycles.pane_top.round().max(0.0) as i32,
                                w: (x - prior_x).abs().round().max(1.0) as i32,
                                h: (cycles.pane_bottom - cycles.pane_top).round().max(1.0) as i32,
                            },
                            color: fill,
                        });
                    }
                    previous_x = Some(x);
                });
                cycles.for_each_visible_line(|index, x| {
                    out.push(Prim::VLine {
                        x: x.round() as i32,
                        y0: cycles.pane_top.round().max(0.0) as i32,
                        y1: cycles.pane_bottom.round().max(0.0) as i32,
                        width: crisp_width,
                        style: drawing.style,
                        color,
                    });
                    if drawing.kind == DrawingKind::TimeCycles {
                        out.push(Prim::Text {
                            x: (x + 4.0 * vpr) as f32,
                            y: (cycles.pane_top + 14.0 * vpr) as f32,
                            text: index.to_string(),
                            color,
                            size: (self.options.get().layout.font_size * vpr) as f32,
                            family: self.options.get().layout.font_family.clone(),
                            align: TextAlign::Left,
                            weight: drawing.text_weight.unwrap_or(400),
                            italic: drawing.text_italic,
                        });
                    }
                });
            }
            DrawingBodyGeometry::Sine(sine) => {
                if let Some((left, right)) = sine.visible_x() {
                    let count = sine.sample_count();
                    let first_point = points.len() as u32;
                    for step in 0..=count {
                        let x = left + (right - left) * f64::from(step) / f64::from(count);
                        points.push([x as f32, sine.y(x) as f32]);
                    }
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: count + 1,
                        width: (drawing.width * vpr) as f32,
                        style: drawing.style,
                        line_type: LineType::Simple,
                        color,
                    });
                }
            }
            DrawingBodyGeometry::Marker(marker) => {
                let (a, b) = marker.stem();
                push_segment(a, b, drawing, color, vpr, out, points);
                let [a, b, c] = marker.triangle();
                out.push(Prim::Triangle {
                    a: [a.0 as f32, a.1 as f32],
                    b: [b.0 as f32, b.1 as f32],
                    c: [c.0 as f32, c.1 as f32],
                    color,
                });
            }
            DrawingBodyGeometry::PriceLabel { x, y } => {
                let label = self.price_label_layout(drawing, (x, y), vpr);
                let [left, top, width, height] = label.rect;
                let bottom = top + height;
                let radius = (3.0 * vpr).round().max(1.0) as f32;
                out.push(Prim::RoundRect {
                    x: left as f32,
                    y: top as f32,
                    w: width as f32,
                    h: height as f32,
                    radii: [radius, radius, radius, 0.0],
                    fill: color,
                    border_width: 0.0,
                    border_color: color,
                });
                out.push(Prim::Triangle {
                    a: [x as f32, y as f32],
                    b: [left as f32, bottom as f32],
                    c: [(left + label.tail * 1.5) as f32, bottom as f32],
                    color,
                });
                let layout = &self.options.get().layout;
                let ink = drawing
                    .text_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or_else(|| color.contrast_text());
                let lines = TextBlock::unmeasured(
                    &label.text,
                    label.size,
                    crate::drawings::DrawingTextVAlign::Middle,
                );
                for (x, y, line) in
                    lines.line_anchors(left + label.padding, top + height / 2.0, 0.0)
                {
                    out.push(Prim::Text {
                        x: x as f32,
                        y: y as f32,
                        text: line.to_string(),
                        color: ink,
                        size: label.size as f32,
                        family: layout.font_family.clone(),
                        align: TextAlign::Left,
                        weight: drawing_text_weight(drawing),
                        italic: drawing.text_italic,
                    });
                }
            }
            DrawingBodyGeometry::IconStamp { center, size } => {
                let rect = [
                    (center.0 - size / 2.0) as f32,
                    (center.1 - size / 2.0) as f32,
                    size as f32,
                    size as f32,
                ];
                // An arrow marker paints its built-in arrow. A stamp's name resolves to a
                // host-registered image first, then a built-in icon.
                let name = match arrow_marker_icon(drawing.kind) {
                    Some((arrow, _)) => Some(arrow),
                    None => drawing.icon_name.as_deref(),
                };
                let image = (drawing.kind == DrawingKind::IconStamp)
                    .then(|| name.and_then(|name| self.drawing_icons.get(name)))
                    .flatten();
                if let Some(image) = image {
                    out.push(Prim::Image {
                        image: image.clone(),
                        source: image.full_source(),
                        rect,
                        opacity: 1.0,
                        sampling: ImageSampling::Linear,
                    });
                } else if let Some(icon) = name.and_then(builtin_icon) {
                    // A solid icon rasterizes at its whole-device-px size on a pixel-aligned
                    // square, so executors draw it 1:1 without resampling.
                    let edge = size.round().max(1.0);
                    let image = self
                        .icon_rasters
                        .borrow_mut()
                        .raster(icon, edge as u32, color);
                    out.push(Prim::Image {
                        rect: [
                            (center.0 - edge / 2.0).round() as f32,
                            (center.1 - edge / 2.0).round() as f32,
                            image.width as f32,
                            image.height as f32,
                        ],
                        source: image.full_source(),
                        image,
                        opacity: 1.0,
                        sampling: ImageSampling::Linear,
                    });
                } else {
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: rect[0].round() as i32,
                            y: rect[1].round() as i32,
                            w: size.round().max(1.0) as i32,
                            h: size.round().max(1.0) as i32,
                        },
                        color,
                    });
                }
            }
            DrawingBodyGeometry::GannGrid(grid) => {
                let box_bounds = grid.bounds();
                if drawing.fill_enabled {
                    let fill = area_fill(drawing, color);
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: box_bounds.left.round() as i32,
                            y: box_bounds.top.round() as i32,
                            w: (box_bounds.right - box_bounds.left).round().max(1.0) as i32,
                            h: (box_bounds.bottom - box_bounds.top).round().max(1.0) as i32,
                        },
                        color: fill,
                    });
                    let mut prior_grid: Option<f64> = None;
                    for level in drawing.levels.iter().filter(|level| level.visible) {
                        let value = drawing.level_value(level.value);
                        if level.fill_between
                            && let Some(previous) = prior_grid
                        {
                            let x0 = grid.start.0 + (grid.end.0 - grid.start.0) * previous;
                            let x1 = grid.start.0 + (grid.end.0 - grid.start.0) * value;
                            let y0 = grid.start.1 + (grid.end.1 - grid.start.1) * previous;
                            let y1 = grid.start.1 + (grid.end.1 - grid.start.1) * value;
                            let fill = Self::drawing_level_fill(level, color);
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: x0.min(x1).round() as i32,
                                    y: y0.min(y1).round() as i32,
                                    w: (x1 - x0).abs().round().max(1.0) as i32,
                                    h: (y1 - y0).abs().round().max(1.0) as i32,
                                },
                                color: fill,
                            });
                        }
                        prior_grid = Some(value);
                    }
                    let mut prior_fan: Option<(f64, f64)> = None;
                    for level in drawing.gann_fans.iter().filter(|level| level.visible) {
                        let (pivot, end) = grid.fan_segment(level.value, drawing.level_reverse);
                        if level.fill_between
                            && let Some(previous) = prior_fan
                        {
                            out.push(Prim::Triangle {
                                a: [pivot.0 as f32, pivot.1 as f32],
                                b: [previous.0 as f32, previous.1 as f32],
                                c: [end.0 as f32, end.1 as f32],
                                color: Self::drawing_level_fill(level, color),
                            });
                        }
                        prior_fan = Some(end);
                    }
                    let gann_segments = gann_arc_segments(grid, drawing);
                    let mut prior_arc: Option<f64> = None;
                    for level in drawing.gann_arcs.iter().filter(|level| level.visible) {
                        if level.fill_between
                            && let Some(previous) = prior_arc
                        {
                            let upper_first = points.len() as u32;
                            for step in 0..=gann_segments {
                                let p = grid.arc_point(
                                    previous,
                                    f64::from(step) / f64::from(gann_segments),
                                    drawing.level_reverse,
                                );
                                points.push([p.0 as f32, p.1 as f32]);
                            }
                            let lower_first = points.len() as u32;
                            for step in 0..=gann_segments {
                                let p = grid.arc_point(
                                    level.value,
                                    f64::from(step) / f64::from(gann_segments),
                                    drawing.level_reverse,
                                );
                                points.push([p.0 as f32, p.1 as f32]);
                            }
                            out.push(Prim::BandFill {
                                upper_first,
                                lower_first,
                                point_count: gann_segments + 1,
                                line_type: LineType::Simple,
                                fill: Self::drawing_level_fill(level, color),
                            });
                        }
                        prior_arc = Some(level.value);
                    }
                }
                let corners = [
                    (box_bounds.left, box_bounds.top),
                    (box_bounds.right, box_bounds.top),
                    (box_bounds.right, box_bounds.bottom),
                    (box_bounds.left, box_bounds.bottom),
                ];
                for index in 0..4 {
                    push_segment(
                        corners[index],
                        corners[(index + 1) % 4],
                        drawing,
                        color,
                        vpr,
                        out,
                        points,
                    );
                }
                // Border labels already placed; a label that would overlap one is skipped, so
                // a small box keeps its labels readable.
                let mut placed_labels: Vec<[f64; 4]> = Vec::new();
                for level in drawing.levels.iter().filter(|level| level.visible) {
                    let level_color = Color::parse_css(&level.color).unwrap_or(color);
                    let style = match level.style.as_str() {
                        "dotted" | "sparse_dotted" => LineStyle::Dotted,
                        "dashed" | "large_dashed" => LineStyle::Dashed,
                        _ => LineStyle::Solid,
                    };
                    for (a, b) in grid.level_lines(drawing.level_value(level.value)) {
                        let first_point = points.len() as u32;
                        points.extend([[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]]);
                        out.push(Prim::Polyline {
                            first_point,
                            point_count: 2,
                            width: (drawing.width * vpr) as f32,
                            style,
                            line_type: LineType::Simple,
                            color: level_color,
                        });
                    }
                    if level.label_visible {
                        let x = grid.start.0
                            + (grid.end.0 - grid.start.0) * drawing.level_value(level.value);
                        let y = grid.start.1
                            + (grid.end.1 - grid.start.1) * drawing.level_value(level.value);
                        let price = self
                            .drawing_scale_for(drawing.pane_index, drawing.price_scale)
                            .map(|scale| {
                                scale.coordinate_to_price(
                                    y / vpr,
                                    self.drawing_scale_base_for(
                                        drawing.pane_index,
                                        drawing.price_scale,
                                    ),
                                )
                            });
                        // Levels label on the box border, outside the grid that the fans and arcs
                        // fill: the price level left of the left edge (with its price), the time
                        // level under the bottom edge.
                        let layout = &self.options.get().layout;
                        let size = drawing.resolved_text_size(layout.font_size) * vpr;
                        let gap = POINT_LABEL_GAP_CSS * vpr;
                        let edges = grid.bounds();
                        let mut push_label = |text: String, x: f64, y: f64, align: TextAlign| {
                            let width = self.measure_text_run(
                                &text,
                                size,
                                &layout.font_family,
                                drawing.text_weight.unwrap_or(400),
                                drawing.text_italic,
                            );
                            let left = match align {
                                TextAlign::Left => x,
                                TextAlign::Center => x - width / 2.0,
                                TextAlign::Right => x - width,
                            };
                            let rect = [left, y - size * 0.6, left + width, y + size * 0.6];
                            if placed_labels.iter().any(|other| {
                                rect[0] < other[2] + gap
                                    && other[0] < rect[2] + gap
                                    && rect[1] < other[3]
                                    && other[1] < rect[3]
                            }) {
                                return;
                            }
                            placed_labels.push(rect);
                            out.push(Prim::Text {
                                x: x as f32,
                                y: y as f32,
                                text,
                                color: level_color,
                                size: size as f32,
                                family: layout.font_family.clone(),
                                align,
                                weight: drawing.text_weight.unwrap_or(400),
                                italic: drawing.text_italic,
                            });
                        };
                        if let Some(text) = self.drawing_level_label(drawing, level.value, price) {
                            push_label(text, edges.left - gap, y, TextAlign::Right);
                        }
                        if let Some(text) = self.drawing_level_label(drawing, level.value, None) {
                            push_label(text, x, edges.bottom + gap + size * 0.6, TextAlign::Center);
                        }
                    }
                }
                if grid.kind != DrawingKind::GannBox {
                    for level in drawing.gann_fans.iter().filter(|level| level.visible) {
                        let (a, b) = grid.fan_segment(level.value, drawing.level_reverse);
                        let level_color = Color::parse_css(&level.color).unwrap_or(color);
                        let first_point = points.len() as u32;
                        points.extend([[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]]);
                        out.push(Prim::Polyline {
                            first_point,
                            point_count: 2,
                            width: (drawing.width * vpr) as f32,
                            style: Self::drawing_level_style(&level.style),
                            line_type: LineType::Simple,
                            color: level_color,
                        });
                    }
                    let gann_segments = gann_arc_segments(grid, drawing);
                    for level in drawing.gann_arcs.iter().filter(|level| level.visible) {
                        let level_color = Color::parse_css(&level.color).unwrap_or(color);
                        let first_point = points.len() as u32;
                        for step in 0..=gann_segments {
                            let point = grid.arc_point(
                                level.value,
                                f64::from(step) / f64::from(gann_segments),
                                drawing.level_reverse,
                            );
                            points.push([point.0 as f32, point.1 as f32]);
                        }
                        out.push(Prim::Polyline {
                            first_point,
                            point_count: gann_segments + 1,
                            width: (drawing.width * vpr) as f32,
                            style: Self::drawing_level_style(&level.style),
                            line_type: LineType::Simple,
                            color: level_color,
                        });
                    }
                }
            }
            DrawingBodyGeometry::Quad { corners } => {
                if drawing.fill_enabled {
                    let fill = area_fill(drawing, color);
                    let a = [corners[0].0 as f32, corners[0].1 as f32];
                    let b = [corners[1].0 as f32, corners[1].1 as f32];
                    let c = [corners[2].0 as f32, corners[2].1 as f32];
                    let d = [corners[3].0 as f32, corners[3].1 as f32];
                    out.push(Prim::Triangle {
                        a,
                        b,
                        c,
                        color: fill,
                    });
                    out.push(Prim::Triangle {
                        a,
                        b: c,
                        c: d,
                        color: fill,
                    });
                }
                for index in 0..4 {
                    push_segment(
                        corners[index],
                        corners[(index + 1) % 4],
                        drawing,
                        color,
                        vpr,
                        out,
                        points,
                    );
                }
            }
            DrawingBodyGeometry::Ellipse { center, rx, ry } => {
                if rx > 0.0 && ry > 0.0 {
                    // The fill halves and the outline share one tessellation, so the fill edge
                    // runs exactly under the stroke.
                    let segments = ellipse_segments(rx, ry);
                    let half = segments / 2;
                    if drawing.fill_enabled {
                        let fill = area_fill(drawing, color);
                        let upper_first = points.len() as u32;
                        for step in 0..=half {
                            let theta = std::f64::consts::PI * f64::from(step) / f64::from(half);
                            points.push([
                                (center.0 - rx * theta.cos()) as f32,
                                (center.1 - ry * theta.sin()) as f32,
                            ]);
                        }
                        let lower_first = points.len() as u32;
                        for step in 0..=half {
                            let theta = std::f64::consts::PI * f64::from(step) / f64::from(half);
                            points.push([
                                (center.0 - rx * theta.cos()) as f32,
                                (center.1 + ry * theta.sin()) as f32,
                            ]);
                        }
                        out.push(Prim::BandFill {
                            upper_first,
                            lower_first,
                            point_count: half + 1,
                            line_type: LineType::Simple,
                            fill,
                        });
                    }
                    let first_point = points.len() as u32;
                    for step in 0..=segments {
                        let theta = std::f64::consts::TAU * f64::from(step) / f64::from(segments);
                        points.push([
                            (center.0 + rx * theta.cos()) as f32,
                            (center.1 + ry * theta.sin()) as f32,
                        ]);
                    }
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: segments + 1,
                        width: (drawing.width * vpr) as f32,
                        style: drawing.style,
                        line_type: LineType::Simple,
                        color,
                    });
                }
            }
            DrawingBodyGeometry::Circle { center, radius } => {
                let fill = if drawing.fill_enabled {
                    area_fill(drawing, color)
                } else {
                    Color::rgba(0, 0, 0, 0)
                };
                if drawing.fill_enabled {
                    out.push(Prim::Circle {
                        cx: center.0 as f32,
                        cy: center.1 as f32,
                        radius: radius as f32,
                        fill,
                        stroke_width: 0.0,
                        stroke: color,
                    });
                }
                let segments = arc_segments(radius, std::f64::consts::TAU);
                let first_point = points.len() as u32;
                for step in 0..=segments {
                    let theta = std::f64::consts::TAU * f64::from(step) / f64::from(segments);
                    points.push([
                        (center.0 + radius * theta.cos()) as f32,
                        (center.1 + radius * theta.sin()) as f32,
                    ]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: segments + 1,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingBodyGeometry::Triangle { corners } => {
                if drawing.fill_enabled {
                    let fill = area_fill(drawing, color);
                    out.push(Prim::Triangle {
                        a: [corners[0].0 as f32, corners[0].1 as f32],
                        b: [corners[1].0 as f32, corners[1].1 as f32],
                        c: [corners[2].0 as f32, corners[2].1 as f32],
                        color: fill,
                    });
                }
                for index in 0..3 {
                    push_segment(
                        corners[index],
                        corners[(index + 1) % 3],
                        drawing,
                        color,
                        vpr,
                        out,
                        points,
                    );
                }
            }
            DrawingBodyGeometry::Arc(arc) => {
                let segments = arc.segments();
                let first_point = points.len() as u32;
                for step in 0..=segments {
                    let (x, y) = arc.point(f64::from(step) / f64::from(segments));
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: segments + 1,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingBodyGeometry::Curve(curve) => {
                let segments = curve.segments();
                let first_point = points.len() as u32;
                for step in 0..=segments {
                    let (x, y) = curve.point(f64::from(step) / f64::from(segments));
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: segments + 1,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingBodyGeometry::Rectangle {
                left,
                right,
                top,
                bottom,
            } => {
                let left = left.round() as i32;
                let right = right.round() as i32;
                let top = top.round() as i32;
                let bottom = bottom.round() as i32;
                // Official `positionsBox`: both endpoint pixels belong to the box, so an
                // equal-point preview still occupies one bitmap pixel.
                let width = (right - left).abs() + 1;
                let height = (bottom - top).abs() + 1;
                // reference rectangle-drawing-tool default: the fill is the border color washed
                // out (its `previewFillColor`/`fillColor` alpha pattern) — 20% here.
                let fill = area_fill(drawing, color);
                if drawing.fill_enabled {
                    out.push(Prim::Rect {
                        rect: IRect {
                            x: left,
                            y: top,
                            w: width,
                            h: height,
                        },
                        color: fill,
                    });
                }
                if drawing.middle_line_visible {
                    let line_color = drawing
                        .middle_line_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(color);
                    let line_width = (drawing.middle_line_width * vpr).round().max(1.0) as i32;
                    let y = top + (height - 1) / 2;
                    let (a, b) = (
                        (f64::from(left), f64::from(y)),
                        (f64::from(left + width), f64::from(y)),
                    );
                    let cutout = self.line_text_cutout(drawing, px, pane_w_px, vpr, a, b);
                    for ((x0, _), (x1, _)) in cut_segment(a, b, cutout) {
                        let (x0, x1) = (x0.round() as i32, x1.round() as i32);
                        if x0 != x1 {
                            out.push(Prim::HLine {
                                y,
                                x0: x0.min(x1),
                                x1: x0.max(x1),
                                width: line_width,
                                style: drawing.middle_line_style,
                                color: line_color,
                            });
                        }
                    }
                }
                if !drawing.border_visible {
                    return;
                }
                if drawing.style == LineStyle::Solid {
                    out.push(Prim::RectFrame {
                        rect: IRect {
                            x: left,
                            y: top,
                            w: width,
                            h: height,
                        },
                        border: crisp_width,
                        color,
                    });
                } else {
                    // Dotted/dashed border: four crisp line prims sharing the dash pattern,
                    // centered on the frame's inner edge (where RectFrame paints).
                    let half = (crisp_width as f64 / 2.0) as i32;
                    for y in [top + half, top + height - half] {
                        out.push(Prim::HLine {
                            y,
                            x0: left,
                            x1: left + width,
                            width: crisp_width,
                            style: drawing.style,
                            color,
                        });
                    }
                    for x in [left + half, left + width - half] {
                        out.push(Prim::VLine {
                            x,
                            y0: top,
                            y1: top + height,
                            width: crisp_width,
                            style: drawing.style,
                            color,
                        });
                    }
                }
            }
            DrawingBodyGeometry::Position(position) => {
                let (reward, risk) = position_colors(drawing, self.gain_loss_colors());
                push_position_zone(out, position.reward_zone(), reward);
                push_position_zone(out, position.risk_zone(), risk);

                let left_px = position.left.round() as i32;
                let right_px = position.right.round() as i32;
                if left_px != right_px {
                    let entry_path = [
                        [left_px as f32, position.entry_y as f32],
                        [right_px as f32, position.entry_y as f32],
                    ];
                    super::series_geometry::push_line_stroke(
                        out,
                        points,
                        &entry_path,
                        (POSITION_ENTRY_WIDTH_CSS * vpr) as f32,
                        drawing.style,
                        LineType::Simple,
                        POSITION_ENTRY,
                    );
                }
            }
            DrawingBodyGeometry::Measure(measure) => {
                self.build_measure_prims(drawing, measure, vpr, out, points);
            }
            // The text tool's geometry is its label (emitted by `build_drawing_text`).
            DrawingBodyGeometry::Empty => {}
            DrawingBodyGeometry::Polyline {
                points: line_points,
                line_type,
                terminal,
            } => {
                let color = if drawing.kind == DrawingKind::Highlighter {
                    Color::rgba(
                        color.r(),
                        color.g(),
                        color.b(),
                        ((color.a() as u16 * 64) / 255) as u8,
                    )
                } else {
                    color
                };
                let first_point = points.len() as u32;
                for &(x, y) in line_points {
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: line_points.len() as u32,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type,
                    color,
                });
                if let (Some(first), Some(last)) = (line_points.first(), line_points.last())
                    && line_points.len() >= 2
                {
                    push_drawing_cap(
                        drawing.stroke_start,
                        *first,
                        line_points[1],
                        drawing.width,
                        vpr,
                        color,
                        out,
                        points,
                    );
                    push_drawing_cap(
                        drawing.stroke_end,
                        *last,
                        line_points[line_points.len() - 2],
                        drawing.width,
                        vpr,
                        color,
                        out,
                        points,
                    );
                }
                if let Some(terminal) = terminal {
                    let first_point = points.len() as u32;
                    for (x, y) in terminal {
                        points.push([x as f32, y as f32]);
                    }
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: 3,
                        width: (drawing.width * vpr) as f32,
                        style: LineStyle::Solid,
                        line_type: LineType::Simple,
                        color,
                    });
                }
                if let Some(labels) = drawing.kind.vertex_labels() {
                    let label_color = vertex_label_color(drawing, color);
                    let size =
                        drawing.resolved_text_size(self.options.get().layout.font_size) * vpr;
                    let family = &self.options.get().layout.font_family;
                    let weight = drawing.text_weight.unwrap_or(400);
                    for (index, (&vertex, label)) in
                        line_points.iter().zip(labels.iter()).enumerate()
                    {
                        let text = if drawing.kind.is_elliott() {
                            format!("{label} ({})", drawing.wave_degree)
                        } else {
                            (*label).to_string()
                        };
                        // Peaks label above, troughs below, ends beyond the end: always in the
                        // open side of the legs that meet at the vertex.
                        let legs = [
                            index.checked_sub(1).and_then(|prev| line_points.get(prev)),
                            line_points.get(index + 1),
                        ];
                        let legs = legs.into_iter().flatten().copied().collect::<Vec<_>>();
                        let width =
                            self.measure_text_run(&text, size, family, weight, drawing.text_italic);
                        let (x, y) = clear_label_center(
                            vertex,
                            &legs,
                            width,
                            size * 1.2,
                            VERTEX_LABEL_GAP_CSS * vpr,
                            (0.0, -1.0),
                        );
                        out.push(Prim::Text {
                            x: x as f32,
                            y: y as f32,
                            text,
                            color: label_color,
                            size: size as f32,
                            family: self.options.get().layout.font_family.clone(),
                            align: TextAlign::Center,
                            weight: drawing.text_weight.unwrap_or(400),
                            italic: drawing.text_italic,
                        });
                    }
                }
            }
        }
        if drawing.kind == DrawingKind::Forecast
            && let (Some(entry), Some(target)) = (drawing.points.first(), px.get(1))
        {
            let change = drawing.points[1].price - entry.price;
            let percent = if entry.price.abs() > f64::EPSILON {
                change / entry.price.abs() * 100.0
            } else {
                0.0
            };
            let result = match self.forecast_result(drawing) {
                Some(true) => "target reached",
                Some(false) => "expired",
                None => "pending",
            };
            out.push(Prim::Text {
                x: target.0 as f32,
                y: (target.1 - 10.0 * vpr) as f32,
                text: format!("{:+.1}% · {}", percent, result),
                color,
                size: (self.options.get().layout.font_size * vpr) as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Center,
                weight: drawing.text_weight.unwrap_or(400),
                italic: drawing.text_italic,
            });
        }
    }

    fn build_bars_pattern_prims(
        &self,
        drawing: &Drawing,
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let color = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
        let ghost = Color::rgba(color.r(), color.g(), color.b(), 160);
        let hpr = f64::from(pane_w_px) / self.pane_w.max(1.0);
        let tick = (self.time_scale.bar_spacing() * hpr * 0.25)
            .round()
            .clamp(2.0, 6.0) as i32;
        let mode = drawing.bars_pattern_mode.as_str();
        let first_point = points.len() as u32;
        let mut line_count = 0_u32;
        for &bar in &drawing.bars_pattern {
            let projected = bar.project(drawing);
            let mut encoded = [(0.0, 0.0); 4];
            let mut valid = true;
            for (slot, point) in encoded.iter_mut().zip(projected) {
                if let Some((x, y)) =
                    self.drawing_to_px_for(drawing.pane_index, drawing.price_scale, point)
                {
                    *slot = (x * hpr, y * vpr);
                } else {
                    valid = false;
                    break;
                }
            }
            if !valid {
                continue;
            }
            let x = encoded[0].0.round() as i32;
            if mode == "bars" {
                if x < -tick || x > pane_w_px + tick {
                    continue;
                }
                out.push(Prim::VLine {
                    x,
                    y0: encoded[1].1.min(encoded[2].1).round() as i32,
                    y1: encoded[1].1.max(encoded[2].1).round() as i32,
                    width: (drawing.width * vpr).round().max(1.0) as i32,
                    style: drawing.style,
                    color: ghost,
                });
                out.push(Prim::HLine {
                    y: encoded[0].1.round() as i32,
                    x0: x - tick,
                    x1: x,
                    width: 1,
                    style: drawing.style,
                    color: ghost,
                });
                out.push(Prim::HLine {
                    y: encoded[3].1.round() as i32,
                    x0: x,
                    x1: x + tick,
                    width: 1,
                    style: drawing.style,
                    color: ghost,
                });
            } else {
                let index = match mode {
                    "line_open" => 0,
                    "line_high" => 1,
                    "line_low" => 2,
                    _ => 3,
                };
                points.push([encoded[index].0 as f32, encoded[index].1 as f32]);
                line_count += 1;
            }
        }
        if line_count >= 2 {
            out.push(Prim::Polyline {
                first_point,
                point_count: line_count,
                width: (drawing.width * vpr) as f32,
                style: drawing.style,
                line_type: LineType::Simple,
                color: ghost,
            });
        }
    }

    /// Volume profile drawings paint around one spine at the range start: total volume to the
    /// right (value-area rows in the value-area color), delta to the left, then the range
    /// edges (the full box for a fixed range), value-area lines and point of control. Rows and
    /// levels come from the shared media-space geometry, so hit testing and axis tags agree
    /// with these pixels.
    #[allow(clippy::too_many_arguments)]
    fn build_profile_drawing_prims(
        &self,
        drawing: &Drawing,
        snapshot: Option<&crate::ProfileDrawingSnapshot>,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let hpr = f64::from(pane_w_px) / self.pane_w.max(1.0);
        match snapshot {
            Some(crate::ProfileDrawingSnapshot::Volume(_)) | None => {
                let profile = match snapshot {
                    Some(crate::ProfileDrawingSnapshot::Volume(profile)) => Some(profile),
                    _ => None,
                };
                let anchors_x = px.iter().map(|point| point.0 / hpr).collect::<Vec<_>>();
                let Some(geometry) = self.profile_drawing_geometry(drawing, profile, &anchors_x)
                else {
                    return;
                };
                let colors = drawing.profile_colors();
                let snap_x = |x: f64| (x * hpr).round() as i32;
                let snap_y = |y: f64| (y * vpr).round() as i32;
                let spine = snap_x(geometry.spine);
                if let Some(profile) = profile {
                    self.for_each_profile_drawing_row(drawing, profile, &geometry, |row| {
                        let y = snap_y(row.top);
                        let pitch = (row.bottom - row.top) * vpr;
                        // Every row keeps one thickness and a one-device-pixel gap whenever the
                        // pitch allows it; rounding each row's own edges alternated thick and thin
                        // rows that merged into solid slabs.
                        let h = if pitch >= 2.0 {
                            (pitch.floor() as i32 - 1).max(1)
                        } else {
                            (snap_y(row.bottom) - y).max(1)
                        };
                        let right = snap_x(row.volume_right);
                        if right > spine {
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: spine,
                                    y,
                                    w: right - spine,
                                    h,
                                },
                                color: if row.value_area {
                                    colors.value_area
                                } else {
                                    colors.volume
                                },
                            });
                        }
                        let left = snap_x(row.delta_left);
                        if left < spine {
                            out.push(Prim::Rect {
                                rect: IRect {
                                    x: left,
                                    y,
                                    w: spine - left,
                                    h,
                                },
                                color: if row.positive {
                                    colors.positive
                                } else {
                                    colors.negative
                                },
                            });
                        }
                    });
                }
                let crisp = (drawing.width * vpr).round().max(1.0) as i32;
                // The box and spine are neutral chrome derived from the chart text color, so the
                // profile reads as a container rather than a colored rectangle drawing; the
                // drawing color stays on the value-area levels.
                let ink =
                    Color::parse_css(&self.options.get().layout.text_color).unwrap_or(colors.line);
                let edge = Color::rgba(ink.r(), ink.g(), ink.b(), PROFILE_FRAME_ALPHA);
                let right = snap_x(geometry.right);
                // Levels start at the spine so they never cross the delta histogram.
                let level_right = if drawing.extend_right {
                    pane_w_px
                } else {
                    right
                };
                let (top, bottom) = (snap_y(geometry.top), snap_y(geometry.bottom));
                let boxed = drawing.kind == DrawingKind::FixedRangeVolumeProfile;
                let hairline = vpr.round().max(1.0) as i32;
                if boxed {
                    for y in [top, bottom] {
                        out.push(Prim::HLine {
                            y,
                            x0: spine,
                            x1: right,
                            width: hairline,
                            style: LineStyle::Solid,
                            color: edge,
                        });
                    }
                }
                out.push(Prim::VLine {
                    x: spine,
                    y0: top,
                    y1: bottom,
                    width: hairline,
                    style: LineStyle::Solid,
                    color: edge,
                });
                if boxed {
                    out.push(Prim::VLine {
                        x: right,
                        y0: top,
                        y1: bottom,
                        width: hairline,
                        style: LineStyle::Solid,
                        color: edge,
                    });
                }
                if drawing.profile_value_area_lines_visible {
                    for (_, y) in [geometry.value_area_high, geometry.value_area_low]
                        .into_iter()
                        .flatten()
                    {
                        out.push(Prim::HLine {
                            y: snap_y(y),
                            x0: spine,
                            x1: level_right,
                            width: crisp,
                            style: drawing.style,
                            color: colors.line,
                        });
                    }
                }
                if drawing.profile_poc_visible
                    && let Some((_, y)) = geometry.poc
                {
                    out.push(Prim::HLine {
                        y: snap_y(y),
                        x0: spine,
                        x1: level_right,
                        width: (2.0 * vpr).round().max(1.0) as i32,
                        style: LineStyle::Solid,
                        color: colors.poc,
                    });
                }
            }
            Some(crate::ProfileDrawingSnapshot::Vwap(values)) => {
                let mut center = Vec::with_capacity(values.len());
                let mut upper = Vec::with_capacity(values.len());
                let mut lower = Vec::with_capacity(values.len());
                for value in values {
                    let seconds = value.timestamp_micros.div_euclid(1_000_000) as f64;
                    let Some(logical) = self.time_to_index(seconds, true).map(|index| index as f64)
                    else {
                        continue;
                    };
                    for (price, target) in [
                        (value.vwap, &mut center),
                        (value.upper_band, &mut upper),
                        (value.lower_band, &mut lower),
                    ] {
                        if let Some((x, y)) = self.drawing_to_px_for(
                            drawing.pane_index,
                            drawing.price_scale,
                            crate::DrawingPoint { logical, price },
                        ) {
                            target.push([(x * hpr) as f32, (y * vpr) as f32]);
                        }
                    }
                }
                let color = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
                super::series_geometry::push_line_stroke(
                    out,
                    points,
                    &center,
                    (drawing.width * vpr) as f32,
                    drawing.style,
                    LineType::Simple,
                    color,
                );
                let band = Color::rgba(color.r(), color.g(), color.b(), color.a().min(150));
                for path in [&upper, &lower] {
                    super::series_geometry::push_line_stroke(
                        out,
                        points,
                        path,
                        vpr.max(1.0) as f32,
                        LineStyle::Dashed,
                        LineType::Simple,
                        band,
                    );
                }
            }
        }
    }

    /// The text run's resolved glyph size (bitmap px, placeholder floor included), aligned
    /// anchor point, and horizontal alignment — shared by the label prim, the container box,
    /// and the focus/hover chrome so every consumer draws the same geometry.
    /// Label ink: explicit text color, then a line label's line color, then chart foreground.
    pub(crate) fn drawing_text_color(&self, drawing: &Drawing) -> Color {
        drawing
            .text_color
            .as_deref()
            .and_then(Color::parse_css)
            .or_else(|| {
                drawing
                    .kind
                    .has_line_text()
                    .then(|| Color::parse_css(&drawing.color))
                    .flatten()
            })
            .or_else(|| Color::parse_css(&self.options.get().layout.text_color))
            .unwrap_or_else(|| {
                let fallback = aeris_charts_core::style::DEFAULT_FOREGROUND_RGB;
                Color::rgb(fallback.0, fallback.1, fallback.2)
            })
    }

    /// The colors the frame paints for a drawing's color properties, keyed by property name, with
    /// unset (theme-following) values resolved the way the frame resolves them. A property with
    /// no single painted color is omitted: a position's labels contrast with each zone, and a box
    /// without a border color paints no border.
    pub(crate) fn drawing_painted_colors(&self, drawing: &Drawing) -> Vec<(String, Color)> {
        let stroke = Color::parse_css(&drawing.color).unwrap_or(PRIMARY);
        let area_stroke = if drawing.kind.is_measure() {
            self.measure_color(drawing)
        } else {
            stroke
        };
        let position = matches!(
            drawing.kind,
            DrawingKind::LongPosition | DrawingKind::ShortPosition
        );
        let annotation = drawing
            .kind
            .is_annotation()
            .then(|| self.annotation_palette(drawing, stroke));
        let (profit, loss) = position_colors(drawing, self.gain_loss_colors());
        crate::drawing_contract::drawing_property_schema(drawing.kind)
            .properties
            .into_iter()
            .filter(|property| {
                property.property_type == crate::drawing_contract::DrawingPropertyType::Color
            })
            .filter_map(|property| {
                let color = match (property.name.as_str(), annotation) {
                    ("color", _) => stroke,
                    ("position_profit_color", _) => profit,
                    ("position_loss_color", _) => loss,
                    ("fill_color", _) => area_fill(drawing, area_stroke),
                    ("box_color", Some((fill, ..))) => fill,
                    ("box_border_color", Some((_, border, ..))) => border?,
                    ("text_color", Some((_, _, ink, _))) => ink,
                    ("text_color", None) if position => return None,
                    ("text_color", None) if drawing.kind.vertex_labels().is_some() => {
                        vertex_label_color(drawing, stroke)
                    }
                    ("text_color", None) => self.drawing_text_color(drawing),
                    ("profile_value_area_color", _) => drawing.profile_colors().value_area,
                    ("profile_volume_color", _) => drawing.profile_colors().volume,
                    ("profile_positive_color", _) => drawing.profile_colors().positive,
                    ("profile_negative_color", _) => drawing.profile_colors().negative,
                    ("profile_poc_color", _) => drawing.profile_colors().poc,
                    _ => return None,
                };
                Some((property.name, color))
            })
            .collect()
    }

    /// The engine-painted selection and caret of an open drawing text session (native hosts).
    /// Both sit on the label's own transform and line layout and rotate with a trend label:
    /// the selection washes each selected line's run (plus a small nub for a selected line
    /// break) as one mesh, and the caret is a 1 px bar spanning its line's box, placed by the
    /// measured prefix on its line. An empty label uses the one-em editing slot the middle-line
    /// cutout reserves.
    fn build_drawing_text_session(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(session) = self
            .drawing_text_edit
            .as_ref()
            .filter(|session| session.id == drawing.id && session.paint_caret)
        else {
            return;
        };
        let Some(run) = self.text_run_geometry(drawing, px, pane_w_px, vpr) else {
            return;
        };
        let block = self.drawing_label_block(drawing, run.size);
        let measure = self.drawing_text_measurer(drawing, run.size);
        // Browser caret parity: offsets land on the next whole CSS px after the prefix run.
        let offset = |prefix: &str| (measure(prefix) / vpr).ceil() * vpr;
        let line_prefix = |index: usize, chars: usize| {
            let text = block.lines[index].text;
            &text[..text
                .char_indices()
                .nth(chars)
                .map_or(text.len(), |(byte, _)| byte)]
        };
        let snap = run.angle.abs() <= 1e-6;
        if let Some((start, end)) = session.selection() {
            let first_point = points.len() as u32;
            for (index, line) in block.lines.iter().enumerate() {
                let line_len = line.text.chars().count();
                let line_end = line.start + line_len;
                if end < line.start || start > line_end {
                    continue;
                }
                let from = start.max(line.start) - line.start;
                let to = end.min(line_end) - line.start;
                let left = block.line_left(index, run.align);
                let mut x0 = left + offset(line_prefix(index, from));
                let mut x1 = left + offset(line_prefix(index, to));
                if end > line_end {
                    x1 += run.size * 0.3;
                }
                let center = block.line_center(index);
                let (mut y0, mut y1) = (
                    center - block.line_height / 2.0,
                    center + block.line_height / 2.0,
                );
                if snap {
                    // An unrotated wash lands on whole device px like the crisp caret.
                    (x0, x1) = ((run.x + x0).round() - run.x, (run.x + x1).round() - run.x);
                    (y0, y1) = ((run.y + y0).round() - run.y, (run.y + y1).round() - run.y);
                }
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                let corner = |x: f64, y: f64| {
                    let (x, y) = run.screen((x, y));
                    [x as f32, y as f32]
                };
                let [a, b, c, d] = [
                    corner(x0, y0),
                    corner(x1, y0),
                    corner(x1, y1),
                    corner(x0, y1),
                ];
                points.extend([a, b, c, a, c, d]);
            }
            let point_count = points.len() as u32 - first_point;
            if point_count > 0 {
                out.push(Prim::MeshFill {
                    first_point,
                    point_count,
                    color: TEXT_SELECTION,
                });
            }
        }
        if !session.caret_shown {
            return;
        }
        let (line, prefix) = block.caret_prefix(session.caret);
        let local_x = block.line_left(line, run.align) + offset(prefix);
        let center = block.line_center(line);
        let half_height = block.line_height / 2.0;
        let color = self.drawing_text_color(drawing);
        if snap {
            // An unrotated caret snaps to the device grid like every other crisp 1px rule, so
            // it keeps one thickness wherever the caret moves.
            let width = vpr.round().max(1.0) as i32;
            let (top, bottom) = (
                (run.y + center - half_height).round(),
                (run.y + center + half_height).round(),
            );
            out.push(Prim::Rect {
                rect: IRect {
                    x: (run.x + local_x).round() as i32,
                    y: top as i32,
                    w: width,
                    h: (bottom - top).max(1.0) as i32,
                },
                color,
            });
            return;
        }
        let at = |local_y: f64| {
            let (x, y) = run.screen((local_x, local_y));
            [x as f32, y as f32]
        };
        let first_point = points.len() as u32;
        points.extend([at(center - half_height), at(center + half_height)]);
        out.push(Prim::Polyline {
            first_point,
            point_count: 2,
            width: vpr.max(1.0) as f32,
            style: LineStyle::Solid,
            line_type: LineType::Simple,
            color,
        });
    }

    /// The label's run in bitmap px (drawings.rs `drawing_text_run`).
    fn text_run_geometry(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
    ) -> Option<TextRun> {
        self.drawing_text_run(drawing, px, f64::from(pane_w_px), vpr)
    }

    /// A label block's crisp integer-snapped box padded by `pad` on every side.
    fn padded_block_rect(run: &TextRun, block: &TextBlock<'_>, pad: f64) -> IRect {
        let (left, top) = (run.x + block.left(run.align), run.y + block.top());
        IRect {
            x: (left - pad).round() as i32,
            y: (top - pad).round() as i32,
            w: (block.width + 2.0 * pad).round().max(1.0) as i32,
            h: (block.height() + 2.0 * pad).round().max(1.0) as i32,
        }
    }

    /// The text tool's interaction chrome (hover ring, focus border): a crisp integer-snapped
    /// hollow frame on the SAME box the host's editing wrap draws — the label's line block
    /// (widest line × one 1.2em line box per line, the hit test's convention) padded by the
    /// editing chrome's 2 px border + 4 px padding (drawings.rs `TEXT_CHROME_PAD`). Selection,
    /// hover, and typing mode land on one outline, so entering/leaving the editor moves nothing.
    fn push_text_chrome(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        color: Color,
        out: &mut Vec<Prim>,
    ) {
        let Some(run) = self.text_run_geometry(drawing, px, pane_w_px, vpr) else {
            return;
        };
        let block = self.drawing_label_block(drawing, run.size);
        out.push(Prim::RectFrame {
            rect: Self::padded_block_rect(&run, &block, TEXT_CHROME_PAD * vpr),
            border: (2.0 * vpr).round().max(1.0) as i32,
            color,
        });
    }

    /// One drawing's text label (every tool can carry one): the placement resolves the 3×3
    /// alignment against the tool's reference box, except lines, whose slots follow the line
    /// itself and whose middle slot opens a measured stroke gap. Line labels emit
    /// `Prim::RotatedText`; other drawing text emits `Prim::Text`. In either contract x is the
    /// aligned edge and y is the vertical center (the IR's middle-baseline convention). Empty
    /// standalone text paints nothing; an empty selected line paints its dedicated prompt at
    /// the canonical label transform. A text tool
    /// with a `box_color`/`box_border_color` gets its container (crisp integer-snapped
    /// `Rect`/`RectFrame` prims behind the run — the public reference's text-box background/border).
    fn build_drawing_text(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        // Empty text paints nothing. While the host typing-mode editor is open the LABEL and
        // the focus border still paint — the editor wrap is borderless with transparent glyphs,
        // so entering edit cannot lift the text or shift the outline (the public reference's
        // overlay-caret model).
        // Box annotations paint their own text with their box (`build_annotation_prims`).
        if drawing.kind.is_annotation() {
            return;
        }
        let Some((text, placeholder)) = self.drawing_frame_text(drawing) else {
            return;
        };
        let is_text_tool = drawing.kind == DrawingKind::Text || drawing.kind.is_text_annotation();
        let Some(run) = self.text_run_geometry(drawing, px, pane_w_px, vpr) else {
            return;
        };
        let layout = &self.options.get().layout;
        let mut color = self.drawing_text_color(drawing);
        if placeholder {
            color = Color::rgba(
                color.r(),
                color.g(),
                color.b(),
                color.a().min(LINE_TEXT_PLACEHOLDER_ALPHA),
            );
        }

        // The container (text tool with a background/border): a box wrapping the run, emitted
        // as the rectangle tool's crisp integer-snapped prims (`Rect` fill + `RectFrame`
        // border) — strong-color thin geometry at fractional positions AA-phases differently
        // between the backends, so the box snaps to whole device px (the public reference's boxes are
        // crisp the same way).
        let box_fill = drawing.box_color.as_deref().and_then(Color::parse_css);
        let box_border = drawing
            .box_border_color
            .as_deref()
            .and_then(Color::parse_css);
        if is_text_tool && (box_fill.is_some() || box_border.is_some()) {
            let block = self.drawing_label_block(drawing, run.size);
            let rect = Self::padded_block_rect(&run, &block, 4.0 * vpr);
            if let Some(fill) = box_fill {
                out.push(Prim::Rect { rect, color: fill });
            }
            if let Some(border) = box_border {
                out.push(Prim::RectFrame {
                    rect,
                    // Browser border semantics: whole device pixels, rounded down, at least one.
                    border: (drawing.box_border_width * vpr).floor().max(1.0) as i32,
                    color: border,
                });
            }
        }

        // One prim per line: backends never see a line break.
        let lines = TextBlock::unmeasured(text, run.size, run.v_align);
        for (x, y, line) in lines.line_anchors(run.x, run.y, run.angle) {
            let (x, y, text) = (x as f32, y as f32, line.to_string());
            let (size, family) = (run.size as f32, layout.font_family.clone());
            let (align, weight) = (text_align(run.align), drawing_text_weight(drawing));
            let italic = drawing.text_italic;
            out.push(if drawing.kind.has_line_text() {
                Prim::RotatedText {
                    x,
                    y,
                    text,
                    color,
                    size,
                    family,
                    align,
                    weight,
                    italic,
                    angle: run.angle as f32,
                }
            } else {
                Prim::Text {
                    x,
                    y,
                    text,
                    color,
                    size,
                    family,
                    align,
                    weight,
                    italic,
                }
            });
        }
    }

    fn build_drawing_labels(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(anchor) = px.first().copied() else {
            return;
        };
        // These kinds lay their metric labels out with their geometry (build_drawing_prims).
        if drawing.labels.is_empty()
            || matches!(
                drawing.kind,
                DrawingKind::TrendAngle | DrawingKind::InfoLine
            )
        {
            return;
        }
        let color = drawing
            .text_color
            .as_deref()
            .and_then(Color::parse_css)
            .or_else(|| Color::parse_css(&drawing.color))
            .unwrap_or_else(|| Color::rgb(255, 255, 255));
        let size = drawing.resolved_text_size(self.options.get().layout.font_size) * vpr;
        for (index, label) in drawing.labels.iter().enumerate() {
            if !label.visible {
                continue;
            }
            let value = label.text.clone().unwrap_or_else(|| {
                let first = drawing.points.first().map_or(0.0, |point| point.price);
                let second = drawing.points.get(1).map(|point| point.price);
                match label.metric {
                    crate::DrawingLabelMetric::Price => self.format_drawing_price(drawing, first),
                    crate::DrawingLabelMetric::PriceChange => second
                        .map(|value| self.format_drawing_price(drawing, value - first))
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::PercentChange => second
                        .filter(|_| first.abs() > f64::EPSILON)
                        .map(|value| format!("{:.2}%", (value - first) / first * 100.0))
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::Ticks => second
                        .map(|value| format!("{:.4}", value - first))
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::BarCount => drawing
                        .points
                        .get(1)
                        .map(|value| {
                            format!(
                                "{} bars",
                                (value.logical - drawing.points[0].logical).abs().round() as i64
                            )
                        })
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::DateTimeRange => "range".to_string(),
                    crate::DrawingLabelMetric::Duration => drawing
                        .points
                        .get(1)
                        .map(|value| {
                            format!(
                                "{:.2} bars",
                                (value.logical - drawing.points[0].logical).abs()
                            )
                        })
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::Angle => px
                        .get(1)
                        .map(|value| {
                            let dx = value.0 - px[0].0;
                            let dy = px[0].1 - value.1;
                            format!("{:.1}°", dy.atan2(dx).to_degrees())
                        })
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::Distance => drawing
                        .points
                        .get(1)
                        .map(|value| {
                            format!(
                                "{:.2}",
                                (value.logical - drawing.points[0].logical)
                                    .hypot(value.price - first)
                            )
                        })
                        .unwrap_or_default(),
                    crate::DrawingLabelMetric::VolumeInRange => "volume".to_string(),
                }
            });
            if value.is_empty() {
                continue;
            }
            let offset = (index as f64 + 1.0) * size * 1.25;
            let y = match label.position {
                crate::DrawingLabelPosition::Above => anchor.1 - offset,
                crate::DrawingLabelPosition::Below => anchor.1 + offset,
                crate::DrawingLabelPosition::Inside | crate::DrawingLabelPosition::On => anchor.1,
                crate::DrawingLabelPosition::Outside => anchor.1 + offset,
            };
            out.push(Prim::Text {
                x: anchor.0 as f32,
                y: y as f32,
                text: value,
                color,
                size: size as f32,
                family: self.options.get().layout.font_family.clone(),
                align: TextAlign::Left,
                weight: drawing.text_weight.unwrap_or(400),
                italic: drawing.text_italic,
            });
        }
    }

    /// Measured quantities between a measuring tool's start and end anchors. Bars and elapsed
    /// time come from the anchors' time slots (projected beyond canonical data like the time
    /// axis); ticks use the instrument tick, falling back to the bound scale's `min_move`.
    fn measure_stats(
        &self,
        drawing: &Drawing,
        start: DrawingPoint,
        end: DrawingPoint,
    ) -> MeasureStats {
        let price_change = end.price - start.price;
        let percent =
            (start.price.abs() > f64::EPSILON).then(|| price_change / start.price.abs() * 100.0);
        let ticks = self
            .position_price_tick(drawing.pane_index, drawing.price_scale)
            .map(|tick| (price_change / tick).round());
        let start_index = start.logical.round() as i64;
        let end_index = end.logical.round() as i64;
        let seconds = self
            .axis_time_seconds_at_logical(start_index)
            .zip(self.axis_time_seconds_at_logical(end_index))
            .map(|(from, to)| (to - from).round() as i64);
        MeasureStats {
            price_change,
            percent,
            ticks,
            bars: end_index - start_index,
            seconds,
        }
    }

    /// The measured direction picks the tool color: the date tool follows time, the price tools
    /// follow price. Rising/forward measurements use the drawing color, falling/backward ones
    /// the market-down token, so a pull upward reads positive and a pull downward negative.
    pub(crate) fn measure_color(&self, drawing: &Drawing) -> Color {
        let negative = match (drawing.points.first(), drawing.points.get(1)) {
            (Some(start), Some(end)) if drawing.kind == DrawingKind::DateRange => {
                end.logical.round() < start.logical.round()
            }
            (Some(start), Some(end)) => end.price < start.price,
            _ => false,
        };
        if negative {
            self.market_colors().down
        } else {
            Color::parse_css(&drawing.color).unwrap_or(PRIMARY)
        }
    }

    fn measure_label_lines(&self, drawing: &Drawing, stats: MeasureStats) -> Vec<String> {
        let Some(axes) = MeasureAxes::for_kind(drawing.kind) else {
            return Vec::new();
        };
        let mut lines = Vec::with_capacity(2);
        if axes.price() {
            let percent = stats
                .percent
                .map_or_else(|| "—".to_string(), |value| signed_stat(value, 2));
            let mut line = format!(
                "{} ({percent}%)",
                self.format_drawing_price(drawing, stats.price_change)
            );
            if let Some(ticks) = stats.ticks {
                line.push(' ');
                line.push_str(&signed_stat(ticks, 0));
            }
            lines.push(line);
        }
        if axes.date() {
            let bars = signed_stat(stats.bars as f64, 0);
            lines.push(match stats.seconds {
                Some(seconds) => format!("{bars} bars, {}", format_measure_duration(seconds)),
                None => format!("{bars} bars"),
            });
        }
        lines
    }

    /// The visible label of `metric`: `Some(custom text)` when shown, `None` when hidden/absent.
    fn visible_label(
        drawing: &Drawing,
        metric: crate::DrawingLabelMetric,
    ) -> Option<Option<String>> {
        drawing
            .labels
            .iter()
            .find(|label| label.metric == metric && label.visible)
            .map(|label| label.text.clone())
    }

    /// Trend Angle: a dotted horizontal reference from the first anchor, a dotted arc sweeping
    /// from it to the line, and the signed angle (counter-clockwise positive, as on a y-up
    /// chart) beside the reference's end.
    fn build_trend_angle_prims(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        color: Color,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (Some(&start), Some(&end)) = (px.first(), px.get(1)) else {
            return;
        };
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        if dx.hypot(dy) <= f64::EPSILON {
            return;
        }
        let radius = TREND_ANGLE_RADIUS_CSS * vpr;
        let width = vpr.round().max(1.0) as f32;
        let sweep = dy.atan2(dx);
        let first_point = points.len() as u32;
        points.extend([
            [start.0 as f32, start.1 as f32],
            [(start.0 + radius) as f32, start.1 as f32],
        ]);
        out.push(Prim::Polyline {
            first_point,
            point_count: 2,
            width,
            style: LineStyle::Dotted,
            line_type: LineType::Simple,
            color,
        });
        let steps = arc_segments(radius, sweep);
        let first_point = points.len() as u32;
        for step in 0..=steps {
            let angle = sweep * f64::from(step) / f64::from(steps);
            points.push([
                (start.0 + radius * angle.cos()) as f32,
                (start.1 + radius * angle.sin()) as f32,
            ]);
        }
        out.push(Prim::Polyline {
            first_point,
            point_count: steps + 1,
            width,
            style: LineStyle::Dotted,
            line_type: LineType::Simple,
            color,
        });
        let Some(custom) = Self::visible_label(drawing, crate::DrawingLabelMetric::Angle) else {
            return;
        };
        let layout = &self.options.get().layout;
        out.push(Prim::Text {
            x: (start.0 + radius + TREND_ANGLE_LABEL_GAP_CSS * vpr) as f32,
            y: start.1 as f32,
            text: custom.unwrap_or_else(|| line_angle_text(-sweep.to_degrees())),
            color,
            size: (stat_label_size(layout.font_size) * vpr) as f32,
            family: layout.font_family.clone(),
            align: TextAlign::Left,
            weight: drawing.text_weight.unwrap_or(400),
            italic: drawing.text_italic,
        });
    }

    /// The Info Line's statistics card rows: price change / percent / ticks, bars / elapsed
    /// time / pixel distance, and angle. Each row lists only its visible metrics.
    fn info_line_rows(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        hpr: f64,
        vpr: f64,
    ) -> Vec<(InfoLineIcon, String)> {
        use crate::DrawingLabelMetric as Metric;
        let (Some(&start), Some(&end)) = (drawing.points.first(), drawing.points.get(1)) else {
            return Vec::new();
        };
        let (Some(&a), Some(&b)) = (px.first(), px.get(1)) else {
            return Vec::new();
        };
        let stats = self.measure_stats(drawing, start, end);
        let mut rows = Vec::with_capacity(3);

        let mut price = String::new();
        if let Some(custom) = Self::visible_label(drawing, Metric::PriceChange) {
            price =
                custom.unwrap_or_else(|| self.format_drawing_price(drawing, stats.price_change));
        }
        if let Some(custom) = Self::visible_label(drawing, Metric::PercentChange) {
            let percent = custom.unwrap_or_else(|| {
                stats.percent.map_or_else(
                    || "—".to_string(),
                    |value| format!("{}%", signed_stat(value, 2)),
                )
            });
            price = if price.is_empty() {
                percent
            } else {
                format!("{price} ({percent})")
            };
        }
        if let Some(custom) = Self::visible_label(drawing, Metric::Ticks) {
            let ticks = custom.unwrap_or_else(|| {
                stats
                    .ticks
                    .map_or_else(|| "—".to_string(), |value| signed_stat(value, 0))
            });
            price = if price.is_empty() {
                ticks
            } else {
                format!("{price}, {ticks}")
            };
        }
        if !price.is_empty() {
            rows.push((InfoLineIcon::Price, price));
        }

        let mut time = String::new();
        if let Some(custom) = Self::visible_label(drawing, Metric::BarCount) {
            time = custom.unwrap_or_else(|| format!("{} bars", signed_stat(stats.bars as f64, 0)));
        }
        let duration = Self::visible_label(drawing, Metric::Duration)
            .or_else(|| Self::visible_label(drawing, Metric::DateTimeRange));
        if let Some(custom) = duration
            && let Some(elapsed) = custom.or_else(|| stats.seconds.map(format_measure_duration))
        {
            time = if time.is_empty() {
                elapsed
            } else {
                format!("{time} ({elapsed})")
            };
        }
        if let Some(custom) = Self::visible_label(drawing, Metric::Distance) {
            let distance = custom.unwrap_or_else(|| {
                let css = ((b.0 - a.0) / hpr).hypot((b.1 - a.1) / vpr);
                format!("distance: {} px", css.round() as i64)
            });
            time = if time.is_empty() {
                distance
            } else {
                format!("{time}, {distance}")
            };
        }
        if !time.is_empty() {
            rows.push((InfoLineIcon::Time, time));
        }

        if let Some(custom) = Self::visible_label(drawing, Metric::Angle) {
            rows.push((
                InfoLineIcon::Angle,
                custom.unwrap_or_else(|| {
                    let degrees = (-(b.1 - a.1) / vpr).atan2((b.0 - a.0) / hpr).to_degrees();
                    line_angle_text(degrees)
                }),
            ));
        }
        rows
    }

    /// A box annotation from the shared layout (annotations.rs): its connector, the rounded box
    /// (the callout's box and tail as one outlined shape), then the text, all in the colors of
    /// [`Self::annotation_palette`].
    fn build_annotation_prims(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        color: Color,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Some(layout) = self.annotation_layout(drawing, px, vpr) else {
            return;
        };
        let (fill, border, ink, muted) = self.annotation_palette(drawing, color);
        let border_width = if drawing.kind == DrawingKind::Callout {
            (2.0 * vpr).round().max(1.0)
        } else {
            (drawing.box_border_width * vpr).floor().max(1.0)
        };
        let mut stroke = |a: (f64, f64), b: (f64, f64), width: f64, color: Color| {
            let first_point = points.len() as u32;
            points.extend([[a.0 as f32, a.1 as f32], [b.0 as f32, b.1 as f32]]);
            out.push(Prim::Polyline {
                first_point,
                point_count: 2,
                width: width as f32,
                style: drawing.style,
                line_type: LineType::Simple,
                color,
            });
        };
        if let Some((a, b)) = layout.connector {
            stroke(a, b, drawing.width * vpr, color);
        }
        let [left, top, width, height] = layout.rect;
        if let Some(outline) = &layout.outline {
            // The callout is one shape: the box fill, the tail fill tucked into the box so the two
            // surfaces overlap instead of meeting at an anti-aliased seam, then one closed
            // outline stroked around both, so the border runs into the tail without a break.
            out.push(Prim::RoundRect {
                x: left as f32,
                y: top as f32,
                w: width as f32,
                h: height as f32,
                radii: layout.radii.map(|radius| radius as f32),
                fill,
                border_width: 0.0,
                border_color: fill,
            });
            if let Some([tip, base_a, base_b]) = layout.tail {
                let center = (left + width / 2.0, top + height / 2.0);
                let tuck = |(x, y): (f64, f64)| {
                    let (dx, dy) = (center.0 - x, center.1 - y);
                    let distance = dx.hypot(dy).max(f64::EPSILON);
                    let inset = border_width + vpr;
                    [
                        (x + dx / distance * inset) as f32,
                        (y + dy / distance * inset) as f32,
                    ]
                };
                out.push(Prim::Triangle {
                    a: [tip.0 as f32, tip.1 as f32],
                    b: tuck(base_a),
                    c: tuck(base_b),
                    color: fill,
                });
            }
            let first_point = points.len() as u32;
            points.extend(outline.iter().map(|&(x, y)| [x as f32, y as f32]));
            out.push(Prim::Polyline {
                first_point,
                point_count: outline.len() as u32,
                width: border_width as f32,
                style: LineStyle::Solid,
                line_type: LineType::Simple,
                color: border.unwrap_or(color),
            });
        } else {
            // The box snaps to whole device pixels so its edges stay crisp on every backend.
            let (x0, y0) = (left.round(), top.round());
            let (x1, y1) = ((left + width).round(), (top + height).round());
            out.push(Prim::RoundRect {
                x: x0 as f32,
                y: y0 as f32,
                w: (x1 - x0).max(1.0) as f32,
                h: (y1 - y0).max(1.0) as f32,
                radii: layout.radii.map(|radius| radius.round() as f32),
                fill,
                border_width: if border.is_some() {
                    border_width as f32
                } else {
                    0.0
                },
                border_color: border.unwrap_or(fill),
            });
        }
        let text_color = if layout.placeholder { muted } else { ink };
        let lines = TextBlock::unmeasured(&layout.text, layout.size, drawing.label_v_align());
        for (x, y, line) in lines.line_anchors(layout.text_x, layout.text_y, 0.0) {
            out.push(Prim::Text {
                x: x as f32,
                y: y as f32,
                text: line.to_string(),
                color: text_color,
                size: layout.size as f32,
                family: self.options.get().layout.font_family.clone(),
                align: text_align(drawing.label_align()),
                weight: drawing_text_weight(drawing),
                italic: drawing.text_italic,
            });
        }
    }

    /// A box annotation's painted (box fill, box border, text ink, placeholder ink). Unset box
    /// colors follow the painted theme's tokens (note, signpost) or the drawing `color` (comment
    /// and price-note fills, the callout's opaque tint and border); the ink is `text_color`, else
    /// a token or the contrast of the fill. `None` border: the box paints no border.
    fn annotation_palette(
        &self,
        drawing: &Drawing,
        color: Color,
    ) -> (Color, Option<Color>, Color, Color) {
        let tokens = ChromeTokens::for_theme(self.surface_theme());
        let parse = |css: &Option<String>| css.as_deref().and_then(Color::parse_css);
        let (box_fill, box_border) = (parse(&drawing.box_color), parse(&drawing.box_border_color));
        let (fill, border, ink, muted) = match drawing.kind {
            DrawingKind::Note => (
                box_fill.unwrap_or(tokens.accent),
                box_border,
                tokens.foreground,
                tokens.muted,
            ),
            DrawingKind::Signpost => (
                box_fill.unwrap_or(tokens.surface),
                Some(box_border.unwrap_or(tokens.border)),
                tokens.foreground,
                tokens.muted,
            ),
            // An opaque tint of the drawing color over the chart background: the box and its
            // tail fill as one surface, so their overlap never doubles any alpha.
            DrawingKind::Callout => (
                box_fill.unwrap_or_else(|| {
                    let background = Color::parse_css(&self.options.get().layout.background.color)
                        .unwrap_or(tokens.surface);
                    let mix = |a: u8, b: u8| {
                        ((f64::from(a) * 0.55) + (f64::from(b) * 0.45)).round() as u8
                    };
                    Color::rgb(
                        mix(background.r(), color.r()),
                        mix(background.g(), color.g()),
                        mix(background.b(), color.b()),
                    )
                }),
                Some(box_border.unwrap_or(color)),
                tokens.foreground,
                tokens.muted,
            ),
            _ => {
                let fill = box_fill.unwrap_or(color);
                let ink = fill.contrast_text();
                (
                    fill,
                    box_border,
                    ink,
                    Color::rgba(ink.r(), ink.g(), ink.b(), 170),
                )
            }
        };
        let ink = drawing
            .text_color
            .as_deref()
            .and_then(Color::parse_css)
            .unwrap_or(ink);
        (fill, border, ink, muted)
    }

    /// A straight line's optional midpoint dot and anchor price labels (above an anchor that
    /// sits higher than the other end, below the lower one, so neither covers the line).
    fn build_line_point_marks(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        color: Color,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let (Some(&a), Some(&b)) = (px.first(), px.get(1)) else {
            return;
        };
        if drawing.middle_point_visible {
            let mid = point_on_segment(a, b, 0.5);
            out.push(Prim::Circle {
                cx: mid.0 as f32,
                cy: mid.1 as f32,
                radius: ((drawing.width / 2.0 + 2.0) * vpr) as f32,
                fill: color,
                stroke_width: 0.0,
                stroke: color,
            });
        }
        let (Some(&start), Some(&end)) = (drawing.points.first(), drawing.points.get(1)) else {
            return;
        };
        if !drawing.price_labels_visible {
            return;
        }
        let layout = &self.options.get().layout;
        let size = drawing.resolved_text_size(layout.font_size) * vpr;
        let offset = POINT_LABEL_GAP_CSS * vpr + size * 0.6;
        for ((x, y), other, point) in [(a, b.1, start), (b, a.1, end)] {
            out.push(Prim::Text {
                x: x as f32,
                y: (if y <= other { y - offset } else { y + offset }) as f32,
                text: self.format_drawing_price(drawing, point.price),
                color,
                size: size as f32,
                family: layout.font_family.clone(),
                align: TextAlign::Center,
                weight: drawing.text_weight.unwrap_or(400),
                italic: drawing.text_italic,
            });
        }
    }

    /// A line's statistics card: a rounded panel beside the segment's midpoint (right of it,
    /// left of it, or centered per `stats_position`), on the vertical side the line leaves free
    /// (above a falling line, below a rising one), kept inside the pane. It is painted with the
    /// brushable-area tooltip's popover chrome: same fill, hairline, radius and inner highlight.
    fn build_info_line_prims(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (Some(&a), Some(&b)) = (px.first(), px.get(1)) else {
            return;
        };
        let Some(pane) = self.panes.get(drawing.pane_index) else {
            return;
        };
        let hpr = f64::from(pane_w_px) / self.pane_w.max(1.0);
        let rows = self.info_line_rows(drawing, px, hpr, vpr);
        if rows.is_empty() {
            return;
        }
        let layout = &self.options.get().layout;
        let popover = self.popover_tokens();
        let size = INFO_LINE_FONT_CSS * vpr;
        let line_height = INFO_LINE_LINE_HEIGHT_CSS * vpr;
        let row_gap = INFO_LINE_ROW_GAP_CSS * vpr;
        let pad_x = INFO_LINE_PAD_X_CSS * vpr;
        let (pad_top, pad_bottom) = (INFO_LINE_PAD_TOP_CSS * vpr, INFO_LINE_PAD_BOTTOM_CSS * vpr);
        let icon = INFO_LINE_ICON_CSS * vpr;
        let text_inset = pad_x + icon + INFO_LINE_ICON_GAP_CSS * vpr;
        let text_width = rows
            .iter()
            .map(|(_, text)| self.measure_text_run(text, size, &layout.font_family, 400, false))
            .fold(0.0_f64, f64::max);
        let width = (text_inset + text_width + pad_x).round();
        let height = (pad_top
            + rows.len() as f64 * line_height
            + (rows.len() - 1) as f64 * row_gap
            + pad_bottom)
            .round();
        let mid = point_on_segment(a, b, 0.5);
        let gap_x = 12.0 * vpr;
        let gap_y = 16.0 * vpr;
        let pane_top = pane.top * vpr;
        let pane_bottom = (pane.top + pane.height) * vpr;
        let left = match drawing.stats_position {
            DrawingTextHAlign::Left => mid.0 - gap_x - width,
            DrawingTextHAlign::Center => mid.0 - width / 2.0,
            DrawingTextHAlign::Right => mid.0 + gap_x,
        }
        .min(f64::from(pane_w_px) - width)
        .max(0.0)
        .round();
        let top = if (b.0 - a.0) * (b.1 - a.1) >= 0.0 {
            mid.1 - gap_y - height
        } else {
            mid.1 + gap_y
        }
        .min(pane_bottom - height)
        .max(pane_top)
        .round();
        let card = (left as f32, top as f32, width as f32, height as f32);
        let ratio = hpr.min(vpr);
        let radius = (aeris_charts_core::style::RADIUS_COMPACT * ratio).round() as f32;
        let border = aeris_charts_core::style::border_width_device_px(ratio) as f32;
        out.push(Prim::RoundRect {
            x: card.0,
            y: card.1,
            w: card.2,
            h: card.3,
            radii: [radius; 4],
            fill: popover.surface,
            border_width: border,
            border_color: popover.border,
        });
        for (index, (kind, text)) in rows.into_iter().enumerate() {
            let center_y =
                top + pad_top + (line_height + row_gap) * index as f64 + line_height / 2.0;
            push_info_line_icon(
                kind,
                (left + pad_x + icon / 2.0, center_y),
                popover.muted,
                vpr,
                out,
                points,
            );
            out.push(Prim::Text {
                x: (left + text_inset) as f32,
                y: center_y as f32,
                text,
                color: popover.foreground,
                size: size as f32,
                family: layout.font_family.clone(),
                align: TextAlign::Left,
                weight: 400,
                italic: false,
            });
        }
        out.push(popover.highlight_prim(card, radius, border));
    }

    /// One measuring tool: translucent measured area, crisp boundary rules, start→end arrows,
    /// and a solid statistics label beyond the end (below for the date tool). Every edge snaps to
    /// whole device pixels and arrow tips sit on the shaft's pixel center, so all executors
    /// receive identical crisp rects plus one shared chevron stroke per arrow.
    fn build_measure_prims(
        &self,
        drawing: &Drawing,
        measure: MeasureGeometry,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let (Some(&start), Some(&end)) = (drawing.points.first(), drawing.points.get(1)) else {
            return;
        };
        let color = self.measure_color(drawing);
        let width = (drawing.width * vpr).round().max(1.0) as i32;
        let left = measure.left().round() as i32;
        let right = measure.right().round() as i32;
        let top = measure.top().round() as i32;
        let bottom = measure.bottom().round() as i32;
        let (start_x, start_y) = (
            measure.start.0.round() as i32,
            measure.start.1.round() as i32,
        );
        let (end_x, end_y) = (measure.end.0.round() as i32, measure.end.1.round() as i32);
        if drawing.fill_enabled {
            let fill = area_fill(drawing, color);
            // Both endpoint pixels belong to the area, matching the rectangle tool's box.
            out.push(Prim::Rect {
                rect: IRect {
                    x: left,
                    y: top,
                    w: right - left + 1,
                    h: bottom - top + 1,
                },
                color: fill,
            });
        }
        match measure.axes {
            MeasureAxes::Price => {
                for y in [start_y, end_y] {
                    out.push(Prim::HLine {
                        y,
                        x0: left,
                        x1: right + 1,
                        width,
                        style: drawing.style,
                        color,
                    });
                }
            }
            MeasureAxes::Date => {
                for x in [start_x, end_x] {
                    out.push(Prim::VLine {
                        x,
                        y0: top,
                        y1: bottom + 1,
                        width,
                        style: drawing.style,
                        color,
                    });
                }
            }
            MeasureAxes::DatePrice => {}
        }
        if measure.axes.price() && start_y != end_y {
            let x = (left + right).div_euclid(2);
            out.push(Prim::VLine {
                x,
                y0: start_y.min(end_y),
                y1: start_y.max(end_y) + 1,
                width,
                style: LineStyle::Solid,
                color,
            });
            // VLine covers columns `[x - width/2, x - width/2 + width)`; its center is the tip.
            let center = f64::from(x - width / 2) + f64::from(width) / 2.0;
            let tip = (center, f64::from(end_y) + 0.5);
            let direction = if end_y > start_y { 1.0 } else { -1.0 };
            push_measure_arrowhead(tip, (0.0, direction), width, vpr, color, out, points);
        }
        if measure.axes.date() && start_x != end_x {
            let y = (top + bottom).div_euclid(2);
            out.push(Prim::HLine {
                y,
                x0: start_x.min(end_x),
                x1: start_x.max(end_x) + 1,
                width,
                style: LineStyle::Solid,
                color,
            });
            let center = f64::from(y - width / 2) + f64::from(width) / 2.0;
            let tip = (f64::from(end_x) + 0.5, center);
            let direction = if end_x > start_x { 1.0 } else { -1.0 };
            push_measure_arrowhead(tip, (direction, 0.0), width, vpr, color, out, points);
        }

        let lines = self.measure_label_lines(drawing, self.measure_stats(drawing, start, end));
        if lines.is_empty() {
            return;
        }
        let layout = &self.options.get().layout;
        let half_height = stat_label_height(layout.font_size, lines.len()) * vpr / 2.0;
        let gap = MEASURE_LABEL_GAP * vpr;
        let center_x = (measure.left() + measure.right()) / 2.0;
        // Price labels sit beyond the end level (above a rise, below a fall); the date tool
        // labels below its range.
        let center_y = if measure.axes.price() && measure.end.1 < measure.start.1 {
            measure.end.1 - gap - half_height
        } else if measure.axes.price() {
            measure.end.1.max(measure.start.1) + gap + half_height
        } else {
            measure.bottom() + gap + half_height
        };
        self.push_stat_label_block(out, (center_x, center_y), &lines, (color, None), vpr);
    }

    /// A price-valued drawing statistic in the bound scale's price format.
    pub(crate) fn format_drawing_price(&self, drawing: &Drawing, value: f64) -> String {
        let scale_target = match drawing.price_scale {
            crate::DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            crate::DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            crate::DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        self.format_scale_price(drawing.pane_index, scale_target, value)
    }

    /// The price label's bubble in the anchor's px basis (`scale` is the bitmap/media ratio;
    /// 1.0 at hit-test). The bubble rises above-right of the anchor and its tail's tip touches
    /// the anchor, so the label and its handle are one attached shape.
    pub(crate) fn price_label_layout(
        &self,
        drawing: &Drawing,
        anchor: (f64, f64),
        scale: f64,
    ) -> PriceLabelLayout {
        let text = if drawing.text.is_empty() {
            self.format_drawing_price(drawing, drawing.points[0].price)
        } else {
            drawing.text.clone()
        };
        let layout = &self.options.get().layout;
        let size = drawing.resolved_text_size(layout.font_size) * scale;
        let block = self.drawing_text_block(
            drawing,
            &text,
            size,
            crate::drawings::DrawingTextVAlign::Middle,
            &layout.font_family,
        );
        let (text_width, text_height) = (block.width, block.height());
        let padding = 6.0 * scale;
        let tail = 6.0 * scale;
        let height = text_height + 8.0 * scale;
        PriceLabelLayout {
            rect: [
                anchor.0,
                anchor.1 - tail - height,
                text_width + 2.0 * padding,
                height,
            ],
            text,
            size,
            padding,
            tail,
        }
    }

    fn build_position_labels(
        &self,
        drawing: &Drawing,
        position: PositionGeometry,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let (reward, risk) = position_colors(drawing, self.gain_loss_colors());
        let ink = drawing.text_color.as_deref().and_then(Color::parse_css);
        let (Some(entry), Some(target), Some(stop)) = (
            drawing.points.first(),
            drawing.points.get(1),
            drawing.points.get(2),
        ) else {
            return;
        };
        let reward_distance = (target.price - entry.price).abs();
        let risk_distance = (entry.price - stop.price).abs();
        let base = entry.price.abs();
        let reward_percent = if base > f64::EPSILON {
            reward_distance / base * 100.0
        } else {
            0.0
        };
        let risk_percent = if base > f64::EPSILON {
            risk_distance / base * 100.0
        } else {
            0.0
        };
        let scale_target = match drawing.price_scale {
            crate::DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            crate::DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            crate::DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        let tick = self.position_price_tick(drawing.pane_index, drawing.price_scale);
        let format_price = |value: f64| self.format_drawing_price(drawing, value);
        let ticks = |distance: f64| {
            tick.map_or_else(
                || "—".to_string(),
                |tick| position_stat_number(distance / tick, 0),
            )
        };
        let point_value = self.trading_state.instrument.point_value.unwrap_or(1.0);
        let risk_amount = drawing.position_account_size * drawing.position_risk_percent / 100.0;
        let quantity = if risk_distance > 0.0 {
            risk_amount / risk_distance / point_value
        } else {
            f64::NAN
        };
        let quantity_precision = self
            .trading_state
            .instrument
            .quantity_precision
            .unwrap_or(3) as usize;
        let target_amount =
            drawing.position_account_size + reward_distance * quantity * point_value;
        let stop_amount = drawing.position_account_size - risk_distance * quantity * point_value;
        let target_text = format!(
            "Target: {} ({reward_percent:.3}%) {}, Amount: {}",
            format_price(reward_distance),
            ticks(reward_distance),
            position_stat_number(target_amount, 2),
        );
        let stop_text = format!(
            "Stop: {} ({risk_percent:.3}%) {}, Amount: {}",
            format_price(risk_distance),
            ticks(risk_distance),
            position_stat_number(stop_amount, 2),
        );
        let run = self.position_run_progress(drawing);
        // Unfilled/historical/future drawings still show the right-edge or latest close.
        // This is an estimate, not a broker execution; preserve the run's exact frozen exit.
        let current = run.map(|run| run.point.price).or_else(|| {
            let series = self.scale_formatter_source(drawing.pane_index, scale_target)?;
            if series.kind == crate::SeriesKind::Custom {
                return None;
            }
            let plot = self.data.plot(series.id);
            let row = plot.last_non_whitespace_row(target.logical.floor() as i64)?;
            let close = plot.value_at(row, PlotValueIndex::Close);
            close.is_finite().then_some(close)
        });
        let direction = if drawing.kind == DrawingKind::LongPosition {
            1.0
        } else {
            -1.0
        };
        let pnl = current.map(|price| (price - entry.price) * direction);
        let status = if run.is_some_and(|run| run.closed) {
            "Closed"
        } else {
            "Open"
        };
        let middle = [
            format!(
                "{status} P&L: {}, Qty: {}",
                pnl.map_or_else(|| "—".to_string(), format_price),
                position_stat_number(quantity, quantity_precision)
            ),
            format!(
                "Risk/reward ratio: {}",
                position_stat_number(reward_distance / risk_distance, 2)
            ),
        ];
        let center_x = (position.left + position.right) / 2.0;
        let label_offset = 14.0 * vpr;
        let target_label_y = if position.target_y < position.entry_y {
            position.target_y - label_offset
        } else {
            position.target_y + label_offset
        };
        let stop_label_y = if position.stop_y < position.entry_y {
            position.stop_y - label_offset
        } else {
            position.stop_y + label_offset
        };
        self.push_stat_label_block(
            out,
            (center_x, target_label_y),
            &[target_text],
            (reward, ink),
            vpr,
        );
        self.push_stat_label_block(
            out,
            (center_x, stop_label_y),
            &[stop_text],
            (risk, ink),
            vpr,
        );
        let pnl_color = if pnl.is_some_and(|value| value < 0.0) {
            risk
        } else {
            reward
        };
        self.push_stat_label_block(
            out,
            (center_x, position.entry_y),
            &middle,
            (pnl_color, ink),
            vpr,
        );
    }

    /// Dynamic position progress is rebuilt with pane chrome rather than retained drawing
    /// geometry: series updates already invalidate chrome, so the darker traversed fill and
    /// terminal-candle trend can follow data without rebuilding every drawing on each tick.
    /// `parts` records each position's range so reassembly paints it in its drawing's z-slot.
    pub(super) fn build_position_progress_frame(
        &self,
        pane_index: usize,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
        parts: &mut Vec<super::RetainedDrawingPart>,
        hpr: f64,
        vpr: f64,
    ) {
        for drawing in self.drawings.iter().filter(|drawing| {
            drawing.pane_index == pane_index
                && drawing.visible
                && drawing.interval_visibility.allows(self.drawing_interval)
                && matches!(
                    drawing.kind,
                    DrawingKind::LongPosition | DrawingKind::ShortPosition
                )
                && drawing.points.len() == 3
        }) {
            let Some(run) = self.position_run_progress(drawing) else {
                continue;
            };
            let run_point = run.point;
            let run_start = run.start;
            let entry = drawing.points[0].price;
            if !run_point.price.is_finite() || !entry.is_finite() {
                continue;
            }

            let Some(px) = self.drawing_px(drawing) else {
                continue;
            };
            let px = px
                .into_iter()
                .map(|(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>();
            let Some(geometry) = resolve_drawing_geometry(
                drawing.kind,
                &px,
                self.pane_w * hpr,
                self.panes[pane_index].top * vpr,
                self.panes[pane_index].height * vpr,
                DrawingGeometryOptions {
                    line_width: drawing.width,
                    device_scale: vpr,
                    extend_left: drawing.extend_left,
                    icon_size: drawing.icon_size,
                    extend_right: drawing.extend_right,
                },
            ) else {
                continue;
            };
            let DrawingBodyGeometry::Position(position) = geometry.body else {
                continue;
            };

            // The progress pivot is the first post-placement candle that actually reaches/crosses
            // the entry. A position that has not filled emits no progress geometry at all.
            let Some((start_x, _)) =
                self.drawing_to_px_for(pane_index, drawing.price_scale, run_start)
            else {
                continue;
            };
            let Some((run_x, run_y)) =
                self.drawing_to_px_for(pane_index, drawing.price_scale, run_point)
            else {
                continue;
            };
            let start_x = (start_x * hpr).clamp(position.left, position.right);
            let run_x = (run_x * hpr).clamp(position.left, position.right);
            let run_y = run_y * vpr;
            let (reward, risk) = position_colors(drawing, self.gain_loss_colors());
            let semantic = match run.side {
                PositionRunSide::Reward => reward,
                PositionRunSide::Risk => risk,
            };

            // Stronger opacity represents only the price/time space actually travelled since the
            // fill: first-fill x -> current/terminal x, entry y -> current/terminal y. It never
            // darkens the untouched remainder of either TP/SL zone.
            let (prim_start, point_start) = (out.len(), points.len());
            let travel_left = start_x.min(run_x);
            let travel_right = start_x.max(run_x);
            if travel_right > travel_left && (run_y - position.entry_y).abs() > f64::EPSILON {
                push_position_zone_with_alpha(
                    out,
                    PositionZone {
                        left: travel_left,
                        right: travel_right,
                        y0: position.entry_y,
                        y1: run_y,
                    },
                    semantic,
                    POSITION_PROGRESS_ALPHA,
                );
            }
            let progress_path = [
                [start_x as f32, position.entry_y as f32],
                [run_x as f32, run_y as f32],
            ];
            if progress_path[0] != progress_path[1] {
                super::series_geometry::push_line_stroke(
                    out,
                    points,
                    &progress_path,
                    // A hairline: one whole device pixel, never a fractional-DPR widened stroke.
                    vpr.floor().max(1.0) as f32,
                    LineStyle::Dashed,
                    LineType::Simple,
                    POSITION_ENTRY,
                );
            }
            if out.len() != prim_start {
                parts.push(super::RetainedDrawingPart {
                    id: drawing.id,
                    prim_start,
                    prim_end: out.len(),
                    point_start,
                    point_end: points.len(),
                });
            }
        }
    }

    pub(super) fn position_run_progress(&self, drawing: &Drawing) -> Option<PositionRunProgress> {
        let target = match drawing.price_scale {
            crate::DrawingPriceScale::Right => crate::PriceScaleTarget::Right,
            crate::DrawingPriceScale::Left => crate::PriceScaleTarget::Left,
            crate::DrawingPriceScale::Overlay => crate::PriceScaleTarget::Overlay,
        };
        let series = self.series.iter().find(|series| {
            series.visible
                && !series.removed
                && series.pane_index == drawing.pane_index
                && super::series_scale_target(series) == target
        })?;
        // Custom-series frame values expose only their current value, not historical OHLC
        // extrema. Fabricating a "run" endpoint from that current value would violate the
        // position contract, so only canonical plot-backed series participate here.
        if series.kind == crate::SeriesKind::Custom {
            return None;
        }
        let plot = self.data.plot(series.id);
        let entry = drawing.points.first()?;
        let extent = drawing.points.get(1)?;
        let target_price = extent.price;
        let stop_price = drawing.points.get(2)?.price;
        if !entry.logical.is_finite()
            || !entry.price.is_finite()
            || !extent.logical.is_finite()
            || !target_price.is_finite()
            || !stop_price.is_finite()
            || extent.logical < entry.logical
        {
            return None;
        }
        let first_index = entry.logical.ceil();
        let last_index = extent.logical.floor();
        if first_index < i64::MIN as f64
            || first_index > i64::MAX as f64
            || last_index < i64::MIN as f64
            || last_index > i64::MAX as f64
            || first_index > last_index
        {
            return None;
        }
        let first_row = plot.first_non_whitespace_row(first_index as i64)?;
        let last_row = plot.last_non_whitespace_row(last_index as i64)?;
        if first_row > last_row {
            return None;
        }

        let first_high = plot.value_at(first_row, PlotValueIndex::High);
        let first_low = plot.value_at(first_row, PlotValueIndex::Low);
        if !first_high.is_finite() || !first_low.is_finite() {
            return None;
        }

        // Before fill, entry is approached from whichever side contains the first post-placement
        // candle. A candle already spanning entry fills immediately. Otherwise a one-sided extrema
        // predicate (High >= entry from below, Low <= entry from above) lets the LOD hierarchy find
        // the first touch/cross without scanning every historical candle. A gap across entry is a
        // deterministic OHLC "cross" and is anchored visually at the exact entry level.
        let starts_below = first_high < entry.price;
        let starts_above = first_low > entry.price;
        let fill_row = if !starts_below && !starts_above {
            first_row
        } else {
            let range_crosses_entry = |start: usize, end: usize| {
                let mut crossed = false;
                let mut inspect = |row: usize| {
                    if crossed || plot.is_whitespace_row(row) {
                        return;
                    }
                    let value_index = if starts_below {
                        PlotValueIndex::High
                    } else {
                        PlotValueIndex::Low
                    };
                    let value = plot.value_at(row, value_index);
                    crossed |= value.is_finite()
                        && if starts_below {
                            value >= entry.price
                        } else {
                            value <= entry.price
                        };
                };
                if let Some(lod) = plot.lod() {
                    let (rows, _) = lod.rows_on_range(start..end, usize::MAX);
                    for row in rows.iter() {
                        inspect(row);
                    }
                } else {
                    for row in start..end {
                        inspect(row);
                    }
                }
                crossed
            };
            if !range_crosses_entry(first_row, last_row + 1) {
                return None;
            }
            let mut lo = first_row;
            let mut hi = last_row;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if range_crosses_entry(first_row, mid + 1) {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            lo
        };

        let range_hits = |start: usize, end: usize| {
            let mut target_hit = false;
            let mut stop_hit = false;
            let mut inspect = |row: usize| {
                if plot.is_whitespace_row(row) {
                    return;
                }
                let high = plot.value_at(row, PlotValueIndex::High);
                let low = plot.value_at(row, PlotValueIndex::Low);
                match drawing.kind {
                    DrawingKind::LongPosition => {
                        target_hit |= high.is_finite() && high >= target_price;
                        stop_hit |= low.is_finite() && low <= stop_price;
                    }
                    DrawingKind::ShortPosition => {
                        target_hit |= low.is_finite() && low <= target_price;
                        stop_hit |= high.is_finite() && high >= stop_price;
                    }
                    _ => {}
                }
            };
            if let Some(lod) = plot.lod() {
                let (rows, _) = lod.rows_on_range(start..end, usize::MAX);
                for row in rows.iter() {
                    inspect(row);
                }
            } else {
                for row in start..end {
                    inspect(row);
                }
            }
            (target_hit, stop_hit)
        };

        let (any_target, any_stop) = range_hits(fill_row, last_row + 1);
        let (terminal_row, side, closed) = if any_target || any_stop {
            // Prefix boundary-hit is monotonic, so binary search finds the first candle touching
            // either target or stop without rescanning a long-lived position on every frame.
            let mut lo = fill_row;
            let mut hi = last_row;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let (target_hit, stop_hit) = range_hits(fill_row, mid + 1);
                if target_hit || stop_hit {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            let (target_hit, stop_hit) = range_hits(lo, lo + 1);
            // OHLC cannot tell intrabar order when both boundaries are touched by one candle.
            // Resolve that ambiguity conservatively as stop-first.
            let side = if stop_hit {
                PositionRunSide::Risk
            } else if target_hit {
                PositionRunSide::Reward
            } else {
                return None;
            };
            (lo, side, true)
        } else {
            let current = plot.value_at(last_row, PlotValueIndex::Close);
            if !current.is_finite() {
                return None;
            }
            let current = current.clamp(target_price.min(stop_price), target_price.max(stop_price));
            let side = match drawing.kind {
                DrawingKind::LongPosition => {
                    if current >= entry.price {
                        PositionRunSide::Reward
                    } else {
                        PositionRunSide::Risk
                    }
                }
                DrawingKind::ShortPosition => {
                    if current <= entry.price {
                        PositionRunSide::Reward
                    } else {
                        PositionRunSide::Risk
                    }
                }
                _ => return None,
            };
            (last_row, side, false)
        };

        let logical = plot.index_at(terminal_row)?;
        let price = if closed {
            match side {
                PositionRunSide::Reward => target_price,
                PositionRunSide::Risk => stop_price,
            }
        } else {
            plot.value_at(terminal_row, PlotValueIndex::Close)
                .clamp(target_price.min(stop_price), target_price.max(stop_price))
        };
        let start_logical = plot.index_at(fill_row)?;
        price.is_finite().then_some(PositionRunProgress {
            start: crate::drawings::DrawingPoint {
                logical: start_logical as f64,
                price: entry.price,
            },
            point: crate::drawings::DrawingPoint {
                logical: logical as f64,
                price,
            },
            side,
            closed,
        })
    }

    /// Position information labels paint in chrome after the dynamic run overlay. This keeps the
    /// dashed run line visually behind the label chips instead of striking through their text.
    pub(super) fn build_position_labels_frame(
        &self,
        pane_index: usize,
        out: &mut Vec<Prim>,
        hpr: f64,
        vpr: f64,
    ) {
        // Statistic labels belong to the active position only; idle positions show their zones.
        for drawing in self.drawings.iter().filter(|drawing| {
            self.is_drawing_selected(drawing.id)
                && drawing.pane_index == pane_index
                && drawing.visible
                && drawing.interval_visibility.allows(self.drawing_interval)
                && matches!(
                    drawing.kind,
                    DrawingKind::LongPosition | DrawingKind::ShortPosition
                )
                && drawing.points.len() == 3
        }) {
            let Some(px) = self.drawing_px(drawing) else {
                continue;
            };
            let px = px
                .into_iter()
                .map(|(x, y)| (x * hpr, y * vpr))
                .collect::<Vec<_>>();
            let Some(geometry) = resolve_drawing_geometry(
                drawing.kind,
                &px,
                self.pane_w * hpr,
                self.panes[pane_index].top * vpr,
                self.panes[pane_index].height * vpr,
                DrawingGeometryOptions {
                    line_width: drawing.width,
                    device_scale: vpr,
                    extend_left: drawing.extend_left,
                    icon_size: drawing.icon_size,
                    extend_right: drawing.extend_right,
                },
            ) else {
                continue;
            };
            let DrawingBodyGeometry::Position(position) = geometry.body else {
                continue;
            };
            self.build_position_labels(drawing, position, vpr, out);
        }
    }

    fn push_stat_label_block(
        &self,
        out: &mut Vec<Prim>,
        center: (f64, f64),
        lines: &[String],
        (background, ink): (Color, Option<Color>),
        vpr: f64,
    ) {
        let (x, y) = center;
        if lines.is_empty() {
            return;
        }
        let layout = &self.options.get().layout;
        let size = stat_label_size(layout.font_size) * vpr;
        let line_height = size * 1.25;
        let pad_x = 6.0 * vpr;
        let width = lines
            .iter()
            .map(|line| self.measure_text_run(line, size, &layout.font_family, 400, false))
            .fold(0.0_f64, f64::max)
            + 2.0 * pad_x;
        let height = stat_label_height(layout.font_size, lines.len()) * vpr;
        let rect = IRect {
            x: (x - width / 2.0).round() as i32,
            y: (y - height / 2.0).round() as i32,
            w: width.round().max(1.0) as i32,
            h: height.round().max(1.0) as i32,
        };
        // The solid semantic fill under contrast text already separates the label from the
        // chart, so it carries no outline.
        out.push(Prim::RoundRect {
            x: rect.x as f32,
            y: rect.y as f32,
            w: rect.w as f32,
            h: rect.h as f32,
            radii: [(3.0 * vpr).round().max(1.0) as f32; 4],
            fill: background.solid(),
            border_width: 0.0,
            border_color: background.solid(),
        });
        let text_color = ink.unwrap_or_else(|| background.contrast_text());
        let first_y = y - ((lines.len() as f64 - 1.0) * line_height) / 2.0;
        for (index, text) in lines.iter().enumerate() {
            out.push(Prim::Text {
                x: x as f32,
                y: (first_y + index as f64 * line_height) as f32,
                text: text.clone(),
                color: text_color,
                size: size as f32,
                family: layout.font_family.clone(),
                align: TextAlign::Center,
                weight: 400,
                italic: false,
            });
        }
    }

    /// The anchor-handle fill for the current theme (white on light backgrounds, black on dark —
    /// the series selection anchors' luminance rule, series_geometry.rs).
    fn anchor_fill(&self) -> Color {
        match self.surface_theme() {
            crate::ChartTheme::Light => Color::rgb(0xff, 0xff, 0xff),
            crate::ChartTheme::Dark => Color::rgb(0, 0, 0),
        }
    }

    /// The token theme matching the painted chart background, so in-chart chrome follows the
    /// surface hosts actually show even when they restyle `layout` without `set_theme`. Host DOM
    /// chrome drawn over the chart (tooltips) selects its tokens from the same theme.
    pub fn surface_theme(&self) -> crate::ChartTheme {
        let fallback = aeris_charts_core::style::DEFAULT_SURFACE_RGB;
        let background = Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2));
        if background.luminance() > 160.0 {
            crate::ChartTheme::Light
        } else {
            crate::ChartTheme::Dark
        }
    }
}

/// `ChartEngine::price_label_layout`: bubble `[left, top, width, height]`, label text and
/// glyph size, the text inset, and the tail height below the bubble.
pub(crate) struct PriceLabelLayout {
    pub(crate) rect: [f64; 4],
    pub(crate) text: String,
    pub(crate) size: f64,
    pub(crate) padding: f64,
    pub(crate) tail: f64,
}

/// Measured quantities between a measuring tool's anchors (`ChartEngine::measure_stats`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct MeasureStats {
    price_change: f64,
    percent: Option<f64>,
    ticks: Option<f64>,
    bars: i64,
    seconds: Option<i64>,
}

/// Arrowhead wing reach along and across the shaft, in CSS px.
const MEASURE_ARROW_CSS: f64 = 5.0;

/// Open chevron at `tip` pointing along the unit `direction`, stroked at the shaft's width.
fn push_measure_arrowhead(
    tip: (f64, f64),
    direction: (f64, f64),
    width: i32,
    vpr: f64,
    color: Color,
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
) {
    let reach = MEASURE_ARROW_CSS * vpr;
    let base = (tip.0 - direction.0 * reach, tip.1 - direction.1 * reach);
    let side = (-direction.1 * reach, direction.0 * reach);
    let first_point = points.len() as u32;
    points.extend([
        [(base.0 + side.0) as f32, (base.1 + side.1) as f32],
        [tip.0 as f32, tip.1 as f32],
        [(base.0 - side.0) as f32, (base.1 - side.1) as f32],
    ]);
    out.push(Prim::Polyline {
        first_point,
        point_count: 3,
        width: width as f32,
        style: LineStyle::Solid,
        line_type: LineType::Simple,
        color,
    });
}

/// Trend Angle reference/arc radius and the gap before its angle label, in CSS px.
const TREND_ANGLE_RADIUS_CSS: f64 = 60.0;
const TREND_ANGLE_LABEL_GAP_CSS: f64 = 10.0;
/// Info Line card metrics in CSS px, following the package's `.aeris_charts-tooltip` panel
/// (12/16 px type, 10 px side and 6/7 px vertical padding, 12 px column gap). Rows get a little
/// more spacing than the tooltip's 2 px so the 14 px icons do not touch.
const INFO_LINE_FONT_CSS: f64 = 12.0;
const INFO_LINE_LINE_HEIGHT_CSS: f64 = 16.0;
const INFO_LINE_ROW_GAP_CSS: f64 = 4.0;
const INFO_LINE_PAD_X_CSS: f64 = 10.0;
const INFO_LINE_PAD_TOP_CSS: f64 = 6.0;
const INFO_LINE_PAD_BOTTOM_CSS: f64 = 7.0;
const INFO_LINE_ICON_CSS: f64 = 16.0;
const INFO_LINE_ICON_GAP_CSS: f64 = 12.0;

/// On-chart annotation colors from the design tokens of the painted theme (the package CSS's
/// `--surface`, `--border`, `--text-default`, `--text-secondary`, and accent surface) for the
/// note, signpost and callout boxes. Floating cards such as the Info Line statistics and the
/// tooltips use [`super::PopoverTokens`] instead.
struct ChromeTokens {
    surface: Color,
    accent: Color,
    border: Color,
    foreground: Color,
    muted: Color,
}

impl ChromeTokens {
    fn for_theme(theme: crate::ChartTheme) -> Self {
        use aeris_charts_core::style::*;
        let (surface, accent, border, foreground, muted) = match theme {
            crate::ChartTheme::Light => (
                LIGHT_SURFACE_RGB,
                LIGHT_ACCENT_RGB,
                LIGHT_BORDER_RGB,
                LIGHT_FOREGROUND_RGB,
                LIGHT_MUTED_FOREGROUND_RGB,
            ),
            crate::ChartTheme::Dark => (
                DARK_SURFACE_RGB,
                DARK_ACCENT_RGB,
                DARK_BORDER_RGB,
                DARK_FOREGROUND_RGB,
                DARK_MUTED_FOREGROUND_RGB,
            ),
        };
        let rgb = |(r, g, b): (u8, u8, u8)| Color::rgb(r, g, b);
        Self {
            surface: rgb(surface),
            accent: rgb(accent),
            border: rgb(border),
            foreground: rgb(foreground),
            muted: rgb(muted),
        }
    }
}

/// A segment angle in degrees, at most two decimals and without trailing zeros (`-43.73°`).
fn line_angle_text(degrees: f64) -> String {
    format!("{}°", position_stat_number(degrees, 2))
}

/// The glyph leading each Info Line card row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InfoLineIcon {
    /// Price range: a double arrow between two horizontal bars.
    Price,
    /// Time range: a double arrow between two candles.
    Time,
    /// Angle: a ray rising from a baseline with its arc.
    Angle,
}

/// Strokes `kind`'s glyph centered at `center`, drawn on a 16 CSS px grid.
fn push_info_line_icon(
    kind: InfoLineIcon,
    center: (f64, f64),
    color: Color,
    vpr: f64,
    out: &mut Vec<Prim>,
    points: &mut Vec<[f32; 2]>,
) {
    let (cx, cy) = center;
    let at = |x: f64, y: f64| [(cx + x * vpr) as f32, (cy + y * vpr) as f32];
    let mut stroke = |path: &[[f32; 2]]| {
        let first_point = points.len() as u32;
        points.extend_from_slice(path);
        out.push(Prim::Polyline {
            first_point,
            point_count: path.len() as u32,
            width: vpr.round().max(1.0) as f32,
            style: LineStyle::Solid,
            line_type: LineType::Simple,
            color,
        });
    };
    match kind {
        InfoLineIcon::Price => {
            stroke(&[at(-5.0, -7.0), at(5.0, -7.0)]);
            stroke(&[at(-5.0, 7.0), at(5.0, 7.0)]);
            stroke(&[at(0.0, -5.0), at(0.0, 5.0)]);
            stroke(&[at(-3.0, -2.0), at(0.0, -5.0), at(3.0, -2.0)]);
            stroke(&[at(-3.0, 2.0), at(0.0, 5.0), at(3.0, 2.0)]);
        }
        InfoLineIcon::Time => {
            for side in [-1.0, 1.0] {
                let x = 7.0 * side;
                stroke(&[at(x, -7.0), at(x, -3.0)]);
                stroke(&[at(x, 3.0), at(x, 7.0)]);
                stroke(&[
                    at(x - 1.5, -3.0),
                    at(x + 1.5, -3.0),
                    at(x + 1.5, 3.0),
                    at(x - 1.5, 3.0),
                    at(x - 1.5, -3.0),
                ]);
                stroke(&[
                    at(-1.5 * side, -2.0),
                    at(-3.5 * side, 0.0),
                    at(-1.5 * side, 2.0),
                ]);
            }
            stroke(&[at(-3.5, 0.0), at(3.5, 0.0)]);
        }
        InfoLineIcon::Angle => {
            stroke(&[at(-7.0, 6.0), at(7.0, 6.0)]);
            stroke(&[at(-7.0, 6.0), at(2.0, -7.0)]);
            let ray = (-13.0_f64).atan2(9.0);
            let arc: Vec<[f32; 2]> = (0..=8)
                .map(|step| {
                    let angle = ray * f64::from(step) / 8.0;
                    at(-7.0 + 8.0 * angle.cos(), 6.0 + 8.0 * angle.sin())
                })
                .collect();
            stroke(&arc);
        }
    }
}

/// A signed statistic with the typographic minus used by price formatting; zero is unsigned.
fn signed_stat(value: f64, precision: usize) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    let text = format!("{:.precision$}", value.abs());
    if value < 0.0 && text.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!(
            "{}{text}",
            aeris_charts_core::format::price_formatter::MINUS_SIGN
        )
    } else {
        text
    }
}

/// Elapsed time as its two most significant non-zero units (`1d 4h`, `45m`, `30s`).
fn format_measure_duration(seconds: i64) -> String {
    let magnitude = seconds.unsigned_abs();
    let parts = [
        (magnitude / 86_400, "d"),
        (magnitude % 86_400 / 3_600, "h"),
        (magnitude % 3_600 / 60, "m"),
        (magnitude % 60, "s"),
    ];
    let mut text = String::new();
    if seconds < 0 {
        text.push(aeris_charts_core::format::price_formatter::MINUS_SIGN);
    }
    let mut written = 0;
    for (value, unit) in parts {
        if value == 0 || written == 2 {
            continue;
        }
        if written == 1 {
            text.push(' ');
        }
        text.push_str(&format!("{value}{unit}"));
        written += 1;
    }
    if written == 0 {
        return "0s".to_string();
    }
    text
}

#[cfg(test)]
mod measure_format_tests {
    use super::{format_measure_duration, signed_stat};

    #[test]
    fn durations_show_the_two_most_significant_units_with_a_typographic_sign() {
        assert_eq!(format_measure_duration(0), "0s");
        assert_eq!(format_measure_duration(45), "45s");
        assert_eq!(format_measure_duration(2_700), "45m");
        assert_eq!(format_measure_duration(9_000), "2h 30m");
        assert_eq!(format_measure_duration(90_061), "1d 1h");
        assert_eq!(format_measure_duration(86_400 * 3 + 120), "3d 2m");
        assert_eq!(format_measure_duration(-3_600), "\u{2212}1h");
    }

    #[test]
    fn signed_statistics_never_show_negative_zero() {
        assert_eq!(signed_stat(-12.5, 2), "\u{2212}12.50");
        assert_eq!(signed_stat(12.5, 2), "12.50");
        assert_eq!(signed_stat(-0.001, 2), "0.00");
        assert_eq!(signed_stat(-6.0, 0), "\u{2212}6");
        assert_eq!(signed_stat(f64::NAN, 0), "—");
    }
}

/// Statistic label glyph size in CSS px for the chart's layout font size.
pub(crate) fn stat_label_size(layout_font_size: f64) -> f64 {
    layout_font_size.max(11.0)
}

/// Statistic label container height in CSS px: `lines` line boxes plus 3 px padding per side.
pub(crate) fn stat_label_height(layout_font_size: f64, lines: usize) -> f64 {
    lines as f64 * stat_label_size(layout_font_size) * 1.25 + 6.0
}

/// A pattern's vertex label ink: its `text_color`, else its stroke.
fn vertex_label_color(drawing: &Drawing, stroke: Color) -> Color {
    drawing
        .text_color
        .as_deref()
        .and_then(Color::parse_css)
        .unwrap_or(stroke)
}

/// The area fill a drawing paints: its `fill_color`, else `stroke` at the kind's default alpha.
fn area_fill(drawing: &Drawing, stroke: Color) -> Color {
    drawing
        .fill_color
        .as_deref()
        .and_then(Color::parse_css)
        .unwrap_or(Color::rgba(
            stroke.r(),
            stroke.g(),
            stroke.b(),
            drawing.kind.default_fill_alpha(),
        ))
}

/// A position's (profit, stop) colors: the drawing's own, else the platform gain/loss pair.
fn position_colors(drawing: &Drawing, (gain, loss): (Color, Color)) -> (Color, Color) {
    let resolve = |custom: Option<&str>, fallback: Color| {
        custom.and_then(Color::parse_css).unwrap_or(fallback)
    };
    (
        resolve(drawing.position_profit_color.as_deref(), gain),
        resolve(drawing.position_loss_color.as_deref(), loss),
    )
}

fn push_position_zone(out: &mut Vec<Prim>, zone: PositionZone, color: Color) {
    push_position_zone_with_alpha(out, zone, color, POSITION_ZONE_ALPHA);
}

fn push_position_zone_with_alpha(out: &mut Vec<Prim>, zone: PositionZone, color: Color, alpha: u8) {
    let left = zone.left.round() as i32;
    let right = zone.right.round() as i32;
    let top = zone.y0.min(zone.y1).round() as i32;
    let bottom = zone.y0.max(zone.y1).round() as i32;
    let width = (right - left).abs() + 1;
    let height = (bottom - top).abs() + 1;
    let rect = IRect {
        x: left,
        y: top,
        w: width,
        h: height,
    };
    out.push(Prim::Rect {
        rect,
        color: Color::rgba(color.r(), color.g(), color.b(), alpha),
    });
}

/// Gap between a point label's text box and the point it names, in CSS px.
const POINT_LABEL_GAP_CSS: f64 = 4.0;
/// Vertices carry selection handles, so their labels clear the handle as well.
const VERTEX_LABEL_GAP_CSS: f64 = ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH + POINT_LABEL_GAP_CSS;

/// The center of a `width` x `height` text box naming `point`, placed so it never sits on the
/// strokes that leave the point toward `toward` (their far ends or any point along them): in the
/// widest angle between those strokes, along its bisector, with the whole box `gap` clear of the
/// point. With no strokes, or between two equally wide openings, it leans toward `prefer` (a unit
/// direction, e.g. up or radially outward).
fn clear_label_center(
    point: (f64, f64),
    toward: &[(f64, f64)],
    width: f64,
    height: f64,
    gap: f64,
    prefer: (f64, f64),
) -> (f64, f64) {
    let mut angles: Vec<f64> = toward
        .iter()
        .filter(|&&(x, y)| (x - point.0).hypot(y - point.1) > f64::EPSILON)
        .map(|&(x, y)| (y - point.1).atan2(x - point.0))
        .collect();
    angles.sort_by(f64::total_cmp);
    let prefer_angle = prefer.1.atan2(prefer.0);
    let direction = if angles.is_empty() {
        prefer_angle
    } else {
        // Each opening runs from one stroke to the next counterclockwise; the widest wins, and
        // near-ties go to the opening whose bisector is closest to the preference.
        let tau = std::f64::consts::TAU;
        let mut best = (f64::MIN, f64::MIN, prefer_angle);
        for (index, &start) in angles.iter().enumerate() {
            let end = angles.get(index + 1).copied().unwrap_or(angles[0] + tau);
            let span = end - start;
            let bisector = start + span / 2.0;
            let lean = (bisector - prefer_angle).cos();
            let wider = span > best.0 + 1e-6;
            let tie = (span - best.0).abs() <= 1e-6 && lean > best.1;
            if wider || tie {
                best = (span, lean, bisector);
            }
        }
        best.2
    };
    let (dx, dy) = (direction.cos(), direction.sin());
    // The box's support distance along the direction keeps its nearest corner `gap` away; in a
    // narrow opening the box steps further out until it clears every stroke through the point.
    let mut reach = gap + dx.abs() * width / 2.0 + dy.abs() * height / 2.0;
    let clear = |reach: f64| {
        let (cx, cy) = (point.0 + dx * reach, point.1 + dy * reach);
        let bounds = [
            cx - width / 2.0 - gap / 2.0,
            cy - height / 2.0 - gap / 2.0,
            cx + width / 2.0 + gap / 2.0,
            cy + height / 2.0 + gap / 2.0,
        ];
        !toward
            .iter()
            .any(|&end| segment_crosses_box(point, end, bounds))
    };
    for _ in 0..32 {
        if clear(reach) {
            break;
        }
        reach += gap;
    }
    (point.0 + dx * reach, point.1 + dy * reach)
}

/// Whether the segment `a`-`b` enters the box `[left, top, right, bottom]`.
fn segment_crosses_box(a: (f64, f64), b: (f64, f64), [left, top, right, bottom]: [f64; 4]) -> bool {
    // Liang-Barsky clipping: the segment's parameter interval inside the box is non-empty.
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut enter, mut leave) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-dx, a.0 - left),
        (dx, right - a.0),
        (-dy, a.1 - top),
        (dy, bottom - a.1),
    ] {
        if p.abs() <= f64::EPSILON {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                enter = enter.max(t);
            } else {
                leave = leave.min(t);
            }
        }
    }
    enter <= leave
}

/// Every drawing handle, round or square: one pixel-aligned shape that owns both its fill and
/// its inside border, so the ring is the same whole-device-pixel width on every side, tool, and
/// backend (separately rounded fill and border shapes drifted apart at fractional positions).
fn push_handle(center: (f64, f64), square: bool, vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    let side = (2.0 * (ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr)
        .round()
        .max(3.0);
    let radius = if square {
        (ANCHOR_SQUARE_RADIUS * vpr).round().max(1.0)
    } else {
        side / 2.0
    };
    out.push(Prim::RoundRect {
        x: (center.0 - side / 2.0).round() as f32,
        y: (center.1 - side / 2.0).round() as f32,
        w: side as f32,
        h: side as f32,
        radii: [radius as f32; 4],
        fill,
        border_width: (ANCHOR_BORDER_WIDTH * vpr).round().max(1.0) as f32,
        border_color: ANCHOR_BORDER,
    });
}

/// One round handle per anchor.
fn build_anchor_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    for &center in px {
        push_handle(center, false, vpr, fill, out);
    }
}

/// Long/Short Position selection controls: target, entry/pivot (round), horizontal extent, and
/// stop. They are intentionally not generic drawing-point anchors.
fn build_position_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    let &[entry, target, stop] = px else {
        return;
    };
    for (center, round) in [
        ((entry.0, target.1), false),
        (entry, true),
        ((target.0, entry.1), false),
        ((entry.0, stop.1), false),
    ] {
        push_handle(center, !round, vpr, fill, out);
    }
}

/// The bounds handles of `mode` (`DrawingHandleMode::bounds_slots`): round on the corners,
/// square on the edge midpoints (each midpoint resizes one edge independently).
fn build_rectangle_handles(
    px: &[(f64, f64)],
    mode: DrawingHandleMode,
    vpr: f64,
    fill: Color,
    out: &mut Vec<Prim>,
) {
    let anchors = ChartEngine::rectangle_anchors(px);
    for &index in mode.bounds_slots().unwrap_or_default() {
        push_handle(anchors[index], index % 2 == 1, vpr, fill, out);
    }
}

fn position_stat_number(value: f64, precision: usize) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    let text = format!("{value:.precision$}");
    let text = if precision > 0 {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}
