//! Drawing-tool frame emission (model in drawings.rs): each pane's committed drawings in
//! z-order, the selected drawing's anchor handles, and the interactive-creation preview.
//!
//! Coordinates follow the frame build's conventions (frame/mod.rs): x is pane-local media px
//! scaled by the exact horizontal ratio (the trailing `translate_prims_x` shifts everything
//! when a left axis reserves space), y is chart-top-relative media px scaled by the vertical
//! ratio — the same space the price-line/series geometry uses, so a drawing's prims land
//! exactly on its converted anchors. Standalone drawing text uses `Prim::Text`; trend labels
//! use the backend-neutral `Prim::RotatedText` contract so every executor receives the same
//! aligned anchor and segment-normalized angle.

use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, LineStyle, LineType, Prim, TextAlign};
use std::fmt::Write;

use super::{POSITION_ENTRY, PRIMARY};
use crate::ChartEngine;
use crate::drawings::{
    Drawing, DrawingBodyGeometry, DrawingGeometryOptions, DrawingHandleMode, DrawingId,
    DrawingKind, DrawingPoint, DrawingTextHAlign, MEASURE_LABEL_GAP, MeasureAxes, MeasureGeometry,
    PositionGeometry, PositionZone, TEXT_CHROME_PAD, TEXT_PAD, TREND_TEXT_PLACEHOLDER,
    resolve_drawing_geometry,
};
use aeris_charts_core::model::plot_list::PlotValueIndex;

/// industry-standard drawing anchor handle: a theme-derived disc with the primary-token border
/// (the crosshair-marks disc idiom — the border is a larger filled disc underneath). Slightly
/// larger than the series selection anchors (2.5/1.5, series_geometry.rs) since these are
/// drag targets.
const ANCHOR_RADIUS: f64 = 4.0;
const ANCHOR_BORDER_WIDTH: f64 = 1.5;
const ANCHOR_BORDER: Color = PRIMARY;
const POSITION_ENTRY_WIDTH_CSS: f64 = 0.5;
const POSITION_ZONE_ALPHA: u8 = 70;
/// Progress must read as the emphasized portion of either semantic side on its own. Keep this
/// above the base-zone alpha instead of relying on a second lower-alpha pass to become visible
/// only through accidental compositing.
const POSITION_PROGRESS_ALPHA: u8 = 96;
/// The hover ring's dimmed variant of the focus border (the public reference shows the same border at
/// roughly half strength until the drawing is actually selected).
const HOVER_BORDER: Color = Color(PRIMARY.0 & 0xFFFF_FF00 | 0x73);
const TREND_TEXT_PLACEHOLDER_ALPHA: u8 = 0x99;

fn point_on_segment(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
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

fn push_drawing_cap(
    cap: crate::DrawingLineCap,
    endpoint: (f64, f64),
    toward: (f64, f64),
    width: f64,
    color: Color,
    out: &mut Vec<Prim>,
) {
    if cap == crate::DrawingLineCap::None {
        return;
    }
    let dx = toward.0 - endpoint.0;
    let dy = toward.1 - endpoint.1;
    let distance = dx.hypot(dy);
    if distance <= f64::EPSILON {
        return;
    }
    let ux = dx / distance;
    let uy = dy / distance;
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
        crate::DrawingLineCap::Arrow => {
            let base_x = endpoint.0 + ux * radius * 2.0;
            let base_y = endpoint.1 + uy * radius * 2.0;
            let side_x = -uy * radius;
            let side_y = ux * radius;
            out.push(Prim::Triangle {
                a: [endpoint.0 as f32, endpoint.1 as f32],
                b: [(base_x + side_x) as f32, (base_y + side_y) as f32],
                c: [(base_x - side_x) as f32, (base_y - side_y) as f32],
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
        (drawing.kind == DrawingKind::TrendLine
            && self.hovered_text == Some(drawing.id)
            && self.editing_drawing != Some(drawing.id))
        .then_some((TREND_TEXT_PLACEHOLDER, true))
    }

    /// Width source for the middle-line cutout. Hover reserves the full prompt; once editing
    /// begins, an empty value uses the editor's one-em caret opening and measured text expands it.
    fn drawing_frame_gap_text<'a>(&self, drawing: &'a Drawing) -> Option<&'a str> {
        if drawing.kind == DrawingKind::TrendLine && self.editing_drawing == Some(drawing.id) {
            return Some(drawing.display_text());
        }
        if !drawing.text.is_empty() {
            return Some(drawing.display_text());
        }
        (drawing.kind == DrawingKind::TrendLine && self.hovered_text == Some(drawing.id))
            .then_some(TREND_TEXT_PLACEHOLDER)
    }

    fn measure_drawing_frame_text(&self, drawing: &Drawing, text: &str, size: f64) -> f64 {
        let layout = &self.options.get().layout;
        self.measure_text_run(
            text,
            size,
            &layout.font_family,
            drawing.text_weight.unwrap_or(400),
            drawing.text_italic,
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
            self.build_drawing_text_caret(drawing, &px, pane_w_px, vpr, out, points);
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
                self.build_drawing_text_caret(drawing, &px, pane_w_px, vpr, out, points);
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
                self.build_drawing_text_caret(drawing, &px, pane_w_px, vpr, out, points);
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
                    if preview_drawing.kind.spec().handles == DrawingHandleMode::RectangleBounds
                        && let Some(fill) = preview_drawing.preview_fill_color.clone()
                    {
                        preview_drawing.fill_color = Some(fill);
                    }
                    self.build_drawing_prims(&preview_drawing, &px, pane_w_px, vpr, out, points);
                    if pending.drawing.kind.spec().handles == DrawingHandleMode::RectangleBounds {
                        // the public reference shows all eight anchors while the rectangle is being
                        // drawn (committed corner + live preview corner), not only after
                        // the commit.
                        build_rectangle_handles(&px, vpr, self.anchor_fill(), out);
                    } else {
                        let committed = pending.drawing.points.len();
                        build_anchor_handles(&px[..committed], vpr, self.anchor_fill(), out);
                    }
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

    /// The selected/hovered drawing's converted bitmap-px anchor points, or `None` when the
    /// id is stale, on another pane, or off-screen.
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
                .take(drawing.points.len())
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
    ) {
        let Some(id) = self.selected_drawing else {
            return;
        };
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        if drawing.kind.spec().requests_text_editor {
            let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
                return;
            };
            self.push_text_chrome(drawing, &px, pane_w_px, vpr, ANCHOR_BORDER, out);
            return;
        }
        let Some(px) = self.overlay_drawing_px(pane_index, id, hpr, vpr) else {
            return;
        };
        match drawing.kind.spec().handles {
            DrawingHandleMode::None => {}
            DrawingHandleMode::RectangleBounds if px.len() == 2 => {
                build_rectangle_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Position if px.len() == 3 => {
                build_position_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Endpoints if !px.is_empty() => {
                build_anchor_handles(&[px[0], px[px.len() - 1]], vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::Anchors | DrawingHandleMode::Endpoints => {
                build_anchor_handles(&px, vpr, self.anchor_fill(), out);
            }
            DrawingHandleMode::RectangleBounds | DrawingHandleMode::Position => {}
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
        if self.selected_drawing == Some(id) {
            return;
        }
        let Some(drawing) = self.drawing(id) else {
            return;
        };
        if drawing.kind != DrawingKind::Text && !drawing.kind.is_text_annotation() {
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
        if matches!(
            drawing.kind,
            DrawingKind::FixedRangeVolumeProfile
                | DrawingKind::AnchoredVolumeProfile
                | DrawingKind::AnchoredVwap
        ) && drawing.profile.is_some()
        {
            self.build_profile_drawing_prims(drawing, px, pane_w_px, vpr, out, points);
            return;
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
        match geometry.body {
            DrawingBodyGeometry::Segment { a, b } => {
                let label_gap = self
                    .drawing_frame_gap_text(drawing)
                    .filter(|_| {
                        drawing.kind == DrawingKind::TrendLine
                            && drawing.text_v_align == crate::drawings::DrawingTextVAlign::Middle
                    })
                    .and_then(|text| {
                        let (size, x, y, align, angle) =
                            self.text_run_geometry(drawing, px, pane_w_px, vpr);
                        let mut width = self.measure_drawing_frame_text(drawing, text, size);
                        if self.editing_drawing == Some(drawing.id) {
                            // Match the host editor's one-em empty/minimum width. This leaves a
                            // compact caret slot and then grows from actual shaped advance.
                            width = width.max(size);
                        }
                        let gap = TEXT_PAD * vpr;
                        let (local_start, local_end) = match align {
                            DrawingTextHAlign::Left => (-gap, width + gap),
                            DrawingTextHAlign::Center => (-width / 2.0 - gap, width / 2.0 + gap),
                            DrawingTextHAlign::Right => (-width - gap, gap),
                        };
                        let length_sq = (b.0 - a.0).powi(2) + (b.1 - a.1).powi(2);
                        if length_sq <= f64::EPSILON {
                            return None;
                        }
                        let project = |distance: f64| {
                            let px = x + angle.cos() * distance;
                            let py = y + angle.sin() * distance;
                            ((px - a.0) * (b.0 - a.0) + (py - a.1) * (b.1 - a.1)) / length_sq
                        };
                        let first = project(local_start);
                        let second = project(local_end);
                        let start = first.min(second).clamp(0.0, 1.0);
                        let end = first.max(second).clamp(0.0, 1.0);
                        (start < end).then_some((start, end))
                    });
                if let Some((gap_start, gap_end)) = label_gap {
                    push_segment(
                        a,
                        point_on_segment(a, b, gap_start),
                        drawing,
                        color,
                        vpr,
                        out,
                        points,
                    );
                    push_segment(
                        point_on_segment(a, b, gap_end),
                        b,
                        drawing,
                        color,
                        vpr,
                        out,
                        points,
                    );
                } else {
                    push_segment(a, b, drawing, color, vpr, out, points);
                }
                push_drawing_cap(drawing.stroke_start, a, b, drawing.width * vpr, color, out);
                push_drawing_cap(drawing.stroke_end, b, a, drawing.width * vpr, color, out);
            }
            DrawingBodyGeometry::Horizontal { y, x0, x1 } => {
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
            DrawingBodyGeometry::Vertical { x, y0, y1 } => {
                out.push(Prim::VLine {
                    x: x.round() as i32,
                    y0: y0.round().max(0.0) as i32,
                    y1: y1.round().max(0.0) as i32,
                    width: crisp_width,
                    style: drawing.style,
                    color,
                });
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
                    let fill = drawing
                        .fill_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51));
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
                    let fill = drawing
                        .fill_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 35));
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
                    if (y0 - y1).abs() <= f64::EPSILON {
                        out.push(Prim::HLine {
                            y: y0.round() as i32,
                            x0: x0.min(x1).round() as i32,
                            x1: x0.max(x1).round() as i32,
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
                    if level.label_visible {
                        let price = self.drawing_level_price_at(drawing, y1, vpr);
                        if let Some(text) = self.drawing_level_label(drawing, level.value, price) {
                            let label_x = match drawing.level_label_align.as_str() {
                                "left" => x0,
                                "center" => (x0 + x1) / 2.0,
                                _ => x1,
                            };
                            out.push(Prim::Text {
                                x: label_x as f32,
                                y: (y1 - 8.0 * vpr) as f32,
                                text,
                                color: level_color,
                                size: (self.options.get().layout.font_size * vpr) as f32,
                                family: self.options.get().layout.font_family.clone(),
                                align: Self::drawing_level_align(drawing),
                                weight: drawing.text_weight.unwrap_or(400),
                                italic: drawing.text_italic,
                            });
                        }
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
                    out.push(Prim::VLine {
                        x: x.round() as i32,
                        y0: time.pane_top.round().max(0.0) as i32,
                        y1: time.pane_bottom.round().max(0.0) as i32,
                        width: crisp_width,
                        style,
                        color: level_color,
                    });
                    if level.label_visible
                        && let Some(text) = self.drawing_level_label(drawing, level.value, None)
                    {
                        out.push(Prim::Text {
                            x: (x + if drawing.level_label_align == "left" {
                                4.0 * vpr
                            } else if drawing.level_label_align == "right" {
                                -4.0 * vpr
                            } else {
                                0.0
                            }) as f32,
                            y: (time.pane_top + 14.0 * vpr) as f32,
                            text,
                            color: level_color,
                            size: (self.options.get().layout.font_size * vpr) as f32,
                            family: self.options.get().layout.font_family.clone(),
                            align: Self::drawing_level_align(drawing),
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
                let segments = arcs.segments();
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
                        let (x, y) = arcs.point(drawing.level_value(level.value), 0.5);
                        let price = self.drawing_level_price_at(drawing, y, vpr);
                        if let Some(text) = self.drawing_level_label(drawing, level.value, price) {
                            out.push(Prim::Text {
                                x: x as f32,
                                y: (y - 8.0 * vpr) as f32,
                                text,
                                color: level_color,
                                size: (self.options.get().layout.font_size * vpr) as f32,
                                family: self.options.get().layout.font_family.clone(),
                                align: Self::drawing_level_align(drawing),
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
                            out.push(Prim::Text {
                                x: anchor.0 as f32,
                                y: (anchor.1 - 8.0 * vpr) as f32,
                                text,
                                color: level_color,
                                size: (self.options.get().layout.font_size * vpr) as f32,
                                family: self.options.get().layout.font_family.clone(),
                                align: Self::drawing_level_align(drawing),
                                weight: drawing.text_weight.unwrap_or(400),
                                italic: drawing.text_italic,
                            });
                        }
                    }
                }
            }
            DrawingBodyGeometry::Cycles(cycles) => {
                let fill = drawing
                    .fill_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 24));
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
                if let Some((a, b)) = marker.stem() {
                    push_segment(a, b, drawing, color, vpr, out, points);
                }
                let [a, b, c] = marker.triangle();
                out.push(Prim::Triangle {
                    a: [a.0 as f32, a.1 as f32],
                    b: [b.0 as f32, b.1 as f32],
                    c: [c.0 as f32, c.1 as f32],
                    color,
                });
            }
            DrawingBodyGeometry::PriceLabel { x, y } => {
                let label = if drawing.text.is_empty() {
                    self.format_drawing_price(drawing, drawing.points[0].price)
                } else {
                    drawing.text.clone()
                };
                let layout = &self.options.get().layout;
                let size = drawing.resolved_text_size(layout.font_size) * vpr;
                let width = self.measure_text_run(
                    &label,
                    size,
                    &layout.font_family,
                    drawing.text_weight.unwrap_or(400),
                    drawing.text_italic,
                );
                let padding = 4.0 * vpr;
                out.push(Prim::Rect {
                    rect: IRect {
                        x: (x - width - 2.0 * padding).round() as i32,
                        y: (y - size * 0.6 - padding).round() as i32,
                        w: (width + 2.0 * padding).round().max(1.0) as i32,
                        h: (size * 1.2 + 2.0 * padding).round().max(1.0) as i32,
                    },
                    color,
                });
                out.push(Prim::Text {
                    x: (x - padding) as f32,
                    y: y as f32,
                    text: label,
                    color: drawing
                        .text_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgb(255, 255, 255)),
                    size: size as f32,
                    family: layout.font_family.clone(),
                    align: TextAlign::Right,
                    weight: drawing.text_weight.unwrap_or(400),
                    italic: drawing.text_italic,
                });
            }
            DrawingBodyGeometry::IconStamp { center, size } => {
                let rect = [
                    (center.0 - size / 2.0) as f32,
                    (center.1 - size / 2.0) as f32,
                    size as f32,
                    size as f32,
                ];
                if let Some(image) = drawing
                    .icon_name
                    .as_deref()
                    .and_then(|name| self.drawing_icons.get(name))
                {
                    out.push(Prim::Image {
                        image: image.clone(),
                        rect,
                        opacity: 1.0,
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
                    let fill = drawing
                        .fill_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 24));
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
                    let mut prior_arc: Option<f64> = None;
                    for level in drawing.gann_arcs.iter().filter(|level| level.visible) {
                        if level.fill_between
                            && let Some(previous) = prior_arc
                        {
                            let upper_first = points.len() as u32;
                            for step in 0..=32 {
                                let p = grid.arc_point(
                                    previous,
                                    f64::from(step) / 32.0,
                                    drawing.level_reverse,
                                );
                                points.push([p.0 as f32, p.1 as f32]);
                            }
                            let lower_first = points.len() as u32;
                            for step in 0..=32 {
                                let p = grid.arc_point(
                                    level.value,
                                    f64::from(step) / 32.0,
                                    drawing.level_reverse,
                                );
                                points.push([p.0 as f32, p.1 as f32]);
                            }
                            out.push(Prim::BandFill {
                                upper_first,
                                lower_first,
                                point_count: 33,
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
                        if let Some(text) = self.drawing_level_label(drawing, level.value, price) {
                            out.push(Prim::Text {
                                x: x as f32,
                                y: (y - 8.0 * vpr) as f32,
                                text,
                                color: level_color,
                                size: (self.options.get().layout.font_size * vpr) as f32,
                                family: self.options.get().layout.font_family.clone(),
                                align: Self::drawing_level_align(drawing),
                                weight: drawing.text_weight.unwrap_or(400),
                                italic: drawing.text_italic,
                            });
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
                    for level in drawing.gann_arcs.iter().filter(|level| level.visible) {
                        let level_color = Color::parse_css(&level.color).unwrap_or(color);
                        let first_point = points.len() as u32;
                        for step in 0..=32 {
                            let point = grid.arc_point(
                                level.value,
                                f64::from(step) / 32.0,
                                drawing.level_reverse,
                            );
                            points.push([point.0 as f32, point.1 as f32]);
                        }
                        out.push(Prim::Polyline {
                            first_point,
                            point_count: 33,
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
                    let fill = drawing
                        .fill_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51));
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
                    if drawing.fill_enabled {
                        let fill = drawing
                            .fill_color
                            .as_deref()
                            .and_then(Color::parse_css)
                            .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51));
                        let upper_first = points.len() as u32;
                        for step in 0..=32 {
                            let theta = std::f64::consts::PI * step as f64 / 32.0;
                            points.push([
                                (center.0 - rx * theta.cos()) as f32,
                                (center.1 - ry * theta.sin()) as f32,
                            ]);
                        }
                        let lower_first = points.len() as u32;
                        for step in 0..=32 {
                            let theta = std::f64::consts::PI * step as f64 / 32.0;
                            points.push([
                                (center.0 - rx * theta.cos()) as f32,
                                (center.1 + ry * theta.sin()) as f32,
                            ]);
                        }
                        out.push(Prim::BandFill {
                            upper_first,
                            lower_first,
                            point_count: 33,
                            line_type: LineType::Simple,
                            fill,
                        });
                    }
                    let first_point = points.len() as u32;
                    for step in 0..=64 {
                        let theta = std::f64::consts::TAU * step as f64 / 64.0;
                        points.push([
                            (center.0 + rx * theta.cos()) as f32,
                            (center.1 + ry * theta.sin()) as f32,
                        ]);
                    }
                    out.push(Prim::Polyline {
                        first_point,
                        point_count: 65,
                        width: (drawing.width * vpr) as f32,
                        style: drawing.style,
                        line_type: LineType::Simple,
                        color,
                    });
                }
            }
            DrawingBodyGeometry::Circle { center, radius } => {
                let fill = if drawing.fill_enabled {
                    drawing
                        .fill_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51))
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
                let first_point = points.len() as u32;
                for step in 0..=64 {
                    let theta = std::f64::consts::TAU * step as f64 / 64.0;
                    points.push([
                        (center.0 + radius * theta.cos()) as f32,
                        (center.1 + radius * theta.sin()) as f32,
                    ]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: 65,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingBodyGeometry::Triangle { corners } => {
                if drawing.fill_enabled {
                    let fill = drawing
                        .fill_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51));
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
                let first_point = points.len() as u32;
                for step in 0..=64 {
                    let (x, y) = arc.point(step as f64 / 64.0);
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: 65,
                    width: (drawing.width * vpr) as f32,
                    style: drawing.style,
                    line_type: LineType::Simple,
                    color,
                });
            }
            DrawingBodyGeometry::Curve(curve) => {
                let first_point = points.len() as u32;
                for step in 0..=64 {
                    let (x, y) = curve.point(step as f64 / 64.0);
                    points.push([x as f32, y as f32]);
                }
                out.push(Prim::Polyline {
                    first_point,
                    point_count: 65,
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
                let fill = drawing
                    .fill_color
                    .as_deref()
                    .and_then(Color::parse_css)
                    .unwrap_or(Color::rgba(color.r(), color.g(), color.b(), 51));
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
                let reward = Color::parse_css(aeris_charts_core::style::MARKET_UP_CSS)
                    .unwrap_or(Color::rgb(8, 153, 129));
                let risk = Color::parse_css(aeris_charts_core::style::MARKET_DOWN_CSS)
                    .unwrap_or(Color::rgb(247, 82, 95));
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
                        drawing.width * vpr,
                        color,
                        out,
                    );
                    push_drawing_cap(
                        drawing.stroke_end,
                        *last,
                        line_points[line_points.len() - 2],
                        drawing.width * vpr,
                        color,
                        out,
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
                    let label_color = drawing
                        .text_color
                        .as_deref()
                        .and_then(Color::parse_css)
                        .unwrap_or(color);
                    let size =
                        drawing.resolved_text_size(self.options.get().layout.font_size) * vpr;
                    for (&(x, y), label) in line_points.iter().zip(labels.iter()) {
                        out.push(Prim::Text {
                            x: x as f32,
                            y: (y - 8.0 * vpr) as f32,
                            text: if drawing.kind.is_elliott() {
                                format!("{label} ({})", drawing.wave_degree)
                            } else {
                                (*label).to_string()
                            },
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

    #[allow(clippy::too_many_arguments)]
    fn build_profile_drawing_prims(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
    ) {
        let Ok(snapshot) = self.profile_drawing_snapshot_for_frame(drawing.id) else {
            return;
        };
        match snapshot {
            crate::ProfileDrawingSnapshot::Volume(profile) => {
                let Some(options) = drawing.profile.as_ref() else {
                    return;
                };
                let left = px
                    .first()
                    .map_or(0.0, |point| point.0)
                    .clamp(0.0, f64::from(pane_w_px));
                let right = match drawing.kind {
                    DrawingKind::FixedRangeVolumeProfile => px
                        .get(1)
                        .map_or(left, |point| point.0)
                        .clamp(0.0, f64::from(pane_w_px)),
                    _ => f64::from(pane_w_px),
                };
                let range_left = left.min(right);
                let range_right = left.max(right);
                let available = (range_right - range_left).max(1.0) * options.width_percent / 100.0;
                let max_volume = profile
                    .rows
                    .iter()
                    .map(|row| row.total_volume)
                    .fold(0.0_f64, f64::max);
                if max_volume <= 0.0 {
                    return;
                }
                let bid = Color::rgba(247, 82, 95, 150);
                let ask = Color::rgba(8, 153, 129, 150);
                let unknown = Color::rgba(120, 130, 145, 130);
                for row in &profile.rows {
                    let Some((_, y0)) = self.drawing_to_px_for(
                        drawing.pane_index,
                        drawing.price_scale,
                        crate::DrawingPoint {
                            logical: drawing.points[0].logical,
                            price: row.low,
                        },
                    ) else {
                        continue;
                    };
                    let Some((_, y1)) = self.drawing_to_px_for(
                        drawing.pane_index,
                        drawing.price_scale,
                        crate::DrawingPoint {
                            logical: drawing.points[0].logical,
                            price: row.high,
                        },
                    ) else {
                        continue;
                    };
                    let width = available * row.total_volume / max_volume;
                    let x0 = range_right - width;
                    let height = ((y0 - y1).abs() * vpr).round().max(1.0) as i32;
                    let y = (y0.min(y1) * vpr).round() as i32;
                    let mut cursor = x0;
                    for (volume, color) in [
                        (row.bid_volume, bid),
                        (row.unknown_volume, unknown),
                        (row.ask_volume, ask),
                    ] {
                        if volume <= 0.0 {
                            continue;
                        }
                        let segment = width * volume / row.total_volume;
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: (cursor * vpr).round() as i32,
                                y,
                                w: (segment * vpr).round().max(1.0) as i32,
                                h: height,
                            },
                            color,
                        });
                        cursor += segment;
                    }
                }
                if let Some(poc) = profile
                    .poc
                    .and_then(|price| {
                        self.drawing_to_px_for(
                            drawing.pane_index,
                            drawing.price_scale,
                            crate::DrawingPoint {
                                logical: drawing.points[0].logical,
                                price,
                            },
                        )
                    })
                    .map(|(_, y)| y * vpr)
                {
                    out.push(Prim::HLine {
                        y: poc.round() as i32,
                        x0: ((range_right - available) * vpr).round() as i32,
                        x1: (range_right * vpr).round() as i32,
                        width: vpr.round().max(1.0) as i32,
                        style: LineStyle::Solid,
                        color: Color::rgb(245, 166, 35),
                    });
                }
            }
            crate::ProfileDrawingSnapshot::Vwap(values) => {
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
                            target.push([x as f32 * vpr as f32, y as f32 * vpr as f32]);
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
    /// Label ink: explicit text color, then a trend label's line color, then chart foreground.
    fn drawing_text_color(&self, drawing: &Drawing) -> Color {
        drawing
            .text_color
            .as_deref()
            .and_then(Color::parse_css)
            .or_else(|| {
                (drawing.kind == DrawingKind::TrendLine)
                    .then(|| Color::parse_css(&drawing.color))
                    .flatten()
            })
            .or_else(|| Color::parse_css(&self.options.get().layout.text_color))
            .unwrap_or_else(|| {
                let fallback = aeris_charts_core::style::DEFAULT_FOREGROUND_RGB;
                Color::rgb(fallback.0, fallback.1, fallback.2)
            })
    }

    /// The engine-painted caret of an open drawing text session (native hosts). It sits on the
    /// label's own transform: the run's measured advance places it, and it rotates with a trend
    /// label. An empty run uses the one-em editing slot the middle-line cutout reserves.
    fn build_drawing_text_caret(
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
        let (size, x, y, align, angle) = self.text_run_geometry(drawing, px, pane_w_px, vpr);
        let text = drawing.display_text();
        let advance = if text.is_empty() {
            size
        } else {
            self.measure_drawing_frame_text(drawing, text, size)
        };
        let prefix: String = text.chars().take(session.caret).collect();
        let start = match align {
            DrawingTextHAlign::Left => 0.0,
            DrawingTextHAlign::Center => -advance / 2.0,
            DrawingTextHAlign::Right => -advance,
        };
        // Browser caret parity: a 1 CSS px bar at the next whole CSS px after the prefix run,
        // spanning the label's 1.2em line box, rotated with the label.
        let prefix_css = self.measure_drawing_frame_text(drawing, &prefix, size) / vpr;
        let local_x = start + prefix_css.ceil() * vpr;
        let half_height = size * 0.6;
        let color = self.drawing_text_color(drawing);
        if angle.abs() <= 1e-6 {
            // An unrotated caret snaps to the device grid like every other crisp 1px rule, so
            // it keeps one thickness wherever the caret moves.
            let width = vpr.round().max(1.0) as i32;
            out.push(Prim::Rect {
                rect: IRect {
                    x: (x + local_x).round() as i32,
                    y: (y - half_height).round() as i32,
                    w: width,
                    h: ((y + half_height).round() - (y - half_height).round()).max(1.0) as i32,
                },
                color,
            });
            return;
        }
        let (sin, cos) = angle.sin_cos();
        let at = |local_y: f64| {
            [
                (x + cos * local_x - sin * local_y) as f32,
                (y + sin * local_x + cos * local_y) as f32,
            ]
        };
        let first_point = points.len() as u32;
        points.extend([at(-half_height), at(half_height)]);
        out.push(Prim::Polyline {
            first_point,
            point_count: 2,
            width: vpr.max(1.0) as f32,
            style: LineStyle::Solid,
            line_type: LineType::Simple,
            color,
        });
    }

    fn text_run_geometry(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
    ) -> (f64, f64, f64, DrawingTextHAlign, f64) {
        let layout = &self.options.get().layout;
        let size = drawing.resolved_text_size(layout.font_size) * vpr;
        let pane = &self.panes[drawing.pane_index];
        let (x, y, align, angle) = ChartEngine::drawing_text_placement(
            drawing,
            px,
            f64::from(pane_w_px),
            pane.top * vpr,
            pane.height * vpr,
            size,
            TEXT_PAD * vpr,
        );
        (size, x, y, align, angle)
    }

    /// The text tool's interaction chrome (hover ring, focus border): a crisp integer-snapped
    /// hollow frame on the SAME box the host's editing wrap draws — the label run (advance ×
    /// 1.2·size, the hit test's line-height convention) padded by the editing chrome's
    /// 2 px border + 4 px padding (drawings.rs `TEXT_CHROME_PAD`). Selection, hover, and
    /// typing mode land on one outline, so entering/leaving the editor moves nothing.
    fn push_text_chrome(
        &self,
        drawing: &Drawing,
        px: &[(f64, f64)],
        pane_w_px: i32,
        vpr: f64,
        color: Color,
        out: &mut Vec<Prim>,
    ) {
        let (size, x, y, align, _) = self.text_run_geometry(drawing, px, pane_w_px, vpr);
        let width = self.measure_drawing_text(drawing, size);
        let height = size * 1.2;
        let pad = TEXT_CHROME_PAD * vpr;
        let left = match align {
            DrawingTextHAlign::Left => x,
            DrawingTextHAlign::Center => x - width / 2.0,
            DrawingTextHAlign::Right => x - width,
        };
        let rect = IRect {
            x: (left - pad).round() as i32,
            y: (y - height / 2.0 - pad).round() as i32,
            w: (width + 2.0 * pad).round().max(1.0) as i32,
            h: (height + 2.0 * pad).round().max(1.0) as i32,
        };
        out.push(Prim::RectFrame {
            rect,
            border: (2.0 * vpr).round().max(1.0) as i32,
            color,
        });
    }

    /// One drawing's text label (every tool can carry one): the placement resolves the 3×3
    /// alignment against the tool's reference box, except trend lines, whose slots follow the
    /// actual segment and whose middle slot opens a measured stroke gap. Trend labels emit
    /// `Prim::RotatedText`; other drawing text emits `Prim::Text`. In either contract x is the
    /// aligned edge and y is the vertical center (the IR's middle-baseline convention). Empty
    /// standalone text paints nothing; an empty hovered trend label paints its dedicated
    /// prompt at the canonical label transform. A text tool
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
        let Some((text, placeholder)) = self.drawing_frame_text(drawing) else {
            return;
        };
        let is_text_tool = drawing.kind == DrawingKind::Text || drawing.kind.is_text_annotation();
        let (size, x, y, align, angle) = self.text_run_geometry(drawing, px, pane_w_px, vpr);
        let layout = &self.options.get().layout;
        let mut color = self.drawing_text_color(drawing);
        if placeholder {
            color = Color::rgba(
                color.r(),
                color.g(),
                color.b(),
                color.a().min(TREND_TEXT_PLACEHOLDER_ALPHA),
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
            let width = self.measure_drawing_text(drawing, size);
            let height = size * 1.2;
            let pad = 4.0 * vpr;
            let left = match align {
                DrawingTextHAlign::Left => x,
                DrawingTextHAlign::Center => x - width / 2.0,
                DrawingTextHAlign::Right => x - width,
            };
            let rect = IRect {
                x: (left - pad).round() as i32,
                y: (y - height / 2.0 - pad).round() as i32,
                w: (width + 2.0 * pad).round().max(1.0) as i32,
                h: (height + 2.0 * pad).round().max(1.0) as i32,
            };
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

        let text_prim = Prim::RotatedText {
            x: x as f32,
            y: y as f32,
            text: text.to_string(),
            color,
            size: size as f32,
            family: layout.font_family.clone(),
            align: match align {
                DrawingTextHAlign::Left => TextAlign::Left,
                DrawingTextHAlign::Center => TextAlign::Center,
                DrawingTextHAlign::Right => TextAlign::Right,
            },
            weight: drawing.text_weight.unwrap_or(400),
            italic: drawing.text_italic,
            angle: angle as f32,
        };
        if drawing.kind == DrawingKind::TrendLine {
            out.push(text_prim);
        } else if let Prim::RotatedText {
            x,
            y,
            text,
            color,
            size,
            family,
            align,
            weight,
            italic,
            ..
        } = text_prim
        {
            out.push(Prim::Text {
                x,
                y,
                text,
                color,
                size,
                family,
                align,
                weight,
                italic,
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
        if drawing.labels.is_empty() {
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
            Color::parse_css(aeris_charts_core::style::MARKET_DOWN_CSS)
                .unwrap_or(Color::rgb(247, 82, 95))
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
            let fill = drawing
                .fill_color
                .as_deref()
                .and_then(Color::parse_css)
                .unwrap_or(Color::rgba(
                    color.r(),
                    color.g(),
                    color.b(),
                    MEASURE_FILL_ALPHA,
                ));
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
        self.push_stat_label_block(out, (center_x, center_y), &lines, color, vpr);
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

    fn build_position_labels(
        &self,
        drawing: &Drawing,
        position: PositionGeometry,
        vpr: f64,
        reward: Color,
        risk: Color,
        out: &mut Vec<Prim>,
    ) {
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
        self.push_stat_label_block(out, (center_x, target_label_y), &[target_text], reward, vpr);
        self.push_stat_label_block(out, (center_x, stop_label_y), &[stop_text], risk, vpr);
        let pnl_color = if pnl.is_some_and(|value| value < 0.0) {
            risk
        } else {
            reward
        };
        self.push_stat_label_block(out, (center_x, position.entry_y), &middle, pnl_color, vpr);
    }

    /// Dynamic position progress belongs to pane chrome rather than retained drawing geometry:
    /// series updates already invalidate chrome, so the darker traversed fill and terminal-candle
    /// trend can follow data without rebuilding every drawing on each tick.
    pub(super) fn build_position_progress_frame(
        &self,
        pane_index: usize,
        out: &mut Vec<Prim>,
        points: &mut Vec<[f32; 2]>,
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
            let semantic = match run.side {
                PositionRunSide::Reward => {
                    Color::parse_css(aeris_charts_core::style::MARKET_UP_CSS)
                        .unwrap_or(Color::rgb(8, 153, 129))
                }
                PositionRunSide::Risk => {
                    Color::parse_css(aeris_charts_core::style::MARKET_DOWN_CSS)
                        .unwrap_or(Color::rgb(247, 82, 95))
                }
            };

            // Stronger opacity represents only the price/time space actually travelled since the
            // fill: first-fill x -> current/terminal x, entry y -> current/terminal y. It never
            // darkens the untouched remainder of either TP/SL zone.
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
                    vpr.max(1.0) as f32,
                    LineStyle::Dashed,
                    LineType::Simple,
                    POSITION_ENTRY,
                );
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
        let reward = Color::parse_css(aeris_charts_core::style::MARKET_UP_CSS)
            .unwrap_or(Color::rgb(8, 153, 129));
        let risk = Color::parse_css(aeris_charts_core::style::MARKET_DOWN_CSS)
            .unwrap_or(Color::rgb(247, 82, 95));
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
            self.build_position_labels(drawing, position, vpr, reward, risk, out);
        }
    }

    fn push_stat_label_block(
        &self,
        out: &mut Vec<Prim>,
        center: (f64, f64),
        lines: &[String],
        background: Color,
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
        let text_color = background.contrast_text();
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
        let fallback = aeris_charts_core::style::DEFAULT_SURFACE_RGB;
        let background = Color::parse_css(&self.options.get().layout.background.color)
            .unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2));
        if background.luminance() > 160.0 {
            Color::rgb(0xff, 0xff, 0xff)
        } else {
            Color::rgb(0, 0, 0)
        }
    }
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

/// Default measured-area wash: the tool color at 20% alpha, like the rectangle tool.
const MEASURE_FILL_ALPHA: u8 = 51;
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

/// One handle per anchor: the border disc underneath, the fill disc on top.
fn build_anchor_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    for &(cx, cy) in px {
        out.push(Prim::Circle {
            cx: cx as f32,
            cy: cy as f32,
            radius: ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32,
            fill: ANCHOR_BORDER,
            stroke_width: 0.0,
            stroke: ANCHOR_BORDER,
        });
        out.push(Prim::Circle {
            cx: cx as f32,
            cy: cy as f32,
            radius: (ANCHOR_RADIUS * vpr) as f32,
            fill,
            stroke_width: 0.0,
            stroke: fill,
        });
    }
}

/// Long/Short Position selection controls. The controls correspond to target, entry/pivot,
/// horizontal extent, and stop; they are intentionally not generic drawing-point anchors.
fn build_position_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    if px.len() != 3 {
        return;
    }
    let entry = px[0];
    let target = px[1];
    let stop = px[2];
    let controls = [
        (entry.0, target.1, false),
        (entry.0, entry.1, true),
        (target.0, entry.1, false),
        (entry.0, stop.1, false),
    ];
    let outer = ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32;
    let inner = (ANCHOR_RADIUS * vpr) as f32;
    for (cx, cy, circular) in controls {
        if circular {
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: outer,
                fill: ANCHOR_BORDER,
                stroke_width: 0.0,
                stroke: ANCHOR_BORDER,
            });
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: inner,
                fill,
                stroke_width: 0.0,
                stroke: fill,
            });
        } else {
            let side = (2.0 * (ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr)
                .round()
                .max(1.0);
            // One shape owns both fill and inside border; independent rect rounding made
            // opposite sides acquire different thicknesses at fractional coordinates/DPR.
            out.push(Prim::RoundRect {
                x: (cx - side / 2.0).round() as f32,
                y: (cy - side / 2.0).round() as f32,
                w: side as f32,
                h: side as f32,
                radii: [(2.0 * vpr).round().max(1.0) as f32; 4],
                fill,
                border_width: (ANCHOR_BORDER_WIDTH * vpr).floor().max(1.0) as f32,
                border_color: ANCHOR_BORDER,
            });
        }
    }
}

/// The rectangle's eight reference-informed handles: fully-rounded discs on the four corners and
/// slightly-rounded square handles on the four edge midpoints (the midpoint drags resize one
/// edge independently).
fn build_rectangle_handles(px: &[(f64, f64)], vpr: f64, fill: Color, out: &mut Vec<Prim>) {
    let anchors = ChartEngine::rectangle_anchors(px);
    for (index, &(cx, cy)) in anchors.iter().enumerate() {
        if index % 2 == 0 {
            // Corners: the standard disc handles.
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32,
                fill: ANCHOR_BORDER,
                stroke_width: 0.0,
                stroke: ANCHOR_BORDER,
            });
            out.push(Prim::Circle {
                cx: cx as f32,
                cy: cy as f32,
                radius: (ANCHOR_RADIUS * vpr) as f32,
                fill,
                stroke_width: 0.0,
                stroke: fill,
            });
        } else {
            // Edge midpoints: slightly-rounded squares (2px corner radius), border square
            // underneath, fill square on top.
            let outer = ((ANCHOR_RADIUS + ANCHOR_BORDER_WIDTH) * vpr) as f32;
            let inner = (ANCHOR_RADIUS * vpr) as f32;
            let radii = [2.0 * vpr as f32; 4];
            out.push(Prim::RoundRect {
                x: cx as f32 - outer,
                y: cy as f32 - outer,
                w: outer * 2.0,
                h: outer * 2.0,
                radii,
                fill: ANCHOR_BORDER,
                border_width: 0.0,
                border_color: ANCHOR_BORDER,
            });
            out.push(Prim::RoundRect {
                x: cx as f32 - inner,
                y: cy as f32 - inner,
                w: inner * 2.0,
                h: inner * 2.0,
                radii,
                fill,
                border_width: 0.0,
                border_color: fill,
            });
        }
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
